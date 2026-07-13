use super::{
    forwarded_host, forwarded_prefix, forwarded_proto, lock_cache, AppState, ENTITY_CACHE_TTL,
};
use crate::deploy::collect_publishable_files;
use crate::live_reload::ReloadFrame;
use crate::scene::{b64_hash, b64_unhash, Project};
use axum::{
    extract::{ws::Message, Path as AxPath, RawQuery, State, WebSocketUpgrade},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
    Json,
};
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;

pub(super) async fn root(State(st): State<Arc<AppState>>, req: axum::extract::Request) -> Response {
    let is_ws = req
        .headers()
        .get(header::UPGRADE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("websocket"))
        .unwrap_or(false);
    if !is_ws {
        let prefix = forwarded_prefix(req.headers());
        return Redirect::temporary(&format!("{prefix}/about")).into_response();
    }
    let (mut parts, _body) = req.into_parts();
    match <WebSocketUpgrade as axum::extract::FromRequestParts<()>>::from_request_parts(
        &mut parts,
        &(),
    )
    .await
    {
        Ok(upgrade) => upgrade.on_upgrade(move |socket| handle_ws(socket, st)),
        Err(e) => e.into_response(),
    }
}

async fn handle_ws(socket: axum::extract::ws::WebSocket, st: Arc<AppState>) {
    let mut rx = st.reload_tx.subscribe();
    let (mut sink, mut stream) = socket.split();
    tracing::info!("scene-update websocket client connected");
    loop {
        tokio::select! {
            msg = rx.recv() => match msg {
                Ok(frame) => {
                    let message = match frame {
                        ReloadFrame::Text(text) => Message::Text(text.into()),
                        ReloadFrame::Binary(bytes) => Message::Binary(bytes.into()),
                    };
                    if sink.send(message).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            },
            incoming = stream.next() => match incoming {
                Some(Ok(_)) => continue,
                _ => break,
            },
        }
    }
    tracing::info!("scene-update websocket client disconnected");
}

pub(super) async fn about(
    State(st): State<Arc<AppState>>,
    req: axum::extract::Request,
) -> Json<Value> {
    let headers = req.headers();
    let host = forwarded_host(headers).unwrap_or_else(|| {
        headers
            .get(header::HOST)
            .and_then(|h| h.to_str().ok())
            .unwrap_or("127.0.0.1")
            .to_string()
    });
    let proto = forwarded_proto(headers);
    let ws_proto = if proto == "https" { "wss" } else { "ws" };
    let prefix = forwarded_prefix(headers);
    let fixed_adapter = if st.offline_comms {
        "offline:offline".to_string()
    } else {
        format!("ws-room:{ws_proto}://{host}{prefix}/mini-comms/room-1")
    };
    let parcels: Vec<String> = st.projects.iter().flat_map(|p| p.parcels()).collect();
    let scenes_urn: Vec<String> = st
        .projects
        .iter()
        .map(|p| {
            format!(
                "urn:decentraland:entity:{}?=&baseUrl={proto}://{host}{prefix}/content/contents/",
                scene_id_for(p, &st.machine)
            )
        })
        .collect();
    Json(json!({
        "acceptingUsers": true,
        "bff": { "healthy": false, "publicUrl": host },
        "comms": {
            "healthy": true,
            "protocol": "v3",
            "fixedAdapter": fixed_adapter
        },
        "configurations": {
            "networkId": 0,
            "globalScenesUrn": [],
            "localSceneParcels": parcels,
            "scenesUrn": scenes_urn,
            "realmName": "LocalPreview"
        },
        "content": { "healthy": true, "publicUrl": format!("{proto}://{host}{prefix}/content") },
        "lambdas": { "healthy": true, "publicUrl": format!("{proto}://{host}{prefix}/lambdas") },
        "healthy": true
    }))
}

pub(super) async fn scenes() -> Json<Value> {
    Json(json!({ "scenes": [], "total": 0 }))
}

pub(super) async fn contents(
    method: axum::http::Method,
    State(st): State<Arc<AppState>>,
    AxPath(hash): AxPath<String>,
    headers: HeaderMap,
) -> Response {
    let Some(path_str) = b64_unhash(&hash, &st.machine) else {
        return (StatusCode::NOT_FOUND, "unknown hash format").into_response();
    };
    let path = PathBuf::from(&path_str);
    let Ok(canonical) = dunce::canonicalize(&path) else {
        return (StatusCode::NOT_FOUND, "file not found").into_response();
    };
    let Some(project) = project_for(&st, &canonical) else {
        return (StatusCode::FORBIDDEN, "outside project root").into_response();
    };
    if canonical == project.root {
        tracing::info!(target: "access", "contents <scene-entity-json> 200");
        return Json(scene_entity(&st, project)).into_response();
    }
    if !is_published_hash(&st, project, &hash) {
        tracing::info!(target: "access", "contents {hash} 404 not-published");
        return (StatusCode::NOT_FOUND, "not a published content file").into_response();
    }
    let rel = canonical
        .strip_prefix(&project.root)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| canonical.display().to_string());
    let Ok(file) = tokio::fs::File::open(&canonical).await else {
        return (StatusCode::NOT_FOUND, "file not found").into_response();
    };
    let Ok(meta) = file.metadata().await else {
        return (StatusCode::NOT_FOUND, "file not found").into_response();
    };
    let etag = file_etag(&meta);
    let if_none_match = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if if_none_match == etag {
        tracing::info!(target: "access", "contents {rel} 304 etag={etag} sent=0");
        return (
            StatusCode::NOT_MODIFIED,
            [
                (header::ETAG, etag),
                (header::CACHE_CONTROL, "no-cache".to_string()),
            ],
        )
            .into_response();
    }
    let len = meta.len();
    let response_headers = [
        (header::CONTENT_TYPE, mime_for(&canonical).to_string()),
        (header::CONTENT_LENGTH, len.to_string()),
        (header::ETAG, etag.clone()),
        (header::CACHE_CONTROL, "no-cache".to_string()),
    ];
    if method == axum::http::Method::HEAD {
        tracing::info!(target: "access", "contents {rel} 200 etag={etag} sent=0");
        return (response_headers, axum::body::Body::empty()).into_response();
    }
    tracing::info!(target: "access", "contents {rel} 200 etag={etag} sent={len}");
    let stream = futures::stream::unfold(file, |mut file| async move {
        use tokio::io::AsyncReadExt;
        let mut buf = vec![0u8; 64 * 1024];
        match file.read(&mut buf).await {
            Ok(0) => None,
            Ok(n) => {
                buf.truncate(n);
                Some((Ok::<Vec<u8>, std::io::Error>(buf), file))
            }
            Err(e) => Some((Err(e), file)),
        }
    });
    (response_headers, axum::body::Body::from_stream(stream)).into_response()
}

