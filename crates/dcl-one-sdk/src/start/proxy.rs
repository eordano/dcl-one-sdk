//! Catalyst proxy routes, ported from upstream sdk-commands
//! `start/server/endpoints.ts`: the explorer talks to the preview realm's
//! lambdas/content for profiles and profile deploys. Without these the v0.158+
//! desktop client clears a cached identity on boot (a profile 404 reads as
//! abandoned onboarding) and the new-account lobby's profile deploy fails.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, PoisonError};

use axum::body::Bytes;
use axum::extract::Request;
use axum::http::{header, HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use serde_json::json;

use super::http::{contents_cache_dir, preview_host, preview_origin};

/// Where profiles, wearables and avatars come from. This used to default to a
/// local catalyrst on 5141, which almost nobody runs, so every preview lost its
/// avatars; upstream `@dcl/sdk-commands` ships a default here too
/// (`logic/config.ts::getCatalystBaseUrl`). The env knob still aims it anywhere.
const DEFAULT_CATALYST: &str = "https://interconnected.online";

/// Ours first, then upstream's own name so a project already configured for
/// `@dcl/sdk-commands` does not need a second variable.
const CATALYST_ENV: [&str; 2] = ["DCL_ONE_SDK_CATALYST", "DCL_CATALYST"];

/// Where `/world/{name}/about` is proxied from. Deliberately no baked default:
/// whatever host went here would be infrastructure somebody runs, and shipping
/// one silently points every preview at it (bevy-explorer takes the same line
/// with `DCL_WORLD_REALM_BASE`). Unset, only the world proxy stops working.
pub(crate) const WORLD_BASE_ENV: &str = "DCL_ONE_SDK_WORLD_BASE";

/// Immutable content hashes never revalidate.
const IMMUTABLE: &str = "public, max-age=31536000, immutable";

/// First of `names` set to a non-blank value, trimmed and without a trailing
/// slash.
pub(super) fn configured_base(names: &[&str]) -> Option<String> {
    names.iter().find_map(|k| match std::env::var(k) {
        Ok(v) if !v.trim().is_empty() => Some(v.trim().trim_end_matches('/').to_string()),
        _ => None,
    })
}

/// The one sentence every unconfigured-upstream route says, so the "we bake in
/// no third-party host" promise is worded the same wherever it surfaces.
pub(super) fn unconfigured_host_hint(what: &str, env: &str, serves: &str) -> String {
    format!(
        "no {what} host configured — set {env} to the base URL that serves {serves}. This \
         toolchain ships no default, so nothing is fetched from a third party unless you name it."
    )
}

pub(crate) fn world_base() -> Option<String> {
    configured_base(&[WORLD_BASE_ENV])
}

pub(crate) fn world_base_hint() -> String {
    unconfigured_host_hint("worlds", WORLD_BASE_ENV, "/<world>/about")
}

pub(crate) fn catalyst_base() -> String {
    configured_base(&CATALYST_ENV).unwrap_or_else(|| DEFAULT_CATALYST.to_string())
}

/// The primary upstream plus two fallbacks, so a timeout or 5xx on one catalyst
/// does not strand wearable/profile fetches. Only a rotation someone named is
/// used: unlike `deploy`, a preview must not source a realm unasked.
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

fn axum_status(status: reqwest::StatusCode) -> StatusCode {
    StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY)
}

fn content_type_or(resp: &reqwest::Response, default: &str) -> String {
    resp.headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or(default)
        .to_string()
}

/// Serve `hash` from the on-disk LRU if it is there. Shared by the catalyst
/// back-fill and the world mirror, which cache into the same directory.
async fn cached_content(
    dir: &std::path::Path,
    hash: &str,
    method: &Method,
    label: &str,
) -> Option<Response> {
    let (bytes, ct) = super::content_cache::get(dir, hash).await?;
    let ct = ct.unwrap_or_else(|| "application/octet-stream".to_string());
    tracing::info!(target: "access", "{label} {hash} 200 dcl-cache sent={}", bytes.len());
    let resp_headers = [
        (header::CONTENT_TYPE, ct),
        (header::CACHE_CONTROL, IMMUTABLE.to_string()),
        (header::CONTENT_LENGTH, bytes.len().to_string()),
    ];
    Some(match *method == Method::HEAD {
        true => (resp_headers, axum::body::Body::empty()).into_response(),
        false => (resp_headers, bytes).into_response(),
    })
}

