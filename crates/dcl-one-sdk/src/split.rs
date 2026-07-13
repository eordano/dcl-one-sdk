use crate::scene::Project;
use anyhow::{Context, Result};
use std::path::Path;

const LOADER_TEMPLATE: &str = include_str!("templates/split-loader.js");
const LOADER_MARKER: &str = "__dclOneSdkChunkPath";
const MARKER_FILE: &str = "split";

const REGISTRY_KEYS: &[&str] = &[
    "@dcl/sdk",
    "@dcl/sdk/ecs",
    "@dcl/sdk/math",
    "@dcl/sdk/react-ecs",
    "@dcl/sdk/composite-provider",
    "@dcl/sdk/observables",
    "@dcl/sdk/message-bus",
    "@dcl/sdk/players",
    "@dcl/sdk/network",
    "@dcl/sdk/ethereum-provider",
    "@dcl/sdk/testing",
    "@dcl/sdk/internal/Observable",
    "@dcl/ecs",
    "@dcl/ecs/dist/components",
    "@dcl/ecs/dist/components/component-number",
    "@dcl/ecs/dist/serialization/ByteBuffer",
    "@dcl/ecs/dist/systems/crdt",
    "@dcl/ecs-math",
    "@dcl/ecs-math/dist/Matrix",
    "@dcl/ecs-math/dist/Plane",
    "@dcl/react-ecs",
    "react",
    "~sdk/all-composites",
    "~sdk/script-utils",
];

pub fn has_asset_packs(project: &Project) -> bool {
    project.node_module("@dcl/asset-packs").is_some()
        || project
            .node_module("@dcl/inspector/node_modules/@dcl/asset-packs")
            .is_some()
}

fn has_jsx_runtime(project: &Project) -> bool {
    project.node_module("react/jsx-runtime.js").is_some()
        || project
            .node_module("@dcl/react-ecs/node_modules/react/jsx-runtime.js")
            .is_some()
}

pub fn registry_keys(project: &Project) -> Vec<&'static str> {
    let mut keys: Vec<&'static str> = REGISTRY_KEYS.to_vec();
    if has_asset_packs(project) {
        keys.push("@dcl/asset-packs");
        keys.push("@dcl/asset-packs/dist/scene-entrypoint");
    }
    if has_jsx_runtime(project) {
        keys.push("react/jsx-runtime");
    }
    keys
}