fn file_etag(meta: &std::fs::Metadata) -> String {
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .unwrap_or_default();
    format!(
        "\"{:x}-{:x}.{:x}\"",
        meta.len(),
        mtime.as_secs(),
        mtime.subsec_nanos()
    )
}

fn mime_for(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "js" => "application/javascript",
        "json" | "composite" => "application/json",
        "glb" => "model/gltf-binary",
        "gltf" => "model/gltf+json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "mp4" => "video/mp4",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

fn is_published_hash(st: &AppState, project: &Project, hash: &str) -> bool {
    scene_entity(st, project)
        .get("content")
        .and_then(|c| c.as_array())
        .is_some_and(|arr| {
            arr.iter()
                .any(|e| e.get("hash").and_then(|h| h.as_str()) == Some(hash))
        })
}

fn scene_entity(st: &AppState, project: &Project) -> Value {
    if let Some((at, cached)) = lock_cache(st).get(&project.root) {
        if at.elapsed() < ENTITY_CACHE_TTL {
            return cached.clone();
        }
    }
    let entity = build_scene_entity(project, &st.machine);
    lock_cache(st).insert(project.root.clone(), (Instant::now(), entity.clone()));
    entity
}

pub(super) fn build_scene_entity(project: &Project, machine: &str) -> Value {
    let root = &project.root;
    let rels = match collect_publishable_files(root) {
        Ok(rels) => rels,
        Err(e) => {
            tracing::warn!(
                "collecting scene files under {} failed ({e:#}); serving an empty scene entity",
                root.display()
            );
            Vec::new()
        }
    };
    let content: Vec<Value> = rels
        .iter()
        .map(|rel| {
            let abs = root.join(rel).display().to_string();
            json!({ "file": rel, "hash": b64_hash(&abs, machine) })
        })
        .collect();
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    json!({
        "version": "v3",
        "type": "scene",
        "id": scene_id_for(project, machine),
        "pointers": project.parcels(),
        "timestamp": ts,
        "content": content,
        "metadata": project.scene_json,
    })
}

pub(super) fn scene_id_for(project: &Project, machine: &str) -> String {
    b64_hash(&project.root.display().to_string(), machine)
}

pub(super) fn project_for<'a>(
    st: &'a AppState,
    canonical: &std::path::Path,
) -> Option<&'a Project> {
    st.projects
        .iter()
        .filter(|p| canonical.starts_with(&p.root))
        .max_by_key(|p| p.root.components().count())
}

pub(super) fn entities_for(st: &AppState, pointers: &[String]) -> Vec<Value> {
    let entities: Vec<Value> = st.projects.iter().map(|p| scene_entity(st, p)).collect();
    if pointers.is_empty() {
        return entities;
    }
    entities
        .into_iter()
        .filter(|e| {
            e.get("pointers")
                .and_then(|p| p.as_array())
                .is_some_and(|arr| {
                    arr.iter()
                        .any(|v| v.as_str().is_some_and(|s| pointers.iter().any(|q| q == s)))
                })
        })
        .collect()
}

pub(super) async fn entities_active(
    State(st): State<Arc<AppState>>,
    body: Option<Json<Value>>,
) -> Json<Value> {
    let pointers: Vec<String> = body
        .as_ref()
        .and_then(|b| b.0.get("pointers"))
        .and_then(|p| p.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    Json(Value::Array(entities_for(&st, &pointers)))
}

pub(super) async fn entities_scene(
    State(st): State<Arc<AppState>>,
    RawQuery(query): RawQuery,
) -> Json<Value> {
    let pointers: Vec<String> = query
        .map(|q| {
            url::form_urlencoded::parse(q.as_bytes())
                .filter(|(k, _)| k == "pointer")
                .map(|(_, v)| v.into_owned())
                .collect()
        })
        .unwrap_or_default();
    Json(Value::Array(entities_for(&st, &pointers)))
}