/// Forward a request and hand back status + content type + body unchanged, for
/// routes that exist only to lift a fetch onto the preview origin (CORS).
pub(super) async fn passthrough(method: Method, url: &str) -> Response {
    let client = match proxy_client() {
        Ok(c) => c,
        Err(resp) => return resp,
    };
    match client.request(method, url).send().await {
        Ok(resp) => {
            let status = axum_status(resp.status());
            let ct = content_type_or(&resp, "application/octet-stream");
            match resp.bytes().await {
                Ok(body) => (status, [(header::CONTENT_TYPE, ct)], body).into_response(),
                Err(e) => (StatusCode::BAD_GATEWAY, format!("{url}: {e}")).into_response(),
            }
        }
        Err(e) => (StatusCode::BAD_GATEWAY, format!("{url}: {e}")).into_response(),
    }
}

/// `{realm}/optimized-assets/*` — the path the explorer derives from the realm
/// under `local-ab=true`, pinned by `RealmLaunchSettings.OPTIMIZED_ASSETS_PATH`
/// and carrying no URL or port of its own. Serving it here keeps the client on
/// the realm it already has: no extra port in the deep link, no second firewall
/// approval on the LAN. The sidecar still does the work.
pub(super) async fn optimized_assets(
    method: Method,
    axum::extract::State(st): axum::extract::State<std::sync::Arc<super::AppState>>,
    axum::extract::Path(path): axum::extract::Path<String>,
    raw_query: axum::extract::RawQuery,
) -> Response {
    let Some(base) = st.optimized_assets_url.get() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "asset-bundle sidecar is not running (--no-asset-bundles?)",
        )
            .into_response();
    };
    let query = raw_query.0.map(|q| format!("?{q}")).unwrap_or_default();
    passthrough(method, &format!("{base}/{path}{query}")).await
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

fn complained_bases() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    static SEEN: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
        std::sync::OnceLock::new();
    SEEN.get_or_init(Default::default)
}

/// Once per base, not once per request: the explorer re-asks for profiles and
/// wearables continuously, so a per-request warning buried everything else.
fn note_upstream_unreachable(base: &str) {
    let mut seen = complained_bases().lock().unwrap_or_else(|e| e.into_inner());
    if !seen.insert(base.to_string()) {
        return;
    }
    crate::ux::note_stderr(format!(
        "catalyst {base} is unreachable — avatars, wearables and profiles will not load until it \
         answers. Point DCL_ONE_SDK_CATALYST at one that does (the default is \
         {DEFAULT_CATALYST}). Re-run with --verbose for the per-request detail. The scene itself \
         is unaffected."
    ));
}

