use crate::jsjson::{self, array_index, ordered_entries, JsValue};
use anyhow::{bail, Result};
use base64::Engine;
use std::collections::{BTreeMap, HashSet};
use std::sync::OnceLock;

fn get_set<'a>(value: &'a JsValue, key: &str) -> Option<&'a JsValue> {
    value.get(key).filter(|v| !matches!(v, JsValue::Null))
}

fn truthy(value: &JsValue) -> bool {
    match value {
        JsValue::Null => false,
        JsValue::Bool(b) => *b,
        JsValue::Number(n) => *n != 0.0 && !n.is_nan(),
        JsValue::String(s) => !s.is_empty(),
        JsValue::Array(_) | JsValue::Object(_) => true,
    }
}

fn obj(entries: Vec<(&str, JsValue)>) -> JsValue {
    JsValue::Object(
        entries
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect(),
    )
}

/// `JSON.stringify` with full JS number formatting (`NaN`, `1e+21`, ...).
pub fn write_json(value: &JsValue, out: &mut String) {
    jsjson::write_with(value, out, &|n| Ok(js_number_string(n))).expect("infallible");
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

fn js_string(value: &JsValue) -> String {
    match value {
        JsValue::Null => "null".into(),
        JsValue::Bool(b) => b.to_string(),
        JsValue::Number(n) => js_number_string(*n),
        JsValue::String(s) => s.clone(),
        JsValue::Array(items) => items
            .iter()
            .map(|item| match item {
                JsValue::Null => String::new(),
                other => js_string(other),
            })
            .collect::<Vec<_>>()
            .join(","),
        JsValue::Object(_) => "[object Object]".into(),
    }
}

fn js_number(value: &JsValue) -> f64 {
    match value {
        JsValue::Null => 0.0,
        JsValue::Bool(b) => f64::from(u8::from(*b)),
        JsValue::Number(n) => *n,
        JsValue::String(s) => js_parse_number(s),
        JsValue::Array(_) => js_parse_number(&js_string(value)),
        JsValue::Object(_) => f64::NAN,
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
    for (prefix, radix) in [("0x", 16), ("0o", 8), ("0b", 2)] {
        if let Some(rest) = t
            .strip_prefix(prefix)
            .or_else(|| t.strip_prefix(prefix.to_ascii_uppercase().as_str()))
        {
            return js_parse_radix(rest, radix);
        }
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

/// Re-encode as `atob`/`btoa` would: url-safe alphabet accepted, whitespace
/// skipped, decoding stops at the first foreign character.
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

fn normalize_data_entry(value: &JsValue) -> Result<JsValue> {
    if matches!(value, JsValue::Null) {
        bail!("data entry is null");
    }
    if let Some(json) = get_set(value, "json") {
        return Ok(obj(vec![("json", json.clone())]));
    }
    if let Some(binary) = get_set(value, "binary") {
        let JsValue::String(b64) = binary else {
            bail!("data entry binary is not a base64 string");
        };
        return Ok(obj(vec![(
            "binary",
            JsValue::String(canonical_base64(b64)),
        )]));
    }
    Ok(obj(Vec::new()))
}

/// Entity keys are re-spelled through `Number` → `String`, so `"512"`, `"5.12e2"`
/// and `" 512 "` all land on the index 512; non-index survivors keep insertion order.
fn normalize_data(pairs: &[(String, &JsValue)]) -> Result<JsValue> {
    let mut indexed: BTreeMap<u32, JsValue> = BTreeMap::new();
    let mut plain: Vec<(String, JsValue)> = Vec::new();
    for (key, value) in ordered_entries(pairs) {
        let entry = normalize_data_entry(value)?;
        let out_key = js_number_string(js_parse_number(key));
        match array_index(&out_key) {
            Some(n) => {
                indexed.insert(n, entry);
            }
            None => jsjson::set(&mut plain, out_key, entry),
        }
    }
    let mut entries: Vec<(String, JsValue)> = indexed
        .into_iter()
        .map(|(n, e)| (n.to_string(), e))
        .collect();
    entries.extend(plain);
    Ok(JsValue::Object(entries))
}

fn normalize_component(component: &JsValue) -> Result<JsValue> {
    if matches!(component, JsValue::Null) {
        bail!("component is null");
    }
    let name = get_set(component, "name").map_or_else(String::new, js_string);
    let pairs: Vec<(String, &JsValue)> = match component.get("data") {
        Some(JsValue::Object(entries)) => entries.iter().map(|(k, v)| (k.clone(), v)).collect(),
        Some(JsValue::Array(items)) => items
            .iter()
            .enumerate()
            .map(|(i, v)| (i.to_string(), v))
            .collect(),
        _ => Vec::new(),
    };
    let mut entries = vec![("name", JsValue::String(name))];
    if let Some(schema) = get_set(component, "jsonSchema") {
        entries.push(("jsonSchema", schema.clone()));
    }
    entries.push(("data", normalize_data(&pairs)?));
    Ok(obj(entries))
}

fn normalize_definition(root: &JsValue) -> Result<JsValue> {
    if matches!(root, JsValue::Null) {
        bail!("composite root is null");
    }
    let version = js_math_round(get_set(root, "version").map_or(0.0, js_number));
    let version_value = if version.is_finite() {
        JsValue::Number(version)
    } else {
        JsValue::Null
    };
    let components = match root.get("components") {
        Some(JsValue::Array(items)) => items
            .iter()
            .map(normalize_component)
            .collect::<Result<Vec<_>>>()?,
        _ => Vec::new(),
    };
    Ok(obj(vec![
        ("version", version_value),
        ("components", JsValue::Array(components)),
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

/// Normalizes composites the way `@dcl/ecs` `Composite.toJson` does, tracking
/// which component names earlier composites have already defined.
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
        let normalized = normalize_definition(&jsjson::parse(raw)?)?;
        self.check_instanceable(&normalized)?;
        let mut out = String::new();
        write_json(&normalized, &mut out);
        Ok(out)
    }

    fn check_instanceable(&mut self, normalized: &JsValue) -> Result<()> {
        let Some(JsValue::Array(components)) = normalized.get("components") else {
            return Ok(());
        };
        for component in components {
            let Some(JsValue::String(name)) = component.get("name") else {
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
            if component.get("jsonSchema").is_some_and(truthy) {
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

    #[test]
    fn schema_table_tracks_ecs_7_27_0() {
        let raw = include_str!("../docs/composite-component-schemas.json");
        let parsed: serde_json::Value = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed["ecsVersion"], serde_json::json!("7.27.0"));
        let table = static_core_table();
        for name in [
            "core::ExplorerUiEventsResult",
            "core::TouchScreenControls",
            "core::UiInputBinding",
            "core::AvatarEmoteCommand",
        ] {
            assert!(table.contains(name), "missing {name}");
        }
    }

    fn edge_cases() -> Vec<(String, String)> {
        let raw = include_str!("../docs/composite-tojson-edge-cases.json");
        let JsValue::Array(cases) = jsjson::parse(raw).expect("edge cases parse") else {
            panic!("edge cases must be an array");
        };
        cases
            .iter()
            .map(|case| {
                let input = case.get("input").expect("input");
                let Some(JsValue::String(expected)) = case.get("output") else {
                    panic!("output must be a string");
                };
                let mut input_text = String::new();
                write_json(input, &mut input_text);
                (input_text, expected.clone())
            })
            .collect()
    }

    fn normalize_text(input: &str) -> String {
        let parsed = jsjson::parse(input).expect("parse");
        let normalized = normalize_definition(&parsed).expect("normalize");
        let mut out = String::new();
        write_json(&normalized, &mut out);
        out
    }

    #[test]
    fn edge_cases_match_upstream() {
        let cases = edge_cases();
        assert!(cases.len() >= 20);
        for (input, expected) in cases {
            assert_eq!(normalize_text(&input), expected, "input: {input}");
        }
    }

    #[test]
    fn normalization_is_idempotent() {
        for (_, expected) in edge_cases() {
            assert_eq!(normalize_text(&expected), expected);
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
