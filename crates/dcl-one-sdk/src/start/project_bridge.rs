use super::{forwarded_prefix, AppState};
use axum::{
    extract::{ConnectInfo, DefaultBodyLimit, Query, Request, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    net::SocketAddr,
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
};

const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_FILES: usize = 4096;
static WRITER: Mutex<()> = Mutex::new(());
type Failure = (StatusCode, String);

pub(super) fn creator_url(realm: &str) -> String {
    let configured = std::env::var("DCL_ONE_SDK_CREATOR_HUB_URL").ok();
    let mut url = configured
        .as_deref()
        .and_then(|value| url::Url::parse(value).ok())
        .filter(|url| matches!(url.scheme(), "http" | "https"))
        .unwrap_or_else(|| {
            url::Url::parse("https://catalyst.example.com/create").expect("static URL")
        });
    url.query_pairs_mut().append_pair("projectUrl", realm);
    url.to_string()
}

pub(super) fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/project", get(info))
        .route("/api/project/files", get(files))
        .route("/api/project/file", get(read).put(write).delete(delete))
        .merge(super::ui_designer::routes())
        .merge(super::assistant::routes())
        .merge(super::project_assets::routes())
        .merge(super::external_debug::routes())
        .layer(DefaultBodyLimit::max(MAX_BYTES * 2))
        .layer(middleware::from_fn(access))
        .with_state(state)
}

fn allowed(headers: &HeaderMap, peer: SocketAddr, origins: &[String]) -> bool {
    if super::remote_peer(false, peer, headers) {
        return false;
    }
    if let Some(forwarded) = headers.get("x-forwarded-for") {
        if !forwarded.to_str().is_ok_and(|s| {
            s.split(',').all(|ip| {
                ip.trim()
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
            })
        }) {
            return false;
        }
    }
    let origin = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok());
    if let Some(origin) = origin {
        let Ok(url) = url::Url::parse(origin) else {
            return false;
        };
        if !matches!(url.scheme(), "http" | "https") || url.origin().ascii_serialization() != origin
        {
            return false;
        }
        if origins.iter().any(|a| a == origin) {
            return true;
        }
    }
    let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let Ok(local) = url::Url::parse(&format!("http://{host}")) else {
        return false;
    };
    let loopback = match local.host() {
        Some(url::Host::Domain("localhost")) => true,
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        _ => false,
    };
    loopback
        && match origin {
            Some(origin) => url::Url::parse(origin).is_ok_and(|u| {
                u.host() == local.host()
                    && u.port_or_known_default() == local.port_or_known_default()
            }),
            None => !headers
                .get("sec-fetch-site")
                .is_some_and(|s| s != "none" && s != "same-origin"),
        }
}

async fn access(req: Request, next: Next) -> Response {
    let peer = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|p| p.0);
    if !peer.is_some_and(|p| allowed(req.headers(), p, &super::allowed_editor_origins())) {
        return (StatusCode::FORBIDDEN, "Connect from the SDK machine and allow the Creator Hub origin with DCL_ONE_SDK_ALLOWED_ORIGINS").into_response();
    }
    let origin = req.headers().get(header::ORIGIN).cloned();
    let preflight = req.method() == Method::OPTIONS;
    let mut response = if preflight {
        StatusCode::NO_CONTENT.into_response()
    } else {
        next.run(req).await
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
        .headers_mut()
        .insert(header::VARY, HeaderValue::from_static("Origin"));
    response.headers_mut().insert(
        "cross-origin-resource-policy",
        HeaderValue::from_static("cross-origin"),
    );
    if let Some(origin) = origin {
        response
            .headers_mut()
            .insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
        response.headers_mut().insert(
            header::ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("GET, HEAD, PUT, POST, DELETE, OPTIONS"),
        );
        response.headers_mut().insert(
            header::ACCESS_CONTROL_ALLOW_HEADERS,
            HeaderValue::from_static("content-type, if-match, if-none-match"),
        );
        response.headers_mut().insert(
            header::ACCESS_CONTROL_EXPOSE_HEADERS,
            HeaderValue::from_static("etag, content-length"),
        );
        if preflight {
            response.headers_mut().insert(
                "access-control-allow-private-network",
                HeaderValue::from_static("true"),
            );
        }
    }
    response
}

