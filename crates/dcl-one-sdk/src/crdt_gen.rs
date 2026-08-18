//! Native `main.crdt` generation from `.composite` files.
//!
//! Replaces the node + `@dcl/inspector` fallback in the build path: composites
//! are parsed in Rust and instanced into PUT_COMPONENT messages byte-identical
//! to the upstream toolchain's.
//!
//! A composite needs *two* serializers, because @dcl/ecs has two: core
//! components (`core::Foo`) are protobuf, encoded here against the vendored
//! @dcl/protocol descriptors (see build.rs); everything else is encoded by
//! [`crate::schema_crdt`] against the `jsonSchema` the composite carries.
//!
//! ts-proto writer semantics reproduced deliberately (verified against
//! main.crdt files produced by @dcl/ecs 7.24.5):
//! - packed numeric repeated fields always write their tag + length, even when
//!   empty (`BoxMesh.uvs = []` encodes as `0a 00`)
//! - repeated string/bytes/message fields write nothing when empty
//! - non-`optional` scalars are skipped at their proto3 default
//! - `optional` (explicit-presence) scalars are written whenever the composite
//!   provides them, even at the default value
//! - oneof members (`{"$case": "box", "box": …}`) are always written when
//!   selected, even when empty
//! - fields encode in field-number order

use crate::jsjson::JsValue;
use crate::schema_crdt::{self, Schema, SchemaError};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
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

/// The whole proto3 scalar set. The vendored descriptors happen to use none of
/// the sint/fixed/sfixed variants today, hence the allow.
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
    /// Emitted by build.rs for readability of the generated table; never read.
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
const COMPOSITE_ROOT: &str = "composite::root";

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
    pub composites: u64,
    pub bytes: Vec<u8>,
}

