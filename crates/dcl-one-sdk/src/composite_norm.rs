use anyhow::{bail, Result};
use base64::Engine;
use serde::de::{Error as DeError, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::sync::OnceLock;

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    fn get_set(&self, key: &str) -> Option<&Json> {
        self.get(key).filter(|v| !matches!(v, Json::Null))
    }

    fn is_truthy(&self) -> bool {
        match self {
            Json::Null => false,
            Json::Bool(b) => *b,
            Json::Num(n) => *n != 0.0 && !n.is_nan(),
            Json::Str(s) => !s.is_empty(),
            Json::Arr(_) | Json::Obj(_) => true,
        }
    }
}

impl<'de> Deserialize<'de> for Json {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct JsonVisitor;

        impl<'de> Visitor<'de> for JsonVisitor {
            type Value = Json;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("any JSON value")
            }

            fn visit_bool<E: DeError>(self, v: bool) -> Result<Json, E> {
                Ok(Json::Bool(v))
            }

            fn visit_i64<E: DeError>(self, v: i64) -> Result<Json, E> {
                Ok(Json::Num(v as f64))
            }

            fn visit_u64<E: DeError>(self, v: u64) -> Result<Json, E> {
                Ok(Json::Num(v as f64))
            }

            fn visit_f64<E: DeError>(self, v: f64) -> Result<Json, E> {
                Ok(Json::Num(v))
            }

            fn visit_str<E: DeError>(self, v: &str) -> Result<Json, E> {
                Ok(Json::Str(v.to_owned()))
            }

            fn visit_string<E: DeError>(self, v: String) -> Result<Json, E> {
                Ok(Json::Str(v))
            }

            fn visit_unit<E: DeError>(self) -> Result<Json, E> {
                Ok(Json::Null)
            }

            fn visit_none<E: DeError>(self) -> Result<Json, E> {
                Ok(Json::Null)
            }

            fn visit_some<D2: Deserializer<'de>>(self, d: D2) -> Result<Json, D2::Error> {
                Json::deserialize(d)
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Json, A::Error> {
                let mut items = Vec::new();
                while let Some(v) = seq.next_element()? {
                    items.push(v);
                }
                Ok(Json::Arr(items))
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Json, A::Error> {
                let mut entries: Vec<(String, Json)> = Vec::new();
                while let Some((k, v)) = map.next_entry::<String, Json>()? {
                    match entries.iter_mut().find(|(ek, _)| *ek == k) {
                        Some(slot) => slot.1 = v,
                        None => entries.push((k, v)),
                    }
                }
                if let [(k, Json::Str(s))] = entries.as_slice() {
                    if k == "$serde_json::private::Number" {
                        if let Ok(n) = s.parse::<f64>() {
                            return Ok(Json::Num(n));
                        }
                    }
                }
                Ok(Json::Obj(entries))
            }
        }

        deserializer.deserialize_any(JsonVisitor)
    }
}

fn ordered_entries(entries: &[(String, Json)]) -> Vec<&(String, Json)> {
    let mut indexed: Vec<(u64, &(String, Json))> = Vec::new();
    let mut rest: Vec<&(String, Json)> = Vec::new();
    for entry in entries {
        match array_index(&entry.0) {
            Some(n) => indexed.push((n, entry)),
            None => rest.push(entry),
        }
    }
    indexed.sort_by_key(|(n, _)| *n);
    indexed.into_iter().map(|(_, e)| e).chain(rest).collect()
}

pub fn write_json(value: &Json, out: &mut String) {
    match value {
        Json::Null => out.push_str("null"),
        Json::Bool(true) => out.push_str("true"),
        Json::Bool(false) => out.push_str("false"),
        Json::Num(n) => out.push_str(&js_number_string(*n)),
        Json::Str(s) => write_json_string(s, out),
        Json::Arr(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_json(item, out);
            }
            out.push(']');
        }
        Json::Obj(entries) => {
            out.push('{');
            for (i, entry) in ordered_entries(entries).into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_json_string(&entry.0, out);
                out.push(':');
                write_json(&entry.1, out);
            }
            out.push('}');
        }
    }
}

