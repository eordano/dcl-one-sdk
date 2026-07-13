use crate::ux::{self, TrySteps, UserError};
use anyhow::{Context, Result};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::watch;

const DRIVER_TEMPLATE: &str = include_str!("templates/data-layer-host.mjs");
const READY_TIMEOUT: Duration = Duration::from_secs(60);
const DUMP_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Clone)]
pub struct DataLayerState {
    pub port_rx: watch::Receiver<u16>,
    pub public_dir: PathBuf,
}

pub fn locate_inspector_public(root: &Path) -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("DCL_ONE_INSPECTOR_DIR") {
        let d = PathBuf::from(&dir);
        for candidate in [d.join("public"), d.clone()] {
            if candidate.join("index.html").is_file() {
                return Ok(candidate);
            }
        }
        return Err(UserError::new(
            "DCL_ONE_INSPECTOR_DIR does not contain an inspector build",
            TrySteps::one(
                "point it at an @dcl/inspector package dir (one containing public/index.html)",
            )
            .and("or unset it to use the scene's own node_modules"),
        )
        .why(format!("no index.html under {dir} or {dir}/public"))
        .into());
    }
    let mut dir = Some(root);
    while let Some(d) = dir {
        let candidate = d.join("node_modules/@dcl/inspector/public");
        if candidate.join("index.html").is_file() {
            return Ok(candidate);
        }
        dir = d.parent();
    }
    Err(UserError::new(
        "the visual editor UI (@dcl/inspector) is not installed in this scene",
        TrySteps::one("run npm install in the scene directory (@dcl/sdk-commands ships it)")
            .and("or set DCL_ONE_INSPECTOR_DIR=<path-to-an-@dcl/inspector-package>"),
    )
    .why(format!(
        "no node_modules/@dcl/inspector/public/index.html at or above {}",
        root.display()
    ))
    .into())
}

pub fn inject_config(html: &str, config_json: &str) -> String {
    html.replace(
        "const config = '$CONFIG'",
        &format!("const config = '{config_json}'"),
    )
}

pub fn inspector_config_json(ws_url: &str) -> String {
    json!({ "dataLayerRpcWsUrl": ws_url }).to_string()
}

