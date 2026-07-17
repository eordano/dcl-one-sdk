//! Native `main.crdt` generation from `.composite` files.
//!
//! Replaces the node + `@dcl/inspector` fallback in the build path: composites
//! are parsed in Rust and core components are serialized against the vendored
//! @dcl/protocol descriptors (see build.rs), producing output byte-identical
//! to the upstream toolchain's.
//!
//! ts-proto writer semantics reproduced deliberately (verified against
//! main.crdt files produced by @dcl/ecs 7.24.5):
//! - packed numeric repeated fields always write their tag + length, even when
//!   empty (`BoxMesh.uvs = []` encodes as `0a 00`)
//! - repeated string/bytes/message fields write nothing when empty
//! - non-`optional` scalars are skipped at their proto3 default
//! - `optional` (explicit-presence) scalars are written whenever the composite
//!   provides them, even at the default value
//! - oneof members (`{"$case": "box", "box": …}` in composite JSON) are always
//!   written when selected, even when empty
//! - fields encode in field-number order

use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

include!(concat!(env!("OUT_DIR"), "/components_schema.rs"));

pub(crate) struct MsgDef {
    pub name: &'static str,
    pub map_entry: bool,
    pub fields: &'static [FieldDef],
}

pub(crate) struct FieldDef {
    pub number: u32,
    pub json_name: &'static str,
    pub kind: FieldKind,
    pub repeated: bool,
    pub packed: bool,
    pub optional: bool,
    /// ts-proto JSON name of the containing (real) oneof, if any.
    pub oneof: Option<&'static str>,
}

#[derive(Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub(crate) enum FieldKind {
    Double,
    Float,
    Int32,
    Int64,
    Uint32,
    Uint64,
    Sint32,
    Sint64,
    Fixed32,
    Fixed64,
    Sfixed32,
    Sfixed64,
    Bool,
    Str,
    Bytes,
    Enum(usize),
    Msg(usize),
}

pub(crate) struct EnumDef {
    #[allow(dead_code)]
    pub name: &'static str,
    pub values: &'static [(&'static str, i32)],
}

pub(crate) struct ComponentDef {
    pub name: &'static str,
    pub id: u32,
    pub msg: usize,
}

const TRANSFORM_COMPONENT_ID: u32 = 1;
const CRDT_PUT_COMPONENT: u32 = 1;
const CRDT_HEADER_LEN: u32 = 24;

#[derive(Debug)]
pub enum GenError {
    /// The scene uses something the native path does not cover (custom
    /// jsonSchema components, unknown component names); the node data-layer
    /// fallback may still handle it.
    Unsupported(String),
    /// The composite itself is malformed; no toolchain can instance it.
    Invalid(String),
}

impl fmt::Display for GenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GenError::Unsupported(s) | GenError::Invalid(s) => f.write_str(s),
        }
    }
}

impl std::error::Error for GenError {}

pub struct Generated {
    /// Number of composite files instanced.
    pub composites: u64,
    /// The main.crdt bytes.
    pub bytes: Vec<u8>,
}

/// Instance every `.composite` under `root` (direct entity mapping, later
/// files override earlier ones per entity+component) and serialize the result
/// as main.crdt PUT_COMPONENT messages. Returns None when the scene has no
/// composites.
pub fn generate(root: &Path) -> Result<Option<Generated>, GenError> {
    let files = crate::entrypoint::find_composites(root);
    if files.is_empty() {
        return Ok(None);
    }
    // Component order follows first appearance across the sorted composite
    // files, matching the engine's registration-on-first-use order upstream.
    let mut order: Vec<String> = Vec::new();
    let mut comps: BTreeMap<String, BTreeMap<u32, Value>> = BTreeMap::new();
    for file in &files {
        let text = std::fs::read_to_string(file)
            .map_err(|e| GenError::Invalid(format!("reading {}: {e}", file.display())))?;
        let doc: Value = serde_json::from_str(&text)
            .map_err(|e| GenError::Invalid(format!("{}: {e}", file.display())))?;
        let components = doc
            .get("components")
            .and_then(|c| c.as_array())
            .ok_or_else(|| GenError::Invalid(format!("{}: no components array", file.display())))?;
        for comp in components {
            let name = comp.get("name").and_then(|n| n.as_str()).ok_or_else(|| {
                GenError::Invalid(format!("{}: component without a name", file.display()))
            })?;
            if name != "core::Transform"
                && component_by_name(name).is_none()
                && comp.get("jsonSchema").is_some()
            {
                return Err(GenError::Unsupported(format!(
                    "component '{name}' declares a custom jsonSchema"
                )));
            }
            let data = comp
                .get("data")
                .and_then(|d| d.as_object())
                .ok_or_else(|| {
                    GenError::Invalid(format!(
                        "{}: component '{name}' has no data",
                        file.display()
                    ))
                })?;
            if !comps.contains_key(name) {
                order.push(name.to_string());
            }
            let slot = comps.entry(name.to_string()).or_default();
            for (key, entry) in data {
                let entity: u32 = key.parse().map_err(|_| {
                    GenError::Invalid(format!(
                        "{}: '{name}' has non-numeric entity id '{key}'",
                        file.display()
                    ))
                })?;
                let json = entry.get("json").ok_or_else(|| {
                    GenError::Unsupported(format!(
                        "'{name}' entity {entity} has no json value (binary composites are not supported)"
                    ))
                })?;
                slot.insert(entity, json.clone());
            }
        }
    }
    let mut bytes = Vec::new();
    for name in &order {
        for (entity, json) in &comps[name] {
            let (id, data) = encode_component(name, json)?;
            put_component(&mut bytes, *entity, id, &data);
        }
    }
    Ok(Some(Generated {
        composites: files.len() as u64,
        bytes,
    }))
}