fn write_json_string(s: &str, out: &mut String) {
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
                use fmt::Write;
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn js_number_string(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if x == 0.0 {
        return "0".into();
    }
    let mag = x.abs();
    if (1e-6..1e21).contains(&mag) {
        return format!("{x}");
    }
    let exp = format!("{x:e}");
    match exp.find('e') {
        Some(p) if !exp[p + 1..].starts_with('-') => format!("{}e+{}", &exp[..p], &exp[p + 1..]),
        _ => exp,
    }
}

fn js_string(value: &Json) -> String {
    match value {
        Json::Null => "null".into(),
        Json::Bool(b) => b.to_string(),
        Json::Num(n) => js_number_string(*n),
        Json::Str(s) => s.clone(),
        Json::Arr(items) => items
            .iter()
            .map(|item| match item {
                Json::Null => String::new(),
                other => js_string(other),
            })
            .collect::<Vec<_>>()
            .join(","),
        Json::Obj(_) => "[object Object]".into(),
    }
}

fn js_number(value: &Json) -> f64 {
    match value {
        Json::Null => 0.0,
        Json::Bool(b) => f64::from(u8::from(*b)),
        Json::Num(n) => *n,
        Json::Str(s) => js_parse_number(s),
        Json::Arr(_) => js_parse_number(&js_string(value)),
        Json::Obj(_) => f64::NAN,
    }
}

fn js_parse_number(s: &str) -> f64 {
    let t = s.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    if t.is_empty() {
        return 0.0;
    }
    match t {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    if let Some(rest) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return js_parse_radix(rest, 16);
    }
    if let Some(rest) = t.strip_prefix("0o").or_else(|| t.strip_prefix("0O")) {
        return js_parse_radix(rest, 8);
    }
    if let Some(rest) = t.strip_prefix("0b").or_else(|| t.strip_prefix("0B")) {
        return js_parse_radix(rest, 2);
    }
    if is_decimal_literal(t) {
        t.parse::<f64>().unwrap_or(f64::NAN)
    } else {
        f64::NAN
    }
}

fn js_parse_radix(digits: &str, radix: u32) -> f64 {
    if digits.is_empty() {
        return f64::NAN;
    }
    let mut acc = 0.0_f64;
    for c in digits.chars() {
        match c.to_digit(radix) {
            Some(d) => acc = acc * f64::from(radix) + f64::from(d),
            None => return f64::NAN,
        }
    }
    acc
}

fn is_decimal_literal(t: &str) -> bool {
    let b = t.as_bytes();
    let mut i = 0;
    if matches!(b.first(), Some(b'+') | Some(b'-')) {
        i = 1;
    }
    let int_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let int_len = i - int_start;
    let mut frac_len = 0;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        let frac_start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        frac_len = i - frac_start;
    }
    if int_len == 0 && frac_len == 0 {
        return false;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let exp_start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == exp_start {
            return false;
        }
    }
    i == b.len()
}

fn js_math_round(x: f64) -> f64 {
    if !x.is_finite() {
        return x;
    }
    let f = x.floor();
    if x - f >= 0.5 {
        f + 1.0
    } else {
        f
    }
}

