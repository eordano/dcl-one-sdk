use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const READY_TIMEOUT: Duration = Duration::from_secs(15);
const READY_POLL_INTERVAL: Duration = Duration::from_millis(250);
/// abgen ships inside this binary, so a failure here is never a missing install.
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

/// The sidecar's registered port (ports.toml :5175), purely a debuggability
/// nicety: the preview server proxies to whichever port was actually bound, so
/// a second preview on the same box just lands on a random free port. It must
/// never be 5147 — the dedicated stack abgen owns that (deploy/PORTS.md), and a
/// sidecar preferring it races the stack unit for the bind on every restart.
const SIDECAR_PREFERRED_PORT: u16 = 5175;

/// The probe binds the wildcard address because that is what abgen binds: a
/// loopback probe false-passes when another abgen holds the wildcard with
/// SO_REUSEADDR.
fn sidecar_port() -> Option<u16> {
    if std::net::TcpListener::bind(("0.0.0.0", SIDECAR_PREFERRED_PORT)).is_ok() {
        return Some(SIDECAR_PREFERRED_PORT);
    }
    free_port()
}

/// Where abgen looks for an already-converted bundle before converting one
/// itself (ABGEN_UPSTREAM_AB_CDN overrides). Must never be the sidecar's own
/// address: every miss re-entered the same server until "Too many open files
/// (os error 24)". A real CDN also makes wearables usable without converting
/// Decentraland's own on every boot.
fn upstream_ab_cdn_default() -> String {
    "https://ab-cdn.interconnected.online".to_string()
}

/// The upstream to hand the sidecar, with an upstream that IS the sidecar
/// dropped: the loop it causes reads as descriptor exhaustion, not as a
/// misconfiguration. Empty is abgen's own "no read-through" (`abcdn/config.rs`
/// filters an empty value out), so disabling needs no sentinel of ours.
fn upstream_ab_cdn_for(port: u16) -> String {
    let url = env_or("ABGEN_UPSTREAM_AB_CDN", upstream_ab_cdn_default());
    if !points_at_port(&url, port) {
        return url;
    }
    crate::ux::note_stderr(format!(
        "ignoring ABGEN_UPSTREAM_AB_CDN={url}: that is this asset-bundle sidecar's own address, \
         so every lookup would re-enter it until the process runs out of file descriptors. \
         Converting locally instead."
    ));
    String::new()
}

fn points_at_port(url: &str, port: u16) -> bool {
    url.trim()
        .trim_end_matches('/')
        .rsplit_once(':')
        .is_some_and(|(host, p)| {
            p == port.to_string()
                && matches!(
                    host,
                    "http://127.0.0.1" | "http://localhost" | "http://0.0.0.0" | "http://[::1]"
                )
        })
}

fn host_platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "mac"
    } else {
        "windows"
    }
}

/// abgen lanes get ceil(3/4 · ncpu), leaving a quarter for the preview server,
/// the explorer and the rest of the machine.
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
        None => "abgen".to_string(),
    }
}

/// The abgen the sidecar runs. Every binary embeds one, so there is no install
/// step and no per-scene lookup; ABGEN_BIN overrides it.
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
    gpu: std::sync::Arc<std::sync::Mutex<GpuQual>>,
}

/// What abgen's startup qualification chatter amounts to: which GPU backend
/// qualified, if any. The banner prints this one word instead of the four
/// stderr lines the qualification takes to say it (`gpu/mod.rs log_status` in
/// abgen: the `qualified=true` line names the backend auto settled on, since
/// auto returns on its first success).
#[derive(Default)]
struct GpuQual {
    cuda: bool,
    wgpu: bool,
}

impl GpuQual {
    fn label(&self) -> &'static str {
        if self.cuda {
            "CUDA"
        } else if self.wgpu {
            "GPU"
        } else {
            "CPU"
        }
    }
}

/// The startup lines the one-word label replaces, parsed and withheld (they
/// still print under --verbose). Only qualification-time chatter is absorbed:
/// a mid-run "GPU init panicked" or a forced-GPU error still flows through
/// [`looks_like_problem`].
fn absorb_gpu_chatter(line: &str, qual: &std::sync::Mutex<GpuQual>) -> bool {
    let line = line.trim_start();
    if let Some(rest) = line.strip_prefix("abgen-gpu: qualification ") {
        let mut backend = "";
        let mut qualified = false;
        for token in rest.split_whitespace() {
            if let Some(v) = token.strip_prefix("backend=") {
                backend = v;
            }
            if let Some(v) = token.strip_prefix("qualified=") {
                qualified = v == "true";
            }
        }
        if qualified {
            let mut qual = qual
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match backend {
                "cuda" => qual.cuda = true,
                "wgpu" => qual.wgpu = true,
                _ => {}
            }
        }
        return true;
    }
    line.starts_with("abgen-gpu: wgpu adapter:")
        || line.starts_with("abgen-gpu: macOS default is CPU")
        || line.starts_with("warning: no GPU available")
}