fn component_by_name(name: &str) -> Option<&'static ComponentDef> {
    COMPONENTS.iter().find(|c| c.name == name)
}

fn encode_component(name: &str, json: &Value) -> Result<(u32, Vec<u8>), GenError> {
    if name == "core::Transform" {
        return Ok((TRANSFORM_COMPONENT_ID, encode_transform(json)));
    }
    let comp = component_by_name(name)
        .ok_or_else(|| GenError::Unsupported(format!("unknown component '{name}'")))?;
    let mut out = Vec::new();
    encode_msg(&MESSAGES[comp.msg], json, &mut out)?;
    Ok((comp.id, out))
}

fn put_component(out: &mut Vec<u8>, entity: u32, component: u32, data: &[u8]) {
    let words = [
        CRDT_HEADER_LEN + data.len() as u32,
        CRDT_PUT_COMPONENT,
        entity,
        component,
        0, // timestamp
        data.len() as u32,
    ];
    for w in words {
        out.extend_from_slice(&w.to_le_bytes());
    }
    out.extend_from_slice(data);
}

/// core::Transform is not protobuf: @dcl/ecs serializes it as a fixed 44-byte
/// struct (position, rotation, scale, parent — all LE).
fn encode_transform(json: &Value) -> Vec<u8> {
    let g = |obj: &str, key: &str, default: f32| -> f32 {
        json.get(obj)
            .and_then(|o| o.get(key))
            .and_then(|v| v.as_f64())
            .map(|v| v as f32)
            .unwrap_or(default)
    };
    let mut out = Vec::with_capacity(44);
    for v in [
        g("position", "x", 0.0),
        g("position", "y", 0.0),
        g("position", "z", 0.0),
        g("rotation", "x", 0.0),
        g("rotation", "y", 0.0),
        g("rotation", "z", 0.0),
        g("rotation", "w", 1.0),
        g("scale", "x", 1.0),
        g("scale", "y", 1.0),
        g("scale", "z", 1.0),
    ] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    let parent = json.get("parent").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    out.extend_from_slice(&parent.to_le_bytes());
    out
}

// ---- protobuf writer -------------------------------------------------------

fn varint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn tag(out: &mut Vec<u8>, number: u32, wire: u32) {
    varint(out, ((number as u64) << 3) | wire as u64);
}

fn len_delimited(out: &mut Vec<u8>, number: u32, payload: &[u8]) {
    tag(out, number, 2);
    varint(out, payload.len() as u64);
    out.extend_from_slice(payload);
}

fn err_for(msg: &MsgDef, field: &FieldDef, what: &str) -> GenError {
    GenError::Invalid(format!("{}.{}: {what}", msg.name, field.json_name))
}

fn json_f64(v: &Value) -> Option<f64> {
    v.as_f64()
}

fn json_i64(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| v.as_f64().map(|f| f as i64))
}

fn json_u64(v: &Value) -> Option<u64> {
    v.as_u64().or_else(|| v.as_f64().map(|f| f as u64))
}

