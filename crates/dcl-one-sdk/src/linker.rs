use crate::deploy::{self, Prepared};
use crate::ux::{self, TrySteps, UserError};
use anyhow::{Context, Result};
use axum::{
    extract::State,
    response::Html,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

pub struct LinkerDeploy {
    pub prepared: Prepared,
    pub target_content: String,
    pub world: Option<String>,
    pub needs_delete: bool,
    pub timestamp_override: Option<i64>,
    pub entity_out: Option<PathBuf>,
    pub scene_title: String,
    pub base_parcel: String,
    pub multi_scene: bool,
    pub check_permissions: bool,
}

pub struct LinkerOptions {
    pub port: Option<u16>,
    pub open_browser: bool,
    pub timeout: Duration,
}

pub const DEFAULT_TIMEOUT_SECS: u64 = 600;

pub fn linker_timeout() -> Duration {
    let secs = std::env::var("DCL_ONE_SDK_LINKER_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|s| *s > 0)
        .unwrap_or(DEFAULT_TIMEOUT_SECS);
    Duration::from_secs(secs)
}

struct PendingEntity {
    bytes: Vec<u8>,
    delete_payload: Option<String>,
}

type DoneSender = tokio::sync::oneshot::Sender<Result<String>>;

pub struct LinkerState {
    dep: LinkerDeploy,
    pending: Mutex<HashMap<String, PendingEntity>>,
    done: Mutex<Option<DoneSender>>,
}

pub fn new_state(
    dep: LinkerDeploy,
) -> (
    Arc<LinkerState>,
    tokio::sync::oneshot::Receiver<Result<String>>,
) {
    let (tx, rx) = tokio::sync::oneshot::channel();
    (
        Arc::new(LinkerState {
            dep,
            pending: Mutex::new(HashMap::new()),
            done: Mutex::new(Some(tx)),
        }),
        rx,
    )
}

pub fn router(state: Arc<LinkerState>) -> Router {
    Router::new()
        .route("/", get(page))
        .route("/api/info", get(info))
        .route("/api/sign", post(sign))
        .with_state(state)
}

async fn page() -> Html<&'static str> {
    Html(PAGE)
}

async fn info(State(st): State<Arc<LinkerState>>) -> Json<Value> {
    let d = &st.dep;
    let ts = d.timestamp_override.unwrap_or_else(deploy::now_ms);
    let (entity_id, entity_bytes) = match deploy::build_entity(&d.prepared, ts) {
        Ok(x) => x,
        Err(e) => return Json(json!({ "error": format!("could not build the entity: {e:#}") })),
    };
    let delete_payload = if d.needs_delete {
        d.world.as_deref().map(deploy::build_delete_payload)
    } else {
        None
    };
    {
        let mut pending = st.pending.lock().unwrap_or_else(PoisonError::into_inner);
        if pending.len() > 32 {
            pending.clear();
        }
        pending.insert(
            entity_id.clone(),
            PendingEntity {
                bytes: entity_bytes,
                delete_payload: delete_payload.clone(),
            },
        );
    }
    let play_url = match &d.world {
        Some(w) => format!("https://decentraland.org/play/?realm={w}"),
        None => format!(
            "https://play.decentraland.org/?NETWORK=mainnet&position={}",
            d.base_parcel
        ),
    };
    Json(json!({
        "sceneTitle": d.scene_title,
        "baseParcel": d.base_parcel,
        "parcels": d.prepared.pointers,
        "world": d.world,
        "targetContent": d.target_content,
        "entityId": entity_id,
        "timestamp": ts,
        "deletePayload": delete_payload,
        "multiScene": d.multi_scene,
        "playUrl": play_url,
        "files": d.prepared.files.iter().map(|(f, h, b)| json!({"file": f, "hash": h, "size": b.len()})).collect::<Vec<_>>(),
    }))
}

#[derive(Deserialize)]
struct SignReq {
    address: String,
    signature: String,
    #[serde(rename = "entityId")]
    entity_id: String,
    #[serde(rename = "deleteSignature")]
    delete_signature: Option<String>,
}

async fn sign(State(st): State<Arc<LinkerState>>, Json(req): Json<SignReq>) -> Json<Value> {
    let pending = st
        .pending
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&req.entity_id);
    let Some(pending) = pending else {
        return Json(json!({
            "ok": false,
            "fatal": false,
            "error": "unknown or stale entity id — reload the page and sign again"
        }));
    };
    if st.dep.check_permissions {
        if let Some(w) = st.dep.world.as_deref() {
            if let Err(e) = deploy::enforce_world_permission(
                &st.dep.target_content,
                w,
                &req.address,
                &st.dep.prepared.pointers,
            )
            .await
            {
                let msg = format!("{e:#}");
                finish(&st, Err(e));
                return Json(json!({ "ok": false, "fatal": true, "error": msg }));
            }
        }
    }
    if let Some(payload) = &pending.delete_payload {
        let Some(dsig) = &req.delete_signature else {
            return Json(json!({
                "ok": false,
                "fatal": false,
                "error": "this deploy also removes the existing world scenes and needs the second signature — reload and sign both prompts"
            }));
        };
        let chain = deploy::simple_auth_chain(&req.address, payload, dsig);
        if let Err(e) = deploy::send_world_delete(
            &st.dep.target_content,
            st.dep.world.as_deref().unwrap_or_default(),
            &chain,
        )
        .await
        {
            let msg = format!("{e:#}");
            finish(&st, Err(e));
            return Json(json!({ "ok": false, "fatal": true, "error": msg }));
        }
    }
    match deploy::upload_entity(
        &st.dep.target_content,
        &req.entity_id,
        pending.bytes.clone(),
        &st.dep.prepared.files,
        &req.address,
        &req.signature,
    )
    .await
    {
        Ok(message) => {
            if let Some(path) = &st.dep.entity_out {
                if let Err(e) = std::fs::write(path, &pending.bytes) {
                    tracing::warn!("could not write --entity-out {}: {e}", path.display());
                }
            }
            finish(&st, Ok(message.clone()));
            Json(json!({ "ok": true, "message": message }))
        }
        Err(e) => {
            let msg = format!("{e:#}");
            finish(&st, Err(e));
            Json(json!({ "ok": false, "fatal": true, "error": msg }))
        }
    }
}

