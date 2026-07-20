use crate::ux::{self, TrySteps, UserError};
use crate::workspace::Workspace;
use crate::{entrypoint, esbuild, scene::Project, split};
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
    let mut steps = ux::Steps::new(if opts.skip_type_check { 4 } else { 5 });

    let generated = entrypoint::generate(
        &project,
        opts.ignore_composite,
        opts.custom_entry_point,
        true,
    )?;
    split::write_generated(&project, &generated.dir)?;
    split::write_marker(&generated.dir)?;

    let mut sdk_aliases = esbuild::resolve_aliases(&project)?;
    sdk_aliases.push((
        "~sdk/all-composites".to_string(),
        generated.dir.join("composite-slot.js"),
    ));
    sdk_aliases.push((
        "~sdk/script-utils".to_string(),
        generated.dir.join("script-utils.js"),
    ));
    let sdk_opts = esbuild::EsbuildOptions {
        production: opts.production,
        entrypoint: generated.dir.join("sdk-runtime-entry.js"),
        outfile: project.root.join(&sdk_rel),
        tsconfig: tsconfig.clone(),
        aliases: sdk_aliases,
        externals: vec![],
    };
    let started = Instant::now();
    esbuild::bundle(&project, &sdk_opts).await?;
    tracing::info!("sdk chunk saved {}", sdk_opts.outfile.display());
    steps.done(format!(
        "SDK chunk saved {} ({})",
        ux::rel_to(&project.root, &sdk_opts.outfile),
        ux::fmt_elapsed(started.elapsed())
    ));

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

    split::write_loader_stub(
        &outfile,
        &sdk_rel,
        &scene_rel,
        generated.max_composite_entity,
    )?;
    tracing::info!("loader stub saved {}", outfile.display());
    steps.done(format!(
        "Loader stub saved {}",
        ux::rel_to(&project.root, &outfile)
    ));

    match crate::data_layer::regenerate_main_crdt(&project.root, opts.ignore_composite).await? {
        Some(n) => steps.done(format!(
            "main.crdt regenerated ({n} composite{})",
            if n == 1 { "" } else { "s" }
        )),
        None => steps.done("main.crdt skipped (no composite)"),
    }

    if opts.skip_type_check {
        ux::note("type check skipped (--skip-type-check)");
    } else {
        type_check(&project).await?;
        tracing::info!("type checking completed without errors");
        steps.done("Type check passed");
    }

    Ok(Built { project, outfile })
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
