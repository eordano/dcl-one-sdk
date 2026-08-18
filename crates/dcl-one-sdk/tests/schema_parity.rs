//! Differential test: the Rust ISchema encoder against @dcl/ecs itself.
//!
//! The three scene goldens pin the constructs those scenes happen to use. This
//! covers the rest — and, more importantly, the classes of bug that are silent
//! rather than loud: property order (a wrong order is the same length and still
//! parses), optional truthiness, numeric coercions, and the one-of index base.
//! Schemas are emitted as raw text with a deliberately non-alphabetical
//! property order, because that is exactly what an ordering regression would
//! hide behind.
//!
//! Skipped when node is unavailable; @dcl/ecs comes from the crate's own
//! vendored node_modules, so the test does not need a scene installed.

use dcl_one_sdk::schema_crdt::{compile, encode_component_value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

const CASES: usize = 600;

fn main_seed() -> u64 {
    std::env::var("DCL_ONE_SCHEMA_FUZZ_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0x5eed_1234_abcd_0001)
}

// ---- the generated schema shape ------------------------------------------

#[derive(Clone, Debug)]
enum Gen {
    Map(Vec<(String, Gen)>),
    Array(Box<Gen>),
    Optional(Box<Gen>),
    OneOf(Vec<(String, Gen)>),
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
    EnumStr(Option<&'static str>),
}

impl Gen {
    /// Render as JSON text. Not via serde_json: its map is a BTreeMap, and
    /// alphabetising the properties would erase the property order this test
    /// exists to check.
    fn json(&self) -> String {
        match self {
            Gen::Map(props) => format!(
                r#"{{"type":"object","serializationType":"map","properties":{}}}"#,
                object(props)
            ),
            Gen::OneOf(props) => format!(
                r#"{{"type":"object","serializationType":"one-of","properties":{}}}"#,
                object(props)
            ),
            Gen::Array(items) => format!(
                r#"{{"type":"array","serializationType":"array","items":{}}}"#,
                items.json()
            ),
            Gen::Optional(inner) => format!(
                r#"{{"type":"object","serializationType":"optional","optionalJsonSchema":{}}}"#,
                inner.json()
            ),
            Gen::Str => primitive("string", "utf8-string"),
            Gen::Bool => primitive("boolean", "boolean"),
            Gen::Int8 => primitive("integer", "int8"),
            Gen::Int16 => primitive("integer", "int16"),
            Gen::Int32 => primitive("integer", "int32"),
            Gen::Int64 => primitive("integer", "int64"),
            Gen::Entity => primitive("integer", "entity"),
            Gen::F32 => primitive("number", "float32"),
            Gen::F64 => primitive("number", "float64"),
            Gen::Vector3 => primitive("object", "vector3"),
            Gen::Quaternion => primitive("object", "quaternion"),
            Gen::Color3 => primitive("object", "color3"),
            Gen::Color4 => primitive("object", "color4"),
            Gen::EnumInt(default) => {
                // A TS numeric enum is reverse-mapped, and IntEnum validates
                // that it is (`totalCount !== valueCount * 2` throws).
                let head = r#"{"type":"integer","serializationType":"enum-int","enumObject":{"A":0,"B":1,"C":7,"0":"A","1":"B","7":"C"},"enum":[0,1,7]"#;
                match default {
                    Some(d) => format!(r#"{head},"default":{d}}}"#),
                    None => format!("{head}}}"),
                }
            }
            Gen::EnumStr(default) => {
                let head = r#"{"type":"string","serializationType":"enum-string","enumObject":{"A":"a","B":"bb"},"enum":["a","bb"]"#;
                match default {
                    Some(d) => format!(r#"{head},"default":"{d}"}}"#),
                    None => format!("{head}}}"),
                }
            }
        }
    }
}

fn primitive(ty: &str, serialization: &str) -> String {
    format!(r#"{{"type":"{ty}","serializationType":"{serialization}"}}"#)
}

fn object(props: &[(String, Gen)]) -> String {
    let body: Vec<String> = props
        .iter()
        .map(|(k, v)| format!(r#""{k}":{}"#, v.json()))
        .collect();
    format!("{{{}}}", body.join(","))
}

// ---- generation ----------------------------------------------------------

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

/// Deliberately unsorted, so a schema's declaration order differs from its
/// alphabetical order in most generated cases.
const FIELD_NAMES: [&str; 6] = ["zulu", "alpha", "mid", "Bravo", "yankee", "delta"];

fn gen_schema(rng: &mut Rng, depth: usize) -> Gen {
    let leaf = |rng: &mut Rng| -> Gen {
        match rng.below(15) {
            0 => Gen::Str,
            1 => Gen::Bool,
            2 => Gen::Int8,
            3 => Gen::Int16,
            4 => Gen::Int32,
            5 => Gen::Int64,
            6 => Gen::Entity,
            7 => Gen::F32,
            8 => Gen::F64,
            9 => Gen::Vector3,
            10 => Gen::Quaternion,
            11 => Gen::Color3,
            12 => Gen::Color4,
            13 => Gen::EnumInt(if rng.chance(70) {
                Some(*rng.pick(&[0i64, 1, 7]))
            } else {
                None
            }),
            _ => Gen::EnumStr(Some(*rng.pick(&["a", "bb"]))),
        }
    };
    if depth == 0 || rng.chance(35) {
        return leaf(rng);
    }
    match rng.below(4) {
        0 => Gen::Map(gen_props(rng, depth)),
        1 => Gen::Array(Box::new(gen_schema(rng, depth - 1))),
        2 => Gen::Optional(Box::new(gen_schema(rng, depth - 1))),
        _ => {
            // one-of needs at least two variants for the index to mean anything
            let mut props = gen_props(rng, depth);
            for name in FIELD_NAMES {
                if props.len() >= 2 {
                    break;
                }
                if !props.iter().any(|(k, _)| k == name) {
                    props.push((name.to_string(), leaf(rng)));
                }
            }
            Gen::OneOf(props)
        }
    }
}

fn gen_props(rng: &mut Rng, depth: usize) -> Vec<(String, Gen)> {
    let count = 1 + rng.below(4);
    let mut names: Vec<&str> = FIELD_NAMES.to_vec();
    // Fisher-Yates, so the declaration order is not the constant order above
    for i in (1..names.len()).rev() {
        names.swap(i, rng.below(i + 1));
    }
    names
        .into_iter()
        .take(count)
        .map(|n| (n.to_string(), gen_schema(rng, depth - 1)))
        .collect()
}

/// A value for `schema`. Only well-typed values (or absent keys) are produced:
/// feeding a string field a number would exercise JS coercions that no editor
/// ever writes into a composite, and the point here is the layout, not the
/// coercion of nonsense.
fn gen_value(rng: &mut Rng, schema: &Gen) -> serde_json::Value {
    use serde_json::{json, Value};
    match schema {
        Gen::Map(props) => {
            let mut obj = serde_json::Map::new();
            for (k, sub) in props {
                // an omitted key is undefined below the top level, which is a
                // real shape: node throws for some types and coerces for others
                if rng.chance(20) {
                    continue;
                }
                obj.insert(k.clone(), gen_value(rng, sub));
            }
            Value::Object(obj)
        }
        Gen::Array(items) => {
            let n = rng.below(4);
            Value::Array((0..n).map(|_| gen_value(rng, items)).collect())
        }
        Gen::Optional(inner) => {
            if rng.chance(35) {
                // every JS falsy value, which must all encode as one 0x00 byte
                rng.pick(&[json!(null), json!(0), json!(-0.0), json!(""), json!(false)])
                    .clone()
            } else {
                gen_value(rng, inner)
            }
        }
        Gen::OneOf(props) => {
            let idx = rng.below(props.len());
            let (case, sub) = &props[idx];
            json!({ "$case": case, "value": gen_value(rng, sub) })
        }
        Gen::Str => Value::String(
            (*rng.pick(&["", "a", "h\u{e9}llo\u{2603}", "with \"quote\"", "0"])).to_string(),
        ),
        Gen::Bool => json!(rng.chance(50)),
        Gen::Int8 | Gen::Int16 | Gen::Int32 | Gen::Entity => {
            json!(*rng.pick(&[
                0i64,
                1,
                -1,
                127,
                128,
                -129,
                32768,
                2147483647,
                -2147483648,
                4294967296,
                8002
            ]))
        }
        Gen::Int64 => json!(*rng.pick(&[
            0i64,
            1,
            -1,
            99929642,
            4149214247,
            9007199254740993,
            -9007199254740993
        ])),
        Gen::F32 | Gen::F64 => json!(*rng.pick(&[
            0.0f64,
            -0.0,
            1.0,
            -1.5,
            0.1,
            85.0,
            3.4028235e38,
            1e-45,
            123456.789
        ])),
        Gen::Vector3 => floats(rng, &["x", "y", "z"]),
        Gen::Quaternion => floats(rng, &["x", "y", "z", "w"]),
        Gen::Color3 => floats(rng, &["r", "g", "b"]),
        Gen::Color4 => floats(rng, &["r", "g", "b", "a"]),
        Gen::EnumInt(_) => json!(*rng.pick(&[0i64, 1, 7])),
        Gen::EnumStr(_) => json!(*rng.pick(&["a", "bb"])),
    }
}

fn floats(rng: &mut Rng, keys: &[&str]) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    for k in keys {
        obj.insert(
            (*k).to_string(),
            serde_json::json!(*rng.pick(&[0.0f64, -0.0, 1.0, -2.25, 85.0, 0.1])),
        );
    }
    serde_json::Value::Object(obj)
}

// ---- the node side -------------------------------------------------------

const RUNNER: &str = r#"
const fs = require('fs')
const path = require('path')
const root = process.argv[2]
const { Schemas } = require(path.join(root, 'node_modules/@dcl/ecs/dist-cjs/schemas'))
const { ReadWriteByteBuffer } = require(path.join(root, 'node_modules/@dcl/ecs/dist-cjs/serialization/ByteBuffer'))
const out = []
for (const line of fs.readFileSync(process.argv[3], 'utf8').split('\n')) {
  if (!line) continue
  const testCase = JSON.parse(line)
  try {
    const schema = Schemas.fromJson(testCase.schema)
    // what componentDefinition.create(entity, value) does with a composite value
    const value = schema.extend ? schema.extend(testCase.value) : testCase.value
    const buffer = new ReadWriteByteBuffer()
    schema.serialize(value, buffer)
    out.push(Buffer.from(buffer.toBinary()).toString('hex'))
  } catch {
    out.push('ERR')
  }
}
fs.writeFileSync(process.argv[4], out.join('\n'))
"#;

fn vendored_ecs(dir: &Path) -> bool {
    let zip_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/vendor/node_modules.zip");
    let Ok(file) = std::fs::File::open(&zip_path) else {
        return false;
    };
    let Ok(mut archive) = zip::ZipArchive::new(file) else {
        return false;
    };
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).unwrap();
        let Some(name) = entry.enclosed_name() else {
            continue;
        };
        let wanted = name.starts_with("node_modules/@dcl/ecs")
            || name.starts_with("node_modules/@protobufjs");
        if !wanted || !entry.is_file() {
            continue;
        }
        let target = dir.join(&name);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        let mut sink = std::fs::File::create(&target).unwrap();
        std::io::copy(&mut entry, &mut sink).unwrap();
    }
    dir.join("node_modules/@dcl/ecs/dist-cjs/schemas/index.js")
        .is_file()
}

struct Tmp(PathBuf);

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn the_rust_encoder_agrees_with_dcl_ecs_on_generated_schemas() {
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("skipping: node is not on PATH");
        return;
    }
    let tmp =
        Tmp(std::env::temp_dir().join(format!("dcl-one-sdk-schema-parity-{}", std::process::id())));
    let _ = std::fs::remove_dir_all(&tmp.0);
    std::fs::create_dir_all(&tmp.0).unwrap();
    assert!(
        vendored_ecs(&tmp.0),
        "the vendored node_modules must carry @dcl/ecs for this test to mean anything"
    );

    let mut rng = Rng(main_seed());
    let mut cases = Vec::with_capacity(CASES);
    let mut input = std::fs::File::create(tmp.0.join("cases.ndjson")).unwrap();
    for _ in 0..CASES {
        // A component's top level is always a map, the one place `extend` runs.
        let schema = Gen::Map(gen_props(&mut rng, 3));
        let value = gen_value(&mut rng, &schema);
        let schema_text = schema.json();
        writeln!(
            input,
            r#"{{"schema":{},"value":{}}}"#,
            schema_text,
            serde_json::to_string(&value).unwrap()
        )
        .unwrap();
        cases.push((schema_text, value));
    }
    drop(input);

    std::fs::write(tmp.0.join("runner.js"), RUNNER).unwrap();
    let status = Command::new("node")
        .arg(tmp.0.join("runner.js"))
        .arg(&tmp.0)
        .arg(tmp.0.join("cases.ndjson"))
        .arg(tmp.0.join("out.txt"))
        .status()
        .expect("running the node oracle");
    assert!(status.success(), "the node oracle must run to completion");
    let expected = std::fs::read_to_string(tmp.0.join("out.txt")).unwrap();
    let expected: Vec<&str> = expected.split('\n').collect();
    assert_eq!(expected.len(), cases.len());

    let mut agreed_errors = 0;
    for (i, (schema_text, value)) in cases.iter().enumerate() {
        let js = dcl_one_sdk::jsjson::parse(schema_text).unwrap();
        let schema = compile(&js).unwrap_or_else(|e| panic!("case {i}: {schema_text}: {e}"));
        let ours = encode_component_value(&schema, value);
        match (&ours, expected[i]) {
            (Ok(bytes), "ERR") => panic!(
                "case {i}: @dcl/ecs refused to serialize this, we emitted {}\nschema: {schema_text}\nvalue: {value}",
                hex::encode(bytes)
            ),
            (Err(why), "ERR") => {
                let _ = why;
                agreed_errors += 1;
            }
            (Err(why), _) => panic!(
                "case {i}: @dcl/ecs serialized this, we refused ({why})\nschema: {schema_text}\nvalue: {value}"
            ),
            (Ok(bytes), theirs) => assert_eq!(
                hex::encode(bytes),
                theirs,
                "case {i}\nschema: {schema_text}\nvalue: {value}"
            ),
        }
    }
    // A run where everything throws would pass vacuously.
    assert!(
        agreed_errors * 2 < CASES,
        "{agreed_errors}/{CASES} cases threw on both sides; the generator is producing junk"
    );
}
