//! Catalyst proxy routes, ported from upstream sdk-commands
//! `start/server/endpoints.ts`: the explorer talks to the preview realm's
//! lambdas/content for profiles and profile deploys, so a local realm must
//! forward what it does not serve itself. Without these the v0.158+ desktop
//! client clears a cached identity on boot (profile fetch 404s → treated as
//! abandoned onboarding) and the new-account lobby's profile deploy fails.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, PoisonError};

use axum::body::Bytes;
use axum::extract::Request;
use axum::http::{header, HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use serde_json::json;

use super::http::preview_host;

/// Falls back to the local catalyrst content server rather than a public
/// catalyst: a preview must never silently source its realm from production.
const LOCAL_CATALYST: &str = "http://127.0.0.1:5141";

/// Where `/world/{name}/about` is proxied from. That proxy exists purely to
/// lift the fetch onto the preview origin, since a browser page served from a
/// different origin is CORS-blocked talking to a worlds host directly.
///
/// There is deliberately NO baked default. Whatever host went in here would be
/// infrastructure somebody runs, and a toolchain shipping one silently points
/// every user's preview at it. The engine takes the same line — bevy-explorer
/// refuses to fall back to a public host and tells you to set
/// `DCL_WORLD_REALM_BASE` — so this is configuration, not a constant.
///
/// Unset, the world proxy is the only thing that stops working; the rest of the
/// preview is unaffected.
pub(crate) const WORLD_BASE_ENV: &str = "DCL_ONE_SDK_WORLD_BASE";

pub(crate) fn world_base() -> Option<String> {
    match std::env::var(WORLD_BASE_ENV) {
        Ok(v) if !v.trim().is_empty() => Some(v.trim().trim_end_matches('/').to_string()),
        _ => None,
    }
}

/// One sentence, wherever the absence has to be explained to a human.
pub(crate) fn world_base_hint() -> String {
    format!(
        "no worlds host configured — set {WORLD_BASE_ENV} to the base URL that serves \
         /<world>/about. This toolchain ships no default, so nothing is fetched from a \
         third party unless you name it."
    )
}

pub(crate) fn catalyst_base() -> String {
    match std::env::var("DCL_ONE_SDK_CATALYST") {
        Ok(v) if !v.trim().is_empty() => v.trim().trim_end_matches('/').to_string(),
        _ => LOCAL_CATALYST.to_string(),
    }
}

/// The primary upstream plus two fallbacks from a configured rotation, so a
/// timeout or 5xx on one catalyst does not strand wearable/profile fetches.
/// An explicit DCL_ONE_SDK_CATALYST is respected first but still falls back.
/// Only a rotation someone named is used: unlike `deploy`, whose purpose is to
/// publish to the public network, a preview must not source a realm from it
/// unasked.
fn upstream_candidates() -> Vec<String> {
    let primary = catalyst_base();
    let mut out = vec![primary.clone()];
    out.extend(
        crate::deploy::configured_catalyst_rotation()
            .unwrap_or_default()
            .into_iter()
            .filter(|c| *c != primary)
            .take(2),
    );
    out
}

fn proxy_client() -> Result<reqwest::Client, Response> {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                format!("catalyst proxy client: {e}"),
            )
                .into_response()
        })
}

/// Upstream parity: the realm advertises itself as the only realm.
pub(super) async fn lambdas_explore_realms(req: Request) -> Json<serde_json::Value> {
    let host = preview_host(req.headers());
    Json(json!([{
        "serverName": "localhost",
        "url": format!("http://{host}"),
        "layer": "stub",
        "usersCount": 0,
        "maxUsers": 100,
        "userParcels": []
    }]))
}

/// Upstream parity: a single stub catalyst contract entry pointing local.
pub(super) async fn lambdas_contracts_servers(req: Request) -> Json<serde_json::Value> {
    let host = preview_host(req.headers());
    Json(json!([{
        "address": format!("http://{host}"),
        "owner": "0x0000000000000000000000000000000000000000",
        "id": "0x0000000000000000000000000000000000000000000000000000000000000000"
    }]))
}