fn canonical_base64(input: &str) -> String {
    let mut sextets: Vec<u8> = Vec::with_capacity(input.len());
    for c in input.chars() {
        let v = match c {
            'A'..='Z' => c as u8 - b'A',
            'a'..='z' => c as u8 - b'a' + 26,
            '0'..='9' => c as u8 - b'0' + 52,
            '+' | '-' => 62,
            '/' | '_' => 63,
            c if c.is_ascii_whitespace() => continue,
            _ => break,
        };
        sextets.push(v);
    }
    let mut bytes = Vec::with_capacity(sextets.len() * 3 / 4 + 2);
    for chunk in sextets.chunks(4) {
        if chunk.len() >= 2 {
            bytes.push((chunk[0] << 2) | (chunk[1] >> 4));
        }
        if chunk.len() >= 3 {
            bytes.push((chunk[1] << 4) | (chunk[2] >> 2));
        }
        if chunk.len() == 4 {
            bytes.push((chunk[2] << 6) | chunk[3]);
        }
    }
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn array_index(key: &str) -> Option<u64> {
    if key == "0" {
        return Some(0);
    }
    let bytes = key.as_bytes();
    if bytes.is_empty() || bytes.len() > 10 || bytes[0] == b'0' {
        return None;
    }
    if !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let n: u64 = key.parse().ok()?;
    (n <= 4_294_967_294).then_some(n)
}

fn normalize_data_entry(value: &Json) -> Result<Json> {
    if matches!(value, Json::Null) {
        bail!("data entry is null");
    }
    if let Some(json) = value.get_set("json") {
        return Ok(Json::Obj(vec![("json".to_owned(), json.clone())]));
    }
    if let Some(binary) = value.get_set("binary") {
        let Json::Str(b64) = binary else {
            bail!("data entry binary is not a base64 string");
        };
        return Ok(Json::Obj(vec![(
            "binary".to_owned(),
            Json::Str(canonical_base64(b64)),
        )]));
    }
    Ok(Json::Obj(Vec::new()))
}

fn normalize_data(pairs: &[(String, &Json)]) -> Result<Json> {
    let mut indexed: Vec<(u64, usize)> = Vec::new();
    let mut plain: Vec<usize> = Vec::new();
    for (pos, (key, _)) in pairs.iter().enumerate() {
        match array_index(key) {
            Some(n) => indexed.push((n, pos)),
            None => plain.push(pos),
        }
    }
    indexed.sort_by_key(|(n, _)| *n);
    let mut out_indexed: BTreeMap<u64, Json> = BTreeMap::new();
    let mut out_plain: Vec<(String, Json)> = Vec::new();
    for pos in indexed.into_iter().map(|(_, p)| p).chain(plain) {
        let (key, value) = &pairs[pos];
        let entry = normalize_data_entry(value)?;
        let out_key = js_number_string(js_parse_number(key));
        match array_index(&out_key) {
            Some(n) => {
                out_indexed.insert(n, entry);
            }
            None => match out_plain.iter_mut().find(|(k, _)| *k == out_key) {
                Some(slot) => slot.1 = entry,
                None => out_plain.push((out_key, entry)),
            },
        }
    }
    let mut entries: Vec<(String, Json)> = out_indexed
        .into_iter()
        .map(|(n, e)| (n.to_string(), e))
        .collect();
    entries.extend(out_plain);
    Ok(Json::Obj(entries))
}

fn normalize_component(component: &Json) -> Result<Json> {
    if matches!(component, Json::Null) {
        bail!("component is null");
    }
    let name = component
        .get_set("name")
        .map_or_else(String::new, js_string);
    let json_schema = component.get_set("jsonSchema").cloned();
    let data = match component.get("data") {
        Some(Json::Obj(obj_entries)) => {
            let pairs: Vec<(String, &Json)> =
                obj_entries.iter().map(|(k, v)| (k.clone(), v)).collect();
            normalize_data(&pairs)?
        }
        Some(Json::Arr(items)) => {
            let pairs: Vec<(String, &Json)> = items
                .iter()
                .enumerate()
                .map(|(i, v)| (i.to_string(), v))
                .collect();
            normalize_data(&pairs)?
        }
        _ => Json::Obj(Vec::new()),
    };
    let mut entries = vec![("name".to_owned(), Json::Str(name))];
    if let Some(schema) = json_schema {
        entries.push(("jsonSchema".to_owned(), schema));
    }
    entries.push(("data".to_owned(), data));
    Ok(Json::Obj(entries))
}

fn normalize_definition(root: &Json) -> Result<Json> {
    if matches!(root, Json::Null) {
        bail!("composite root is null");
    }
    let version = js_math_round(root.get_set("version").map_or(0.0, js_number));
    let version_value = if version.is_finite() {
        Json::Num(version)
    } else {
        Json::Null
    };
    let components = match root.get("components") {
        Some(Json::Arr(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(normalize_component(item)?);
            }
            out
        }
        _ => Vec::new(),
    };
    Ok(Json::Obj(vec![
        ("version".to_owned(), version_value),
        ("components".to_owned(), Json::Arr(components)),
    ]))
}

fn static_core_table() -> &'static HashSet<String> {
    static TABLE: OnceLock<HashSet<String>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let raw = include_str!("../docs/composite-component-schemas.json");
        let parsed: serde_json::Value = serde_json::from_str(raw).unwrap_or_default();
        parsed["components"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter(|c| c["inStaticTable"].as_bool() == Some(true))
                    .filter_map(|c| c["name"].as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    })
}

pub struct CompositeNormalizer {
    defined: HashSet<String>,
}

impl Default for CompositeNormalizer {
    fn default() -> Self {
        Self::new()
    }
}

