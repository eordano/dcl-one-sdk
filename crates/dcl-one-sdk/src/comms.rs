use anyhow::{anyhow, bail, Context, Result};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use catalyrst_crypto::sign::verify_signed_message;
use catalyrst_crypto::AuthChain;
use catalyrst_types::is_eth_address;
use futures::stream::SplitSink;
use futures::{SinkExt, StreamExt};
use prost::Message as _;
use rand::RngExt;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
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

#[derive(Default)]
struct Registry {
    counter: u32,
    rooms: HashMap<String, HashMap<u32, Peer>>,
    addresses: HashMap<String, (String, u32)>,
}

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

async fn handshake(socket: &mut WebSocket, st: &CommsState) -> Result<String> {
    let packet = recv_packet(socket, "Timed out waiting for peer identification").await?;
    let Some(ws_packet::Message::PeerIdentification(ident)) = packet.message else {
        bail!("Invalid protocol. peerIdentification packet missed");
    };
    if !is_eth_address(&ident.address) {
        bail!("Invalid protocol. peerIdentification has an invalid address");
    }
    let address = ident.address.to_lowercase();
    let challenge_to_sign = format!("dcl-{:x}", rand::rng().random::<u128>());
    let already_connected = st
        .registry
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .addresses
        .contains_key(&address);
    tracing::debug!(
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

fn join_room(
    st: &CommsState,
    room_id: &str,
    address: &str,
    tx: mpsc::UnboundedSender<PeerFrame>,
) -> (u32, Vec<u8>) {
    let mut reg = st
        .registry
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    reg.counter += 1;
    let alias = reg.counter;
    if let Some((old_room, old_alias)) = reg.addresses.remove(address) {
        if let Some(room) = reg.rooms.get_mut(&old_room) {
            if let Some(old_peer) = room.remove(&old_alias) {
                tracing::info!(room = %old_room, address, alias = old_alias, "mini-comms kicking previous session");
                let _ = old_peer
                    .tx
                    .send(PeerFrame::Kick(craft(ws_packet::Message::PeerKicked(
                        WsKicked {
                            reason: "Already logged in".to_string(),
                        },
                    ))));
            }
            if room.is_empty() {
                reg.rooms.remove(&old_room);
            } else {
                let leave = craft(ws_packet::Message::PeerLeaveMessage(WsPeerLeave {
                    alias: old_alias,
                }));
                for peer in reg.rooms[&old_room].values() {
                    let _ = peer.tx.send(PeerFrame::Packet(leave.clone()));
                }
            }
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
    for peer in room.values() {
        let _ = peer.tx.send(PeerFrame::Packet(join.clone()));
    }
    room.insert(
        alias,
        Peer {
            address: address.to_string(),
            tx,
        },
    );
    reg.addresses
        .insert(address.to_string(), (room_id.to_string(), alias));
    let welcome = craft(ws_packet::Message::WelcomeMessage(WsWelcome {
        alias,
        peer_identities,
    }));
    (alias, welcome)
}

fn broadcast_update(st: &CommsState, room_id: &str, from_alias: u32, update: WsPeerUpdate) {
    let reg = st
        .registry
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(room) = reg.rooms.get(room_id) else {
        return;
    };
    let bytes = craft(ws_packet::Message::PeerUpdateMessage(WsPeerUpdate {
        from_alias,
        body: update.body,
        unreliable: update.unreliable,
    }));
    for (peer_alias, peer) in room {
        if *peer_alias == from_alias {
            continue;
        }
        let _ = peer.tx.send(PeerFrame::Packet(bytes.clone()));
    }
}

fn leave_room(st: &CommsState, room_id: &str, alias: u32, address: &str) {
    let mut reg = st
        .registry
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(room) = reg.rooms.get_mut(room_id) else {
        return;
    };
    if room.remove(&alias).is_none() {
        return;
    }
    let now_empty = room.is_empty();
    if reg
        .addresses
        .get(address)
        .is_some_and(|(r, a)| r == room_id && *a == alias)
    {
        reg.addresses.remove(address);
    }
    if now_empty {
        reg.rooms.remove(room_id);
    } else {
        let leave = craft(ws_packet::Message::PeerLeaveMessage(WsPeerLeave { alias }));
        for peer in reg.rooms[room_id].values() {
            let _ = peer.tx.send(PeerFrame::Packet(leave.clone()));
        }
    }
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

/* ---------- host side-door (multiplayer-server-design.md, M1) ----------
The scene host is just another room peer, but it is OUR process: it joins
through this JSON door instead of the protobuf handshake, and the relay
transcodes both ways. Sender addresses on inbound updates are stamped by
the relay from the room registry -- the host trusts ctx.from because this
process verified the signed challenge, not because a client said so.

Loopback-only: the host runs beside the preview. A stranger on the LAN
gets a 403 here where a wallet-signed handshake meets them on the peer
door. Frames, host -> relay:
  {"type":"update","body":"<base64>","unreliable":bool,"to":["0x..",..]?}
relay -> host:
  {"type":"welcome","alias":n,"peers":{"<alias>":"0x..",..}}
  {"type":"join","alias":n,"address":"0x.."}
  {"type":"leave","alias":n,"address":"0x.."}
  {"type":"update","from":n,"fromAddress":"0x..","body":"<base64>"}   */

/// The address the host peer joins under: the zero address is valid to every
/// eth-address check yet mintable by no wallet, so a real client can never
/// collide with (or spoof) the host slot through the signed door.
const HOST_ADDRESS: &str = "0x0000000000000000000000000000000000000000";

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
    let reg = st
        .registry
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
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
    let reg = st
        .registry
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
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
            leave_room(&st, &room_id, alias, HOST_ADDRESS);
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
    leave_room(&st, &room_id, alias, HOST_ADDRESS);
    tracing::info!(room = %room_id, alias, "mini-comms host disconnected");
}

async fn handle_socket(mut socket: WebSocket, st: Arc<CommsState>, room_id: String) {
    let address = match handshake(&mut socket, &st).await {
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
        leave_room(&st, &room_id, alias, &address);
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
    leave_room(&st, &room_id, alias, &address);
    tracing::info!(room = %room_id, address, alias, "mini-comms peer disconnected");
}
