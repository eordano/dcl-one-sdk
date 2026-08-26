//! The @dcl/ecs `ISchema` encoder — the second serializer a composite needs.
//!
//! Core components ride on protobuf (see crdt_gen); everything the editor and
//! the asset packs define (`core-schema::*`, `inspector::*`, `asset-packs::*`,
//! and arbitrary user component names) is serialized by @dcl/ecs's own schema
//! runtime: a tagless, positional, little-endian format driven entirely by the
//! `jsonSchema` the composite carries, which is what makes user-defined
//! components work with no static component table.
//!
//! Verified against @dcl/ecs 7.26.0 (`schemas/`, `components/component-number.js`,
//! `serialization/ByteBuffer`). JS semantics reproduced deliberately:
//! - map: properties back to back in declaration order — no count, no tags, no
//!   lengths (`Map.js`)
//! - optional: presence is JS *truthiness*, not "key present": 0, "", false and
//!   null encode as absent, while `[]` and `{}` are present (`Optional.js`)
//! - one-of: a 1-based uint8 index into the property order (`OneOf.js`)
//! - a value missing below the top level is `undefined`, and JS throws for only
//!   some of those: numbers coerce (NaN → 0 for ints, NaN for floats), booleans
//!   and optionals read as falsy, everything else throws
//! - int32/entity/enum-int go through ECMAScript ToInt32 (wrap, not saturate);
//!   int64 goes through `BigInt()`, which reads a string exactly and throws
//!   where `Number` would have produced NaN
//! - no leaf checks the type it is handed, so an ill-typed value is a silent
//!   coercion rather than an error: a number on a string leaf writes the empty
//!   string (`@protobufjs/utf8` walks `.length`), and a string on an array leaf
//!   serializes as its characters (a string is iterable)
use crate::jsjson::JsValue;
use serde_json::Value;

/// Component ids below this are the engine's own; hashed names are pushed above
/// it so they can never collide (`components/component-number.js`).
const MAX_STATIC_COMPONENT: u32 = 1 << 11;

/// `@protobufjs/utf8` writes the name into a fixed 128-byte view.
const NAME_WINDOW: usize = 128;

#[derive(Debug)]
pub enum SchemaError {
    /// A jsonSchema construct this encoder does not implement; the node
    /// data-layer may still know how to serialize it.
    Unsupported(String),
    /// The value does not fit its schema in a way @dcl/ecs would also throw on.
    Invalid(String),
}

impl std::fmt::Display for SchemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SchemaError::Unsupported(s) | SchemaError::Invalid(s) => f.write_str(s),
        }
    }
}

impl std::error::Error for SchemaError {}

/// The component id @dcl/ecs derives for a name it has no static mapping for.
///
/// The window is not a limit check: `utf8.write` keeps writing past the end of
/// the typed array, where the writes are dropped, so a longer name is hashed
/// truncated — possibly mid-character.
pub fn component_number_from_name(name: &str) -> u32 {
    let mut buf = [0u8; NAME_WINDOW];
    let bytes = name.as_bytes();
    let n = bytes.len().min(NAME_WINDOW);
    buf[..n].copy_from_slice(&bytes[..n]);
    crc32fast::hash(&buf).wrapping_add(MAX_STATIC_COMPONENT)
}

/// A compiled `jsonSchema`. Property order is fixed at compile time because it
/// *is* the wire format: nothing on the wire identifies a field.
#[derive(Debug, Clone, PartialEq)]
pub enum Schema {
    Map(Vec<(String, Schema)>),
    Array(Box<Schema>),
    Optional(Box<Schema>),
    OneOf(Vec<(String, Schema)>),
    Str,
    Bool,
    Int8,
    Int16,
    Int32,
    Int64,
    Entity,
    F32,
    F64,
    Vector3,
    Quaternion,
    Color3,
    Color4,
    EnumInt(Option<i64>),
    EnumStr(Option<String>),
}

