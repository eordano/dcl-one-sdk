//! The authoritative host runs upstream's real room protocol end to end: a
//! scene built with `authoritativeMultiplayer` under the host isolate, joined
//! through the preview's mini-comms door, answers a fake explorer peer the way
//! upstream's `@dcl/sdk/network` promises — a unicast CRDT state reply to the
//! peer that asked, and a registered-message pong broadcast with the asker's
//! address as the sender context.
//!
//! Wire facts pinned here (js-sdk-toolchain `network/binary-message-bus.js`
//! and `network/events/protocol.js` on the auth-server line): a scene frame is
//! `[type u8][payload]`; the host stamps inbound frames as
//! `[senderLen u8][sender][frame]`; a custom event's payload is the envelope
//! `{eventType: String, timestamp: Int64}` followed by the registered schema,
//! all little-endian with u32 length-prefixed utf8 strings.

mod common;

use common::{connect, handshake};
use dcl_one_sdk::build::{self, BuildOptions};
use dcl_one_sdk::comms::proto::{ws_packet, WsPacket, WsPeerUpdate};
use dcl_one_sdk::comms::{routes, CommsState};
use dcl_one_sdk::{host, init};
use futures::{SinkExt, StreamExt};
use prost::Message as _;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_tungstenite::tungstenite::Message;

const HOST_ADDRESS: &str = "0x0000000000000000000000000000000000000000";
const CUSTOM_EVENT: u8 = 6;
const REQ_CRDT_STATE: u8 = 8;
const RES_CRDT_STATE: u8 = 9;

/// The proof scene: one synced cube (so the state reply carries bytes) and a
/// server-side ping handler that answers with the sender the room handed it.
const SCENE: &str = r#"import { engine, Transform, MeshRenderer, Schemas } from '@dcl/sdk/ecs'
import { Vector3 } from '@dcl/sdk/math'
import { isServer, registerMessages, syncEntity } from '@dcl/sdk/network'

const room = registerMessages({
  ping: Schemas.Map({ n: Schemas.Int }),
  pong: Schemas.Map({ n: Schemas.Int, from: Schemas.String })
})

export function main() {
  const cube = engine.addEntity()
  Transform.create(cube, { position: Vector3.create(8, 1, 8) })
  MeshRenderer.setBox(cube)
  syncEntity(cube, [Transform.componentId], 1)
  if (isServer()) {
    room.onMessage('ping', (msg, ctx) => {
      room.send('pong', { n: msg.n + 1, from: ctx?.from ?? '' })
    })
  }
}
"#;

fn node_bin() -> Option<PathBuf> {
    match build::find_node() {
        Some(p) => Some(p),
        None => common::testgate::unavailable(
            "node",
            "install node; the authoritative host runs the scene under it",
        ),
    }
}

/// A fresh scaffold with the flag set and the proof scene in place, built
/// the way `start` builds it.
async fn build_flagged_scene() -> PathBuf {
    for key in ["DCL_PRIVATE_KEY", "RUST_LOG", "NO_COLOR"] {
        std::env::remove_var(key);
    }
    let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("host-room")
        .join(format!("scene-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    init::init(&init::InitOptions {
        dir: root.clone(),
        project: Some(init::ProjectKind::Scene),
        yes: true,
        node_modules_only: false,
    })
    .expect("scaffolding the proof scene");
    std::fs::write(root.join("src/index.ts"), SCENE).unwrap();
    let scene_json = root.join("scene.json");
    let mut scene: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&scene_json).unwrap()).unwrap();
    scene["authoritativeMultiplayer"] = serde_json::Value::Bool(true);
    std::fs::write(&scene_json, serde_json::to_string_pretty(&scene).unwrap()).unwrap();
    build::build(&BuildOptions {
        built_in: false,
        dir: root.clone(),
        production: true,
        ignore_composite: false,
        custom_entry_point: false,
        skip_type_check: false,
        out_root: None,
        quiet: true,
    })
    .await
    .expect("building the proof scene");
    dunce::canonicalize(root).unwrap()
}

/// The comms routes alone, served with connect info so the loopback-only
/// host door admits the isolate.
async fn spawn_comms() -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = routes(Arc::new(CommsState::default()));
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    addr
}

fn u32_le(n: u32) -> [u8; 4] {
    n.to_le_bytes()
}

/// `[CUSTOM_EVENT][envelope][payload]` for `ping {n}`.
fn ping_frame(n: i32, timestamp: i64) -> Vec<u8> {
    let mut out = vec![CUSTOM_EVENT];
    out.extend_from_slice(&u32_le(4));
    out.extend_from_slice(b"ping");
    out.extend_from_slice(&timestamp.to_le_bytes());
    out.extend_from_slice(&n.to_le_bytes());
    out
}