pub fn inspector_mime(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "application/javascript",
        "css" => "text/css",
        "map" | "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "svg" => "image/svg+xml",
        "gif" => "image/gif",
        "ico" => "image/x-icon",
        "wasm" => "application/wasm",
        "ttf" => "font/ttf",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

pub fn write_driver(root: &Path) -> Result<PathBuf> {
    let dir = root.join(".dcl-one");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let path = dir.join("data-layer-host.mjs");
    std::fs::write(&path, DRIVER_TEMPLATE)
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

fn node_bin() -> Result<PathBuf> {
    match crate::build::find_node() {
        Some(p) => Ok(p),
        None => Err(UserError::new(
            "node is required for the visual editor data layer but is not on PATH",
            TrySteps::one("install Node.js or add it to PATH")
                .and("to preview without the editor, drop --data-layer"),
        )
        .into()),
    }
}

struct Driver {
    child: tokio::process::Child,
    _stdin: Option<tokio::process::ChildStdin>,
    port: u16,
}

async fn launch(node: &Path, driver: &Path, root: &Path) -> Result<Driver> {
    let mut child = tokio::process::Command::new(node)
        .arg(driver)
        .arg(root)
        .arg("serve")
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| {
            anyhow::Error::from(
                UserError::new(
                    "could not start the data-layer host (node)",
                    TrySteps::one("check node runs: node --version")
                        .and("to preview without the editor, drop --data-layer"),
                )
                .caused_by(e),
            )
        })?;
    let stdin = child.stdin.take();
    let stdout = child.stdout.take().context("driver stdout missing")?;
    let stderr = child.stderr.take().context("driver stderr missing")?;
    let mut err_lines = BufReader::new(stderr).lines();
    let (err_tx, mut err_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    tokio::spawn(async move {
        while let Ok(Some(line)) = err_lines.next_line().await {
            tracing::debug!(target: "data_layer", "{line}");
            let _ = err_tx.send(line);
        }
    });
    let mut out_lines = BufReader::new(stdout).lines();
    let ready = tokio::time::timeout(READY_TIMEOUT, async {
        while let Ok(Some(line)) = out_lines.next_line().await {
            tracing::debug!(target: "data_layer", "{line}");
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                if v.get("ready").and_then(|r| r.as_bool()) == Some(true) {
                    if let Some(port) = v.get("port").and_then(|p| p.as_u64()) {
                        return Some(port as u16);
                    }
                }
            }
        }
        None
    })
    .await;
    tokio::spawn(async move {
        while let Ok(Some(line)) = out_lines.next_line().await {
            tracing::debug!(target: "data_layer", "{line}");
        }
    });
    match ready {
        Ok(Some(port)) => Ok(Driver {
            child,
            _stdin: stdin,
            port,
        }),
        _ => {
            let _ = child.kill().await;
            let mut tail = Vec::new();
            while let Ok(line) = err_rx.try_recv() {
                tail.push(line);
            }
            let why = if tail.is_empty() {
                "the driver exited before reporting its port".to_string()
            } else {
                tail.join("\n")
            };
            Err(UserError::new(
                "the data-layer host did not come up",
                TrySteps::one("run npm install in the scene directory (@dcl/inspector, @dcl/rpc and ws must resolve)")
                    .and("or set DCL_ONE_INSPECTOR_DIR=<path-to-an-@dcl/inspector-package>")
                    .and("re-run with --verbose for the full driver log"),
            )
            .why(why)
            .into())
        }
    }
}

pub async fn spawn(root: &Path) -> Result<watch::Receiver<u16>> {
    let node = node_bin()?;
    let driver = write_driver(root)?;
    let mut current = launch(&node, &driver, root).await?;
    let (tx, rx) = watch::channel(current.port);
    tracing::info!("data-layer host ready on 127.0.0.1:{}", current.port);
    let root = root.to_path_buf();
    tokio::spawn(async move {
        let mut backoff = Duration::from_secs(1);
        loop {
            let started = std::time::Instant::now();
            let status = current.child.wait().await;
            if tx.is_closed() {
                return;
            }
            let _ = tx.send(0);
            ux::report_watch(
                &UserError::new(
                    "the visual-editor data layer stopped \u{2014} restarting it",
                    TrySteps::one(
                        "reload the editor page after it reconnects (unsaved edits may be lost)",
                    )
                    .and("re-run with --verbose to capture why it exited"),
                )
                .why(match status {
                    Ok(s) => format!("driver exited with {s}"),
                    Err(e) => format!("driver wait failed: {e}"),
                })
                .into(),
            );
            if started.elapsed() > Duration::from_secs(60) {
                backoff = Duration::from_secs(1);
            }
            loop {
                tokio::time::sleep(backoff).await;
                backoff = std::cmp::min(backoff * 2, Duration::from_secs(30));
                match launch(&node, &driver, &root).await {
                    Ok(next) => {
                        current = next;
                        if tx.send(current.port).is_err() {
                            return;
                        }
                        tracing::info!("data-layer host restarted on 127.0.0.1:{}", current.port);
                        break;
                    }
                    Err(e) => ux::report_watch(&e),
                }
            }
        }
    });
    Ok(rx)
}

pub async fn dump_crdt(root: &Path) -> Result<u64> {
    let node = node_bin()?;
    let driver = write_driver(root)?;
    let out = tokio::time::timeout(
        DUMP_TIMEOUT,
        tokio::process::Command::new(&node)
            .arg(&driver)
            .arg(root)
            .arg("dump-crdt")
            .current_dir(root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| {
        anyhow::Error::from(UserError::new(
            "main.crdt regeneration timed out",
            TrySteps::one("re-run with --verbose and check the composite files"),
        ))
    })?
    .map_err(|e| {
        anyhow::Error::from(
            UserError::new(
                "could not run the main.crdt regeneration (node)",
                TrySteps::one("check node runs: node --version"),
            )
            .caused_by(e),
        )
    })?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        return Err(UserError::new(
            "main.crdt regeneration failed \u{2014} keeping the existing main.crdt",
            TrySteps::one("check the composite files named below")
                .and("run npm install if @dcl/inspector cannot be resolved"),
        )
        .why(format!("{}{}", stdout.trim(), stderr.trim()))
        .into());
    }
    let summary = stdout
        .lines()
        .rev()
        .find_map(|l| serde_json::from_str::<serde_json::Value>(l.trim()).ok())
        .unwrap_or_default();
    if summary.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        return Err(UserError::new(
            "some composites could not be instanced \u{2014} main.crdt may be incomplete",
            TrySteps::one("fix the composite errors below, then rebuild"),
        )
        .why(stderr.trim().to_string())
        .into());
    }
    Ok(summary
        .get("composites")
        .and_then(|v| v.as_u64())
        .unwrap_or(0))
}