pub fn scene_externals(project: &Project) -> Vec<String> {
    let mut externals: Vec<String> = [
        "@dcl/sdk",
        "@dcl/sdk/*",
        "@dcl/ecs",
        "@dcl/ecs/*",
        "@dcl/ecs-math",
        "@dcl/ecs-math/*",
        "@dcl/react-ecs",
        "@dcl/react-ecs/*",
        "react",
        "react/*",
        "~sdk/all-composites",
        "~sdk/script-utils",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    if has_asset_packs(project) {
        externals.push("@dcl/asset-packs".to_string());
        externals.push("@dcl/asset-packs/*".to_string());
    }
    externals
}

pub fn write_generated(project: &Project, dir: &Path) -> Result<()> {
    let slot = dir.join("composite-slot.js");
    std::fs::write(&slot, "export const compositeFromLoader = {}\n")
        .with_context(|| format!("writing {}", slot.display()))?;
    let entry = dir.join("sdk-runtime-entry.js");
    std::fs::write(&entry, sdk_runtime_entry(project))
        .with_context(|| format!("writing {}", entry.display()))?;
    Ok(())
}

fn sdk_runtime_entry(project: &Project) -> String {
    let defs = registry_keys(project)
        .iter()
        .map(|k| format!("  '{k}': __dclOneMemo(function () {{ return require('{k}') }})"))
        .collect::<Vec<_>>()
        .join(",\n");
    format!(
        r#"'use strict'
function __dclOneMemo(load) {{
  var value
  var done = false
  return function () {{
    if (!done) {{
      value = load()
      done = true
    }}
    return value
  }}
}}
var __dclOneDefs = {{
{defs}
}}
var __dclOneRegistry = {{}}
Object.keys(__dclOneDefs).forEach(function (key) {{
  Object.defineProperty(__dclOneRegistry, key, {{ enumerable: true, get: __dclOneDefs[key] }})
}})
module.exports = __dclOneRegistry
"#
    )
}

pub fn loader_stub(sdk_chunk_rel: &str, scene_chunk_rel: &str) -> String {
    LOADER_TEMPLATE
        .replace("__DCL_ONE_SDK_CHUNK__", sdk_chunk_rel)
        .replace("__DCL_ONE_SCENE_CHUNK__", scene_chunk_rel)
}

pub fn write_loader_stub(outfile: &Path, sdk_chunk_rel: &str, scene_chunk_rel: &str) -> Result<()> {
    if let Some(dir) = outfile.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(outfile, loader_stub(sdk_chunk_rel, scene_chunk_rel)).map_err(|e| {
        crate::ux::UserError::new(
            format!(
                "cannot write the split loader stub to {}",
                outfile.display()
            ),
            crate::ux::TrySteps::one("check write permission on the project directory")
                .and("check \"main\" in scene.json points at a writable file path"),
        )
        .caused_by(e)
        .into()
    })
}

pub fn chunk_rel_paths(main: &str) -> (String, String) {
    match main.rsplit_once('/') {
        Some((dir, _)) => (format!("{dir}/sdk-runtime.js"), format!("{dir}/scene.js")),
        None => ("sdk-runtime.js".to_string(), "scene.js".to_string()),
    }
}

pub fn write_marker(generated_dir: &Path) -> Result<()> {
    let p = generated_dir.join(MARKER_FILE);
    std::fs::write(&p, "1\n").with_context(|| format!("writing {}", p.display()))
}

pub fn clear_marker(generated_dir: &Path) {
    let _ = std::fs::remove_file(generated_dir.join(MARKER_FILE));
}

pub fn detect_split_build(root: &Path, main: &str) -> bool {
    if root.join(".dcl-one").join(MARKER_FILE).exists() {
        return true;
    }
    std::fs::read_to_string(root.join(main))
        .map(|s| s.contains(LOADER_MARKER))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loader_stub_substitutes_chunk_paths() {
        let s = loader_stub("bin/sdk-runtime.js", "bin/scene.js");
        assert!(s.contains("'bin/sdk-runtime.js'"));
        assert!(s.contains("'bin/scene.js'"));
        assert!(!s.contains("__DCL_ONE_SDK_CHUNK__"));
        assert!(!s.contains("__DCL_ONE_SCENE_CHUNK__"));
    }

    #[test]
    fn registry_keys_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for k in REGISTRY_KEYS {
            assert!(seen.insert(*k), "duplicate registry key {k}");
        }
    }

    #[test]
    fn detect_split_build_via_loader_marker_or_marker_file() {
        let root = std::env::temp_dir().join("dcl-one-sdk-split-detect-test");
        std::fs::remove_dir_all(&root).ok();
        std::fs::create_dir_all(root.join("bin")).unwrap();
        assert!(!detect_split_build(&root, "bin/index.js"));
        std::fs::write(
            root.join("bin/index.js"),
            "'use strict'\nmodule.exports.onStart = async function () {}\n",
        )
        .unwrap();
        assert!(!detect_split_build(&root, "bin/index.js"));
        std::fs::write(
            root.join("bin/index.js"),
            loader_stub("bin/sdk-runtime.js", "bin/scene.js"),
        )
        .unwrap();
        assert!(detect_split_build(&root, "bin/index.js"));
        std::fs::remove_file(root.join("bin/index.js")).unwrap();
        std::fs::create_dir_all(root.join(".dcl-one")).unwrap();
        write_marker(&root.join(".dcl-one")).unwrap();
        assert!(detect_split_build(&root, "bin/index.js"));
        clear_marker(&root.join(".dcl-one"));
        assert!(!detect_split_build(&root, "bin/index.js"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn chunk_paths_derive_from_main() {
        assert_eq!(
            chunk_rel_paths("bin/index.js"),
            ("bin/sdk-runtime.js".to_string(), "bin/scene.js".to_string())
        );
        assert_eq!(
            chunk_rel_paths("index.js"),
            ("sdk-runtime.js".to_string(), "scene.js".to_string())
        );
    }
}