async fn forward_raw(
    method: Method,
    upstream_path_and_query: String,
    headers: &HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, String, Bytes), Response> {
    let client = proxy_client()?;
    let retriable = method == Method::GET || method == Method::HEAD;
    let candidates = if retriable {
        upstream_candidates()
    } else {
        vec![catalyst_base()]
    };
    let mut last_err: Option<Response> = None;
    let mut last_5xx: Option<(StatusCode, String, Bytes)> = None;
    for (i, base) in candidates.iter().enumerate() {
        let url = format!("{base}{upstream_path_and_query}");
        if i > 0 {
            tracing::info!("catalyst proxy retrying against {base}");
        }
        let mut req = client.request(method.clone(), &url);
        if let Some(ct) = headers.get(header::CONTENT_TYPE) {
            req = req.header(header::CONTENT_TYPE, ct);
        }
        if !retriable {
            req = req.body(body.to_vec());
        }
        match req.send().await {
            Ok(resp) => {
                let status = resp.status();
                let content_type = resp
                    .headers()
                    .get(header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("application/binary")
                    .to_string();
                // reqwest decompresses; content-encoding/length must not be
                // forwarded (upstream drops them for the same reason).
                let bytes = resp.bytes().await.unwrap_or_default();
                let status =
                    StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
                if status.is_server_error() && i + 1 < candidates.len() {
                    tracing::warn!("catalyst proxy {url}: {status}");
                    last_5xx = Some((status, content_type, bytes));
                    continue;
                }
                return Ok((status, content_type, bytes));
            }
            Err(e) => {
                tracing::warn!("catalyst proxy {url}: {e}");
                last_err =
                    Some((StatusCode::BAD_GATEWAY, format!("catalyst proxy: {e}")).into_response());
            }
        }
    }
    if let Some(res) = last_5xx {
        return Ok(res);
    }
    Err(last_err.unwrap_or_else(|| {
        (StatusCode::BAD_GATEWAY, "catalyst proxy: no upstream").into_response()
    }))
}

async fn forward(
    method: Method,
    upstream_path_and_query: String,
    headers: &HeaderMap,
    body: Bytes,
) -> Response {
    match forward_raw(method, upstream_path_and_query, headers, body).await {
        Ok((status, ct, bytes)) => (status, [(header::CONTENT_TYPE, ct)], bytes).into_response(),
        Err(resp) => resp,
    }
}

/// Backup fetch: content hashes the local scene does not own (wearable GLBs,
/// emotes, profile snapshots) are served from the upstream catalyst so the
/// explorer can render avatars in preview. Successful GETs land in the
/// scene's `.dcl-cache` LRU so the next preview session serves them offline
/// and instantly; content is immutable, so hits never revalidate.
pub(super) async fn contents_upstream(
    method: Method,
    hash: &str,
    headers: &HeaderMap,
    cache_dir: Option<&std::path::Path>,
) -> Response {
    const IMMUTABLE: &str = "public, max-age=31536000, immutable";
    if let Some(dir) = cache_dir {
        if let Some((bytes, ct)) = super::content_cache::get(dir, hash).await {
            let ct = ct.unwrap_or_else(|| "application/octet-stream".to_string());
            tracing::info!(target: "access", "contents {hash} 200 dcl-cache sent={}", bytes.len());
            let resp_headers = [
                (header::CONTENT_TYPE, ct),
                (header::CACHE_CONTROL, IMMUTABLE.to_string()),
                (header::CONTENT_LENGTH, bytes.len().to_string()),
            ];
            if method == Method::HEAD {
                return (resp_headers, axum::body::Body::empty()).into_response();
            }
            return (resp_headers, bytes).into_response();
        }
    }
    match forward_raw(
        method.clone(),
        format!("/content/contents/{hash}"),
        headers,
        Bytes::new(),
    )
    .await
    {
        Ok((status, ct, bytes)) => {
            if status == StatusCode::OK && method == Method::GET {
                if let Some(dir) = cache_dir {
                    super::content_cache::put(dir, hash, &bytes, Some(&ct)).await;
                }
            }
            (status, [(header::CONTENT_TYPE, ct)], bytes).into_response()
        }
        Err(resp) => resp,
    }
}

/// Backup fetch: pointers no local scene covers (wearable/emote URNs) resolve
/// against the upstream catalyst; failures degrade to local-only results.
pub(super) async fn entities_active_upstream(pointers: &[String]) -> Vec<serde_json::Value> {
    let url = format!("{}/content/entities/active", catalyst_base());
    let Ok(client) = proxy_client() else {
        return Vec::new();
    };
    match client
        .post(&url)
        .json(&json!({ "pointers": pointers }))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => match resp.json::<serde_json::Value>().await {
            Ok(serde_json::Value::Array(arr)) => arr,
            _ => Vec::new(),
        },
        Ok(resp) => {
            tracing::warn!("catalyst entities/active {url}: {}", resp.status());
            Vec::new()
        }
        Err(e) => {
            tracing::warn!("catalyst entities/active {url}: {e}");
            Vec::new()
        }
    }
}

