use super::{assistant_history as history, assistant_provider as provider, AppState};
use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::{mpsc, watch},
};

type Failure = (StatusCode, String);
#[derive(Default)]
pub(super) struct Assistant {
    pub(super) active: Mutex<Option<(String, watch::Sender<bool>)>>,
}

pub(super) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/project/assistant/conversations", get(history::list))
        .route(
            "/api/project/assistant/conversations/{id}",
            get(history::get).delete(history::remove),
        )
        .route("/api/project/assistant/providers", get(providers))
        .route("/api/project/assistant/turn", post(turn))
        .route("/api/project/assistant/turn/{id}", delete(cancel))
        .route("/api/project/assistant/mcp", post(mcp_proxy))
        .route(
            "/api/project/assistant/bridge",
            get(super::assistant_bridge::upgrade),
        )
}

pub(super) fn mcp_url() -> Option<String> {
    local_mcp_url(&std::env::var("DCL_ONE_SDK_SCENE_MCP_URL").ok()?)
}

fn local_mcp_url(value: &str) -> Option<String> {
    let parsed = url::Url::parse(value).ok()?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.username() != ""
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return None;
    }
    match parsed.host()? {
        url::Host::Domain("localhost") => Some(value.to_owned()),
        url::Host::Ipv4(ip) if ip.is_loopback() => Some(value.to_owned()),
        url::Host::Ipv6(ip) if ip.is_loopback() => Some(value.to_owned()),
        _ => None,
    }
}

async fn mcp_request(url: &str, body: Value) -> Result<Value, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;
    let mut request = client
        .post(url)
        .header("Accept", "application/json, text/event-stream")
        .json(&body);
    if let Ok(token) = std::env::var("DCL_SCENE_MCP_BEARER") {
        request = request.bearer_auth(token);
    }
    let response = request.send().await.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("Scene tools returned HTTP {}", response.status()));
    }
    response.json().await.map_err(|e| e.to_string())
}

async fn scene_tools() -> Value {
    let Some(url) = mcp_url() else {
        return json!({"available":false,"url":null,"paired":false});
    };
    let init = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"dcl-one-sdk","version":env!("CARGO_PKG_VERSION")}}});
    let available = tokio::time::timeout(Duration::from_secs(2), mcp_request(&url, init))
        .await
        .ok()
        .and_then(Result::ok)
        .is_some_and(|v| v["result"]["serverInfo"].is_object());
    let paired = if available {
        let status = json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"session_status","arguments":{"include":[]}}});
        tokio::time::timeout(Duration::from_secs(2), mcp_request(&url, status))
            .await
            .ok()
            .and_then(Result::ok)
            .is_some_and(|v| v["result"]["structuredContent"]["paired"] == true)
    } else {
        false
    };
    json!({"available":available,"url":url,"paired":paired})
}

async fn providers(State(st): State<Arc<AppState>>, headers: HeaderMap) -> Json<Value> {
    let providers:Vec<Value>=provider::PROVIDERS.iter().map(|(id,label,bin)|json!({"id":id,"label":label,"available":provider::executable(bin).is_some()})).collect();
    let busy = st
        .assistant
        .active
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .is_some();
    let mut tools = scene_tools().await;
    tools["bridge"] =
        if tools["available"] == true && super::assistant_bridge::configured().is_some() {
            json!(format!(
                "{}/api/project/assistant/bridge",
                super::http::preview_ws_origin(&headers)
            ))
        } else {
            Value::Null
        };
    Json(json!({"providers":providers,"sceneTools":tools,"busy":busy}))
}

async fn mcp_proxy(Json(body): Json<Value>) -> Result<Json<Value>, Failure> {
    let url = mcp_url().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Scene tools are not configured".into(),
    ))?;
    mcp_request(&url, body)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Turn {
    provider: String,
    prompt: String,
    session_id: Option<String>,
    conversation_id: Option<String>,
    #[serde(default)]
    selected_entities: Vec<history::EntityContext>,
    model: Option<String>,
}

pub(super) fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 200
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.:/".contains(c))
}

