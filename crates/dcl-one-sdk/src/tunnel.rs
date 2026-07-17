use crate::ux::{self, TrySteps, UserError};
use anyhow::Result;
use catalyrst_preview_tunnel::protocol::{
    decode_data, encode_data, is_hop_by_hop, ChannelKind, Control, Resume,
};
use futures::{SinkExt, StreamExt};
use rand::RngExt;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::Message;

const WELCOME_TIMEOUT: Duration = Duration::from_secs(10);
const BACKOFF_MIN: Duration = Duration::from_secs(1);
const BACKOFF_MAX: Duration = Duration::from_secs(30);
const TRUNK_BUFFER: usize = 1024;
const WS_EVENT_BUFFER: usize = 256;
const BODY_HARD_CAP: usize = 256 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct AgentConfig {
    pub trunk_url: String,
    pub token: Option<String>,
    pub local_port: u16,
}

#[derive(Debug)]
pub enum AgentEvent {
    Connected { public_url: String },
    ConnectFailed { error: String },
    Disconnected { error: String },
}

pub fn normalize_trunk_url(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(bad_tunnel_url(raw, "the URL is empty"));
    }
    let with_scheme = if let Some((scheme, rest)) = trimmed.split_once("://") {
        match scheme.to_ascii_lowercase().as_str() {
            "ws" | "wss" => trimmed.to_string(),
            "http" => format!("ws://{rest}"),
            "https" => format!("wss://{rest}"),
            other => {
                return Err(bad_tunnel_url(
                    raw,
                    &format!("unsupported scheme \"{other}\" (use wss:// or ws://)"),
                ));
            }
        }
    } else {
        format!("wss://{trimmed}")
    };
    let mut url = url::Url::parse(&with_scheme)
        .map_err(|e| bad_tunnel_url(raw, &format!("not a valid URL ({e})")))?;
    if url.host_str().is_none() {
        return Err(bad_tunnel_url(raw, "the URL has no host"));
    }
    if url.path() == "/" || url.path().is_empty() {
        url.set_path("/t/_connect");
    }
    Ok(url.to_string())
}

fn bad_tunnel_url(raw: &str, why: &str) -> anyhow::Error {
    UserError::new(
        format!("invalid --tunnel URL \"{raw}\""),
        TrySteps::one("pass the tunnel host, e.g. --tunnel wss://<tunnel-host>")
            .and("run: dcl-one-sdk start --tunnel help \u{2014} for setup + a zero-infra ssh -R fallback"),
    )
    .why(why.to_string())
    .into()
}

pub const TOKEN_ENV: &str = "DCL_ONE_SDK_TUNNEL_TOKEN";

pub fn resolve_token(
    flag: Option<String>,
    file: Option<&std::path::Path>,
) -> Result<Option<String>> {
    if flag.is_some() {
        return Ok(flag);
    }
    if let Some(path) = file {
        let raw = std::fs::read_to_string(path).map_err(|e| {
            anyhow::Error::from(
                UserError::new(
                    format!("could not read the token file {}", path.display()),
                    TrySteps::one("check the --tunnel-token-file path"),
                )
                .caused_by(e),
            )
        })?;
        return Ok(Some(raw.trim().to_string()));
    }
    if let Some(token) = std::env::var(TOKEN_ENV)
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
    {
        ux::note_stderr(format!("using the tunnel token from {TOKEN_ENV}"));
        return Ok(Some(token));
    }
    Ok(None)
}

pub fn spawn(cfg: AgentConfig) -> mpsc::UnboundedReceiver<AgentEvent> {
    let (events_tx, events_rx) = mpsc::unbounded_channel();
    tokio::spawn(run(cfg, events_tx));
    events_rx
}

async fn run(cfg: AgentConfig, events: mpsc::UnboundedSender<AgentEvent>) {
    let mut resume: Option<Resume> = None;
    let mut delay = BACKOFF_MIN;
    loop {
        match connect_and_serve(&cfg, &mut resume, &events).await {
            Ok(error) => {
                delay = BACKOFF_MIN;
                if events.send(AgentEvent::Disconnected { error }).is_err() {
                    return;
                }
            }
            Err(error) => {
                tracing::debug!("tunnel connect failed: {error:#}");
                if events
                    .send(AgentEvent::ConnectFailed {
                        error: crate::ux::concise_cause(&error),
                    })
                    .is_err()
                {
                    return;
                }
            }
        }
        tokio::time::sleep(jitter(delay)).await;
        delay = (delay * 2).min(BACKOFF_MAX);
    }
}

