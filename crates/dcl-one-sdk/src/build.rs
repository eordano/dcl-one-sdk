use crate::ux::{self, TrySteps, UserError};
use crate::workspace::Workspace;
use crate::{entrypoint, esbuild, prebuilt, scene::Project, split};
use anyhow::Result;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Instant;

pub struct BuildOptions {
    pub dir: PathBuf,
    pub production: bool,
    pub ignore_composite: bool,
    pub custom_entry_point: bool,
    pub skip_type_check: bool,
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
    let outfile = project.root.join(&main);
    let (sdk_rel, scene_rel) = split::chunk_rel_paths(&main);
    let smart_rel = split::smart_chunk_rel_path(&main);
    // Collected before the step count so the entity-names step can be counted
    // only when it will actually report — a scene whose composite names nothing
    // must not be told it has a step it never runs.
    let entity_names = if opts.ignore_composite {
        Default::default()
    } else {
        crate::entity_names::collect(&project.root)
    };
    let base_steps = if opts.skip_type_check { 4 } else { 5 };
    let mut steps = ux::Steps::new(base_steps + usize::from(!entity_names.is_empty()));

    let generated = entrypoint::generate(
        &project,
        opts.ignore_composite,
        opts.custom_entry_point,
        true,
    )?;
    split::write_generated(&project, &generated.dir)?;
    split::write_marker(&generated.dir)?;

    // The vendored toolchain ships the SDK runtime chunk prebuilt: it is
    // scene-independent, so re-deriving it here would spend seconds of rolldown
    // to reproduce bytes we already have. An npm-installed scene has no
    // prebuilt chunk and takes the source path below, unchanged.
    let started = Instant::now();
    let prebuilt = prebuilt::locate(&project);
    match &prebuilt {
        Some(chunks) => {
            prebuilt::install(&chunks.core, &project.root.join(&sdk_rel))?;
            tracing::info!("prebuilt sdk chunk installed {sdk_rel}");
            steps.done(format!("SDK chunk installed {sdk_rel} (prebuilt)"));
        }
        None => {
            let sdk_opts = sdk_chunk_options(&project, &generated, &sdk_rel, &tsconfig, opts)?;
            esbuild::bundle(&project, &sdk_opts).await?;
            tracing::info!("sdk chunk saved {}", sdk_opts.outfile.display());
            steps.done(format!(
                "SDK chunk saved {} ({})",
                ux::rel_to(&project.root, &sdk_opts.outfile),
                ux::fmt_elapsed(started.elapsed())
            ));
        }
    }

    let scene_opts = esbuild::EsbuildOptions {
        production: opts.production,
        entrypoint: generated.entrypoint.clone(),
        outfile: project.root.join(&scene_rel),
        tsconfig,
        aliases: vec![],
        externals: split::scene_externals(&project),
    };
    let started = Instant::now();
    esbuild::bundle(&project, &scene_opts).await?;
    tracing::info!("scene chunk saved {}", scene_opts.outfile.display());
    steps.done(format!(
        "Scene chunk saved {} ({})",
        ux::rel_to(&project.root, &scene_opts.outfile),
        ux::fmt_elapsed(started.elapsed())
    ));

    let smart_installed = install_smart_chunk(&project, prebuilt.as_ref(), &scene_rel, &smart_rel)?;
    split::write_loader_stub(
        &outfile,
        &sdk_rel,
        smart_installed.then_some(smart_rel.as_str()),
        &scene_rel,
        generated.max_composite_entity,
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

    // Ahead of main.crdt on purpose. The names are read straight out of the
    // composite JSON and owe nothing to the crdt encoder, so a Hub scene whose
    // composite the native generator cannot encode — an asset-packs component
    // with its own jsonSchema is enough — still gets a correct enum instead of
    // a stale one. It also has to land before the type check below, which is
    // what fails when a scene imports EntityNames and the file is out of date.
    if !opts.ignore_composite {
        match crate::entity_names::write(&project.root, &entity_names) {
            Ok(Some(n)) => steps.done(format!(
                "{} regenerated ({n} name{})",
                crate::entity_names::OUTPUT_PATH,
                if n == 1 { "" } else { "s" }
            )),
            Ok(None) => {}
            // Never fail the build over the names file: a scene that does not
            // import it does not care, and one that does gets a clear error
            // from the type check a few lines below.
            Err(e) => ux::note(&format!(
                "could not write {}: {e}",
                crate::entity_names::OUTPUT_PATH
            )),
        }
    }

    match crate::data_layer::regenerate_main_crdt(&project.root, opts.ignore_composite).await? {
        Some(crate::data_layer::CrdtRegen::Native(n)) => steps.done(format!(
            "main.crdt regenerated ({n} composite{})",
            if n == 1 { "" } else { "s" }
        )),
        Some(crate::data_layer::CrdtRegen::NodeDataLayer) => {
            steps.done("main.crdt regenerated via the node data-layer")
        }
        None => steps.done("main.crdt skipped (no composite)"),
    }

    if opts.skip_type_check {
        ux::note("type check skipped (--skip-type-check)");
    } else {
        // The slowest step by far on a real scene, and the only one that used
        // to run with no output at all — several seconds of dead terminal read
        // as a hang.
        let progress = ux::Slow::start("type checking");
        let checked = type_check(&project).await;
        progress.finish();
        checked?;
        tracing::info!("type checking completed without errors");
        steps.done("Type check passed");
    }

    Ok(Built { project, outfile })
}

/// The source-path SDK chunk: one rolldown pass over the registry of everything
/// installed. Only reached when the scene has no prebuilt chunk — an npm
/// install, or a vendored tree from before the prebuilt split.
pub fn sdk_chunk_options(
    project: &Project,
    generated: &entrypoint::Generated,
    sdk_rel: &str,
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
        outfile: project.root.join(sdk_rel),
        tsconfig: tsconfig.to_path_buf(),
        aliases,
        externals: vec![],
    })
}