/// pgid of the running sidecar (0 = none). kill_on_drop only fires on a clean
/// drop and only reaches the direct child, so an SDK killed by signal would
/// leave the abgen group holding port 5147.
#[cfg(unix)]
static SIDECAR_PGID: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

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
    gpu: std::sync::Arc<std::sync::Mutex<GpuQual>>,
) {
    use tokio::io::AsyncBufReadExt;
    tokio::spawn(async move {
        let mut lines = tokio::io::BufReader::new(stream).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if absorb_gpu_chatter(&line, &gpu) {
                if crate::ux::verbose() {
                    if to_stderr {
                        eprintln!("{line}");
                    } else {
                        println!("{line}");
                    }
                }
                continue;
            }
            match rewrite_build_line(&line, &project_root) {
                Some((msg, true)) => crate::ux::note_stderr(msg),
                Some((msg, false)) => crate::ux::note(msg),
                None if !crate::ux::verbose() && !looks_like_problem(&line) => {}
                None if to_stderr => eprintln!("{line}"),
                None => println!("{line}"),
            }
        }
    });
}

fn looks_like_problem(line: &str) -> bool {
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
    let cache_root: PathBuf = project_root.join(".dcl-optimized-assets");

    let mut cmd = tokio::process::Command::new(&bin);
    #[cfg(unix)]
    cmd.process_group(0);
    cmd.env("HTTP_SERVER_HOST", "0.0.0.0")
        .env("HTTP_SERVER_PORT", port.to_string())
        .env("ABGEN_UPSTREAM_AB_CDN", upstream_ab_cdn_for(port));

    let lanes = three_quarter_cpus().to_string();
    for (name, default) in [
        (
            "ABGEN_CATALYST_URL",
            format!("http://127.0.0.1:{preview_port}/content"),
        ),
        ("ABGEN_WORLDS_CONTENT_URL", "off".to_string()),
        ("ABGEN_INDEX_EAGER_BUILD", "off".to_string()),
        ("ABGEN_INDEX_BUILD_PLATFORMS", host_platform().to_string()),
        (
            "ABGEN_OUT_ROOT",
            cache_root.join("out").display().to_string(),
        ),
        (
            "ABGEN_CACHE_DIR",
            cache_root.join("cache").display().to_string(),
        ),
        ("ABGEN_JIT_BUILD_CONCURRENCY", lanes.clone()),
        ("ABGEN_INDEX_BUILD_CONCURRENCY", lanes.clone()),
        ("RAYON_NUM_THREADS", lanes.clone()),
        ("RUST_LOG", "abgen=info,tower_http=warn".to_string()),
    ] {
        cmd.env(name, env_or(name, default));
    }

    let spawned = cmd
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn();

    let gpu = std::sync::Arc::new(std::sync::Mutex::new(GpuQual::default()));
    match spawned {
        Ok(mut child) => {
            if let Some(out) = child.stdout.take() {
                relay_output(out, false, project_root.to_path_buf(), gpu.clone());
            }
            if let Some(err) = child.stderr.take() {
                relay_output(err, true, project_root.to_path_buf(), gpu.clone());
            }
            #[cfg(unix)]
            let pgid = child.id().map(|id| id as i32).unwrap_or(0);
            #[cfg(unix)]
            SIDECAR_PGID.store(pgid, std::sync::atomic::Ordering::SeqCst);
            let (tx, exited) = tokio::sync::watch::channel(false);
            tokio::spawn(async move {
                let _ = child.wait().await;
                #[cfg(unix)]
                let _ = SIDECAR_PGID.compare_exchange(
                    pgid,
                    0,
                    std::sync::atomic::Ordering::SeqCst,
                    std::sync::atomic::Ordering::SeqCst,
                );
                let _ = tx.send(true);
            });
            Some(Sidecar {
                url,
                bin,
                exited,
                gpu,
            })
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
    /// The backend abgen settled on, as one word for the banner. Read after
    /// [`Self::wait_ready`]: qualification runs during abgen's startup, so by
    /// the time /readyz answers the chatter has been relayed and parsed.
    pub fn backend_label(&self) -> &'static str {
        self.gpu
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .label()
    }

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

    fn scratch(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "dcl-one-sdk-{tag}-{}-{:x}",
            std::process::id(),
            rand::random::<u64>()
        ))
    }

    #[test]
    fn the_sidecar_never_prefers_the_dedicated_abgen_port() {
        assert_ne!(
            SIDECAR_PREFERRED_PORT, 5147,
            "5147 is the dedicated stack abgen's port (deploy/PORTS.md); a \
             sidecar preferring it races the stack unit for the bind"
        );
    }

    #[test]
    fn the_upstream_ab_cdn_is_never_this_sidecar() {
        std::env::remove_var("ABGEN_UPSTREAM_AB_CDN");
        let default = upstream_ab_cdn_default();
        assert!(default.starts_with("https://"), "{default}");
        assert!(!points_at_port(&default, 5147));
        assert_eq!(upstream_ab_cdn_for(5147), default);

        for form in [
            "http://127.0.0.1:5147",
            "http://localhost:5147/",
            "http://0.0.0.0:5147",
        ] {
            std::env::set_var("ABGEN_UPSTREAM_AB_CDN", form);
            assert_eq!(upstream_ab_cdn_for(5147), "", "{form}");
            assert_eq!(upstream_ab_cdn_for(5148), form);
        }

        std::env::set_var("ABGEN_UPSTREAM_AB_CDN", "https://ab-cdn.example.org");
        assert_eq!(upstream_ab_cdn_for(5147), "https://ab-cdn.example.org");
        std::env::remove_var("ABGEN_UPSTREAM_AB_CDN");
    }

    #[test]
    fn rewrite_build_line_formats_ok_fail_and_passthrough() {
        let root = scratch("abgen-line-test");
        std::fs::create_dir_all(root.join("images")).unwrap();
        std::fs::write(root.join("images/scene-thumbnail.png"), vec![0u8; 2048]).unwrap();

        let ok = r#"ABGEN_BUILD {"entity":"b64-x","entity_type":"scene","file":"images/scene-thumbnail.png","platform":"mac","hash":"b64-y","build_ms":16,"out_bytes":33866,"result":"ok"}"#;
        assert_eq!(
            rewrite_build_line(ok, &root),
            Some((
                "abgen build: images/scene-thumbnail.png (mac) 16.0 ms, in 2.0kb, out 33.1kb"
                    .to_string(),
                false
            ))
        );

        let fail = r#"ABGEN_BUILD {"file":"assets/tree.glb","platform":"windows","build_ms":6230,"result":"decode error"}"#;
        assert_eq!(
            rewrite_build_line(fail, &root),
            Some((
                "abgen build FAIL: assets/tree.glb (windows) 6.23 sec \u{2014} decode error"
                    .to_string(),
                true
            ))
        );

        assert_eq!(rewrite_build_line("plain log line", &root), None);
        assert_eq!(rewrite_build_line("ABGEN_BUILD not-json", &root), None);
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// The four startup lines the terminal used to carry fold into one word:
    /// a `qualified=true` line names the backend abgen picked (auto stops at
    /// its first success), and everything else means CPU. Runtime problems —
    /// a mid-run GPU panic, a forced-GPU error — are not qualification
    /// chatter and must keep flowing to the terminal.
    #[test]
    fn gpu_chatter_folds_into_one_backend_word() {
        let qual = std::sync::Mutex::new(GpuQual::default());
        for absorbed in [
            "abgen-gpu: qualification backend=cuda qualified=false reason=init failed: gpu init: loading the CUDA driver library failed (no NVIDIA driver?)",
            "abgen-gpu: qualification backend=wgpu qualified=false reason=init failed: no wgpu adapter",
            "abgen-gpu: qualification backend=auto qualified=false reason=auto: cuda backend disabled: init failed",
            "warning: no GPU available (auto: cuda backend disabled); continuing on CPU",
            "abgen-gpu: wgpu adapter: NVIDIA T4 (Vulkan)",
            "abgen-gpu: macOS default is CPU (integrated Metal is slower than the CPU for BC7); set ABGEN_GPU=1 or ABGEN_GPU_BACKEND=wgpu to force the GPU",
        ] {
            assert!(absorb_gpu_chatter(absorbed, &qual), "{absorbed}");
        }
        assert_eq!(qual.lock().unwrap().label(), "CPU");

        assert!(absorb_gpu_chatter(
            "abgen-gpu: qualification backend=wgpu qualified=true reason=-",
            &qual
        ));
        assert_eq!(qual.lock().unwrap().label(), "GPU");
        assert!(absorb_gpu_chatter(
            "abgen-gpu: qualification backend=cuda qualified=true reason=-",
            &qual
        ));
        assert_eq!(qual.lock().unwrap().label(), "CUDA", "cuda outranks wgpu");

        for passes_through in [
            "abgen-gpu: GPU init panicked; continuing on CPU",
            "error: ABGEN_GPU set but no GPU available: no adapter",
            "2026-08-26T00:00:00 INFO abgen: serving",
        ] {
            assert!(
                !absorb_gpu_chatter(passes_through, &qual),
                "{passes_through}"
            );
        }
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

        let deadline = Instant::now() + Duration::from_secs(5);
        while unsafe { libc::kill(-pgid, 0) } == 0 {
            assert!(
                Instant::now() < deadline,
                "process group {pgid} still alive after kill_process_group"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        kill_process_group(0);
        kill_process_group(pgid);
    }

    /// The scene's own `@dcl/abgen` package was once in the lookup chain; a
    /// scene that still has it installed must not swap the sidecar version.
    #[test]
    fn an_npm_abgen_in_the_scene_is_ignored() {
        let root = scratch("npm-abgen");
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
