use futures::StreamExt;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime};
use tokio_tungstenite::tungstenite::Message;

const BIN: &str = env!("CARGO_BIN_EXE_dcl-one-sdk");

const CLIENT_SCRIPT: &str = r#"
import { createRequire } from 'node:module'
import path from 'node:path'

const projectDir = path.resolve(process.argv[2])
const wsUrl = process.argv[3]
const req = createRequire(path.join(projectDir, 'package.json'))

const { createRpcClient } = req('@dcl/rpc')
const codegen = req('@dcl/rpc/dist/codegen')
const { WebSocketTransport } = req('@dcl/rpc/dist/transports/WebSocket')
const WebSocket = req('ws')
const { DataServiceDefinition } = req('@dcl/inspector')

const ws = new WebSocket(wsUrl)
ws.binaryType = 'arraybuffer'
const transport = WebSocketTransport(ws)
const client = await createRpcClient(transport)
const port = await client.createPort('scene-ctx')
const service = codegen.loadService(port, DataServiceDefinition)

const prefs = await service.getInspectorPreferences({})
console.log('PREFS', JSON.stringify(prefs))

function crdtPutTransform(entityId, timestamp, x) {
  const data = Buffer.alloc(44)
  data.writeFloatLE(x, 0)
  data.writeFloatLE(1.0, 4)
  data.writeFloatLE(8.0, 8)
  data.writeFloatLE(0, 12)
  data.writeFloatLE(0, 16)
  data.writeFloatLE(0, 20)
  data.writeFloatLE(1.0, 24)
  data.writeFloatLE(1.0, 28)
  data.writeFloatLE(1.0, 32)
  data.writeFloatLE(1.0, 36)
  data.writeUInt32LE(0, 40)
  const msg = Buffer.alloc(24 + data.length)
  msg.writeUInt32LE(24 + data.length, 0)
  msg.writeUInt32LE(1, 4)
  msg.writeUInt32LE(entityId, 8)
  msg.writeUInt32LE(1, 12)
  msg.writeUInt32LE(timestamp, 16)
  msg.writeUInt32LE(data.length, 20)
  data.copy(msg, 24)
  return msg
}

const outgoing = []
let push
const queue = (m) => {
  outgoing.push(m)
  if (push) push()
}
async function* stream() {
  while (true) {
    while (outgoing.length) yield { data: outgoing.shift() }
    await new Promise((r) => (push = r))
  }
}

const incoming = service.crdtStream(stream())
const first = await incoming.next()
console.log('INITIAL_STATE_BYTES', first.value?.data?.length ?? 0)

queue(new Uint8Array(crdtPutTransform(515, 1, 7.5)))
await new Promise((r) => setTimeout(r, 500))

await service.save({})
console.log('SAVED')
process.exit(0)
"#;