/// `router.all('/lambdas/:path+')` upstream: forward verbatim to the catalyst.
pub(super) async fn lambdas_proxy(req: Request) -> Response {
    proxy_request(req).await
}

/// `router.all('/explorer/:path+')` upstream.
pub(super) async fn explorer_proxy(req: Request) -> Response {
    proxy_request(req).await
}

/// `router.post('/content/entities')` upstream: the client's own deploys
/// (profile publication from the onboarding lobby) go through the realm.
pub(super) async fn entities_deploy_proxy(req: Request) -> Response {
    proxy_request(req).await
}

async fn proxy_request(req: Request) -> Response {
    let method = req.method().clone();
    let path_and_query = req
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str().to_string())
        .unwrap_or_else(|| req.uri().path().to_string());
    let headers = req.headers().clone();
    let body = match axum::body::to_bytes(req.into_body(), 64 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            return (StatusCode::PAYLOAD_TOO_LARGE, format!("proxy body: {e}")).into_response()
        }
    };
    forward(method, path_and_query, &headers, body).await
}

/// World name (lowercased) -> candidate upstream contents prefixes
/// (`…/contents/`), best first, learned from the world's own /about. Content
/// hashes are immutable and the list is re-derivable by refetching the about,
/// so a plain process cache is enough.
static WORLD_CONTENT_UPSTREAMS: LazyLock<Mutex<HashMap<String, Vec<String>>>> =
    LazyLock::new(Default::default);

fn world_upstreams() -> std::sync::MutexGuard<'static, HashMap<String, Vec<String>>> {
    WORLD_CONTENT_UPSTREAMS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

fn valid_world_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

async fn fetch_world_about(name: &str) -> Result<serde_json::Value, Response> {
    let client = proxy_client()?;
    let base = world_base().ok_or_else(|| {
        // 501 rather than 502: nothing upstream failed, the feature was never
        // configured. Saying so beats proxying to a host the operator did not
        // choose and never learning that it happened.
        (StatusCode::NOT_IMPLEMENTED, world_base_hint()).into_response()
    })?;
    let url = format!("{base}/{name}/about");
    match client.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => {
            resp.json::<serde_json::Value>().await.map_err(|e| {
                (StatusCode::BAD_GATEWAY, format!("world about {url}: {e}")).into_response()
            })
        }
        Ok(resp) => {
            let status =
                StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            Err((status, format!("world about {url}: {status}")).into_response())
        }
        Err(e) => Err((StatusCode::BAD_GATEWAY, format!("world about {url}: {e}")).into_response()),
    }
}

fn origin_of(url: &str) -> Option<String> {
    let scheme_end = url.find("://")? + 3;
    let end = url[scheme_end..]
        .find('/')
        .map(|i| scheme_end + i)
        .unwrap_or(url.len());
    Some(url[..end].to_string())
}

/// Every place the world's content might actually be served from, best guess
/// first. Not just the urn's own baseUrl: a federated deployment can advertise
/// a worlds host that does not expose `/contents/` (some deployments do),
/// while the catalyst that proxied the /about has the entity synced — so the
/// world base's own `/content/contents/` and the about's `content.publicUrl`
/// are kept as fallbacks. Hashes are immutable, so any host that answers 200
/// answers correctly.
fn world_content_candidates(about: &serde_json::Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |prefix: String| {
        let prefix = if prefix.ends_with('/') {
            prefix
        } else {
            format!("{prefix}/")
        };
        if !out.contains(&prefix) {
            out.push(prefix);
        }
    };
    if let Some(urns) = about
        .pointer("/configurations/scenesUrn")
        .and_then(|v| v.as_array())
    {
        for base in urns
            .iter()
            .filter_map(|u| u.as_str())
            .filter_map(|u| u.split_once("baseUrl=").map(|(_, base)| base))
            .filter(|base| !base.is_empty())
        {
            push(base.to_string());
        }
    }
    if let Some(origin) = world_base().as_deref().and_then(origin_of) {
        push(format!("{origin}/content/contents/"));
    }
    if let Some(pu) = about.pointer("/content/publicUrl").and_then(|v| v.as_str()) {
        push(format!("{}/contents/", pu.trim_end_matches('/')));
    }
    out
}

