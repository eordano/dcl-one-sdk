use crate::scene::Project;
use crate::ux::{TrySteps, UserError};
use anyhow::Result;
use std::path::{Path, PathBuf};

pub struct Generated {
    pub dir: PathBuf,
    pub entrypoint: PathBuf,
    pub max_composite_entity: u32,
}

const COMPOSITE_FILE_MAX_BYTES: u64 = 16 * 1024 * 1024;

fn write_error(path: &Path, e: std::io::Error) -> anyhow::Error {
    UserError::new(
        format!("cannot write to {}", path.display()),
        TrySteps::one("check write permission on the project directory")
            .and("re-run from a writable checkout (not a read-only mount)"),
    )
    .caused_by(e)
    .into()
}

pub fn generate(
    project: &Project,
    ignore_composite: bool,
    custom_entry: bool,
    split: bool,
) -> Result<Generated> {
    let dir = project.root.join(".dcl-one");
    std::fs::create_dir_all(&dir).map_err(|e| write_error(&dir, e))?;

    let user_entry = project.root.join("src/index.ts");
    let safe_entry = serde_json::to_string(&user_entry.display().to_string().replace('\\', "/"))?;

    let entry_path = dir.join("entrypoint.ts");
    let content = if custom_entry {
        format!(";\"use strict\";export * from {safe_entry}")
    } else {
        write_all_composites(project, &dir, ignore_composite)?;
        write_script_utils(project, &dir, ignore_composite)?;
        write_sdk_boot(&dir)?;
        let mp = authoritative_multiplayer(project);
        if mp {
            write_mp_client(&dir)?;
        }
        entrypoint_code(&safe_entry, project.is_editor_scene(), split, mp)
    };
    std::fs::write(&entry_path, content).map_err(|e| write_error(&entry_path, e))?;

    let max_composite_entity = if ignore_composite {
        0
    } else {
        scan_max_composite_entity(&project.root)
    };
    Ok(Generated {
        dir,
        entrypoint: entry_path,
        max_composite_entity,
    })
}

/// scene.json's documented activation flag for the authoritative-server
/// surface (docs/multiplayer-server-design.md): with it, the loader arms the
/// comms wrap and the entrypoint pulls in the mp-client half.
pub fn authoritative_multiplayer(project: &Project) -> bool {
    project
        .scene_json
        .get("authoritativeMultiplayer")
        .and_then(|v| v.as_bool())
        == Some(true)
}

const MP_CLIENT_TEMPLATE: &str = include_str!("templates/mp-client.js");

fn write_mp_client(dir: &Path) -> Result<()> {
    let path = dir.join("mp-client.js");
    std::fs::write(&path, MP_CLIENT_TEMPLATE).map_err(|e| write_error(&path, e))?;
    Ok(())
}

