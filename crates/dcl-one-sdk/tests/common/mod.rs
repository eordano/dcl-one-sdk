#![allow(dead_code)]

use catalyrst_crypto::Wallet;
use dcl_one_sdk::comms::proto::{ws_packet, WsIdentification, WsPacket, WsSignedChallenge};
use futures::{SinkExt, StreamExt};
use prost::Message as _;
use std::net::SocketAddr;
use std::time::Duration;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

pub type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

pub async fn connect_ws(url: &str, protocol: &'static str) -> Socket {
    let mut request = url.into_client_request().unwrap();
    request
        .headers_mut()
        .insert("Sec-WebSocket-Protocol", HeaderValue::from_static(protocol));
    let (socket, resp) = tokio_tungstenite::connect_async(request).await.unwrap();
    assert_eq!(
        resp.headers()
            .get("sec-websocket-protocol")
            .expect("server must echo the negotiated subprotocol"),
        protocol
    );
    socket
}

pub async fn connect(addr: SocketAddr, room: &str, protocol: &'static str) -> Socket {
    connect_ws(&format!("ws://{addr}/mini-comms/{room}"), protocol).await
}

pub async fn send_packet(socket: &mut Socket, message: ws_packet::Message) {
    let packet = WsPacket {
        message: Some(message),
    };
    socket
        .send(Message::Binary(packet.encode_to_vec().into()))
        .await
        .unwrap();
}

pub async fn recv_message(socket: &mut Socket) -> ws_packet::Message {
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(3), socket.next())
            .await
            .expect("timed out waiting for a ws-room packet")
            .expect("stream ended unexpectedly")
            .expect("ws frame error");
        match msg {
            Message::Binary(bytes) => {
                let packet = WsPacket::decode(bytes.as_ref()).expect("valid WsPacket");
                return packet.message.expect("non-empty WsPacket");
            }
            Message::Ping(_) | Message::Pong(_) => continue,
            other => panic!("expected binary WsPacket, got {other:?}"),
        }
    }
}

pub fn random_wallet() -> Wallet {
    loop {
        let bytes: [u8; 32] = rand::random();
        if let Ok(w) = Wallet::from_hex(&hex::encode(bytes)) {
            return w;
        }
    }
}

pub fn wallet_address(signer: &Wallet) -> String {
    signer.address()
}

pub fn sign_challenge(signer: &Wallet, challenge: &str) -> String {
    catalyrst_crypto::create_simple_auth_chain(signer, challenge)
        .unwrap()
        .to_string()
}

pub async fn handshake(
    socket: &mut Socket,
    signer: &Wallet,
) -> (u32, std::collections::HashMap<u32, String>) {
    send_packet(
        socket,
        ws_packet::Message::PeerIdentification(WsIdentification {
            address: wallet_address(signer),
        }),
    )
    .await;
    loop {
        match recv_message(socket).await {
            ws_packet::Message::ChallengeMessage(challenge) => {
                assert!(
                    challenge.challenge_to_sign.starts_with("dcl-"),
                    "challenge must start with dcl-, got {}",
                    challenge.challenge_to_sign
                );
                let auth_chain_json = sign_challenge(signer, &challenge.challenge_to_sign);
                send_packet(
                    socket,
                    ws_packet::Message::SignedChallengeForServer(WsSignedChallenge {
                        auth_chain_json,
                    }),
                )
                .await;
            }
            ws_packet::Message::WelcomeMessage(welcome) => {
                return (welcome.alias, welcome.peer_identities);
            }
            other => panic!("unexpected handshake message: {other:?}"),
        }
    }
}
