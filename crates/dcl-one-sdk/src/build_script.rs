//! The package.json build contract, shared by build, preview/watch and deploy.
use crate::{
    build::{BuildOptions, Built},
    scene::Project,
    ux,
};
use anyhow::{bail, Context, Result};
use std::path::{Component, Path};
use std::process::Stdio;

const ACTIVE: &str = "DCL_ONE_SDK_BUILD_SCRIPT";

pub fn command(root: &Path) -> Result<Option<String>> {
    let file = root.join("package.json");
    let bytes = match std::fs::read(&file) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("reading {}", file.display())),
    };
    let package: serde_json::Value =
        serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", file.display()))?;
    let Some(value) = package.get("scripts").and_then(|s| s.get("build")) else {
        return Ok(None);
    };
    let script = value
        .as_str()
        .context("package.json scripts.build must be a nonempty string")?
        .trim();
    if script.is_empty() {
        bail!("package.json scripts.build must be a nonempty string");
    }
    // Existing SDK scaffolds delegated straight back to us. Preserve them as
    // a built-in alias; more complex delegations must use --built-in explicitly.
    if matches!(script, "dcl-one-sdk build" | "dcl-one-sdk build --built-in") {
        return Ok(None);
    }
    Ok(Some(script.to_owned()))
}

pub fn selected(project: &Project, opts: &BuildOptions) -> Result<Option<String>> {
    if opts.built_in {
        return Ok(None);
    }
    command(&project.root)
}

pub async fn run(project: Project, opts: &BuildOptions, script: &str) -> Result<Built> {
    if std::env::var_os(ACTIVE).is_some() {
        bail!("recursive package.json scripts.build invocation; use dcl-one-sdk build --built-in inside the build script");
    }
    let main = project.main_output()?;
    if Path::new(&main)
        .components()
        .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        bail!("custom build scene.json main must be a relative path inside the project");
    }
    let outfile = project.root.join(&main);
    let mut paths = vec![project.root.join("node_modules/.bin")];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    #[cfg(windows)]
    let mut child = {
        let mut c = tokio::process::Command::new("cmd.exe");
        c.args(["/d", "/s", "/c", script]);
        c
    };
    #[cfg(not(windows))]
    let mut child = {
        let mut c = tokio::process::Command::new("sh");
        c.args(["-c", script]);
        c
    };
    if !opts.quiet {
        ux::note(format!("Running package.json scripts.build: {script}"));
        ux::note("The build script owns type checking, bundling and asset generation");
    }
    let status = child
        .current_dir(&project.root)
        .env("PATH", std::env::join_paths(paths)?)
        .env(ACTIVE, &project.root)
        .env(
            "DCL_ONE_SDK_PRODUCTION",
            if opts.production { "1" } else { "0" },
        )
        .env("DCL_ONE_SDK_MAIN", &main)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .status()
        .await
        .context("starting package.json scripts.build")?;
    if !status.success() {
        bail!("package.json scripts.build failed ({status}); the SDK build was not run");
    }
    let metadata = std::fs::metadata(&outfile)
        .with_context(|| format!("scripts.build succeeded but did not produce {main}"))?;
    if !metadata.is_file() || metadata.len() == 0 {
        bail!("scripts.build must produce a nonempty bundle at {main}");
    }
    if !dunce::canonicalize(&outfile)?.starts_with(&project.root) {
        bail!("scripts.build output {main} resolves outside the project");
    }
    if !opts.quiet {
        ux::note(format!("Build script completed: {main}"));
    }
    Ok(Built { project, outfile })
}