fn jitter(base: Duration) -> Duration {
    let factor = 0.8 + 0.4 * rand::rng().random::<f64>();
    base.mul_f64(factor)
}

fn lock<'a, T>(m: &'a Mutex<T>) -> std::sync::MutexGuard<'a, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn local_forward_url(
    scheme: &str,
    local_port: u16,
    path: &str,
    query: Option<&str>,
) -> Option<String> {
    if !path.starts_with('/') {
        return None;
    }
    let query = query.map(|q| format!("?{q}")).unwrap_or_default();
    Some(format!("{scheme}://127.0.0.1:{local_port}{path}{query}"))
}

enum ChanState {
    HttpPending {
        open: HttpOpen,
        body: Vec<u8>,
    },
    Running {
        events: Option<mpsc::Sender<ChanEvent>>,
        task: tokio::task::JoinHandle<()>,
    },
}

struct HttpOpen {
    method: String,
    path: String,
    query: Option<String>,
    headers: Vec<(String, String)>,
}

enum ChanEvent {
    Data {
        binary: bool,
        payload: Vec<u8>,
    },
    Close {
        code: Option<u16>,
        reason: Option<String>,
    },
}

struct Forwarded {
    proto: String,
    host: String,
    prefix: String,
}

async fn connect_and_serve(
    cfg: &AgentConfig,
    resume: &mut Option<Resume>,
    events: &mpsc::UnboundedSender<AgentEvent>,
) -> Result<String> {
    let (socket, _resp) = tokio_tungstenite::connect_async(&cfg.trunk_url).await?;
    let (mut sink, mut stream) = socket.split();
    let hello = Control::Hello {
        token: cfg.token.clone(),
        resume: resume.clone(),
        agent: format!("dcl-one-sdk/{}", env!("CARGO_PKG_VERSION")),
    };
    sink.send(Message::Text(hello.encode().into())).await?;
    let welcome = tokio::time::timeout(WELCOME_TIMEOUT, async {
        loop {
            match stream.next().await {
                Some(Ok(Message::Text(text))) => {
                    if let Some(control) = Control::decode(&text) {
                        return Ok(control);
                    }
                }
                Some(Ok(Message::Close(frame))) => {
                    anyhow::bail!(
                        "tunnel refused the connection{}",
                        frame
                            .map(|f| format!(" ({} {})", u16::from(f.code), f.reason))
                            .unwrap_or_default()
                    );
                }
                Some(Ok(_)) => continue,
                Some(Err(e)) => return Err(e.into()),
                None => anyhow::bail!("tunnel closed the connection during the handshake"),
            }
        }
    })
    .await
    .map_err(|_| anyhow::anyhow!("timed out waiting for the tunnel welcome"))??;
    let (id, public_url, resume_key, ping_s) = match welcome {
        Control::Welcome {
            id,
            public_url,
            resume_key,
            ping_s,
        } => (id, public_url, resume_key, ping_s),
        other => anyhow::bail!("expected a welcome from the tunnel, got {other:?}"),
    };
    *resume = Some(Resume {
        id: id.clone(),
        key: resume_key,
    });
    let _ = events.send(AgentEvent::Connected {
        public_url: public_url.clone(),
    });

    let fwd = Arc::new(forwarded_from_public_url(&public_url));
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let channels: Arc<Mutex<HashMap<u32, ChanState>>> = Arc::new(Mutex::new(HashMap::new()));
    let (trunk_tx, mut trunk_rx) = mpsc::channel::<Message>(TRUNK_BUFFER);
    let ping = Duration::from_secs(ping_s.max(1));
    let mut last_in = tokio::time::Instant::now();
    let mut ticker = tokio::time::interval_at(tokio::time::Instant::now() + ping, ping);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let disconnect_reason;
    loop {
        tokio::select! {
            out = trunk_rx.recv() => match out {
                Some(msg) => {
                    if let Err(e) = sink.send(msg).await {
                        disconnect_reason = format!("send failed: {e}");
                        break;
                    }
                }
                None => {
                    disconnect_reason = "agent shutting down".to_string();
                    break;
                }
            },
            incoming = stream.next() => match incoming {
                Some(Ok(msg)) => {
                    last_in = tokio::time::Instant::now();
                    match msg {
                        Message::Text(text) => {
                            if let Some(control) = Control::decode(&text) {
                                handle_control(control, cfg, &client, &fwd, &channels, &trunk_tx);
                            }
                        }
                        Message::Binary(bytes) => handle_data(&bytes, &channels, &trunk_tx).await,
                        Message::Close(_) => {
                            disconnect_reason = "tunnel closed the trunk".to_string();
                            break;
                        }
                        _ => {}
                    }
                }
                Some(Err(e)) => {
                    disconnect_reason = format!("trunk error: {e}");
                    break;
                }
                None => {
                    disconnect_reason = "tunnel closed the trunk".to_string();
                    break;
                }
            },
            _ = ticker.tick() => {
                if last_in.elapsed() > ping * 3 {
                    disconnect_reason = "tunnel unresponsive (missed pings)".to_string();
                    break;
                }
                if sink.send(Message::Text(Control::Ping.encode().into())).await.is_err() {
                    disconnect_reason = "send failed".to_string();
                    break;
                }
            }
        }
    }
    for (_, state) in lock(&channels).drain() {
        if let ChanState::Running { task, .. } = state {
            task.abort();
        }
    }
    Ok(disconnect_reason)
}