fn entrypoint_code(safe_entry: &str, editor_scene: bool, split: bool, mp: bool) -> String {
    let composite_fill = if split {
        "import { compositeFromLoader as __sceneComposites } from './all-composites.js'\nObject.assign(compositeFromLoader, __sceneComposites)\n"
    } else {
        ""
    };
    let editor_block = if editor_scene {
        "\nimport { syncEntity } from '@dcl/sdk/network'\nimport players from '@dcl/sdk/players'\nimport { setCompositeProvider } from '@dcl/sdk/ecs'\nimport { initAssetPacks } from '@dcl/asset-packs/dist/scene-entrypoint'\nsetCompositeProvider(engine, compositeProvider)\ninitAssetPacks(engine, { syncEntity }, players)\n"
            .to_string()
    } else {
        "false".to_string()
    };
    // before the scene, after sdk-boot: the scene's module scope feature-
    // detects registerMessages, so the graft must already be in place
    let mp_import = if mp { "import './mp-client.js'\n" } else { "" };
    format!(
        r#"// BEGIN AUTO GENERATED CODE "~sdk/scene-entrypoint"
"use strict";
import {{ engine, NetworkEntity }} from '@dcl/sdk/ecs'
import * as sdk from '@dcl/sdk'
import {{ compositeProvider }} from '@dcl/sdk/composite-provider'
import {{ compositeFromLoader }} from '~sdk/all-composites'
import {{ _initializeScripts }} from '~sdk/script-utils'
// Registers the composite provider, and must be imported BEFORE the scene.
// A scene that calls initAssetPacks() at module scope -- as the Creator Hub
// templates do -- reads that provider while its own module body runs, and until
// this existed the only thing registering it was `@dcl/sdk`'s body, which the
// bundler emits at the END of the chunk. So the editor logged "[asset-packs] No
// SDK composite provider registered; SPAWN_ENTITY cannot resolve composites"
// and placing a smart item resolved nothing. Upstream sdk-commands has the same
// ordering (logic/bundle.ts) and gets away with it only because upstream scenes
// never call initAssetPacks themselves.
//
// A separate module because neither cheaper fix works: statements here run after
// every import including the scene's, and the bundler does not honour moving
// `@dcl/sdk` earlier -- a side-effect import is shaken to a bare require and the
// re-export below then reads a `_dcl_sdk` binding that was never declared, while
// `export * from` is emitted at the end wherever it is written.
import './sdk-boot.js'
{mp_import}import * as entrypoint from {safe_entry}
{composite_fill}
{editor_block}

// `console.error(e)` drops the frames, and the host captures its own stack at
// the call site — this catch block. `e.stack` carries the throw site, so it is
// a superset of what upstream prints.
function __reportSceneError(e: any) {{
  console.error(e && e.stack ? e.stack : e)
}}

if ((entrypoint as any).main !== undefined) {{
  function _INTERNAL_startup_system() {{
    try {{
      _initializeScripts(engine)

      const maybePromise = (entrypoint as any).main()
      if (maybePromise && typeof maybePromise === 'object' && typeof (maybePromise as unknown as Promise<unknown>).then === 'function') {{
        maybePromise.catch(__reportSceneError)
      }}
    }} catch (e) {{
      __reportSceneError(e)
    }} finally {{
      engine.removeSystem(_INTERNAL_startup_system)
    }}
  }}
  engine.addSystem(_INTERNAL_startup_system, Infinity)
}}

export * from '@dcl/sdk'
export * from {safe_entry}
export * from '~sdk/script-utils'
"#
    )
}

pub fn find_composites(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk_composites(root, &mut out);
    out.sort();
    out
}

fn walk_composites(dir: &Path, out: &mut Vec<PathBuf>) {
    let rd = match std::fs::read_dir(dir) {
        Ok(x) => x,
        Err(_) => return,
    };
    for entry in rd.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if !name.starts_with('.') && !matches!(name.as_str(), "node_modules" | "bin" | "dist") {
                walk_composites(&path, out);
            }
        } else if name.ends_with(".composite") && !name.starts_with('.') {
            if path.metadata().map(|m| m.len()).unwrap_or(0) > COMPOSITE_FILE_MAX_BYTES {
                tracing::warn!(
                    "composite '{}' exceeds the {COMPOSITE_FILE_MAX_BYTES}-byte cap; refusing to parse",
                    path.display()
                );
            } else {
                out.push(path);
            }
        }
    }
}

