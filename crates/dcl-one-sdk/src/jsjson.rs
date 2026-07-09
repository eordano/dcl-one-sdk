use anyhow::{bail, Result};

#[derive(Clone, Debug, PartialEq)]
pub enum JsValue {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<JsValue>),
    Object(Vec<(String, JsValue)>),
}

impl JsValue {
    pub fn get(&self, key: &str) -> Option<&JsValue> {
        match self {
            JsValue::Object(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            JsValue::String(s) => Some(s),
            _ => None,
        }
    }
}

pub fn set(obj: &mut Vec<(String, JsValue)>, key: String, value: JsValue) {
    if let Some(entry) = obj.iter_mut().find(|(k, _)| *k == key) {
        entry.1 = value;
    } else {
        obj.push((key, value));
    }
}

pub fn parse(s: &str) -> Result<JsValue> {
    let mut p = Parser {
        s,
        b: s.as_bytes(),
        i: 0,
    };
    p.ws();
    let v = p.value()?;
    p.ws();
    if p.i != p.b.len() {
        bail!("trailing characters at byte {}", p.i);
    }
    Ok(v)
}

struct Parser<'a> {
    s: &'a str,
    b: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn value(&mut self) -> Result<JsValue> {
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => Ok(JsValue::String(self.string()?)),
            Some(b't') => {
                self.lit("true")?;
                Ok(JsValue::Bool(true))
            }
            Some(b'f') => {
                self.lit("false")?;
                Ok(JsValue::Bool(false))
            }
            Some(b'n') => {
                self.lit("null")?;
                Ok(JsValue::Null)
            }
            Some(c) if c == b'-' || c.is_ascii_digit() => self.number(),
            _ => bail!("unexpected input at byte {}", self.i),
        }
    }

    fn lit(&mut self, word: &str) -> Result<()> {
        if self.b[self.i..].starts_with(word.as_bytes()) {
            self.i += word.len();
            Ok(())
        } else {
            bail!("invalid literal at byte {}", self.i)
        }
    }

    fn object(&mut self) -> Result<JsValue> {
        self.i += 1;
        let mut entries: Vec<(String, JsValue)> = Vec::new();
        self.ws();
        if self.peek() == Some(b'}') {
            self.i += 1;
            return Ok(JsValue::Object(entries));
        }
        loop {
            self.ws();
            if self.peek() != Some(b'"') {
                bail!("expected object key at byte {}", self.i);
            }
            let key = self.string()?;
            self.ws();
            if self.peek() != Some(b':') {
                bail!("expected ':' at byte {}", self.i);
            }
            self.i += 1;
            self.ws();
            let val = self.value()?;
            set(&mut entries, key, val);
            self.ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b'}') => {
                    self.i += 1;
                    return Ok(JsValue::Object(entries));
                }
                _ => bail!("expected ',' or '}}' at byte {}", self.i),
            }
        }
    }

    fn array(&mut self) -> Result<JsValue> {
        self.i += 1;
        let mut items = Vec::new();
        self.ws();
        if self.peek() == Some(b']') {
            self.i += 1;
            return Ok(JsValue::Array(items));
        }
        loop {
            self.ws();
            items.push(self.value()?);
            self.ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b']') => {
                    self.i += 1;
                    return Ok(JsValue::Array(items));
                }
                _ => bail!("expected ',' or ']' at byte {}", self.i),
            }
        }
    }

    fn hex4(&mut self) -> Result<u16> {
        if self.i + 4 > self.b.len() {
            bail!("truncated \\u escape");
        }
        let mut v: u16 = 0;
        for _ in 0..4 {
            let c = self.b[self.i];
            let d = match c {
                b'0'..=b'9' => c - b'0',
                b'a'..=b'f' => c - b'a' + 10,
                b'A'..=b'F' => c - b'A' + 10,
                _ => bail!("bad hex digit in \\u escape at byte {}", self.i),
            };
            v = v * 16 + d as u16;
            self.i += 1;
        }
        Ok(v)
    }

    fn string(&mut self) -> Result<String> {
        self.i += 1;
        let mut out = String::new();
        loop {
            let Some(c) = self.peek() else {
                bail!("unterminated string");
            };
            match c {
                b'"' => {
                    self.i += 1;
                    return Ok(out);
                }
                b'\\' => {
                    self.i += 1;
                    let Some(e) = self.peek() else {
                        bail!("unterminated escape");
                    };
                    self.i += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let u = self.hex4()?;
                            if (0xD800..0xDC00).contains(&u) {
                                if self.peek() != Some(b'\\')
                                    || self.b.get(self.i + 1) != Some(&b'u')
                                {
                                    bail!("lone surrogate in string (unsupported)");
                                }
                                self.i += 2;
                                let lo = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&lo) {
                                    bail!("lone surrogate in string (unsupported)");
                                }
                                let cp = 0x10000
                                    + (((u as u32) - 0xD800) << 10)
                                    + ((lo as u32) - 0xDC00);
                                out.push(
                                    char::from_u32(cp).expect("valid supplementary codepoint"),
                                );
                            } else if (0xDC00..0xE000).contains(&u) {
                                bail!("lone surrogate in string (unsupported)");
                            } else {
                                out.push(char::from_u32(u as u32).expect("valid BMP codepoint"));
                            }
                        }
                        _ => bail!("invalid escape at byte {}", self.i),
                    }
                }
                0x00..=0x1f => bail!("raw control character in string at byte {}", self.i),
                _ => {
                    let ch = self.s[self.i..].chars().next().expect("valid utf8");
                    out.push(ch);
                    self.i += ch.len_utf8();
                }
            }
        }
    }

    fn number(&mut self) -> Result<JsValue> {
        let start = self.i;
        if self.peek() == Some(b'-') {
            self.i += 1;
        }
        match self.peek() {
            Some(b'0') => self.i += 1,
            Some(b'1'..=b'9') => {
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.i += 1;
                }
            }
            _ => bail!("invalid number at byte {}", self.i),
        }
        if self.peek() == Some(b'.') {
            self.i += 1;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                bail!("invalid number at byte {}", self.i);
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.i += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.i += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                bail!("invalid number at byte {}", self.i);
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.i += 1;
            }
        }
        let text = &self.s[start..self.i];
        Ok(JsValue::Number(text.parse::<f64>()?))
    }
}