impl CompositeNormalizer {
    pub fn new() -> Self {
        let defined = [
            "core::Transform",
            "core-schema::Network-Entity",
            "core-schema::Network-Parent",
            "composite::root",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        Self { defined }
    }

    pub fn normalize(&mut self, raw: &str) -> Result<String> {
        let parsed: Json = serde_json::from_str(raw)?;
        let normalized = normalize_definition(&parsed)?;
        self.check_instanceable(&normalized)?;
        let mut out = String::new();
        write_json(&normalized, &mut out);
        Ok(out)
    }

    fn check_instanceable(&mut self, normalized: &Json) -> Result<()> {
        let Some(Json::Arr(components)) = normalized.get("components") else {
            return Ok(());
        };
        for component in components {
            let Some(Json::Str(name)) = component.get("name") else {
                continue;
            };
            if self.defined.contains(name) {
                continue;
            }
            if name.starts_with("core::") {
                if static_core_table().contains(name) {
                    self.defined.insert(name.clone());
                    continue;
                }
                bail!("the core component {name} was not found");
            }
            if component.get("jsonSchema").is_some_and(Json::is_truthy) {
                self.defined.insert(name.clone());
                continue;
            }
            bail!("{name} is not defined and there is no schema to define it");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge_cases() -> Vec<(String, String)> {
        let raw = include_str!("../docs/composite-tojson-edge-cases.json");
        let parsed: Json = serde_json::from_str(raw).expect("edge cases parse");
        let Json::Arr(cases) = parsed else {
            panic!("edge cases must be an array");
        };
        cases
            .iter()
            .map(|case| {
                let input = case.get("input").expect("input");
                let Some(Json::Str(expected)) = case.get("output") else {
                    panic!("output must be a string");
                };
                let mut input_text = String::new();
                write_json(input, &mut input_text);
                (input_text, expected.clone())
            })
            .collect()
    }

    #[test]
    fn edge_cases_match_upstream() {
        let cases = edge_cases();
        assert!(cases.len() >= 20);
        for (input, expected) in cases {
            let parsed: Json = serde_json::from_str(&input).expect("parse");
            let normalized = normalize_definition(&parsed).expect("normalize");
            let mut out = String::new();
            write_json(&normalized, &mut out);
            assert_eq!(out, expected, "input: {input}");
        }
    }

    #[test]
    fn normalization_is_idempotent() {
        for (_, expected) in edge_cases() {
            let parsed: Json = serde_json::from_str(&expected).expect("parse");
            let normalized = normalize_definition(&parsed).expect("normalize");
            let mut out = String::new();
            write_json(&normalized, &mut out);
            assert_eq!(out, expected);
        }
    }

    #[test]
    fn unknown_core_component_is_rejected() {
        let mut n = CompositeNormalizer::new();
        let raw = r#"{"version":1,"components":[{"name":"core::NotAThing","data":{}}]}"#;
        assert!(n.normalize(raw).is_err());
    }

    #[test]
    fn custom_component_needs_schema_until_defined() {
        let mut n = CompositeNormalizer::new();
        let no_schema = r#"{"version":1,"components":[{"name":"my::Thing","data":{}}]}"#;
        assert!(n.normalize(no_schema).is_err());
        let with_schema = r#"{"version":1,"components":[{"name":"my::Thing","jsonSchema":{"type":"object"},"data":{}}]}"#;
        assert!(n.normalize(with_schema).is_ok());
        assert!(n.normalize(no_schema).is_ok());
    }

    #[test]
    fn known_components_pass() {
        let mut n = CompositeNormalizer::new();
        let raw = r#"{"version":1,"components":[{"name":"core::MeshRenderer","data":{}},{"name":"core::Transform","data":{}},{"name":"composite::root","data":{}}]}"#;
        assert!(n.normalize(raw).is_ok());
    }

    #[test]
    fn null_entries_reject_the_composite() {
        let mut n = CompositeNormalizer::new();
        assert!(n.normalize("null").is_err());
        assert!(n.normalize(r#"{"components":[null]}"#).is_err());
        assert!(n
            .normalize(r#"{"components":[{"name":"core::Transform","data":{"512":null}}]}"#)
            .is_err());
    }

    #[test]
    fn number_formatting_matches_js() {
        assert_eq!(js_number_string(0.0), "0");
        assert_eq!(js_number_string(-0.0), "0");
        assert_eq!(js_number_string(5.0), "5");
        assert_eq!(js_number_string(0.1), "0.1");
        assert_eq!(js_number_string(1e20), "100000000000000000000");
        assert_eq!(js_number_string(1e21), "1e+21");
        assert_eq!(js_number_string(1.5e-7), "1.5e-7");
        assert_eq!(js_number_string(f64::NAN), "NaN");
    }
}