/// Compile a composite's `jsonSchema`, mirroring `jsonSchemaToSchema`: dispatch
/// is on `serializationType` alone, and anything unknown is a hard stop.
///
/// Takes the order-preserving [`JsValue`] tree because serde_json's default map
/// alphabetises property order away — and that order is the whole wire layout.
pub fn compile(js: &JsValue) -> Result<Schema, SchemaError> {
    let st = js
        .get("serializationType")
        .and_then(JsValue::as_str)
        .ok_or_else(|| SchemaError::Unsupported("jsonSchema without serializationType".into()))?;
    Ok(match st {
        "map" => Schema::Map(compile_properties(js, st)?),
        "one-of" => Schema::OneOf(compile_properties(js, st)?),
        "array" => Schema::Array(Box::new(compile(required_child(js, "items", st)?)?)),
        "optional" => Schema::Optional(Box::new(compile(required_child(
            js,
            "optionalJsonSchema",
            st,
        )?)?)),
        "utf8-string" => Schema::Str,
        "boolean" => Schema::Bool,
        "int8" => Schema::Int8,
        "int16" => Schema::Int16,
        "int32" => Schema::Int32,
        "int64" => Schema::Int64,
        "entity" => Schema::Entity,
        "float32" => Schema::F32,
        "float64" => Schema::F64,
        "vector3" => Schema::Vector3,
        "quaternion" => Schema::Quaternion,
        "color3" => Schema::Color3,
        "color4" => Schema::Color4,
        "enum-int" => Schema::EnumInt(match js.get("default") {
            None | Some(JsValue::Null) => None,
            Some(JsValue::Number(n)) if n.fract() == 0.0 && n.is_finite() => Some(*n as i64),
            Some(_) => {
                return Err(SchemaError::Unsupported(
                    "enum-int with a non-integer default".into(),
                ))
            }
        }),
        "enum-string" => Schema::EnumStr(match js.get("default") {
            None | Some(JsValue::Null) => None,
            Some(JsValue::String(s)) => Some(s.clone()),
            Some(_) => {
                return Err(SchemaError::Unsupported(
                    "enum-string with a non-string default".into(),
                ))
            }
        }),
        other => {
            return Err(SchemaError::Unsupported(format!(
                "serializationType '{other}'"
            )))
        }
    })
}

fn required_child<'a>(js: &'a JsValue, key: &str, st: &str) -> Result<&'a JsValue, SchemaError> {
    js.get(key)
        .ok_or_else(|| SchemaError::Unsupported(format!("{st} jsonSchema without '{key}'")))
}

fn compile_properties(js: &JsValue, st: &str) -> Result<Vec<(String, Schema)>, SchemaError> {
    let entries = match js.get("properties") {
        None | Some(JsValue::Null) => return Ok(Vec::new()),
        Some(JsValue::Object(entries)) => entries,
        Some(_) => {
            return Err(SchemaError::Unsupported(format!(
                "{st} jsonSchema with non-object properties"
            )))
        }
    };
    let mut out: Vec<(String, Schema)> = Vec::with_capacity(entries.len());
    for idx in for_in_order(entries) {
        let (key, value) = &entries[idx];
        let compiled = compile(value)?;
        match out.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = compiled,
            None => out.push((key.clone(), compiled)),
        }
    }
    Ok(out)
}

/// `for...in` visits canonical array indices first in ascending numeric order,
/// then the remaining keys in insertion order. Field names are never indices in
/// practice, but the wire layout hangs off this order.
fn for_in_order(entries: &[(String, JsValue)]) -> Vec<usize> {
    let index_of = |k: &str| -> Option<u32> {
        if k.is_empty() || (k.len() > 1 && k.starts_with('0')) {
            return None;
        }
        let n: u32 = k.parse().ok()?;
        (n != u32::MAX).then_some(n)
    };
    let mut indexed: Vec<(u32, usize)> = Vec::new();
    let mut named: Vec<usize> = Vec::new();
    for (i, (k, _)) in entries.iter().enumerate() {
        match index_of(k) {
            Some(n) => indexed.push((n, i)),
            None => named.push(i),
        }
    }
    indexed.sort_unstable();
    indexed
        .into_iter()
        .map(|(_, i)| i)
        .chain(named)
        .collect::<Vec<_>>()
}

/// The value @dcl/ecs's `create()` produces for a schema. `None` is JS
/// `undefined`: an optional, or an enum with no declared default.
pub fn create_default(schema: &Schema) -> Option<Value> {
    Some(match schema {
        Schema::Map(spec) => {
            let mut obj = serde_json::Map::new();
            for (k, sub) in spec {
                if let Some(v) = create_default(sub) {
                    obj.insert(k.clone(), v);
                }
            }
            Value::Object(obj)
        }
        Schema::Array(_) => Value::Array(Vec::new()),
        Schema::Optional(_) => return None,
        Schema::OneOf(_) => Value::Object(serde_json::Map::new()),
        Schema::Str => Value::String(String::new()),
        Schema::Bool => Value::Bool(false),
        Schema::Int8 | Schema::Int16 | Schema::Int32 | Schema::Int64 | Schema::Entity => {
            Value::from(0)
        }
        Schema::F32 | Schema::F64 => Value::from(0.0),
        Schema::Vector3 => zeroed(VECTOR3),
        Schema::Quaternion => zeroed(QUATERNION),
        Schema::Color3 => zeroed(COLOR3),
        Schema::Color4 => zeroed(COLOR4),
        Schema::EnumInt(default) => Value::from((*default)?),
        Schema::EnumStr(default) => Value::String(default.clone()?),
    })
}

