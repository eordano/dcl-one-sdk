use anyhow::{anyhow, bail, Context, Result};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use catalyrst_auth_chain::is_eth_address;
use catalyrst_crypto::sign::verify_signed_message;
use catalyrst_crypto::AuthChain;
use futures::stream::SplitSink;
use futures::{SinkExt, StreamExt};
use prost::Message as _;
use rand::RngExt;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;
use tokio::sync::mpsc;

pub mod proto {
    include!(concat!(
        env!("OUT_DIR"),
        "/decentraland.kernel.comms.rfc5.rs"
    ));
}

use proto::{
    ws_packet, WsChallengeRequired, WsKicked, WsPacket, WsPeerJoin, WsPeerLeave, WsPeerUpdate,
    WsWelcome,
};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_millis(1000);

#[derive(Default)]
pub struct CommsState {
    registry: Mutex<Registry>,
}

impl CommsState {
    fn reg(&self) -> MutexGuard<'_, Registry> {
        self.registry.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A session is identified by (room, address): one wallet may sit in the realm
/// room and in any number of scene rooms at once, and only a second session in
/// the SAME room displaces the first.
#[derive(Default)]
struct Registry {
    counter: u32,
    rooms: HashMap<String, Room>,
}

impl Registry {
    /// The alias `address` currently holds in `room_id`, if any.
    fn session_in(&self, room_id: &str, address: &str) -> Option<u32> {
        self.rooms
            .get(room_id)?
            .iter()
            .find(|(_, peer)| peer.address == address)
            .map(|(alias, _)| *alias)
    }
}

type Room = HashMap<u32, Peer>;

struct Peer {
    address: String,
    tx: mpsc::UnboundedSender<PeerFrame>,
}

enum PeerFrame {
    Packet(Vec<u8>),
    Kick(Vec<u8>),
}

pub fn routes(state: Arc<CommsState>) -> Router {
    Router::new()
        .route("/mini-comms/{room_id}", get(ws_upgrade))
        .route("/mini-comms/{room_id}/host", get(host_upgrade))
        .with_state(state)
}

async fn ws_upgrade(
    ws: WebSocketUpgrade,
    Path(room_id): Path<String>,
    State(st): State<Arc<CommsState>>,
) -> Response {
    ws.protocols(["rfc5", "rfc4"])
        .on_upgrade(move |socket| handle_socket(socket, st, room_id))
}

fn craft(message: ws_packet::Message) -> Vec<u8> {
    let packet = WsPacket {
        message: Some(message),
    };
    let mut buf = Vec::with_capacity(packet.encoded_len());
    packet.encode(&mut buf).expect("WsPacket encodes");
    buf
}

fn send_all(room: &Room, except: Option<u32>, bytes: &[u8]) {
    for (alias, peer) in room {
        if Some(*alias) != except {
            let _ = peer.tx.send(PeerFrame::Packet(bytes.to_vec()));
        }
    }
}

async fn recv_packet(socket: &mut WebSocket, timeout_error: &str) -> Result<WsPacket> {
    let recv = async {
        loop {
            match socket.recv().await {
                Some(Ok(Message::Binary(bytes))) => {
                    return WsPacket::decode(bytes.as_ref()).context("decoding WsPacket");
                }
                Some(Ok(Message::Close(_))) | None => bail!("connection closed"),
                Some(Ok(_)) => continue,
                Some(Err(e)) => return Err(e).context("websocket receive"),
            }
        }
    };
    match tokio::time::timeout(HANDSHAKE_TIMEOUT, recv).await {
        Ok(result) => result,
        Err(_) => bail!("{timeout_error}"),
    }
}

async fn handshake(socket: &mut WebSocket, st: &CommsState, room_id: &str) -> Result<String> {
    let packet = recv_packet(socket, "Timed out waiting for peer identification").await?;
    let Some(ws_packet::Message::PeerIdentification(ident)) = packet.message else {
        bail!("Invalid protocol. peerIdentification packet missed");
    };
    if !is_eth_address(&ident.address) {
        bail!("Invalid protocol. peerIdentification has an invalid address");
    }
    let address = ident.address.to_lowercase();
    let challenge_to_sign = format!("dcl-{:x}", rand::rng().random::<u128>());
    let already_connected = st.reg().session_in(room_id, &address).is_some();
    tracing::debug!(
        room = %room_id,
        challenge_to_sign,
        address,
        already_connected,
        "mini-comms generating challenge"
    );
    socket
        .send(Message::Binary(
            craft(ws_packet::Message::ChallengeMessage(WsChallengeRequired {
                challenge_to_sign: challenge_to_sign.clone(),
                already_connected,
            }))
            .into(),
        ))
        .await
        .context("sending challenge")?;
    let packet = recv_packet(socket, "Timed out waiting for signed challenge response").await?;
    let Some(ws_packet::Message::SignedChallengeForServer(signed)) = packet.message else {
        bail!("Invalid protocol. signedChallengeForServer packet missed");
    };
    let chain: AuthChain =
        serde_json::from_str(&signed.auth_chain_json).context("parsing authChainJson")?;
    verify_signed_message(&chain, &challenge_to_sign, &address, None)
        .map_err(|e| anyhow!("Authentication failed: {e}"))?;
    Ok(address)
}

/// Removes `alias` from its room, dropping the room when it empties and telling
/// the rest of the room otherwise.
fn drop_peer(reg: &mut Registry, room_id: &str, alias: u32) -> Option<Peer> {
    let room = reg.rooms.get_mut(room_id)?;
    let peer = room.remove(&alias)?;
    if room.is_empty() {
        reg.rooms.remove(room_id);
    } else {
        let leave = craft(ws_packet::Message::PeerLeaveMessage(WsPeerLeave { alias }));
        send_all(room, None, &leave);
    }
    Some(peer)
}

fn join_room(
    st: &CommsState,
    room_id: &str,
    address: &str,
    tx: mpsc::UnboundedSender<PeerFrame>,
) -> (u32, Vec<u8>) {
    let mut reg = st.reg();
    reg.counter += 1;
    let alias = reg.counter;
    if let Some(old_alias) = reg.session_in(room_id, address) {
        if let Some(old_peer) = drop_peer(&mut reg, room_id, old_alias) {
            tracing::info!(room = %room_id, address, alias = old_alias, "mini-comms kicking previous session");
            let _ = old_peer
                .tx
                .send(PeerFrame::Kick(craft(ws_packet::Message::PeerKicked(
                    WsKicked {
                        reason: "Already logged in".to_string(),
                    },
                ))));
        }
    }
    let room = reg.rooms.entry(room_id.to_string()).or_default();
    let peer_identities: HashMap<u32, String> = room
        .iter()
        .filter(|(_, peer)| peer.address != address)
        .map(|(peer_alias, peer)| (*peer_alias, peer.address.clone()))
        .collect();
    let join = craft(ws_packet::Message::PeerJoinMessage(WsPeerJoin {
        alias,
        address: address.to_string(),
    }));
    send_all(room, None, &join);
    room.insert(
        alias,
        Peer {
            address: address.to_string(),
            tx,
        },
    );
    let welcome = craft(ws_packet::Message::WelcomeMessage(WsWelcome {
        alias,
        peer_identities,
    }));
    (alias, welcome)
}

fn broadcast_update(st: &CommsState, room_id: &str, from_alias: u32, update: WsPeerUpdate) {
    let reg = st.reg();
    let Some(room) = reg.rooms.get(room_id) else {
        return;
    };
    let bytes = craft(ws_packet::Message::PeerUpdateMessage(WsPeerUpdate {
        from_alias,
        body: update.body,
        unreliable: update.unreliable,
    }));
    send_all(room, Some(from_alias), &bytes);
}

fn leave_room(st: &CommsState, room_id: &str, alias: u32) {
    drop_peer(&mut st.reg(), room_id, alias);
}

async fn deliver(sink: &mut SplitSink<WebSocket, Message>, frame: Option<PeerFrame>) -> Result<()> {
    match frame {
        Some(PeerFrame::Packet(bytes)) => {
            sink.send(Message::Binary(bytes.into()))
                .await
                .context("forwarding packet")?;
            Ok(())
        }
        Some(PeerFrame::Kick(bytes)) => {
            let _ = sink.send(Message::Binary(bytes.into())).await;
            let _ = sink.send(Message::Close(None)).await;
            bail!("kicked")
        }
        None => bail!("peer channel closed"),
    }
}

/// The host peer's address: valid to every eth-address check yet mintable by
/// no wallet, so no client can collide with or spoof the host slot.
const HOST_ADDRESS: &str = "0x0000000000000000000000000000000000000000";

/// JSON side door for the scene host (multiplayer-server-design.md, M1). The
/// host is our own process beside the preview, so it skips the signed
/// handshake and is loopback-only; the relay transcodes protobuf both ways and
/// stamps `fromAddress` from the registry, which is why the host may trust it.
async fn host_upgrade(
    ws: WebSocketUpgrade,
    Path(room_id): Path<String>,
    State(st): State<Arc<CommsState>>,
    request: axum::extract::Request,
) -> Response {
    let loopback = request
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .is_some_and(|ci| ci.0.ip().is_loopback());
    if !loopback {
        return axum::response::IntoResponse::into_response((
            axum::http::StatusCode::FORBIDDEN,
            "the scene host joins from the machine hosting this preview\n",
        ));
    }
    ws.on_upgrade(move |socket| handle_host_socket(socket, st, room_id))
}

#[derive(serde::Deserialize)]
struct HostFrame {
    body: String,
    #[serde(default)]
    unreliable: bool,
    #[serde(default)]
    to: Option<Vec<String>>,
}

fn host_json(st: &CommsState, room_id: &str, packet: &[u8]) -> Option<String> {
    use base64::Engine as _;
    let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
    let decoded = WsPacket::decode(packet).ok()?;
    let reg = st.reg();
    let addr_of = |alias: u32| -> String {
        reg.rooms
            .get(room_id)
            .and_then(|room| room.get(&alias))
            .map(|p| p.address.clone())
            .unwrap_or_default()
    };
    let v = match decoded.message? {
        ws_packet::Message::WelcomeMessage(w) => serde_json::json!({
            "type": "welcome", "alias": w.alias, "peers": w.peer_identities
        }),
        ws_packet::Message::PeerJoinMessage(j) => serde_json::json!({
            "type": "join", "alias": j.alias, "address": j.address
        }),
        ws_packet::Message::PeerLeaveMessage(l) => serde_json::json!({
            "type": "leave", "alias": l.alias, "address": addr_of(l.alias)
        }),
        ws_packet::Message::PeerUpdateMessage(u) => serde_json::json!({
            "type": "update", "from": u.from_alias,
            "fromAddress": addr_of(u.from_alias), "body": b64(&u.body)
        }),
        ws_packet::Message::PeerKicked(k) => serde_json::json!({
            "type": "kicked", "reason": k.reason
        }),
        _ => return None,
    };
    Some(v.to_string())
}

fn send_update_to(st: &CommsState, room_id: &str, from_alias: u32, to: &[String], bytes: &[u8]) {
    let reg = st.reg();
    let Some(room) = reg.rooms.get(room_id) else {
        return;
    };
    let wanted: std::collections::HashSet<String> = to.iter().map(|a| a.to_lowercase()).collect();
    for (peer_alias, peer) in room {
        if *peer_alias == from_alias || !wanted.contains(&peer.address) {
            continue;
        }
        let _ = peer.tx.send(PeerFrame::Packet(bytes.to_vec()));
    }
}

async fn handle_host_socket(socket: WebSocket, st: Arc<CommsState>, room_id: String) {
    use base64::Engine as _;
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (alias, welcome) = join_room(&st, &room_id, HOST_ADDRESS, tx);
    tracing::info!(room = %room_id, alias, "mini-comms host joined");
    let (mut sink, mut stream) = socket.split();
    if let Some(json) = host_json(&st, &room_id, &welcome) {
        if sink.send(Message::Text(json.into())).await.is_err() {
            leave_room(&st, &room_id, alias);
            return;
        }
    }
    loop {
        tokio::select! {
            frame = rx.recv() => match frame {
                Some(PeerFrame::Packet(bytes)) => {
                    if let Some(json) = host_json(&st, &room_id, &bytes) {
                        if sink.send(Message::Text(json.into())).await.is_err() {
                            break;
                        }
                    }
                }
                Some(PeerFrame::Kick(_)) | None => break,
            },
            incoming = stream.next() => match incoming {
                Some(Ok(Message::Text(text))) => {
                    let Ok(frame) = serde_json::from_str::<HostFrame>(&text) else {
                        tracing::warn!(room = %room_id, "host sent an undecodable frame, terminating");
                        break;
                    };
                    let Ok(body) =
                        base64::engine::general_purpose::STANDARD.decode(&frame.body)
                    else {
                        tracing::warn!(room = %room_id, "host update body is not base64, terminating");
                        break;
                    };
                    let update = WsPeerUpdate {
                        from_alias: alias,
                        body,
                        unreliable: frame.unreliable,
                    };
                    match &frame.to {
                        Some(to) => {
                            let bytes = craft(ws_packet::Message::PeerUpdateMessage(update));
                            send_update_to(&st, &room_id, alias, to, &bytes);
                        }
                        None => broadcast_update(&st, &room_id, alias, update),
                    }
                }
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(_)) => {}
                Some(Err(_)) => break,
            }
        }
    }
    leave_room(&st, &room_id, alias);
    tracing::info!(room = %room_id, alias, "mini-comms host disconnected");
}