fn write_all_composites(project: &Project, dir: &Path, ignore: bool) -> Result<()> {
    let mut lines = Vec::new();
    if !ignore {
        let mut normalizer = crate::composite_norm::CompositeNormalizer::new();
        for path in find_composites(&project.root) {
            let rel = path
                .strip_prefix(&project.root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            if rel != "main.composite" {
                continue;
            }
            let normalized = std::fs::read_to_string(&path)
                .map_err(anyhow::Error::from)
                .and_then(|raw| normalizer.normalize(&raw));
            match normalized {
                Ok(json) => lines.push(format!("'{rel}':{json}")),
                Err(err) => {
                    return Err(UserError::new(
                        format!("{rel} could not be loaded, so the scene would build with no content from the editor"),
                        TrySteps::one(format!("fix {rel} — the underlying error is below"))
                            .and("or build without it on purpose: dcl-one-sdk build --ignoreComposite"),
                    )
                    .why(format!("{err:#}"))
                    .into());
                }
            }
        }
    }
    let content = format!("export const compositeFromLoader = {{{}}}", lines.join(","));
    let path = dir.join("all-composites.js");
    std::fs::write(&path, content).map_err(|e| write_error(&path, e))?;
    Ok(())
}

pub fn scan_max_composite_entity(root: &Path) -> u32 {
    let mut max = 0u32;
    for path in find_composites(root) {
        let Ok(raw) = std::fs::read(&path) else {
            continue;
        };
        let Ok(json) = serde_json::from_slice::<serde_json::Value>(&raw) else {
            continue;
        };
        let comps = json
            .get("components")
            .and_then(|c| c.as_array())
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        for comp in comps {
            let Some(data) = comp.get("data").and_then(|d| d.as_object()) else {
                continue;
            };
            for key in data.keys() {
                if let Ok(id) = key.parse::<u64>() {
                    max = max.max((id & 0xffff) as u32);
                }
            }
        }
    }
    max
}

/// The `~sdk/script-utils` no-op a scene without the smart-item runtime links
/// against; the generated entrypoint calls `_initializeScripts` unconditionally.
/// Same export surface as upstream's `generateScriptStubModuleContent`.
pub const SCRIPT_UTILS_STUB: &str = "export function _initializeScripts(_engine) {}\nexport function getScriptInstance(_entity, _scriptPath) { return null }\nexport function getScriptInstancesByPath(_scriptPath) { return [] }\nexport function getAllScriptInstances(_entity) { return [] }\nexport function callScriptMethod(entity, scriptPath, methodName, ..._args) {\n  console.error(`Method ${methodName} not found on script ${scriptPath} for entity ${entity}`)\n  return undefined\n}\n";

/// The real `~sdk/script-utils`: `@dcl/sdk-commands`' compiled smart-item script
/// runtime, de-CommonJS'd so rolldown can bundle it as an ES module. `None`
/// unless both packages are on disk — the npm flow; the vendored blob ships
/// this same output prebuilt, via `prebuilt::build_chunks`.
pub fn script_utils_source(project: &Project) -> Option<String> {
    let code = project
        .node_module("@dcl/asset-packs")
        .and_then(|_| project.node_module("@dcl/sdk-commands/dist/logic/runtime-script.js"))
        .and_then(|p| std::fs::read_to_string(p).ok())?;
    let runtime = rewrite_requires(&strip_cjs(&code.replace(
        "@dcl/inspector/node_modules/@dcl/asset-packs",
        "@dcl/asset-packs",
    )));
    Some(format!(
        "{runtime}\n\nexport function _initializeScripts(engine) {{\n  const scriptsArray = []\n  return runScripts(engine, scriptsArray)\n}}\n\nexport {{ getScriptInstance, getScriptInstancesByPath, getAllScriptInstances, callScriptMethod }}\n"
    ))
}

/// Upstream skips the embedded script runtime for scenes that author no scripts
/// and are not editor scenes.
pub fn script_utils_content(project: &Project, ignore_composite: bool) -> String {
    let has_scripts = !ignore_composite && composites_have_scripts(&project.root);
    if !has_scripts && !project.is_editor_scene() {
        return SCRIPT_UTILS_STUB.to_string();
    }
    script_utils_source(project).unwrap_or_else(|| SCRIPT_UTILS_STUB.to_string())
}

/// Does any composite author a smart-item script? The Rust side of upstream's
/// `compositeData.scripts.size > 0`: any entry in an `asset-packs::Script`
/// component's `value` array flips the gate. Composites that would fail to
/// instance contribute nothing, matching upstream's per-composite try/catch.
pub fn composites_have_scripts(root: &Path) -> bool {
    let (has_scripts, skipped) = composites_script_scan(root);
    for message in skipped {
        crate::ux::note_stderr(message);
    }
    has_scripts
}

fn composites_script_scan(root: &Path) -> (bool, Vec<String>) {
    let mut normalizer = crate::composite_norm::CompositeNormalizer::new();
    let mut skipped = Vec::new();
    let has_scripts = find_composites(root).into_iter().any(|path| {
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return false;
        };
        if let Err(err) = normalizer.normalize(&raw) {
            skipped.push(format!(
                "composite {} can't be instanced, so its components are ignored: {err:#}",
                crate::ux::rel_to(root, &path)
            ));
            return false;
        }
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) else {
            return false;
        };
        json.get("components")
            .and_then(|c| c.as_array())
            .is_some_and(|comps| {
                comps
                    .iter()
                    .filter(|c| {
                        c.get("name").and_then(|n| n.as_str()) == Some("asset-packs::Script")
                    })
                    .any(script_component_has_instances)
            })
    });
    (has_scripts, skipped)
}

