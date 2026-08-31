//! `dcl-one-sdk host` -- run the scene's authoritative-server isolate
//! (docs/multiplayer-server-design.md, M1).
//!
//! The isolate is a node process running the built scene with
//! `isServer() == true`, joined to a preview's mini-comms room through the
//! JSON host door (`/mini-comms/{room}/host`). Storage lands in
//! `.dcl-one/storage.json`. This command hosts against an ALREADY RUNNING
//! preview; `start` growing an auto-host when scene.json carries
//! `authoritativeMultiplayer` is the M4 integration.

use crate::scene::Project;
use crate::ux::{TrySteps, UserError};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

const HOST_TEMPLATE: &str = include_str!("templates/host-runtime.mjs");

pub struct HostOptions {
    pub dir: PathBuf,
    /// The preview server whose room this host joins.
    pub preview: String,
    pub room: String,
}

fn write_harness(root: &Path) -> Result<PathBuf> {
    let dir = root.join(".dcl-one");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let path = dir.join("host-runtime.mjs");
    std::fs::write(&path, HOST_TEMPLATE).with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

/// A running server isolate. Dropping it closes the stdin lifeline, which
/// the harness exits on -- covering every way the parent can die, including
/// the hard exit(0) paths that skip kill_on_drop (the data-layer driver's
/// pattern).
pub struct Isolate {
    pub child: tokio::process::Child,
    _stdin: Option<tokio::process::ChildStdin>,
}

/// Spawn the scene's server isolate against a preview's room. The scene must
/// already be built; the harness reconnects until the door answers, so the
/// preview may still be binding when this returns.
pub fn spawn_isolate(root: &Path, preview: &str, room: &str) -> Result<Isolate> {
    let node = crate::build::require_node(
        "the authoritative-server isolate",
        "the host runs the scene under node",
    )?;
    let harness = write_harness(root)?;
    let storage = root.join(".dcl-one").join("storage.json");
    let url = door_url(preview, room);
    let mut child = tokio::process::Command::new(&node)
        .arg(&harness)
        .arg(root)
        .arg(&url)
        .arg(&storage)
        .current_dir(root)
        .stdin(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("spawning {}", node.display()))?;
    let stdin = child.stdin.take();
    Ok(Isolate {
        child,
        _stdin: stdin,
    })
}

fn door_url(preview: &str, room: &str) -> String {
    let base = preview.trim_end_matches('/');
    let ws = if let Some(rest) = base.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = base.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        format!("ws://{base}")
    };
    format!("{ws}/mini-comms/{room}/host")
}

pub async fn host(opts: &HostOptions) -> Result<()> {
    let project = Project::load(&opts.dir)?;
    let main = project
        .scene_json
        .get("main")
        .and_then(|m| m.as_str())
        .unwrap_or("bin/index.js");
    if !project.root.join(main).is_file() {
        return Err(UserError::new(
            format!("the scene is not built ({main} is missing)"),
            TrySteps::one("dcl-one-sdk build"),
        )
        .into());
    }
    if project
        .scene_json
        .get("authoritativeMultiplayer")
        .and_then(|v| v.as_bool())
        != Some(true)
    {
        crate::ux::note(
            "scene.json has no \"authoritativeMultiplayer\": true -- hosting anyway, but \
             clients will not look for a server",
        );
    }
    let url = door_url(&opts.preview, &opts.room);
    crate::ux::note_arrow(format!("hosting {main} against {url}"));
    crate::ux::note(format!(
        "storage: {}",
        project.root.join(".dcl-one").join("storage.json").display()
    ));
    let mut isolate = spawn_isolate(&project.root, &opts.preview, &opts.room)?;
    let status = isolate
        .child
        .wait()
        .await
        .context("waiting on the host isolate")?;
    if !status.success() {
        return Err(UserError::new(
            format!("the host isolate exited with {status}"),
            TrySteps::one("read the [multiplayer] lines above")
                .and("is the preview running? dcl-one-sdk start serves the room door"),
        )
        .into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn door_url_swaps_scheme_and_appends_the_host_path() {
        assert_eq!(
            door_url("http://127.0.0.1:8001", "room-1"),
            "ws://127.0.0.1:8001/mini-comms/room-1/host"
        );
        assert_eq!(
            door_url("https://tunnel.example/t/abc/", "room-1"),
            "wss://tunnel.example/t/abc/mini-comms/room-1/host"
        );
        assert_eq!(
            door_url("127.0.0.1:8000", "r"),
            "ws://127.0.0.1:8000/mini-comms/r/host"
        );
    }
}