async fn handle_socket(mut socket: WebSocket, st: Arc<CommsState>, room_id: String) {
    let address = match handshake(&mut socket, &st, &room_id).await {
        Ok(address) => address,
        Err(e) => {
            tracing::warn!(room = %room_id, "mini-comms handshake failed: {e:#}");
            let _ = socket.send(Message::Close(None)).await;
            return;
        }
    };
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (alias, welcome) = join_room(&st, &room_id, &address, tx);
    tracing::info!(room = %room_id, address, alias, "mini-comms peer welcomed");
    let (mut sink, mut stream) = socket.split();
    if sink.send(Message::Binary(welcome.into())).await.is_err() {
        leave_room(&st, &room_id, alias);
        return;
    }
    loop {
        tokio::select! {
            frame = rx.recv() => {
                if deliver(&mut sink, frame).await.is_err() {
                    break;
                }
            }
            incoming = stream.next() => match incoming {
                Some(Ok(Message::Binary(bytes))) => match WsPacket::decode(bytes.as_ref()) {
                    Ok(WsPacket {
                        message: Some(ws_packet::Message::PeerUpdateMessage(update)),
                    }) => broadcast_update(&st, &room_id, alias, update),
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!(room = %room_id, alias, "mini-comms undecodable frame, terminating: {e}");
                        break;
                    }
                },
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(_)) => {}
                Some(Err(_)) => break,
            }
        }
    }
    leave_room(&st, &room_id, alias);
    tracing::info!(room = %room_id, address, alias, "mini-comms peer disconnected");
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc::UnboundedReceiver;

    const ALICE: &str = "0x00000000000000000000000000000000000000a1";
    const BOB: &str = "0x00000000000000000000000000000000000000b2";

    fn join(st: &CommsState, room: &str, address: &str) -> (u32, UnboundedReceiver<PeerFrame>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let (alias, _welcome) = join_room(st, room, address, tx);
        (alias, rx)
    }

    fn was_kicked(rx: &mut UnboundedReceiver<PeerFrame>) -> bool {
        std::iter::from_fn(|| rx.try_recv().ok()).any(|f| matches!(f, PeerFrame::Kick(_)))
    }

    fn aliases(st: &CommsState, room: &str) -> Vec<u32> {
        let reg = st.reg();
        let mut out: Vec<u32> = reg
            .rooms
            .get(room)
            .map(|r| r.keys().copied().collect())
            .unwrap_or_default();
        out.sort();
        out
    }

    #[test]
    fn the_same_address_holds_a_session_in_every_room_it_joins() {
        let st = CommsState::default();
        let (realm, mut realm_rx) = join(&st, "room-1", ALICE);
        let (scene, mut scene_rx) = join(&st, "scene-bafy1", ALICE);
        assert_eq!(aliases(&st, "room-1"), vec![realm]);
        assert_eq!(aliases(&st, "scene-bafy1"), vec![scene]);
        assert!(
            !was_kicked(&mut realm_rx),
            "joining a scene room must not kick the realm session"
        );
        assert!(!was_kicked(&mut scene_rx));
    }

    #[test]
    fn a_second_session_in_the_same_room_kicks_the_first() {
        let st = CommsState::default();
        let (first, mut first_rx) = join(&st, "room-1", ALICE);
        let (other, mut other_rx) = join(&st, "scene-bafy1", ALICE);
        let (second, mut second_rx) = join(&st, "room-1", ALICE);
        assert_ne!(first, second);
        assert!(was_kicked(&mut first_rx));
        assert!(!was_kicked(&mut second_rx));
        assert_eq!(aliases(&st, "room-1"), vec![second]);
        assert!(
            !was_kicked(&mut other_rx),
            "the kick is scoped to the room being joined"
        );
        assert_eq!(aliases(&st, "scene-bafy1"), vec![other]);
    }

    #[test]
    fn already_connected_is_answered_per_room() {
        let st = CommsState::default();
        let (alias, _rx) = join(&st, "room-1", ALICE);
        let (_bob, _bob_rx) = join(&st, "room-1", BOB);
        assert_eq!(st.reg().session_in("room-1", ALICE), Some(alias));
        assert_eq!(st.reg().session_in("scene-bafy1", ALICE), None);
        assert_eq!(
            st.reg()
                .session_in("room-1", "0x00000000000000000000000000000000000000c3"),
            None
        );
    }

    #[test]
    fn leaving_one_room_keeps_the_sessions_in_the_others() {
        let st = CommsState::default();
        let (realm, _realm_rx) = join(&st, "room-1", ALICE);
        let (scene, _scene_rx) = join(&st, "scene-bafy1", ALICE);
        leave_room(&st, "scene-bafy1", scene);
        assert_eq!(st.reg().session_in("scene-bafy1", ALICE), None);
        assert!(
            !st.reg().rooms.contains_key("scene-bafy1"),
            "an emptied room is dropped"
        );
        assert_eq!(st.reg().session_in("room-1", ALICE), Some(realm));
    }
}
