//! Browser signing for world-management requests.
//!
//! The deploy linker (`crate::linker`) signs an entity id. This one signs the
//! ADR signed-fetch payload (`method:path:timestamp:{}`) that world settings
//! and permission changes authenticate with, so granting a deploy key no
//! longer requires exporting the owner's private key.
//!
//! Unlike the deploy flow, a rejection here is usually "wrong wallet
//! connected" rather than "the deployment is broken", so 401/403 keeps the
//! page alive and invites another attempt instead of killing the CLI.

use crate::linker::{fmt_wait, linker_bind_host, spawn_browser, LinkerOptions};
use crate::ux::{self, TrySteps, UserError};
use crate::world::{browser_headers, signed_fetch_payload, WorldAction};
use anyhow::{Context, Result};
use axum::{
    extract::State,
    response::Html,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

pub struct WorldSignRequest {
    pub base: String,
    pub name: String,
    pub action: WorldAction,
}

type DoneSender = tokio::sync::oneshot::Sender<Result<String>>;

pub struct WorldLinkerState {
    req: WorldSignRequest,
    /// Payloads minted by /api/info and not yet spent. Bounded so a page left
    /// reloading cannot grow it without limit.
    pending: Mutex<HashSet<String>>,
    done: Mutex<Option<DoneSender>>,
}

pub fn new_state(
    req: WorldSignRequest,
) -> (
    Arc<WorldLinkerState>,
    tokio::sync::oneshot::Receiver<Result<String>>,
) {
    let (tx, rx) = tokio::sync::oneshot::channel();
    (
        Arc::new(WorldLinkerState {
            req,
            pending: Mutex::new(HashSet::new()),
            done: Mutex::new(Some(tx)),
        }),
        rx,
    )
}

pub fn router(state: Arc<WorldLinkerState>) -> Router {
    Router::new()
        .route("/", get(page))
        .route("/api/info", get(info))
        .route("/api/sign", post(sign))
        .with_state(state)
}

async fn page() -> Html<&'static str> {
    Html(PAGE)
}

async fn info(State(st): State<Arc<WorldLinkerState>>) -> Json<Value> {
    let r = &st.req;
    let path = r.action.path(&r.name);
    let method = r.action.method();
    let payload = signed_fetch_payload(method, &path, crate::deploy::now_ms());
    {
        let mut pending = st.pending.lock().unwrap_or_else(PoisonError::into_inner);
        if pending.len() > 32 {
            pending.clear();
        }
        pending.insert(payload.clone());
    }
    let details = match &r.action {
        WorldAction::SettingsSet(update) => update.changed_fields(),
        WorldAction::Permission {
            permission,
            address,
            revoke,
        } => vec![
            format!("permission={permission}"),
            format!("address={address}"),
            format!("operation={}", if *revoke { "revoke" } else { "grant" }),
        ],
    };
    Json(json!({
        "world": r.name,
        "summary": r.action.summary(),
        "method": method.to_uppercase(),
        "path": path,
        "targetContent": r.base,
        "payload": payload,
        "details": details,
    }))
}

#[derive(Deserialize)]
struct SignReq {
    address: String,
    signature: String,
    payload: String,
}

