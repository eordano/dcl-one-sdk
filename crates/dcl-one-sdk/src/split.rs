use crate::scene::Project;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

const LOADER_TEMPLATE: &str = include_str!("templates/split-loader.js");
const LOADER_MARKER: &str = "__dclOneSdkChunkPath";
const MARKER_FILE: &str = "split";

/// What the *core* runtime chunk publishes.
///
/// Every specifier a later chunk may `require()` that is not `~system/*` has to
/// appear here or in [`SMART_REGISTRY_KEYS`], or the loader throws
/// "not in the sdk runtime registry" at eval time.
///
/// `@dcl/sdk/platform` and `@dcl/sdk/text-codec` are here because
/// `@dcl/asset-packs` requires both, and once the smart-item runtime lives in
/// its *own* chunk those requires cross the chunk boundary and have to be
/// served by the registry. They were invisible while asset-packs was bundled
/// into the same chunk as the SDK. Adding them also fixes the same latent
/// throw for a scene that imports either subpath directly: `scene_externals()`
/// has always externalised all of `@dcl/sdk/*`, which is broader than the
/// registry on purpose.
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
    "@dcl/sdk/platform",
    "@dcl/sdk/text-codec",
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

/// What the *smart-item* chunk publishes, on top of the core registry.
///
/// `~sdk/script-utils` is deliberately in both lists. The core chunk carries
/// the no-op stub (`_initializeScripts` does nothing); the smart chunk carries
/// the real `runScripts` runtime and must shadow it. That shadowing is what the
/// loader's later-chunk-wins overlay exists for — without it a smart-item scene
/// loads the smart chunk and still never runs a script.
const SMART_REGISTRY_KEYS: &[&str] = &[
    "@dcl/asset-packs",
    "@dcl/asset-packs/dist/scene-entrypoint",
    "~sdk/script-utils",
];

/// Wildcards for everything the *core* chunk owns. Broader than the registry on
/// purpose (design section 4): a specifier that matches here but is not a
/// registry key fails loudly in the loader instead of being silently inlined
/// into a second copy of the SDK.
const SDK_EXTERNALS: &[&str] = &[
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
];

pub fn smart_registry_keys() -> &'static [&'static str] {
    SMART_REGISTRY_KEYS
}

pub fn has_asset_packs(project: &Project) -> bool {
    project.node_module("@dcl/asset-packs").is_some()
        || project
            .node_module("@dcl/inspector/node_modules/@dcl/asset-packs")
            .is_some()
}

/// react 18 ships `react/jsx-runtime`; a tree without it cannot bundle the key.
fn has_jsx_runtime(project: &Project) -> bool {
    project.node_module("react/jsx-runtime.js").is_some()
        || project
            .node_module("@dcl/react-ecs/node_modules/react/jsx-runtime.js")
            .is_some()
}

/// The core chunk's keys: never the asset-packs ones, which belong to the smart
/// chunk alone so a scene without smart items stays off that +30% payload.
pub fn core_registry_keys(project: &Project) -> Vec<&'static str> {
    let mut keys: Vec<&'static str> = REGISTRY_KEYS.to_vec();
    if has_jsx_runtime(project) {
        keys.push("react/jsx-runtime");
    }
    keys
}

/// The keys of the single mono SDK chunk built from source — the npm flow,
/// where `@dcl/asset-packs` is in the scene's own `node_modules` and lands in
/// the same chunk as the SDK.
pub fn registry_keys(project: &Project) -> Vec<&'static str> {
    let mut keys = core_registry_keys(project);
    if has_asset_packs(project) {
        keys.push("@dcl/asset-packs");
        keys.push("@dcl/asset-packs/dist/scene-entrypoint");
    }
    keys
}

pub fn scene_externals(project: &Project) -> Vec<String> {
    let mut externals = smart_externals();
    externals.push("~sdk/script-utils".to_string());
    if has_asset_packs(project) || crate::prebuilt::locate(project).is_some() {
        externals.push("@dcl/asset-packs".to_string());
        externals.push("@dcl/asset-packs/*".to_string());
    }
    externals
}