fn forwarded_from_public_url(public_url: &str) -> Forwarded {
    match url::Url::parse(public_url) {
        Ok(u) => {
            let proto = if u.scheme() == "https" {
                "https"
            } else {
                "http"
            };
            let host = match (u.host_str(), u.port()) {
                (Some(h), Some(p)) => format!("{h}:{p}"),
                (Some(h), None) => h.to_string(),
                _ => String::new(),
            };
            Forwarded {
                proto: proto.to_string(),
                host,
                prefix: u.path().trim_end_matches('/').to_string(),
            }
        }
        Err(_) => Forwarded {
            proto: "http".into(),
            host: String::new(),
            prefix: String::new(),
        },
    }
}

fn handle_control(
    control: Control,
    cfg: &AgentConfig,
    client: &reqwest::Client,
    fwd: &Arc<Forwarded>,
    channels: &Arc<Mutex<HashMap<u32, ChanState>>>,
    trunk_tx: &mpsc::Sender<Message>,
) {
    match control {
        Control::Open {
            ch,
            kind: ChannelKind::Http,
            method,
            path,
            query,
            headers,
            ..
        } => {
            lock(channels).insert(
                ch,
                ChanState::HttpPending {
                    open: HttpOpen {
                        method: method.unwrap_or_else(|| "GET".into()),
                        path,
                        query,
                        headers,
                    },
                    body: Vec::new(),
                },
            );
        }
        Control::Open {
            ch,
            kind: ChannelKind::Ws,
            path,
            query,
            subprotocols,
            ..
        } => {
            let (events_tx, events_rx) = mpsc::channel(WS_EVENT_BUFFER);
            let task = tokio::spawn(run_ws_channel(
                ch,
                cfg.local_port,
                path,
                query,
                subprotocols.unwrap_or_default(),
                Arc::clone(fwd),
                trunk_tx.clone(),
                Arc::clone(channels),
                events_rx,
            ));
            lock(channels).insert(
                ch,
                ChanState::Running {
                    events: Some(events_tx),
                    task,
                },
            );
        }
        Control::End { ch } => {
            let pending = {
                let mut map = lock(channels);
                match map.remove(&ch) {
                    Some(ChanState::HttpPending { open, body }) => Some((open, body)),
                    Some(running) => {
                        map.insert(ch, running);
                        None
                    }
                    None => None,
                }
            };
            if let Some((open, body)) = pending {
                let task = tokio::spawn(run_http_channel(
                    ch,
                    cfg.local_port,
                    open,
                    body,
                    client.clone(),
                    Arc::clone(fwd),
                    trunk_tx.clone(),
                    Arc::clone(channels),
                ));
                lock(channels).insert(ch, ChanState::Running { events: None, task });
            }
        }
        Control::Close { ch, code, reason } => {
            let state = lock(channels).remove(&ch);
            match state {
                Some(ChanState::Running { events, task }) => match events {
                    Some(tx) => {
                        if tx.try_send(ChanEvent::Close { code, reason }).is_err() {
                            task.abort();
                        }
                    }
                    None => task.abort(),
                },
                Some(ChanState::HttpPending { .. }) | None => {}
            }
        }
        Control::Ping => {
            let _ = trunk_tx.try_send(Message::Text(Control::Pong.encode().into()));
        }
        _ => {}
    }
}