const VECTOR3: &[&str] = &["x", "y", "z"];
const QUATERNION: &[&str] = &["x", "y", "z", "w"];
const COLOR3: &[&str] = &["r", "g", "b"];
const COLOR4: &[&str] = &["r", "g", "b", "a"];

fn zeroed(keys: &[&str]) -> Value {
    Value::Object(
        keys.iter()
            .map(|k| ((*k).to_string(), Value::from(0.0)))
            .collect(),
    )
}

/// Serialize one component value, applying the top-level `extend` first.
///
/// `extend` is `{ ...create(), ...defaultValue, ...base }` — a *shallow* spread
/// (`Map.js`). A key missing inside a nested map, or inside a map in an array,
/// is never filled; it stays `undefined` and is encoded (or rejected) as such.
pub fn encode_component_value(schema: &Schema, json: &Value) -> Result<Vec<u8>, SchemaError> {
    let mut out = Vec::new();
    match schema {
        Schema::Map(spec) => {
            for (key, sub) in spec {
                match json.get(key) {
                    Some(v) => encode(sub, Some(v), &mut out)?,
                    None => encode(sub, create_default(sub).as_ref(), &mut out)?,
                }
            }
        }
        _ => encode(schema, Some(json), &mut out)?,
    }
    Ok(out)
}

/// Serialize `value` against `schema`. `None` is JS `undefined` — a key the
/// composite omitted below the extended top level.
pub fn encode(
    schema: &Schema,
    value: Option<&Value>,
    out: &mut Vec<u8>,
) -> Result<(), SchemaError> {
    match schema {
        Schema::Map(spec) => {
            let obj = property_source(value, "map")?;
            for (key, sub) in spec {
                encode(sub, obj.and_then(|o| o.get(key)), out)?;
            }
        }
        Schema::Array(items) => match value {
            Some(Value::Array(arr)) => {
                out.extend_from_slice(&(arr.len() as u32).to_le_bytes());
                for item in arr {
                    encode(items, Some(item), out)?;
                }
            }
            Some(Value::String(s)) => {
                out.extend_from_slice(&(s.encode_utf16().count() as u32).to_le_bytes());
                for ch in s.chars() {
                    encode(items, Some(&Value::String(ch.to_string())), out)?;
                }
            }
            other => {
                return Err(SchemaError::Invalid(format!(
                    "expected an array, got {}",
                    describe(other)
                )))
            }
        },
        Schema::Optional(inner) => {
            if truthy(value) {
                out.push(1);
                encode(inner, value, out)?;
            } else {
                out.push(0);
            }
        }
        Schema::OneOf(specs) => {
            let obj = value.and_then(Value::as_object).ok_or_else(|| {
                SchemaError::Invalid(format!("expected a one-of object, got {}", describe(value)))
            })?;
            let case = obj.get("$case").and_then(Value::as_str).ok_or_else(|| {
                SchemaError::Invalid("one-of value without a string $case".to_string())
            })?;
            let idx = specs
                .iter()
                .position(|(k, _)| k == case)
                .ok_or_else(|| SchemaError::Invalid(format!("one-of has no variant '{case}'")))?;
            out.push((idx + 1) as u8);
            encode(&specs[idx].1, obj.get("value"), out)?;
        }
        Schema::Str | Schema::EnumStr(_) => write_utf8_string(value, out)?,
        Schema::Bool => out.push(truthy(value) as u8),
        Schema::Int8 => out.push(to_int32(to_number(value)) as u8),
        Schema::Int16 => {
            out.extend_from_slice(&(to_int32(to_number(value)) as i16).to_le_bytes());
        }
        Schema::Int32 | Schema::Entity | Schema::EnumInt(_) => {
            out.extend_from_slice(&to_int32(to_number(value)).to_le_bytes());
        }
        Schema::Int64 => out.extend_from_slice(&to_big_int64(value)?.to_le_bytes()),
        Schema::F32 => out.extend_from_slice(&(to_number(value) as f32).to_le_bytes()),
        Schema::F64 => out.extend_from_slice(&to_number(value).to_le_bytes()),
        Schema::Vector3 => write_floats(value, VECTOR3, out)?,
        Schema::Quaternion => write_floats(value, QUATERNION, out)?,
        Schema::Color3 => write_floats(value, COLOR3, out)?,
        Schema::Color4 => write_floats(value, COLOR4, out)?,
    }
    Ok(())
}

