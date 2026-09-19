use super::AppState;
use axum::{
    body::Body,
    extract::{
        ws::{Message, WebSocket},
        Query, State, WebSocketUpgrade,
    },
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::VecDeque, sync::Arc, time::Duration};
use tokio::sync::{broadcast, mpsc, oneshot};
type Failure = (StatusCode, String);

pub(super) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/project/debug", get(descriptor))
        .route("/api/project/debug/sessions", get(sessions))
        .route("/api/project/debug/events", get(events))
        .route("/api/project/debug/command", post(command))
}

fn add_param(url: &str, key: &str, value: &str) -> String {
    let (prefix, query) = if let Some((prefix, query)) = url.split_once('?') {
        (format!("{prefix}?"), query)
    } else {
        (
            String::from("decentraland://"),
            url.strip_prefix("decentraland://").expect("SDK deep link"),
        )
    };
    let params: Vec<_> = url::form_urlencoded::parse(query.as_bytes())
        .filter(|(name, _)| !name.eq_ignore_ascii_case(key))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    let query = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(params)
        .append_pair(key, value)
        .finish();
    format!("{prefix}{query}")
}

async fn descriptor(
    State(st): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, Failure> {
    let project = super::project_bridge::single_project(&st)?;
    let realm = super::http::preview_origin(&headers);
    let ws = super::http::preview_ws_origin(&headers);
    let position = crate::joinblock::base_coords(&project.scene_json);
    let extra = crate::joinblock::deep_link_extra(
        st.local_ab,
        st.mcp,
        st.mcp.then_some(st.mcp_port),
        &st.explorer_params,
    );
    let producer = format!("{ws}/scene-inspector?token={}", st.external_debug.token);
    let native = add_param(
        &crate::joinblock::desktop_deep_link(&realm, position, None, &extra),
        "scene-inspector",
        &producer,
    );
    let multi = add_param(&native, "multi-instance", "true");
    let mobile = crate::netinfo::share_ip(&crate::netinfo::enumerate()).map(|ip| {
        let mut lan = url::Url::parse(&realm).expect("preview URL");
        lan.set_host(Some(&ip.to_string())).expect("LAN IP");
        let lan = lan.as_str().trim_end_matches('/');
        let mobile = crate::joinblock::mobile_deep_link(lan, position);
        let mut producer = url::Url::parse(&producer).unwrap();
        producer.set_host(Some(&ip.to_string())).unwrap();
        add_param(&mobile, "scene-inspector", producer.as_str())
    });
    let prefix = super::forwarded_prefix(&headers);
    Ok(Json(
        json!({"nativeUrl":native,"multiInstanceUrl":multi,"mobileQr":mobile.as_deref().and_then(crate::joinblock::qr_svg_data_url),"mobileUrl":mobile,
        "eventsUrl":format!("{prefix}/api/project/debug/events"),"sessionsUrl":format!("{prefix}/api/project/debug/sessions"),"commandUrl":format!("{prefix}/api/project/debug/command"),
        "capabilities":{"telemetry":true,"commands":["pause","resume","reload_scene"],"nativeMcp":st.mcp}}),
    ))
}

async fn sessions(State(st): State<Arc<AppState>>) -> Json<Value> {
    Json(json!({"sessions":st.external_debug.sessions()}))
}

async fn events(State(st): State<Arc<AppState>>) -> Response {
    let rx = st.external_debug.events.subscribe();
    let mut queued = VecDeque::new();
    queued.extend(
        st.external_debug
            .store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .history
            .iter()
            .map(|(_, v)| v.clone()),
    );
    queued.push_back(Arc::new(
        json!({"type":"sessions","sessions":st.external_debug.sessions()}),
    ));
    let stream = futures::stream::unfold(
        (queued, rx, 0u64),
        |(mut queued, mut rx, mut last)| async move {
            loop {
                let event = match queued.pop_front() {
                    Some(v) => v,
                    None => match rx.recv().await {
                        Ok(v) => v,
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            Arc::new(json!({"type":"gap","missed":n}))
                        }
                        Err(_) => return None,
                    },
                };
                if let Some(seq) = event["seq"].as_u64() {
                    if seq <= last {
                        continue;
                    }
                    last = seq;
                }
                return Some((
                    Ok::<_, std::convert::Infallible>(format!("{event}\n")),
                    (queued, rx, last),
                ));
            }
        },
    );
    (
        [
            (header::CONTENT_TYPE, "application/x-ndjson"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        Body::from_stream(stream),
    )
        .into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Command {
    session_id: u64,
    cmd: String,
    #[serde(default = "empty_args")]
    args: Value,
}
fn empty_args() -> Value {
    json!({})
}
async fn command(
    State(st): State<Arc<AppState>>,
    Json(input): Json<Command>,
) -> Result<Json<Value>, Failure> {
    if !matches!(input.cmd.as_str(), "pause" | "resume" | "reload_scene")
        || !input.args.is_object()
        || input.args.to_string().len() > 64 * 1024
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "Choose pause, resume, or reload_scene with an object of arguments".into(),
        ));
    }
    let id = format!("cmd-{:032x}", rand::random::<u128>());
    let (reply_tx, reply_rx) = oneshot::channel();
    let send = {
        let mut store = st
            .external_debug
            .store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let session = store
            .sessions
            .get_mut(&input.session_id)
            .filter(|s| s.send.is_some())
            .ok_or((StatusCode::NOT_FOUND, "Session is not connected".into()))?;
        if session.pending.len() >= 16 {
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                "Wait for pending debug commands".into(),
            ));
        }
        session.pending.insert(id.clone(), reply_tx);
        session.send.as_ref().unwrap().clone()
    };
    let _pending = PendingGuard {
        state: st.clone(),
        session_id: input.session_id,
        id: id.clone(),
    };
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        send.send(json!({"type":"SCENE_INSPECTOR_CMD","id":id,"cmd":input.cmd,"args":input.args}))
            .await
            .map_err(|_| "session disconnected")?;
        reply_rx.await.map_err(|_| "session disconnected")
    })
    .await;
    Ok(Json(match result {
        Ok(Ok(reply)) => reply,
        Ok(Err(error)) => json!({"ok":false,"data":{"error":error}}),
        Err(_) => json!({"ok":false,"data":{"error":"timeout"}}),
    }))
}

