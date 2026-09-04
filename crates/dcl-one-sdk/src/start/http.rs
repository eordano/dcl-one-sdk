use super::{
    forwarded_host, forwarded_prefix, forwarded_proto, lock_cache, AppState, ENTITY_CACHE_TTL,
};
use crate::deploy::collect_publishable_files;
use crate::live_reload::ReloadFrame;
use crate::scene::{
    b64_content_hash_in_root, b64_hash_in_root, b64_unhash, hash_path_part, root_tag, Project,
};
use axum::{
    extract::{ws::Message, Path as AxPath, RawQuery, Request, State, WebSocketUpgrade},
    http::{header, HeaderMap, Method, StatusCode},
    response::{IntoResponse, Redirect, Response},
    Json,
};
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;

pub(super) async fn root(State(st): State<Arc<AppState>>, req: Request) -> Response {
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
            return super::landing::page(&st, req.headers(), req.uri().query());
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
/// Everything this server advertises about itself is built on it.
pub(super) fn preview_origin(headers: &HeaderMap) -> String {
    format!(
        "{}://{}{}",
        forwarded_proto(headers),
        preview_host(headers),
        forwarded_prefix(headers)
    )
}

/// [`preview_origin`] with the websocket scheme.
pub(super) fn preview_ws_origin(headers: &HeaderMap) -> String {
    let ws_proto = if forwarded_proto(headers) == "https" {
        "wss"
    } else {
        "ws"
    };
    format!(
        "{ws_proto}://{}{}",
        preview_host(headers),
        forwarded_prefix(headers)
    )
}

/// Where the catalyst back-fill and the world mirror keep fetched content.
pub(super) fn contents_cache_dir(st: &AppState) -> Option<PathBuf> {
    st.first_project()
        .map(|p| p.root.join(".dcl-cache").join("contents"))
}

pub(super) async fn about(State(st): State<Arc<AppState>>, req: Request) -> Json<Value> {
    let headers = req.headers();
    let host = preview_host(headers);
    let origin = preview_origin(headers);
    let fixed_adapter = if st.offline_comms {
        "offline:offline".to_string()
    } else {
        format!("ws-room:{}/mini-comms/room-1", preview_ws_origin(headers))
    };
    let projects = st.projects();
    let parcels: Vec<String> = projects.iter().flat_map(|p| p.parcels()).collect();
    let scenes_urn: Vec<String> = projects
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

/// The first project's scene.json, as upstream serves it off disk; the
/// in-memory copy is the same document (the watcher and the editors both
/// write through `set_scene_json`).
pub(super) async fn scene_json(State(st): State<Arc<AppState>>) -> Response {
    match st.first_project() {
        Some(p) => Json(p.scene_json).into_response(),
        None => (StatusCode::NOT_FOUND, "no scene loaded").into_response(),
    }
}

/// Upstream hardcodes `https://feature-flags.decentraland.zone`; this toolchain
/// bakes in no third-party host, as with `proxy::WORLD_BASE_ENV`.
const FEATURE_FLAGS_ENV: &str = "DCL_ONE_SDK_FEATURE_FLAGS";

/// Proxied so a page on the preview origin is not CORS-blocked fetching flags.
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
    super::proxy::passthrough(Method::GET, &format!("{base}/{file}")).await
}

/// The smart-wearable manifests in this workspace, content URLs rebased onto
/// the preview origin. Upstream deprecates it for `/content/entities/active`;
/// kept for older explorer builds.
pub(super) async fn preview_wearables(
    State(st): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Json<Value> {
    let base = format!("{}/content/contents", preview_origin(&headers));
    Json(json!({
        "ok": true,
        "data": collect_preview_wearables(&st.projects(), &base, &st.machine),
    }))
}

/// One entry per project with a readable `wearable.json`; a plain scene
/// contributes nothing, so the route answers `{ok: true, data: []}` like
/// upstream. Hashes are the preview's own reversible path hashes.
fn collect_preview_wearables(projects: &[Project], base: &str, machine: &str) -> Vec<Value> {
    let mut out = Vec::new();
    for p in projects {
        let Ok(text) = std::fs::read_to_string(p.root.join("wearable.json")) else {
            continue;
        };
        let Ok(mut wearable) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let tag = root_tag(&p.root, machine);
        // Deliberately sequential where upstream parallelized (b7a44a20): this
        // route is unauthenticated and uncached, a wearable's file set is
        // small, and content_tag memoises the digests anyway. Also stronger
        // than upstream on ids: we mint content-versioned hashes here where
        // upstream keeps plain ones — the read side resolves on
        // hash_path_part alone, so both shapes decode.
        let contents: Vec<Value> = collect_publishable_files(&p.root)
            .unwrap_or_default()
            .iter()
            .map(|rel| {
                let hash = b64_content_hash_in_root(&tag, rel, &p.root.join(rel));
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

fn not_found() -> Response {
    (StatusCode::NOT_FOUND, "file not found").into_response()
}

/// `/content/contents/{hash}`. Resolution is by PATH, not by content: the hash
/// splits into a project's [`crate::scene::root_tag`] and the path inside it,
/// and the digest half is never compared against the file. A hash minted
/// before the last edit therefore returns 200 with the CURRENT bytes —
/// deliberately, so a fetch in flight when the watcher rebuilds does not fail,
/// and unavoidably, since no old version is kept. Pinned by
/// `tests::a_stale_digest_serves_the_current_bytes`.
///
/// A hash of ours naming a project we do not serve 404s rather than being
/// proxied upstream; a hash that is not ours at all (an IPFS CID) is proxied.
pub(super) async fn contents(
    method: Method,
    State(st): State<Arc<AppState>>,
    AxPath(hash): AxPath<String>,
    headers: HeaderMap,
) -> Response {
    let Some((tag, rel)) = b64_unhash(&hash) else {
        let cache_dir = contents_cache_dir(&st);
        return super::proxy::contents_upstream(method, &hash, &headers, cache_dir.as_deref())
            .await;
    };
    let Some(project) = project_for_tag(&st, &tag) else {
        tracing::info!(target: "access", "contents {hash} 404 unknown-scene");
        return not_found();
    };
    if rel.is_empty() {
        tracing::info!(target: "access", "contents <scene-entity-json> 200");
        return Json(scene_entity(&st, &project)).into_response();
    }
    let Ok(canonical) = dunce::canonicalize(project.root.join(&rel)) else {
        return not_found();
    };
    if !canonical.starts_with(&project.root) {
        return (StatusCode::FORBIDDEN, "outside project root").into_response();
    }
    if !is_published_hash(&st, &project, &hash) {
        tracing::info!(target: "access", "contents {hash} 404 not-published");
        return (StatusCode::NOT_FOUND, "not a published content file").into_response();
    }
    let Ok(file) = tokio::fs::File::open(&canonical).await else {
        return not_found();
    };
    let Ok(meta) = file.metadata().await else {
        return not_found();
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
    if method == Method::HEAD {
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

fn mime_for(path: &Path) -> &'static str {
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

/// Whether the hash names a file the scene publishes (keeps a `.dclignore`d
/// file from being read back out). Compares [`hash_path_part`], so it
/// authorizes a PATH: a digest one edit behind still passes, which is why a
/// reload does not 404 requests already in flight.
fn is_published_hash(st: &AppState, project: &Project, hash: &str) -> bool {
    published_paths(st, project).contains(hash_path_part(hash))
}

/// The explorer's asset-bundle verdict, read off the entity as `status`
/// (`DCL.Ipfs.TrimmedEntityDefinitionBase.assetBundleRegistryEnum`). Upstream
/// sends no such field, so the client defaults to `complete`; the sidecar IS
/// the registry for a preview, so it can answer honestly. `fallback` is
/// literal — the client falls back to raw GLTFs.
fn ab_status(optimized_assets_url: &OnceLock<String>) -> &'static str {
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
    build_and_cache(st, project).0
}

/// The published [`hash_path_part`]s of one generation of a scene entity.
type PublishedPaths = Arc<HashSet<String>>;

/// The sets that go with the entities in [`lock_cache`], stamped with their
/// entry's `Instant`: a set is returned only when its stamp is the one the
/// cached entity carries, and both are written under the entity-cache lock in
/// [`build_and_cache`], so a set never outlives the generation it describes —
/// including when the watcher drops a root's entry to force a rebuild.
type PublishedMemo = HashMap<PathBuf, (Instant, PublishedPaths)>;

fn lock_memo() -> MutexGuard<'static, PublishedMemo> {
    static MEMO: OnceLock<Mutex<PublishedMemo>> = OnceLock::new();
    MEMO.get_or_init(Default::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

fn published_paths_of(entity: &Value) -> HashSet<String> {
    entity
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|e| e.get("hash").and_then(Value::as_str))
        .map(|h| hash_path_part(h).to_string())
        .collect()
}

/// The published-path set for the entity currently cached for `project`,
/// rebuilding the entity if the cache is cold or stale, without deep-cloning
/// the content list on every asset request.
fn published_paths(st: &AppState, project: &Project) -> PublishedPaths {
    {
        let cache = lock_cache(st);
        if let Some((at, cached)) = cache.get(&project.root) {
            let at = *at;
            if at.elapsed() < ENTITY_CACHE_TTL {
                let mut memo = lock_memo();
                if let Some((stamp, paths)) = memo.get(&project.root) {
                    if *stamp == at {
                        return paths.clone();
                    }
                }
                let paths: PublishedPaths = Arc::new(published_paths_of(cached));
                memo.insert(project.root.clone(), (at, paths.clone()));
                return paths;
            }
        }
    }
    build_and_cache(st, project).1
}

/// Builds a scene entity and publishes it as one generation: entity and path
/// set land in their two maps under a single hold of the entity-cache lock.
fn build_and_cache(st: &AppState, project: &Project) -> (Value, PublishedPaths) {
    let entity = build_scene_entity(project, &st.machine);
    let paths: PublishedPaths = Arc::new(published_paths_of(&entity));
    let at = Instant::now();
    let mut cache = lock_cache(st);
    cache.insert(project.root.clone(), (at, entity.clone()));
    lock_memo().insert(project.root.clone(), (at, paths.clone()));
    (entity, paths)
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
    let tag = root_tag(root, machine);
    let content: Vec<Value> = crate::scene::parallel_map(&rels, |rel| {
        json!({
            "file": rel,
            "hash": b64_content_hash_in_root(&tag, rel, &root.join(rel)),
        })
    });
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

/// The scene entity id: the project's own [`root_tag`] with an empty path.
/// Built from the root the server serves, not the nearest `scene.json` above
/// it, so it is the exact tag [`project_for_tag`] matches on.
pub(super) fn scene_id_for(project: &Project, machine: &str) -> String {
    b64_hash_in_root(&root_tag(&project.root, machine), "")
}

/// The project a hash was minted under: a tag is a digest, so this is the only
/// way back to a root.
fn project_for_tag(st: &AppState, tag: &str) -> Option<Project> {
    st.projects()
        .into_iter()
        .find(|p| root_tag(&p.root, &st.machine) == tag)
}

fn pointers_of(entity: &Value) -> impl Iterator<Item = &str> {
    entity
        .get("pointers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
}

pub(super) fn entities_for(st: &AppState, pointers: &[String]) -> Vec<Value> {
    let entities: Vec<Value> = st.projects().iter().map(|p| scene_entity(st, p)).collect();
    if pointers.is_empty() {
        return entities;
    }
    entities
        .into_iter()
        .filter(|e| pointers_of(e).any(|s| pointers.iter().any(|q| q == s)))
        .collect()
}

pub(super) async fn entities_active(
    State(st): State<Arc<AppState>>,
    body: Option<Json<Value>>,
) -> Json<Value> {
    let pointers: Vec<String> = body
        .as_ref()
        .and_then(|b| b.0.get("pointers"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();
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
            !entities
                .iter()
                .any(|e| pointers_of(e).any(|s| s.eq_ignore_ascii_case(q)))
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
    /// The path-discovering entry point the handlers no longer call: the tests
    /// mint hashes the way a client's URL arrives, from an absolute path alone,
    /// so they also pin that both doors produce the same hash.
    use crate::scene::b64_content_hash;
    use crate::start::testkit::{self, body_text, Tmp};

    fn project_at(root: PathBuf) -> Project {
        Project {
            root,
            scene_json: json!({ "main": "bin/index.js" }),
        }
    }

    fn contents_state(projects: Vec<Project>) -> Arc<AppState> {
        let mut st = testkit::state(projects);
        st.machine = "m".to_string();
        st.port = 8000;
        st.local_ab = false;
        Arc::new(st)
    }

    async fn get_contents_response(st: &Arc<AppState>, hash: &str) -> Response {
        contents(
            Method::GET,
            State(st.clone()),
            AxPath(hash.to_string()),
            HeaderMap::new(),
        )
        .await
    }

    async fn get_contents(st: &Arc<AppState>, hash: &str) -> StatusCode {
        get_contents_response(st, hash).await.status()
    }

    async fn get_contents_body(st: &Arc<AppState>, hash: &str) -> (StatusCode, String) {
        let response = get_contents_response(st, hash).await;
        (response.status(), body_text(response).await)
    }

    /// A one-file scene at `<dir>/gather`, with `bin/index.js` holding `body`.
    fn published_scene(dir: &Path, body: &str) -> Project {
        testkit::scene(dir, "gather", &["0,0"], body)
    }

    fn hash_of(project: &Project, rel: &str) -> String {
        b64_content_hash(&project.root.join(rel).display().to_string(), "m")
    }

    fn b64(s: &str) -> String {
        use base64::Engine;
        format!(
            "b64-{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(s)
        )
    }

    /// The read side is NOT content-addressed: after an edit, a URL carrying
    /// the OLD digest is answered 200 with the NEW bytes — no old version is
    /// kept, and failing would break a request in flight when the watcher
    /// rewrote the file. The write half does hold: the edit changes the hash.
    #[tokio::test]
    async fn a_stale_digest_serves_the_current_bytes() {
        let tmp = Tmp::new("stale-digest");
        let project = published_scene(&tmp.0, "// first");
        let js = project.root.join("bin/index.js");
        let st = contents_state(vec![project.clone()]);

        let before = hash_of(&project, "bin/index.js");
        assert_eq!(
            get_contents_body(&st, &before).await,
            (StatusCode::OK, "// first".to_string())
        );

        std::fs::write(&js, "// second, and longer").unwrap();
        let after = hash_of(&project, "bin/index.js");
        assert_ne!(
            after, before,
            "the write side IS content-addressed: an edit must rename the file"
        );
        assert_eq!(
            hash_path_part(&after),
            hash_path_part(&before),
            "only the digest half moves; the path half is what the route matches on"
        );

        assert_eq!(
            get_contents_body(&st, &before).await,
            (StatusCode::OK, "// second, and longer".to_string()),
            "a superseded digest is neither rejected nor honoured — it serves current bytes"
        );
        assert_eq!(
            get_contents_body(&st, &after).await,
            (StatusCode::OK, "// second, and longer".to_string())
        );

        let invented = format!("{}.ffffffffffff", hash_path_part(&before));
        assert_eq!(
            get_contents_body(&st, &invented).await,
            (StatusCode::OK, "// second, and longer".to_string()),
            "a digest that never named any version of this file resolves just the same"
        );
    }

    #[tokio::test]
    async fn a_content_hash_still_resolves_when_it_is_fresh_stale_or_the_scene_id() {
        let tmp = Tmp::new("resolve");
        let project = published_scene(&tmp.0, "module.exports={}");
        let st = contents_state(vec![project.clone()]);

        let fresh = hash_of(&project, "bin/index.js");
        assert_eq!(get_contents(&st, &fresh).await, StatusCode::OK);

        let stale = format!("{}.000000000000", hash_path_part(&fresh));
        assert_ne!(stale, fresh);
        assert_eq!(
            get_contents(&st, &stale).await,
            StatusCode::OK,
            "a hash minted before the last edit still names the same file"
        );

        assert_eq!(
            get_contents(&st, &scene_id_for(&project, "m")).await,
            StatusCode::OK
        );

        let tag = root_tag(&project.root, "m");
        let escape = b64(&format!("{tag}/../../etc/passwd"));
        assert_ne!(get_contents(&st, &escape).await, StatusCode::OK);

        let other_scene = b64("0123456789abcdef/bin/index.js");
        assert_eq!(
            get_contents(&st, &other_scene).await,
            StatusCode::NOT_FOUND,
            "a hash minted for a root we do not serve must not be answered from disk"
        );
    }

    /// A published tree can contain a second `scene.json` (a vendored scene).
    /// The files under it are published by THIS scene, so their hashes must
    /// name this scene's root; minting from the absolute path would tag them
    /// under the nearest `scene.json`, a project the server does not serve.
    #[tokio::test]
    async fn a_file_under_a_nested_scene_json_is_still_served_by_its_own_scene() {
        let tmp = Tmp::new("nested-root");
        let project = published_scene(&tmp.0, "// main");
        std::fs::create_dir_all(project.root.join("sub")).unwrap();
        std::fs::write(project.root.join("sub/scene.json"), "{}").unwrap();
        std::fs::write(project.root.join("sub/model.glb"), b"glb").unwrap();
        let st = contents_state(vec![project.clone()]);

        let entity = build_scene_entity(&project, "m");
        let hash = entity["content"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["file"] == json!("sub/model.glb"))
            .expect("the nested file is published by this scene")["hash"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(
            b64_unhash(&hash).unwrap(),
            (root_tag(&project.root, "m"), "sub/model.glb".to_string()),
            "the entity tagged the file under the nested scene.json, not the \
             root this server serves it from"
        );
        assert_eq!(
            get_contents_body(&st, &hash).await,
            (StatusCode::OK, "glb".to_string()),
            "a hash this very entity advertises must resolve"
        );
    }

    /// The published-path set must never outlive its entity generation: the
    /// watcher drops a root's entity so the next request sees the new file
    /// list, and a set keyed by root alone would keep answering from the old
    /// one.
    #[tokio::test]
    async fn dropping_the_cached_entity_drops_its_published_set_too() {
        let tmp = Tmp::new("published-set");
        let project = published_scene(&tmp.0, "// main");
        let st = contents_state(vec![project.clone()]);

        assert_eq!(
            get_contents(&st, &hash_of(&project, "bin/index.js")).await,
            StatusCode::OK,
            "the first request fills both the entity cache and its path set"
        );

        std::fs::write(project.root.join("late.glb"), b"glb").unwrap();
        lock_cache(&st).remove(&project.root);
        assert_eq!(
            get_contents(&st, &hash_of(&project, "late.glb")).await,
            StatusCode::OK,
            "a file added since the dropped entity was built is still unpublished"
        );

        std::fs::write(project.root.join(".dclignore"), "late.glb\n").unwrap();
        lock_cache(&st).remove(&project.root);
        assert_eq!(
            get_contents(&st, &hash_of(&project, "late.glb")).await,
            StatusCode::NOT_FOUND,
            "a file .dclignore now excludes is still being byte-served"
        );

        std::fs::remove_file(project.root.join(".dclignore")).unwrap();
        let fresh = build_scene_entity(&project, &st.machine);
        lock_cache(&st).insert(project.root.clone(), (Instant::now(), fresh));
        assert_eq!(
            get_contents(&st, &hash_of(&project, "late.glb")).await,
            StatusCode::OK,
            "membership was answered from a set the cached entity did not come from"
        );
    }

    #[test]
    fn ab_status_tells_the_truth_about_whether_bundles_are_served() {
        let url = OnceLock::new();
        assert_eq!(ab_status(&url), "fallback");
        let _ = url.set("http://127.0.0.1:5147".to_string());
        assert_eq!(ab_status(&url), "complete");
    }

    #[test]
    fn a_scene_without_a_wearable_json_contributes_no_entries() {
        let tmp = Tmp::new("plain");
        std::fs::write(tmp.0.join("scene.json"), "{}").unwrap();
        let out = collect_preview_wearables(&[project_at(tmp.0.clone())], "http://x/c", "m");
        assert!(out.is_empty());
    }

    #[test]
    fn a_smart_wearable_is_listed_with_preview_resolvable_urls() {
        let tmp = Tmp::new("sw");
        std::fs::write(tmp.0.join("scene.json"), "{}").unwrap();
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
        assert_eq!(
            b64_unhash(model["hash"].as_str().unwrap()).unwrap(),
            (root_tag(&tmp.0, "m"), "model.glb".to_string()),
            "the url has to name the file inside the wearable's own root, or \
             /content/contents cannot find it"
        );
    }

    #[test]
    fn a_malformed_wearable_json_is_skipped_not_fatal() {
        let tmp = Tmp::new("bad");
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
