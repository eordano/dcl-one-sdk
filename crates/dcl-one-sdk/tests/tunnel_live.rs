mod common;

use common::{connect_ws, handshake, recv_message, send_packet, wallet_address};
use dcl_one_sdk::comms::proto::{ws_packet, WsPeerUpdate};
use futures::StreamExt;
use serde_json::Value;
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

fn ws_base(http_base: &str) -> String {
    http_base
        .replacen("https://", "wss://", 1)
        .replacen("http://", "ws://", 1)
}

#[tokio::test]
async fn drive_a_live_tunnel_origin() {
    let Ok(public) = std::env::var("DCL1_TUNNEL_PUBLIC_URL") else {
        eprintln!(
            "skipping: DCL1_TUNNEL_PUBLIC_URL not set (this test drives a live start --tunnel)"
        );
        return;
    };
    let local = std::env::var("DCL1_TUNNEL_LOCAL_URL").ok();
    let touch = std::env::var("DCL1_TUNNEL_TOUCH_FILE").ok();
    let public = public.trim_end_matches('/').to_string();
    let prefix = url::Url::parse(&public)
        .unwrap()
        .path()
        .trim_end_matches('/')
        .to_string();

    let about: Value = reqwest::get(format!("{public}/about"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let adapter = about["comms"]["fixedAdapter"].as_str().unwrap();
    assert!(
        adapter.contains(&format!("{prefix}/mini-comms/room-1")),
        "fixedAdapter must carry the tunnel prefix, got {adapter}"
    );
    let content_url = about["content"]["publicUrl"].as_str().unwrap();
    assert!(
        content_url.contains(&prefix),
        "content.publicUrl must carry the tunnel prefix, got {content_url}"
    );
    let scenes_urn = about["configurations"]["scenesUrn"][0].as_str().unwrap();
    assert!(scenes_urn.contains(&format!("{prefix}/content/contents/")));
    println!("PASS /about through tunnel: fixedAdapter={adapter}");

    let client = reqwest::Client::new();
    let entities: Value = client
        .post(format!("{public}/content/entities/active"))
        .json(&serde_json::json!({ "pointers": [] }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let content = entities[0]["content"].as_array().unwrap();
    let (file, hash) = content
        .iter()
        .find_map(|c| {
            let file = c["file"].as_str()?;
            let hash = c["hash"].as_str()?;
            (file == "bin/index.js").then(|| (file.to_string(), hash.to_string()))
        })
        .or_else(|| {
            content.first().and_then(|c| {
                Some((
                    c["file"].as_str()?.to_string(),
                    c["hash"].as_str()?.to_string(),
                ))
            })
        })
        .expect("scene entity must list content files");
    let tunneled = reqwest::get(format!("{public}/content/contents/{hash}"))
        .await
        .unwrap();
    assert_eq!(tunneled.status(), 200);
    let tunneled_bytes = tunneled.bytes().await.unwrap();
    assert!(!tunneled_bytes.is_empty());
    if let Some(local) = &local {
        let direct_bytes = reqwest::get(format!(
            "{}/content/contents/{hash}",
            local.trim_end_matches('/')
        ))
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
        assert_eq!(
            tunneled_bytes, direct_bytes,
            "content bytes must be identical through the tunnel"
        );
    }
    println!(
        "PASS content fetch through tunnel: {file} ({} bytes)",
        tunneled_bytes.len()
    );

    if let Some(touch) = touch {
        let (mut reload_ws, _) = tokio_tungstenite::connect_async(format!("{}/", ws_base(&public)))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        let previous = std::fs::read_to_string(&touch).unwrap();
        std::fs::write(&touch, format!("{previous}\n")).unwrap();
        let mut got_text = false;
        let mut got_binary = false;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        while !(got_text && got_binary) {
            let remaining = deadline
                .checked_duration_since(tokio::time::Instant::now())
                .expect("timed out waiting for scene-update frames through the tunnel");
            let msg = tokio::time::timeout(remaining, reload_ws.next())
                .await
                .expect("timed out waiting for scene-update frames through the tunnel")
                .expect("scene-update ws ended")
                .expect("scene-update ws error");
            match msg {
                Message::Text(text) if text.contains("SCENE_UPDATE") => got_text = true,
                Message::Binary(bytes) if !bytes.is_empty() => got_binary = true,
                _ => {}
            }
        }
        println!(
            "PASS scene-update dual frame (JSON + protobuf) through tunnel after editing {touch}"
        );
    } else {
        println!("SKIP scene-update push (DCL1_TUNNEL_TOUCH_FILE not set)");
    }

    let signer_tunneled = common::random_wallet();
    let mut client_tunneled =
        connect_ws(&format!("{}/mini-comms/room-1", ws_base(&public)), "rfc5").await;
    let (alias_t, _) = handshake(&mut client_tunneled, &signer_tunneled).await;
    println!(
        "PASS rfc5 handshake through tunnel: alias={alias_t} address={}",
        wallet_address(&signer_tunneled)
    );

    if let Some(local) = &local {
        let signer_direct = common::random_wallet();
        let mut client_direct = connect_ws(
            &format!("{}/mini-comms/room-1", ws_base(local.trim_end_matches('/'))),
            "rfc5",
        )
        .await;
        let (alias_d, peers_d) = handshake(&mut client_direct, &signer_direct).await;
        assert_eq!(
            peers_d.get(&alias_t).map(String::as_str),
            Some(wallet_address(&signer_tunneled).to_lowercase().as_str())
        );
        match recv_message(&mut client_tunneled).await {
            ws_packet::Message::PeerJoinMessage(join) => assert_eq!(join.alias, alias_d),
            other => panic!("expected peerJoinMessage, got {other:?}"),
        }
        send_packet(
            &mut client_tunneled,
            ws_packet::Message::PeerUpdateMessage(WsPeerUpdate {
                from_alias: 0,
                body: b"live tunnel probe".to_vec(),
                unreliable: false,
            }),
        )
        .await;
        match recv_message(&mut client_direct).await {
            ws_packet::Message::PeerUpdateMessage(update) => {
                assert_eq!(update.from_alias, alias_t);
                assert_eq!(update.body, b"live tunnel probe".to_vec());
            }
            other => panic!("expected the tunneled peerUpdate, got {other:?}"),
        }
        println!("PASS comms update tunneled-peer -> direct-peer (alias {alias_t} -> {alias_d})");
    } else {
        println!("SKIP second comms peer (DCL1_TUNNEL_LOCAL_URL not set)");
    }
}