fn script_component_has_instances(comp: &serde_json::Value) -> bool {
    comp.get("data")
        .and_then(|d| d.as_object())
        .is_some_and(|data| {
            data.values().any(|entry| match entry.get("json") {
                Some(json) => json
                    .get("value")
                    .and_then(|v| v.as_array())
                    .is_some_and(|v| !v.is_empty()),
                None => entry.get("binary").is_some(),
            })
        })
}

// Exactly what `@dcl/sdk`'s own module body does (its index.ts), pulled into a
// module the generated entrypoint can import before the scene.
fn write_sdk_boot(dir: &Path) -> Result<()> {
    let content = "import { engine, setCompositeProvider } from '@dcl/sdk/ecs'\n\
                   import { compositeProvider } from '@dcl/sdk/composite-provider'\n\
                   setCompositeProvider(engine, compositeProvider)\n";
    let path = dir.join("sdk-boot.js");
    std::fs::write(&path, content).map_err(|e| write_error(&path, e))?;
    Ok(())
}

fn write_script_utils(project: &Project, dir: &Path, ignore_composite: bool) -> Result<()> {
    let content = script_utils_content(project, ignore_composite);
    let path = dir.join("script-utils.js");
    std::fs::write(&path, content).map_err(|e| write_error(&path, e))?;
    Ok(())
}

fn strip_cjs(code: &str) -> String {
    let mut out = String::with_capacity(code.len());
    for line in code.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("\"use strict\"")
            || trimmed.starts_with("Object.defineProperty(exports,")
            || trimmed.starts_with("import ")
            || trimmed.starts_with("//# sourceMappingURL")
        {
            continue;
        }
        let mut l = line.to_string();
        if let Some(rest) = trimmed.strip_prefix("export ") {
            let indent_len = line.len() - trimmed.len();
            l = format!("{}{}", &line[..indent_len], rest);
        }
        while let Some(idx) = l.find("exports.") {
            let after = &l[idx + 8..];
            if let Some(eq) = after.find('=') {
                let ident = &after[..eq];
                if ident
                    .trim()
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == ' ')
                {
                    let rhs = after[eq + 1..].trim_start();
                    if rhs.starts_with("void 0") {
                        l = format!("{}{}", &l[..idx], strip_void_stmt(&after[eq + 1..]));
                        continue;
                    }
                    l = format!("{}{}", &l[..idx], after[eq + 1..].trim_start());
                    continue;
                }
            }
            break;
        }
        out.push_str(&l);
        out.push('\n');
    }
    out.trim().to_string()
}

fn strip_void_stmt(rest: &str) -> String {
    rest.trim_start()
        .strip_prefix("void 0")
        .map(|r| r.trim_start_matches(';').trim_start().to_string())
        .unwrap_or_default()
}