pub fn stringify(v: &JsValue) -> Result<String> {
    let mut out = String::new();
    write_value(v, &mut out)?;
    Ok(out)
}

fn write_value(v: &JsValue, out: &mut String) -> Result<()> {
    match v {
        JsValue::Null => out.push_str("null"),
        JsValue::Bool(true) => out.push_str("true"),
        JsValue::Bool(false) => out.push_str("false"),
        JsValue::Number(n) => out.push_str(&format_number(*n)?),
        JsValue::String(s) => write_string(s, out),
        JsValue::Array(items) => {
            out.push('[');
            for (idx, item) in items.iter().enumerate() {
                if idx > 0 {
                    out.push(',');
                }
                write_value(item, out)?;
            }
            out.push(']');
        }
        JsValue::Object(entries) => {
            out.push('{');
            let mut first = true;
            for (k, val) in ordered_entries(entries) {
                if !first {
                    out.push(',');
                }
                first = false;
                write_string(k, out);
                out.push(':');
                write_value(val, out)?;
            }
            out.push('}');
        }
    }
    Ok(())
}

fn array_index(k: &str) -> Option<u32> {
    if k.is_empty() || k.len() > 10 || !k.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if k.len() > 1 && k.starts_with('0') {
        return None;
    }
    let n: u64 = k.parse().ok()?;
    (n < u32::MAX as u64).then_some(n as u32)
}

fn ordered_entries(entries: &[(String, JsValue)]) -> Vec<(&String, &JsValue)> {
    let mut indexed: Vec<(u32, &String, &JsValue)> = Vec::new();
    let mut rest: Vec<(&String, &JsValue)> = Vec::new();
    for (k, v) in entries {
        match array_index(k) {
            Some(n) => indexed.push((n, k, v)),
            None => rest.push((k, v)),
        }
    }
    indexed.sort_by_key(|(n, _, _)| *n);
    indexed
        .into_iter()
        .map(|(_, k, v)| (k, v))
        .chain(rest)
        .collect()
}

