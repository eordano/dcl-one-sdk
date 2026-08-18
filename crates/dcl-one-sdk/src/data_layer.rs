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
    /// Where the editor's browser bundle lives, if one is installed. `None` is
    /// normal: the data layer and the UI are separable, so `--data-layer`
    /// without `@dcl/inspector` still serves the protocol and 404s
    /// `/inspector/*`. Treating that as fatal made the vendored host
    /// unreachable, since a real `@dcl/inspector` wins at `require()` time.
    pub public_dir: Option<PathBuf>,
}

/// The editor UI for this scene, if one is installed. `Ok(None)` is not an
/// error; a `DCL_ONE_INSPECTOR_DIR` that names a non-inspector build is.
pub fn locate_inspector_public(root: &Path) -> Result<Option<PathBuf>> {
    if let Ok(dir) = std::env::var("DCL_ONE_INSPECTOR_DIR") {
        let d = PathBuf::from(&dir);
        for candidate in [d.join("public"), d.clone()] {
            if candidate.join("index.html").is_file() {
                return Ok(Some(candidate));
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
            return Ok(Some(candidate));
        }
        dir = d.parent();
    }
    Ok(None)
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

/// Where an inspector asset actually lives on disk. The vendored blob stores
/// the big bundles `.gz` (11 MB unpacked instead of 73 MB); an npm-installed
/// `@dcl/inspector` has them plain, so plain wins and `.gz` is the fallback.
pub enum Asset {
    Plain(PathBuf),
    Gzipped(PathBuf),
}

pub fn resolve_asset(public_dir: &Path, rel: &str) -> Option<Asset> {
    if rel.split('/').any(|seg| seg == "..") {
        return None;
    }
    let base = dunce::canonicalize(public_dir).unwrap_or_else(|_| public_dir.to_path_buf());
    let found = match dunce::canonicalize(public_dir.join(rel)) {
        Ok(p) => Asset::Plain(p),
        Err(_) => Asset::Gzipped(dunce::canonicalize(public_dir.join(format!("{rel}.gz"))).ok()?),
    };
    let path = match &found {
        Asset::Plain(p) | Asset::Gzipped(p) => p,
    };
    path.starts_with(&base).then_some(found)
}

/// Does the client take gzip? `identity` and `gzip;q=0` both mean no.
pub fn accepts_gzip(accept_encoding: Option<&str>) -> bool {
    let Some(value) = accept_encoding else {
        return false;
    };
    value.split(',').any(|part| {
        let mut bits = part.split(';');
        let coding = bits.next().unwrap_or("").trim();
        if !coding.eq_ignore_ascii_case("gzip") && coding != "*" {
            return false;
        }
        !bits.any(|p| {
            let p = p.trim();
            p.strip_prefix("q=")
                .and_then(|q| q.trim().parse::<f32>().ok())
                .is_some_and(|q| q <= 0.0)
        })
    })
}

pub fn gunzip(bytes: &[u8]) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(bytes).read_to_end(&mut out)?;
    Ok(out)
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

fn node_for_data_layer() -> Result<PathBuf> {
    crate::build::require_node(
        "the visual editor data layer",
        "to preview without the editor, drop --data-layer",
    )
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
                TrySteps::one(
                    "run dcl-one-sdk init --node-modules-only \u{2014} the host, @dcl/rpc and ws are vendored",
                )
                .and("or npm install in the scene (@dcl/inspector, @dcl/rpc and ws must resolve)")
                .and("re-run with --verbose for the full driver log"),
            )
            .why(why)
            .into())
        }
    }
}