/// `writeUtf8String` never checks the type: `@protobufjs/utf8` walks
/// `value.length` and `value.charCodeAt(i)`. A value with no `length` — a
/// number, a boolean, a plain object — therefore writes as the empty string,
/// while a positive `length` with no `charCodeAt` (any non-empty array) is a
/// TypeError, as is reading `.length` off `null` or `undefined`.
fn write_utf8_string(value: Option<&Value>, out: &mut Vec<u8>) -> Result<(), SchemaError> {
    let text = match value {
        Some(Value::String(s)) => s.as_str(),
        None | Some(Value::Null) => {
            return Err(SchemaError::Invalid(format!(
                "a string leaf cannot read .length off {}",
                describe(value)
            )))
        }
        Some(Value::Array(items)) if !items.is_empty() => {
            return Err(SchemaError::Invalid(
                "a non-empty array on a string leaf has no charCodeAt".into(),
            ))
        }
        Some(Value::Object(obj)) if to_number(obj.get("length")) > 0.0 => {
            return Err(SchemaError::Invalid(
                "an object with a positive length on a string leaf has no charCodeAt".into(),
            ))
        }
        Some(_) => "",
    };
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(text.as_bytes());
    Ok(())
}

fn write_floats(
    value: Option<&Value>,
    keys: &[&str],
    out: &mut Vec<u8>,
) -> Result<(), SchemaError> {
    let obj = property_source(value, "vector")?;
    for key in keys {
        let component = to_number(obj.and_then(|o| o.get(*key))) as f32;
        out.extend_from_slice(&component.to_le_bytes());
    }
    Ok(())
}

/// Reading a property off `undefined` or `null` is a TypeError in JS; off any
/// other non-object it yields `undefined` for every key.
fn property_source<'a>(
    value: Option<&'a Value>,
    what: &str,
) -> Result<Option<&'a serde_json::Map<String, Value>>, SchemaError> {
    match value {
        None => Err(SchemaError::Invalid(format!("{what} value is missing"))),
        Some(Value::Null) => Err(SchemaError::Invalid(format!("{what} value is null"))),
        Some(Value::Object(obj)) => Ok(Some(obj)),
        Some(_) => Ok(None),
    }
}

fn describe(value: Option<&Value>) -> String {
    match value {
        None => "nothing".to_string(),
        Some(v) => v.to_string(),
    }
}

/// ECMAScript truthiness: `undefined`, `null`, `false`, `0`, `-0` and `""` are
/// falsy; `[]` and `{}` are not.
fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// ECMAScript ToNumber, restricted to the shapes JSON can hold.
fn to_number(value: Option<&Value>) -> f64 {
    match value {
        None => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(b)) => *b as u8 as f64,
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(s)) => string_to_number(s),
        Some(Value::Array(items)) => match items.as_slice() {
            [] => 0.0,
            [only] => string_to_number(&join_element(only)),
            _ => f64::NAN,
        },
        Some(Value::Object(_)) => f64::NAN,
    }
}

/// One element as `Array.prototype.join` renders it: null joins as empty, a
/// nested array joins recursively, anything else is its String() form.
fn join_element(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        Value::Array(items) => items.iter().map(join_element).collect::<Vec<_>>().join(","),
        Value::Object(_) => "[object Object]".to_string(),
    }
}

/// ECMAScript ToNumber over a string (the StringNumericLiteral grammar).
///
/// `f64::from_str` misses the radix prefixes silently: "0x10" fails to parse,
/// becomes NaN, and ToInt32 turns that into 0 where the runtime reads 16. It
/// also accepts spellings JS rejects ("inf", "1_0").
fn string_to_number(s: &str) -> f64 {
    let t = s.trim();
    if t.is_empty() {
        return 0.0;
    }
    let radix = match t.get(..2).map(str::to_ascii_lowercase).as_deref() {
        Some("0x") => Some(16),
        Some("0o") => Some(8),
        Some("0b") => Some(2),
        _ => None,
    };
    if let Some(radix) = radix {
        return u64::from_str_radix(&t[2..], radix).map_or(f64::NAN, |v| v as f64);
    }
    match t {
        "Infinity" | "+Infinity" => f64::INFINITY,
        "-Infinity" => f64::NEG_INFINITY,
        _ if t.contains('_') => f64::NAN,
        _ if t.contains(|c: char| c.is_ascii_alphabetic() && c != 'e' && c != 'E') => f64::NAN,
        _ => t.parse::<f64>().unwrap_or(f64::NAN),
    }
}

/// ECMAScript ToInt32: NaN and the infinities collapse to 0, everything else
/// truncates toward zero and wraps modulo 2^32 (Rust's `as` would saturate).
fn to_int32(n: f64) -> i32 {
    if !n.is_finite() {
        return 0;
    }
    (n.trunc().rem_euclid(4294967296.0) as u32) as i32
}