pub async fn regenerate_main_crdt(root: &Path, ignore_composite: bool) -> Result<Option<u64>> {
    if ignore_composite {
        return Ok(None);
    }
    if crate::entrypoint::find_composites(root).is_empty() {
        return Ok(None);
    }
    let n = dump_crdt(root).await?;
    tracing::info!("main.crdt regenerated from {n} composite(s)");
    Ok(Some(n))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tmp(PathBuf);

    impl Tmp {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "dcl-one-sdk-datalayer-{tag}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Tmp(dir)
        }

        fn write(&self, rel: &str, contents: &str) {
            let p = self.0.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, contents).unwrap();
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn config_injection_rewrites_only_the_assignment() {
        let html = "<script>const config = '$CONFIG'\nif (config !== '$CONFIG') { globalThis.InspectorConfig = JSON.parse(config) }</script>";
        let injected = inject_config(html, &inspector_config_json("ws://x/data-layer"));
        assert!(injected.contains("const config = '{\"dataLayerRpcWsUrl\":\"ws://x/data-layer\"}'"));
        assert!(
            injected.contains("if (config !== '$CONFIG')"),
            "the sentinel comparison must stay untouched or the config never loads: {injected}"
        );
    }

    #[test]
    fn locate_walks_up_to_find_the_inspector_public_dir() {
        let t = Tmp::new("locate");
        t.write(
            "node_modules/@dcl/inspector/public/index.html",
            "<html>$CONFIG</html>",
        );
        t.write("ws/member/scene.json", "{}");
        let found = locate_inspector_public(&t.0.join("ws/member")).unwrap();
        assert_eq!(found, t.0.join("node_modules/@dcl/inspector/public"));
        assert!(locate_inspector_public(Path::new("/nonexistent-dcl1")).is_err());
    }

    #[test]
    fn driver_template_is_embedded_and_mode_aware() {
        assert!(DRIVER_TEMPLATE.contains("createDataLayerHost"));
        assert!(DRIVER_TEMPLATE.contains("dump-crdt"));
        assert!(DRIVER_TEMPLATE.contains("DataServiceDefinition"));
    }

    #[test]
    fn mime_table_covers_the_inspector_bundle() {
        assert_eq!(
            inspector_mime(Path::new("index.html")),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            inspector_mime(Path::new("bundle.js")),
            "application/javascript"
        );
        assert_eq!(inspector_mime(Path::new("bundle.css")), "text/css");
        assert_eq!(
            inspector_mime(Path::new("bundle.js.map")),
            "application/json"
        );
        assert_eq!(
            inspector_mime(Path::new("x.bin")),
            "application/octet-stream"
        );
    }
}