fn enum_number(def: &EnumDef, v: &Value) -> Option<i64> {
    if let Some(n) = json_i64(v) {
        return Some(n);
    }
    let name = v.as_str()?;
    def.values
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, num)| *num as i64)
}

fn zigzag32(v: i64) -> u64 {
    let v = v as i32;
    ((v << 1) ^ (v >> 31)) as u32 as u64
}

fn zigzag64(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}

fn bytes_value(v: &Value) -> Option<Vec<u8>> {
    use base64::Engine;
    if let Some(s) = v.as_str() {
        return base64::engine::general_purpose::STANDARD.decode(s).ok();
    }
    v.as_array().map(|arr| {
        arr.iter()
            .filter_map(|b| b.as_u64().map(|b| b as u8))
            .collect()
    })
}

/// A scalar is at its proto3 default (and a non-`optional` field would skip it).
fn is_default(kind: FieldKind, v: &Value) -> bool {
    match kind {
        FieldKind::Bool => v.as_bool() == Some(false),
        FieldKind::Str => v.as_str() == Some(""),
        FieldKind::Bytes => bytes_value(v).is_some_and(|b| b.is_empty()),
        FieldKind::Enum(e) => enum_number(&ENUMS[e], v) == Some(0),
        FieldKind::Msg(_) => false,
        _ => json_f64(v) == Some(0.0),
    }
}

/// Write one scalar value without its tag (packed-array element form).
fn write_scalar_raw(
    msg: &MsgDef,
    field: &FieldDef,
    v: &Value,
    out: &mut Vec<u8>,
) -> Result<(), GenError> {
    let num = || json_f64(v).ok_or_else(|| err_for(msg, field, "expected a number"));
    match field.kind {
        FieldKind::Double => out.extend_from_slice(&num()?.to_le_bytes()),
        FieldKind::Float => out.extend_from_slice(&(num()? as f32).to_le_bytes()),
        FieldKind::Fixed32 => out.extend_from_slice(
            &(json_u64(v).ok_or_else(|| err_for(msg, field, "expected a number"))? as u32)
                .to_le_bytes(),
        ),
        FieldKind::Sfixed32 => out.extend_from_slice(
            &(json_i64(v).ok_or_else(|| err_for(msg, field, "expected a number"))? as i32)
                .to_le_bytes(),
        ),
        FieldKind::Fixed64 => out.extend_from_slice(
            &json_u64(v)
                .ok_or_else(|| err_for(msg, field, "expected a number"))?
                .to_le_bytes(),
        ),
        FieldKind::Sfixed64 => out.extend_from_slice(
            &json_i64(v)
                .ok_or_else(|| err_for(msg, field, "expected a number"))?
                .to_le_bytes(),
        ),
        FieldKind::Bool => varint(
            out,
            v.as_bool()
                .ok_or_else(|| err_for(msg, field, "expected a bool"))? as u64,
        ),
        FieldKind::Int32 | FieldKind::Int64 => varint(
            out,
            json_i64(v).ok_or_else(|| err_for(msg, field, "expected a number"))? as u64,
        ),
        FieldKind::Uint32 | FieldKind::Uint64 => varint(
            out,
            json_u64(v).ok_or_else(|| err_for(msg, field, "expected a number"))?,
        ),
        FieldKind::Sint32 => {
            let n = json_i64(v).ok_or_else(|| err_for(msg, field, "expected a number"))?;
            varint(out, zigzag32(n));
        }
        FieldKind::Sint64 => {
            let n = json_i64(v).ok_or_else(|| err_for(msg, field, "expected a number"))?;
            varint(out, zigzag64(n));
        }
        FieldKind::Enum(e) => {
            let n = enum_number(&ENUMS[e], v)
                .ok_or_else(|| err_for(msg, field, "unknown enum value"))?;
            varint(out, n as u64);
        }
        FieldKind::Str | FieldKind::Bytes | FieldKind::Msg(_) => {
            unreachable!("length-delimited kinds are not packed")
        }
    }
    Ok(())
}

fn wire_type(kind: FieldKind) -> u32 {
    match kind {
        FieldKind::Double | FieldKind::Fixed64 | FieldKind::Sfixed64 => 1,
        FieldKind::Float | FieldKind::Fixed32 | FieldKind::Sfixed32 => 5,
        FieldKind::Str | FieldKind::Bytes | FieldKind::Msg(_) => 2,
        _ => 0,
    }
}