fn format_number(v: f64) -> Result<String> {
    if !v.is_finite() {
        return Ok("null".to_string());
    }
    if v == 0.0 {
        return Ok("0".to_string());
    }
    let a = v.abs();
    if !(1e-6..1e21).contains(&a) {
        bail!(
            "number {v:e} requires ECMAScript exponent formatting (unsupported for entity parity)"
        );
    }
    Ok(format!("{v}"))
}

fn write_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                use std::fmt::Write;
                write!(out, "\\u{:04x}", c as u32).expect("write to string");
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(v: f64) -> String {
        stringify(&JsValue::Number(v)).unwrap()
    }

    fn roundtrip(input: &str) -> String {
        stringify(&parse(input).unwrap()).unwrap()
    }

    #[test]
    fn numbers_match_js_tostring() {
        assert_eq!(n(8.0), "8");
        assert_eq!(n(-0.0), "0");
        assert_eq!(n(0.1), "0.1");
        assert_eq!(n(1e20), "100000000000000000000");
        assert_eq!(n(5.5), "5.5");
        assert_eq!(n(1e-6), "0.000001");
        assert_eq!(n(123456789012345678000.0), "123456789012345680000");
        assert_eq!(n(9007199254740992.0), "9007199254740992");
        assert_eq!(n(1.75e12), "1750000000000");
        assert_eq!(n(33.333333333333336), "33.333333333333336");
        assert_eq!(n(-12.25), "-12.25");
        assert_eq!(n(16.000000000000004), "16.000000000000004");
    }

    #[test]
    fn numbers_outside_fixed_range_rejected() {
        assert!(stringify(&JsValue::Number(1e21)).is_err());
        assert!(stringify(&JsValue::Number(1e-7)).is_err());
        assert!(stringify(&JsValue::Number(-9.5e-7)).is_err());
    }

    #[test]
    fn integer_like_keys_sort_first() {
        assert_eq!(
            roundtrip(r#"{"b":1,"2":2,"1":3,"a":{"10":true,"x":[1.5,8,0.1],"02":"n"}}"#),
            r#"{"1":3,"2":2,"b":1,"a":{"10":true,"x":[1.5,8,0.1],"02":"n"}}"#
        );
        assert_eq!(
            roundtrip(r#"{"4294967295":1,"4294967294":2,"zz":3}"#),
            r#"{"4294967294":2,"4294967295":1,"zz":3}"#
        );
    }

    #[test]
    fn duplicate_keys_last_value_first_position() {
        assert_eq!(roundtrip(r#"{"a":1,"b":2,"a":3}"#), r#"{"a":3,"b":2}"#);
    }

    #[test]
    fn string_escapes_match_js() {
        let s = JsValue::String("q\u{8}w\u{c}e\u{1}z\u{1f}".to_string());
        assert_eq!(stringify(&s).unwrap(), "\"q\\bw\\fe\\u0001z\\u001f\"");
        let u = JsValue::String("é€😀/".to_string());
        assert_eq!(stringify(&u).unwrap(), "\"é€😀/\"");
    }

    #[test]
    fn parse_escapes_and_surrogates() {
        let v = parse(r#""A😀\/\n""#).unwrap();
        assert_eq!(v, JsValue::String("A😀/\n".to_string()));
        let pair = parse("\"\\uD83D\\uDE00\"").unwrap();
        assert_eq!(pair, JsValue::String("😀".to_string()));
        assert!(parse(r#""\uD83D""#).is_err());
        assert!(parse(r#""\uDE00""#).is_err());
    }

    #[test]
    fn strict_json_rejections() {
        assert!(parse("{\"a\":01}").is_err());
        assert!(parse("[1,]").is_err());
        assert!(parse("{}x").is_err());
        assert!(parse("+1").is_err());
    }
}
