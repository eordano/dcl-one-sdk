use dcl_one_sdk::scene::{b64_hash, machine_id};
use futures::StreamExt;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime};
use tokio_tungstenite::tungstenite::Message;

const BIN: &str = env!("CARGO_BIN_EXE_dcl-one-sdk");

fn sandbox_node_modules() -> Option<PathBuf> {
    std::env::var_os("DCL_ONE_SDK_TEST_NODE_MODULES")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
}

fn scene_json(title: &str, parcel: &str) -> String {
    json!({
        "display": { "title": title },
        "main": "bin/index.js",
        "runtimeVersion": "7",
        "scene": { "parcels": [parcel], "base": parcel }
    })
    .to_string()
}

const TSCONFIG: &str = r#"{
  "compilerOptions": { "strict": true, "baseUrl": "." },
  "include": ["src/**/*.ts"],
  "extends": "@dcl/sdk/types/tsconfig.ecs7.json"
}"#;

fn write(path: &Path, contents: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn make_member(root: &Path, name: &str, title: &str, parcel: &str, node_modules: &Path) {
    let member = root.join(name);
    write(&member.join("scene.json"), &scene_json(title, parcel));
    write(&member.join("tsconfig.json"), TSCONFIG);
    write(&member.join("src/index.ts"), "export function main() {}\n");
    let status = Command::new("cp")
        .arg("-al")
        .arg(node_modules)
        .arg(member.join("node_modules"))
        .status()
        .expect("cp -al node_modules");
    assert!(status.success(), "hardlinking node_modules into {name}");
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
    let deadline = Instant::now() + Duration::from_secs(60);
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_member_workspace_builds_serves_and_reloads_per_member() {
    let Some(node_modules) = sandbox_node_modules() else {
        eprintln!(
            "skipping workspace e2e: set DCL_ONE_SDK_TEST_NODE_MODULES to a scene node_modules dir (same filesystem) to run it"
        );
        return;
    };

    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("dcl-one-sdk-ws-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    write(
        &root.join("dcl-workspace.json"),
        r#"{"folders":[{"path":"scene-a"},{"path":"scene-b"}]}"#,
    );
    make_member(&root, "scene-a", "WS A", "0,0", &node_modules);
    make_member(&root, "scene-b", "WS B", "1,0", &node_modules);
    let root_str = root.display().to_string();

    let out = Command::new(BIN)
        .args(["build", "--dir", &root_str, "--skip-type-check"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "workspace build failed\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(stdout.contains("[1/2] in scene-a:"), "stdout: {stdout}");
    assert!(stdout.contains("[2/2] in scene-b:"), "stdout: {stdout}");
    let a_bin = root.join("scene-a/bin/index.js");
    let b_bin = root.join("scene-b/bin/index.js");
    assert!(a_bin.is_file());
    assert!(b_bin.is_file());

    let canon = root.canonicalize().unwrap();
    let machine = machine_id();
    let id_a = b64_hash(&canon.join("scene-a").display().to_string(), &machine);
    let id_b = b64_hash(&canon.join("scene-b").display().to_string(), &machine);

    let port = free_port();
    let log = std::fs::File::create(root.join("start.log")).unwrap();
    let child = Command::new(BIN)
        .args([
            "start",
            "--dir",
            &root_str,
            "--port",
            &port.to_string(),
            "--skip-build",
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
    let about = wait_for_about(&base, &client).await;

    let scenes_urn: Vec<String> = about["configurations"]["scenesUrn"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(scenes_urn.len(), 2, "about: {about}");
    assert!(scenes_urn[0].contains(&id_a));
    assert!(scenes_urn[1].contains(&id_b));
    let parcels: Vec<String> = about["configurations"]["localSceneParcels"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(parcels.contains(&"0,0".to_string()));
    assert!(parcels.contains(&"1,0".to_string()));

    let both: Value = client
        .post(format!("{base}/content/entities/active"))
        .json(&json!({ "pointers": ["0,0", "1,0"] }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let both = both.as_array().unwrap();
    assert_eq!(both.len(), 2, "entities/active union: {both:?}");
    assert_eq!(both[0]["id"], json!(id_a));
    assert_eq!(both[1]["id"], json!(id_b));
    assert!(both[1]["content"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["file"] == json!("bin/index.js")));

    let only_b: Value = client
        .post(format!("{base}/content/entities/active"))
        .json(&json!({ "pointers": ["1,0"] }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let only_b = only_b.as_array().unwrap();
    assert_eq!(only_b.len(), 1);
    assert_eq!(only_b[0]["id"], json!(id_b));

    let only_a: Value = client
        .get(format!("{base}/content/entities/scene?pointer=0,0"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let only_a = only_a.as_array().unwrap();
    assert_eq!(only_a.len(), 1);
    assert_eq!(only_a[0]["id"], json!(id_a));

    let entity_b: Value = client
        .get(format!("{base}/content/contents/{id_b}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(entity_b["id"], json!(id_b));
    assert_eq!(entity_b["pointers"], json!(["1,0"]));
    let b_bundle_hash = entity_b["content"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["file"] == json!("bin/index.js"))
        .unwrap()["hash"]
        .as_str()
        .unwrap()
        .to_string();
    let served = client
        .get(format!("{base}/content/contents/{b_bundle_hash}"))
        .send()
        .await
        .unwrap();
    assert!(served.status().is_success());
    let served_bytes = served.bytes().await.unwrap();
    assert_eq!(served_bytes.as_ref(), std::fs::read(&b_bin).unwrap());

    let (ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/"))
        .await
        .expect("scene-update websocket");
    let (_, mut ws_rx) = ws.split();

    let a_before = mtime(&a_bin);
    let b_before = mtime(&b_bin);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let b_src = root.join("scene-b/src/index.ts");
    std::fs::write(
        &b_src,
        "export function main() {}\nexport const workspaceEdit = 2\n",
    )
    .unwrap();

    let mut update_ids = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(45);
    while update_ids.is_empty() && Instant::now() < deadline {
        let Ok(Some(Ok(msg))) = tokio::time::timeout(Duration::from_secs(5), ws_rx.next()).await
        else {
            continue;
        };
        if let Message::Text(text) = msg {
            let v: Value = serde_json::from_str(&text).unwrap();
            if v["type"] == json!("SCENE_UPDATE") {
                update_ids.push(v["payload"]["sceneId"].as_str().unwrap().to_string());
            }
        }
    }
    assert_eq!(
        update_ids,
        vec![id_b.clone()],
        "expected exactly one SCENE_UPDATE for scene-b"
    );

    let settle = Instant::now() + Duration::from_secs(2);
    while Instant::now() < settle {
        let Ok(Some(Ok(msg))) =
            tokio::time::timeout(Duration::from_millis(500), ws_rx.next()).await
        else {
            continue;
        };
        if let Message::Text(text) = msg {
            let v: Value = serde_json::from_str(&text).unwrap();
            if v["type"] == json!("SCENE_UPDATE") {
                assert_ne!(
                    v["payload"]["sceneId"],
                    json!(id_a),
                    "scene-a must not rebuild on a scene-b edit"
                );
            }
        }
    }

    assert!(mtime(&b_bin) > b_before, "scene-b bundle must be rebuilt");
    assert_eq!(
        mtime(&a_bin),
        a_before,
        "scene-a bundle must stay untouched"
    );

    drop(_guard);
    let _ = std::fs::remove_dir_all(&root);
}