fn finish(st: &Arc<LinkerState>, result: Result<String>) {
    let tx = st
        .done
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    if let Some(tx) = tx {
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(150)).await;
            let _ = tx.send(result);
        });
    }
}

fn spawn_browser(url: &str) {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "xdg-open"
    };
    let spawned = std::process::Command::new(program)
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    if spawned.is_err() {
        ux::note("could not open a browser automatically \u{2014} open the URL above manually");
    }
}

fn fmt_wait(timeout: Duration) -> String {
    let secs = timeout.as_secs();
    if secs >= 60 && secs.is_multiple_of(60) {
        let mins = secs / 60;
        if mins == 1 {
            "1 minute".to_string()
        } else {
            format!("{mins} minutes")
        }
    } else if secs == 1 {
        "1 second".to_string()
    } else {
        format!("{secs} seconds")
    }
}

fn timeout_error(timeout: Duration, url: &str) -> anyhow::Error {
    UserError::new(
        format!(
            "no signature arrived within {} \u{2014} deployment abandoned",
            fmt_wait(timeout)
        ),
        TrySteps::one("re-run dcl-one-sdk deploy and sign on the printed URL")
            .and("raise the wait with DCL_ONE_SDK_LINKER_TIMEOUT_SECS=<seconds>")
            .and("for headless deploys set DCL_PRIVATE_KEY or pass --sign-key <file>"),
    )
    .why(format!("the signing page was served at {url}"))
    .into()
}

pub fn linker_bind_host() -> String {
    std::env::var("DCL_ONE_SDK_LINKER_HOST")
        .ok()
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "127.0.0.1".to_string())
}

