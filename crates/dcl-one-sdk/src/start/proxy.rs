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

const DEFAULT_CATALYST: &str = "https://peer.decentraland.org";

fn catalyst_base() -> String {
    match std::env::var("DCL_ONE_SDK_CATALYST") {
        Ok(v) if !v.is_empty() => v.trim_end_matches('/').to_string(),
        _ => DEFAULT_CATALYST.to_string(),
    }
}

fn proxy_client() -> Result<reqwest::Client, Response> {
    reqwest::Client::builder()
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

async fn forward(
    method: Method,
    upstream_path_and_query: String,
    headers: &HeaderMap,
    body: Bytes,
) -> Response {
    let url = format!("{}{}", catalyst_base(), upstream_path_and_query);
    let client = match proxy_client() {
        Ok(c) => c,
        Err(resp) => return resp,
    };
    let mut req = client.request(method.clone(), &url);
    if let Some(ct) = headers.get(header::CONTENT_TYPE) {
        req = req.header(header::CONTENT_TYPE, ct);
    }
    if method != Method::GET && method != Method::HEAD {
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
            (
                StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
                [(header::CONTENT_TYPE, content_type)],
                bytes,
            )
                .into_response()
        }
        Err(e) => {
            tracing::warn!("catalyst proxy {url}: {e}");
            (StatusCode::BAD_GATEWAY, format!("catalyst proxy: {e}")).into_response()
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
