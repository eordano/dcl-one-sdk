use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const READY_TIMEOUT: Duration = Duration::from_secs(15);
const READY_POLL_INTERVAL: Duration = Duration::from_millis(250);
/// abgen ships inside this binary, so a failure here is the sidecar itself
/// misbehaving, not a missing install — say what to do about that instead.
const INSTALL_HINT: &str =
    "ABGEN_BIN overrides the embedded copy; --no-asset-bundles silences this";

fn env_or(name: &str, default: String) -> String {
    match std::env::var(name) {
        Ok(v) if !v.is_empty() => v,
        _ => default,
    }
}

fn free_port() -> Option<u16> {
    let l = std::net::TcpListener::bind(("0.0.0.0", 0)).ok()?;
    Some(l.local_addr().ok()?.port())
}

/// abgen's canonical port; taken (second preview, unrelated tenant) falls
/// back to a random port that the deeplink carries. The probe binds the
/// wildcard address because that is what abgen binds: a loopback probe
/// false-passes when another abgen holds 0.0.0.0:5147 with SO_REUSEADDR.
fn sidecar_port() -> Option<u16> {
    const PREFERRED: u16 = 5147;
    if std::net::TcpListener::bind(("0.0.0.0", PREFERRED)).is_ok() {
        return Some(PREFERRED);
    }
    free_port()
}

/// The local catalyrst-abgen serve endpoint. Overridden with
/// ABGEN_UPSTREAM_AB_CDN; never defaults to a public asset-bundle CDN.
fn upstream_ab_cdn_default() -> String {
    "http://127.0.0.1:5147".to_string()
}

fn host_platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "mac"
    } else {
        "windows"
    }
}

/// Leave a quarter of the cores for the preview server, the explorer and the
/// rest of the machine: abgen lanes get ceil(3/4 · ncpu).
fn three_quarter_cpus() -> usize {
    let n = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    (n * 3).div_ceil(4).max(1)
}

fn pick_bin(env_bin: Option<String>, embedded: Option<PathBuf>) -> String {
    if let Some(v) = env_bin.filter(|v| !v.is_empty()) {
        return v;
    }
    match embedded {
        Some(p) => p.display().to_string(),
        // Only reachable if the embed failed to unpack (a full or read-only
        // temp dir), which already printed why. PATH is the last thing left to
        // try before giving up on asset bundles for this run.
        None => "abgen".to_string(),
    }
}

/// The abgen the sidecar runs. Every dcl-one-sdk binary embeds one, so this
/// needs no install step and has no per-scene lookup; ABGEN_BIN overrides it
/// for advanced use — a locally built abgen, a bisect, a test.
pub fn resolve_bin() -> String {
    pick_bin(
        std::env::var("ABGEN_BIN").ok(),
        crate::abgen_embed::ensure_extracted(),
    )
}

pub struct Sidecar {
    pub url: String,
    pub bin: String,
    exited: tokio::sync::watch::Receiver<bool>,
}

