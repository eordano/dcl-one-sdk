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
        write_script_utils(project, &dir)?;
        entrypoint_code(&safe_entry, project.is_editor_scene(), split)
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

fn entrypoint_code(safe_entry: &str, editor_scene: bool, split: bool) -> String {
    let composite_fill = if split {
        "import { compositeFromLoader as __sceneComposites } from './all-composites.js'\nObject.assign(compositeFromLoader, __sceneComposites)\n"
    } else {
        ""
    };
    let editor_block = if editor_scene {
        "\nimport { syncEntity } from '@dcl/sdk/network'\nimport players from '@dcl/sdk/players'\nimport { initAssetPacks } from '@dcl/asset-packs/dist/scene-entrypoint'\ninitAssetPacks(engine, { syncEntity }, players)\n"
            .to_string()
    } else {
        "false".to_string()
    };
    format!(
        r#"// BEGIN AUTO GENERATED CODE "~sdk/scene-entrypoint"
"use strict";
import * as entrypoint from {safe_entry}
import {{ engine, NetworkEntity }} from '@dcl/sdk/ecs'
import * as sdk from '@dcl/sdk'
import {{ compositeProvider }} from '@dcl/sdk/composite-provider'
import {{ compositeFromLoader }} from '~sdk/all-composites'
import {{ _initializeScripts }} from '~sdk/script-utils'
{composite_fill}
{editor_block}

if ((entrypoint as any).main !== undefined) {{
  function _INTERNAL_startup_system() {{
    try {{
      _initializeScripts(engine)

      const maybePromise = (entrypoint as any).main()
      if (maybePromise && typeof maybePromise === 'object' && typeof (maybePromise as unknown as Promise<unknown>).then === 'function') {{
        maybePromise.catch(console.error)
      }}
    }} catch (e) {{
     console.error(e)
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
                Err(err) => tracing::warn!("composite '{rel}' skipped: {err:#}"),
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

fn write_script_utils(project: &Project, dir: &Path) -> Result<()> {
    let runtime = project
        .node_module("@dcl/asset-packs")
        .and_then(|_| project.node_module("@dcl/sdk-commands/dist/logic/runtime-script.js"))
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|code| {
            rewrite_requires(&strip_cjs(&code.replace(
                "@dcl/inspector/node_modules/@dcl/asset-packs",
                "@dcl/asset-packs",
            )))
        });
    let content = match runtime {
        Some(code) => format!(
            "{code}\n\nexport function _initializeScripts(engine) {{\n  const scriptsArray = []\n  return runScripts(engine, scriptsArray)\n}}\n\nexport {{ getScriptInstance, getScriptInstancesByPath, getAllScriptInstances, callScriptMethod }}\n"
        ),
        None => "export function _initializeScripts(_engine) {}\nexport function getScriptInstance() { return null }\nexport function getScriptInstancesByPath() { return [] }\nexport function getAllScriptInstances() { return [] }\nexport function callScriptMethod() {}\n".to_string(),
    };
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
    use super::{rewrite_requires, scan_max_composite_entity, strip_cjs};

    #[test]
    fn max_composite_entity_scans_every_parseable_composite() {
        let dir =
            std::env::temp_dir().join(format!("dcl-one-sdk-maxentity-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
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