/// `writeInt64` takes `BigInt(value)`, and `BigInt` is not `Number`: it throws
/// on a fractional number rather than truncating, throws on anything it cannot
/// read exactly (`"1.5"`, `"abc"`, `null`, an object), and reads a string with
/// full precision — `BigInt("9007199254740993")` is exact where `Number` of the
/// same digits has already rounded. `setBigInt64` then wraps modulo 2^64.
///
/// A JSON *number* is still routed through f64: node reaches it through
/// `JSON.parse`, so a literal past 2^53 was rounded before @dcl/ecs saw it.
fn to_big_int64(value: Option<&Value>) -> Result<i64, SchemaError> {
    let f = match value {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::Bool(b)) => return Ok(*b as i64),
        Some(Value::String(s)) => return string_to_big_int(s),
        Some(Value::Array(items)) => {
            return match items.as_slice() {
                [] => Ok(0),
                [only] => string_to_big_int(&join_element(only)),
                _ => Err(SchemaError::Invalid(format!(
                    "int64 cannot parse the joined array {}",
                    describe(value)
                ))),
            }
        }
        other => {
            return Err(SchemaError::Invalid(format!(
                "int64 cannot convert {}",
                describe(other)
            )))
        }
    };
    if !f.is_finite() || f.fract() != 0.0 {
        return Err(SchemaError::Invalid(format!(
            "int64 cannot hold the non-integral value {f}"
        )));
    }
    Ok(wrap_to_i64(f))
}

/// `StringToBigInt`: the radix prefixes of ToNumber, but no fraction, no
/// exponent, no `Infinity`, and a syntax error instead of NaN. Accumulated with
/// wrapping arithmetic, which is `BigInt.asIntN(64, ...)` for free — and keeps
/// digits past 2^53 exact, which is the whole reason this is not `to_number`.
fn string_to_big_int(s: &str) -> Result<i64, SchemaError> {
    let refuse = || SchemaError::Invalid(format!("int64 cannot parse the string {s:?}"));
    let t = s.trim();
    if t.is_empty() {
        return Ok(0);
    }
    let (radix, digits, negative) = match t.get(..2).map(str::to_ascii_lowercase).as_deref() {
        Some("0x") => (16, &t[2..], false),
        Some("0o") => (8, &t[2..], false),
        Some("0b") => (2, &t[2..], false),
        _ => match t.strip_prefix('-') {
            Some(rest) => (10, rest, true),
            None => (10, t.strip_prefix('+').unwrap_or(t), false),
        },
    };
    if digits.is_empty() {
        return Err(refuse());
    }
    let mut magnitude: u64 = 0;
    for c in digits.chars() {
        let digit = c.to_digit(radix).ok_or_else(refuse)?;
        magnitude = magnitude
            .wrapping_mul(u64::from(radix))
            .wrapping_add(u64::from(digit));
    }
    Ok(if negative {
        (magnitude as i64).wrapping_neg()
    } else {
        magnitude as i64
    })
}

