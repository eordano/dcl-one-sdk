use crate::ux::{self, TrySteps, UserError};
use crate::workspace::Workspace;
use crate::{entrypoint, esbuild, prebuilt, scene::Project, split};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Instant;

pub struct BuildOptions {
    pub dir: PathBuf,
    pub production: bool,
    pub ignore_composite: bool,
    pub custom_entry_point: bool,
    pub skip_type_check: bool,
    /// Where the bundle artifacts land. `None` builds in place — the dev
    /// tree the watcher owns. A deploy builds into [`RELEASE_OUT`] instead:
    /// the debug/release split rustc keeps, so the two profiles stop
    /// clobbering one file and a publish stops rewriting the very tree it
    /// just fingerprinted.
    pub out_root: Option<PathBuf>,
    /// No progress narration: for a build whose story a page already tells
    /// (a page-driven publish). Errors and warnings still print.
    pub quiet: bool,
}

/// The release profile's artifact root, relative to the scene: where a
/// deploy's production bundle lands, and the first place `deploy::prepare`
/// reads a payload file from. Stale only when `--skip-build` skips the
/// rebuild that normally refreshes it — the same hazard a stale in-tree
/// bundle always had.
pub const RELEASE_OUT: &str = ".dcl-one/release";

/// `"" / "s"`, so a count and its noun agree.
pub fn plural(n: u64) -> &'static str {
    match n {
        1 => "",
        _ => "s",
    }
}

/// `<what> saved <path> (<elapsed>)`, the shape every emitted-chunk step uses.
pub fn saved(
    what: &str,
    root: &std::path::Path,
    out: &std::path::Path,
    started: Instant,
) -> String {
    format!(
        "{what} saved {} ({})",
        ux::rel_to(root, out),
        ux::fmt_elapsed_tinted(started.elapsed(), "")
    )
}

pub struct Built {
    pub project: Project,
    pub outfile: PathBuf,
}

pub fn member_options(opts: &BuildOptions, project: &Project) -> BuildOptions {
    BuildOptions {
        dir: project.root.clone(),
        production: opts.production,
        ignore_composite: opts.ignore_composite,
        custom_entry_point: opts.custom_entry_point,
        skip_type_check: opts.skip_type_check,
        // Each member's release artifacts land under its own root.
        out_root: opts
            .out_root
            .as_ref()
            .map(|_| project.root.join(RELEASE_OUT)),
        quiet: opts.quiet,
    }
}

pub async fn build_workspace(ws: &Workspace, opts: &BuildOptions) -> Result<()> {
    for (i, project) in ws.projects.iter().enumerate() {
        if let Some(header) = ws.member_header(i) {
            ux::note(header);
        }
        build(&member_options(opts, project)).await?;
    }
    Ok(())
}