/// Decodes a `pong {n, from}` custom event; `None` for any other frame.
fn decode_pong(frame: &[u8]) -> Option<(i32, String)> {
    let mut at = 0usize;
    let take = |at: &mut usize, n: usize| -> Option<&[u8]> {
        let slice = frame.get(*at..*at + n)?;
        *at += n;
        Some(slice)
    };
    if *take(&mut at, 1)?.first()? != CUSTOM_EVENT {
        return None;
    }
    let read_string = |at: &mut usize| -> Option<String> {
        let len = u32::from_le_bytes(take(at, 4)?.try_into().ok()?) as usize;
        String::from_utf8(take(at, len)?.to_vec()).ok()
    };
    if read_string(&mut at)? != "pong" {
        return None;
    }
    take(&mut at, 8)?;
    let n = i32::from_le_bytes(take(&mut at, 4)?.try_into().ok()?);
    let from = read_string(&mut at)?;
    (at == frame.len()).then_some((n, from))
}

/// The next peer update on the socket before `deadline`, skipping presence
/// traffic; the host may take a while to load the scene under node.
async fn next_update(socket: &mut common::Socket, deadline: Instant) -> WsPeerUpdate {
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        assert!(!left.is_zero(), "timed out waiting for a host update");
        let msg = tokio::time::timeout(left, socket.next())
            .await
            .expect("timed out waiting for a host update")
            .expect("the room closed")
            .expect("ws frame error");
        match msg {
            Message::Binary(bytes) => {
                let packet = WsPacket::decode(bytes.as_ref()).expect("valid WsPacket");
                if let Some(ws_packet::Message::PeerUpdateMessage(u)) = packet.message {
                    return u;
                }
            }
            Message::Ping(_) | Message::Pong(_) => continue,
            other => panic!("unexpected frame {other:?}"),
        }
    }
}

async fn send_scene_frame(socket: &mut common::Socket, body: Vec<u8>) {
    let packet = WsPacket {
        message: Some(ws_packet::Message::PeerUpdateMessage(WsPeerUpdate {
            from_alias: 0,
            body,
            unreliable: false,
        })),
    };
    socket
        .send(Message::Binary(packet.encode_to_vec().into()))
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_host_answers_state_requests_and_registered_messages() {
    if node_bin().is_none() {
        return;
    }
    let root = build_flagged_scene().await;
    let addr = spawn_comms().await;
    let preview = format!("http://{addr}");
    let _isolate = host::spawn_isolate(&root, &preview, "room-1").expect("spawning the host");

    let signer = common::random_wallet();
    let me = common::wallet_address(&signer).to_lowercase();
    let mut peer = connect(addr, "room-1", "rfc5").await;
    let (_alias, peers) = handshake(&mut peer, &signer).await;
    let deadline = Instant::now() + Duration::from_secs(90);
    // the host may have joined before us (in the welcome) or after (a join)
    if !peers.values().any(|a| a == HOST_ADDRESS) {
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let msg = tokio::time::timeout(left, peer.next())
                .await
                .expect("timed out waiting for the host to join")
                .unwrap()
                .unwrap();
            if let Message::Binary(bytes) = msg {
                let packet = WsPacket::decode(bytes.as_ref()).unwrap();
                if let Some(ws_packet::Message::PeerJoinMessage(j)) = packet.message {
                    if j.address == HOST_ADDRESS {
                        break;
                    }
                }
            }
        }
    }

    // upstream's client opens with REQ_CRDT_STATE; the host answers the asker
    // alone with RES_CRDT_STATE (the relay unicasts on the host's `to` list)
    send_scene_frame(&mut peer, vec![REQ_CRDT_STATE]).await;
    let state = next_update(&mut peer, deadline).await;
    assert_eq!(
        state.body.first(),
        Some(&RES_CRDT_STATE),
        "the host answers a state request with RES_CRDT_STATE, got {:?}",
        &state.body[..state.body.len().min(16)]
    );
    assert!(
        state.body.len() > 1,
        "the synced cube gives the state reply a body"
    );

    // a registered message reaches the server handler with the sender in its
    // context, and the reply comes back through the same room
    send_scene_frame(&mut peer, ping_frame(1, 1_700_000_000_000)).await;
    let (n, from) = loop {
        let update = next_update(&mut peer, deadline).await;
        if let Some(pong) = decode_pong(&update.body) {
            break pong;
        }
    };
    assert_eq!(n, 2, "the handler adds one");
    assert_eq!(from, me, "the server handler sees the asker's address");
}

#[test]
fn ping_frames_round_trip_through_the_decoder() {
    let mut pong = ping_frame(7, 42);
    pong.splice(5..9, *b"pong");
    pong.extend_from_slice(&u32_le(2));
    pong.extend_from_slice(b"me");
    assert_eq!(decode_pong(&pong), Some((7, "me".to_string())));
    assert_eq!(decode_pong(&ping_frame(1, 1)), None, "a ping is not a pong");
    assert_eq!(decode_pong(&[RES_CRDT_STATE, 1, 2]), None);
}