/// `BigInt.asIntN(64, …)`: the low 64 bits of an integral f64. Rust's `as i64`
/// saturates instead of wrapping, so anything out of range is reassembled from
/// the mantissa.
fn wrap_to_i64(f: f64) -> i64 {
    const MIN: f64 = -9223372036854775808.0;
    const MAX: f64 = 9223372036854775808.0;
    if (MIN..MAX).contains(&f) {
        return f as i64;
    }
    let bits = f.abs().to_bits();
    let exponent = ((bits >> 52) & 0x7ff) as i64 - 1075;
    let mantissa = (bits & ((1u64 << 52) - 1)) | (1u64 << 52);
    let low = if exponent >= 64 {
        0
    } else {
        mantissa.wrapping_shl(exponent as u32)
    };
    let magnitude = low as i64;
    if f.is_sign_negative() {
        magnitude.wrapping_neg()
    } else {
        magnitude
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema(js: serde_json::Value) -> Schema {
        compile(&crate::jsjson::parse(&js.to_string()).unwrap()).unwrap()
    }

    fn enc(schema: &Schema, value: &serde_json::Value) -> Vec<u8> {
        encode_component_value(schema, value).unwrap()
    }

    #[test]
    fn component_ids_match_the_inspector_shim_table() {
        for (name, id) in [
            ("core-schema::Network-Entity", 1075645054u32),
            ("core-schema::Network-Parent", 1420531287),
            ("core-schema::Name", 3864921337),
            ("core-schema::Sync-Components", 2241712969),
            ("core-schema::Tags", 4148173090),
            ("inspector::Nodes", 2032030903),
            ("asset-packs::AdminTools", 99929642),
        ] {
            assert_eq!(component_number_from_name(name), id, "{name}");
        }
        assert_eq!(component_number_from_name("composite::root"), 2548763028);
        assert_eq!(component_number_from_name("cube-id"), 1270506178);
    }

    #[test]
    fn a_name_longer_than_the_window_hashes_truncated() {
        let long = "x".repeat(200);
        assert_eq!(
            component_number_from_name(&long),
            component_number_from_name(&"x".repeat(NAME_WINDOW))
        );
    }

    #[test]
    fn map_properties_serialize_in_declaration_order_not_alphabetically() {
        let s = schema(json!({
            "type": "object",
            "properties": {
                "networkId": { "type": "integer", "serializationType": "int64" },
                "entityId": { "type": "integer", "serializationType": "entity" }
            },
            "serializationType": "map"
        }));
        assert_eq!(
            enc(&s, &json!({ "networkId": 0, "entityId": 8002 })),
            vec![0, 0, 0, 0, 0, 0, 0, 0, 0x42, 0x1f, 0, 0]
        );
    }

    #[test]
    fn strings_are_length_prefixed_without_a_terminator() {
        let s = schema(json!({
            "type": "object",
            "properties": { "value": { "type": "string", "serializationType": "utf8-string" } },
            "serializationType": "map"
        }));
        let mut expected = vec![0x0b, 0, 0, 0];
        expected.extend_from_slice(b"Admin Tools");
        assert_eq!(enc(&s, &json!({ "value": "Admin Tools" })), expected);
    }

    #[test]
    fn arrays_write_a_u32_count_then_their_items() {
        let s = schema(json!({
            "type": "object",
            "properties": {
                "componentIds": {
                    "type": "array",
                    "items": { "type": "integer", "serializationType": "int64" },
                    "serializationType": "array"
                }
            },
            "serializationType": "map"
        }));
        assert_eq!(
            enc(&s, &json!({ "componentIds": [99929642] })),
            vec![1, 0, 0, 0, 0x2a, 0xce, 0xf4, 0x05, 0, 0, 0, 0]
        );
    }

    #[test]
    fn a_property_less_map_serializes_to_nothing() {
        let s = schema(json!({
            "type": "object",
            "properties": {},
            "serializationType": "map"
        }));
        assert!(enc(&s, &json!({})).is_empty());
    }

    #[test]
    fn optional_presence_is_js_truthiness_not_key_presence() {
        let s = |inner: serde_json::Value| {
            schema(
                json!({ "type": "object", "serializationType": "optional", "optionalJsonSchema": inner }),
            )
        };
        let int = s(json!({ "type": "integer", "serializationType": "int32" }));
        let string = s(json!({ "type": "string", "serializationType": "utf8-string" }));
        let boolean = s(json!({ "type": "boolean", "serializationType": "boolean" }));
        let list = s(json!({
            "type": "array",
            "items": { "type": "integer", "serializationType": "int32" },
            "serializationType": "array"
        }));
        for (schema, value) in [
            (&int, json!(0)),
            (&string, json!("")),
            (&boolean, json!(false)),
            (&int, json!(null)),
        ] {
            assert_eq!(
                enc(schema, &value),
                vec![0],
                "{value} must encode as absent"
            );
        }
        assert_eq!(enc(&list, &json!([])), vec![1, 0, 0, 0, 0]);
        assert_eq!(enc(&int, &json!(7)), vec![1, 7, 0, 0, 0]);
    }

    #[test]
    fn one_of_writes_a_one_based_index_into_the_property_order() {
        let s = schema(json!({
            "type": "object",
            "serializationType": "one-of",
            "properties": {
                "single": { "type": "number", "serializationType": "float32" },
                "range": {
                    "type": "array",
                    "items": { "type": "number", "serializationType": "float32" },
                    "serializationType": "array"
                }
            }
        }));
        assert_eq!(
            enc(&s, &json!({ "$case": "range", "value": [85] })),
            vec![2, 1, 0, 0, 0, 0, 0, 0xaa, 0x42]
        );
        assert_eq!(
            enc(&s, &json!({ "$case": "single", "value": 85 })),
            vec![1, 0, 0, 0xaa, 0x42]
        );
    }

    #[test]
    fn the_top_level_extend_fills_only_the_top_level() {
        let s = schema(json!({
            "type": "object",
            "serializationType": "map",
            "properties": {
                "flag": { "type": "boolean", "serializationType": "boolean" },
                "nested": {
                    "type": "object",
                    "serializationType": "map",
                    "properties": {
                        "n": { "type": "integer", "serializationType": "int32" },
                        "s": { "type": "string", "serializationType": "utf8-string" }
                    }
                }
            }
        }));
        assert_eq!(enc(&s, &json!({})), vec![0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(
            enc(&s, &json!({ "nested": { "s": "" } })),
            vec![0, 0, 0, 0, 0, 0, 0, 0, 0]
        );
        assert!(encode_component_value(&s, &json!({ "nested": { "n": 1 } })).is_err());
    }

    #[test]
    fn int32_wraps_like_to_int32_instead_of_saturating() {
        assert_eq!(to_int32(4294967298.0), 2);
        assert_eq!(to_int32(-1.0), -1);
        assert_eq!(to_int32(2147483648.0), i32::MIN);
        assert_eq!(to_int32(3.9), 3);
        assert_eq!(to_int32(-3.9), -3);
        assert_eq!(to_int32(f64::NAN), 0);
        assert_eq!(to_int32(f64::INFINITY), 0);
        assert_eq!(to_int32(1e300), 0);
    }

    /// Coercion, not layout, is where a silently wrong byte hides: nothing
    /// errors, the scene just loads with a different value. Each case was
    /// checked against @dcl/ecs rather than read off the spec.
    #[test]
    fn to_number_matches_the_javascript_grammar_on_the_awkward_shapes() {
        assert_eq!(string_to_number("0x10"), 16.0);
        assert_eq!(string_to_number("0b101"), 5.0);
        assert_eq!(string_to_number("0o17"), 15.0);
        assert!(string_to_number("-0x10").is_nan());
        assert!(string_to_number("inf").is_nan());
        assert!(string_to_number("nan").is_nan());
        assert!(string_to_number("1_0").is_nan());
        assert_eq!(string_to_number("Infinity"), f64::INFINITY);
        assert_eq!(string_to_number("  12  "), 12.0);
        assert_eq!(string_to_number(""), 0.0);
        assert_eq!(string_to_number("1e3"), 1000.0);

        assert!(to_number(Some(&json!([true]))).is_nan());
        assert_eq!(to_number(Some(&json!([]))), 0.0);
        assert_eq!(to_number(Some(&json!([5]))), 5.0);
        assert_eq!(to_number(Some(&json!(["0x10"]))), 16.0);
        assert_eq!(to_number(Some(&json!([null]))), 0.0);
        assert!(to_number(Some(&json!([1, 2]))).is_nan());
    }

    /// `writeUtf8String` is `@protobufjs/utf8` with no type check in front of
    /// it, so an ill-typed value is not an error there — it is an empty string,
    /// which is a silently wrong byte rather than a loud one. Each expectation
    /// was read off @dcl/ecs, not off the spec.
    #[test]
    fn a_non_string_on_a_string_leaf_writes_the_empty_string() {
        let s = schema(json!({ "type": "string", "serializationType": "utf8-string" }));
        for value in [
            json!(5),
            json!(-1.5),
            json!(true),
            json!(false),
            json!({}),
            json!([]),
        ] {
            assert_eq!(enc(&s, &value), vec![0, 0, 0, 0], "{value} writes as \"\"");
        }
        for value in [
            json!(null),
            json!([true]),
            json!(["ab"]),
            json!({ "length": 2 }),
        ] {
            assert!(
                encode_component_value(&s, &value).is_err(),
                "{value} must throw the way JS does"
            );
        }
        assert_eq!(enc(&s, &json!({ "length": 0 })), vec![0, 0, 0, 0]);
    }

    /// A string on an int64 leaf goes through `BigInt`, not `Number`: it keeps
    /// every digit (where `Number` has already rounded), reads the radix
    /// prefixes, and throws where `Number` would have produced NaN.
    #[test]
    fn int64_coerces_through_bigint_rather_than_number() {
        assert_eq!(to_big_int64(Some(&json!("0x10"))).unwrap(), 16);
        assert_eq!(to_big_int64(Some(&json!("0X10"))).unwrap(), 16);
        assert_eq!(to_big_int64(Some(&json!("0o17"))).unwrap(), 15);
        assert_eq!(to_big_int64(Some(&json!("0b101"))).unwrap(), 5);
        assert_eq!(to_big_int64(Some(&json!("  12  "))).unwrap(), 12);
        assert_eq!(to_big_int64(Some(&json!(""))).unwrap(), 0);
        assert_eq!(to_big_int64(Some(&json!("-12"))).unwrap(), -12);
        assert_eq!(
            to_big_int64(Some(&json!("9007199254740993"))).unwrap(),
            9007199254740993
        );
        assert_eq!(to_big_int64(Some(&json!(true))).unwrap(), 1);
        assert_eq!(to_big_int64(Some(&json!([]))).unwrap(), 0);
        assert_eq!(to_big_int64(Some(&json!([5]))).unwrap(), 5);
        assert_eq!(to_big_int64(Some(&json!(["0x10"]))).unwrap(), 16);
        assert_eq!(to_big_int64(Some(&json!([null]))).unwrap(), 0);
        assert_eq!(to_big_int64(Some(&json!([[7]]))).unwrap(), 7);
        for value in [
            json!("1.5"),
            json!("inf"),
            json!("Infinity"),
            json!("1_0"),
            json!("-0x10"),
            json!("0x"),
            json!("1e3"),
            json!([true]),
            json!([1, 2]),
            json!({}),
            json!(null),
        ] {
            assert!(
                to_big_int64(Some(&value)).is_err(),
                "BigInt({value}) must throw"
            );
        }
    }

    /// An array leaf writes `value.length` and then iterates `value`. A string
    /// satisfies both, so it serializes as an array of its characters — and the
    /// count is UTF-16 code units while the iteration walks code points, which
    /// is why an astral character writes one more than it produces.
    #[test]
    fn a_string_on_an_array_leaf_serializes_as_its_characters() {
        let ints = schema(json!({
            "type": "array",
            "serializationType": "array",
            "items": { "type": "integer", "serializationType": "int32" }
        }));
        assert_eq!(
            enc(&ints, &json!("12")),
            vec![2, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0]
        );
        assert_eq!(enc(&ints, &json!("")), vec![0, 0, 0, 0]);
        let strings = schema(json!({
            "type": "array",
            "serializationType": "array",
            "items": { "type": "string", "serializationType": "utf8-string" }
        }));
        let mut expected = vec![4, 0, 0, 0, 1, 0, 0, 0];
        expected.extend_from_slice(b"a");
        expected.extend_from_slice(&[4, 0, 0, 0]);
        expected.extend_from_slice("\u{1F600}".as_bytes());
        expected.extend_from_slice(&[1, 0, 0, 0]);
        expected.extend_from_slice(b"b");
        assert_eq!(enc(&strings, &json!("a\u{1F600}b")), expected);
        for value in [json!(5), json!(true), json!(null), json!({})] {
            assert!(encode_component_value(&ints, &value).is_err(), "{value}");
        }
    }

    #[test]
    fn int64_rejects_a_fractional_value_the_way_bigint_does() {
        assert!(to_big_int64(Some(&json!(1.5))).is_err());
        assert_eq!(
            to_big_int64(Some(&json!(4149214247u64))).unwrap(),
            4149214247
        );
        assert_eq!(to_big_int64(Some(&json!(-1))).unwrap(), -1);
        assert_eq!(
            to_big_int64(Some(&json!(9007199254740993u64))).unwrap(),
            9007199254740992
        );
        assert_eq!(wrap_to_i64(18446744073709551616.0), 0);
        assert_eq!(wrap_to_i64(9223372036854775808.0), i64::MIN);
        assert_eq!(wrap_to_i64(-9223372036854777856.0), 9223372036854773760);
    }

    #[test]
    fn a_missing_nested_number_coerces_the_way_javascript_does() {
        let s = schema(json!({
            "type": "object",
            "serializationType": "map",
            "properties": {
                "v": { "type": "object", "serializationType": "vector3" }
            }
        }));
        let bytes = enc(&s, &json!({ "v": { "x": 1.0, "z": 2.0 } }));
        assert_eq!(f32::from_le_bytes(bytes[0..4].try_into().unwrap()), 1.0);
        assert!(f32::from_le_bytes(bytes[4..8].try_into().unwrap()).is_nan());
        assert_eq!(f32::from_le_bytes(bytes[8..12].try_into().unwrap()), 2.0);
    }

    #[test]
    fn an_unknown_serialization_type_is_unsupported_not_a_guess() {
        let js = crate::jsjson::parse(
            &json!({ "serializationType": "protocol-buffer", "protocolBuffer": "PBAnimator" })
                .to_string(),
        )
        .unwrap();
        assert!(matches!(compile(&js), Err(SchemaError::Unsupported(_))));
    }

    #[test]
    fn for_in_visits_array_indices_first_in_numeric_order() {
        let entries: Vec<(String, JsValue)> = ["b", "10", "2", "a", "01"]
            .iter()
            .map(|k| ((*k).to_string(), JsValue::Null))
            .collect();
        let order: Vec<&str> = for_in_order(&entries)
            .into_iter()
            .map(|i| entries[i].0.as_str())
            .collect();
        assert_eq!(order, vec!["2", "10", "b", "a", "01"]);
    }
}
