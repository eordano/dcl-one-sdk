use super::{
    forwarded_host, forwarded_prefix, forwarded_proto, lock_cache, AppState, ENTITY_CACHE_TTL,
};
use crate::deploy::collect_publishable_files;
use crate::live_reload::ReloadFrame;
use crate::scene::{b64_content_hash, b64_hash, b64_unhash, Project};
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
        let accepts_html = req
            .headers()
            .get(header::ACCEPT)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|a| a.contains("text/html"));
        if accepts_html {
            return super::landing::page(&st, req.headers());
        }
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

pub(super) fn preview_host(headers: &HeaderMap) -> String {
    forwarded_host(headers).unwrap_or_else(|| {
        headers
            .get(header::HOST)
            .and_then(|h| h.to_str().ok())
            .unwrap_or("127.0.0.1")
            .to_string()
    })
}

/// `scheme://host[/prefix]` as the client reached us, reverse proxy included.
/// Everything this server advertises about itself has to be built on it.
pub(super) fn preview_origin(headers: &HeaderMap) -> String {
    format!(
        "{}://{}{}",
        forwarded_proto(headers),
        preview_host(headers),
        forwarded_prefix(headers)
    )
}

/// Where the catalyst back-fill and the world mirror keep fetched content.
pub(super) fn contents_cache_dir(st: &AppState) -> Option<PathBuf> {
    st.projects
        .first()
        .map(|p| p.root.join(".dcl-cache").join("contents"))
}

pub(super) async fn about(
    State(st): State<Arc<AppState>>,
    req: axum::extract::Request,
) -> Json<Value> {
    let headers = req.headers();
    let host = preview_host(headers);
    let ws_proto = if forwarded_proto(headers) == "https" {
        "wss"
    } else {
        "ws"
    };
    let prefix = forwarded_prefix(headers);
    let origin = preview_origin(headers);
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
                "urn:decentraland:entity:{}?=&baseUrl={origin}/content/contents/",
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
        "content": { "healthy": true, "publicUrl": format!("{origin}/content") },
        "lambdas": { "healthy": true, "publicUrl": format!("{origin}/lambdas") },
        "healthy": true
    }))
}

pub(super) async fn scenes() -> Json<Value> {
    Json(json!({ "scenes": [], "total": 0 }))
}

/// Upstream serves the first project's scene.json off disk (sdk-commands
/// `endpoints.js`); the in-memory copy is the same document and stays correct
/// across a composite rebuild.
pub(super) async fn scene_json(State(st): State<Arc<AppState>>) -> Response {
    match st.projects.first() {
        Some(p) => Json(p.scene_json.clone()).into_response(),
        None => (StatusCode::NOT_FOUND, "no scene loaded").into_response(),
    }
}

/// Upstream hardcodes `https://feature-flags.decentraland.zone`; this toolchain
/// bakes in no third-party host, as with `proxy::WORLD_BASE_ENV`.
pub(super) const FEATURE_FLAGS_ENV: &str = "DCL_ONE_SDK_FEATURE_FLAGS";

/// `/feature-flags/{file}` — upstream proxies this so a browser page served from
/// the preview origin is not CORS-blocked fetching flags.
pub(super) async fn feature_flags(AxPath(file): AxPath<String>) -> Response {
    if file.contains('/') || file.contains('\\') || file.contains("..") {
        return (StatusCode::BAD_REQUEST, "bad feature-flag file").into_response();
    }
    let Some(base) = super::proxy::configured_base(&[FEATURE_FLAGS_ENV]) else {
        return (
            StatusCode::NOT_IMPLEMENTED,
            super::proxy::unconfigured_host_hint(
                "feature-flag",
                FEATURE_FLAGS_ENV,
                "/<file> (upstream uses https://feature-flags.decentraland.zone)",
            ),
        )
            .into_response();
    };
    super::proxy::passthrough(axum::http::Method::GET, &format!("{base}/{file}")).await
}

