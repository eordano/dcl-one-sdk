//! Native `assets/scene/entity-names.ts` generation from `.composite` files.
//!
//! Creator Hub rewrites this file on every save so scene code can reach an
//! editor-placed entity by name rather than by a magic number:
//!
//! ```ts
//! import { EntityNames } from '../assets/scene/entity-names'
//! const screen = engine.getEntityOrNullByName(EntityNames.Video_Screen)
//! ```
//!
//! The vendored inspector shim declines to implement upstream's
//! `generateEntityNamesType` because it lives behind the editor UI package
//! (see `src/vendor/inspector-shim/index.js`). That was fine while nothing in
//! this toolchain read the file — but a Hub-authored scene imports it, so
//! building such a scene here left a stale file on disk, and renaming an entity
//! in the composite silently desynced the enum from the scene until someone
//! opened the Hub again. Generating it in Rust closes that loop: the same build
//! that regenerates main.crdt from the composites regenerates the names from
//! them too, so the two can never disagree.
//!
//! Output is byte-identical to upstream's, trailing `"} \n"` included, so a
//! scene that moves between the Hub and this toolchain shows no diff.

use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

/// The composite component Creator Hub stores entity names in.
const NAME_COMPONENT: &str = "core-schema::Name";

/// Where Creator Hub writes the file. Relative to the scene root.
pub const OUTPUT_PATH: &str = "assets/scene/entity-names.ts";

const HEADER: &str = "// Auto-generated entity names from the scene\n\n\n/**\n * Object containing all entity names in the scene for autocomplete support.\n */\nexport enum EntityNames {\n";

/// A TypeScript enum key for `name`, or None when nothing usable survives.
///
/// Upstream replaces every non-alphanumeric run character-for-character, so
/// "base_theatre.glb" becomes `base_theatre_glb` and "Admin Tools" becomes
/// `Admin_Tools`; underscores already present are left alone because `_` is
/// alphanumeric-adjacent for this purpose. A leading digit would not parse as
/// an enum key, so it gets an underscore in front of it.
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

/// Escape for a TypeScript double-quoted string literal. Entity names come from
/// a text field in the editor, so a quote or a backslash in one is a typo away.
fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Render the file for an already-collected key -> name map.
///
/// The map is a BTreeMap because upstream emits the entries in ASCII order of
/// the enum key, not in entity order — in the gather scene entity 514
/// (`base_theatre.glb`) sorts last, after entity 517.
pub fn render(names: &BTreeMap<String, String>) -> String {
    let mut out = String::from(HEADER);
    for (key, value) in names {
        out.push_str("  ");
        out.push_str(key);
        out.push_str(" = \"");
        out.push_str(&escape(value));
        out.push_str("\",\n");
    }
    out.push_str("} \n");
    out
}

/// Collect entity names from every composite under `root`.
///
/// Later composites win, matching how `crdt_gen` instances them. Two entities
/// that sanitize to the same key collapse into one; the later entity wins, so
/// the result stays a function of the composites rather than of iteration luck.
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
///
/// Returns the number of names written, or None when the scene has no composite
/// naming anything — in which case an existing file is left alone rather than
/// truncated, because a scene may carry a hand-written one.
///
/// Only writing on a real change keeps the file's mtime stable, which matters:
/// `watch` triggers rebuilds off mtimes, and rewriting identical bytes every
/// build would have the scene rebuild itself in a loop.
pub fn write_if_changed(root: &Path) -> std::io::Result<Option<usize>> {
    write(root, &collect(root))
}

/// `write_if_changed` for a map the caller already collected.
///
/// `build` needs to know whether this step will report BEFORE it numbers the
/// steps — "[6/5]" is not a step count — so it collects first and hands the
/// result over rather than paying for the composite parse twice.
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

    struct Tmp(std::path::PathBuf);

    impl Tmp {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "dcl-one-sdk-entity-names-{tag}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(dir.join("assets/scene")).unwrap();
            Tmp(dir)
        }
        fn write(&self, rel: &str, contents: &str) {
            let p = self.0.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, contents).unwrap();
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

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

        let tmp = Tmp::new("gather");
        tmp.write(
            "assets/scene/main.composite",
            &composite_with(&[
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
        let tmp = Tmp::new("empty");
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
        let tmp = Tmp::new("stable");
        tmp.write(
            "assets/scene/main.composite",
            &composite_with(&[(512, "Coil")]),
        );

        assert_eq!(write_if_changed(&tmp.0).unwrap(), Some(1));
        let path = tmp.0.join(OUTPUT_PATH);
        let first = std::fs::metadata(&path).unwrap().modified().unwrap();

        assert_eq!(write_if_changed(&tmp.0).unwrap(), Some(1));
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), first);
    }
}
