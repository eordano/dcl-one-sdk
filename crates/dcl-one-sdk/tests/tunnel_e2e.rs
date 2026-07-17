mod common;

use axum::extract::ws::Message as AxMessage;
use axum::extract::{State, WebSocketUpgrade};
use axum::http::HeaderMap;
use axum::response::Response;
use axum::routing::get;
use axum::{Json, Router};
use catalyrst_preview_tunnel::{router, AppState, Config};
use common::{connect_ws, handshake, recv_message, send_packet, wallet_address};
use dcl_one_sdk::comms::proto::{ws_packet, WsPeerUpdate};
use dcl_one_sdk::comms::{routes as comms_routes, CommsState};
use dcl_one_sdk::live_reload::{scene_update_json, update_scene_frame};
use dcl_one_sdk::tunnel::{spawn, AgentConfig, AgentEvent};
use futures::StreamExt;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::Message;

const BLOB: &[u8] = &[0xA5; 300_000];

#[derive(Clone)]
struct PreviewState {
    reload_tx: broadcast::Sender<ReloadFrame>,
}

#[derive(Clone, Debug)]
enum ReloadFrame {
    Text(String),
    Binary(Vec<u8>),
}

async fn about_echo(headers: HeaderMap) -> Json<Value> {
    let h = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };
    Json(json!({
        "proto": h("x-forwarded-proto"),
        "host": h("x-forwarded-host"),
        "prefix": h("x-forwarded-prefix"),
    }))
}

async fn blob() -> Vec<u8> {
    BLOB.to_vec()
}

async fn scene_update_ws(State(st): State<PreviewState>, ws: WebSocketUpgrade) -> Response {
    let mut rx = st.reload_tx.subscribe();
    ws.on_upgrade(move |mut socket| async move {
        while let Ok(frame) = rx.recv().await {
            let msg = match frame {
                ReloadFrame::Text(text) => AxMessage::Text(text.into()),
                ReloadFrame::Binary(bytes) => AxMessage::Binary(bytes.into()),
            };
            if socket.send(msg).await.is_err() {
                break;
            }
        }
    })
}

async fn spawn_preview() -> (SocketAddr, broadcast::Sender<ReloadFrame>) {
    let (reload_tx, _) = broadcast::channel(16);
    let state = PreviewState {
        reload_tx: reload_tx.clone(),
    };
    let app = Router::new()
        .route("/", get(scene_update_ws))
        .route("/about", get(about_echo))
        .route("/content/contents/blob", get(blob))
        .with_state(state)
        .merge(comms_routes(Arc::new(CommsState::default())));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (addr, reload_tx)
}

async fn spawn_tunnel_service() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let cfg = Config {
        public_base_url: Some(format!("http://{addr}")),
        ..Config::default()
    };
    let app = router(Arc::new(AppState::new(cfg)));
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    addr
}

async fn wait_connected(events: &mut tokio::sync::mpsc::UnboundedReceiver<AgentEvent>) -> String {
    loop {
        let event = tokio::time::timeout(Duration::from_secs(5), events.recv())
            .await
            .expect("timed out waiting for the tunnel agent to connect")
            .expect("agent event stream ended");
        match event {
            AgentEvent::Connected { public_url } => return public_url,
            AgentEvent::ConnectFailed { error } => panic!("agent failed to connect: {error}"),
            AgentEvent::Disconnected { .. } => continue,
        }
    }
}

