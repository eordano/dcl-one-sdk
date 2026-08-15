use axum::body::{to_bytes, Body};
use axum::extract::State;
use axum::http::{Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use catalyrst_crypto::Wallet;
use serde_json::Value;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};

const BIN: &str = env!("CARGO_BIN_EXE_dcl-one-sdk");
const KEY: &str = "0000000000000000000000000000000000000000000000000000000000000001";

const SCENE_LAND: &str = r#"{"main":"bin/index.js","runtimeVersion":"7","scene":{"parcels":["52,-52"],"base":"52,-52"}}"#;
const SCENE_WORLD: &str = r#"{"main":"bin/index.js","runtimeVersion":"7","scene":{"parcels":["0,0"],"base":"0,0"},"worldConfiguration":{"name":"example.dcl.eth"}}"#;

struct Seen {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

struct Rule {
    method: &'static str,
    path: String,
    status: u16,
    body: String,
}

#[derive(Clone)]
struct Mock {
    seen: Arc<Mutex<Vec<Seen>>>,
    rules: Arc<Vec<Rule>>,
}

async fn handle(State(mock): State<Mock>, req: Request<Body>) -> Response {
    let method = req.method().as_str().to_string();
    let path = req.uri().path().to_string();
    let headers = req
        .headers()
        .iter()
        .map(|(k, v)| {
            (
                k.as_str().to_string(),
                String::from_utf8_lossy(v.as_bytes()).into_owned(),
            )
        })
        .collect();
    let body = to_bytes(req.into_body(), usize::MAX)
        .await
        .unwrap()
        .to_vec();
    mock.seen.lock().unwrap().push(Seen {
        method: method.clone(),
        path: path.clone(),
        headers,
        body,
    });
    for r in mock.rules.iter() {
        if r.method == method && r.path == path {
            return (StatusCode::from_u16(r.status).unwrap(), r.body.clone()).into_response();
        }
    }
    (StatusCode::NOT_FOUND, "no mock rule").into_response()
}

async fn serve(mut rules: Vec<Rule>) -> (String, Arc<Mutex<Vec<Seen>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    for r in &mut rules {
        r.body = r.body.replace("{base}", &base);
    }
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mock = Mock {
        seen: seen.clone(),
        rules: Arc::new(rules),
    };
    let app = Router::new().fallback(handle).with_state(mock);
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (base, seen)
}

struct Fixture(PathBuf);

impl Fixture {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "dcl-one-sdk-landdeploy-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Fixture(dir)
    }

    fn write(&self, rel: &str, contents: &str) {
        let p = self.0.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, contents).unwrap();
    }

    fn scene(tag: &str, scene_json: &str) -> Self {
        let f = Fixture::new(tag);
        f.write("scene.json", scene_json);
        f.write("bin/index.js", "console.log(1);\n");
        f
    }

    fn dir_arg(&self) -> String {
        self.0.display().to_string()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn run_bin(args: Vec<String>) -> Output {
    tokio::task::spawn_blocking(move || {
        let mut cmd = Command::new(BIN);
        cmd.args(&args);
        for k in ["RUST_LOG", "DCL_ONE_SDK_DEFAULT_TARGET"] {
            cmd.env_remove(k);
        }
        cmd.env("NO_COLOR", "1");
        cmd.env("DCL_PRIVATE_KEY", KEY);
        cmd.output().unwrap()
    })
    .await
    .unwrap()
}

fn combined(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn deploy_args(fix: &Fixture, target_content: &str) -> Vec<String> {
    vec![
        "deploy".into(),
        "--dir".into(),
        fix.dir_arg(),
        "--skip-build".into(),
        "--ci".into(),
        "--target-content".into(),
        target_content.into(),
    ]
}

fn header<'a>(seen: &'a Seen, name: &str) -> &'a str {
    seen.headers
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
        .unwrap_or_else(|| panic!("missing header {name}"))
}

