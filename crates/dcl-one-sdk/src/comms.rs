use anyhow::{anyhow, bail, Context, Result};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use catalyrst_crypto::sign::verify_signed_message;
use catalyrst_crypto::AuthChain;
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

fn is_eth_address(addr: &str) -> bool {
    addr.len() == 42 && addr.starts_with("0x") && addr[2..].chars().all(|c| c.is_ascii_hexdigit())
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