async fn sign(State(st): State<Arc<WorldLinkerState>>, Json(req): Json<SignReq>) -> Json<Value> {
    let known = st
        .pending
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&req.payload);
    if !known {
        return Json(json!({
            "ok": false,
            "fatal": false,
            "error": "unknown or stale request — reload the page and sign again"
        }));
    }
    let r = &st.req;
    let headers = browser_headers(&req.address, &req.payload, &req.signature);
    match r.action.send(&r.base, &r.name, headers).await {
        Ok((status, body)) if (200..300).contains(&status) => {
            r.action.print_body(&body);
            let message = r.action.success(&r.name);
            finish(&st, Ok(message.clone()));
            Json(json!({ "ok": true, "message": message }))
        }
        Ok((status, body)) => {
            let detail = body.trim();
            let expired = is_expired_signature(detail);
            let retryable = expired || status == 401 || status == 403;
            let error = if expired {
                format!(
                    "the signature expired before it reached the server \u{2014} it is only valid for about a minute.{}",
                    if detail.is_empty() {
                        String::new()
                    } else {
                        format!("\n\n{detail}")
                    }
                )
            } else if retryable {
                format!(
                    "the worlds server refused this request (HTTP {status}) — {} is not allowed to {} on {}.{}",
                    req.address,
                    r.action.summary(),
                    r.name,
                    if detail.is_empty() {
                        String::new()
                    } else {
                        format!("\n\n{detail}")
                    }
                )
            } else {
                format!(
                    "the worlds server rejected this request (HTTP {status}){}",
                    if detail.is_empty() {
                        String::new()
                    } else {
                        format!("\n\n{detail}")
                    }
                )
            };
            if !retryable {
                let err = UserError::new(
                    format!(
                        "the worlds server refused to {} (HTTP {status})",
                        r.action.summary()
                    ),
                    TrySteps::one("read the server message above")
                        .and("re-run with --verbose for the full response"),
                );
                let err = if detail.is_empty() {
                    err
                } else {
                    err.why(detail.to_string())
                };
                finish(&st, Err(err.into()));
            }
            Json(json!({
                "ok": false,
                "fatal": !retryable,
                "expired": expired,
                "error": error,
            }))
        }
        Err(e) => {
            let msg = format!("{e:#}");
            finish(&st, Err(e));
            Json(json!({ "ok": false, "fatal": true, "error": msg }))
        }
    }
}

/// Does this server response mean the signed-fetch timestamp aged out?
///
/// The worlds server answers an expired payload with an "Expired signature"
/// error; matching on that (rather than on the status code, which is a plain
/// 401) is what lets the page distinguish "sign again" from "wrong wallet".
fn is_expired_signature(body: &str) -> bool {
    let lower = body.to_lowercase();
    lower.contains("expired signature") || lower.contains("timestamp expiration")
}

fn finish(st: &Arc<WorldLinkerState>, result: Result<String>) {
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

fn timeout_error(timeout: Duration, url: &str, summary: &str) -> anyhow::Error {
    UserError::new(
        format!(
            "no signature arrived within {} \u{2014} request abandoned",
            fmt_wait(timeout)
        ),
        TrySteps::one("re-run the command and sign on the printed URL")
            .and("raise the wait with DCL_ONE_SDK_LINKER_TIMEOUT_SECS=<seconds>")
            .and("for headless runs set DCL_PRIVATE_KEY or pass --sign-key <file>"),
    )
    .why(format!("the page at {url} was waiting to {summary}"))
    .into()
}

pub async fn run(req: WorldSignRequest, opts: LinkerOptions) -> Result<String> {
    let summary = req.action.summary();
    let world = req.name.clone();
    let (state, rx) = new_state(req);
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
    println!("Sign this change to {world} with your wallet in a browser:");
    println!("  {url}");
    ux::note(format!("the wallet must be allowed to {summary}"));
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
                TrySteps::one("re-run the command"),
            )
            .into())
        }
        res = rx => match res {
            Ok(outcome) => outcome,
            Err(_) => Err(UserError::new(
                "the signing flow ended without a result",
                TrySteps::one("re-run the command"),
            )
            .into()),
        },
        _ = tokio::time::sleep(opts.timeout) => Err(timeout_error(opts.timeout, &url, &summary)),
    }
}