pub async fn build(opts: &BuildOptions) -> Result<Built> {
    let project = Project::load(&opts.dir)?;
    let main = project.main_output()?;
    let tsconfig = project.tsconfig()?;
    let art_root = opts
        .out_root
        .clone()
        .unwrap_or_else(|| project.root.clone());
    let outfile = art_root.join(&main);
    if let Some(parent) = outfile.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let (sdk_rel, scene_rel) = split::chunk_rel_paths(&main);
    let smart_rel = split::smart_chunk_rel_path(&main);
    let entity_names = if opts.ignore_composite {
        Default::default()
    } else {
        crate::entity_names::collect(&project.root)
    };
    let base_steps = if opts.skip_type_check { 4 } else { 5 };
    let mut steps = match opts.quiet {
        true => ux::Steps::silent(),
        false => ux::Steps::new(base_steps + usize::from(!entity_names.is_empty())),
    };

    let generated = entrypoint::generate(
        &project,
        opts.ignore_composite,
        opts.custom_entry_point,
        true,
    )?;
    split::write_generated(&project, &generated.dir)?;
    split::write_marker(&generated.dir)?;

    let entity_names_written = match opts.ignore_composite {
        true => Ok(None),
        false => crate::entity_names::write(&project.root, &entity_names),
    };

    let checking = match opts.skip_type_check {
        true => None,
        false => {
            let project = project.clone();
            Some(tokio::spawn(async move {
                let started = Instant::now();
                (type_check(&project, Reloaded::No).await, started.elapsed())
            }))
        }
    };

    let started = Instant::now();
    let prebuilt = prebuilt::locate(&project);
    match &prebuilt {
        Some(chunks) => {
            prebuilt::install(&chunks.core, &art_root.join(&sdk_rel))?;
            tracing::info!("prebuilt sdk chunk installed {sdk_rel}");
            steps.done(format!("SDK chunk installed {sdk_rel} (prebuilt)"));
        }
        None => {
            let sdk_opts = sdk_chunk_options(
                &project,
                &generated,
                art_root.join(&sdk_rel),
                &tsconfig,
                opts,
            )?;
            esbuild::bundle(&project, &sdk_opts).await?;
            tracing::info!("sdk chunk saved {}", sdk_opts.outfile.display());
            steps.done(saved(
                "SDK chunk",
                &project.root,
                &sdk_opts.outfile,
                started,
            ));
        }
    }

    let scene_opts = esbuild::EsbuildOptions {
        production: opts.production,
        entrypoint: generated.entrypoint.clone(),
        outfile: art_root.join(&scene_rel),
        tsconfig,
        aliases: vec![],
        externals: split::scene_externals(&project),
    };
    let started = Instant::now();
    esbuild::bundle(&project, &scene_opts).await?;
    tracing::info!("scene chunk saved {}", scene_opts.outfile.display());
    steps.done(saved(
        "Scene chunk",
        &project.root,
        &scene_opts.outfile,
        started,
    ));

    let smart_installed = install_smart_chunk(
        &project,
        prebuilt.as_ref(),
        &art_root,
        &scene_rel,
        &smart_rel,
    )?;
    split::write_loader_stub(
        &outfile,
        &sdk_rel,
        smart_installed.then_some(smart_rel.as_str()),
        &scene_rel,
        generated.max_composite_entity,
        crate::entrypoint::authoritative_multiplayer(&project),
    )?;
    tracing::info!("loader stub saved {}", outfile.display());
    steps.done(if smart_installed {
        format!(
            "Loader stub saved {} (core + smart-item chunks)",
            ux::rel_to(&project.root, &outfile)
        )
    } else {
        format!("Loader stub saved {}", ux::rel_to(&project.root, &outfile))
    });

    match entity_names_written {
        Ok(Some(n)) => steps.done(format!(
            "{} regenerated ({n} name{})",
            crate::entity_names::OUTPUT_PATH,
            plural(n as u64)
        )),
        Ok(None) => {}
        Err(e) => ux::note(format!(
            "could not write {}: {e}",
            crate::entity_names::OUTPUT_PATH
        )),
    }

    match crate::data_layer::regenerate_main_crdt(&project.root, opts.ignore_composite).await? {
        Some(crate::data_layer::CrdtRegen::Native(n)) => steps.done(format!(
            "main.crdt regenerated ({n} composite{})",
            plural(n)
        )),
        Some(crate::data_layer::CrdtRegen::NodeDataLayer) => {
            steps.done("main.crdt regenerated via the node data-layer")
        }
        None => steps.done("main.crdt skipped (no composite)"),
    }

    match checking {
        None => {
            if !opts.quiet {
                ux::note("type check skipped (--skip-type-check)");
            }
        }
        Some(handle) => {
            let progress = (!opts.quiet).then(|| ux::Slow::start("type checking"));
            let (checked, took) = handle.await.map_err(|e| match e.try_into_panic() {
                Ok(panic) => std::panic::resume_unwind(panic),
                Err(e) => anyhow::anyhow!("type check task: {e}"),
            })?;
            if let Some(progress) = progress {
                progress.finish();
            }
            checked?;
            tracing::info!("type checking completed without errors");
            steps.done(format!(
                "Type check passed ({})",
                ux::fmt_elapsed_tinted(took, "")
            ));
        }
    }

    Ok(Built { project, outfile })
}

/// The source-path SDK chunk: one rolldown pass over everything installed. Only
/// reached when the scene has no prebuilt chunk.
pub fn sdk_chunk_options(
    project: &Project,
    generated: &entrypoint::Generated,
    outfile: PathBuf,
    tsconfig: &std::path::Path,
    opts: &BuildOptions,
) -> Result<esbuild::EsbuildOptions> {
    let mut aliases = esbuild::resolve_aliases(project)?;
    aliases.push((
        "~sdk/all-composites".to_string(),
        generated.dir.join("composite-slot.js"),
    ));
    aliases.push((
        "~sdk/script-utils".to_string(),
        generated.dir.join("script-utils.js"),
    ));
    Ok(esbuild::EsbuildOptions {
        production: opts.production,
        entrypoint: generated.dir.join("sdk-runtime-entry.js"),
        outfile,
        tsconfig: tsconfig.to_path_buf(),
        aliases,
        externals: vec![],
    })
}

