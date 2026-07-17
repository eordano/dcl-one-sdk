//! Catalyst proxy routes, ported from upstream sdk-commands
//! `start/server/endpoints.ts`: the explorer talks to the preview realm's
//! lambdas/content for profiles and profile deploys, so a local realm must
//! forward what it does not serve itself. Without these the v0.158+ desktop
//! client clears a cached identity on boot (profile fetch 404s → treated as
//! abandoned onboarding) and the new-account lobby's profile deploy fails.

use axum::body::Bytes;
use axum::extract::Request;
use axum::http::{header, HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use serde_json::json;

use super::http::preview_host;

const DEFAULT_CATALYST: &str = "https://interconnected.online";

pub(crate) fn catalyst_base() -> String {
    match std::env::var("DCL_ONE_SDK_CATALYST") {
        Ok(v) if !v.is_empty() => v.trim_end_matches('/').to_string(),
        _ => DEFAULT_CATALYST.to_string(),
    }
}

/// The primary upstream plus two fallbacks from the deploy rotation, so a
/// timeout or 5xx on one catalyst does not strand wearable/profile fetches.
/// An explicit DCL_ONE_SDK_CATALYST is respected first but still falls back.
fn upstream_candidates() -> Vec<String> {
    let primary = catalyst_base();
    let mut out = vec![primary.clone()];
    out.extend(
        crate::deploy::CATALYST_ROTATION
            .iter()
            .map(|c| c.to_string())
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