async fn info(State(st): State<Arc<AppState>>, headers: HeaderMap) -> Result<Json<Value>, Failure> {
    let project = single_project(&st)?;
    let prefix = forwarded_prefix(&headers);
    let origin = super::http::preview_origin(&headers);
    let ws = super::http::preview_ws_origin(&headers);
    let designer = super::ui_designer::available(&st);
    let composite = safe_path(&project.root, "main.composite")
        .ok()
        .filter(|p| p.is_file())
        .map(|_| "main.composite");
    Ok(Json(json!({
        "version": 1, "name": crate::joinblock::scene_title(&project.scene_json), "scene": project.scene_json,
        "compositePath": composite,
        "capabilities": {"files":true,"write":true,"watch":st.project_watch,"dataLayer":st.data_layer.is_some(),"mcp":st.mcp},
        "links": {"files":format!("{prefix}/api/project/files"),"file":format!("{prefix}/api/project/file"),"reload":format!("{ws}/"),"preview":origin,
            "settings":format!("{prefix}/scene"),"publish":format!("{prefix}/deploy"),"storage":format!("{prefix}/storage"),
            "inspector":st.data_layer.as_ref().and_then(|dl| dl.public_dir.as_ref()).map(|_|format!("{prefix}/inspector/")),
            "uiDesigner":designer.then(||format!("{prefix}/inspector/?uiDesignerOpen=true&uiEditorEnabled=true&uiEditorSupported=true")),
            "uiDesignerRuntime":designer.then(||format!("{prefix}/api/project/ui-designer/runtime.js")),
            "dataLayer":st.data_layer.as_ref().map(|_|format!("{ws}/data-layer")),
            "mcp":st.mcp.then(|| format!("http://127.0.0.1:{}/unity-explorer-mcp",st.mcp_port))}
    })))
}

pub(super) fn single_project(st: &AppState) -> Result<crate::scene::Project, Failure> {
    let projects = st.projects();
    if projects.len() != 1 {
        return Err((StatusCode::CONFLICT, "Open one scene with dcl-one-sdk start --dir <scene>; workspace editing requires a scene selection".into()));
    }
    Ok(projects[0].clone())
}

pub(super) fn normal_path(path: &str) -> bool {
    !path.is_empty() && !path.contains('\\') && path.len() <= 1024 && Path::new(path).components().all(|c| matches!(c, Component::Normal(name) if !name.to_string_lossy().starts_with('.') && !matches!(name.to_str(), Some("node_modules"|"bin"|"dist"|"build"|"target"))))
}

