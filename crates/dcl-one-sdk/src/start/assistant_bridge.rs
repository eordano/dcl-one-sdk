use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    http::StatusCode,
    response::Response,
};
use futures::{SinkExt, StreamExt};
use serde_json::Value;
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message as UpstreamMessage;

pub(super) fn configured() -> Option<(String, String)> {
    let mut url = url::Url::parse(&super::assistant::mcp_url()?).ok()?;
    let token = std::env::var("DCL_SCENE_MCP_TOKEN")
        .ok()
        .filter(|token| !token.is_empty())?;
    let prefix = url.path().strip_suffix("/mcp")?.to_string();
    url.set_path(&format!("{prefix}/bridge"));
    url.set_scheme(if url.scheme() == "https" { "wss" } else { "ws" })
        .ok()?;
    Some((url.into(), token))
}

pub(super) async fn upgrade(ws: WebSocketUpgrade) -> Result<Response, (StatusCode, &'static str)> {
    let (url, token) = configured().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Configure a local scene MCP URL and pairing token first",
    ))?;
    Ok(ws
        .max_message_size(32 * 1024 * 1024)
        .on_upgrade(move |client| proxy(client, url, token)))
}

fn hello(text: &str, token: &str) -> Option<String> {
    let mut value: Value = serde_json::from_str(text).ok()?;
    if value["kind"] != "hello" {
        return None;
    }
    value["token"] = Value::String(token.to_string());
    serde_json::to_string(&value).ok()
}

async fn proxy(mut client: WebSocket, url: String, token: String) {
    let first = tokio::time::timeout(Duration::from_secs(5), client.next()).await;
    let Ok(Some(Ok(Message::Text(text)))) = first else {
        let _ = client.close().await;
        return;
    };
    let Some(first) = hello(&text, &token) else {
        let _ = client.close().await;
        return;
    };
    let connected = tokio::time::timeout(
        Duration::from_secs(5),
        tokio_tungstenite::connect_async(&url),
    )
    .await;
    let Ok(Ok((mut upstream, _))) = connected else {
        let _ = client.close().await;
        return;
    };
    if upstream
        .send(UpstreamMessage::Text(first.into()))
        .await
        .is_err()
    {
        let _ = client.close().await;
        return;
    }
    let (mut client_tx, mut client_rx) = client.split();
    let (mut upstream_tx, mut upstream_rx) = upstream.split();
    let to_upstream = async {
        while let Some(Ok(message)) = client_rx.next().await {
            let message = match message {
                Message::Text(text) => UpstreamMessage::Text(text.to_string().into()),
                Message::Binary(bytes) => UpstreamMessage::Binary(bytes),
                Message::Ping(bytes) => UpstreamMessage::Ping(bytes),
                Message::Pong(bytes) => UpstreamMessage::Pong(bytes),
                Message::Close(_) => break,
            };
            if upstream_tx.send(message).await.is_err() {
                break;
            }
        }
        let _ = upstream_tx.close().await;
    };
    let to_client = async {
        while let Some(Ok(message)) = upstream_rx.next().await {
            let message = match message {
                UpstreamMessage::Text(text) => Message::Text(text.to_string().into()),
                UpstreamMessage::Binary(bytes) => Message::Binary(bytes),
                UpstreamMessage::Ping(bytes) => Message::Ping(bytes),
                UpstreamMessage::Pong(bytes) => Message::Pong(bytes),
                UpstreamMessage::Close(_) => break,
                _ => continue,
            };
            if client_tx.send(message).await.is_err() {
                break;
            }
        }
        let _ = client_tx.close().await;
    };
    tokio::select! {_=to_upstream=>{},_=to_client=>{}}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replaces_only_the_hello_token_and_keeps_editor_identity() {
        let input = r#"{"kind":"hello","token":"sdk-project","bridgeVersion":4,"page":{"project":"sdk-scene"},"sceneReady":true,"playing":false}"#;
        let replaced = hello(input, "server-side-pairing-token").unwrap();
        let output: Value = serde_json::from_str(&replaced).unwrap();
        assert_eq!(output["token"], "server-side-pairing-token");
        assert_eq!(output["page"]["project"], "sdk-scene");
        assert_eq!(output["sceneReady"], true);
        assert!(hello(r#"{"kind":"bus","token":"sdk-project"}"#, "secret").is_none());
    }
    #[tokio::test]
    async fn websocket_proxy_pairs_existing_relay_without_exposing_token() {
        use axum::{routing::get, Router};
        use std::sync::Arc;
        let (hello_tx, mut hello_rx) = tokio::sync::mpsc::unbounded_channel();
        let relay = Router::new().route(
            "/bridge",
            get(move |ws: WebSocketUpgrade| {
                let hello_tx = hello_tx.clone();
                async move {
                    ws.on_upgrade(move |mut socket| async move {
                        let Some(Ok(Message::Text(first))) = socket.next().await else {
                            panic!("missing hello");
                        };
                        hello_tx.send(first.to_string()).unwrap();
                        socket
                            .send(Message::Text(
                                r#"{"kind":"hello-ok","serverVersion":"test"}"#.into(),
                            ))
                            .await
                            .unwrap();
                        if let Some(Ok(message)) = socket.next().await {
                            socket.send(message).await.unwrap();
                        }
                    })
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream = Arc::new(format!("ws://{}/bridge", listener.local_addr().unwrap()));
        let relay_server = tokio::spawn(async move {
            axum::serve(listener, relay).await.unwrap();
        });
        let app = Router::new().route(
            "/proxy",
            get(move |ws: WebSocketUpgrade| {
                let upstream = upstream.clone();
                async move {
                    ws.on_upgrade(move |client| {
                        proxy(client, upstream.to_string(), "only-sdk-knows-this".into())
                    })
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}/proxy", listener.local_addr().unwrap());
        let sdk_server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let (mut browser, _) = tokio_tungstenite::connect_async(url).await.unwrap();
        browser
            .send(UpstreamMessage::Text(
                r#"{"kind":"hello","token":"sdk-project","page":{"project":"local-scene"}}"#.into(),
            ))
            .await
            .unwrap();
        let upstream_hello: Value = serde_json::from_str(&hello_rx.recv().await.unwrap()).unwrap();
        assert_eq!(upstream_hello["token"], "only-sdk-knows-this");
        let response = browser.next().await.unwrap().unwrap().into_text().unwrap();
        assert!(response.contains("hello-ok"));
        assert!(!response.contains("only-sdk-knows-this"));
        let message = r#"{"kind":"event","message":{"type":"scene-ready"}}"#;
        browser
            .send(UpstreamMessage::Text(message.into()))
            .await
            .unwrap();
        assert_eq!(
            browser.next().await.unwrap().unwrap().into_text().unwrap(),
            message
        );
        browser.close(None).await.unwrap();
        relay_server.abort();
        sdk_server.abort();
    }
}
