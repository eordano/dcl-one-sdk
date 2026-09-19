use super::{project_bridge as files, AppState};
use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{path::Path, sync::Arc};
type Failure = (StatusCode, String);
const MAX_BYTES: usize = 64 * 1024 * 1024;

pub(super) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/project/assets", get(list))
        .route("/api/project/asset", get(read).put(write).delete(delete))
        .layer(DefaultBodyLimit::max(MAX_BYTES))
}

fn kind(path: &str) -> Option<&'static str> {
    if !files::normal_path(path) {
        return None;
    }
    match Path::new(path)
        .extension()?
        .to_str()?
        .to_ascii_lowercase()
        .as_str()
    {
        "glb" => Some("model/gltf-binary"),
        "gltf" => Some("model/gltf+json"),
        "bin" => Some("application/octet-stream"),
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        "ktx2" => Some("image/ktx2"),
        "mp3" => Some("audio/mpeg"),
        "ogg" => Some("audio/ogg"),
        "wav" => Some("audio/wav"),
        "mp4" => Some("video/mp4"),
        "webm" => Some("video/webm"),
        _ => None,
    }
}
fn asset(path: &str) -> bool {
    kind(path).is_some()
}

#[derive(Deserialize)]
struct AssetQuery {
    path: String,
}

async fn list(State(st): State<Arc<AppState>>) -> Result<Json<Value>, Failure> {
    let project = files::single_project(&st)?;
    tokio::task::spawn_blocking(move || {
        let mut found = Vec::new();
        files::list_matching(&project.root, &project.root, &mut found, asset)?;
        found.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
        Ok(Json(json!({"files":found})))
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
}

async fn read(
    State(st): State<Arc<AppState>>,
    Query(query): Query<AssetQuery>,
) -> Result<Response, Failure> {
    let project = files::single_project(&st)?;
    tokio::task::spawn_blocking(move || {
        use std::io::Read;
        let path = files::safe_path_for(&project.root, &query.path, asset)?;
        let file = std::fs::File::open(path).map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;
        let mut bytes = Vec::new();
        file.take((MAX_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        if bytes.len() > MAX_BYTES {
            return Err((StatusCode::PAYLOAD_TOO_LARGE, "Asset exceeds 64 MiB".into()));
        }
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static(kind(&query.path).unwrap()),
        );
        headers.insert(
            header::ETAG,
            format!("\"{}\"", files::revision(&bytes)).parse().unwrap(),
        );
        headers.insert(
            header::CONTENT_LENGTH,
            bytes.len().to_string().parse().unwrap(),
        );
        headers.insert(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        );
        Ok((headers, bytes).into_response())
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
}

pub(super) fn expected(headers: &HeaderMap, create: bool) -> Result<Option<String>, Failure> {
    if create
        && headers.get(header::IF_NONE_MATCH).is_some_and(|v| v == "*")
        && !headers.contains_key(header::IF_MATCH)
    {
        return Ok(None);
    }
    let value = headers
        .get(header::IF_MATCH)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim_matches('"'));
    if let Some(value) = value.filter(|s| s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit()))
    {
        return Ok(Some(value.to_ascii_lowercase()));
    }
    Err((
        StatusCode::PRECONDITION_REQUIRED,
        "Read the asset revision and send If-Match, or If-None-Match: * to create".into(),
    ))
}

async fn write(
    State(st): State<Arc<AppState>>,
    Query(query): Query<AssetQuery>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, Failure> {
    let project = files::single_project(&st)?;
    let expected = expected(&headers, true)?;
    if body.len() > MAX_BYTES {
        return Err((StatusCode::PAYLOAD_TOO_LARGE, "Asset exceeds 64 MiB".into()));
    }
    tokio::task::spawn_blocking(move || {
        let revision = files::write_bytes(
            &project.root,
            &query.path,
            &body,
            expected.as_deref(),
            asset,
        )?;
        Ok(Json(
            json!({"path":query.path,"revision":revision,"size":body.len()}),
        ))
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
}

async fn delete(
    State(st): State<Arc<AppState>>,
    Query(query): Query<AssetQuery>,
    headers: HeaderMap,
) -> Result<Json<Value>, Failure> {
    let project = files::single_project(&st)?;
    let expected = expected(&headers, false)?.expect("delete requires revision");
    tokio::task::spawn_blocking(move || {
        files::delete_file(&project.root, &query.path, &expected, asset)?;
        Ok(Json(json!({"path":query.path,"deleted":true})))
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
}

#[cfg(test)]
#[path = "project_assets_tests.rs"]
mod tests;