/// `/preview-wearables` — the smart-wearable manifests in this workspace, with
/// content URLs rebased onto the preview origin. Upstream marks it deprecated in
/// favour of `/content/entities/active`; it is here for older explorer builds.
pub(super) async fn preview_wearables(
    State(st): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Json<Value> {
    let base = format!("{}/content/contents", preview_origin(&headers));
    Json(json!({
        "ok": true,
        "data": collect_preview_wearables(&st.projects, &base, &st.machine),
    }))
}

/// One entry per project carrying a readable `wearable.json`. A plain scene
/// contributes nothing, which is why the route answers `{ok: true, data: []}`
/// rather than 404 — the same shape upstream returns. Hashes are the preview's
/// own reversible path hashes, so the URLs resolve through
/// `/content/contents/{hash}` exactly like scene files do.
fn collect_preview_wearables(projects: &[Project], base: &str, machine: &str) -> Vec<Value> {
    let mut out = Vec::new();
    for p in projects {
        let Ok(text) = std::fs::read_to_string(p.root.join("wearable.json")) else {
            continue;
        };
        let Ok(mut wearable) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let contents: Vec<Value> = collect_publishable_files(&p.root)
            .unwrap_or_default()
            .iter()
            .map(|rel| {
                let abs = p.root.join(rel).display().to_string();
                let hash = b64_content_hash(&abs, machine);
                json!({ "key": rel, "url": format!("{base}/{hash}"), "hash": hash })
            })
            .collect();
        if let Some(obj) = wearable.as_object_mut() {
            obj.insert("baseUrl".into(), json!(base));
            obj.insert("contents".into(), json!(contents));
        }
        out.push(wearable);
    }
    out
}

pub(super) async fn contents(
    method: axum::http::Method,
    State(st): State<Arc<AppState>>,
    AxPath(hash): AxPath<String>,
    headers: HeaderMap,
) -> Response {
    let Some(path_str) = b64_unhash(&hash, &st.machine) else {
        let cache_dir = contents_cache_dir(&st);
        return super::proxy::contents_upstream(method, &hash, &headers, cache_dir.as_deref())
            .await;
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
    let requested = crate::scene::hash_path_part(hash);
    scene_entity(st, project)
        .get("content")
        .and_then(|c| c.as_array())
        .is_some_and(|arr| {
            arr.iter().any(|e| {
                e.get("hash")
                    .and_then(|h| h.as_str())
                    .is_some_and(|h| crate::scene::hash_path_part(h) == requested)
            })
        })
}

/// The explorer's asset-bundle verdict, read off the entity as `status`
/// (`DCL.Ipfs.TrimmedEntityDefinitionBase.assetBundleRegistryEnum`). Upstream
/// sdk-commands sends no such field, so the client defaults the enum to
/// `complete` whether or not a bundle exists; the sidecar IS the registry for a
/// preview, so it can answer honestly. `fallback` is literal — the client falls
/// back to raw GLTFs.
fn ab_status(optimized_assets_url: &std::sync::OnceLock<String>) -> &'static str {
    match optimized_assets_url.get() {
        Some(_) => "complete",
        None => "fallback",
    }
}

fn scene_entity(st: &AppState, project: &Project) -> Value {
    let mut entity = scene_entity_cached(st, project);
    if let Some(obj) = entity.as_object_mut() {
        obj.insert("status".into(), json!(ab_status(&st.optimized_assets_url)));
    }
    entity
}

fn scene_entity_cached(st: &AppState, project: &Project) -> Value {
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
            json!({ "file": rel, "hash": b64_content_hash(&abs, machine) })
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
    let mut entities = entities_for(&st, &pointers);
    let missing = unmatched_pointers(&entities, &pointers);
    if !missing.is_empty() {
        entities.extend(super::proxy::entities_active_upstream(&missing).await);
    }
    Json(Value::Array(entities))
}

/// Requested pointers no local entity covers; these resolve upstream. Parcel
/// pointers never do — they would return the production scene at those
/// coordinates (Genesis Plaza) instead of the local preview.
fn unmatched_pointers(entities: &[Value], pointers: &[String]) -> Vec<String> {
    pointers
        .iter()
        .filter(|q| !is_parcel_pointer(q))
        .filter(|q| {
            !entities.iter().any(|e| {
                e.get("pointers")
                    .and_then(|p| p.as_array())
                    .is_some_and(|arr| {
                        arr.iter()
                            .any(|v| v.as_str().is_some_and(|s| s.eq_ignore_ascii_case(q)))
                    })
            })
        })
        .cloned()
        .collect()
}

fn is_parcel_pointer(p: &str) -> bool {
    let mut it = p.split(',');
    match (it.next(), it.next(), it.next()) {
        (Some(x), Some(y), None) => {
            x.trim().parse::<i32>().is_ok() && y.trim().parse::<i32>().is_ok()
        }
        _ => false,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    struct WearableTmp(PathBuf);

    impl WearableTmp {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "dcl-one-sdk-wearables-{tag}-{}-{:x}",
                std::process::id(),
                rand::random::<u64>()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            WearableTmp(dir)
        }
    }

    impl Drop for WearableTmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn project_at(root: PathBuf) -> Project {
        Project {
            root,
            scene_json: json!({ "main": "bin/index.js" }),
        }
    }

    #[test]
    fn ab_status_tells_the_truth_about_whether_bundles_are_served() {
        let url = std::sync::OnceLock::new();
        assert_eq!(ab_status(&url), "fallback");
        let _ = url.set("http://127.0.0.1:5147".to_string());
        assert_eq!(ab_status(&url), "complete");
    }

    #[test]
    fn a_scene_without_a_wearable_json_contributes_no_entries() {
        let tmp = WearableTmp::new("plain");
        std::fs::write(tmp.0.join("scene.json"), "{}").unwrap();
        let out = collect_preview_wearables(&[project_at(tmp.0.clone())], "http://x/c", "m");
        assert!(out.is_empty());
    }

    #[test]
    fn a_smart_wearable_is_listed_with_preview_resolvable_urls() {
        let tmp = WearableTmp::new("sw");
        std::fs::write(
            tmp.0.join("wearable.json"),
            r#"{"id":"urn:x","data":{"category":"eyewear"}}"#,
        )
        .unwrap();
        std::fs::write(tmp.0.join("model.glb"), b"glb").unwrap();
        let out = collect_preview_wearables(&[project_at(tmp.0.clone())], "http://x/c", "m");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["id"], "urn:x");
        assert_eq!(out[0]["baseUrl"], "http://x/c");
        let contents = out[0]["contents"].as_array().unwrap();
        let model = contents
            .iter()
            .find(|c| c["key"] == "model.glb")
            .expect("model.glb listed");
        let abs = tmp.0.join("model.glb").display().to_string();
        assert_eq!(model["hash"], json!(b64_content_hash(&abs, "m")));
        assert_eq!(
            model["url"],
            json!(format!("http://x/c/{}", b64_content_hash(&abs, "m")))
        );
    }

    #[test]
    fn a_malformed_wearable_json_is_skipped_not_fatal() {
        let tmp = WearableTmp::new("bad");
        std::fs::write(tmp.0.join("wearable.json"), "{not json").unwrap();
        assert!(
            collect_preview_wearables(&[project_at(tmp.0.clone())], "http://x/c", "m").is_empty()
        );
    }

    static FLAGS_ENV: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[tokio::test]
    async fn feature_flags_without_a_configured_host_is_501_not_a_baked_default() {
        let _env = FLAGS_ENV.lock().await;
        std::env::remove_var(FEATURE_FLAGS_ENV);
        let resp = feature_flags(AxPath("flags.json".to_string())).await;
        assert_eq!(resp.status(), StatusCode::NOT_IMPLEMENTED);
    }

    #[tokio::test]
    async fn feature_flags_refuses_a_file_that_could_climb_out_of_the_base() {
        let _env = FLAGS_ENV.lock().await;
        std::env::set_var(FEATURE_FLAGS_ENV, "https://flags.example");
        for bad in ["../secret", "a/b", "..\\secret"] {
            let resp = feature_flags(AxPath(bad.to_string())).await;
            assert_eq!(
                resp.status(),
                StatusCode::BAD_REQUEST,
                "{bad} must not reach the upstream"
            );
        }
        std::env::remove_var(FEATURE_FLAGS_ENV);
    }

    #[test]
    fn unmatched_pointers_splits_local_from_upstream() {
        let local = vec![json!({ "pointers": ["0,0", "0,1"] })];
        let asked = vec![
            "0,0".to_string(),
            "0,1".to_string(),
            "urn:decentraland:off-chain:base-avatars:BaseMale".to_string(),
        ];
        assert_eq!(
            unmatched_pointers(&local, &asked),
            vec!["urn:decentraland:off-chain:base-avatars:BaseMale".to_string()]
        );
        assert!(unmatched_pointers(&local, &["0,0".to_string()]).is_empty());
        assert!(unmatched_pointers(&local, &[]).is_empty());
        let mixed = vec![json!({ "pointers": ["urn:x:Y"] })];
        assert!(unmatched_pointers(&mixed, &["urn:x:y".to_string()]).is_empty());
    }

    #[test]
    fn parcel_pointers_never_go_upstream() {
        let local = vec![json!({ "pointers": ["0,0"] })];
        assert!(unmatched_pointers(&local, &["5,-12".to_string()]).is_empty());
        assert!(unmatched_pointers(&local, &[" -3 , 4 ".to_string()]).is_empty());
        assert!(is_parcel_pointer("0,0"));
        assert!(is_parcel_pointer("-73,50"));
        assert!(!is_parcel_pointer(
            "urn:decentraland:off-chain:base-avatars:BaseMale"
        ));
        assert!(!is_parcel_pointer("0,0,0"));
        assert!(!is_parcel_pointer("main.crdt"));
    }
}