#[tokio::test]
async fn full_flow_through_the_tunnel_origin() {
    let (preview_addr, reload_tx) = spawn_preview().await;
    let tunnel_addr = spawn_tunnel_service().await;

    let mut events = spawn(AgentConfig {
        trunk_url: format!("ws://{tunnel_addr}/t/_connect"),
        token: None,
        local_port: preview_addr.port(),
    });
    let public_url = wait_connected(&mut events).await;
    assert!(public_url.starts_with(&format!("http://{tunnel_addr}/t/")));

    let about: Value = reqwest::get(format!("{public_url}/about"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = public_url.rsplit('/').next().unwrap();
    assert_eq!(about["proto"], json!("http"));
    assert_eq!(about["host"], json!(tunnel_addr.to_string()));
    assert_eq!(about["prefix"], json!(format!("/t/{id}")));

    let content = reqwest::get(format!("{public_url}/content/contents/blob"))
        .await
        .unwrap();
    assert_eq!(content.status(), 200);
    let bytes = content.bytes().await.unwrap();
    assert_eq!(bytes.len(), BLOB.len());
    assert_eq!(&bytes[..], BLOB);

    let ws_url = public_url.replacen("http://", "ws://", 1);
    let (mut reload_ws, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    reload_tx
        .send(ReloadFrame::Text(scene_update_json("scene-1")))
        .unwrap();
    reload_tx
        .send(ReloadFrame::Binary(update_scene_frame("scene-1")))
        .unwrap();
    match tokio::time::timeout(Duration::from_secs(3), reload_ws.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
    {
        Message::Text(text) => {
            assert!(text.contains("SCENE_UPDATE"), "got {text}");
            assert!(text.contains("scene-1"));
        }
        other => panic!("expected the JSON scene-update frame first, got {other:?}"),
    }
    match tokio::time::timeout(Duration::from_secs(3), reload_ws.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
    {
        Message::Binary(bytes) => assert_eq!(bytes.to_vec(), update_scene_frame("scene-1")),
        other => panic!("expected the protobuf scene-update frame second, got {other:?}"),
    }

    let signer_tunneled = common::random_wallet();
    let signer_direct = common::random_wallet();
    let mut client_tunneled = connect_ws(&format!("{ws_url}/mini-comms/room-1"), "rfc5").await;
    let (alias_t, peers_t) = handshake(&mut client_tunneled, &signer_tunneled).await;
    assert!(peers_t.is_empty());

    let mut client_direct =
        connect_ws(&format!("ws://{preview_addr}/mini-comms/room-1"), "rfc5").await;
    let (alias_d, peers_d) = handshake(&mut client_direct, &signer_direct).await;
    assert_ne!(alias_t, alias_d);
    assert_eq!(
        peers_d.get(&alias_t).map(String::as_str),
        Some(wallet_address(&signer_tunneled).to_lowercase().as_str())
    );

    match recv_message(&mut client_tunneled).await {
        ws_packet::Message::PeerJoinMessage(join) => assert_eq!(join.alias, alias_d),
        other => panic!("expected peerJoinMessage through the tunnel, got {other:?}"),
    }

    send_packet(
        &mut client_tunneled,
        ws_packet::Message::PeerUpdateMessage(WsPeerUpdate {
            from_alias: 0,
            body: b"through the tunnel".to_vec(),
            unreliable: false,
        }),
    )
    .await;
    match recv_message(&mut client_direct).await {
        ws_packet::Message::PeerUpdateMessage(update) => {
            assert_eq!(update.from_alias, alias_t);
            assert_eq!(update.body, b"through the tunnel".to_vec());
        }
        other => panic!("expected the tunneled update on the direct client, got {other:?}"),
    }

    send_packet(
        &mut client_direct,
        ws_packet::Message::PeerUpdateMessage(WsPeerUpdate {
            from_alias: 0,
            body: b"back through the tunnel".to_vec(),
            unreliable: false,
        }),
    )
    .await;
    match recv_message(&mut client_tunneled).await {
        ws_packet::Message::PeerUpdateMessage(update) => {
            assert_eq!(update.from_alias, alias_d);
            assert_eq!(update.body, b"back through the tunnel".to_vec());
        }
        other => panic!("expected the direct update on the tunneled client, got {other:?}"),
    }

    client_tunneled.close(None).await.unwrap();
    match recv_message(&mut client_direct).await {
        ws_packet::Message::PeerLeaveMessage(leave) => assert_eq!(leave.alias, alias_t),
        other => panic!("expected peerLeaveMessage after tunneled close, got {other:?}"),
    }
}

#[tokio::test]
async fn unreachable_tunnel_reports_connect_failed_and_retries() {
    let mut events = spawn(AgentConfig {
        trunk_url: "ws://127.0.0.1:9/t/_connect".into(),
        token: None,
        local_port: 4242,
    });
    for _ in 0..2 {
        let event = tokio::time::timeout(Duration::from_secs(10), events.recv())
            .await
            .expect("timed out waiting for a connect failure")
            .expect("agent event stream ended");
        match event {
            AgentEvent::ConnectFailed { error } => {
                assert!(!error.is_empty());
            }
            other => panic!("expected ConnectFailed, got {other:?}"),
        }
    }
}