/// Install the prebuilt smart-item chunk if this scene uses smart items, clear
/// a stale one if it no longer does, and say whether the loader should name it.
/// Only the prebuilt path has one: the source path bundles `@dcl/asset-packs`
/// into the single SDK chunk.
pub fn install_smart_chunk(
    project: &Project,
    prebuilt: Option<&prebuilt::Prebuilt>,
    art_root: &Path,
    scene_rel: &str,
    smart_rel: &str,
) -> Result<bool> {
    let Some(chunks) = prebuilt else {
        return Ok(false);
    };
    let scene_chunk = art_root.join(scene_rel);
    if !prebuilt::scene_needs_smart_chunk(project, &scene_chunk) {
        prebuilt::remove_stale_smart_chunk(art_root, smart_rel);
        return Ok(false);
    }
    let Some(smart) = &chunks.smart else {
        return Err(UserError::new(
            "this scene uses smart items but the vendored toolchain has no smart-item chunk",
            TrySteps::one(
                "re-install the vendored toolchain with dcl-one-sdk init --node-modules-only",
            ),
        )
        .why(format!(
            "{} does not exist",
            project
                .root
                .join("node_modules")
                .join(prebuilt::SMART_FILE)
                .display()
        ))
        .into());
    };
    prebuilt::install(smart, &art_root.join(smart_rel))?;
    tracing::info!("prebuilt smart-item chunk installed {smart_rel}");
    Ok(true)
}

/// A type check that runs beside the watch loop rather than in front of it, so
/// a save reloads immediately. Only the newest edit matters, so starting one
/// aborts any still running; `type_check` uses `kill_on_drop`, so the abort
/// reaches the tsc process rather than orphaning it.
#[derive(Default)]
pub struct BackgroundCheck {
    running: Option<tokio::task::JoinHandle<()>>,
    /// Did the last check that ran to completion report errors? Lets a recovery
    /// retract the failure still on screen. Only a COMPLETED check writes it —
    /// an aborted one proves nothing about the newer edit.
    failing: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl BackgroundCheck {
    pub fn restart(&mut self, project: Project) {
        if let Some(previous) = self.running.take() {
            previous.abort();
        }
        let failing = self.failing.clone();
        self.running = Some(tokio::spawn(async move {
            use std::sync::atomic::Ordering;
            let started = std::time::Instant::now();
            match type_check(&project, Reloaded::Yes).await {
                Ok(()) => {
                    let was_failing = failing.swap(false, Ordering::Relaxed);
                    if let Some(line) = pass_note(was_failing, started.elapsed()) {
                        match was_failing {
                            true => crate::ux::note_good(line),
                            false => crate::ux::note_arrow(line),
                        }
                    }
                }
                Err(e) => {
                    failing.store(true, Ordering::Relaxed);
                    crate::ux::report_watch(&e);
                }
            }
        }));
    }
}

/// The first thing to try under a failed check. Under a watcher the errors land
/// a second AFTER the reload they describe, which reads as "my edit was
/// rejected" — so say outright that it was not.
fn fix_step(reloaded: Reloaded) -> &'static str {
    match reloaded {
        Reloaded::Yes => {
            "fix the type errors above (changes DID take effect \u{2014} the scene already reloaded)"
        }
        Reloaded::No => "fix the type errors above",
    }
}

/// The `--skip-type-check` escape hatch, offered once per process: under
/// `start` it would otherwise repeat under every save, padding the errors with
/// advice already declined. `build` exits on its first failure, so it sees it.
fn skip_type_check_hint() -> Option<&'static str> {
    static OFFERED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    match OFFERED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        true => None,
        false => Some(
            "to preview while iterating, pass --skip-type-check (the bundle was already saved)",
        ),
    }
}

/// What a passing check should say, or None to stay quiet. A recovery always
/// speaks, since nothing else retracts the errors still on screen.
fn pass_note(was_failing: bool, elapsed: std::time::Duration) -> Option<String> {
    pass_note_text(was_failing, elapsed, |d| {
        crate::ux::fmt_elapsed_tinted(d, crate::ux::RESTORE_DIM)
    })
}

