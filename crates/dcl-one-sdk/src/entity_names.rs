//! Native `assets/scene/entity-names.ts` generation from `.composite` files.
//!
//! Creator Hub rewrites this file on every save (`generateEntityNamesType`,
//! which lives behind the editor UI package the vendored inspector shim does
//! not implement) and Hub-authored scenes import it, so a build here must
//! regenerate it from the same composites as main.crdt or a renamed entity
//! silently desyncs the enum. Output is byte-identical to upstream's, trailing
//! `"} \n"` included, so a scene moving between the Hub and this toolchain
//! shows no diff.

use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

const NAME_COMPONENT: &str = "core-schema::Name";

pub const OUTPUT_PATH: &str = "assets/scene/entity-names.ts";

const HEADER: &str = "// Auto-generated entity names from the scene\n\n\n/**\n * Object containing all entity names in the scene for autocomplete support.\n */\nexport enum EntityNames {\n";

/// A TypeScript enum key for `name`, or None when nothing usable survives.
/// Upstream replaces every non-alphanumeric character with `_` one for one
/// (existing underscores are left alone) and prefixes `_` to a leading digit.
fn enum_key(name: &str) -> Option<String> {
    let mut key: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if key.chars().all(|c| c == '_') {
        return None;
    }
    if key.starts_with(|c: char| c.is_ascii_digit()) {
        key.insert(0, '_');
    }
    Some(key)
}

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Render the file for an already-collected key -> name map. Upstream emits
/// entries in ASCII order of the enum key, not entity order — hence a BTreeMap.
pub fn render(names: &BTreeMap<String, String>) -> String {
    let mut out = String::from(HEADER);
    for (key, value) in names {
        out.push_str(&format!("  {key} = \"{}\",\n", escape(value)));
    }
    out.push_str("} \n");
    out
}

/// Collect entity names from every composite under `root`. Later composites
/// win, matching how `crdt_gen` instances them; two entities that sanitize to
/// the same key collapse into one, the later entity winning.
pub fn collect(root: &Path) -> BTreeMap<String, String> {
    let mut names: BTreeMap<String, String> = BTreeMap::new();
    for file in crate::entrypoint::find_composites(root) {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let Ok(doc) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let Some(components) = doc.get("components").and_then(|c| c.as_array()) else {
            continue;
        };
        for comp in components {
            if comp.get("name").and_then(|n| n.as_str()) != Some(NAME_COMPONENT) {
                continue;
            }
            let Some(data) = comp.get("data").and_then(|d| d.as_object()) else {
                continue;
            };
            let mut entries: Vec<(u64, &Value)> = data
                .iter()
                .filter_map(|(k, v)| k.parse::<u64>().ok().map(|id| (id, v)))
                .collect();
            entries.sort_by_key(|(id, _)| *id);
            for (_, entry) in entries {
                let Some(value) = entry
                    .get("json")
                    .and_then(|j| j.get("value"))
                    .and_then(|v| v.as_str())
                else {
                    continue;
                };
                if value.is_empty() {
                    continue;
                }
                if let Some(key) = enum_key(value) {
                    names.insert(key, value.to_string());
                }
            }
        }
    }
    names
}

/// Regenerate `assets/scene/entity-names.ts` when the composites imply a
/// different file than the one on disk.
pub fn write_if_changed(root: &Path) -> std::io::Result<Option<usize>> {
    write(root, &collect(root))
}

/// Write the file for a map the caller already collected (`build` collects
/// first so it can number its steps). Returns the number of names written, or
/// None when no composite names anything — an existing file is then left
/// alone, since a scene may carry a hand-written one. Identical bytes are not
/// rewritten: `watch` rebuilds off mtimes, so touching the file every build
/// would loop.
pub fn write(root: &Path, names: &BTreeMap<String, String>) -> std::io::Result<Option<usize>> {
    if names.is_empty() {
        return Ok(None);
    }
    let rendered = render(names);
    let path = root.join(OUTPUT_PATH);
    if let Ok(existing) = std::fs::read_to_string(&path) {
        if existing == rendered {
            return Ok(Some(names.len()));
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, rendered)?;
    Ok(Some(names.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crdt_gen::TestDir;

    fn composite_with(names: &[(u32, &str)]) -> String {
        let data: Vec<String> = names
            .iter()
            .map(|(id, n)| format!(r#""{id}":{{"json":{{"value":"{n}"}}}}"#))
            .collect();
        format!(
            r#"{{"version":1,"components":[{{"name":"core-schema::Name","data":{{{}}}}}]}}"#,
            data.join(",")
        )
    }

    /// The gather scene's committed file, produced by Creator Hub itself. If
    /// this drifts, a Hub save and a build here will fight over the file.
    #[test]
    fn matches_creator_hub_output_byte_for_byte() {
        let expected = "// Auto-generated entity names from the scene\n\n\n/**\n * Object containing all entity names in the scene for autocomplete support.\n */\nexport enum EntityNames {\n  Admin_Tools = \"Admin Tools\",\n  Fixed_View_Camera = \"Fixed View Camera\",\n  Labyrinthia_Teleporter = \"Labyrinthia Teleporter\",\n  Video_Screen = \"Video Screen\",\n  base_theatre_glb = \"base_theatre.glb\",\n} \n";

        let tmp = TestDir::new("names-gather");
        tmp.write(
            "assets/scene/main.composite",
            composite_with(&[
                (513, "Admin Tools"),
                (514, "base_theatre.glb"),
                (515, "Fixed View Camera"),
                (516, "Video Screen"),
                (517, "Labyrinthia Teleporter"),
            ]),
        );

        assert_eq!(render(&collect(&tmp.0)), expected);
    }

    #[test]
    fn sanitises_keys_without_touching_values() {
        assert_eq!(enum_key("Admin Tools").as_deref(), Some("Admin_Tools"));
        assert_eq!(
            enum_key("base_theatre.glb").as_deref(),
            Some("base_theatre_glb")
        );
        assert_eq!(enum_key("2nd Floor").as_deref(), Some("_2nd_Floor"));
        assert_eq!(enum_key("---"), None);
    }

    #[test]
    fn escapes_quotes_in_the_value() {
        let mut names = BTreeMap::new();
        names.insert("The__Coil_".to_string(), "The \"Coil\"".to_string());
        assert!(render(&names).contains(r#"The__Coil_ = "The \"Coil\"","#));
    }

    #[test]
    fn no_names_leaves_an_existing_file_alone() {
        let tmp = TestDir::new("names-empty");
        tmp.write(
            "assets/scene/main.composite",
            r#"{"version":1,"components":[]}"#,
        );
        tmp.write("assets/scene/entity-names.ts", "hand written\n");
        assert_eq!(write_if_changed(&tmp.0).unwrap(), None);
        assert_eq!(
            std::fs::read_to_string(tmp.0.join(OUTPUT_PATH)).unwrap(),
            "hand written\n"
        );
    }

    #[test]
    fn rewrites_only_when_the_bytes_change() {
        let tmp = TestDir::new("names-stable");
        tmp.write(
            "assets/scene/main.composite",
            composite_with(&[(512, "Coil")]),
        );

        assert_eq!(write_if_changed(&tmp.0).unwrap(), Some(1));
        let path = tmp.0.join(OUTPUT_PATH);
        let first = std::fs::metadata(&path).unwrap().modified().unwrap();

        assert_eq!(write_if_changed(&tmp.0).unwrap(), Some(1));
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), first);
    }
}