async fn handle_data(
    bytes: &[u8],
    channels: &Arc<Mutex<HashMap<u32, ChanState>>>,
    trunk_tx: &mpsc::Sender<Message>,
) {
    let Some(frame) = decode_data(bytes) else {
        return;
    };
    let ch = frame.ch;
    enum Routed {
        Buffered,
        Overflow,
        Event(mpsc::Sender<ChanEvent>),
        Gone,
    }
    let routed = {
        let mut map = lock(channels);
        match map.get_mut(&ch) {
            Some(ChanState::HttpPending { body, .. }) => {
                if body.len() + frame.payload.len() > BODY_HARD_CAP {
                    map.remove(&ch);
                    Routed::Overflow
                } else {
                    body.extend_from_slice(&frame.payload);
                    Routed::Buffered
                }
            }
            Some(ChanState::Running {
                events: Some(tx), ..
            }) => Routed::Event(tx.clone()),
            Some(ChanState::Running { events: None, .. }) => Routed::Gone,
            None => Routed::Gone,
        }
    };
    match routed {
        Routed::Buffered | Routed::Gone => {}
        Routed::Overflow => {
            let _ = trunk_tx.try_send(Message::Text(
                Control::OpenErr {
                    ch,
                    error: "request body too large".into(),
                }
                .encode()
                .into(),
            ));
        }
        Routed::Event(tx) => {
            let _ = tx
                .send(ChanEvent::Data {
                    binary: frame.binary,
                    payload: frame.payload,
                })
                .await;
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_http_channel(
    ch: u32,
    local_port: u16,
    open: HttpOpen,
    body: Vec<u8>,
    client: reqwest::Client,
    fwd: Arc<Forwarded>,
    trunk_tx: mpsc::Sender<Message>,
    channels: Arc<Mutex<HashMap<u32, ChanState>>>,
) {
    let Some(url) = local_forward_url("http", local_port, &open.path, open.query.as_deref()) else {
        send_control(
            &trunk_tx,
            Control::OpenErr {
                ch,
                error: "invalid forward path".into(),
            },
        )
        .await;
        lock(&channels).remove(&ch);
        return;
    };
    let method = match reqwest::Method::from_bytes(open.method.as_bytes()) {
        Ok(m) => m,
        Err(_) => {
            send_control(
                &trunk_tx,
                Control::OpenErr {
                    ch,
                    error: format!("unsupported method {}", open.method),
                },
            )
            .await;
            lock(&channels).remove(&ch);
            return;
        }
    };
    let mut request = client.request(method, url);
    for (name, value) in &open.headers {
        if is_hop_by_hop(name)
            || name.eq_ignore_ascii_case("host")
            || name.eq_ignore_ascii_case("content-length")
            || name.to_ascii_lowercase().starts_with("x-forwarded-")
        {
            continue;
        }
        request = request.header(name, value);
    }
    request = request
        .header("x-forwarded-proto", &fwd.proto)
        .header("x-forwarded-prefix", &fwd.prefix);
    if !fwd.host.is_empty() {
        request = request.header("x-forwarded-host", &fwd.host);
    }
    if !body.is_empty() {
        request = request.body(body);
    }
    match request.send().await {
        Ok(mut response) => {
            let status = response.status().as_u16();
            let headers: Vec<(String, String)> = response
                .headers()
                .iter()
                .filter(|(name, _)| !is_hop_by_hop(name.as_str()))
                .filter_map(|(name, value)| {
                    value
                        .to_str()
                        .ok()
                        .map(|v| (name.as_str().to_string(), v.to_string()))
                })
                .collect();
            send_control(
                &trunk_tx,
                Control::OpenOk {
                    ch,
                    status,
                    headers: Some(headers),
                    subprotocol: None,
                },
            )
            .await;
            loop {
                match response.chunk().await {
                    Ok(Some(chunk)) => {
                        if !chunk.is_empty()
                            && trunk_tx
                                .send(Message::Binary(encode_data(ch, true, &chunk).into()))
                                .await
                                .is_err()
                        {
                            break;
                        }
                    }
                    Ok(None) => {
                        send_control(&trunk_tx, Control::End { ch }).await;
                        break;
                    }
                    Err(e) => {
                        send_control(
                            &trunk_tx,
                            Control::Close {
                                ch,
                                code: Some(1011),
                                reason: Some(format!("local read failed: {e}")),
                            },
                        )
                        .await;
                        break;
                    }
                }
            }
        }
        Err(e) => {
            send_control(
                &trunk_tx,
                Control::OpenErr {
                    ch,
                    error: format!("local preview unreachable: {e}"),
                },
            )
            .await;
        }
    }
    lock(&channels).remove(&ch);
}

#[allow(clippy::too_many_arguments)]
async fn run_ws_channel(
    ch: u32,
    local_port: u16,
    path: String,
    query: Option<String>,
    subprotocols: Vec<String>,
    fwd: Arc<Forwarded>,
    trunk_tx: mpsc::Sender<Message>,
    channels: Arc<Mutex<HashMap<u32, ChanState>>>,
    mut events_rx: mpsc::Receiver<ChanEvent>,
) {
    let Some(url) = local_forward_url("ws", local_port, &path, query.as_deref()) else {
        send_control(
            &trunk_tx,
            Control::OpenErr {
                ch,
                error: "invalid forward path".into(),
            },
        )
        .await;
        lock(&channels).remove(&ch);
        return;
    };
    let mut request = match url.as_str().into_client_request() {
        Ok(r) => r,
        Err(e) => {
            send_control(
                &trunk_tx,
                Control::OpenErr {
                    ch,
                    error: format!("bad ws path: {e}"),
                },
            )
            .await;
            lock(&channels).remove(&ch);
            return;
        }
    };
    if !subprotocols.is_empty() {
        if let Ok(value) = HeaderValue::from_str(&subprotocols.join(", ")) {
            request
                .headers_mut()
                .insert("Sec-WebSocket-Protocol", value);
        }
    }
    for (name, value) in [
        ("x-forwarded-proto", fwd.proto.as_str()),
        ("x-forwarded-prefix", fwd.prefix.as_str()),
        ("x-forwarded-host", fwd.host.as_str()),
    ] {
        if !value.is_empty() {
            if let (Ok(n), Ok(v)) = (
                tokio_tungstenite::tungstenite::http::HeaderName::from_bytes(name.as_bytes()),
                HeaderValue::from_str(value),
            ) {
                request.headers_mut().insert(n, v);
            }
        }
    }
    let (socket, response) = match tokio_tungstenite::connect_async(request).await {
        Ok(ok) => ok,
        Err(e) => {
            send_control(
                &trunk_tx,
                Control::OpenErr {
                    ch,
                    error: format!("local preview unreachable: {e}"),
                },
            )
            .await;
            lock(&channels).remove(&ch);
            return;
        }
    };
    let subprotocol = response
        .headers()
        .get("sec-websocket-protocol")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    send_control(
        &trunk_tx,
        Control::OpenOk {
            ch,
            status: 101,
            headers: None,
            subprotocol,
        },
    )
    .await;
    let (mut local_sink, mut local_stream) = socket.split();
    let mut notify_service = true;
    loop {
        tokio::select! {
            event = events_rx.recv() => match event {
                Some(ChanEvent::Data { binary, payload }) => {
                    let msg = if binary {
                        Message::Binary(payload.into())
                    } else {
                        match String::from_utf8(payload) {
                            Ok(text) => Message::Text(text.into()),
                            Err(_) => break,
                        }
                    };
                    if local_sink.send(msg).await.is_err() {
                        break;
                    }
                }
                Some(ChanEvent::Close { code, reason }) => {
                    let _ = local_sink
                        .send(Message::Close(Some(CloseFrame {
                            code: CloseCode::from(code.unwrap_or(1000)),
                            reason: reason.unwrap_or_default().into(),
                        })))
                        .await;
                    notify_service = false;
                    break;
                }
                None => break,
            },
            incoming = local_stream.next() => match incoming {
                Some(Ok(Message::Text(text))) => {
                    if trunk_tx
                        .send(Message::Binary(encode_data(ch, false, text.as_bytes()).into()))
                        .await
                        .is_err()
                    {
                        notify_service = false;
                        break;
                    }
                }
                Some(Ok(Message::Binary(bytes))) => {
                    if trunk_tx
                        .send(Message::Binary(encode_data(ch, true, &bytes).into()))
                        .await
                        .is_err()
                    {
                        notify_service = false;
                        break;
                    }
                }
                Some(Ok(Message::Close(frame))) => {
                    send_control(
                        &trunk_tx,
                        Control::Close {
                            ch,
                            code: frame.as_ref().map(|f| u16::from(f.code)),
                            reason: frame
                                .filter(|f| !f.reason.is_empty())
                                .map(|f| f.reason.to_string()),
                        },
                    )
                    .await;
                    notify_service = false;
                    break;
                }
                Some(Ok(_)) => continue,
                Some(Err(_)) | None => break,
            },
        }
    }
    if notify_service {
        send_control(
            &trunk_tx,
            Control::Close {
                ch,
                code: Some(1001),
                reason: None,
            },
        )
        .await;
    }
    lock(&channels).remove(&ch);
}

async fn send_control(trunk_tx: &mpsc::Sender<Message>, control: Control) {
    let _ = trunk_tx.send(Message::Text(control.encode().into())).await;
}

pub fn tunnel_help() -> String {
    [
        "Internet reach for a local preview",
        "",
        "Option 1 \u{2014} preview-tunnel service (recommended):",
        "  dcl-one-sdk start --tunnel wss://<tunnel-host>",
        "  The service is the catalyrst-preview-tunnel crate; any box with an",
        "  https vhost can host it (one nginx location for /t/, no wildcard DNS).",
        "  It prints a public https realm URL (https://<tunnel-host>/t/<id>) that",
        "  works in every client, including browsers. If the operator configured",
        "  TUNNEL_TOKENS, pass the token via DCL_ONE_SDK_TUNNEL_TOKEN=<token> or",
        "  --tunnel-token-file <path> (preferred); --tunnel-token <token> also",
        "  works but exposes the secret to ps and shell history.",
        "",
        "Option 2 \u{2014} zero-infra ssh -R fallback (any box with sshd + a public address):",
        "  native/desktop clients (an http realm is fine for them):",
        "    ssh -R 0.0.0.0:8100:127.0.0.1:<port> <user>@<vps-host>",
        "    then join with realm=http://<vps-host>:8100",
        "    (needs GatewayPorts clientspecified in the VPS sshd_config; verify with",
        "     curl http://<vps-host>:8100/about before sharing)",
        "  web explorer too (needs https):",
        "    ssh -R 127.0.0.1:8100:127.0.0.1:<port> <user>@<vps-host>",
        "    plus an https vhost on the VPS proxying to 127.0.0.1:8100 that sets",
        "    X-Forwarded-Proto and X-Forwarded-Prefix \"\"",
        "  second PC on this LAN whose browser blocks the http realm:",
        "    ssh -L <port>:127.0.0.1:<port> <user>@<this-machine>",
        "    then join with realm=http://127.0.0.1:<port>",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_accepts_bare_hosts_and_appends_the_trunk_path() {
        assert_eq!(
            normalize_trunk_url("tunnel.example").unwrap(),
            "wss://tunnel.example/t/_connect"
        );
        assert_eq!(
            normalize_trunk_url("wss://tunnel.example").unwrap(),
            "wss://tunnel.example/t/_connect"
        );
        assert_eq!(
            normalize_trunk_url("ws://127.0.0.1:5167").unwrap(),
            "ws://127.0.0.1:5167/t/_connect"
        );
        assert_eq!(
            normalize_trunk_url("https://tunnel.example").unwrap(),
            "wss://tunnel.example/t/_connect"
        );
        assert_eq!(
            normalize_trunk_url("wss://tunnel.example/custom/path").unwrap(),
            "wss://tunnel.example/custom/path"
        );
    }

    #[test]
    fn normalize_rejects_garbage_with_a_user_error() {
        for bad in ["", "   ", "ftp://x", "wss://"] {
            let err = normalize_trunk_url(bad).unwrap_err();
            assert!(
                err.chain().any(|c| c.downcast_ref::<UserError>().is_some()),
                "expected a UserError for {bad:?}"
            );
        }
    }

    #[test]
    fn forwarded_derives_proto_host_prefix_from_the_public_url() {
        let fwd = forwarded_from_public_url("https://tunnel.example/t/abc123defg");
        assert_eq!(fwd.proto, "https");
        assert_eq!(fwd.host, "tunnel.example");
        assert_eq!(fwd.prefix, "/t/abc123defg");
        let fwd = forwarded_from_public_url("http://127.0.0.1:5637/t/xyz/");
        assert_eq!(fwd.proto, "http");
        assert_eq!(fwd.host, "127.0.0.1:5637");
        assert_eq!(fwd.prefix, "/t/xyz");
    }

    #[test]
    fn jitter_stays_within_twenty_percent() {
        for _ in 0..100 {
            let d = jitter(Duration::from_secs(10));
            assert!(d >= Duration::from_secs(8) && d <= Duration::from_secs(12));
        }
    }

    #[test]
    fn local_forward_url_pins_the_loopback_host_and_rejects_authority_escapes() {
        assert_eq!(
            local_forward_url("http", 8000, "/about", None).as_deref(),
            Some("http://127.0.0.1:8000/about")
        );
        assert_eq!(
            local_forward_url("http", 8000, "/content", Some("a=b")).as_deref(),
            Some("http://127.0.0.1:8000/content?a=b")
        );
        assert_eq!(
            local_forward_url("ws", 5142, "/mini-comms/room-1", None).as_deref(),
            Some("ws://127.0.0.1:5142/mini-comms/room-1")
        );
        for evil in ["@evil.example/x", "evil.example", "\\evil", " /x"] {
            assert!(
                local_forward_url("http", 8000, evil, None).is_none(),
                "a path that does not start with '/' must be rejected: {evil:?}"
            );
        }
    }

    #[test]
    fn tunnel_help_names_both_options() {
        let help = tunnel_help();
        assert!(help.contains("--tunnel wss://<tunnel-host>"));
        assert!(help.contains("ssh -R 0.0.0.0:8100:127.0.0.1:<port>"));
        assert!(help.contains("GatewayPorts clientspecified"));
        assert!(help.contains("ssh -L <port>:127.0.0.1:<port>"));
    }

    #[test]
    fn token_precedence_is_flag_then_file_then_env() {
        let dir = std::env::temp_dir().join(format!(
            "dcl-one-sdk-tunnel-token-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("token.txt");
        std::fs::write(&file, "from-file\n").unwrap();
        std::env::set_var(TOKEN_ENV, "from-env");
        assert_eq!(
            resolve_token(Some("from-flag".into()), Some(&file)).unwrap(),
            Some("from-flag".into())
        );
        assert_eq!(
            resolve_token(None, Some(&file)).unwrap(),
            Some("from-file".into())
        );
        assert_eq!(resolve_token(None, None).unwrap(), Some("from-env".into()));
        std::env::set_var(TOKEN_ENV, "");
        assert_eq!(resolve_token(None, None).unwrap(), None);
        std::env::remove_var(TOKEN_ENV);
        assert_eq!(resolve_token(None, None).unwrap(), None);
        assert!(resolve_token(None, Some(&dir.join("missing.txt"))).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