/// Send to the catalyst, walking the fallback rotation on connect errors and
/// 5xx. Idempotent methods retry; anything else goes to the primary only.
async fn forward_to_catalyst(
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
                let status = axum_status(resp.status());
                let content_type = content_type_or(&resp, "application/binary");
                let bytes = resp.bytes().await.unwrap_or_default();
                if status.is_server_error() && i + 1 < candidates.len() {
                    tracing::warn!("catalyst proxy {url}: {status}");
                    last_5xx = Some((status, content_type, bytes));
                    continue;
                }
                return Ok((status, content_type, bytes));
            }
            Err(e) => {
                note_upstream_unreachable(base);
                tracing::info!("catalyst proxy {url}: {e}");
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

/// Backup fetch: content hashes the local scene does not own (wearable GLBs,
/// emotes, profile snapshots) come from the upstream catalyst so the explorer
/// can render avatars in preview. Successful GETs land in the scene's
/// `.dcl-cache` LRU, which the next session serves offline.
pub(super) async fn contents_upstream(
    method: Method,
    hash: &str,
    headers: &HeaderMap,
    cache_dir: Option<&std::path::Path>,
) -> Response {
    if let Some(dir) = cache_dir {
        if let Some(hit) = cached_content(dir, hash, &method, "contents").await {
            return hit;
        }
    }
    match forward_to_catalyst(
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

/// Upstream's `router.all('/lambdas/:path+')`, `router.all('/explorer/:path+')`
/// and `router.post('/content/entities')` — the last carries the client's own
/// deploys (profile publication from the onboarding lobby) through the realm.
pub(super) async fn catalyst_proxy(req: Request) -> Response {
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
    match forward_to_catalyst(method, path_and_query, &headers, body).await {
        Ok((status, ct, bytes)) => (status, [(header::CONTENT_TYPE, ct)], bytes).into_response(),
        Err(resp) => resp,
    }
}

/// World name (lowercased) -> candidate upstream contents prefixes, best first,
/// learned from the world's own /about and re-derivable by refetching it.
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
    let base = world_base()
        .ok_or_else(|| (StatusCode::NOT_IMPLEMENTED, world_base_hint()).into_response())?;
    let url = format!("{base}/{name}/about");
    match client.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => {
            resp.json::<serde_json::Value>().await.map_err(|e| {
                (StatusCode::BAD_GATEWAY, format!("world about {url}: {e}")).into_response()
            })
        }
        Ok(resp) => {
            let status = axum_status(resp.status());
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

/// Every place the world's content might be served from, best guess first. Not
/// just the urn's own baseUrl: a federated deployment can advertise a worlds
/// host that does not expose `/contents/`, while the catalyst that proxied the
/// /about has the entity synced. Hashes are immutable, so any host answering
/// 200 answers correctly.
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
/// portable world without the CORS wall around the public worlds host. Content
/// moves to `/world-content/…`, which the permissive CORS layer already covers.
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
    rewrite_scenes_urn(
        &mut about,
        &format!(
            "{}/world-content/{name}/contents/",
            preview_origin(&headers)
        ),
    );
    Json(about).into_response()
}

/// Content half of the world mirror: immutable hashes, so hits land in the same
/// `.dcl-cache` LRU the catalyst back-fill uses and never revalidate.
pub(super) async fn world_content(
    method: Method,
    axum::extract::State(st): axum::extract::State<std::sync::Arc<super::AppState>>,
    axum::extract::Path((name, hash)): axum::extract::Path<(String, String)>,
) -> Response {
    if !valid_world_name(&name) || !hash.chars().all(|c| c.is_ascii_alphanumeric()) {
        return (StatusCode::BAD_REQUEST, "invalid world content path").into_response();
    }
    let cache_dir = contents_cache_dir(&st);
    if let Some(dir) = cache_dir.as_deref() {
        if let Some(hit) = cached_content(dir, &hash, &method, "world-content").await {
            return hit;
        }
    }
    let cached = world_upstreams().get(&name.to_ascii_lowercase()).cloned();
    let candidates = match cached {
        Some(c) => c,
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
                let ct = content_type_or(&resp, "application/octet-stream");
                let bytes = resp.bytes().await.unwrap_or_default();
                if method == Method::GET {
                    if let Some(dir) = cache_dir.as_deref() {
                        super::content_cache::put(dir, &hash, &bytes, Some(&ct)).await;
                    }
                }
                if i > 0 {
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
                let status = axum_status(resp.status());
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
    fn an_unreachable_upstream_is_explained_once_per_base_not_once_per_request() {
        let base = "http://127.0.0.1:59999";
        complained_bases().lock().unwrap().remove(&base.to_string());
        note_upstream_unreachable(base);
        assert!(complained_bases().lock().unwrap().contains(base));
        note_upstream_unreachable(base);
        note_upstream_unreachable(base);
        let other = "http://127.0.0.1:59998";
        complained_bases()
            .lock()
            .unwrap()
            .remove(&other.to_string());
        note_upstream_unreachable(other);
        let seen = complained_bases().lock().unwrap();
        assert!(seen.contains(base) && seen.contains(other));
    }

    #[test]
    fn the_unconfigured_default_is_a_reachable_catalyst_and_both_env_names_win() {
        std::env::remove_var("DCL_ONE_SDK_CATALYST");
        std::env::remove_var("DCL_CATALYST");
        assert_eq!(catalyst_base(), DEFAULT_CATALYST);
        assert!(DEFAULT_CATALYST.starts_with("https://"));

        std::env::set_var("DCL_CATALYST", "https://peer.decentraland.org/");
        assert_eq!(catalyst_base(), "https://peer.decentraland.org");
        std::env::set_var("DCL_ONE_SDK_CATALYST", "http://127.0.0.1:5141");
        assert_eq!(catalyst_base(), "http://127.0.0.1:5141");
        std::env::remove_var("DCL_ONE_SDK_CATALYST");
        std::env::remove_var("DCL_CATALYST");
    }

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
        match world_base().as_deref().and_then(origin_of) {
            Some(origin) => assert!(candidates.contains(&format!("{origin}/content/contents/"))),
            None => assert!(
                candidates
                    .iter()
                    .all(|c| c.starts_with("https://worlds.example")
                        || c.starts_with("https://peer.example")),
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