pub async fn run(dep: LinkerDeploy, opts: LinkerOptions) -> Result<String> {
    let (state, rx) = new_state(dep);
    let app = router(state);
    let bind_host = linker_bind_host();
    let loopback = bind_host == "127.0.0.1";
    let listener = tokio::net::TcpListener::bind((bind_host.as_str(), opts.port.unwrap_or(0)))
        .await
        .map_err(|e| {
            anyhow::Error::from(
                UserError::new(
                    match opts.port {
                        Some(p) => format!("port {p} cannot be opened for the signing page"),
                        None => "no port could be opened for the signing page".to_string(),
                    },
                    TrySteps::one("pass --port <free-port> or free the port and retry"),
                )
                .caused_by(e),
            )
        })?;
    let port = listener
        .local_addr()
        .context("reading the signing page port")?
        .port();
    let url = format!("http://localhost:{port}/");
    println!();
    println!("Sign the deployment with your wallet in a browser:");
    println!("  {url}");
    if loopback {
        ux::note("to sign from another device, re-run with DCL_ONE_SDK_LINKER_HOST=0.0.0.0");
    } else {
        ux::note("from another device, replace localhost with this machine's address");
    }
    if opts.open_browser {
        spawn_browser(&url);
    } else {
        ux::note("browser auto-open disabled \u{2014} open the URL manually");
    }
    let serve = axum::serve(listener, app);
    tokio::select! {
        r = serve => {
            r.context("serving the signing page")?;
            Err(UserError::new(
                "the signing page stopped before a signature arrived",
                TrySteps::one("re-run dcl-one-sdk deploy"),
            )
            .into())
        }
        res = rx => match res {
            Ok(outcome) => outcome,
            Err(_) => Err(UserError::new(
                "the signing flow ended without a result",
                TrySteps::one("re-run dcl-one-sdk deploy"),
            )
            .into()),
        },
        _ = tokio::time::sleep(opts.timeout) => Err(timeout_error(opts.timeout, &url)),
    }
}