async fn turn(
    State(st): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(input): Json<Turn>,
) -> Result<Response, Failure> {
    let project = super::project_bridge::single_project(&st)?;
    if input.prompt.trim().is_empty() || input.prompt.len() > 64 * 1024 {
        return Err((
            StatusCode::BAD_REQUEST,
            "Enter a prompt of at most 64 KiB".into(),
        ));
    }
    if input
        .conversation_id
        .as_ref()
        .is_some_and(|s| !identifier(s))
        || input.session_id.as_ref().is_some_and(|s| !identifier(s))
        || input.model.as_ref().is_some_and(|s| !identifier(s))
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "Invalid model or session identifier".into(),
        ));
    }
    let prompt = history::prompt(&input.prompt, &input.selected_entities)?;
    let mut history = history::History::open(&project.root)?;
    let saved = history.resolve(
        input.conversation_id.as_deref(),
        &input.provider,
        input.session_id.as_deref(),
    )?;
    let conversation_id = saved
        .as_ref()
        .map(|(id, _)| id.clone())
        .unwrap_or_else(|| format!("{:032x}", rand::random::<u128>()));
    let resume = saved
        .as_ref()
        .and_then(|(_, resume)| resume.as_deref())
        .filter(|_| input.provider != "gemini");
    let bin = provider::PROVIDERS
        .iter()
        .find(|(id, _, _)| *id == input.provider)
        .and_then(|(_, _, bin)| provider::executable(bin))
        .ok_or((
            StatusCode::SERVICE_UNAVAILABLE,
            "Install and sign in to this assistant CLI before using it".into(),
        ))?;
    let id = format!("{:032x}", rand::random::<u128>());
    let (cancel_tx, cancel_rx) = watch::channel(false);
    {
        let mut active = st
            .assistant
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if active.is_some() {
            return Err((
                StatusCode::CONFLICT,
                "An assistant turn is already running".into(),
            ));
        }
        *active = Some((id.clone(), cancel_tx));
    }
    let guard = TurnGuard {
        state: st.clone(),
        id: id.clone(),
    };
    let tools = scene_tools().await;
    let proxy = format!(
        "{}/api/project/assistant/mcp",
        super::http::preview_origin(&headers)
    );
    let invocation = provider::invocation(
        &input.provider,
        &prompt,
        resume,
        input.model.as_deref(),
        &project.root,
        (tools["available"] == true).then_some(proxy.as_str()),
    )
    .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    let mut command = tokio::process::Command::new(bin);
    command
        .args(&invocation.args)
        .current_dir(&project.root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.as_std_mut().process_group(0);
    }
    history.begin(
        &conversation_id,
        &input.provider,
        &input.prompt,
        &input.selected_entities,
    )?;
    let mut child = command
        .spawn()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    let text = invocation.stdin.clone();
    let input_task = tokio::spawn(async move {
        let _ = stdin.write_all(text.as_bytes()).await;
        let _ = stdin.shutdown().await;
    });
    let (tx, rx) = mpsc::channel::<Value>(64);
    tokio::spawn(async move {
        let _guard = guard;
        let _overlays = invocation.overlays;
        let _ = tx.send(json!({"type":"started","turnId":id,"provider":input.provider,"conversationId":conversation_id})).await;
        let (events, received) = mpsc::channel::<Value>(64);
        let provider = input.provider.clone();
        let child_task = tokio::spawn(async move {
            run_child(child, &provider, cancel_rx, events).await;
        });
        history::record_stream(&mut history, &conversation_id, received, &tx).await;
        let _ = child_task.await;
        input_task.abort();
    });
    let stream = futures::stream::unfold(rx, |mut rx| async move {
        rx.recv()
            .await
            .map(|v| (Ok::<_, std::convert::Infallible>(format!("{v}\n")), rx))
    });
    Ok((
        [
            (header::CONTENT_TYPE, "application/x-ndjson"),
            (header::CACHE_CONTROL, "no-store"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        Body::from_stream(stream),
    )
        .into_response())
}

struct TurnGuard {
    state: Arc<AppState>,
    id: String,
}
impl Drop for TurnGuard {
    fn drop(&mut self) {
        let mut active = self
            .state
            .assistant
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if active.as_ref().is_some_and(|(id, _)| id == &self.id) {
            *active = None;
        }
    }
}

async fn stop_child(child: &mut tokio::process::Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }
    let _ = child.kill().await;
    let _ = child.wait().await;
}