fn editable(path: &str) -> bool {
    if !normal_path(path) {
        return false;
    }
    let p = Path::new(path);
    if matches!(
        path,
        "scene.json" | "package.json" | "tsconfig.json" | "main.composite"
    ) {
        return true;
    }
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
    ext == "composite"
        || (path.starts_with("src/")
            && matches!(ext, "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "json"))
}

fn removable(path: &str) -> bool {
    editable(path)
        && !matches!(
            path,
            "scene.json" | "package.json" | "tsconfig.json" | "main.composite"
        )
}

async fn delete(
    State(st): State<Arc<AppState>>,
    Query(query): Query<FileQuery>,
    headers: HeaderMap,
) -> Result<Json<Value>, Failure> {
    let project = single_project(&st)?;
    let expected =
        super::project_assets::expected(&headers, false)?.expect("delete requires revision");
    tokio::task::spawn_blocking(move || {
        delete_file(&project.root, &query.path, &expected, removable)?;
        Ok(Json(json!({"path":query.path,"deleted":true})))
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
}

fn safe_path(root: &Path, rel: &str) -> Result<PathBuf, Failure> {
    safe_path_for(root, rel, editable)
}

pub(super) fn safe_path_for(
    root: &Path,
    rel: &str,
    allowed: fn(&str) -> bool,
) -> Result<PathBuf, Failure> {
    if !allowed(rel) {
        return Err((
            StatusCode::BAD_REQUEST,
            "Only scene source, composites and project configuration can be edited".into(),
        ));
    }
    let mut path = root.to_path_buf();
    let parts: Vec<_> = Path::new(rel).components().collect();
    for (index, part) in parts.iter().enumerate() {
        path.push(part);
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err((
                    StatusCode::FORBIDDEN,
                    "Symbolic links cannot be edited".into(),
                ))
            }
            Ok(meta) if index + 1 < parts.len() && !meta.is_dir() => {
                return Err((StatusCode::BAD_REQUEST, "Parent is not a directory".into()))
            }
            Ok(meta) if index + 1 == parts.len() && !meta.is_file() => {
                return Err((StatusCode::BAD_REQUEST, "Path is not a file".into()))
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err((StatusCode::NOT_FOUND, e.to_string())),
        }
    }
    Ok(path)
}

pub(super) fn revision(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn read_file(root: &Path, rel: &str) -> Result<Value, Failure> {
    let path = safe_path(root, rel)?;
    let meta = std::fs::metadata(&path).map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;
    if meta.len() > MAX_BYTES as u64 {
        return Err((StatusCode::PAYLOAD_TOO_LARGE, "File exceeds 16 MiB".into()));
    }
    let content = std::fs::read_to_string(path)
        .map_err(|e| (StatusCode::UNPROCESSABLE_ENTITY, e.to_string()))?;
    Ok(json!({"path":rel,"revision":revision(content.as_bytes()),"content":content}))
}

#[derive(Deserialize)]
struct FileQuery {
    path: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileWrite {
    content: String,
    revision: Option<String>,
}

async fn read(
    State(st): State<Arc<AppState>>,
    Query(q): Query<FileQuery>,
) -> Result<Json<Value>, Failure> {
    let p = single_project(&st)?;
    tokio::task::spawn_blocking(move || read_file(&p.root, &q.path).map(Json))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
}

fn write_file(root: &Path, rel: &str, edit: FileWrite) -> Result<Value, Failure> {
    if edit.content.len() > MAX_BYTES {
        return Err((StatusCode::PAYLOAD_TOO_LARGE, "File exceeds 16 MiB".into()));
    }
    if matches!(
        Path::new(rel).extension().and_then(|e| e.to_str()),
        Some("json" | "composite")
    ) {
        serde_json::from_str::<Value>(&edit.content)
            .map_err(|e| (StatusCode::UNPROCESSABLE_ENTITY, e.to_string()))?;
    }
    let revision = write_bytes(
        root,
        rel,
        edit.content.as_bytes(),
        edit.revision.as_deref(),
        editable,
    )?;
    Ok(json!({"path":rel,"revision":revision,"content":edit.content}))
}

pub(super) fn write_bytes(
    root: &Path,
    rel: &str,
    bytes: &[u8],
    expected: Option<&str>,
    allowed: fn(&str) -> bool,
) -> Result<String, Failure> {
    let _guard = WRITER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let path = safe_path_for(root, rel, allowed)?;
    check_revision(&path, expected)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }
    safe_path_for(root, rel, allowed)?;
    let tmp = path.with_file_name(format!(".creator-save-{:x}.tmp", rand::random::<u64>()));
    let result = (|| -> std::io::Result<()> {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        if let Ok(meta) = std::fs::metadata(&path) {
            f.set_permissions(meta.permissions())?;
        }
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, &path)
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(tmp);
        return Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()));
    }
    Ok(revision(bytes))
}

fn check_revision(path: &Path, expected: Option<&str>) -> Result<(), Failure> {
    use std::io::Read;
    let current = match std::fs::File::open(path) {
        Ok(mut file) => {
            let mut hash = Sha256::new();
            let mut buf = [0u8; 8192];
            loop {
                let n = file
                    .read(&mut buf)
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
                if n == 0 {
                    break;
                }
                hash.update(&buf[..n]);
            }
            Some(
                hash.finalize()
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>(),
            )
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    };
    if current.as_deref() != expected {
        return Err((
            StatusCode::CONFLICT,
            "File changed on disk; reload it before saving or deleting".into(),
        ));
    }
    Ok(())
}

pub(super) fn delete_file(
    root: &Path,
    rel: &str,
    expected: &str,
    allowed: fn(&str) -> bool,
) -> Result<(), Failure> {
    let _guard = WRITER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let path = safe_path_for(root, rel, allowed)?;
    check_revision(&path, Some(expected))?;
    std::fs::remove_file(path).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn write(
    State(st): State<Arc<AppState>>,
    Query(q): Query<FileQuery>,
    Json(edit): Json<FileWrite>,
) -> Result<Json<Value>, Failure> {
    let p = single_project(&st)?;
    tokio::task::spawn_blocking(move || {
        let result = write_file(&p.root, &q.path, edit)?;
        if q.path == "scene.json" {
            st.refresh_scene_json(&p.root);
        }
        Ok(Json(result))
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
}

pub(super) fn list_matching(
    root: &Path,
    dir: &Path,
    out: &mut Vec<Value>,
    allowed: fn(&str) -> bool,
) -> Result<(), Failure> {
    for entry in
        std::fs::read_dir(dir).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    {
        let entry = entry.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.')
            || matches!(
                name.as_ref(),
                "node_modules" | "bin" | "dist" | "build" | "target"
            )
        {
            continue;
        }
        let kind = entry
            .file_type()
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        if kind.is_symlink() {
            continue;
        }
        let path = entry.path();
        if kind.is_dir() {
            list_matching(root, &path, out, allowed)?;
        } else if kind.is_file() {
            let rel = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if allowed(&rel) {
                out.push(json!({"path":rel,"size":entry.metadata().map(|m|m.len()).unwrap_or(0)}));
                if out.len() > MAX_FILES {
                    return Err((
                        StatusCode::PAYLOAD_TOO_LARGE,
                        "Project exceeds 4096 editable files".into(),
                    ));
                }
            }
        }
    }
    Ok(())
}

async fn files(State(st): State<Arc<AppState>>) -> Result<Json<Value>, Failure> {
    let p = single_project(&st)?;
    tokio::task::spawn_blocking(move || {
        let mut files = Vec::new();
        list_matching(&p.root, &p.root, &mut files, editable)?;
        files.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
        Ok(Json(json!({"files":files})))
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
}

#[cfg(test)]
#[path = "project_bridge_tests.rs"]
mod tests;