/// Instance every `.composite` under `root` (direct entity mapping, later files
/// override earlier ones per entity+component) as main.crdt PUT_COMPONENT
/// messages. None when the scene has no composites.
pub fn generate(root: &Path) -> Result<Option<Generated>, GenError> {
    let files = crate::entrypoint::find_composites(root);
    if files.is_empty() {
        return Ok(None);
    }
    let mut order: Vec<String> = Vec::new();
    let mut comps: BTreeMap<String, BTreeMap<u32, Value>> = BTreeMap::new();
    let mut schemas: HashMap<String, Schema> = HashMap::new();
    for file in &files {
        let text = std::fs::read_to_string(file)
            .map_err(|e| GenError::Invalid(format!("reading {}: {e}", file.display())))?;
        let doc: Value = serde_json::from_str(&text)
            .map_err(|e| GenError::Invalid(format!("{}: {e}", file.display())))?;
        let ordered = crate::jsjson::parse(&text)
            .map_err(|e| GenError::Invalid(format!("{}: {e}", file.display())))?;
        let ordered_components: &[JsValue] = match ordered.get("components") {
            Some(JsValue::Array(items)) => items,
            _ => &[],
        };
        let components = doc
            .get("components")
            .and_then(|c| c.as_array())
            .ok_or_else(|| GenError::Invalid(format!("{}: no components array", file.display())))?;
        for (index, comp) in components.iter().enumerate() {
            let name = comp.get("name").and_then(|n| n.as_str()).ok_or_else(|| {
                GenError::Invalid(format!("{}: component without a name", file.display()))
            })?;
            if name == COMPOSITE_ROOT {
                if references_nested_composite(comp) {
                    return Err(GenError::Unsupported(format!(
                        "{} instances a nested composite",
                        file.display()
                    )));
                }
                continue;
            }
            if name != "core::Transform" && component_by_name(name).is_none() {
                register_schema(file, name, index, ordered_components, &mut schemas)?;
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
            let (id, data) = encode_component(name, json, &schemas).map_err(|e| match e {
                GenError::Invalid(why) => {
                    GenError::Invalid(format!("'{name}' entity {entity}: {why}"))
                }
                other => other,
            })?;
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

/// A `composite::root` entry naming another composite means the scene needs
/// nested instancing, which the native path does not do.
///
/// A `binary` entry counts: only the `json` form spells `src` out, but node
/// deserializes the binary one and instances it just the same, so reading only
/// `json.src` would let a binary root through and emit a scene quietly missing
/// everything the nested composite contributes.
fn references_nested_composite(comp: &Value) -> bool {
    comp.get("data")
        .and_then(|d| d.as_object())
        .into_iter()
        .flatten()
        .any(|(_, entry)| match entry.get("json") {
            Some(json) => json
                .get("src")
                .and_then(|s| s.as_str())
                .is_some_and(|s| !s.is_empty()),
            None => true,
        })
}

/// Compile the jsonSchema a composite carries for a component the static
/// protobuf table does not cover. The first composite to declare a name wins,
/// matching `engine.getComponentOrNull` short-circuiting every later one.
fn register_schema(
    file: &Path,
    name: &str,
    index: usize,
    ordered: &[JsValue],
    schemas: &mut HashMap<String, Schema>,
) -> Result<(), GenError> {
    if schemas.contains_key(name) {
        return Ok(());
    }
    if name.starts_with("core::") {
        return Err(GenError::Unsupported(format!(
            "core component '{name}' is missing from the vendored @dcl/protocol descriptors"
        )));
    }
    let json_schema = ordered
        .get(index)
        .filter(|c| c.get("name").and_then(JsValue::as_str) == Some(name))
        .and_then(|c| c.get("jsonSchema"))
        .ok_or_else(|| {
            GenError::Unsupported(format!(
                "{}: component '{name}' has no jsonSchema to define it from",
                file.display()
            ))
        })?;
    let schema = schema_crdt::compile(json_schema).map_err(|e| match e {
        SchemaError::Unsupported(why) => {
            GenError::Unsupported(format!("component '{name}': {why}"))
        }
        SchemaError::Invalid(why) => {
            GenError::Invalid(format!("{}: '{name}': {why}", file.display()))
        }
    })?;
    schemas.insert(name.to_string(), schema);
    Ok(())
}

fn encode_component(
    name: &str,
    json: &Value,
    schemas: &HashMap<String, Schema>,
) -> Result<(u32, Vec<u8>), GenError> {
    if name == "core::Transform" {
        return Ok((TRANSFORM_COMPONENT_ID, encode_transform(json)));
    }
    if let Some(comp) = component_by_name(name) {
        let mut out = Vec::new();
        encode_msg(&MESSAGES[comp.msg], json, &mut out)?;
        return Ok((comp.id, out));
    }
    let schema = schemas
        .get(name)
        .ok_or_else(|| GenError::Unsupported(format!("unknown component '{name}'")))?;
    let bytes = schema_crdt::encode_component_value(schema, json).map_err(|e| match e {
        SchemaError::Unsupported(why) | SchemaError::Invalid(why) => {
            GenError::Unsupported(format!("'{name}': {why}"))
        }
    })?;
    Ok((schema_crdt::component_number_from_name(name), bytes))
}

/// Describe the first difference between two main.crdt streams in terms
/// somebody can act on. A byte offset into the file is unactionable: the file is
/// a run of length-prefixed messages, so one extra byte early renames every
/// offset after it.
pub fn describe_difference(native: &[u8], reference: &[u8], root: &Path) -> Option<String> {
    if native == reference {
        return None;
    }
    let names = component_names(root);
    let name_of = |id: u32| -> String {
        names
            .get(&id)
            .cloned()
            .unwrap_or_else(|| format!("<unknown component {id}>"))
    };
    let (ours, theirs) = (parse_messages(native), parse_messages(reference));
    for (i, (a, b)) in ours.iter().zip(theirs.iter()).enumerate() {
        if a.0 != b.0 || a.1 != b.1 {
            return Some(format!(
                "message {i}: native has {} on entity {}, node has {} on entity {}",
                name_of(a.1),
                a.0,
                name_of(b.1),
                b.0
            ));
        }
        if a.2 == b.2 {
            continue;
        }
        let offset =
            a.2.iter()
                .zip(b.2.iter())
                .position(|(x, y)| x != y)
                .unwrap_or_else(|| a.2.len().min(b.2.len()));
        return Some(format!(
            "{} on entity {}: payload differs at byte {offset} of {}/{} (native/node); \
             run scripts/crdt-diff.py for the schema field path",
            name_of(a.1),
            a.0,
            a.2.len(),
            b.2.len()
        ));
    }
    Some(format!(
        "message counts differ: native wrote {}, node wrote {}",
        ours.len(),
        theirs.len()
    ))
}

fn parse_messages(bytes: &[u8]) -> Vec<(u32, u32, &[u8])> {
    let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let mut out = Vec::new();
    let mut offset = 0usize;
    while offset + CRDT_HEADER_LEN as usize <= bytes.len() {
        let length = word(offset) as usize;
        let payload_len = word(offset + 20) as usize;
        let start = offset + CRDT_HEADER_LEN as usize;
        if length < CRDT_HEADER_LEN as usize || offset + length > bytes.len() {
            break;
        }
        out.push((
            word(offset + 8),
            word(offset + 12),
            &bytes[start..(start + payload_len).min(bytes.len())],
        ));
        offset += length;
    }
    out
}

/// Every component id the scene can possibly emit, so a diff can name them.
fn component_names(root: &Path) -> HashMap<u32, String> {
    let mut out: HashMap<u32, String> = COMPONENTS
        .iter()
        .map(|c| (c.id, c.name.to_string()))
        .collect();
    out.insert(TRANSFORM_COMPONENT_ID, "core::Transform".to_string());
    for file in crate::entrypoint::find_composites(root) {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let Ok(doc) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        for comp in doc
            .get("components")
            .and_then(|c| c.as_array())
            .into_iter()
            .flatten()
        {
            if let Some(name) = comp.get("name").and_then(|n| n.as_str()) {
                out.entry(schema_crdt::component_number_from_name(name))
                    .or_insert_with(|| name.to_string());
            }
        }
    }
    out
}

fn put_component(out: &mut Vec<u8>, entity: u32, component: u32, data: &[u8]) {
    let words = [
        CRDT_HEADER_LEN + data.len() as u32,
        CRDT_PUT_COMPONENT,
        entity,
        component,
        0,
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

fn field_error(msg: &MsgDef, field: &FieldDef, what: &str) -> GenError {
    GenError::Invalid(format!("{}.{}: {what}", msg.name, field.json_name))
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
fn is_proto3_default(kind: FieldKind, v: &Value) -> bool {
    match kind {
        FieldKind::Bool => v.as_bool() == Some(false),
        FieldKind::Str => v.as_str() == Some(""),
        FieldKind::Bytes => bytes_value(v).is_some_and(|b| b.is_empty()),
        FieldKind::Enum(e) => enum_number(&ENUMS[e], v) == Some(0),
        FieldKind::Msg(_) => false,
        _ => v.as_f64() == Some(0.0),
    }
}

/// Write one scalar without its tag — the packed-array element form.
fn write_untagged_scalar(
    msg: &MsgDef,
    field: &FieldDef,
    v: &Value,
    out: &mut Vec<u8>,
) -> Result<(), GenError> {
    let want = |what: &'static str| field_error(msg, field, what);
    let f = || v.as_f64().ok_or_else(|| want("expected a number"));
    let i = || json_i64(v).ok_or_else(|| want("expected a number"));
    let u = || json_u64(v).ok_or_else(|| want("expected a number"));
    match field.kind {
        FieldKind::Double => out.extend_from_slice(&f()?.to_le_bytes()),
        FieldKind::Float => out.extend_from_slice(&(f()? as f32).to_le_bytes()),
        FieldKind::Fixed32 => out.extend_from_slice(&(u()? as u32).to_le_bytes()),
        FieldKind::Sfixed32 => out.extend_from_slice(&(i()? as i32).to_le_bytes()),
        FieldKind::Fixed64 => out.extend_from_slice(&u()?.to_le_bytes()),
        FieldKind::Sfixed64 => out.extend_from_slice(&i()?.to_le_bytes()),
        FieldKind::Bool => varint(
            out,
            v.as_bool().ok_or_else(|| want("expected a bool"))? as u64,
        ),
        FieldKind::Int32 | FieldKind::Int64 => varint(out, i()? as u64),
        FieldKind::Uint32 | FieldKind::Uint64 => varint(out, u()?),
        FieldKind::Sint32 => varint(out, zigzag32(i()?)),
        FieldKind::Sint64 => varint(out, zigzag64(i()?)),
        FieldKind::Enum(e) => varint(
            out,
            enum_number(&ENUMS[e], v).ok_or_else(|| want("unknown enum value"))? as u64,
        ),
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

fn write_tagged_field(
    msg: &MsgDef,
    field: &FieldDef,
    v: &Value,
    out: &mut Vec<u8>,
) -> Result<(), GenError> {
    match field.kind {
        FieldKind::Str => {
            let s = v
                .as_str()
                .ok_or_else(|| field_error(msg, field, "expected a string"))?;
            len_delimited(out, field.number, s.as_bytes());
        }
        FieldKind::Bytes => {
            let b = bytes_value(v).ok_or_else(|| field_error(msg, field, "expected bytes"))?;
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
            write_untagged_scalar(msg, field, v, out)?;
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
        .ok_or_else(|| field_error(msg, field, "expected an object for a map field"))?;
    let (key_field, val_field) = (&entry_def.fields[0], &entry_def.fields[1]);
    for (k, val) in obj {
        let mut entry = Vec::new();
        let key_json = match key_field.kind {
            FieldKind::Str => Value::String(k.clone()),
            _ => serde_json::from_str(k)
                .map_err(|_| field_error(msg, field, "non-numeric key for a numeric map"))?,
        };
        if !is_proto3_default(key_field.kind, &key_json) {
            write_tagged_field(entry_def, key_field, &key_json, &mut entry)?;
        }
        if !is_proto3_default(val_field.kind, val) {
            write_tagged_field(entry_def, val_field, val, &mut entry)?;
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
            write_tagged_field(def, field, v, out)?;
            continue;
        }
        if field.repeated {
            let arr = json.get(field.json_name).and_then(|v| v.as_array());
            if field.packed {
                let mut payload = Vec::new();
                for item in arr.into_iter().flatten() {
                    write_untagged_scalar(def, field, item, &mut payload)?;
                }
                len_delimited(out, field.number, &payload);
            } else {
                for item in arr.into_iter().flatten() {
                    write_tagged_field(def, field, item, out)?;
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
        if matches!(field.kind, FieldKind::Msg(_))
            || field.optional
            || !is_proto3_default(field.kind, v)
        {
            write_tagged_field(def, field, v, out)?;
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

    /// Every golden here was regenerated through the node data-layer
    /// (`data-layer-host.mjs <scene> dump-crdt`, @dcl/ecs 7.26.0) at the moment
    /// it was committed — a scene's checked-in main.crdt is not trustworthy as a
    /// reference, it can predate its own composite.
    #[test]
    fn real_scene_fixtures_are_byte_identical_to_the_upstream_toolchain() {
        let cases: [(&str, &str, &[u8]); 4] = [
            (
                "opera",
                include_str!("../testdata/opera-main.composite"),
                include_bytes!("../testdata/opera-main.crdt"),
            ),
            (
                "gather",
                include_str!("../testdata/gather-main.composite"),
                include_bytes!("../testdata/gather-main.crdt"),
            ),
            (
                "gather2",
                include_str!("../testdata/gather2-main.composite"),
                include_bytes!("../testdata/gather2-main.crdt"),
            ),
            (
                "museum",
                include_str!("../testdata/museum-main.composite"),
                include_bytes!("../testdata/museum-main.crdt"),
            ),
        ];
        for (name, composite, expected) in cases {
            let tmp = Tmp::new(name);
            std::fs::create_dir_all(tmp.0.join("assets/scene")).unwrap();
            std::fs::write(tmp.0.join("assets/scene/main.composite"), composite).unwrap();
            let generated = generate(&tmp.0).unwrap().unwrap();
            assert_eq!(generated.composites, 1);
            assert_eq!(
                generated.bytes, expected,
                "native main.crdt for '{name}' must match the node data-layer's"
            );
        }
    }

    #[test]
    fn no_composites_yields_none() {
        let tmp = Tmp::new("empty");
        assert!(generate(&tmp.0).unwrap().is_none());
    }

    fn generate_from(tag: &str, composite: Value) -> Result<Option<Generated>, GenError> {
        let tmp = Tmp::new(tag);
        std::fs::write(tmp.0.join("main.composite"), composite.to_string()).unwrap();
        generate(&tmp.0)
    }

    #[test]
    fn a_custom_jsonschema_component_encodes_against_its_own_schema() {
        let generated = generate_from(
            "custom",
            json!({
                "version": 1,
                "components": [{
                    "name": "core-schema::Name",
                    "jsonSchema": {
                        "type": "object",
                        "properties": {
                            "value": { "type": "string", "serializationType": "utf8-string" }
                        },
                        "serializationType": "map"
                    },
                    "data": { "513": { "json": { "value": "Admin Tools" } } }
                }]
            }),
        )
        .unwrap()
        .unwrap();
        let mut expected = Vec::new();
        let mut payload = vec![0x0b, 0, 0, 0];
        payload.extend_from_slice(b"Admin Tools");
        put_component(&mut expected, 513, 3864921337, &payload);
        assert_eq!(generated.bytes, expected);
    }

    #[test]
    fn a_component_with_neither_a_schema_nor_a_descriptor_stays_unsupported() {
        match generate_from(
            "no-schema",
            json!({
                "version": 1,
                "components": [{
                    "name": "inspector::Scene",
                    "data": { "0": { "json": {} } }
                }]
            }),
        ) {
            Err(GenError::Unsupported(why)) => assert!(why.contains("inspector::Scene")),
            other => panic!("expected Unsupported, got {:?}", other.map(|_| ())),
        }
    }

    #[test]
    fn an_unknown_core_component_points_at_the_vendored_descriptors() {
        match generate_from(
            "unknown-core",
            json!({
                "version": 1,
                "components": [{
                    "name": "core::BrandNew",
                    "jsonSchema": {
                        "serializationType": "protocol-buffer",
                        "protocolBuffer": "PBBrandNew"
                    },
                    "data": { "512": { "json": {} } }
                }]
            }),
        ) {
            Err(GenError::Unsupported(why)) => assert!(why.contains("@dcl/protocol")),
            other => panic!("expected Unsupported, got {:?}", other.map(|_| ())),
        }
    }

    #[test]
    fn composite_root_is_skipped_and_nested_composites_are_not_instanced() {
        let root = |src: &str| {
            json!({
                "version": 1,
                "components": [{
                    "name": "composite::root",
                    "jsonSchema": {
                        "type": "object",
                        "serializationType": "map",
                        "properties": {
                            "src": { "type": "string", "serializationType": "utf8-string" }
                        }
                    },
                    "data": { "512": { "json": { "src": src } } }
                }]
            })
        };
        assert!(generate_from("root-empty", root(""))
            .unwrap()
            .unwrap()
            .bytes
            .is_empty());
        match generate_from("root-nested", root("other.composite")) {
            Err(GenError::Unsupported(why)) => assert!(why.contains("nested composite")),
            other => panic!("expected Unsupported, got {:?}", other.map(|_| ())),
        }
    }

    fn encode_by_name(name: &str, json: &Value) -> Vec<u8> {
        encode_component(name, json, &HashMap::new()).unwrap().1
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
        let bytes = encode_by_name(
            "core::MeshRenderer",
            &json!({ "mesh": { "$case": "box", "box": { "uvs": [] } } }),
        );
        assert_eq!(bytes, vec![0x0a, 0x02, 0x0a, 0x00]);
    }

    #[test]
    fn optional_scalars_write_even_at_zero_and_plain_scalars_skip_defaults() {
        let bytes = encode_by_name(
            "core::MeshCollider",
            &json!({ "mesh": { "$case": "box", "box": {} }, "collisionMask": 0 }),
        );
        assert_eq!(bytes, vec![0x08, 0x00, 0x12, 0x00]);
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
            0x0a, 0x0f, 0x0d, 0x00, 0x00, 0x60, 0x41, 0x15, 0x00, 0x00, 0xc0, 0x40, 0x1d, 0x00,
            0x00, 0x60, 0x41, 0x1a, 0x02, 0x00, 0x01,
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
        assert_eq!(generated.bytes.len(), 24 + 44);
        let y = f32::from_le_bytes(generated.bytes[24 + 4..24 + 8].try_into().unwrap());
        assert_eq!(y, 2.0);
    }
}
