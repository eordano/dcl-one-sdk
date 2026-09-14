//! `start --livekit-url` against a LiveKit server that is actually running:
//! `/about` hands out `signed-login:`, a wallet-signed POST to it gets a
//! `livekit:` adapter whose token the SFU itself accepts (`/rtc/validate`),
//! and the scene gatekeeper mints a second room the same way. Ignored unless
//! `LIVEKIT_URL`, `LIVEKIT_API_KEY` and `LIVEKIT_API_SECRET` (the `lk` CLI's
//! variables) name the server; see docs/testing.md.
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

async fn wait_for_about(base: &str, client: &reqwest::Client, log: &Path) -> Value {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Ok(resp) = client.get(format!("{base}/about")).send().await {
            if resp.status().is_success() {
                return resp.json().await.unwrap();
            }
        }
        assert!(
            Instant::now() < deadline,
            "preview server did not come up on {base}:\n{}",
            std::fs::read_to_string(log).unwrap_or_default()
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// The websocket form `start` advertises, whichever form the env gave.
fn ws_form(url: &str) -> String {
    let url = url.trim().trim_end_matches('/');
    url.replacen("https://", "wss://", 1)
        .replacen("http://", "ws://", 1)
}

fn http_form(url: &str) -> String {
    let url = url.trim().trim_end_matches('/');
    url.replacen("wss://", "https://", 1)
        .replacen("ws://", "http://", 1)
}

fn token_of(adapter: &str) -> &str {
    adapter
        .split_once("?access_token=")
        .unwrap_or_else(|| panic!("no access_token in the adapter {adapter}"))
        .1
}

fn claims_of(token: &str) -> Value {
    let payload = token.split('.').nth(1).unwrap();
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .unwrap();
    serde_json::from_slice(&raw).unwrap()
}

/// What the SFU says about a token; only the status matters, the body is
/// `success` or the reason.
async fn validate(client: &reqwest::Client, sfu: &str, token: &str) -> (u16, String) {
    let resp = client
        .get(format!("{sfu}/rtc/validate"))
        .query(&[("access_token", token)])
        .send()
        .await
        .expect("the SFU answers /rtc/validate");
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs LIVEKIT_URL, LIVEKIT_API_KEY and LIVEKIT_API_SECRET: a running LiveKit server; see docs/testing.md"]
async fn a_livekit_preview_mints_tokens_the_sfu_accepts() {
    let Some(url) = common::testgate::require_env("LIVEKIT_URL") else {
        return;
    };
    let Some(api_key) = common::testgate::require_env("LIVEKIT_API_KEY") else {
        return;
    };
    let Some(api_secret) = common::testgate::require_env("LIVEKIT_API_SECRET") else {
        return;
    };
    let sfu = std::env::var("LIVEKIT_HTTP_URL")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| http_form(&url));

    let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("dcl-one-sdk-livekit-live-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
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
    let secret_file = tmp.join("livekit-secret");
    write(&secret_file, &format!("{api_secret}\n"));

    let port = free_port();
    let log_path = tmp.join("start.log");
    let log = std::fs::File::create(&log_path).unwrap();
    let child = Command::new(BIN)
        .args([
            "start",
            "--dir",
            &root.display().to_string(),
            "--port",
            &port.to_string(),
            "--skip-build",
            "--no-watch",
            "--no-asset-bundles",
            "--no-mcp",
            "--livekit-url",
            &url,
            "--livekit-api-key",
            &api_key,
            "--livekit-api-secret-file",
            &secret_file.display().to_string(),
        ])
        .env_remove("LIVEKIT_API_SECRET")
        .stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap();
    let _guard = ChildGuard(child);

    let base = format!("http://127.0.0.1:{port}");
    let client = reqwest::Client::new();
    let about = wait_for_about(&base, &client, &log_path).await;
    assert_eq!(
        about["comms"]["fixedAdapter"],
        format!("signed-login:{base}/signed-login")
    );
    assert_eq!(
        about["comms"]["gatekeeperUrl"],
        format!("{base}/get-scene-adapter")
    );
    println!("PASS /about advertises signed-login on {base}");

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
    assert_eq!(
        resp.status(),
        200,
        "{}",
        resp.text().await.unwrap_or_default()
    );
    let login: Value = resp.json().await.unwrap();
    let adapter = login["fixedAdapter"].as_str().expect("fixedAdapter");
    let expected_prefix = format!("livekit:{}?access_token=", ws_form(&url));
    assert!(
        adapter.starts_with(&expected_prefix),
        "adapter does not dial {expected_prefix}"
    );
    let realm_token = token_of(adapter);
    let claims = claims_of(realm_token);
    assert_eq!(claims["sub"], me);
    assert_eq!(claims["iss"], api_key);
    assert_eq!(claims["video"]["room"], "LocalPreview");
    let (status, body) = validate(&client, &sfu, realm_token).await;
    assert_eq!(status, 200, "the SFU refused the realm token: {body}");
    println!("PASS realm room token for {me} accepted by {sfu}/rtc/validate");

    // Flip the first character of the signature: it carries six bits of it,
    // where the last one is mostly base64 padding a lenient decoder ignores.
    let (signed_part, sig) = realm_token.rsplit_once('.').unwrap();
    let flipped = if sig.starts_with('A') { 'B' } else { 'A' };
    let forged = format!("{signed_part}.{flipped}{}", &sig[1..]);
    let (status, _) = validate(&client, &sfu, &forged).await;
    assert_ne!(
        status, 200,
        "the SFU accepted a tampered token, so validate proves nothing"
    );

    let scene_id = "bafkreilivekitlive";
    let resp = signed(
        client.post(format!("{base}/get-scene-adapter")),
        &wallet,
        "/get-scene-adapter",
    )
    .json(&json!({ "sceneId": scene_id, "realmName": "LocalPreview" }))
    .send()
    .await
    .unwrap();
    assert_eq!(
        resp.status(),
        200,
        "{}",
        resp.text().await.unwrap_or_default()
    );
    let minted: Value = resp.json().await.unwrap();
    let adapter = minted["adapter"].as_str().expect("adapter");
    assert!(
        adapter.starts_with(&expected_prefix),
        "scene adapter does not dial {expected_prefix}"
    );
    let scene_token = token_of(adapter);
    let claims = claims_of(scene_token);
    assert_eq!(claims["sub"], me);
    assert_eq!(
        claims["video"]["room"],
        format!("scene:LocalPreview:{scene_id}")
    );
    let (status, body) = validate(&client, &sfu, scene_token).await;
    assert_eq!(status, 200, "the SFU refused the scene token: {body}");
    println!("PASS scene room token for {me} accepted by {sfu}/rtc/validate");

    let _ = std::fs::remove_dir_all(&tmp);
}