fn rewrite_requires(code: &str) -> String {
    let mut out = String::with_capacity(code.len());
    for line in code.lines() {
        match top_level_require(line) {
            Some((name, spec)) => {
                let spec = if spec == "@dcl/ecs/dist-cjs" || spec.starts_with("@dcl/ecs/dist-cjs/")
                {
                    "@dcl/ecs"
                } else {
                    spec
                };
                out.push_str(&format!("import * as {name} from \"{spec}\"\n"));
            }
            None => {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    out.trim().to_string()
}

fn top_level_require(line: &str) -> Option<(&str, &str)> {
    let rest = ["const ", "var ", "let "]
        .iter()
        .find_map(|kw| line.strip_prefix(kw))?;
    let (name, rest) = rest.split_once('=')?;
    let name = name.trim();
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '$')
    {
        return None;
    }
    let rest = rest.trim().strip_prefix("require(\"")?;
    let (spec, tail) = rest.split_once('"')?;
    let tail = tail.trim_start().strip_prefix(')')?;
    if !tail.trim_end_matches(';').trim().is_empty() {
        return None;
    }
    Some((name, spec))
}

#[cfg(test)]
mod tests {
    use super::{
        composites_have_scripts, composites_script_scan, rewrite_requires,
        scan_max_composite_entity, script_utils_content, strip_cjs, SCRIPT_UTILS_STUB,
    };
    use crate::scene::Project;
    use std::path::PathBuf;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dcl-one-sdk-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn composites_have_scripts_requires_script_instances() {
        let dir = scratch("hasscripts");
        std::fs::create_dir_all(dir.join("assets/scene")).unwrap();
        assert!(!composites_have_scripts(&dir));
        std::fs::write(
            dir.join("assets/scene/main.composite"),
            r#"{"version":1,"components":[{"name":"asset-packs::Script","jsonSchema":{"type":"object"},"data":{"512":{"json":{"value":[]}}}},{"name":"asset-packs::Actions","jsonSchema":{"type":"object"},"data":{"512":{"json":{}}}}]}"#,
        )
        .unwrap();
        assert!(!composites_have_scripts(&dir));
        std::fs::write(
            dir.join("assets/scene/main.composite"),
            r#"{"version":1,"components":[{"name":"asset-packs::Script","jsonSchema":{"type":"object"},"data":{"512":{"json":{"value":[{"path":"src/counter.ts","priority":0}]}}}}]}"#,
        )
        .unwrap();
        assert!(composites_have_scripts(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn composites_have_scripts_counts_binary_entries_in_instanceable_composites() {
        let dir = scratch("binscripts");
        std::fs::create_dir_all(dir.join("assets/scene")).unwrap();
        std::fs::write(
            dir.join("assets/scene/main.composite"),
            r#"{"version":1,"components":[{"name":"asset-packs::Script","jsonSchema":{"type":"object"},"data":{"512":{"binary":"AQID"}}}]}"#,
        )
        .unwrap();
        assert!(composites_have_scripts(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn non_instanceable_composites_do_not_flip_the_script_gate() {
        let dir = scratch("badscripts");
        std::fs::create_dir_all(dir.join("assets/scene")).unwrap();
        std::fs::write(
            dir.join("assets/scene/main.composite"),
            r#"{"version":1,"components":[{"name":"core::NotAThing","data":{}},{"name":"asset-packs::Script","jsonSchema":{"type":"object"},"data":{"512":{"json":{"value":[{"path":"src/counter.ts"}]}}}}]}"#,
        )
        .unwrap();
        assert!(!composites_have_scripts(&dir));
        std::fs::write(
            dir.join("assets/scene/other.composite"),
            r#"{"version":1,"components":[{"name":"asset-packs::Script","data":{"512":{"json":{"value":[{"path":"src/counter.ts"}]}}}}]}"#,
        )
        .unwrap();
        assert!(!composites_have_scripts(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn non_instanceable_composite_skip_is_reported() {
        let dir = scratch("skipnote");
        std::fs::create_dir_all(dir.join("assets/scene")).unwrap();
        std::fs::write(
            dir.join("assets/scene/main.composite"),
            r#"{"version":1,"components":[{"name":"core::NotAThing","data":{}},{"name":"asset-packs::Script","jsonSchema":{"type":"object"},"data":{"512":{"json":{"value":[{"path":"src/counter.ts"}]}}}}]}"#,
        )
        .unwrap();
        let (has_scripts, skipped) = composites_script_scan(&dir);
        assert!(!has_scripts);
        assert_eq!(skipped.len(), 1);
        assert!(skipped[0].contains("can't be instanced"), "{}", skipped[0]);
        assert!(skipped[0].contains("main.composite"), "{}", skipped[0]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn script_utils_stub_for_scriptless_scenes_even_with_the_runtime_installed() {
        let dir = scratch("scriptgate");
        std::fs::create_dir_all(dir.join("node_modules/@dcl/asset-packs")).unwrap();
        std::fs::create_dir_all(dir.join("node_modules/@dcl/sdk-commands/dist/logic")).unwrap();
        std::fs::write(
            dir.join("node_modules/@dcl/sdk-commands/dist/logic/runtime-script.js"),
            "\"use strict\";\nexports.runScripts = runScripts;\nfunction runScripts(engine, scripts) {}\n",
        )
        .unwrap();
        let project = Project {
            root: dir.clone(),
            scene_json: serde_json::json!({"main": "bin/index.js"}),
        };
        assert_eq!(script_utils_content(&project, false), SCRIPT_UTILS_STUB);
        std::fs::create_dir_all(dir.join("assets/scene")).unwrap();
        std::fs::write(
            dir.join("assets/scene/main.composite"),
            r#"{"version":1,"components":[{"name":"asset-packs::Script","jsonSchema":{"type":"object"},"data":{"0":{"json":{"value":[{"path":"src/counter.ts"}]}}}}]}"#,
        )
        .unwrap();
        assert!(script_utils_content(&project, false).contains("runScripts"));
        assert_eq!(script_utils_content(&project, true), SCRIPT_UTILS_STUB);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stub_matches_upstreams_export_surface() {
        for export in [
            "_initializeScripts",
            "getScriptInstance",
            "getScriptInstancesByPath",
            "getAllScriptInstances",
            "callScriptMethod",
        ] {
            assert!(SCRIPT_UTILS_STUB.contains(&format!("export function {export}")));
        }
        assert!(SCRIPT_UTILS_STUB.contains("not found on script"));
    }

    #[test]
    fn max_composite_entity_scans_every_parseable_composite() {
        let dir = scratch("maxentity");
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        assert_eq!(scan_max_composite_entity(&dir), 0);
        std::fs::write(
            dir.join("main.composite"),
            r#"{"version":1,"components":[{"name":"core::Transform","data":{"512":{},"600":{}}}]}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("sub/other.composite"),
            r#"{"version":1,"components":[{"name":"my::Thing","data":{"5170":{}}}]}"#,
        )
        .unwrap();
        std::fs::write(dir.join("sub/broken.composite"), "not json").unwrap();
        assert_eq!(scan_max_composite_entity(&dir), 5170);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rewrites_dist_cjs_barrel_require_to_esm_barrel_import() {
        let out = rewrite_requires("const entity_1 = require(\"@dcl/ecs/dist-cjs\");");
        assert_eq!(out, "import * as entity_1 from \"@dcl/ecs\"");
    }

    #[test]
    fn rewrites_dist_cjs_leaf_require_to_esm_barrel_import() {
        let out =
            rewrite_requires("const entity_1 = require(\"@dcl/ecs/dist-cjs/engine/entity\");");
        assert_eq!(out, "import * as entity_1 from \"@dcl/ecs\"");
    }

    #[test]
    fn rewrites_other_requires_keeping_the_spec() {
        let out = rewrite_requires("const asset_packs_1 = require(\"@dcl/asset-packs\");");
        assert_eq!(out, "import * as asset_packs_1 from \"@dcl/asset-packs\"");
    }

    #[test]
    fn leaves_indented_and_non_require_lines_alone() {
        let src = "function lazy() {\n  const x = require(\"fs\");\n  return x\n}\nconst n = 1;";
        assert_eq!(rewrite_requires(src), src);
    }

    #[test]
    fn leaves_multi_statement_lines_alone() {
        let src = "const a = require(\"x\"); const b = 2;";
        assert_eq!(rewrite_requires(src), src);
    }

    #[test]
    fn full_pipeline_on_compiled_runtime_script_header() {
        let compiled = "\"use strict\";\nObject.defineProperty(exports, \"__esModule\", { value: true });\nexports.runScripts = runScripts;\nconst entity_1 = require(\"@dcl/ecs/dist-cjs\");\nconst asset_packs_1 = require(\"@dcl/asset-packs\");\nfunction entityIsRemoved(engine, entity) {\n    return engine.getEntityState(entity) === entity_1.EntityState.Removed;\n}\n";
        let out = rewrite_requires(&strip_cjs(compiled));
        assert!(out.contains("import * as entity_1 from \"@dcl/ecs\""));
        assert!(out.contains("import * as asset_packs_1 from \"@dcl/asset-packs\""));
        assert!(!out.contains("require("));
        assert!(out.contains("entity_1.EntityState.Removed"));
    }
}