/// The formatter is injected so the text can be asserted without depending on
/// whether the test harness happens to own a terminal.
fn pass_note_text(
    was_failing: bool,
    elapsed: std::time::Duration,
    fmt: impl Fn(std::time::Duration) -> String,
) -> Option<String> {
    let took = fmt(elapsed);
    match was_failing {
        true => Some(format!("type errors fixed ({took})")),
        false if crate::ux::elapsed_is_notable(elapsed) => {
            Some(format!("type check passed ({took})"))
        }
        false => None,
    }
}

impl Drop for BackgroundCheck {
    fn drop(&mut self) {
        if let Some(running) = self.running.take() {
            running.abort();
        }
    }
}

/// Where tsc keeps what it learned last run. Under a dot-dir so the watcher
/// skips it and tsc cannot feed back into the rebuild that started it.
const TSBUILDINFO: &str = ".dcl-cache/tsbuildinfo";

/// Whether the code this check covers is already running.
#[derive(Clone, Copy, PartialEq)]
pub enum Reloaded {
    Yes,
    No,
}

pub async fn type_check(project: &Project, reloaded: Reloaded) -> Result<()> {
    let tsc = project.require_node_module("typescript/lib/tsc.js")?;
    let node = require_node(
        "type checking",
        "to build without type checking, pass --skip-type-check",
    )?;
    let buildinfo = project.root.join(TSBUILDINFO);
    if let Some(dir) = buildinfo.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let out = tokio::process::Command::new(node)
        .arg(tsc)
        .args(["-p", "tsconfig.json", "--noEmit"])
        .args(["--incremental", "--tsBuildInfoFile"])
        .arg(&buildinfo)
        .args(if std::io::IsTerminal::is_terminal(&std::io::stderr()) {
            &[] as &[&str]
        } else {
            &["--pretty", "false"]
        })
        .current_dir(&project.root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| {
            anyhow::Error::from(
                UserError::new(
                    "could not start the TypeScript compiler (node_modules/typescript)",
                    TrySteps::one("run dcl-one-sdk init --node-modules-only to restore the vendored node_modules (or npm install)")
                        .and("to build without type checking, pass --skip-type-check"),
                )
                .caused_by(e),
            )
        })?;
    if !out.status.success() {
        let body = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let body = body.trim();
        let count = body.matches("error TS").count();
        let what = match count {
            0 => "type check failed".to_string(),
            n => format!("type check failed \u{2014} {n} error{}", plural(n as u64)),
        };
        let mut steps = TrySteps::one(fix_step(reloaded));
        if let Some(hint) = skip_type_check_hint() {
            steps = steps.and(hint);
        }
        return Err(UserError::new(what, steps).why(body).into());
    }
    Ok(())
}

pub fn find_node() -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .flat_map(|dir| ["node", "node.exe"].map(|name| dir.join(name)))
        .find(|p| p.is_file())
}

/// `purpose` completes "node is required for _ but is not on PATH"; `without`
/// is the second try-step, naming the flag that skips the work needing node.
pub fn require_node(purpose: &str, without: &str) -> Result<PathBuf> {
    match find_node() {
        Some(p) => Ok(p),
        None => Err(UserError::new(
            format!("node is required for {purpose} but is not on PATH"),
            TrySteps::one("install Node.js or add it to PATH").and(without),
        )
        .into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn a_recovered_check_says_so_and_a_quick_pass_stays_quiet() {
        assert_eq!(
            pass_note_text(true, Duration::from_millis(120), crate::ux::fmt_elapsed),
            Some("type errors fixed (120 ms)".to_string())
        );
        assert_eq!(
            pass_note_text(true, Duration::from_secs(3), crate::ux::fmt_elapsed),
            Some("type errors fixed (3.00 sec)".to_string())
        );
        assert_eq!(
            pass_note_text(false, Duration::from_millis(20), crate::ux::fmt_elapsed),
            None
        );
        assert_eq!(
            pass_note_text(false, Duration::from_millis(120), crate::ux::fmt_elapsed),
            Some("type check passed (120 ms)".to_string())
        );
        assert_eq!(
            pass_note_text(false, Duration::from_secs(3), crate::ux::fmt_elapsed),
            Some("type check passed (3.00 sec)".to_string())
        );
    }

    #[test]
    fn the_skip_type_check_hint_is_offered_once() {
        assert!(skip_type_check_hint().is_some());
        assert!(skip_type_check_hint().is_none());
        assert!(skip_type_check_hint().is_none());
    }

    #[test]
    fn a_watched_failure_says_the_change_landed_anyway() {
        assert!(fix_step(Reloaded::Yes).contains("changes DID take effect"));
        assert_eq!(fix_step(Reloaded::No), "fix the type errors above");
    }
}