/// Externals for the *smart-item* chunk: everything the core chunk owns.
/// `~sdk/script-utils` is absent on purpose — the smart chunk has to *bundle*
/// the real implementation, not import the core chunk's stub.
pub fn smart_externals() -> Vec<String> {
    SDK_EXTERNALS
        .iter()
        .map(|s| s.to_string())
        .chain(["~sdk/all-composites".to_string()])
        .collect()
}

pub fn write_generated(project: &Project, dir: &Path) -> Result<()> {
    let slot = dir.join("composite-slot.js");
    std::fs::write(&slot, "export const compositeFromLoader = {}\n")
        .with_context(|| format!("writing {}", slot.display()))?;
    let entry = dir.join("sdk-runtime-entry.js");
    std::fs::write(&entry, registry_module(&registry_keys(project)))
        .with_context(|| format!("writing {}", entry.display()))?;
    Ok(())
}

/// The entry module of a runtime chunk: a registry object of lazily-`require`d
/// namespaces.
///
/// The getters must stay lazy — `@dcl/sdk/platform` calls
/// `getExplorerInformation({})` at module scope, so an eager object literal
/// evaluates a host call the moment the chunk is eval'd, before the scene has
/// started. `configurable: true` is what lets the loader overlay one registry
/// on another (`Object.defineProperty` refuses to redefine a non-configurable
/// property), which is how the smart chunk shadows `~sdk/script-utils`.
pub fn registry_module(keys: &[&str]) -> String {
    let defs = keys
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
  Object.defineProperty(__dclOneRegistry, key, {{ enumerable: true, configurable: true, get: __dclOneDefs[key] }})
}})
module.exports = __dclOneRegistry
"#
    )
}

pub fn loader_stub(
    sdk_chunk_rel: &str,
    smart_chunk_rel: Option<&str>,
    scene_chunk_rel: &str,
    max_composite_entity: u32,
    mp: bool,
) -> String {
    LOADER_TEMPLATE
        .replace("__DCL_ONE_SDK_CHUNK__", sdk_chunk_rel)
        .replace("__DCL_ONE_SMART_CHUNK__", smart_chunk_rel.unwrap_or(""))
        .replace("__DCL_ONE_SCENE_CHUNK__", scene_chunk_rel)
        .replace(
            "__DCL_ONE_MAX_COMPOSITE_ENTITY__",
            &max_composite_entity.to_string(),
        )
        .replace("__DCL_ONE_MP__", if mp { "true" } else { "false" })
}