pub async fn spawn(root: &Path) -> Result<watch::Receiver<u16>> {
    let node = node_for_data_layer()?;
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
    let node = node_for_data_layer()?;
    let driver = write_driver(root)?;
    let _progress = crate::ux::Slow::start("regenerating main.crdt");
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

/// Which generator produced main.crdt. User-visible: the fallback shells out to
/// node and needs `@dcl/inspector` resolvable, so it is slower and fails more.
pub enum CrdtRegen {
    /// Generated in-process, from this many composite files.
    Native(u64),
    /// The node data-layer's summary carries no composite count we can trust,
    /// so none is reported rather than a zero that reads as "nothing included".
    NodeDataLayer,
}

/// Re-run the node data-layer and shout if its bytes differ from the ones we
/// just wrote; a `DCL_ONE_CRDT_VERIFY=1` soak that reports nothing is the gate
/// for deleting the node fallback. The driver only ever writes
/// `<root>/main.crdt`, so this borrows that path and restores it afterwards.
async fn shadow_verify(root: &Path, native: &[u8]) {
    let main_crdt = root.join("main.crdt");
    let outcome = match dump_crdt(root).await {
        Ok(_) => tokio::fs::read(&main_crdt)
            .await
            .map_err(anyhow::Error::from),
        Err(e) => Err(e),
    };
    match outcome {
        Ok(reference) => {
            if let Some(difference) = crate::crdt_gen::describe_difference(native, &reference, root)
            {
                tracing::error!("DCL_ONE_CRDT_VERIFY: native main.crdt differs from the node data-layer's — {difference}");
            } else {
                tracing::info!("DCL_ONE_CRDT_VERIFY: native main.crdt matches the node data-layer byte for byte");
            }
        }
        Err(e) => tracing::error!("DCL_ONE_CRDT_VERIFY: the node data-layer could not run ({e})"),
    }
    if let Err(e) = tokio::fs::write(&main_crdt, native).await {
        tracing::error!("DCL_ONE_CRDT_VERIFY: could not restore the native main.crdt ({e})");
    }
}

pub async fn regenerate_main_crdt(
    root: &Path,
    ignore_composite: bool,
) -> Result<Option<CrdtRegen>> {
    if ignore_composite {
        return Ok(None);
    }
    let progress = crate::ux::Slow::start("regenerating main.crdt");
    let generated = crate::crdt_gen::generate(root);
    progress.finish();
    match generated {
        Ok(None) => Ok(None),
        Ok(Some(generated)) => {
            tokio::fs::write(root.join("main.crdt"), &generated.bytes)
                .await
                .context("writing main.crdt")?;
            tracing::info!(
                "main.crdt regenerated natively from {} composite(s)",
                generated.composites
            );
            if std::env::var("DCL_ONE_CRDT_VERIFY").as_deref() == Ok("1") {
                shadow_verify(root, &generated.bytes).await;
            }
            Ok(Some(CrdtRegen::Native(generated.composites)))
        }
        Err(crate::crdt_gen::GenError::Unsupported(why)) => {
            tracing::info!(
                "native main.crdt generation does not cover this scene ({why}); using the node data-layer"
            );
            let n = dump_crdt(root).await?;
            tracing::info!("main.crdt regenerated from {n} composite(s)");
            Ok(Some(CrdtRegen::NodeDataLayer))
        }
        Err(crate::crdt_gen::GenError::Invalid(why)) => Err(UserError::new(
            "main.crdt regeneration failed \u{2014} keeping the existing main.crdt",
            TrySteps::one("check the composite files named below"),
        )
        .why(why)
        .into()),
    }
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
        assert_eq!(found, Some(t.0.join("node_modules/@dcl/inspector/public")));
        assert_eq!(
            locate_inspector_public(Path::new("/nonexistent-dcl1")).unwrap(),
            None
        );
    }

    #[test]
    fn driver_template_is_embedded_and_mode_aware() {
        assert!(DRIVER_TEMPLATE.contains("createDataLayerHost"));
        assert!(DRIVER_TEMPLATE.contains("dump-crdt"));
        assert!(DRIVER_TEMPLATE.contains("DataServiceDefinition"));
    }

    #[test]
    fn resolve_prefers_a_plain_asset_and_falls_back_to_the_gzipped_one() {
        let t = Tmp::new("resolve");
        t.write("public/index.html", "<html></html>");
        t.write("public/bundle.js.gz", "not really gzip");
        t.write("public/plain.js", "console.log(1)");
        let dir = t.0.join("public");
        assert!(matches!(
            resolve_asset(&dir, "plain.js"),
            Some(Asset::Plain(_))
        ));
        assert!(matches!(
            resolve_asset(&dir, "bundle.js"),
            Some(Asset::Gzipped(_))
        ));
        t.write("public/bundle.js", "plain wins");
        assert!(matches!(
            resolve_asset(&dir, "bundle.js"),
            Some(Asset::Plain(_))
        ));
        assert!(resolve_asset(&dir, "missing.js").is_none());
    }

    #[test]
    fn resolve_refuses_paths_that_escape_the_public_dir() {
        let t = Tmp::new("escape");
        t.write("public/index.html", "<html></html>");
        t.write("secret.txt", "nope");
        let dir = t.0.join("public");
        assert!(resolve_asset(&dir, "../secret.txt").is_none());
        assert!(resolve_asset(&dir, "a/../../secret.txt").is_none());
        assert!(resolve_asset(&dir, "../secret.txt.gz").is_none());
    }

    #[test]
    fn accepts_gzip_reads_the_q_value() {
        assert!(accepts_gzip(Some("gzip, deflate, br")));
        assert!(accepts_gzip(Some("br;q=1.0, gzip;q=0.8")));
        assert!(accepts_gzip(Some("GZIP")));
        assert!(accepts_gzip(Some("*")));
        assert!(!accepts_gzip(None));
        assert!(!accepts_gzip(Some("identity")));
        assert!(!accepts_gzip(Some("deflate, gzip;q=0")));
        assert!(!accepts_gzip(Some("")));
    }

    #[test]
    fn gunzip_round_trips_the_blobs_own_encoding() {
        use std::io::Write;
        let payload = b"globalThis.InspectorConfig = {}\n".repeat(64);
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        enc.write_all(&payload).unwrap();
        let gz = enc.finish().unwrap();
        assert!(gz.len() < payload.len(), "the fixture must actually shrink");
        assert_eq!(gunzip(&gz).unwrap(), payload);
        assert!(gunzip(b"not gzip at all").is_err());
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