fn address() -> String {
    Wallet::from_hex(KEY).unwrap().address()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn land_deploy_posts_entities_and_prints_the_network_scope_line() {
    let (base, seen) = serve(vec![Rule {
        method: "POST",
        path: "/entities".into(),
        status: 200,
        body: r#"{"creationTimestamp":1}"#.into(),
    }])
    .await;
    let fix = Fixture::scene("land", SCENE_LAND);
    let out = run_bin(deploy_args(&fix, &base)).await;
    let text = combined(&out);
    assert!(out.status.success(), "{text}");
    assert!(
        text.contains("not Genesis City on decentraland.org"),
        "{text}"
    );
    let seen = seen.lock().unwrap();
    let post = seen
        .iter()
        .find(|s| s.method == "POST" && s.path == "/entities")
        .expect("no POST /entities captured");
    let body = String::from_utf8_lossy(&post.body).to_lowercase();
    assert!(
        body.contains("name=\"entityid\""),
        "multipart lacks entityId"
    );
    assert!(
        body.contains(&address()),
        "multipart lacks the signer address"
    );
    assert!(body.contains("52,-52"), "multipart lacks the pointer");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn target_flag_resolves_the_content_url_via_about() {
    let (base, seen) = serve(vec![
        Rule {
            method: "GET",
            path: "/about".into(),
            status: 200,
            body: r#"{"healthy":true,"content":{"publicUrl":"{base}/content"}}"#.into(),
        },
        Rule {
            method: "POST",
            path: "/content/entities".into(),
            status: 200,
            body: r#"{"creationTimestamp":1}"#.into(),
        },
    ])
    .await;
    let fix = Fixture::scene("about", SCENE_LAND);
    let out = run_bin(vec![
        "deploy".into(),
        "--dir".into(),
        fix.dir_arg(),
        "--skip-build".into(),
        "--ci".into(),
        "--target".into(),
        base.clone(),
    ])
    .await;
    let text = combined(&out);
    assert!(out.status.success(), "{text}");
    assert!(
        text.contains("not Genesis City on decentraland.org"),
        "{text}"
    );
    let seen = seen.lock().unwrap();
    assert!(
        seen.iter()
            .any(|s| s.method == "POST" && s.path == "/content/entities"),
        "deploy did not follow content.publicUrl from /about"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unpublish_sends_a_signed_delete_to_the_scenes_route() {
    let (base, seen) = serve(vec![Rule {
        method: "DELETE",
        path: "/content/scenes/52,-52".into(),
        status: 200,
        body: "{}".into(),
    }])
    .await;
    let out = run_bin(vec![
        "unpublish".into(),
        "--parcel".into(),
        "52,-52".into(),
        "--target-content".into(),
        format!("{base}/content"),
    ])
    .await;
    let text = combined(&out);
    assert!(out.status.success(), "{text}");
    assert!(text.contains("Unpublished 52,-52"), "{text}");
    let seen = seen.lock().unwrap();
    let del = seen
        .iter()
        .find(|s| s.method == "DELETE")
        .expect("no DELETE captured");
    assert_eq!(del.path, "/content/scenes/52,-52");
    let ts = header(del, "x-identity-timestamp");
    assert_eq!(header(del, "x-identity-metadata"), "{}");
    let link0: Value = serde_json::from_str(header(del, "x-identity-auth-chain-0")).unwrap();
    assert_eq!(link0["type"], "SIGNER");
    assert_eq!(link0["payload"].as_str().unwrap().to_lowercase(), address());
    let link1: Value = serde_json::from_str(header(del, "x-identity-auth-chain-1")).unwrap();
    assert_eq!(link1["type"], "ECDSA_SIGNED_ENTITY");
    assert_eq!(
        link1["payload"].as_str().unwrap(),
        format!("delete:/content/scenes/52,-52:{ts}:{{}}")
    );
    assert!(link1["signature"].as_str().unwrap().starts_with("0x"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unpublish_maps_404_to_the_local_only_hint() {
    let (base, _seen) = serve(vec![Rule {
        method: "DELETE",
        path: "/scenes/9,9".into(),
        status: 404,
        body: "No locally published scene at 9,9".into(),
    }])
    .await;
    let out = run_bin(vec![
        "unpublish".into(),
        "--parcel".into(),
        "9,9".into(),
        "--target-content".into(),
        base,
    ])
    .await;
    let text = combined(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("refused to unpublish 9,9"), "{text}");
    assert!(
        text.contains("synced Genesis City entities are not deletable"),
        "{text}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn world_delete_falls_back_to_the_per_scene_route() {
    let addr = address();
    let (base, seen) = serve(vec![
        Rule {
            method: "GET",
            path: "/world/example.dcl.eth/scenes".into(),
            status: 200,
            body: r#"{"scenes":[{"entityId":"e1","parcels":["5,5"],"baseParcel":"5,5"}]}"#.into(),
        },
        Rule {
            method: "GET",
            path: "/world/example.dcl.eth/permissions".into(),
            status: 200,
            body: format!(r#"{{"owner":"{addr}"}}"#),
        },
        Rule {
            method: "DELETE",
            path: "/entities/example.dcl.eth".into(),
            status: 404,
            body: "not found".into(),
        },
        Rule {
            method: "DELETE",
            path: "/world/example.dcl.eth/scenes/5,5".into(),
            status: 200,
            body: "".into(),
        },
        Rule {
            method: "POST",
            path: "/entities".into(),
            status: 200,
            body: r#"{"creationTimestamp":1}"#.into(),
        },
    ])
    .await;
    let fix = Fixture::scene("worldfb", SCENE_WORLD);
    let mut args = deploy_args(&fix, &base);
    args.push("--yes".into());
    let out = run_bin(args).await;
    let text = combined(&out);
    assert!(out.status.success(), "{text}");
    let seen = seen.lock().unwrap();
    assert!(
        seen.iter()
            .any(|s| s.method == "DELETE" && s.path == "/entities/example.dcl.eth"),
        "upstream-shape delete was never attempted"
    );
    let per_scene = seen
        .iter()
        .find(|s| s.method == "DELETE" && s.path == "/world/example.dcl.eth/scenes/5,5")
        .expect("per-scene delete fallback did not fire");
    let ts = header(per_scene, "x-identity-timestamp");
    let link1: Value = serde_json::from_str(header(per_scene, "x-identity-auth-chain-1")).unwrap();
    assert_eq!(
        link1["payload"].as_str().unwrap(),
        format!("delete:/world/example.dcl.eth/scenes/5,5:{ts}:{{}}")
    );
    assert!(
        seen.iter()
            .any(|s| s.method == "POST" && s.path == "/entities"),
        "entity upload never happened"
    );
}
