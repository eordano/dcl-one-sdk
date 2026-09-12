//! Prebuilt SDK runtime chunks: built once at blob-build time, not per scene.
//!
//! The SDK runtime chunk is scene-independent — keyed only on which `@dcl/*`
//! packages are installed, never on what the scene imports — so the 3.64 MB of
//! SDK source the blob used to ship existed only to let rolldown re-derive the
//! same bytes on every build.
//!
//! Two chunks, not one: **core** (always installed; the registry of
//! [`crate::split::core_registry_keys`]) and **smart** (installed only when the
//! scene uses smart items; `@dcl/asset-packs`, its scene entrypoint and the
//! real `~sdk/script-utils`, resolving everything else through core's
//! registry). `write_script_utils` inlines the smart-item runtime whenever
//! `@dcl/asset-packs` merely *resolves*, +30% on every bundle; the split makes
//! only smart-item scenes pay it.
//!
//! The chunks live inside the vendored `@dcl/sdk` (`node_modules/@dcl/sdk/
//! prebuilt/`) on purpose: they are valid only for the `@dcl/sdk` they were
//! built from, so an `npm install` that replaces the package removes them in
//! the same step and flips the build back to the source path atomically.

use crate::esbuild::EsbuildOptions;
use crate::scene::Project;
use crate::ux::{TrySteps, UserError};
use crate::{entrypoint, split};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub const DIR: &str = "@dcl/sdk/prebuilt";
pub const CORE_FILE: &str = "@dcl/sdk/prebuilt/core.js";
pub const SMART_FILE: &str = "@dcl/sdk/prebuilt/smart.js";
/// The keys each chunk publishes, written next to them so the blob's
/// unresolved-import scan can check the chunks against the registry that
/// actually resolves their imports instead of against `node_modules`.
pub const REGISTRY_FILE: &str = "@dcl/sdk/prebuilt/registry.json";

pub struct Prebuilt {
    pub core: PathBuf,
    pub smart: Option<PathBuf>,
}

/// `None` means a source `node_modules` (an npm install, or an older blob):
/// the SDK chunk is bundled from source.
pub fn locate(project: &Project) -> Option<Prebuilt> {
    let core = project.node_module(CORE_FILE)?;
    Some(Prebuilt {
        core,
        smart: project.node_module(SMART_FILE),
    })
}

/// An editor scene's entrypoint calls `initAssetPacks`; a scene can also import
/// `@dcl/asset-packs/dist/scene-entrypoint` from its own source with no
/// composite at all (`0,0-cube-spawner` in sdk7-test-scenes), and since the
/// package is an external of the scene chunk the specifier survives into the
/// emitted bundle exactly when something reaches it.
pub fn scene_needs_smart_chunk(project: &Project, scene_chunk: &Path) -> bool {
    project.is_editor_scene() || chunk_requires(scene_chunk, "@dcl/asset-packs")
}

fn chunk_requires(chunk: &Path, package: &str) -> bool {
    let Ok(code) = std::fs::read_to_string(chunk) else {
        return false;
    };
    code.contains(&format!("require(\"{package}")) || code.contains(&format!("require('{package}"))
}

pub fn install(src: &Path, dst: &Path) -> Result<()> {
    if let Some(dir) = dst.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("creating the bundle directory {}", dir.display()))?;
    }
    std::fs::copy(src, dst).map_err(|e| {
        anyhow::Error::from(
            UserError::new(
                format!(
                    "cannot install the prebuilt SDK runtime chunk into {}",
                    dst.display()
                ),
                TrySteps::one("check write permission on the project directory").and(
                    "re-install the vendored toolchain with dcl-one-sdk init --node-modules-only",
                ),
            )
            .caused_by(e),
        )
    })?;
    Ok(())
}

/// Deleting the last smart item would otherwise leave a stale
/// `sdk-smart-items.js` the loader no longer names but `deploy` still uploads.
pub fn remove_stale_smart_chunk(root: &Path, smart_rel: &str) {
    let _ = std::fs::remove_file(root.join(smart_rel));
}