struct PendingGuard {
    state: Arc<AppState>,
    session_id: u64,
    id: String,
}
impl Drop for PendingGuard {
    fn drop(&mut self) {
        if let Some(session) = self
            .state
            .external_debug
            .store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .sessions
            .get_mut(&self.session_id)
        {
            session.pending.remove(&self.id);
        }
    }
}

#[derive(Deserialize)]
pub(super) struct Pair {
    token: String,
}
pub(super) async fn producer(
    State(st): State<Arc<AppState>>,
    Query(pair): Query<Pair>,
    ws: WebSocketUpgrade,
) -> Result<Response, Failure> {
    if !token_matches(&pair.token, &st.external_debug.token) {
        return Err((StatusCode::FORBIDDEN, "Invalid debug pairing token".into()));
    }
    Ok(ws
        .max_message_size(16 * 1024 * 1024)
        .on_upgrade(move |socket| receive(socket, st)))
}
fn token_matches(given: &str, expected: &str) -> bool {
    given.len() == expected.len()
        && given
            .bytes()
            .zip(expected.bytes())
            .fold(0u8, |diff, (a, b)| diff | (a ^ b))
            == 0
}

struct SessionGuard {
    state: Arc<AppState>,
    id: u64,
}
impl Drop for SessionGuard {
    fn drop(&mut self) {
        self.state.external_debug.disconnect(self.id);
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let state = self.state.clone();
            runtime.spawn(async move {
                tokio::time::sleep(Duration::from_secs(61)).await;
                state.external_debug.changed();
            });
        }
    }
}
async fn receive(socket: WebSocket, st: Arc<AppState>) {
    let (send, mut commands) = mpsc::channel(16);
    let Some(id) = st.external_debug.connect(send) else {
        return;
    };
    let _guard = SessionGuard {
        state: st.clone(),
        id,
    };
    let (mut tx, mut rx) = socket.split();
    loop {
        tokio::select! {
            command=commands.recv()=>{let Some(command)=command else{break;};if tx.send(Message::Text(command.to_string().into())).await.is_err(){break;}},
            message=rx.next()=>match message {
                Some(Ok(Message::Text(text)))=>{if let Ok(value)=serde_json::from_str(&text){st.external_debug.receive(id,value);}},
                Some(Ok(Message::Binary(bytes)))=>{if let Ok(value)=serde_json::from_slice(&bytes){st.external_debug.receive(id,value);}},
                Some(Ok(Message::Close(_)))|Some(Err(_))|None=>break,
                _=>{},
            }
        }
    }
}

#[cfg(test)]
#[path = "external_debug_tests.rs"]
mod tests;