const PAGE: &str = r##"<!doctype html>
<html lang="en"><head>
<meta charset="utf-8"/>
<meta name="viewport" content="width=device-width, initial-scale=1"/>
<title>dcl-one-sdk deploy — sign</title>
<style>
  :root{color-scheme:dark}
  body{margin:0;background:#0b0e16;color:#e6ebf2;font:15px/1.5 system-ui,sans-serif;
       display:flex;min-height:100vh;align-items:center;justify-content:center}
  .card{width:min(720px,92vw);background:#12151e;border:1px solid #24314d;border-radius:16px;
        padding:28px 30px;box-shadow:0 20px 60px #0008;margin:24px 0}
  h1{margin:0 0 4px;font-size:20px}
  h1 .t{color:#22e6d0}
  .sub{color:#8ea1c0;margin:0 0 20px;font-size:13px}
  .kv{display:grid;grid-template-columns:130px 1fr;gap:6px 12px;font-size:13px;margin:14px 0}
  .kv b{color:#8ea1c0;font-weight:500}
  code{font-family:ui-monospace,monospace;word-break:break-all;color:#cfe}
  ul{margin:6px 0 0;padding-left:18px;color:#b9c6dd;font-size:12px;max-height:200px;overflow:auto}
  .warn{margin-top:14px;padding:10px 12px;border-radius:10px;font-size:13px;display:none;
        background:#2a2114;border:1px solid #6b5526;color:#ffd28a}
  button{margin-top:18px;width:100%;padding:13px;border:0;border-radius:10px;cursor:pointer;
         font-size:15px;font-weight:600;background:#22e6d0;color:#04121a}
  button:disabled{opacity:.5;cursor:default}
  #status{margin-top:16px;padding:12px 14px;border-radius:10px;font-size:13px;white-space:pre-wrap;display:none}
  .ok{background:#0e2a1e;border:1px solid #1f6b46;color:#8ff0bf}
  .err{background:#2a1414;border:1px solid #6b2626;color:#ff9a9a}
  .info{background:#101a2c;border:1px solid #24314d;color:#a9c0e6}
  a{color:#22e6d0}
</style></head>
<body><div class="card">
  <h1>Deploy <span class="t">·</span> signature required</h1>
  <p class="sub">Connect the wallet that may publish this scene and sign the deployment. The command line waits until you finish here.</p>
  <div class="kv">
    <b>Scene</b><code id="title">…</code>
    <b>Deploying to</b><code id="where">…</code>
    <b>Parcels</b><code id="parcels">…</code>
    <b>Content server</b><code id="cs">…</code>
    <b>Entity id</b><code id="eid">…</code>
    <b>Files</b><div><ul id="files"></ul><div id="total" style="font-size:12px;color:#5d6c8c;margin-top:4px"></div></div>
  </div>
  <div class="warn" id="delwarn">This deploy also REMOVES the scenes currently published on other parcels of the world. Your wallet will ask for a second signature authorizing the removal.</div>
  <button id="go" disabled>Connect wallet &amp; sign &amp; deploy</button>
  <div id="status"></div>
</div>
<script>
let INFO=null;
const $=id=>document.getElementById(id);
function show(cls,msg){const s=$("status");s.className=cls;s.style.display="block";s.textContent=msg;}
function fmt(n){return n>=1048576?(n/1048576).toFixed(2)+" MB":n>=1024?(n/1024).toFixed(1)+" KB":n+" B";}
function render(){
  $("title").textContent=INFO.sceneTitle;
  $("where").textContent=INFO.world?("world "+INFO.world+(INFO.multiScene?" (multi-scene, additive)":"")):"Genesis City LAND";
  $("parcels").textContent=INFO.parcels.join("  ")+"  (base "+INFO.baseParcel+")";
  $("cs").textContent=INFO.targetContent;
  $("eid").textContent=INFO.entityId;
  $("files").innerHTML=INFO.files.map(f=>`<li>${f.file} <span style="color:#5d6c8c">(${fmt(f.size)})</span></li>`).join("");
  const total=INFO.files.reduce((a,f)=>a+f.size,0);
  $("total").textContent=INFO.files.length+" files, "+fmt(total)+" total";
  $("delwarn").style.display=INFO.deletePayload?"block":"none";
}
async function load(){
  try{
    INFO=await (await fetch("api/info")).json();
    if(INFO.error){show("err",INFO.error);return;}
    render();
    $("go").disabled=false;
  }catch(e){show("err","Could not load deployment info: "+e);}
}
$("go").onclick=async()=>{
  const btn=$("go");
  try{
    if(!window.ethereum){show("err","No wallet found. Open this page in a browser with MetaMask (or another EIP-1193 wallet).");return;}
    btn.disabled=true;
    show("info","Requesting wallet…");
    const accounts=await window.ethereum.request({method:"eth_requestAccounts"});
    const address=accounts[0];
    show("info","Preparing a fresh deployment…");
    INFO=await (await fetch("api/info")).json();
    if(INFO.error){show("err",INFO.error);btn.disabled=false;return;}
    render();
    show("info","Signing entity id with "+address+" …");
    const signature=await window.ethereum.request({method:"personal_sign",params:[INFO.entityId,address]});
    let deleteSignature=null;
    if(INFO.deletePayload){
      show("info","Signing the scene-removal authorization…");
      deleteSignature=await window.ethereum.request({method:"personal_sign",params:[INFO.deletePayload,address]});
    }
    show("info","Uploading deployment to "+INFO.targetContent+" …");
    const r=await (await fetch("api/sign",{method:"POST",headers:{"content-type":"application/json"},
      body:JSON.stringify({address,signature,entityId:INFO.entityId,deleteSignature})})).json();
    if(r.ok){show("ok","✓ "+r.message+"\n\nOpen: "+INFO.playUrl+"\n\nYou can close this tab; the command line has finished.");}
    else if(r.fatal){show("err","✗ "+r.error+"\n\nThe command line exited with this error — fix it and re-run the deploy.");}
    else{show("err","✗ "+r.error);btn.disabled=false;}
  }catch(e){show("err","✗ "+(e&&e.message?e.message:e));btn.disabled=false;}
};
load();
</script></body></html>
"##;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Project;
    use std::path::Path;

    struct TempTree(PathBuf);

    impl TempTree {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "dcl-one-sdk-linker-test-{tag}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TempTree(dir)
        }

        fn write(&self, rel: &str, contents: &str) {
            let p = self.0.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, contents).unwrap();
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn fixture(tag: &str, world: Option<&str>) -> (TempTree, LinkerDeploy) {
        let t = TempTree::new(tag);
        let world_cfg = match world {
            Some(w) => format!(",\"worldConfiguration\":{{\"name\":\"{w}\"}}"),
            None => String::new(),
        };
        t.write(
            "scene.json",
            &format!(
                "{{\"runtimeVersion\":\"7\",\"main\":\"bin/index.js\",\"display\":{{\"title\":\"Linker Smoke\"}},\"scene\":{{\"parcels\":[\"0,0\"],\"base\":\"0,0\"}}{world_cfg}}}"
            ),
        );
        t.write("bin/index.js", "console.log(\"linker\");\n");
        let project = Project::load(&t.0).unwrap();
        let prepared = deploy::prepare(&project).unwrap();
        let dep = LinkerDeploy {
            prepared,
            target_content: "http://127.0.0.1:9".to_string(),
            world: world.map(str::to_string),
            needs_delete: false,
            timestamp_override: None,
            entity_out: None,
            scene_title: "Linker Smoke".to_string(),
            base_parcel: "0,0".to_string(),
            multi_scene: false,
            check_permissions: false,
        };
        (t, dep)
    }

    async fn serve(
        dep: LinkerDeploy,
    ) -> (
        String,
        tokio::sync::oneshot::Receiver<Result<String>>,
        tokio::task::JoinHandle<()>,
    ) {
        let (state, rx) = new_state(dep);
        let app = router(state);
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (base, rx, handle)
    }

    #[test]
    fn linker_bind_host_defaults_to_loopback() {
        std::env::remove_var("DCL_ONE_SDK_LINKER_HOST");
        assert_eq!(linker_bind_host(), "127.0.0.1");
        std::env::set_var("DCL_ONE_SDK_LINKER_HOST", "0.0.0.0");
        assert_eq!(linker_bind_host(), "0.0.0.0");
        std::env::remove_var("DCL_ONE_SDK_LINKER_HOST");
    }

    #[tokio::test]
    async fn page_and_info_and_stale_sign_smoke() {
        let (_t, dep) = fixture("smoke", None);
        let (base, _rx, handle) = serve(dep).await;
        let client = reqwest::Client::new();

        let page = client.get(format!("{base}/")).send().await.unwrap();
        assert_eq!(page.status().as_u16(), 200);
        let body = page.text().await.unwrap();
        assert!(body.contains("api/info"));
        assert!(body.contains("personal_sign"));

        let info: serde_json::Value = client
            .get(format!("{base}/api/info"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(info["entityId"].as_str().unwrap().starts_with("bafkrei"));
        assert_eq!(info["sceneTitle"], "Linker Smoke");
        assert_eq!(info["files"].as_array().unwrap().len(), 2);
        assert!(info["deletePayload"].is_null());

        let stale: serde_json::Value = client
            .post(format!("{base}/api/sign"))
            .json(&serde_json::json!({"address":"0x0","signature":"0x0","entityId":"bogus"}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(stale["ok"], false);
        assert_eq!(stale["fatal"], false);
        handle.abort();
    }

    #[tokio::test]
    async fn unreachable_target_fails_fatal_and_resolves_cli() {
        let (_t, dep) = fixture("fatal", None);
        let (base, rx, handle) = serve(dep).await;
        let client = reqwest::Client::new();
        let info: serde_json::Value = client
            .get(format!("{base}/api/info"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let entity_id = info["entityId"].as_str().unwrap().to_string();
        let signer = crate::random_test_wallet();
        let sig = signer.sign_message(entity_id.as_bytes()).unwrap();
        let resp: serde_json::Value = client
            .post(format!("{base}/api/sign"))
            .json(&serde_json::json!({
                "address": signer.address(),
                "signature": sig,
                "entityId": entity_id,
            }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(resp["ok"], false);
        assert_eq!(resp["fatal"], true);
        let outcome = rx.await.unwrap();
        assert!(outcome.is_err());
        handle.abort();
    }

    #[tokio::test]
    async fn sign_flow_completes_against_local_worlds() {
        let Ok(key_path) = std::env::var("DCL_ONE_SDK_LINKER_SMOKE_KEY") else {
            eprintln!("skipping: DCL_ONE_SDK_LINKER_SMOKE_KEY not set");
            return;
        };
        let target = std::env::var("DCL_ONE_SDK_LINKER_SMOKE_TARGET")
            .unwrap_or_else(|_| "http://127.0.0.1:5142".to_string());
        let world = std::env::var("DCL_ONE_SDK_LINKER_SMOKE_WORLD")
            .unwrap_or_else(|_| "dcl1test.dcl.eth".to_string());
        let raw = std::fs::read_to_string(Path::new(&key_path)).unwrap();
        let signer = catalyrst_crypto::Wallet::from_hex(&raw).unwrap();

        let (_t, mut dep) = fixture("worlds", Some(&world));
        dep.target_content = target;
        let (base, rx, handle) = serve(dep).await;
        let client = reqwest::Client::new();
        let page = client.get(format!("{base}/")).send().await.unwrap();
        assert_eq!(page.status().as_u16(), 200);
        let info: serde_json::Value = client
            .get(format!("{base}/api/info"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let entity_id = info["entityId"].as_str().unwrap().to_string();
        let sig = signer.sign_message(entity_id.as_bytes()).unwrap();
        let resp: serde_json::Value = client
            .post(format!("{base}/api/sign"))
            .json(&serde_json::json!({
                "address": signer.address(),
                "signature": sig,
                "entityId": entity_id,
            }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(resp["ok"], true, "sign flow failed: {resp}");
        let outcome = rx.await.unwrap();
        let message = outcome.unwrap();
        assert!(
            message.contains("Deployed"),
            "unexpected message: {message}"
        );
        handle.abort();
    }
}
