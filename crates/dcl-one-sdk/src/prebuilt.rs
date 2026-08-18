//! Prebuilt SDK runtime chunks: shipped in the vendored blob, not rebuilt per
//! scene.
//!
//! The SDK runtime chunk is *scene-independent*. Two scenes — one importing
//! three `@dcl/ecs` symbols, one importing react-ecs UI + tweens + audio +
//! animator + players + network — produce byte-identical `bin/sdk-runtime.js`
//! (463,133 B, sha256 ba5189ef…). It is keyed only on which `@dcl/*` packages
//! are installed, never on what the scene imports. So the 3.64 MB of SDK
//! JavaScript the base blob used to ship existed for one purpose: letting
//! rolldown re-derive the same artifact on every build. Building it once, at
//! blob-build time, is the whole idea here.
//!
//! Two chunks, not one:
//!
//! * **core** — always installed. The registry of everything under
//!   `@dcl/{sdk,ecs,ecs-math,react-ecs}` + react, exactly what
//!   [`crate::split::core_registry_keys`] lists.
//! * **smart** — installed only when the scene actually uses smart items. It
//!   carries `@dcl/asset-packs`, its scene entrypoint, and the real
//!   `~sdk/script-utils` runtime, and it resolves everything else through the
//!   core chunk's registry.
//!
//! Splitting them fixes a real defect, not just a size: `write_script_utils`
//! inlines the smart-item runtime whenever `@dcl/asset-packs` merely *resolves*,
//! which grew the single chunk from 463,133 B to 601,100 B (+137,967 B, +30%)
//! for every production bundle — including scenes with no composite and no
//! smart item anywhere. Putting `@dcl/asset-packs` in the base blob (so
//! smart-item scenes type-check and bundle without a 12 MB editor install)
//! would have made that inflation universal. With the split it is paid only by
//! scenes that use smart items, and even they pay slightly less than before
//! (613 KB across two chunks vs 604 KB in one is +1.6%, against −140 KB for
//! every scene that does not).
//!
//! The chunks live *inside* the vendored `@dcl/sdk` at
//! `node_modules/@dcl/sdk/prebuilt/`. That is deliberate: they are only valid
//! for the `@dcl/sdk` version they were built from, and putting them in that
//! package means an `npm install` that replaces `@dcl/sdk` removes them in the
//! same step, which flips the build back to the source path atomically instead
//! of leaving a stale chunk behind.

use crate::esbuild::EsbuildOptions;
use crate::scene::Project;
use crate::ux::{TrySteps, UserError};
use crate::{entrypoint, split};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Package-relative home of the chunks, inside the vendored `@dcl/sdk`.
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

/// The prebuilt chunks of this scene's installed toolchain, if it has them.
///
/// `None` means the scene has a source `node_modules` (an npm install, or a
/// blob predating this change) and the SDK chunk is bundled from source, as
/// before.
pub fn locate(project: &Project) -> Option<Prebuilt> {
    let core = project.node_module(CORE_FILE)?;
    Some(Prebuilt {
        core,
        smart: project.node_module(SMART_FILE),
    })
}

/// Does this scene need the smart-item chunk?
///
/// [`Project::is_editor_scene`] is the primary signal: a composite carrying
/// runtime `asset-packs::` components makes the generated entrypoint call
/// `initAssetPacks`. It is not the only one — a scene can import
/// `@dcl/asset-packs/dist/scene-entrypoint` from its own source with no
/// composite at all (`0,0-cube-spawner` in decentraland/sdk7-test-scenes does
/// exactly that) — so the built scene chunk is also consulted. That second test
/// is exact rather than predictive: `@dcl/asset-packs` is an external of the
/// scene chunk, so the specifier survives verbatim into the emitted bundle
/// precisely when something reaches it.
pub fn scene_needs_smart_chunk(project: &Project, scene_chunk: &Path) -> bool {
    project.is_editor_scene() || chunk_requires(scene_chunk, "@dcl/asset-packs")
}

fn chunk_requires(chunk: &Path, package: &str) -> bool {
    let Ok(code) = std::fs::read_to_string(chunk) else {
        return false;
    };
    code.contains(&format!("require(\"{package}")) || code.contains(&format!("require('{package}"))
}

/// Put a prebuilt chunk in place under the scene's `main` directory.
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

/// Remove a smart-item chunk left behind by an earlier build of the same scene.
///
/// Without this, deleting the last smart item from a scene leaves a stale
/// `sdk-smart-items.js` in `bin/`, which the loader stub no longer names but
/// `deploy` would still upload.
pub fn remove_stale_smart_chunk(root: &Path, smart_rel: &str) {
    let _ = std::fs::remove_file(root.join(smart_rel));
}