/// Rewrites only what the explorer's portable lookup consumes
/// (`configurations.scenesUrn`): every other field keeps its upstream value so
/// unexpected flows fail against the real host instead of a local 404.
fn rewrite_scenes_urn(about: &mut serde_json::Value, local_contents_prefix: &str) {
    let Some(urns) = about
        .pointer_mut("/configurations/scenesUrn")
        .and_then(|v| v.as_array_mut())
    else {
        return;
    };
    for urn in urns {
        if let Some(s) = urn.as_str() {
            if let Some((head, _)) = s.split_once("baseUrl=") {
                *urn = serde_json::Value::String(format!("{head}baseUrl={local_contents_prefix}"));
            }
        }
    }
}

/// Same-origin mirror of a world's /about, so a browser explorer can load a
/// portable world (the movement controller in particular) without the CORS
/// wall around the public worlds host. Content moves to `/world-content/…` on
/// this origin, which the permissive CORS layer already covers.
pub(super) async fn world_about(
    axum::extract::Path(name): axum::extract::Path<String>,
    headers: HeaderMap,
) -> Response {
    if !valid_world_name(&name) {
        return (StatusCode::BAD_REQUEST, "invalid world name").into_response();
    }
    let mut about = match fetch_world_about(&name).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let candidates = world_content_candidates(&about);
    if candidates.is_empty() {
        return (
            StatusCode::BAD_GATEWAY,
            "world about carries no content base",
        )
            .into_response();
    }
    world_upstreams().insert(name.to_ascii_lowercase(), candidates);
    let proto = super::forwarded_proto(&headers);
    let host = preview_host(&headers);
    let prefix = super::forwarded_prefix(&headers);
    rewrite_scenes_urn(
        &mut about,
        &format!("{proto}://{host}{prefix}/world-content/{name}/contents/"),
    );
    Json(about).into_response()
}

