//! `start` with nothing but a scene carries voice: the livekit-server every
//! binary embeds comes up next to the preview, `/about` hands out
//! `signed-login:`, a wallet-signed POST gets a `livekit:` adapter on the
//! preview's own host whose token that very server accepts, the scene
//! gatekeeper mints a second room the same way, and the server dies with the
//! preview. Once on a bare `start` with the build, watcher, sidecar and MCP
//! switched off, and once the way a user runs it: `init`, then `start` in
//! that directory with no flags at all. Skips (through the test gate) on a
//! build that embeds no server and finds none on PATH, which is what macOS
//! gets.
mod common;

use base64::Engine;
use common::random_wallet;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_dcl-one-sdk");

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn write(path: &Path, contents: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn free_port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn fresh_tmp(name: &str) -> PathBuf {
    let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("dcl-one-sdk-livekit-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    tmp
}

/// `start` as the tests run it, logging to `log` and with the `lk` CLI's
/// variables out of the way so a shell set up for an external server does
/// not turn this into the external-server test.
fn start_command(log: &Path) -> Command {
    let log = std::fs::File::create(log).unwrap();
    let mut cmd = Command::new(BIN);
    cmd.arg("start")
        .env_remove("LIVEKIT_URL")
        .env_remove("LIVEKIT_API_KEY")
        .env_remove("LIVEKIT_API_SECRET")
        .stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log);
    cmd
}

fn log_text(log: &Path) -> String {
    std::fs::read_to_string(log).unwrap_or_default()
}

/// The port the banner says the preview took, once it says so.
async fn wait_for_banner_port(log: &Path, guard: &mut ChildGuard) -> u16 {
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        let text = log_text(log);
        if let Some(port) = text.lines().find_map(|l| {
            let l = l.trim();
            l.strip_prefix("Local:")
                .and_then(|rest| rest.trim().strip_prefix("http://127.0.0.1:"))
                .and_then(|p| p.trim().parse::<u16>().ok())
        }) {
            return port;
        }
        if let Some(status) = guard.0.try_wait().unwrap() {
            panic!("start exited ({status}) before printing its port:\n{text}");
        }
        assert!(
            Instant::now() < deadline,
            "start never printed its Local URL:\n{text}"
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

async fn wait_for_about(base: &str, client: &reqwest::Client, log: &Path) -> Value {
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        if let Ok(resp) = client.get(format!("{base}/about")).send().await {
            if resp.status().is_success() {
                return resp.json().await.unwrap();
            }
        }
        assert!(
            Instant::now() < deadline,
            "preview server did not come up on {base}:\n{}",
            log_text(log)
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

fn token_of(adapter: &str) -> &str {
    adapter
        .split_once("?access_token=")
        .unwrap_or_else(|| panic!("no access_token in the adapter {adapter}"))
        .1
}

/// `livekit:ws://127.0.0.1:7883?access_token=…` -> `http://127.0.0.1:7883`.
fn sfu_of(adapter: &str) -> String {
    let ws = adapter
        .strip_prefix("livekit:")
        .unwrap_or_else(|| panic!("not a livekit adapter: {adapter}"))
        .split_once('?')
        .unwrap()
        .0;
    ws.replacen("ws://", "http://", 1)
}

fn claims_of(token: &str) -> Value {
    let payload = token.split('.').nth(1).unwrap();
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .unwrap();
    serde_json::from_slice(&raw).unwrap()
}

async fn validate(client: &reqwest::Client, sfu: &str, token: &str) -> (u16, String) {
    let resp = client
        .get(format!("{sfu}/rtc/validate"))
        .query(&[("access_token", token)])
        .send()
        .await
        .expect("the embedded SFU answers /rtc/validate");
    let status = resp.status().as_u16();
    (status, resp.text().await.unwrap_or_default())
}

fn signed(
    req: reqwest::RequestBuilder,
    wallet: &catalyrst_crypto::Wallet,
    path: &str,
) -> reqwest::RequestBuilder {
    dcl_one_sdk::world::signed_headers(wallet, "post", path)
        .unwrap()
        .into_iter()
        .fold(req, |req, (k, v)| req.header(k, v))
}

/// Everything a preview with its own server must do, from `/about` to the
/// scene gatekeeper. `None` is the gated skip of a build without a server.
async fn assert_voice(base: &str, client: &reqwest::Client, log: &Path) -> Option<String> {
    let about = wait_for_about(base, client, log).await;
    let fixed = about["comms"]["fixedAdapter"]
        .as_str()
        .expect("fixedAdapter")
        .to_string();
    if fixed.starts_with("ws-room:") {
        let text = log_text(log);
        assert!(
            text.contains("voice off"),
            "a bare start fell back to the ws-room without saying why:\n{text}"
        );
        let detail = text
            .lines()
            .find(|l| l.contains("voice off"))
            .unwrap_or_default()
            .to_string();
        return common::testgate::unavailable(
            "a livekit-server (embedded, LIVEKIT_SERVER_BIN or on PATH)",
            &detail,
        );
    }
    assert_eq!(fixed, format!("signed-login:{base}/signed-login"));
    assert_eq!(
        about["comms"]["gatekeeperUrl"],
        format!("{base}/get-scene-adapter")
    );
    let banner = log_text(log);
    assert!(
        banner.contains("Voice: comms on ") && banner.contains(", scene:"),
        "the banner must say voice is on, which server carries it and the scene room:\n{banner}"
    );
    println!("PASS start advertises signed-login on {base}");

    let unsigned = client
        .post(format!("{base}/signed-login"))
        .send()
        .await
        .unwrap();
    assert!(
        unsigned.status().is_client_error(),
        "an unsigned login got {}",
        unsigned.status()
    );

    let wallet = random_wallet();
    let me = wallet.address().to_lowercase();
    let resp = signed(
        client.post(format!("{base}/signed-login")),
        &wallet,
        "/signed-login",
    )
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 200, "{}", resp.text().await.unwrap());
    let login: Value = resp.json().await.unwrap();
    let adapter = login["fixedAdapter"].as_str().expect("fixedAdapter");
    assert!(
        adapter.starts_with("livekit:ws://127.0.0.1:"),
        "the embedded server is advertised on the host the peer dialled: {adapter}"
    );
    let sfu = sfu_of(adapter);
    let token = token_of(adapter);
    let claims = claims_of(token);
    assert_eq!(claims["sub"], me);
    assert_eq!(
        claims["iss"], "dcl-one-sdk",
        "the preview's own key, not some other server's"
    );
    assert_eq!(claims["video"]["room"], "LocalPreview");
    let (status, body) = validate(client, &sfu, token).await;
    assert_eq!(
        status, 200,
        "the embedded SFU refused the realm token: {body}"
    );
    println!("PASS realm token for {me} accepted by the embedded SFU at {sfu}");

    // Flip the first character of the signature: it carries six bits of it,
    // where the last one is mostly base64 padding a lenient decoder ignores.
    let (signed_part, sig) = token.rsplit_once('.').unwrap();
    let flipped = if sig.starts_with('A') { 'B' } else { 'A' };
    let tampered = format!("{signed_part}.{flipped}{}", &sig[1..]);
    let (status, _) = validate(client, &sfu, &tampered).await;
    assert_ne!(status, 200, "a tampered token must be refused");

    let resp = signed(
        client.post(format!("{base}/get-scene-adapter")),
        &wallet,
        "/get-scene-adapter",
    )
    .json(&json!({ "sceneId": "bafkreiscene" }))
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), 200, "{}", resp.text().await.unwrap());
    let scene: Value = resp.json().await.unwrap();
    let scene_adapter = scene["adapter"].as_str().expect("adapter");
    assert_eq!(sfu_of(scene_adapter), sfu);
    let scene_token = token_of(scene_adapter);
    assert_eq!(
        claims_of(scene_token)["video"]["room"],
        "scene:LocalPreview:bafkreiscene"
    );
    let (status, body) = validate(client, &sfu, scene_token).await;
    assert_eq!(
        status, 200,
        "the embedded SFU refused the scene token: {body}"
    );
    println!("PASS scene room token accepted by the embedded SFU");
    Some(sfu)
}

/// The server is the preview's: a clean stop takes it down too.
async fn assert_dies_with_preview(
    guard: &mut ChildGuard,
    client: &reqwest::Client,
    sfu: &str,
    log: &Path,
) {
    #[cfg(unix)]
    {
        let pid = guard.0.id() as i32;
        assert_eq!(unsafe { libc::kill(pid, libc::SIGTERM) }, 0);
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if guard.0.try_wait().unwrap().is_some() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "the preview did not exit on SIGTERM:\n{}",
                log_text(log)
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match client.get(format!("{sfu}/")).send().await {
                Err(_) => break,
                Ok(_) => assert!(
                    Instant::now() < deadline,
                    "the embedded livekit-server at {sfu} outlived the preview"
                ),
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        println!("PASS the embedded livekit-server at {sfu} died with the preview");
    }
    #[cfg(not(unix))]
    {
        let _ = (guard, client, sfu, log);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_bare_start_carries_voice_on_its_own_livekit_server() {
    let tmp = fresh_tmp("bare");
    let root = tmp.join("scene");
    write(
        &root.join("scene.json"),
        &json!({
            "display": { "title": "Voice" },
            "main": "bin/index.js",
            "runtimeVersion": "7",
            "scene": { "parcels": ["0,0"], "base": "0,0" }
        })
        .to_string(),
    );
    write(&root.join("bin/index.js"), "export function main() {}\n");

    let port = free_port();
    let log = tmp.join("start.log");
    let child = start_command(&log)
        .args([
            "--dir",
            &root.display().to_string(),
            "--port",
            &port.to_string(),
            "--skip-build",
            "--no-watch",
            "--no-asset-bundles",
            "--no-mcp",
        ])
        .spawn()
        .unwrap();
    let mut guard = ChildGuard(child);

    let base = format!("http://127.0.0.1:{port}");
    let client = reqwest::Client::new();
    let Some(sfu) = assert_voice(&base, &client, &log).await else {
        return;
    };
    assert_dies_with_preview(&mut guard, &client, &sfu, &log).await;
}

/// The user's path, flag for flag: `dcl-one-sdk init`, then `dcl-one-sdk
/// start` in that directory with every default in force — the build, the
/// watcher, the asset-bundle sidecar, MCP, and the port it picks itself.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn init_then_a_flagless_start_carries_voice() {
    let tmp = fresh_tmp("default");
    let scene = tmp.join("scene");
    let init = Command::new(BIN)
        .args(["init", "--dir", &scene.display().to_string()])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        init.status.success(),
        "init failed:\n{}",
        String::from_utf8_lossy(&init.stderr)
    );
    assert!(scene.join("node_modules/@dcl/sdk").is_dir());

    let log = tmp.join("start.log");
    let child = start_command(&log).current_dir(&scene).spawn().unwrap();
    let mut guard = ChildGuard(child);

    let port = wait_for_banner_port(&log, &mut guard).await;
    let base = format!("http://127.0.0.1:{port}");
    println!("PASS a flagless start built the scene and took port {port}");
    let client = reqwest::Client::new();
    let Some(sfu) = assert_voice(&base, &client, &log).await else {
        return;
    };
    let banner = log_text(&log);
    assert!(
        banner.contains("Preview server ready"),
        "the banner must announce the preview:\n{banner}"
    );
    assert_dies_with_preview(&mut guard, &client, &sfu, &log).await;
}