/// Build both chunks from a scene whose `node_modules` is the full install tree
/// — this is what `scripts/build-base-blob.py` calls through the hidden
/// `vendor-chunks` subcommand, and the only place either chunk is ever
/// produced.
///
/// The two passes differ only in entry module and externals:
///
/// * core: entry is the registry of [`split::core_registry_keys`], nothing is
///   external, `~sdk/script-utils` is aliased to the no-op stub.
/// * smart: entry is the registry of [`split::smart_registry_keys`],
///   everything the core chunk owns is external, and `~sdk/script-utils` is
///   aliased to the *real* `@dcl/sdk-commands` runtime so exactly one copy is
///   bundled and the registry entry and asset-packs' own internal import land
///   on the same module instance.
///
/// Both chunks are then checked: every `require()` a chunk emits must be
/// `~system/*` or a key the loader will have by the time that chunk is
/// evaluated. This is what catches the class of bug that was invisible while
/// asset-packs shared a chunk with the SDK — `@dcl/sdk/platform` and
/// `@dcl/sdk/text-codec` are required by asset-packs, were not registry keys,
/// and would have thrown "not in the sdk runtime registry" at scene start.
pub async fn build_chunks(dir: &Path, out_core: &Path, out_smart: &Path) -> Result<()> {
    let project = Project::load(dir)?;
    let tsconfig = project.tsconfig()?;
    let work = project.root.join(".dcl-one");
    std::fs::create_dir_all(&work).with_context(|| format!("creating {}", work.display()))?;

    let core_keys = split::core_registry_keys(&project);
    let smart_keys = split::smart_registry_keys();

    std::fs::write(
        work.join("composite-slot.js"),
        "export const compositeFromLoader = {}\n",
    )?;
    let stub = work.join("script-utils-stub.js");
    std::fs::write(&stub, entrypoint::SCRIPT_UTILS_STUB)?;
    let core_entry = work.join("core-registry.js");
    std::fs::write(&core_entry, split::registry_module(&core_keys))?;

    let mut core_aliases = crate::esbuild::resolve_aliases(&project)?;
    core_aliases.push((
        "~sdk/all-composites".to_string(),
        work.join("composite-slot.js"),
    ));
    core_aliases.push(("~sdk/script-utils".to_string(), stub));
    crate::esbuild::bundle(
        &project,
        &EsbuildOptions {
            production: true,
            entrypoint: core_entry,
            outfile: out_core.to_path_buf(),
            tsconfig: tsconfig.clone(),
            aliases: core_aliases,
            externals: vec![],
        },
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
    let real_utils = work.join("script-utils.js");
    std::fs::write(&real_utils, script_utils)?;
    let smart_entry = work.join("smart-registry.js");
    std::fs::write(&smart_entry, split::registry_module(smart_keys))?;

    let mut smart_aliases: Vec<(String, PathBuf)> = Vec::new();
    if let Some(ap) = project.node_module("@dcl/asset-packs") {
        smart_aliases.push(("@dcl/asset-packs".to_string(), ap));
    }
    smart_aliases.push(("~sdk/script-utils".to_string(), real_utils));
    crate::esbuild::bundle(
        &project,
        &EsbuildOptions {
            production: true,
            entrypoint: smart_entry,
            outfile: out_smart.to_path_buf(),
            tsconfig,
            aliases: smart_aliases,
            externals: split::smart_externals(),
        },
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
        let dir = std::env::temp_dir().join(format!("dcl-one-prebuilt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let chunk = dir.join("smart.js");
        std::fs::write(
            &chunk,
            r#"require("~system/EngineApi");require("@dcl/sdk/ecs")"#,
        )
        .unwrap();
        assert!(verify_requires(&chunk, &["@dcl/sdk/ecs"]).is_ok());
        assert!(verify_requires(&chunk, &[]).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn chunk_requires_matches_only_a_real_specifier() {
        let dir = std::env::temp_dir().join(format!("dcl-one-prebuilt-cr-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let chunk = dir.join("scene.js");
        std::fs::write(&chunk, "var x = 1 // @dcl/asset-packs is only a comment\n").unwrap();
        assert!(!chunk_requires(&chunk, "@dcl/asset-packs"));
        std::fs::write(
            &chunk,
            r#"var e=require("@dcl/asset-packs/dist/scene-entrypoint");"#,
        )
        .unwrap();
        assert!(chunk_requires(&chunk, "@dcl/asset-packs"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
