use crate::scene::Project;
use crate::ux::{self, TrySteps, UserError};
use anyhow::{bail, Result};
use clap::ValueEnum;
use std::path::PathBuf;
use std::process::Stdio;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum Backend {
    #[default]
    Esbuild,
    Rolldown,
}

pub struct EsbuildOptions {
    pub backend: Backend,
    pub production: bool,
    pub entrypoint: PathBuf,
    pub outfile: PathBuf,
    pub tsconfig: PathBuf,
    pub aliases: Vec<(String, PathBuf)>,
    pub externals: Vec<String>,
}

pub async fn bundle(project: &Project, opts: &EsbuildOptions) -> Result<()> {
    match opts.backend {
        Backend::Esbuild => run(project, opts).await,
        Backend::Rolldown => bundle_rolldown(project, opts).await,
    }
}

#[cfg(feature = "rolldown")]
async fn bundle_rolldown(project: &Project, opts: &EsbuildOptions) -> Result<()> {
    crate::rolldown_backend::run(project, opts).await
}

#[cfg(not(feature = "rolldown"))]
async fn bundle_rolldown(_project: &Project, _opts: &EsbuildOptions) -> Result<()> {
    Err(UserError::new(
        "this binary was built without the rolldown backend",
        TrySteps::one("rebuild with cargo build -p dcl-one-sdk --features rolldown")
            .and("or drop --backend rolldown to use esbuild"),
    )
    .into())
}

pub fn locate(project: &Project) -> Result<PathBuf> {
    if let Ok(explicit) = std::env::var("DCL_ONE_SDK_ESBUILD") {
        let p = PathBuf::from(explicit);
        if p.exists() {
            return Ok(p);
        }
        return Err(UserError::new(
            format!("DCL_ONE_SDK_ESBUILD points at a missing file: {}", p.display()),
            TrySteps::one(
                "fix or unset DCL_ONE_SDK_ESBUILD (unset falls back to the scene's node_modules copy)",
            ),
        )
        .into());
    }
    let candidates = [
        "@esbuild/linux-x64/bin/esbuild",
        "@esbuild/linux-arm64/bin/esbuild",
        "@esbuild/darwin-arm64/bin/esbuild",
        "@esbuild/darwin-x64/bin/esbuild",
        "esbuild/bin/esbuild",
    ];
    for c in candidates {
        if let Some(p) = project.node_module(c) {
            return Ok(p);
        }
    }
    if let Ok(p) = which("esbuild") {
        return Ok(p);
    }
    Err(UserError::new(
        "esbuild is not installed in this scene",
        TrySteps::one("run npm install in the scene directory")
            .and("or set DCL_ONE_SDK_ESBUILD=<path-to-esbuild-binary>"),
    )
    .why(format!(
        "no esbuild binary under {}/node_modules and none on PATH",
        project.root.display()
    ))
    .into())
}

fn which(bin: &str) -> Result<PathBuf> {
    let path = std::env::var("PATH").unwrap_or_default();
    for dir in path.split(':') {
        let p = PathBuf::from(dir).join(bin);
        if p.exists() {
            return Ok(p);
        }
    }
    bail!("{bin} not on PATH")
}

pub fn args(opts: &EsbuildOptions) -> Vec<String> {
    let mut a = vec![opts.entrypoint.display().to_string()];
    a.extend(flags(opts));
    a
}

pub fn flags(opts: &EsbuildOptions) -> Vec<String> {
    let mut a: Vec<String> = vec![
        "--bundle".into(),
        "--platform=browser".into(),
        "--format=cjs".into(),
        format!("--outfile={}", opts.outfile.display()),
        "--target=es2020".into(),
        "--tree-shaking=true".into(),
        "--external:~system/*".into(),
        "--external:@dcl/inspector".into(),
        "--external:@dcl/inspector/*".into(),
        format!("--tsconfig={}", opts.tsconfig.display()),
        "--supported:import-assertions=false".into(),
        "--supported:import-meta=false".into(),
        "--supported:dynamic-import=false".into(),
        "--supported:hashbang=false".into(),
        "--log-override:import-is-undefined=silent".into(),
        "--define:document=undefined".into(),
        "--define:window=undefined".into(),
    ];
    if opts.production {
        a.push("--minify".into());
        a.push("--sourcemap=external".into());
        a.push("--source-root=dcl:///".into());
        a.push("--define:DEBUG=false".into());
        a.push("--define:globalThis.DEBUG=false".into());
        a.push("--define:process.env.NODE_ENV=\"production\"".into());
    } else {
        a.push("--sourcemap=inline".into());
        let out_dir = opts
            .outfile
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        a.push(format!("--source-root=file://{out_dir}"));
        a.push("--define:DEBUG=true".into());
        a.push("--define:globalThis.DEBUG=true".into());
        a.push("--define:process.env.NODE_ENV=\"development\"".into());
    }
    for ext in &opts.externals {
        a.push(format!("--external:{ext}"));
    }
    for (name, path) in &opts.aliases {
        a.push(format!("--alias:{}={}", name, path.display()));
    }
    a
}

pub fn spawn_error(bin: &std::path::Path, e: std::io::Error) -> anyhow::Error {
    UserError::new(
        format!(
            "the esbuild binary at {} could not be started",
            bin.display()
        ),
        TrySteps::one("run npm install to restore a matching binary for this platform")
            .and("check the file is executable for this OS/architecture"),
    )
    .caused_by(e)
    .into()
}

pub async fn run(project: &Project, opts: &EsbuildOptions) -> Result<()> {
    let bin = locate(project)?;
    let out = tokio::process::Command::new(&bin)
        .args(args(opts))
        .current_dir(&project.root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| spawn_error(&bin, e))?;
    if !out.status.success() {
        return Err(ux::bundle_failed(&String::from_utf8_lossy(&out.stderr)));
    }
    Ok(())
}

pub fn resolve_aliases(project: &Project) -> Result<Vec<(String, PathBuf)>> {
    let mut aliases = Vec::new();
    let sdk = project.require_node_module("@dcl/sdk")?;
    aliases.push(("@dcl/sdk".to_string(), sdk));
    if let Some(ecs) = project
        .node_module("@dcl/sdk/node_modules/@dcl/ecs")
        .or_else(|| project.node_module("@dcl/ecs"))
    {
        aliases.push(("@dcl/ecs".to_string(), ecs));
    }
    if let Some(react) = project
        .node_module("react")
        .or_else(|| project.node_module("@dcl/react-ecs/node_modules/react"))
    {
        aliases.push(("react".to_string(), react));
    }
    if let Some(ap) = project
        .node_module("@dcl/asset-packs")
        .or_else(|| project.node_module("@dcl/inspector/node_modules/@dcl/asset-packs"))
    {
        aliases.push(("@dcl/asset-packs".to_string(), ap));
    }
    Ok(aliases)
}
