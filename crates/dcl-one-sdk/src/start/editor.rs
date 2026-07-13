use super::{
    data_layer_origin_allowed, forwarded_host, forwarded_prefix, forwarded_proto, AppState,
};
use crate::data_layer;
use crate::joinblock;
use crate::netinfo;
use axum::{
    extract::{ws::Message, Path as AxPath, Request, State, WebSocketUpgrade},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
    Json,
};
use futures::{SinkExt, StreamExt};
use serde_json::json;
use std::sync::Arc;

pub(super) async fn mobile_preview(State(st): State<Arc<AppState>>) -> Response {
    let ifaces = netinfo::enumerate();
    let Some(ip) = netinfo::share_ip(&ifaces) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "ok": false, "error": "No LAN IP address found" })),
        )
            .into_response();
    };
    let url = format!(
        "decentraland://open?preview=http://{ip}:{}&position={}%2C{}",
        st.port, st.base.0, st.base.1
    );
    match joinblock::qr_svg_data_url(&url) {
        Some(qr) => Json(json!({ "ok": true, "data": { "url": url, "qr": qr } })).into_response(),
        None => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": "QR generation failed" })),
        )
            .into_response(),
    }
}

fn editor_disabled() -> Response {
    (
        StatusCode::NOT_FOUND,
        "the visual editor is off \u{2014} restart with: dcl-one-sdk start --data-layer",
    )
        .into_response()
}

pub(super) async fn data_layer_ws(State(st): State<Arc<AppState>>, req: Request) -> Response {
    let Some(dl) = st.data_layer.clone() else {
        return editor_disabled();
    };
    let port = *dl.port_rx.borrow();
    if port == 0 {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "the data layer is restarting \u{2014} retry in a moment",
        )
            .into_response();
    }
    if !data_layer_origin_allowed(req.headers()) {
        tracing::warn!("data-layer upgrade refused: cross-origin request");
        return (
            StatusCode::FORBIDDEN,
            "cross-origin websocket rejected \u{2014} set DCL_ONE_SDK_ALLOWED_ORIGINS to permit it",
        )
            .into_response();
    }
    let (mut parts, _body) = req.into_parts();
    match <WebSocketUpgrade as axum::extract::FromRequestParts<()>>::from_request_parts(
        &mut parts,
        &(),
    )
    .await
    {
        Ok(upgrade) => upgrade.on_upgrade(move |socket| proxy_data_layer(socket, port)),
        Err(e) => e.into_response(),
    }
}

async fn proxy_data_layer(client: axum::extract::ws::WebSocket, port: u16) {
    use tokio_tungstenite::tungstenite::Message as TgMessage;
    let url = format!("ws://127.0.0.1:{port}/");
    let upstream = match tokio_tungstenite::connect_async(&url).await {
        Ok((socket, _)) => socket,
        Err(e) => {
            tracing::warn!("data-layer upstream connect failed: {e}");
            return;
        }
    };
    tracing::info!("data-layer client connected");
    let (mut client_tx, mut client_rx) = client.split();
    let (mut up_tx, mut up_rx) = upstream.split();
    let to_upstream = async {
        while let Some(Ok(msg)) = client_rx.next().await {
            let out = match msg {
                Message::Binary(bytes) => TgMessage::Binary(bytes),
                Message::Close(_) => TgMessage::Close(None),
                _ => continue,
            };
            let closing = matches!(out, TgMessage::Close(_));
            if up_tx.send(out).await.is_err() || closing {
                break;
            }
        }
    };
    let to_client = async {
        while let Some(Ok(msg)) = up_rx.next().await {
            let out = match msg {
                TgMessage::Binary(bytes) => Message::Binary(bytes),
                TgMessage::Close(_) => Message::Close(None),
                _ => continue,
            };
            let closing = matches!(out, Message::Close(_));
            if client_tx.send(out).await.is_err() || closing {
                break;
            }
        }
    };
    tokio::select! {
        _ = to_upstream => {}
        _ = to_client => {}
    }
    tracing::info!("data-layer client disconnected");
}

pub(super) async fn inspector_redirect(headers: HeaderMap) -> Response {
    let prefix = forwarded_prefix(&headers);
    Redirect::permanent(&format!("{prefix}/inspector/")).into_response()
}

fn editor_ws_url(headers: &HeaderMap) -> String {
    let host = forwarded_host(headers).unwrap_or_else(|| {
        headers
            .get(header::HOST)
            .and_then(|h| h.to_str().ok())
            .unwrap_or("127.0.0.1")
            .to_string()
    });
    let ws_proto = if forwarded_proto(headers) == "https" {
        "wss"
    } else {
        "ws"
    };
    let prefix = forwarded_prefix(headers);
    format!("{ws_proto}://{host}{prefix}/data-layer")
}

pub(super) async fn inspector_index(
    State(st): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let Some(dl) = &st.data_layer else {
        return editor_disabled();
    };
    let index = dl.public_dir.join("index.html");
    let html = match tokio::fs::read_to_string(&index).await {
        Ok(html) => html,
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                "the inspector build has no index.html",
            )
                .into_response()
        }
    };
    let config = data_layer::inspector_config_json(&editor_ws_url(&headers));
    let body = data_layer::inject_config(&html, &config);
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8".to_string()),
            (header::CACHE_CONTROL, "no-cache".to_string()),
        ],
        body,
    )
        .into_response()
}

pub(super) async fn inspector_asset(
    State(st): State<Arc<AppState>>,
    AxPath(path): AxPath<String>,
    headers: HeaderMap,
) -> Response {
    let Some(dl) = &st.data_layer else {
        return editor_disabled();
    };
    if path.split('/').any(|seg| seg == "..") {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    if path.is_empty() || path == "index.html" {
        return inspector_index(State(st.clone()), headers).await;
    }
    let base = dunce::canonicalize(&dl.public_dir).unwrap_or_else(|_| dl.public_dir.clone());
    let Ok(full) = dunce::canonicalize(dl.public_dir.join(&path)) else {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    };
    if !full.starts_with(&base) {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    match tokio::fs::read(&full).await {
        Ok(bytes) => (
            [
                (
                    header::CONTENT_TYPE,
                    data_layer::inspector_mime(&full).to_string(),
                ),
                (header::CACHE_CONTROL, "no-cache".to_string()),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}