/// Build both chunks from a scene whose `node_modules` is the full install tree
/// (what `scripts/build-base-blob.py` calls through the hidden `vendor-chunks`
/// subcommand). The passes differ in entry and externals: core aliases
/// `~sdk/script-utils` to the no-op stub with nothing external; smart makes
/// everything core owns external and aliases `~sdk/script-utils` to the *real*
/// `@dcl/sdk-commands` runtime, so exactly one copy is bundled and the registry
/// entry and asset-packs' own import land on the same module instance. Both are
/// then checked for requires the loader could not serve — `@dcl/sdk/platform`
/// and `@dcl/sdk/text-codec` were invisible while asset-packs shared the SDK's
/// chunk, and would have thrown "not in the sdk runtime registry" at scene start.
pub async fn build_chunks(dir: &Path, out_core: &Path, out_smart: &Path) -> Result<()> {
    let project = Project::load(dir)?;
    let tsconfig = project.tsconfig()?;
    let work = crate::scene::work_dir(&project.root)
        .with_context(|| format!("creating {}", project.root.join(".dcl-one").display()))?;
    let write = |name: &str, content: &str| -> Result<PathBuf> {
        let path = work.join(name);
        std::fs::write(&path, content)?;
        Ok(path)
    };

    let core_keys = split::core_registry_keys(&project);
    let smart_keys = split::smart_registry_keys();

    let slot = write(
        "composite-slot.js",
        "export const compositeFromLoader = {}\n",
    )?;
    let stub = write("script-utils-stub.js", entrypoint::SCRIPT_UTILS_STUB)?;
    let core_entry = write("core-registry.js", &split::registry_module(&core_keys))?;
    let mut core_aliases = crate::esbuild::resolve_aliases(&project)?;
    core_aliases.push(("~sdk/all-composites".to_string(), slot));
    core_aliases.push(("~sdk/script-utils".to_string(), stub));
    bundle_chunk(
        &project,
        core_entry,
        out_core,
        &tsconfig,
        core_aliases,
        vec![],
    )
    .await?;

    let script_utils = entrypoint::script_utils_source(&project).ok_or_else(|| {
        anyhow::Error::from(UserError::new(
            "cannot build the smart-item chunk: the real ~sdk/script-utils runtime is missing",
            TrySteps::one(
                "install @dcl/asset-packs and @dcl/sdk-commands in the blob work tree before building the chunks",
            ),
        )
        .why("@dcl/sdk-commands/dist/logic/runtime-script.js did not resolve"))
    })?;
    let real_utils = write("script-utils.js", &script_utils)?;
    let smart_entry = write("smart-registry.js", &split::registry_module(smart_keys))?;
    let mut smart_aliases = Vec::new();
    if let Some(ap) = project.node_module("@dcl/asset-packs") {
        smart_aliases.push(("@dcl/asset-packs".to_string(), ap));
    }
    smart_aliases.push(("~sdk/script-utils".to_string(), real_utils));
    bundle_chunk(
        &project,
        smart_entry,
        out_smart,
        &tsconfig,
        smart_aliases,
        split::smart_externals(),
    )
    .await?;

    verify_requires(out_core, &[])?;
    verify_requires(out_smart, &core_keys)?;

    let registry = serde_json::json!({
        "core": core_keys,
        "smart": smart_keys,
    });
    let manifest = out_core.with_file_name("registry.json");
    std::fs::write(&manifest, serde_json::to_vec_pretty(&registry)?)
        .with_context(|| format!("writing {}", manifest.display()))?;
    Ok(())
}

async fn bundle_chunk(
    project: &Project,
    entrypoint: PathBuf,
    outfile: &Path,
    tsconfig: &Path,
    aliases: Vec<(String, PathBuf)>,
    externals: Vec<String>,
) -> Result<()> {
    crate::esbuild::bundle(
        project,
        &EsbuildOptions {
            production: true,
            entrypoint,
            outfile: outfile.to_path_buf(),
            tsconfig: tsconfig.to_path_buf(),
            aliases,
            externals,
        },
    )
    .await
}

/// Every `require()` literal in a built chunk must be `~system/*` (passed to
/// the host) or a key the loader's registry already holds.
fn verify_requires(chunk: &Path, allowed: &[&str]) -> Result<()> {
    let code = std::fs::read_to_string(chunk)
        .with_context(|| format!("reading the built chunk {}", chunk.display()))?;
    let mut bad: Vec<String> = Vec::new();
    for spec in require_specifiers(&code) {
        if spec.starts_with("~system/") || allowed.contains(&spec.as_str()) {
            continue;
        }
        if !bad.contains(&spec) {
            bad.push(spec);
        }
    }
    if bad.is_empty() {
        return Ok(());
    }
    bad.sort();
    Err(UserError::new(
        format!(
            "{} requires specifiers the sdk runtime registry does not publish",
            chunk.display()
        ),
        TrySteps::one("add each specifier below to REGISTRY_KEYS in src/split.rs")
            .and("or make it external of the chunk that should own it"),
    )
    .why(bad.join(", "))
    .into())
}

fn require_specifiers(code: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = code.as_bytes();
    let mut i = 0;
    while let Some(pos) = code[i..].find("require(") {
        let start = i + pos + "require(".len();
        i = start;
        let Some(&quote) = bytes.get(start) else {
            break;
        };
        if quote != b'"' && quote != b'\'' {
            continue;
        }
        let Some(end) = code[start + 1..].find(quote as char) else {
            break;
        };
        let spec = &code[start + 1..start + 1 + end];
        if code.as_bytes().get(start + 2 + end) == Some(&b')') {
            out.push(spec.to_string());
        }
        i = start + 1 + end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Tmp;

    #[test]
    fn require_specifiers_reads_both_quote_styles_and_ignores_calls() {
        let code = r#"var a=require("@dcl/sdk/ecs"),b=require('~system/Runtime');require(x);"#;
        assert_eq!(
            require_specifiers(code),
            vec!["@dcl/sdk/ecs".to_string(), "~system/Runtime".to_string()]
        );
    }

    #[test]
    fn verify_requires_accepts_system_and_registry_keys_only() {
        let t = Tmp::new("prebuilt");
        t.write(
            "smart.js",
            r#"require("~system/EngineApi");require("@dcl/sdk/ecs")"#,
        );
        let chunk = t.0.join("smart.js");
        assert!(verify_requires(&chunk, &["@dcl/sdk/ecs"]).is_ok());
        assert!(verify_requires(&chunk, &[]).is_err());
    }

    #[test]
    fn chunk_requires_matches_only_a_real_specifier() {
        let t = Tmp::new("prebuilt-cr");
        let chunk = t.0.join("scene.js");
        t.write(
            "scene.js",
            "var x = 1 // @dcl/asset-packs is only a comment\n",
        );
        assert!(!chunk_requires(&chunk, "@dcl/asset-packs"));
        t.write(
            "scene.js",
            r#"var e=require("@dcl/asset-packs/dist/scene-entrypoint");"#,
        );
        assert!(chunk_requires(&chunk, "@dcl/asset-packs"));
    }
}
