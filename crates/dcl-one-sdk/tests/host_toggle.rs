//! The /scene page's Multiplayer switch, end to end against a running
//! preview: `POST /scene-json` with `authoritativeMultiplayer` writes the
//! flag, the watcher's rebuild re-arms the loader stub and attaches the server
//! isolate, `/scene` reports it running, and switching it off takes the key,
//! the isolate and the loader's arming away again, with no restart between.
mod common;

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

fn free_port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

async fn wait_until(what: &str, log: &Path, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(120);
    while !done() {
        assert!(
            Instant::now() < deadline,
            "never saw {what}:\n{}",
            std::fs::read_to_string(log).unwrap_or_default()
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

fn logged(log: &Path, needle: &str) -> bool {
    std::fs::read_to_string(log).is_ok_and(|text| text.contains(needle))
}

fn flag(scene: &Path) -> Option<Value> {
    let json: Value =
        serde_json::from_str(&std::fs::read_to_string(scene.join("scene.json")).unwrap()).unwrap();
    json.get("authoritativeMultiplayer").cloned()
}

fn loader_armed(scene: &Path) -> Option<bool> {
    let stub = std::fs::read_to_string(scene.join("bin/index.js")).ok()?;
    if stub.contains("var __dclOneMp = true") {
        Some(true)
    } else if stub.contains("var __dclOneMp = false") {
        Some(false)
    } else {
        None
    }
}

async fn flip(client: &reqwest::Client, base: &str, on: bool) {
    let resp = client
        .post(format!("{base}/scene-json"))
        .json(&json!({ "authoritativeMultiplayer": on }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "{}", resp.status());
}

async fn server_state(client: &reqwest::Client, base: &str) -> String {
    let html = client
        .get(format!("{base}/scene"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let at = html.find("data-server=\"").expect("the Multiplayer pane") + "data-server=\"".len();
    html[at..][..html[at..].find('"').unwrap()].to_string()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_switch_attaches_and_drops_the_server_under_a_running_preview() {
    if dcl_one_sdk::build::find_node().is_none() {
        let _: Option<()> = common::testgate::unavailable(
            "node",
            "install node; the authoritative host runs the scene under it",
        );
        return;
    }
    let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("dcl-one-sdk-host-toggle-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let scene = tmp.join("scene");
    let init = Command::new(BIN)
        .args(["init", "--yes", "--dir", &scene.display().to_string()])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        init.status.success(),
        "init failed:\n{}",
        String::from_utf8_lossy(&init.stderr)
    );
    assert_eq!(flag(&scene), None, "a fresh scaffold has no server");

    let port = free_port();
    let log = tmp.join("start.log");
    let out = std::fs::File::create(&log).unwrap();
    let child = Command::new(BIN)
        .args([
            "start",
            "--dir",
            &scene.display().to_string(),
            "--port",
            &port.to_string(),
            "--no-livekit",
            "--no-asset-bundles",
            "--no-mcp",
            "--skip-type-check",
        ])
        .env_remove("RUST_LOG")
        .stdin(Stdio::null())
        .stdout(out.try_clone().unwrap())
        .stderr(out)
        .spawn()
        .unwrap();
    let _guard = ChildGuard(child);

    let base = format!("http://127.0.0.1:{port}");
    let client = reqwest::Client::new();
    wait_until("the preview", &log, || logged(&log, "Preview server ready")).await;
    assert_eq!(server_state(&client, &base).await, "off");
    assert_eq!(loader_armed(&scene), Some(false));
    assert!(!logged(&log, "authoritative host attached"));

    flip(&client, &base, true).await;
    assert_eq!(flag(&scene), Some(json!(true)));
    wait_until("the host in the room", &log, || {
        logged(&log, "host joined the room")
    })
    .await;
    assert_eq!(
        loader_armed(&scene),
        Some(true),
        "the rebuild re-arms the loader, or clients never trust the server"
    );
    assert_eq!(server_state(&client, &base).await, "running");
    println!("PASS the switch attached the server with no restart");

    flip(&client, &base, false).await;
    assert_eq!(flag(&scene), None, "off is the absent key");
    wait_until("the host leaving", &log, || logged(&log, "host detached")).await;
    assert_eq!(loader_armed(&scene), Some(false));
    assert_eq!(server_state(&client, &base).await, "off");
}