/// Write one value with its tag.
fn write_value(
    msg: &MsgDef,
    field: &FieldDef,
    v: &Value,
    out: &mut Vec<u8>,
) -> Result<(), GenError> {
    match field.kind {
        FieldKind::Str => {
            let s = v
                .as_str()
                .ok_or_else(|| err_for(msg, field, "expected a string"))?;
            len_delimited(out, field.number, s.as_bytes());
        }
        FieldKind::Bytes => {
            let b = bytes_value(v).ok_or_else(|| err_for(msg, field, "expected bytes"))?;
            len_delimited(out, field.number, &b);
        }
        FieldKind::Msg(idx) => {
            let nested_def = &MESSAGES[idx];
            if nested_def.map_entry {
                return encode_map(msg, field, nested_def, v, out);
            }
            let mut nested = Vec::new();
            encode_msg(nested_def, v, &mut nested)?;
            len_delimited(out, field.number, &nested);
        }
        _ => {
            tag(out, field.number, wire_type(field.kind));
            write_scalar_raw(msg, field, v, out)?;
        }
    }
    Ok(())
}

/// Map fields arrive as a JSON object; each entry is a nested message with
/// key = field 1, value = field 2.
fn encode_map(
    msg: &MsgDef,
    field: &FieldDef,
    entry_def: &'static MsgDef,
    v: &Value,
    out: &mut Vec<u8>,
) -> Result<(), GenError> {
    let obj = v
        .as_object()
        .ok_or_else(|| err_for(msg, field, "expected an object for a map field"))?;
    let (key_field, val_field) = (&entry_def.fields[0], &entry_def.fields[1]);
    for (k, val) in obj {
        let mut entry = Vec::new();
        let key_json = match key_field.kind {
            FieldKind::Str => Value::String(k.clone()),
            _ => serde_json::from_str(k)
                .map_err(|_| err_for(msg, field, "non-numeric key for a numeric map"))?,
        };
        if !is_default(key_field.kind, &key_json) {
            write_value(entry_def, key_field, &key_json, &mut entry)?;
        }
        if !is_default(val_field.kind, val) {
            write_value(entry_def, val_field, val, &mut entry)?;
        }
        len_delimited(out, field.number, &entry);
    }
    Ok(())
}