fn sandbox_node_modules() -> Option<PathBuf> {
    std::env::var_os("DCL_ONE_SDK_TEST_NODE_MODULES")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
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

fn mtime(path: &Path) -> SystemTime {
    std::fs::metadata(path).unwrap().modified().unwrap()
}

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

async fn wait_for_about(base: &str, client: &reqwest::Client) -> Value {
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        if let Ok(resp) = client.get(format!("{base}/about")).send().await {
            if resp.status().is_success() {
                return resp.json().await.unwrap();
            }
        }
        assert!(
            Instant::now() < deadline,
            "preview server did not come up on {base}"
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

fn wait_for_file(path: &Path, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while !path.is_file() {
        assert!(
            Instant::now() < deadline,
            "{} did not appear",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(250));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn data_layer_rpc_edit_saves_composite_and_reloads() {
    let Some(node_modules) = sandbox_node_modules() else {
        eprintln!(
            "skipping data-layer e2e: set DCL_ONE_SDK_TEST_NODE_MODULES to a scene node_modules dir (same filesystem) to run it"
        );
        return;
    };
    let Some(node) = dcl_one_sdk::build::find_node() else {
        eprintln!("skipping data-layer e2e: node not on PATH");
        return;
    };

    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("dcl-one-sdk-dl-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let scene = root.join("scene");
    write(
        &scene.join("scene.json"),
        &json!({
            "display": { "title": "DL E2E" },
            "main": "bin/index.js",
            "runtimeVersion": "7",
            "scene": { "parcels": ["0,0"], "base": "0,0" }
        })
        .to_string(),
    );
    write(
        &scene.join("tsconfig.json"),
        "{\n  \"compilerOptions\": { \"strict\": true },\n  \"include\": [\"src/**/*.ts\"],\n  \"extends\": \"@dcl/sdk/types/tsconfig.ecs7.json\"\n}",
    );
    write(&scene.join("src/index.ts"), "export function main() {}\n");
    write(&scene.join("package.json"), "{\"name\":\"dl-e2e\"}");
    let status = Command::new("cp")
        .arg("-al")
        .arg(&node_modules)
        .arg(scene.join("node_modules"))
        .status()
        .expect("cp -al node_modules");
    assert!(status.success(), "hardlinking node_modules");
    write(&root.join("client.mjs"), CLIENT_SCRIPT);

    let port = free_port();
    let log = std::fs::File::create(root.join("start.log")).unwrap();
    let child = Command::new(BIN)
        .args([
            "start",
            "--dir",
            &scene.display().to_string(),
            "--port",
            &port.to_string(),
            "--data-layer",
            "--offline-comms",
        ])
        .stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap();
    let _guard = ChildGuard(child);

    let base = format!("http://127.0.0.1:{port}");
    let client = reqwest::Client::new();
    wait_for_about(&base, &client).await;
    eprintln!("phase: server up on {base}");

    let index = client
        .get(format!("{base}/inspector/"))
        .send()
        .await
        .unwrap();
    assert!(index.status().is_success());
    let html = index.text().await.unwrap();
    assert!(
        html.contains(&format!(
            "const config = '{{\"dataLayerRpcWsUrl\":\"ws://127.0.0.1:{port}/data-layer\"}}'"
        )),
        "config not injected: {html}"
    );
    let bundle = client
        .get(format!("{base}/inspector/bundle.js"))
        .send()
        .await
        .unwrap();
    assert!(bundle.status().is_success());
    assert_eq!(
        bundle.headers()["content-type"].to_str().unwrap(),
        "application/javascript"
    );

    let composite = scene.join("assets/scene/main.composite");
    wait_for_file(&composite, Duration::from_secs(60));
    eprintln!("phase: data-layer boot wrote {}", composite.display());
    let composite_before = std::fs::read_to_string(&composite).unwrap();
    assert!(!composite_before.contains("7.5"));

    let (ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/"))
        .await
        .expect("scene-update websocket");
    let (_, mut ws_rx) = ws.split();

    let out = Command::new(&node)
        .arg(root.join("client.mjs"))
        .arg(&scene)
        .arg(format!("ws://127.0.0.1:{port}/data-layer"))
        .output()
        .expect("node rpc client");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "rpc client failed\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(stdout.contains("PREFS {"), "stdout: {stdout}");
    assert!(stdout.contains("SAVED"), "stdout: {stdout}");
    let state_bytes: u64 = stdout
        .lines()
        .find_map(|l| l.strip_prefix("INITIAL_STATE_BYTES "))
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0);
    assert!(state_bytes > 0, "no initial crdt state\nstdout: {stdout}");
    eprintln!("phase: rpc client done (initial state {state_bytes} bytes)");

    let mut got_update = false;
    let deadline = Instant::now() + Duration::from_secs(60);
    while !got_update && Instant::now() < deadline {
        let Ok(Some(Ok(msg))) = tokio::time::timeout(Duration::from_secs(5), ws_rx.next()).await
        else {
            continue;
        };
        if let Message::Text(text) = msg {
            let v: Value = serde_json::from_str(&text).unwrap();
            if v["type"] == json!("SCENE_UPDATE") {
                got_update = true;
            }
        }
    }
    assert!(got_update, "no SCENE_UPDATE after the editor save");
    eprintln!("phase: SCENE_UPDATE received");

    let composite_after = std::fs::read_to_string(&composite).unwrap();
    assert_ne!(composite_before, composite_after);
    assert!(
        composite_after.contains("7.5"),
        "saved composite misses the injected transform"
    );

    let crdt = scene.join("main.crdt");
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if crdt.is_file() && mtime(&crdt) >= mtime(&composite) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "main.crdt was not regenerated after the composite save"
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    assert!(std::fs::metadata(&crdt).unwrap().len() > 0);
    eprintln!(
        "phase: main.crdt regenerated ({} bytes)",
        std::fs::metadata(&crdt).unwrap().len()
    );

    drop(_guard);
    let _ = std::fs::remove_dir_all(&root);
}
