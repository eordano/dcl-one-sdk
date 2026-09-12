use crate::esbuild::EsbuildOptions;
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
    /// `None` builds in place (the dev tree the watcher owns); a deploy builds
    /// into [`RELEASE_OUT`] so the two profiles never clobber one file and a
    /// publish never rewrites the tree it just fingerprinted.
    pub out_root: Option<PathBuf>,
    /// No progress narration (a page-driven publish tells its own story);
    /// errors and warnings still print.
    pub quiet: bool,
}

/// The release profile's artifact root, relative to the scene: where a deploy's
/// production bundle lands. Stale only when `--skip-build` skips the rebuild.
pub const RELEASE_OUT: &str = ".dcl-one/release";

/// `"" / "s"`, so a count and its noun agree.
pub fn plural(n: u64) -> &'static str {
    match n {
        1 => "",
        _ => "s",
    }
}

/// `<what> saved <path> (<elapsed>)`, the shape every emitted-chunk step uses.
pub fn saved(what: &str, root: &Path, out: &Path, started: Instant) -> String {
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

/// What a build needs once the entrypoint is generated: computed once per
/// build, or once per watch session.
pub struct Staged {
    pub generated: entrypoint::Generated,
    pub prebuilt: Option<prebuilt::Prebuilt>,
    pub sdk_opts: EsbuildOptions,
    pub scene_opts: EsbuildOptions,
    pub loader: split::Loader,
}

/// Generate the entrypoint and the runtime-chunk entry under `.dcl-one/`, and
/// resolve where every artifact lands under `art_root`.
pub fn stage(project: &Project, opts: &BuildOptions, art_root: &Path) -> Result<Staged> {
    let main = project.main_output()?;
    let tsconfig = project.tsconfig()?;
    let paths = split::ChunkPaths::of(&main);
    let generated = entrypoint::generate(
        project,
        opts.ignore_composite,
        opts.custom_entry_point,
        true,
    )?;
    split::write_generated(project, &generated.dir)?;
    split::write_marker(&generated.dir)?;
    let sdk_opts = sdk_chunk_options(
        project,
        &generated,
        art_root.join(&paths.sdk),
        &tsconfig,
        opts,
    )?;
    let scene_opts = EsbuildOptions {
        production: opts.production,
        entrypoint: generated.entrypoint.clone(),
        outfile: art_root.join(&paths.scene),
        tsconfig,
        aliases: vec![],
        externals: split::scene_externals(project),
    };
    let loader = split::Loader {
        outfile: art_root.join(&main),
        paths,
        smart_installed: false,
        max_composite_entity: generated.max_composite_entity,
        mp: entrypoint::authoritative_multiplayer(project),
    };
    Ok(Staged {
        generated,
        prebuilt: prebuilt::locate(project),
        sdk_opts,
        scene_opts,
        loader,
    })
}

/// The SDK chunk (installed prebuilt, or bundled from source), the scene chunk,
/// then the optional smart-item chunk: true when the loader should name it.
pub async fn emit_chunks(
    project: &Project,
    staged: &Staged,
    art_root: &Path,
    steps: &mut ux::Steps,
) -> Result<bool> {
    let paths = &staged.loader.paths;
    match &staged.prebuilt {
        Some(chunks) => {
            prebuilt::install(&chunks.core, &art_root.join(&paths.sdk))?;
            tracing::info!("prebuilt sdk chunk installed {}", paths.sdk);
            steps.done(format!("SDK chunk installed {} (prebuilt)", paths.sdk));
        }
        None => bundle_step("SDK chunk", project, &staged.sdk_opts, steps).await?,
    }
    bundle_step("Scene chunk", project, &staged.scene_opts, steps).await?;
    install_smart_chunk(
        project,
        staged.prebuilt.as_ref(),
        art_root,
        &paths.scene,
        &paths.smart,
    )
}

async fn bundle_step(
    what: &str,
    project: &Project,
    opts: &EsbuildOptions,
    steps: &mut ux::Steps,
) -> Result<()> {
    let started = Instant::now();
    esbuild::bundle(project, opts).await?;
    tracing::info!("{} saved {}", what.to_lowercase(), opts.outfile.display());
    steps.done(saved(what, &project.root, &opts.outfile, started));
    Ok(())
}

pub async fn build(opts: &BuildOptions) -> Result<Built> {
    let project = Project::load(&opts.dir)?;
    let main = project.main_output()?;
    project.tsconfig()?;
    let art_root = opts
        .out_root
        .clone()
        .unwrap_or_else(|| project.root.clone());
    let outfile = art_root.join(&main);
    if let Some(parent) = outfile.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
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

    let mut staged = stage(&project, opts, &art_root)?;

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

    staged.loader.smart_installed = emit_chunks(&project, &staged, &art_root, &mut steps).await?;
    staged.loader.write()?;
    let shown = ux::rel_to(&project.root, &outfile);
    steps.done(match staged.loader.smart_installed {
        true => format!("Loader stub saved {shown} (core + smart-item chunks)"),
        false => format!("Loader stub saved {shown}"),
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
            let checked = checked?;
            tracing::info!("type checking completed without errors");
            steps.done(match checked {
                Checked::Ran => format!("Type check passed ({})", ux::fmt_elapsed_tinted(took, "")),
                Checked::Unchanged => {
                    "Type check passed (unchanged since the last pass)".to_string()
                }
            });
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
    tsconfig: &Path,
    opts: &BuildOptions,
) -> Result<EsbuildOptions> {
    let mut aliases = esbuild::resolve_aliases(project)?;
    aliases.push((
        "~sdk/all-composites".to_string(),
        generated.dir.join("composite-slot.js"),
    ));
    aliases.push((
        "~sdk/script-utils".to_string(),
        generated.dir.join("script-utils.js"),
    ));
    Ok(EsbuildOptions {
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
    /// Did the last COMPLETED check report errors? An aborted one proves
    /// nothing about the newer edit, so only a completed one writes it.
    failing: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl BackgroundCheck {
    pub fn restart(&mut self, project: Project) {
        self.abort();
        let failing = self.failing.clone();
        self.running = Some(tokio::spawn(async move {
            use std::sync::atomic::Ordering;
            let started = Instant::now();
            match type_check(&project, Reloaded::Yes).await {
                Ok(_) => {
                    let was_failing = failing.swap(false, Ordering::Relaxed);
                    if let Some(line) = pass_note(was_failing, started.elapsed()) {
                        match was_failing {
                            true => ux::note_good(line),
                            false => ux::note_arrow(line),
                        }
                    }
                }
                Err(e) => {
                    failing.store(true, Ordering::Relaxed);
                    ux::report_watch(&e);
                }
            }
        }));
    }

    fn abort(&mut self) {
        if let Some(running) = self.running.take() {
            running.abort();
        }
    }
}

impl Drop for BackgroundCheck {
    fn drop(&mut self) {
        self.abort();
    }
}

/// Under a watcher the errors land a second AFTER the reload they describe,
/// which reads as "my edit was rejected" — so say outright that it was not.
fn fix_step(reloaded: Reloaded) -> &'static str {
    match reloaded {
        Reloaded::Yes => {
            "fix the type errors above (changes DID take effect \u{2014} the scene already reloaded)"
        }
        Reloaded::No => "fix the type errors above",
    }
}

/// Offered once per process: under `start` it would otherwise repeat under
/// every save, padding the errors with advice already declined.
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
        ux::fmt_elapsed_tinted(d, ux::RESTORE_DIM)
    })
}

/// The formatter is injected so the text can be asserted without a terminal.
fn pass_note_text(
    was_failing: bool,
    elapsed: std::time::Duration,
    fmt: impl Fn(std::time::Duration) -> String,
) -> Option<String> {
    let took = fmt(elapsed);
    match was_failing {
        true => Some(format!("type errors fixed ({took})")),
        false if ux::elapsed_is_notable(elapsed) => Some(format!("type check passed ({took})")),
        false => None,
    }
}

/// Whether the code this check covers is already running.
#[derive(Clone, Copy, PartialEq)]
pub enum Reloaded {
    Yes,
    No,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Checked {
    Ran,
    /// tsc did not run: see [`crate::check_stamp`].
    Unchanged,
}

pub async fn type_check(project: &Project, reloaded: Reloaded) -> Result<Checked> {
    let tsc = project.require_node_module("typescript/lib/tsc.js")?;
    if crate::check_stamp::unchanged(project, &tsc) {
        tracing::info!("type check skipped: nothing changed since the last pass");
        return Ok(Checked::Unchanged);
    }
    let node = require_node(
        "type checking",
        "to build without type checking, pass --skip-type-check",
    )?;
    let buildinfo = project.root.join(crate::check_stamp::TSBUILDINFO);
    if let Some(dir) = buildinfo.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let out = tokio::process::Command::new(node)
        .arg(&tsc)
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
        crate::check_stamp::forget(&project.root);
        let body = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        // TypeScript uses inverse video for source gutters, which produces bright
        // background blocks on dark terminals. Keep the gutter foreground-only.
        let body = body.replace("\x1b[7m", "\x1b[90m");
        let body = body.trim();
        let count = ts_error_count(body);
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
    crate::check_stamp::record(project, &tsc);
    Ok(Checked::Ran)
}

/// How many `error TSnnnn` diagnostics a tsc report carries. Pretty output
/// colours "error" and " TS2339: " separately, so the count reads through the
/// colour codes.
fn ts_error_count(body: &str) -> usize {
    crate::start::ansi::strip(body).matches("error TS").count()
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
    fn the_error_count_reads_through_tsc_colour_codes() {
        let plain = "src/a.ts(1,1): error TS2339: x\nsrc/a.ts(2,1): error TS2551: y\n";
        assert_eq!(ts_error_count(plain), 2);
        let pretty = "\x1b[96msrc/a.ts\x1b[0m:\x1b[93m9\x1b[0m - \x1b[91merror\x1b[0m\x1b[90m TS2339: \x1b[0mx\n";
        assert_eq!(ts_error_count(pretty), 1);
        assert_eq!(ts_error_count("Found 0 errors"), 0);
    }

    #[test]
    fn a_recovered_check_says_so_and_a_quick_pass_stays_quiet() {
        let note =
            |failing, ms| pass_note_text(failing, Duration::from_millis(ms), ux::fmt_elapsed);
        assert_eq!(
            note(true, 120),
            Some("type errors fixed (120 ms)".to_string())
        );
        assert_eq!(
            note(true, 3_000),
            Some("type errors fixed (3.00 sec)".to_string())
        );
        assert_eq!(note(false, 20), None);
        assert_eq!(
            note(false, 120),
            Some("type check passed (120 ms)".to_string())
        );
        assert_eq!(
            note(false, 3_000),
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