/// pgid of the running sidecar (0 = none). kill_on_drop only fires on a clean
/// drop and only reaches the direct child; when the SDK dies by signal the
/// abgen group would survive and keep holding port 5147, so the group id is
/// kept here for an explicit group kill from the shutdown path.
#[cfg(unix)]
static SIDECAR_PGID: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// SIGTERM the whole process group, escalating to SIGKILL if anything in it
/// survives the grace window. `pgid <= 0` is a no-op.
#[cfg(unix)]
fn kill_process_group(pgid: i32) {
    if pgid <= 0 {
        return;
    }
    unsafe { libc::kill(-pgid, libc::SIGTERM) };
    let deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < deadline {
        if unsafe { libc::kill(-pgid, 0) } != 0 {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    unsafe { libc::kill(-pgid, libc::SIGKILL) };
}

/// Kill the spawned sidecar's whole process group, if one is running. Safe to
/// call more than once and with no sidecar at all.
pub fn kill_sidecar_group() {
    #[cfg(unix)]
    kill_process_group(SIDECAR_PGID.swap(0, std::sync::atomic::Ordering::SeqCst));
}

#[derive(serde::Deserialize)]
struct BuildEvent {
    file: String,
    platform: Option<String>,
    build_ms: Option<u64>,
    out_bytes: Option<u64>,
    result: Option<String>,
}

fn rewrite_build_line(line: &str, project_root: &Path) -> Option<(String, bool)> {
    let json = line.trim_start().strip_prefix("ABGEN_BUILD ")?;
    let ev: BuildEvent = serde_json::from_str(json).ok()?;
    let mut tail = ev.file.clone();
    if let Some(platform) = &ev.platform {
        tail.push_str(&format!(" ({platform})"));
    }
    if let Some(ms) = ev.build_ms {
        tail.push_str(&format!(
            " {}",
            crate::ux::fmt_elapsed(Duration::from_millis(ms))
        ));
    }
    if let Ok(meta) = std::fs::metadata(project_root.join(&ev.file)) {
        tail.push_str(&format!(", in {}", crate::ux::fmt_bytes(meta.len())));
    }
    if let Some(out) = ev.out_bytes {
        tail.push_str(&format!(", out {}", crate::ux::fmt_bytes(out)));
    }
    match ev.result.as_deref() {
        Some("ok") | None => Some((format!("abgen build: {tail}"), false)),
        Some(err) => Some((format!("abgen build FAIL: {tail} \u{2014} {err}"), true)),
    }
}

fn relay_output(
    stream: impl tokio::io::AsyncRead + Unpin + Send + 'static,
    to_stderr: bool,
    project_root: PathBuf,
) {
    use tokio::io::AsyncBufReadExt;
    tokio::spawn(async move {
        let mut lines = tokio::io::BufReader::new(stream).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            match rewrite_build_line(&line, &project_root) {
                Some((msg, true)) => crate::ux::note_stderr(msg),
                Some((msg, false)) => crate::ux::note(msg),
                // abgen's startup config dump is INFO-level chatter; without
                // --verbose only its warnings and errors reach the terminal.
                None if !crate::ux::verbose() && !looks_like_problem(&line) => {}
                None if to_stderr => eprintln!("{line}"),
                None => println!("{line}"),
            }
        }
    });
}

fn looks_like_problem(line: &str) -> bool {
    // abgen INFO tracing lines may carry an `error=` field (degraded-mode
    // notes); the INFO level itself marks them as chatter, not problems.
    if line.contains("INFO") {
        return false;
    }
    ["WARN", "ERROR", "error", "panic", "failed"]
        .iter()
        .any(|k| line.contains(k))
}

pub fn spawn_sidecar(preview_port: u16, project_root: &Path) -> Option<Sidecar> {
    let bin = resolve_bin();
    let port = sidecar_port()?;
    let url = format!("http://127.0.0.1:{port}");
    // Conversion output lives next to scene.json: watcher-ignored (or hot
    // reload loops on abgen's revalidation writes) and never deployed.
    let cache_root: PathBuf = project_root.join(".dcl-optimized-assets");

    let mut cmd = tokio::process::Command::new(&bin);
    // Own process group (unix): lets shutdown kill abgen AND anything abgen
    // spawned via one group signal, even when the SDK dies by SIGINT/SIGTERM
    // (kill_on_drop never fires then and reaches only the direct child).
    #[cfg(unix)]
    cmd.process_group(0);
    let spawned = cmd
        // All interfaces, like the preview server itself: LAN devices joining
        // via the network deep link fetch their optimized assets directly.
        .env("HTTP_SERVER_HOST", "0.0.0.0")
        .env("HTTP_SERVER_PORT", port.to_string())
        .env(
            "ABGEN_CATALYST_URL",
            env_or(
                "ABGEN_CATALYST_URL",
                format!("http://127.0.0.1:{preview_port}/content"),
            ),
        )
        // No worlds fallback in preview: nothing remote is locally
        // convertible. abgen defaults this ON when unset, so disable
        // explicitly ("off"; any other value re-enables it).
        .env(
            "ABGEN_WORLDS_CONTENT_URL",
            env_or("ABGEN_WORLDS_CONTENT_URL", "off".to_string()),
        )
        .env(
            "ABGEN_UPSTREAM_AB_CDN",
            env_or("ABGEN_UPSTREAM_AB_CDN", upstream_ab_cdn_default()),
        )
        .env(
            "ABGEN_INDEX_EAGER_BUILD",
            env_or("ABGEN_INDEX_EAGER_BUILD", "off".to_string()),
        )
        .env(
            "ABGEN_INDEX_BUILD_PLATFORMS",
            env_or("ABGEN_INDEX_BUILD_PLATFORMS", host_platform().to_string()),
        )
        .env(
            "ABGEN_OUT_ROOT",
            env_or(
                "ABGEN_OUT_ROOT",
                cache_root.join("out").display().to_string(),
            ),
        )
        .env(
            "ABGEN_CACHE_DIR",
            env_or(
                "ABGEN_CACHE_DIR",
                cache_root.join("cache").display().to_string(),
            ),
        )
        // ABGEN_GPU_BACKEND is not set: abgen's auto is right (CPU on macOS
        // where integrated Metal loses to the CPU for BC7, GPU elsewhere);
        // exporting the var still passes through to the sidecar.
        .env(
            "ABGEN_JIT_BUILD_CONCURRENCY",
            env_or(
                "ABGEN_JIT_BUILD_CONCURRENCY",
                three_quarter_cpus().to_string(),
            ),
        )
        .env(
            "ABGEN_INDEX_BUILD_CONCURRENCY",
            env_or(
                "ABGEN_INDEX_BUILD_CONCURRENCY",
                three_quarter_cpus().to_string(),
            ),
        )
        .env(
            "RAYON_NUM_THREADS",
            env_or("RAYON_NUM_THREADS", three_quarter_cpus().to_string()),
        )
        .env(
            "RUST_LOG",
            env_or("RUST_LOG", "abgen=info,tower_http=warn".to_string()),
        )
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn();

    match spawned {
        Ok(mut child) => {
            if let Some(out) = child.stdout.take() {
                relay_output(out, false, project_root.to_path_buf());
            }
            if let Some(err) = child.stderr.take() {
                relay_output(err, true, project_root.to_path_buf());
            }
            #[cfg(unix)]
            let pgid = child.id().map(|id| id as i32).unwrap_or(0);
            #[cfg(unix)]
            SIDECAR_PGID.store(pgid, std::sync::atomic::Ordering::SeqCst);
            let (tx, exited) = tokio::sync::watch::channel(false);
            tokio::spawn(async move {
                let _ = child.wait().await;
                // Natural exit: forget the group so a later shutdown cannot
                // signal a recycled pgid.
                #[cfg(unix)]
                let _ = SIDECAR_PGID.compare_exchange(
                    pgid,
                    0,
                    std::sync::atomic::Ordering::SeqCst,
                    std::sync::atomic::Ordering::SeqCst,
                );
                let _ = tx.send(true);
            });
            Some(Sidecar { url, bin, exited })
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            crate::ux::note_stderr(format!(
                "asset bundles off \u{2014} {bin} not found ({INSTALL_HINT})"
            ));
            None
        }
        Err(e) => {
            crate::ux::note_stderr(format!(
                "asset bundles off \u{2014} {bin} failed to start: {} ({INSTALL_HINT})",
                e.kind()
            ));
            None
        }
    }
}

impl Sidecar {
    pub async fn wait_ready(&mut self) -> bool {
        let ready_url = format!("{}/readyz", self.url);
        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
        {
            Ok(c) => c,
            Err(_) => return false,
        };
        let deadline = Instant::now() + READY_TIMEOUT;
        while Instant::now() < deadline {
            if *self.exited.borrow() {
                crate::ux::note_stderr(format!(
                    "asset bundles off \u{2014} {} exited before becoming ready ({INSTALL_HINT})",
                    self.bin
                ));
                return false;
            }
            if let Ok(res) = client.get(&ready_url).send().await {
                if res.status().is_success() {
                    return true;
                }
            }
            tokio::select! {
                _ = tokio::time::sleep(READY_POLL_INTERVAL) => {}
                _ = self.exited.changed() => {}
            }
        }
        crate::ux::note_stderr(format!(
            "asset bundles off \u{2014} {} did not come up on {} within {} ({INSTALL_HINT})",
            self.bin,
            self.url,
            crate::ux::fmt_elapsed(READY_TIMEOUT)
        ));
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrite_build_line_formats_ok_fail_and_passthrough() {
        let root = std::env::temp_dir().join(format!(
            "dcl-one-sdk-abgen-line-test-{}-{:x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(root.join("images")).unwrap();
        std::fs::write(root.join("images/scene-thumbnail.png"), vec![0u8; 2048]).unwrap();

        let ok = r#"ABGEN_BUILD {"entity":"b64-x","entity_type":"scene","file":"images/scene-thumbnail.png","platform":"mac","hash":"b64-y","build_ms":16,"out_bytes":33866,"result":"ok"}"#;
        assert_eq!(
            rewrite_build_line(ok, &root),
            Some((
                "abgen build: images/scene-thumbnail.png (mac) 16ms, in 2.0kb, out 33.1kb"
                    .to_string(),
                false
            ))
        );

        let fail = r#"ABGEN_BUILD {"file":"assets/tree.glb","platform":"windows","build_ms":6230,"result":"decode error"}"#;
        assert_eq!(
            rewrite_build_line(fail, &root),
            Some((
                "abgen build FAIL: assets/tree.glb (windows) 6,230ms \u{2014} decode error"
                    .to_string(),
                true
            ))
        );

        assert_eq!(rewrite_build_line("plain log line", &root), None);
        assert_eq!(rewrite_build_line("ABGEN_BUILD not-json", &root), None);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    #[allow(clippy::identity_op)]
    fn three_quarter_cpus_is_at_least_one_and_leaves_headroom() {
        let n = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap();
        let q = three_quarter_cpus();
        assert!(q >= 1);
        assert!(q <= n);
        if n >= 4 {
            assert!(q < n, "must leave at least a quarter of {n} cores free");
        }
        assert_eq!((4usize * 3).div_ceil(4), 3);
        assert_eq!((18usize * 3).div_ceil(4), 14);
        assert_eq!((1usize * 3).div_ceil(4), 1);
    }

    #[test]
    fn abgen_bin_overrides_the_embedded_copy() {
        let embedded = PathBuf::from("/tmp/embedded/abgen");
        assert_eq!(
            pick_bin(Some("/custom/abgen".into()), Some(embedded.clone())),
            "/custom/abgen"
        );
        // An empty ABGEN_BIN is the same as unset, not a request to run "".
        assert_eq!(
            pick_bin(Some(String::new()), Some(embedded.clone())),
            embedded.display().to_string()
        );
        assert_eq!(
            pick_bin(None, Some(embedded.clone())),
            embedded.display().to_string()
        );
        assert_eq!(pick_bin(None, None), "abgen");
    }

    #[test]
    fn resolve_bin_lands_on_the_embedded_abgen_without_any_install() {
        // Guards the whole point of the embed: no scene, no PATH entry, no npm
        // package, and `start` still has an executable to run.
        let prev = std::env::var("ABGEN_BIN").ok();
        std::env::remove_var("ABGEN_BIN");
        let bin = resolve_bin();
        if let Some(v) = prev {
            std::env::set_var("ABGEN_BIN", v);
        }
        assert_ne!(
            bin, "abgen",
            "resolved to bare PATH; the embed did not unpack"
        );
        assert!(Path::new(&bin).is_file(), "{bin} is not a file");
    }

    #[cfg(unix)]
    #[test]
    fn kill_process_group_reaps_leader_and_grandchildren() {
        use std::os::unix::process::CommandExt;
        // sh leads its own group; the backgrounded sleep joins that group and
        // outlives sh, exactly like an abgen worker outliving the server.
        let mut child = std::process::Command::new("/bin/sh")
            .args(["-c", "sleep 300 & exec sleep 300"])
            .process_group(0)
            .stdin(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let pgid = child.id() as i32;
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(unsafe { libc::kill(-pgid, 0) }, 0, "group must be alive");

        kill_process_group(pgid);
        assert!(!child.wait().unwrap().success(), "leader died by signal");

        // The whole group (incl. the reparented background sleep) is gone.
        let deadline = Instant::now() + Duration::from_secs(5);
        while unsafe { libc::kill(-pgid, 0) } == 0 {
            assert!(
                Instant::now() < deadline,
                "process group {pgid} still alive after kill_process_group"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        // pgid 0 / negative are no-ops, and double kill is safe.
        kill_process_group(0);
        kill_process_group(pgid);
    }

    /// The scene's own `@dcl/abgen` platform package used to be part of the
    /// lookup chain. It no longer is: the binary carries abgen, so a scene that
    /// happens to have that package installed must not silently swap the
    /// sidecar for a different version.
    #[test]
    fn an_npm_abgen_in_the_scene_is_ignored() {
        let root = std::env::temp_dir().join(format!(
            "dcl-one-sdk-npm-abgen-{}-{:x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let pkg = root
            .join("node_modules")
            .join("@dcl")
            .join("abgen-darwin-arm64");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(pkg.join("abgen"), b"").unwrap();
        let prev = std::env::var("ABGEN_BIN").ok();
        std::env::remove_var("ABGEN_BIN");
        let bin = resolve_bin();
        if let Some(v) = prev {
            std::env::set_var("ABGEN_BIN", v);
        }
        assert!(
            !bin.contains("node_modules"),
            "resolved to the scene's npm abgen: {bin}"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}