const PAGE: &str = r##"<!doctype html>
<html lang="en"><head>
<meta charset="utf-8"/>
<meta name="viewport" content="width=device-width, initial-scale=1"/>
<title>dcl-one-sdk world — sign</title>
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
  ul{margin:6px 0 0;padding-left:18px;color:#b9c6dd;font-size:12px}
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
  <h1>World change <span class="t">·</span> signature required</h1>
  <p class="sub">Connect the wallet that owns this world (or holds the needed permission) and sign. The command line waits until you finish here.</p>
  <div class="kv">
    <b>World</b><code id="world">…</code>
    <b>Change</b><code id="summary">…</code>
    <b>Details</b><div><ul id="details"></ul></div>
    <b>Request</b><code id="req">…</code>
    <b>Server</b><code id="cs">…</code>
  </div>
  <button id="go" disabled>Connect wallet &amp; sign</button>
  <div id="status"></div>
</div>
<script>
let INFO=null;
const $=id=>document.getElementById(id);
function show(cls,msg){const s=$("status");s.className=cls;s.style.display="block";s.textContent=msg;}
function render(){
  $("world").textContent=INFO.world;
  $("summary").textContent=INFO.summary;
  $("details").innerHTML=INFO.details.map(d=>`<li>${d}</li>`).join("");
  $("req").textContent=INFO.method+" "+INFO.path;
  $("cs").textContent=INFO.targetContent;
}
async function load(){
  try{
    INFO=await (await fetch("api/info")).json();
    if(INFO.error){show("err",INFO.error);return;}
    render();
    $("go").disabled=false;
  }catch(e){show("err","Could not load the request: "+e);}
}
// The server accepts a signed payload for only ~60s, and a wallet prompt can
// easily outlast that. So: connect first (slowest step, done once), then mint
// and sign back-to-back, and if the payload still ages out, mint a fresh one
// and ask again rather than sending the user back to the command line.
const MAX_SIGN_ATTEMPTS=3;
async function attempt(address,left){
  INFO=await (await fetch("api/info")).json();
  if(INFO.error){show("err",INFO.error);return false;}
  render();
  show("info","Signing with "+address+" … (approve promptly — the signature is valid for about a minute)");
  const signature=await window.ethereum.request({method:"personal_sign",params:[INFO.payload,address]});
  show("info","Sending to "+INFO.targetContent+" …");
  const r=await (await fetch("api/sign",{method:"POST",headers:{"content-type":"application/json"},
    body:JSON.stringify({address,signature,payload:INFO.payload})})).json();
  if(r.ok){show("ok","✓ "+r.message+"\n\nYou can close this tab; the command line has finished.");return true;}
  if(r.expired&&left>0){
    show("info","That signature expired before it landed. Signing again with a fresh timestamp — please approve as soon as the prompt appears.");
    return attempt(address,left-1);
  }
  if(r.expired){show("err","✗ "+r.error+"\n\nGave up after "+MAX_SIGN_ATTEMPTS+" attempts. Press the button and approve the wallet prompt immediately.");return false;}
  if(r.fatal){show("err","✗ "+r.error+"\n\nThe command line exited with this error — fix it and re-run.");return false;}
  show("err","✗ "+r.error+"\n\nSwitch to a wallet that is allowed to make this change and press the button again.");
  return false;
}
$("go").onclick=async()=>{
  const btn=$("go");
  try{
    if(!window.ethereum){show("err","No wallet found. Open this page in a browser with MetaMask (or another EIP-1193 wallet).");return;}
    btn.disabled=true;
    show("info","Requesting wallet…");
    const accounts=await window.ethereum.request({method:"eth_requestAccounts"});
    const address=accounts[0];
    const ok=await attempt(address,MAX_SIGN_ATTEMPTS-1);
    if(!ok)btn.disabled=false;
  }catch(e){show("err","✗ "+(e&&e.message?e.message:e));btn.disabled=false;}
};
load();
</script></body></html>
"##;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::SettingsUpdate;

    fn permission_request(base: &str) -> WorldSignRequest {
        WorldSignRequest {
            base: base.to_string(),
            name: "Test.dcl.eth".to_string(),
            action: WorldAction::Permission {
                permission: "deployment".to_string(),
                address: "0x1111111111111111111111111111111111111111".to_string(),
                revoke: false,
            },
        }
    }

    async fn serve(
        req: WorldSignRequest,
    ) -> (
        String,
        tokio::sync::oneshot::Receiver<Result<String>>,
        tokio::task::JoinHandle<()>,
    ) {
        let (state, rx) = new_state(req);
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

    #[tokio::test]
    async fn info_describes_the_permission_change_and_mints_a_signable_payload() {
        let (base, _rx, handle) = serve(permission_request("http://127.0.0.1:9")).await;
        let client = reqwest::Client::new();

        let page = client.get(format!("{base}/")).send().await.unwrap();
        assert_eq!(page.status().as_u16(), 200);
        let body = page.text().await.unwrap();
        assert!(body.contains("personal_sign"));
        assert!(body.contains("api/info"));

        let info: Value = client
            .get(format!("{base}/api/info"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(info["world"], "Test.dcl.eth");
        assert_eq!(info["method"], "PUT");
        assert_eq!(
            info["path"],
            "/world/Test.dcl.eth/permissions/deployment/0x1111111111111111111111111111111111111111"
        );
        let payload = info["payload"].as_str().unwrap();
        assert!(payload.starts_with("put:/world/test.dcl.eth/permissions/deployment/"));
        assert_eq!(payload, payload.to_lowercase());
        handle.abort();
    }

    #[test]
    fn expiry_is_told_apart_from_a_permission_refusal() {
        let expired = r#"{"error":"Expired signature: signature timestamp: 1785345934007, timestamp expiration: 1785345994007, local timestamp: 1785346010626","message":"This endpoint requires a signed fetch request. See ADR-44."}"#;
        assert!(is_expired_signature(expired));
        assert!(is_expired_signature("expired signature"));
        assert!(!is_expired_signature(
            r#"{"error":"Not authorized","message":"wallet is not the owner"}"#
        ));
        assert!(!is_expired_signature(""));
    }

    #[tokio::test]
    async fn a_stale_payload_is_rejected_without_killing_the_cli() {
        let (base, _rx, handle) = serve(permission_request("http://127.0.0.1:9")).await;
        let stale: Value = reqwest::Client::new()
            .post(format!("{base}/api/sign"))
            .json(&json!({"address":"0x0","signature":"0x0","payload":"put:/nope:1:{}"}))
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
    async fn an_unreachable_server_is_fatal_and_resolves_the_cli() {
        let (base, rx, handle) = serve(permission_request("http://127.0.0.1:9")).await;
        let client = reqwest::Client::new();
        let info: Value = client
            .get(format!("{base}/api/info"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let payload = info["payload"].as_str().unwrap().to_string();
        let signer = crate::random_test_wallet();
        let sig = signer.sign_message(payload.as_bytes()).unwrap();
        let resp: Value = client
            .post(format!("{base}/api/sign"))
            .json(&json!({
                "address": signer.address(),
                "signature": sig,
                "payload": payload,
            }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(resp["ok"], false);
        assert_eq!(resp["fatal"], true);
        assert!(rx.await.unwrap().is_err());
        handle.abort();
    }

    #[tokio::test]
    async fn settings_updates_list_only_the_touched_fields() {
        let req = WorldSignRequest {
            base: "http://127.0.0.1:9".to_string(),
            name: "Test.dcl.eth".to_string(),
            action: WorldAction::SettingsSet(SettingsUpdate {
                title: Some("New Title".to_string()),
                description: None,
                content_rating: None,
                spawn_coordinates: None,
                skybox_time: None,
                single_player: Some(true),
                show_in_places: None,
                categories: Vec::new(),
                thumbnail: None,
            }),
        };
        let (base, _rx, handle) = serve(req).await;
        let info: Value = reqwest::Client::new()
            .get(format!("{base}/api/info"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let details: Vec<String> = info["details"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d.as_str().unwrap().to_string())
            .collect();
        assert_eq!(details, vec!["title=New Title", "single_player=true"]);
        assert_eq!(info["path"], "/world/Test.dcl.eth/settings");
        handle.abort();
    }
}