fn encode_msg(def: &'static MsgDef, json: &Value, out: &mut Vec<u8>) -> Result<(), GenError> {
    if !json.is_object() {
        return Err(GenError::Invalid(format!(
            "{}: expected an object, got {json}",
            def.name
        )));
    }
    for field in def.fields {
        if let Some(oneof_name) = field.oneof {
            let Some(sel) = json.get(oneof_name) else {
                continue;
            };
            if sel.get("$case").and_then(|c| c.as_str()) != Some(field.json_name) {
                continue;
            }
            let Some(v) = sel.get(field.json_name) else {
                continue;
            };
            // a selected oneof member is always written, even at its default
            write_value(def, field, v, out)?;
            continue;
        }
        if field.repeated {
            let arr = json.get(field.json_name).and_then(|v| v.as_array());
            if field.packed {
                // ts-proto always writes the tag + length for packed fields,
                // even for an empty (or absent) array
                let mut payload = Vec::new();
                for item in arr.into_iter().flatten() {
                    write_scalar_raw(def, field, item, &mut payload)?;
                }
                len_delimited(out, field.number, &payload);
            } else {
                for item in arr.into_iter().flatten() {
                    write_value(def, field, item, out)?;
                }
            }
            continue;
        }
        let Some(v) = json.get(field.json_name) else {
            continue;
        };
        if v.is_null() {
            continue;
        }
        if matches!(field.kind, FieldKind::Msg(_)) || field.optional || !is_default(field.kind, v) {
            write_value(def, field, v, out)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Tmp(std::path::PathBuf);

    impl Tmp {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("dcl-one-sdk-crdtgen-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Tmp(dir)
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn opera_fixture_is_byte_identical_to_the_upstream_toolchain() {
        let tmp = Tmp::new("opera");
        std::fs::create_dir_all(tmp.0.join("assets/scene")).unwrap();
        std::fs::write(
            tmp.0.join("assets/scene/main.composite"),
            include_str!("../testdata/opera-main.composite"),
        )
        .unwrap();
        let generated = generate(&tmp.0).unwrap().unwrap();
        assert_eq!(generated.composites, 1);
        let expected: &[u8] = include_bytes!("../testdata/opera-main.crdt");
        assert_eq!(
            generated.bytes, expected,
            "native main.crdt must match the @dcl/inspector-generated reference"
        );
    }

    #[test]
    fn no_composites_yields_none() {
        let tmp = Tmp::new("empty");
        assert!(generate(&tmp.0).unwrap().is_none());
    }

    #[test]
    fn custom_jsonschema_components_are_unsupported() {
        let tmp = Tmp::new("custom");
        std::fs::write(
            tmp.0.join("main.composite"),
            json!({
                "version": 1,
                "components": [{
                    "name": "inspector::Scene",
                    "jsonSchema": { "type": "object" },
                    "data": { "0": { "json": {} } }
                }]
            })
            .to_string(),
        )
        .unwrap();
        match generate(&tmp.0) {
            Err(GenError::Unsupported(why)) => assert!(why.contains("inspector::Scene")),
            other => panic!("expected Unsupported, got {:?}", other.map(|_| ())),
        }
    }

    fn encode_by_name(name: &str, json: &Value) -> Vec<u8> {
        encode_component(name, json).unwrap().1
    }

    #[test]
    fn transform_serializes_the_fixed_44_byte_layout_with_defaults() {
        let bytes = encode_by_name("core::Transform", &json!({}));
        assert_eq!(bytes.len(), 44);
        let f = |i: usize| f32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
        assert_eq!(
            (f(0), f(12), f(24), f(28)),
            (0.0, 0.0, 1.0, 1.0),
            "position 0, rotation identity (w at offset 24), scale 1"
        );
    }

    #[test]
    fn empty_packed_arrays_still_write_their_tag() {
        // PBMeshRenderer { mesh: box { uvs: [] } } → 0a 02 0a 00
        let bytes = encode_by_name(
            "core::MeshRenderer",
            &json!({ "mesh": { "$case": "box", "box": { "uvs": [] } } }),
        );
        assert_eq!(bytes, vec![0x0a, 0x02, 0x0a, 0x00]);
    }

    #[test]
    fn optional_scalars_write_even_at_zero_and_plain_scalars_skip_defaults() {
        // PBMeshCollider { collisionMask (optional) = 0, box } → 08 00 12 00
        let bytes = encode_by_name(
            "core::MeshCollider",
            &json!({ "mesh": { "$case": "box", "box": {} }, "collisionMask": 0 }),
        );
        assert_eq!(bytes, vec![0x08, 0x00, 0x12, 0x00]);
        // absent optional writes nothing
        let bytes = encode_by_name(
            "core::MeshCollider",
            &json!({ "mesh": { "$case": "box", "box": {} } }),
        );
        assert_eq!(bytes, vec![0x12, 0x00]);
    }

    #[test]
    fn repeated_strings_write_nothing_when_empty() {
        let bytes = encode_by_name(
            "core::AvatarModifierArea",
            &json!({
                "area": { "x": 14.0, "y": 6.0, "z": 14.0 },
                "excludeIds": [],
                "modifiers": [0, 1]
            }),
        );
        let expected: Vec<u8> = vec![
            0x0a, 0x0f, // area, 15 bytes
            0x0d, 0x00, 0x00, 0x60, 0x41, // x = 14.0
            0x15, 0x00, 0x00, 0xc0, 0x40, // y = 6.0
            0x1d, 0x00, 0x00, 0x60, 0x41, // z = 14.0
            0x1a, 0x02, 0x00, 0x01, // modifiers packed [0, 1]
        ];
        assert_eq!(bytes, expected);
    }

    #[test]
    fn later_composites_override_earlier_entities() {
        let tmp = Tmp::new("override");
        let transform = |y: f64| {
            json!({
                "version": 1,
                "components": [{
                    "name": "core::Transform",
                    "data": { "512": { "json": { "position": { "x": 0, "y": y, "z": 0 } } } }
                }]
            })
        };
        std::fs::write(tmp.0.join("a.composite"), transform(1.0).to_string()).unwrap();
        std::fs::write(tmp.0.join("b.composite"), transform(2.0).to_string()).unwrap();
        let generated = generate(&tmp.0).unwrap().unwrap();
        assert_eq!(generated.composites, 2);
        // one PUT for entity 512; y from b.composite (sorted after a.composite)
        assert_eq!(generated.bytes.len(), 24 + 44);
        let y = f32::from_le_bytes(generated.bytes[24 + 4..24 + 8].try_into().unwrap());
        assert_eq!(y, 2.0);
    }
}
