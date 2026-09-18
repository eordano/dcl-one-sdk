use super::http::preview_ws_origin;
use super::{data_layer_origin_allowed, forwarded_prefix, AppState};
use crate::data_layer;
use crate::joinblock;
use crate::netinfo;
use axum::{
    extract::{ws::Message, Path as AxPath, Request, State, WebSocketUpgrade},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Redirect, Response},
    Json,
};
use futures::{SinkExt, StreamExt};
use serde_json::json;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) async fn mobile_preview(State(st): State<Arc<AppState>>) -> Response {
    let ifaces = netinfo::enumerate();
    let Some(ip) = netinfo::share_ip(&ifaces) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "ok": false, "error": "No LAN IP address found" })),
        )
            .into_response();
    };
    let base = st
        .first_project()
        .map(|p| joinblock::base_coords(&p.scene_json))
        .unwrap_or((0, 0));
    let url = format!(
        "decentraland://open?preview=http://{ip}:{}&position={},{}",
        st.port, base.0, base.1
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

/// The default state: the blob ships the data-layer host and not the 18 MB
/// editor UI, so `/data-layer` works and only `/inspector/*` does not.
fn editor_ui_missing() -> Response {
    (
        StatusCode::NOT_FOUND,
        "the data layer is running on /data-layer, but the editor UI (@dcl/inspector) \
         is not installed \u{2014} npm install --save-dev @dcl/inspector, or set \
         DCL_ONE_INSPECTOR_DIR=<path-to-an-@dcl/inspector-package>",
    )
        .into_response()
}

fn ui_dir(st: &AppState) -> Result<&Path, Response> {
    let dl = st.data_layer.as_ref().ok_or_else(editor_disabled)?;
    dl.public_dir.as_deref().ok_or_else(editor_ui_missing)
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

/// `W/"<mtime>-<len><suffix>"`: what a `no-cache` reload revalidates against,
/// so an unchanged bundle costs a stat, not an 18 MB read.
fn weak_etag(md: &std::fs::Metadata, suffix: &str) -> Option<HeaderValue> {
    let modified = md.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    HeaderValue::from_str(&format!(
        "W/\"{:x}-{:x}{suffix}\"",
        modified.as_nanos(),
        md.len()
    ))
    .ok()
}

fn not_modified(headers: &HeaderMap, etag: &HeaderValue) -> bool {
    let Ok(etag) = etag.to_str() else {
        return false;
    };
    headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|t| t.trim() == etag || t.trim() == "*"))
}

fn revalidated(etag: HeaderValue, vary: bool) -> Response {
    let mut out = HeaderMap::new();
    out.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    out.insert(header::ETAG, etag);
    if vary {
        out.insert(header::VARY, HeaderValue::from_static("accept-encoding"));
    }
    (StatusCode::NOT_MODIFIED, out).into_response()
}

type IndexMemo = Option<(PathBuf, SystemTime, u64, Arc<String>)>;

/// The inspector's index.html, re-read only when its mtime or length moves.
async fn index_html(index: &Path) -> Option<(Arc<String>, Option<HeaderValue>)> {
    static MEMO: Mutex<IndexMemo> = Mutex::new(None);
    let md = tokio::fs::metadata(index).await.ok()?;
    let modified = md.modified().ok()?;
    let etag = weak_etag(&md, "");
    let remembered = MEMO
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .filter(|(p, m, l, _)| p == index && *m == modified && *l == md.len())
        .map(|(_, _, _, html)| html.clone());
    if let Some(html) = remembered {
        return Some((html, etag));
    }
    let html = Arc::new(tokio::fs::read_to_string(index).await.ok()?);
    *MEMO.lock().unwrap_or_else(PoisonError::into_inner) =
        Some((index.to_path_buf(), modified, md.len(), html.clone()));
    Some((html, etag))
}

pub(super) async fn inspector_index(
    State(st): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let public_dir = match ui_dir(&st) {
        Ok(dir) => dir,
        Err(resp) => return resp,
    };
    let index = public_dir.join("index.html");
    let Some((html, etag)) = index_html(&index).await else {
        return (
            StatusCode::NOT_FOUND,
            "the inspector build has no index.html",
        )
            .into_response();
    };
    let ws_url = format!("{}/data-layer", preview_ws_origin(&headers));
    // The injected config rides the tag: a different origin is a different page.
    let etag = etag.and_then(|tag| {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        ws_url.hash(&mut h);
        let tag = tag.to_str().ok()?.trim_end_matches('"').to_string();
        HeaderValue::from_str(&format!("{tag}-{:x}\"", h.finish())).ok()
    });
    let mut out = HeaderMap::new();
    out.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    out.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    if let Some(etag) = etag {
        if not_modified(&headers, &etag) {
            return revalidated(etag, false);
        }
        out.insert(header::ETAG, etag);
    }
    let config = data_layer::inspector_config_json(&ws_url);
    let body = data_layer::inject_config(&html, &config);
    (out, body).into_response()
}

pub(super) async fn inspector_asset(
    State(st): State<Arc<AppState>>,
    AxPath(path): AxPath<String>,
    headers: HeaderMap,
) -> Response {
    let public_dir = match ui_dir(&st) {
        Ok(dir) => dir,
        Err(resp) => return resp,
    };
    if path.is_empty() || path == "index.html" {
        return inspector_index(State(st.clone()), headers).await;
    }
    let Some(asset) = data_layer::resolve_asset(public_dir, &path) else {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    };
    let (full, stored_gzipped) = match asset {
        data_layer::Asset::Plain(p) => (p, false),
        data_layer::Asset::Gzipped(p) => (p, true),
    };
    let accepted = stored_gzipped
        && data_layer::accepts_gzip(
            headers
                .get(header::ACCEPT_ENCODING)
                .and_then(|v| v.to_str().ok()),
        );
    let Ok(md) = tokio::fs::metadata(&full).await else {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    };
    let etag = weak_etag(
        &md,
        if stored_gzipped && !accepted {
            "-plain"
        } else {
            ""
        },
    );
    let mut out = HeaderMap::new();
    out.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(data_layer::inspector_mime(Path::new(&path))),
    );
    out.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    if stored_gzipped {
        out.insert(header::VARY, HeaderValue::from_static("accept-encoding"));
    }
    if let Some(etag) = etag {
        if not_modified(&headers, &etag) {
            return revalidated(etag, stored_gzipped);
        }
        out.insert(header::ETAG, etag);
    }
    let Ok(mut bytes) = tokio::fs::read(&full).await else {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    };
    if accepted {
        out.insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
    } else if stored_gzipped {
        match data_layer::gunzip(&bytes) {
            Ok(plain) => bytes = plain,
            Err(e) => {
                tracing::warn!("could not decompress {}: {e}", full.display());
                return (StatusCode::INTERNAL_SERVER_ERROR, "asset unreadable").into_response();
            }
        }
    }
    (out, bytes).into_response()
}
