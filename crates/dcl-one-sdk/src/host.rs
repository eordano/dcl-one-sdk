//! `dcl-one-sdk host` -- run the scene's authoritative-server isolate
//! (docs/multiplayer-server-design.md, M1): a node process running the built
//! scene with `isServer() == true`, joined to an ALREADY RUNNING preview's
//! mini-comms room through the JSON host door. Auto-hosting from `start` when
//! scene.json carries `authoritativeMultiplayer` is the M4 integration.

use crate::scene::Project;
use crate::ux::{TrySteps, UserError};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, BufReader};

const HOST_TEMPLATE: &str = include_str!("templates/host-runtime.mjs");

pub struct HostOptions {
    pub dir: PathBuf,
    pub preview: String,
    pub room: String,
}

/// A running server isolate. Dropping it closes the stdin lifeline the harness
/// exits on, which covers the hard exit(0) paths that skip kill_on_drop.
pub struct Isolate {
    pub child: tokio::process::Child,
    _stdin: Option<tokio::process::ChildStdin>,
}

/// Spawns the built scene's server isolate; the harness reconnects until the
/// door answers, so the preview may still be binding when this returns.
pub fn spawn_isolate(root: &Path, preview: &str, room: &str) -> Result<Isolate> {
    let node = crate::build::require_node(
        "the authoritative-server isolate",
        "the host runs the scene under node",
    )?;
    let dir = crate::scene::work_dir(root)
        .with_context(|| format!("creating {}", root.join(".dcl-one").display()))?;
    let harness = dir.join("host-runtime.mjs");
    std::fs::write(&harness, HOST_TEMPLATE)
        .with_context(|| format!("writing {}", harness.display()))?;
    let mut child = tokio::process::Command::new(&node)
        .arg(&harness)
        .arg(root)
        .arg(door_url(preview, room))
        .arg(storage_path(root))
        .current_dir(root)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("spawning {}", node.display()))?;
    let stdin = child.stdin.take();
    let stdout = child.stdout.take().context("opening host isolate output")?;
    tokio::spawn(forward_output(stdout));
    Ok(Isolate {
        child,
        _stdin: stdin,
    })
}

/// The host is a Node child, but its lifecycle events belong to the SDK's
/// watch session.  Route its marked output through the same clock and gutter
/// as rebuilds so a multiplayer preview reads as one transcript.
async fn forward_output(stdout: tokio::process::ChildStdout) {
    let mut lines = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if let Some(message) = line.strip_prefix("DCL_ONE_MULTIPLAYER:status:") {
            crate::ux::note_clocked(format!("◉ multiplayer: {message}"));
        } else if let Some(message) = line.strip_prefix("DCL_ONE_MULTIPLAYER:detail:") {
            crate::ux::note_arrow(message);
        } else {
            println!("{line}");
        }
    }
}

fn storage_path(root: &Path) -> PathBuf {
    root.join(".dcl-one").join("storage.json")
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
        storage_path(&project.root).display()
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