pub(super) async fn emit(tx: &mpsc::Sender<Value>, event: Value) -> Result<(), ()> {
    tokio::time::timeout(Duration::from_secs(5), tx.send(event))
        .await
        .map_err(|_| ())?
        .map_err(|_| ())
}

async fn run_child(
    mut child: tokio::process::Child,
    provider: &str,
    mut cancel: watch::Receiver<bool>,
    tx: mpsc::Sender<Value>,
) {
    let mut stdout = BufReader::new(child.stdout.take().expect("piped stdout")).lines();
    let mut stderr = BufReader::new(child.stderr.take().expect("piped stderr")).lines();
    let mut out_open = true;
    let mut err_open = true;
    let mut size = 0usize;
    let mut cancelled = false;
    let deadline = tokio::time::sleep(Duration::from_secs(3600));
    tokio::pin!(deadline);
    loop {
        let event = tokio::select! {
            _=cancel.changed()=>{cancelled=true;None},
            _=tx.closed()=>{cancelled=true;None},
            _=&mut deadline=>{let _=emit(&tx, json!({"type":"error","message":"Assistant exceeded the one-hour turn limit"})).await;cancelled=true;None},
            line=stdout.next_line(),if out_open=>match line {
                Ok(Some(line))=>Some((false,line)),
                Ok(None)=>{out_open=false; if err_open {continue;}None},
                Err(e)=>{let _=emit(&tx, json!({"type":"error","message":e.to_string()})).await;cancelled=true;None},
            },
            line=stderr.next_line(),if err_open=>match line {
                Ok(Some(line))=>Some((true,line)),
                Ok(None)=>{err_open=false; if out_open {continue;}None},
                Err(e)=>{let _=emit(&tx, json!({"type":"error","message":e.to_string()})).await;cancelled=true;None},
            },
        };
        let Some((is_err, line)) = event else {
            break;
        };
        size += line.len();
        if line.len() > 1024 * 1024 || size > 20 * 1024 * 1024 {
            let _ = emit(
                &tx,
                json!({"type":"error","message":"Assistant output exceeded its limit"}),
            )
            .await;
            cancelled = true;
            break;
        }
        let events = if is_err && !line.trim().is_empty() {
            vec![json!({"type":"error","message":line})]
        } else {
            provider::events(provider, &line)
        };
        for event in events {
            let sent = tokio::select! {
                sent = emit(&tx, event) => sent.is_ok(),
                _ = cancel.changed() => false,
            };
            if !sent {
                cancelled = true;
                break;
            }
        }
        if cancelled {
            break;
        }
    }
    if cancelled {
        stop_child(&mut child).await;
    }
    let code = if cancelled {
        None
    } else {
        tokio::select! {
            result=child.wait()=>result.ok().and_then(|s|s.code()),
            _=cancel.changed()=>{cancelled=true;stop_child(&mut child).await;None},
            _=tx.closed()=>{cancelled=true;stop_child(&mut child).await;None},
            _=&mut deadline=>{cancelled=true;stop_child(&mut child).await;None},
        }
    };
    if !cancelled && code != Some(0) {
        let _=emit(&tx, json!({"type":"error","message":format!("Assistant exited with status {code:?}; check CLI authentication and permissions")})).await;
    }
    let done = json!({"type":"done","exitCode":code,"cancelled":cancelled});
    if cancelled {
        let _ = tx.try_send(done);
    } else {
        let _ = emit(&tx, done).await;
    }
}

async fn cancel(
    State(st): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<Value>), Failure> {
    let active = st
        .assistant
        .active
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (_, cancel) = active
        .as_ref()
        .filter(|(current, _)| current == &id)
        .ok_or((
            StatusCode::NOT_FOUND,
            "Assistant turn is no longer running".into(),
        ))?;
    let _ = cancel.send(true);
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({"turnId":id,"cancelled":true})),
    ))
}

#[cfg(test)]
#[path = "assistant_tests.rs"]
mod tests;