/// Install the prebuilt smart-item chunk if this scene uses smart items, and
/// clear a stale one if it no longer does. Returns whether the loader should
/// name it.
///
/// Only the prebuilt path has a smart chunk: on the source path
/// `@dcl/asset-packs` is bundled into the single SDK chunk, exactly as before.
pub fn install_smart_chunk(
    project: &Project,
    prebuilt: Option<&prebuilt::Prebuilt>,
    scene_rel: &str,
    smart_rel: &str,
) -> Result<bool> {
    let Some(chunks) = prebuilt else {
        return Ok(false);
    };
    let scene_chunk = project.root.join(scene_rel);
    if !prebuilt::scene_needs_smart_chunk(project, &scene_chunk) {
        prebuilt::remove_stale_smart_chunk(&project.root, smart_rel);
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
    prebuilt::install(smart, &project.root.join(smart_rel))?;
    tracing::info!("prebuilt smart-item chunk installed {smart_rel}");
    Ok(true)
}

/// A type check that runs beside the watch loop instead of in front of it, so a
/// rebuild reloads the scene immediately and type errors arrive as soon as tsc
/// has an answer. Only the newest edit's check matters, so starting one aborts
/// any still running; `type_check` spawns tsc with `kill_on_drop`, so the abort
/// reaches the process rather than orphaning it.
#[derive(Default)]
pub struct BackgroundCheck {
    running: Option<tokio::task::JoinHandle<()>>,
}

impl BackgroundCheck {
    pub fn restart(&mut self, project: Project) {
        if let Some(previous) = self.running.take() {
            previous.abort();
        }
        self.running = Some(tokio::spawn(async move {
            let started = std::time::Instant::now();
            match type_check(&project).await {
                // Silence is the pass signal; only a check slow enough to have
                // been felt is worth a line of its own.
                Ok(()) if started.elapsed() >= std::time::Duration::from_secs(1) => {
                    crate::ux::note(format!(
                        "type check passed ({})",
                        crate::ux::fmt_elapsed(started.elapsed())
                    ));
                }
                Ok(()) => {}
                Err(e) => crate::ux::report_watch(&e),
            }
        }));
    }
}

impl Drop for BackgroundCheck {
    fn drop(&mut self) {
        if let Some(running) = self.running.take() {
            running.abort();
        }
    }
}

pub async fn type_check(project: &Project) -> Result<()> {
    let tsc = project.require_node_module("typescript/lib/tsc.js")?;
    let node = node_bin()?;
    let out = tokio::process::Command::new(node)
        .arg(tsc)
        .args(["-p", "tsconfig.json", "--noEmit"])
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
            1 => "type check failed \u{2014} 1 error".to_string(),
            n => format!("type check failed \u{2014} {n} errors"),
        };
        return Err(UserError::new(
            what,
            TrySteps::one("fix the type errors above").and(
                "to preview while iterating, pass --skip-type-check (the bundle was already saved)",
            ),
        )
        .why(body)
        .into());
    }
    Ok(())
}

pub fn find_node() -> Option<PathBuf> {
    find_on_path(&["node", "node.exe"])
}

pub fn find_on_path(names: &[&str]) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for name in names {
            let p = dir.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

fn node_bin() -> Result<PathBuf> {
    match find_node() {
        Some(p) => Ok(p),
        None => Err(UserError::new(
            "node is required for type checking but is not on PATH",
            TrySteps::one("install Node.js or add it to PATH")
                .and("to build without type checking, pass --skip-type-check"),
        )
        .into()),
    }
}