pub fn write_loader_stub(
    outfile: &Path,
    sdk_chunk_rel: &str,
    smart_chunk_rel: Option<&str>,
    scene_chunk_rel: &str,
    max_composite_entity: u32,
    mp: bool,
) -> Result<()> {
    if let Some(dir) = outfile.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(
        outfile,
        loader_stub(
            sdk_chunk_rel,
            smart_chunk_rel,
            scene_chunk_rel,
            max_composite_entity,
            mp,
        ),
    )
    .map_err(|e| {
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

/// `name` in `main`'s directory, scene-relative.
fn beside(main: &str, name: &str) -> String {
    match main.rsplit_once('/') {
        Some((dir, _)) => format!("{dir}/{name}"),
        None => name.to_string(),
    }
}

pub fn chunk_rel_paths(main: &str) -> (String, String) {
    (beside(main, "sdk-runtime.js"), beside(main, "scene.js"))
}

pub fn smart_chunk_rel_path(main: &str) -> String {
    beside(main, "sdk-smart-items.js")
}

/// The three chunk paths, scene-relative, next to `main`.
pub struct ChunkPaths {
    pub sdk: String,
    pub scene: String,
    pub smart: String,
}

impl ChunkPaths {
    pub fn of(main: &str) -> Self {
        let (sdk, scene) = chunk_rel_paths(main);
        ChunkPaths {
            sdk,
            scene,
            smart: smart_chunk_rel_path(main),
        }
    }
}

/// Everything the loader stub at `outfile` names; `write` re-emits it after a
/// field moves.
pub struct Loader {
    pub outfile: PathBuf,
    pub paths: ChunkPaths,
    pub smart_installed: bool,
    pub max_composite_entity: u32,
    pub mp: bool,
}

impl Loader {
    pub fn write(&self) -> Result<()> {
        write_loader_stub(
            &self.outfile,
            &self.paths.sdk,
            self.smart_installed.then_some(self.paths.smart.as_str()),
            &self.paths.scene,
            self.max_composite_entity,
            self.mp,
        )?;
        tracing::info!("loader stub saved {}", self.outfile.display());
        Ok(())
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
        let s = loader_stub("bin/sdk-runtime.js", None, "bin/scene.js", 517, false);
        assert!(s.contains("'bin/sdk-runtime.js'"));
        assert!(s.contains("'bin/scene.js'"));
        assert!(s.contains("globalThis.DCL_MAX_COMPOSITE_ENTITY = 517"));
        assert!(!s.contains("__DCL_ONE_SDK_CHUNK__"));
        assert!(!s.contains("__DCL_ONE_SMART_CHUNK__"));
        assert!(!s.contains("__DCL_ONE_SCENE_CHUNK__"));
        assert!(s.contains("var __dclOneMp = false"));
        assert!(
            loader_stub("a.js", None, "b.js", 0, true).contains("var __dclOneMp = true"),
            "the mp flag arms the comms wrap"
        );
        assert!(!s.contains("__DCL_ONE_MAX_COMPOSITE_ENTITY__"));
    }

    #[test]
    fn loader_stub_without_a_smart_chunk_leaves_the_path_empty() {
        let plain = loader_stub("bin/sdk-runtime.js", None, "bin/scene.js", 0, false);
        assert!(plain.contains("__dclOneSmartChunkPath = ''"));
        let smart = loader_stub(
            "bin/sdk-runtime.js",
            Some("bin/sdk-smart-items.js"),
            "bin/scene.js",
            0,
            false,
        );
        assert!(smart.contains("__dclOneSmartChunkPath = 'bin/sdk-smart-items.js'"));
    }

    #[test]
    fn registry_keys_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for k in REGISTRY_KEYS {
            assert!(seen.insert(*k), "duplicate registry key {k}");
        }
        let core: std::collections::HashSet<_> = REGISTRY_KEYS.iter().collect();
        let mut seen_smart = std::collections::HashSet::new();
        for k in SMART_REGISTRY_KEYS {
            assert!(seen_smart.insert(*k), "duplicate smart registry key {k}");
            if core.contains(k) {
                assert_eq!(*k, "~sdk/script-utils", "unexpected shadowed key {k}");
            }
        }
    }

    #[test]
    fn registry_module_getters_are_lazy_and_configurable() {
        let m = registry_module(&["@dcl/sdk/platform"]);
        assert!(m.contains("__dclOneMemo(function () { return require('@dcl/sdk/platform') })"));
        assert!(m.contains("configurable: true"));
    }

    #[test]
    fn smart_externals_do_not_cover_script_utils() {
        let e = smart_externals();
        assert!(e.iter().any(|s| s == "@dcl/sdk/*"));
        assert!(e.iter().any(|s| s == "~sdk/all-composites"));
        assert!(!e.iter().any(|s| s == "~sdk/script-utils"));
    }

    #[test]
    fn detect_split_build_via_loader_marker_or_marker_file() {
        let t = crate::scene::Tmp::new("split-detect");
        let root = &t.0;
        assert!(!detect_split_build(root, "bin/index.js"));
        t.write(
            "bin/index.js",
            "'use strict'\nmodule.exports.onStart = async function () {}\n",
        );
        assert!(!detect_split_build(root, "bin/index.js"));
        t.write(
            "bin/index.js",
            loader_stub("bin/sdk-runtime.js", None, "bin/scene.js", 0, false),
        );
        assert!(detect_split_build(root, "bin/index.js"));
        std::fs::remove_file(root.join("bin/index.js")).unwrap();
        std::fs::create_dir_all(root.join(".dcl-one")).unwrap();
        write_marker(&root.join(".dcl-one")).unwrap();
        assert!(detect_split_build(root, "bin/index.js"));
        clear_marker(&root.join(".dcl-one"));
        assert!(!detect_split_build(root, "bin/index.js"));
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
        assert_eq!(
            smart_chunk_rel_path("bin/index.js"),
            "bin/sdk-smart-items.js"
        );
        assert_eq!(smart_chunk_rel_path("index.js"), "sdk-smart-items.js");
    }
}