/// Content half of the world mirror: immutable hashes, so hits land in the
/// same `.dcl-cache` LRU the catalyst back-fill uses and never revalidate.
pub(super) async fn world_content(
    method: Method,
    axum::extract::State(st): axum::extract::State<std::sync::Arc<super::AppState>>,
    axum::extract::Path((name, hash)): axum::extract::Path<(String, String)>,
) -> Response {
    if !valid_world_name(&name) || !hash.chars().all(|c| c.is_ascii_alphanumeric()) {
        return (StatusCode::BAD_REQUEST, "invalid world content path").into_response();
    }
    let cache_dir = st
        .projects
        .first()
        .map(|p| p.root.join(".dcl-cache").join("contents"));
    const IMMUTABLE: &str = "public, max-age=31536000, immutable";
    if let Some(dir) = cache_dir.as_deref() {
        if let Some((bytes, ct)) = super::content_cache::get(dir, &hash).await {
            let ct = ct.unwrap_or_else(|| "application/octet-stream".to_string());
            tracing::info!(target: "access", "world-content {hash} 200 dcl-cache sent={}", bytes.len());
            let resp_headers = [
                (header::CONTENT_TYPE, ct),
                (header::CACHE_CONTROL, IMMUTABLE.to_string()),
                (header::CONTENT_LENGTH, bytes.len().to_string()),
            ];
            if method == Method::HEAD {
                return (resp_headers, axum::body::Body::empty()).into_response();
            }
            return (resp_headers, bytes).into_response();
        }
    }
    // Bound before the match: the guard must not live across the refetch await.
    let cached = world_upstreams().get(&name.to_ascii_lowercase()).cloned();
    let candidates = match cached {
        Some(c) => c,
        // Server restarted between the about and the content fetch: re-derive.
        None => match fetch_world_about(&name).await.map(|a| {
            let c = world_content_candidates(&a);
            if !c.is_empty() {
                world_upstreams().insert(name.to_ascii_lowercase(), c.clone());
            }
            c
        }) {
            Ok(c) if !c.is_empty() => c,
            Ok(_) => {
                return (
                    StatusCode::BAD_GATEWAY,
                    "world about carries no content base",
                )
                    .into_response()
            }
            Err(resp) => return resp,
        },
    };
    let client = match proxy_client() {
        Ok(c) => c,
        Err(resp) => return resp,
    };
    let mut last: Option<(StatusCode, String)> = None;
    for (i, upstream) in candidates.iter().enumerate() {
        let url = format!("{upstream}{hash}");
        match client.request(method.clone(), &url).send().await {
            Ok(resp) if resp.status().is_success() => {
                let ct = resp
                    .headers()
                    .get(header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("application/octet-stream")
                    .to_string();
                let bytes = resp.bytes().await.unwrap_or_default();
                if method == Method::GET {
                    if let Some(dir) = cache_dir.as_deref() {
                        super::content_cache::put(dir, &hash, &bytes, Some(&ct)).await;
                    }
                }
                if i > 0 {
                    // Promote the answering host so the rest of this scene's
                    // files skip the dead candidates.
                    let mut map = world_upstreams();
                    if let Some(list) = map.get_mut(&name.to_ascii_lowercase()) {
                        if let Some(pos) = list.iter().position(|u| u == upstream) {
                            list.swap(0, pos);
                        }
                    }
                }
                return (
                    StatusCode::OK,
                    [
                        (header::CONTENT_TYPE, ct),
                        (header::CACHE_CONTROL, IMMUTABLE.to_string()),
                    ],
                    bytes,
                )
                    .into_response();
            }
            Ok(resp) => {
                let status =
                    StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
                tracing::warn!("world content {url}: {status}");
                last = Some((status, format!("world content {url}: {status}")));
            }
            Err(e) => {
                tracing::warn!("world content {url}: {e}");
                last = Some((StatusCode::BAD_GATEWAY, format!("world content {url}: {e}")));
            }
        }
    }
    let (status, message) =
        last.unwrap_or((StatusCode::BAD_GATEWAY, "world content: no upstream".into()));
    (status, message).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn scenes_urn_rewrites_to_the_local_mirror_and_keeps_the_entity() {
        let mut about = json!({
            "configurations": { "scenesUrn": [
                "urn:decentraland:entity:bafkreia73rhs?=&baseUrl=https://worlds.example.org/contents/"
            ]},
            "content": { "publicUrl": "https://worlds.example.org" }
        });
        assert_eq!(
            world_content_candidates(&about).first().map(String::as_str),
            Some("https://worlds.example.org/contents/")
        );
        rewrite_scenes_urn(
            &mut about,
            "http://127.0.0.1:8000/world-content/w.dcl.eth/contents/",
        );
        assert_eq!(
            about["configurations"]["scenesUrn"][0],
            json!(
                "urn:decentraland:entity:bafkreia73rhs?=&baseUrl=http://127.0.0.1:8000/world-content/w.dcl.eth/contents/"
            )
        );
        assert_eq!(
            about["content"]["publicUrl"],
            json!("https://worlds.example.org"),
            "only scenesUrn is mirrored; other fields keep their upstream value"
        );
    }

    /// The advertised baseUrl can point at a host that does not serve
    /// `/contents/` (this is a real deployment shape), so the world
    /// base's catalyst and the about's content.publicUrl stay as fallbacks.
    #[test]
    fn content_candidates_are_ordered_and_deduped() {
        let about = json!({
            "configurations": { "scenesUrn": [
                "urn:decentraland:entity:bafkreia?=&baseUrl=https://worlds.example/contents/"
            ]},
            "content": { "publicUrl": "https://peer.example/content" }
        });
        let candidates = world_content_candidates(&about);
        assert_eq!(candidates[0], "https://worlds.example/contents/");
        // The world-base origin is a candidate only when one is configured.
        // Unset — which is how the suite runs — it contributes nothing, and the
        // urn baseUrl and about publicUrl still carry the resolution.
        match world_base().as_deref().and_then(origin_of) {
            Some(origin) => assert!(candidates.contains(&format!("{origin}/content/contents/"))),
            None => assert!(
                candidates
                    .iter()
                    .all(|c| c.starts_with("https://worlds.example") || c.starts_with("https://peer.example")),
                "unconfigured world base must not introduce a candidate host: {candidates:?}"
            ),
        }
        assert!(candidates.contains(&"https://peer.example/content/contents/".to_string()));

        let bare = json!({ "content": { "publicUrl": "https://worlds.example/" } });
        assert!(world_content_candidates(&bare)
            .contains(&"https://worlds.example/contents/".to_string()));

        let unique: std::collections::HashSet<_> = candidates.iter().collect();
        assert_eq!(unique.len(), candidates.len());
    }

    #[test]
    fn origin_of_extracts_scheme_and_authority() {
        assert_eq!(
            origin_of("https://catalyst.example.org/world").as_deref(),
            Some("https://catalyst.example.org")
        );
        assert_eq!(
            origin_of("http://127.0.0.1:5141").as_deref(),
            Some("http://127.0.0.1:5141")
        );
        assert_eq!(origin_of("not-a-url"), None);
    }

    #[test]
    fn world_names_and_hashes_are_narrow() {
        assert!(valid_world_name("basiccontroller.dcl.eth"));
        assert!(valid_world_name("my-world_2.dcl.eth"));
        assert!(!valid_world_name(""));
        assert!(!valid_world_name("a/b"));
        assert!(!valid_world_name("a?b=c"));
    }
}
