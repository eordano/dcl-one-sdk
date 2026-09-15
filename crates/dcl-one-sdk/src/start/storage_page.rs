//! `/storage`: the scene's server-side storage as decentraland.org/storage
//! shows it — Scene, Player and Environment tabs over what `@dcl/sdk/server`
//! reads — plus the routes that storage rides on. The routes are upstream's
//! preview routes (`/values`, `/players/{address}/values`, `/env`), so the
//! host isolate's client, the `storage` CLI and this page all speak one
//! dialect, and a scene tested here behaves the same against production.
//!
//! Where the values live is a per-project switch (`crate::storage::Target`):
//! this project's SQLite file, or a storage service the preview forwards to
//! with signed-fetch headers (`crate::storage_remote`). Writes, and every
//! request while a service is the target, only work from the machine hosting
//! the preview and from this origin, like the other editing routes.

use super::chrome::esc;
use super::deploy_page::{deploy_document, post_gate, remote_notice, reply, token};
use super::{cross_origin_refusal, forwarded_prefix, remote_peer, AppState};
use crate::scene::Project;
use crate::storage::{self, Activity, Db, Entry, EnvSource, Scope, Store, Target};
use crate::storage_remote::{self as remote, Outgoing, Reply, SceneMetadata, Signer};
use axum::body::Bytes;
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Redirect, Response};
use axum::Form;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::{Arc, OnceLock};

pub(super) const SOURCE_HEADER: &str = remote::SOURCE_HEADER;
const CONFIRM_HEADER: &str = remote::CONFIRM_HEADER;
/// Rows a tab lists before it says "and more": one service-sized page;
/// the CLI pages past this.
const LISTED: usize = storage::MAX_LIMIT;
const ACTIVITY_SHOWN: usize = 40;
const SCRIPT: &str = concat!(include_str!("page_common.js"), include_str!("storage.js"));

/// The shared page styles plus this page's layout.
pub(super) fn css() -> &'static str {
    static CSS: OnceLock<String> = OnceLock::new();
    CSS.get_or_init(|| {
        format!(
            "{}{}",
            super::deploy_page::PAGE_CSS,
            include_str!("storage.css")
        )
    })
}

// ---------------------------------------------------------------- replies

/// An error body in the production service's shape: its reason phrase
/// under `error`, the explanation under `message`.
fn message(status: StatusCode, text: impl Into<String>) -> Response {
    let error = match status {
        StatusCode::BAD_REQUEST => "Bad request",
        StatusCode::NOT_FOUND => "Not Found",
        other => other.canonical_reason().unwrap_or("Error"),
    };
    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        json!({ "error": error, "message": text.into() }).to_string(),
    )
        .into_response()
}

fn json_ok(status: StatusCode, value: Value) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        value.to_string(),
    )
        .into_response()
}

fn not_found() -> Response {
    message(StatusCode::NOT_FOUND, "Value not found")
}

fn failed(e: anyhow::Error) -> Response {
    message(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"))
}

/// The service's answer, relayed status, type and bytes intact.
fn relay(reply: Reply) -> Response {
    (
        StatusCode::from_u16(reply.status).unwrap_or(StatusCode::BAD_GATEWAY),
        [(header::CONTENT_TYPE, reply.content_type)],
        reply.body,
    )
        .into_response()
}

/// Who is writing, as the request declares it: the host says `scene`, the
/// CLI `cli`, this page `ui`; anything else is plain `http`.
fn source_of(headers: &HeaderMap) -> String {
    headers
        .get(SOURCE_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| {
            !s.is_empty()
                && s.len() <= 32
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
        .unwrap_or("http")
        .to_string()
}

/// `X-Confirm-Delete-All`, the header every clear needs, read as the
/// production service reads it: present with any non-empty value.
fn confirmed(headers: &HeaderMap) -> bool {
    headers
        .get(CONFIRM_HEADER)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| !v.trim().is_empty())
}

fn unconfirmed() -> Response {
    message(
        StatusCode::BAD_REQUEST,
        "Missing required header: X-Confirm-Delete-All",
    )
}

/// The line `start` prints so nobody is surprised where a scene's values
/// go: the target the project remembers, who signs when it is a service,
/// and the page that switches it.
pub(super) fn start_line(target: &Target, signer: Option<&Signer>, base: &str) -> String {
    let base = base.trim_end_matches('/');
    let what = match target {
        Target::Local => format!("local SQLite (.dcl-one/{})", storage::DB_FILE),
        service => match signer {
            Some(signer) => format!("{}, signed by {}", service.label(), signer.describe()),
            None => format!(
                "{}, unsigned until DCL_PRIVATE_KEY is set or a wallet connects on {base}/deploy",
                service.label()
            ),
        },
    };
    format!("Storage: {what}; switch at {base}/storage")
}

// ---------------------------------------------------------------- context

struct Ctx {
    project: Project,
    db: Db,
    target: Target,
}

fn open_ctx(st: &AppState) -> Result<Ctx, Response> {
    let Some(project) = st.first_project() else {
        return Err(message(
            StatusCode::SERVICE_UNAVAILABLE,
            "no scene is loaded, so there is no storage to serve",
        ));
    };
    let db = storage::open(&project.root).map_err(failed)?;
    let target = db.target().map_err(failed)?;
    Ok(Ctx {
        project,
        db,
        target,
    })
}

/// The write gate the editing routes share, with no remote escape: storage
/// is the developer's data, and a service target signs as their wallet.
fn gate(peer: SocketAddr, headers: &HeaderMap) -> Option<Response> {
    if remote_peer(false, peer, headers) {
        return Some(message(
            StatusCode::FORBIDDEN,
            "storage edits run only on the machine hosting this preview",
        ));
    }
    cross_origin_refusal(headers).map(|why| message(StatusCode::FORBIDDEN, why))
}

/// Who signs forwarded requests: the environment's key, else the session the
/// header's Connect-with-DCL flow delegated.
pub(super) fn signer(st: &AppState) -> Option<Signer> {
    Signer::from_env().or_else(|| super::deploy_page::live_identity(st).map(Signer::Delegated))
}

fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| remote::client().expect("building the storage client"))
}

/// Every storage route starts here. A local target hands the context back;
/// a service target answers the request itself (forwarded and signed), and a
/// refused one answers with the refusal — both as `Err`, the handler's reply.
async fn route(
    st: &AppState,
    peer: SocketAddr,
    headers: &HeaderMap,
    uri: &Uri,
    method: &str,
    body: Option<Bytes>,
    mutating: bool,
) -> Result<Ctx, Response> {
    let ctx = open_ctx(st)?;
    if mutating || !ctx.target.is_local() {
        if let Some(refused) = gate(peer, headers) {
            return Err(refused);
        }
    }
    let Some(base) = ctx.target.url() else {
        return Ok(ctx);
    };
    let metadata = SceneMetadata::from_scene_json(&ctx.project.scene_json);
    let source = source_of(headers);
    let out = Outgoing {
        method,
        path: uri.path(),
        query: uri.query(),
        body: body.map(|b| b.to_vec()),
        confirm_all: confirmed(headers),
        source: &source,
    };
    Err(
        match remote::forward(client(), base, signer(st).as_ref(), &metadata, out).await {
            Ok(reply) => relay(reply),
            Err(e) => message(StatusCode::BAD_GATEWAY, format!("{e:#}")),
        },
    )
}

// ---------------------------------------------------------------- values

#[derive(serde::Deserialize, Default)]
pub(super) struct ListQuery {
    pub(super) prefix: Option<String>,
    pub(super) limit: Option<usize>,
    pub(super) offset: Option<usize>,
}

fn get_value(ctx: &Ctx, scope: Scope<'_>, key: &str) -> Response {
    if let Err(why) = storage::check_key(key) {
        return message(StatusCode::BAD_REQUEST, why);
    }
    match ctx.db.get(scope, key) {
        Ok(Some(value)) => json_ok(StatusCode::OK, json!({ "value": value })),
        Ok(None) => not_found(),
        Err(e) => failed(e),
    }
}

/// The `{ value }` body every PUT carries, within the scope's per-value
/// rules; `env` values must be strings.
fn parse_body(body: &Bytes, scope: Scope<'_>) -> Result<Value, Response> {
    let parsed: Value = serde_json::from_slice(body)
        .map_err(|e| message(StatusCode::BAD_REQUEST, format!("Body is not JSON: {e}")))?;
    let Some(value) = parsed.get("value") else {
        return Err(message(
            StatusCode::BAD_REQUEST,
            "Body must be a JSON object with a \"value\" field",
        ));
    };
    if matches!(scope, Scope::Env) && !value.is_string() {
        return Err(message(
            StatusCode::BAD_REQUEST,
            "Environment variables are strings",
        ));
    }
    if let Err(why) = storage::check_value(scope, value) {
        return Err(message(StatusCode::BAD_REQUEST, why));
    }
    Ok(value.clone())
}

fn put_value(
    ctx: &Ctx,
    scope: Scope<'_>,
    key: &str,
    body: &Bytes,
    headers: &HeaderMap,
) -> Response {
    if let Err(why) = storage::check_key(key) {
        return message(StatusCode::BAD_REQUEST, why);
    }
    let env = matches!(scope, Scope::Env);
    let value = match parse_body(body, scope) {
        Ok(v) => v,
        Err(r) => return r,
    };
    match ctx.db.check_fits(scope, key, storage::value_size(&value)) {
        Ok(Ok(())) => {}
        Ok(Err(why)) => return message(StatusCode::BAD_REQUEST, why),
        Err(e) => return failed(e),
    }
    match ctx.db.set(scope, key, &value, &source_of(headers)) {
        Ok(()) if env => StatusCode::NO_CONTENT.into_response(),
        Ok(()) => json_ok(StatusCode::OK, json!({ "value": value })),
        Err(e) => failed(e),
    }
}

fn delete_value(ctx: &Ctx, scope: Scope<'_>, key: &str, headers: &HeaderMap) -> Response {
    if let Err(why) = storage::check_key(key) {
        return message(StatusCode::BAD_REQUEST, why);
    }
    match ctx.db.delete(scope, key, &source_of(headers)) {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => failed(e),
    }
}

fn list_values(ctx: &Ctx, scope: Scope<'_>, q: &ListQuery) -> Response {
    match ctx.db.list(
        scope,
        q.prefix.as_deref(),
        storage::page_limit(q.limit),
        q.offset.unwrap_or(0),
    ) {
        Ok(page) => json_ok(StatusCode::OK, page.to_json()),
        Err(e) => failed(e),
    }
}

fn clear_values(ctx: &Ctx, scope: Scope<'_>, headers: &HeaderMap) -> Response {
    if !confirmed(headers) {
        return unconfirmed();
    }
    match ctx.db.clear(scope, &source_of(headers)) {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => failed(e),
    }
}

fn player_scope(address: &str) -> Result<String, Response> {
    storage::normalize_address(address).map_err(|why| message(StatusCode::BAD_REQUEST, why))
}

// The handlers: one per upstream route, each a `route` call and one of the
// shared operations above.

pub(super) async fn scene_list(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    uri: Uri,
    Query(q): Query<ListQuery>,
) -> Response {
    match route(&st, peer, &headers, &uri, "GET", None, false).await {
        Ok(ctx) => list_values(&ctx, Scope::Scene, &q),
        Err(r) => r,
    }
}

pub(super) async fn scene_clear(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    match route(&st, peer, &headers, &uri, "DELETE", None, true).await {
        Ok(ctx) => clear_values(&ctx, Scope::Scene, &headers),
        Err(r) => r,
    }
}

pub(super) async fn scene_get(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    uri: Uri,
    Path(key): Path<String>,
) -> Response {
    match route(&st, peer, &headers, &uri, "GET", None, false).await {
        Ok(ctx) => get_value(&ctx, Scope::Scene, &key),
        Err(r) => r,
    }
}

pub(super) async fn scene_put(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    uri: Uri,
    Path(key): Path<String>,
    body: Bytes,
) -> Response {
    match route(&st, peer, &headers, &uri, "PUT", Some(body.clone()), true).await {
        Ok(ctx) => put_value(&ctx, Scope::Scene, &key, &body, &headers),
        Err(r) => r,
    }
}

pub(super) async fn scene_delete(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    uri: Uri,
    Path(key): Path<String>,
) -> Response {
    match route(&st, peer, &headers, &uri, "DELETE", None, true).await {
        Ok(ctx) => delete_value(&ctx, Scope::Scene, &key, &headers),
        Err(r) => r,
    }
}

/// `GET /players`: the addresses with values, a page of address strings
/// as the production service lists them.
pub(super) async fn players(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    uri: Uri,
    Query(q): Query<ListQuery>,
) -> Response {
    match route(&st, peer, &headers, &uri, "GET", None, false).await {
        Ok(ctx) => {
            let limit = storage::page_limit(q.limit);
            let offset = q.offset.unwrap_or(0);
            match ctx.db.player_addresses(limit, offset) {
                Ok((data, total)) => json_ok(
                    StatusCode::OK,
                    json!({ "data": data, "pagination": { "limit": limit, "offset": offset, "total": total } }),
                ),
                Err(e) => failed(e),
            }
        }
        Err(r) => r,
    }
}

/// `DELETE /players`: every player's values, behind the confirm header.
pub(super) async fn players_clear(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    match route(&st, peer, &headers, &uri, "DELETE", None, true).await {
        Ok(ctx) => {
            if !confirmed(&headers) {
                return unconfirmed();
            }
            match ctx.db.clear_all_players(&source_of(&headers)) {
                Ok(_) => StatusCode::NO_CONTENT.into_response(),
                Err(e) => failed(e),
            }
        }
        Err(r) => r,
    }
}

fn usage_of(ctx: &Ctx, scope: Scope<'_>) -> Response {
    match ctx.db.usage(scope) {
        Ok(used) => json_ok(
            StatusCode::OK,
            json!({ "usedBytes": used, "maxTotalSizeBytes": scope.limits().1 }),
        ),
        Err(e) => failed(e),
    }
}

/// `GET /usage/world`, `/usage/players/{address}`, `/usage/env`: bytes
/// used against the scope's ceiling, as the production service reports.
pub(super) async fn usage_world(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    match route(&st, peer, &headers, &uri, "GET", None, false).await {
        Ok(ctx) => usage_of(&ctx, Scope::Scene),
        Err(r) => r,
    }
}

pub(super) async fn usage_player(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    uri: Uri,
    Path(address): Path<String>,
) -> Response {
    match route(&st, peer, &headers, &uri, "GET", None, false).await {
        Ok(ctx) => match player_scope(&address) {
            Ok(address) => usage_of(&ctx, Scope::Player(&address)),
            Err(r) => r,
        },
        Err(r) => r,
    }
}

pub(super) async fn usage_env(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    match route(&st, peer, &headers, &uri, "GET", None, false).await {
        Ok(ctx) => usage_of(&ctx, Scope::Env),
        Err(r) => r,
    }
}

pub(super) async fn player_list(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    uri: Uri,
    Path(address): Path<String>,
    Query(q): Query<ListQuery>,
) -> Response {
    match route(&st, peer, &headers, &uri, "GET", None, false).await {
        Ok(ctx) => match player_scope(&address) {
            Ok(address) => list_values(&ctx, Scope::Player(&address), &q),
            Err(r) => r,
        },
        Err(r) => r,
    }
}

pub(super) async fn player_clear(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    uri: Uri,
    Path(address): Path<String>,
) -> Response {
    match route(&st, peer, &headers, &uri, "DELETE", None, true).await {
        Ok(ctx) => match player_scope(&address) {
            Ok(address) => clear_values(&ctx, Scope::Player(&address), &headers),
            Err(r) => r,
        },
        Err(r) => r,
    }
}

pub(super) async fn player_get(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    uri: Uri,
    Path((address, key)): Path<(String, String)>,
) -> Response {
    match route(&st, peer, &headers, &uri, "GET", None, false).await {
        Ok(ctx) => match player_scope(&address) {
            Ok(address) => get_value(&ctx, Scope::Player(&address), &key),
            Err(r) => r,
        },
        Err(r) => r,
    }
}

pub(super) async fn player_put(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    uri: Uri,
    Path((address, key)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    match route(&st, peer, &headers, &uri, "PUT", Some(body.clone()), true).await {
        Ok(ctx) => match player_scope(&address) {
            Ok(address) => put_value(&ctx, Scope::Player(&address), &key, &body, &headers),
            Err(r) => r,
        },
        Err(r) => r,
    }
}

pub(super) async fn player_delete(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    uri: Uri,
    Path((address, key)): Path<(String, String)>,
) -> Response {
    match route(&st, peer, &headers, &uri, "DELETE", None, true).await {
        Ok(ctx) => match player_scope(&address) {
            Ok(address) => delete_value(&ctx, Scope::Player(&address), &key, &headers),
            Err(r) => r,
        },
        Err(r) => r,
    }
}

/// `GET /env`: the names of every variable a scene could read (runtime
/// overrides and the scene's `.env` alike), a page of key strings as the
/// production service lists them; values stay behind `GET /env/{key}`.
pub(super) async fn env_list(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    uri: Uri,
    Query(q): Query<ListQuery>,
) -> Response {
    match route(&st, peer, &headers, &uri, "GET", None, false).await {
        Ok(ctx) => match storage::env_entries(&ctx.project.root, &ctx.db) {
            Ok(entries) => {
                let prefix = q.prefix.as_deref().unwrap_or_default();
                let limit = storage::page_limit(q.limit);
                let offset = q.offset.unwrap_or(0);
                let keys: Vec<&str> = entries
                    .iter()
                    .map(|e| e.key.as_str())
                    .filter(|k| k.starts_with(prefix))
                    .collect();
                let data: Vec<&str> = keys.iter().skip(offset).take(limit).copied().collect();
                json_ok(
                    StatusCode::OK,
                    json!({ "data": data, "pagination": { "limit": limit, "offset": offset, "total": keys.len() } }),
                )
            }
            Err(e) => failed(e),
        },
        Err(r) => r,
    }
}

pub(super) async fn env_clear(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    match route(&st, peer, &headers, &uri, "DELETE", None, true).await {
        Ok(ctx) => clear_values(&ctx, Scope::Env, &headers),
        Err(r) => r,
    }
}

/// `GET /env/{key}`: the runtime override, else the scene's `.env`.
pub(super) async fn env_get(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    uri: Uri,
    Path(key): Path<String>,
) -> Response {
    match route(&st, peer, &headers, &uri, "GET", None, false).await {
        Ok(ctx) => {
            if let Err(why) = storage::check_key(&key) {
                return message(StatusCode::BAD_REQUEST, why);
            }
            match storage::env_value(&ctx.project.root, &ctx.db, &key) {
                Ok(Some(value)) => json_ok(StatusCode::OK, json!({ "value": value })),
                Ok(None) => not_found(),
                Err(e) => failed(e),
            }
        }
        Err(r) => r,
    }
}

pub(super) async fn env_put(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    uri: Uri,
    Path(key): Path<String>,
    body: Bytes,
) -> Response {
    match route(&st, peer, &headers, &uri, "PUT", Some(body.clone()), true).await {
        Ok(ctx) => put_value(&ctx, Scope::Env, &key, &body, &headers),
        Err(r) => r,
    }
}

pub(super) async fn env_delete(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    uri: Uri,
    Path(key): Path<String>,
) -> Response {
    match route(&st, peer, &headers, &uri, "DELETE", None, true).await {
        Ok(ctx) => delete_value(&ctx, Scope::Env, &key, &headers),
        Err(r) => r,
    }
}

// ---------------------------------------------------------------- snapshots

/// `GET /storage/export`: the local database as upstream's
/// `server-storage.json`, so it drops into an sdk-commands preview as-is.
pub(super) async fn export(State(st): State<Arc<AppState>>) -> Response {
    let ctx = match open_ctx(&st) {
        Ok(ctx) => ctx,
        Err(r) => return r,
    };
    if !ctx.target.is_local() {
        return message(
            StatusCode::CONFLICT,
            "export reads the local database; switch storage back to local first",
        );
    }
    match ctx.db.export() {
        Ok(store) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "application/json".to_string()),
                (
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=\"server-storage.json\"".to_string(),
                ),
            ],
            serde_json::to_string_pretty(&store).unwrap_or_default(),
        )
            .into_response(),
        Err(e) => failed(e),
    }
}

#[derive(serde::Deserialize, Default)]
pub(super) struct ImportQuery {
    #[serde(default)]
    pub(super) merge: Option<String>,
}

/// `POST /storage/import[?merge=1]`: a snapshot in either the upstream or the
/// legacy shape replaces the local database, or with `merge` lays over it.
pub(super) async fn import(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(q): Query<ImportQuery>,
    body: Bytes,
) -> Response {
    if let Some(refused) = gate(peer, &headers) {
        return refused;
    }
    let ctx = match open_ctx(&st) {
        Ok(ctx) => ctx,
        Err(r) => return r,
    };
    if !ctx.target.is_local() {
        return message(
            StatusCode::CONFLICT,
            "import writes the local database; switch storage back to local first",
        );
    }
    let store = match serde_json::from_slice::<Value>(&body)
        .map_err(|e| format!("the snapshot is not JSON: {e}"))
        .and_then(|v| Store::from_value(v).map_err(|e| format!("{e:#}")))
    {
        Ok(store) => store,
        Err(why) => return message(StatusCode::BAD_REQUEST, why),
    };
    let merge = q
        .merge
        .as_deref()
        .is_some_and(|m| !matches!(m, "" | "0" | "false"));
    if let Err(e) = ctx.db.import(&store, merge, &source_of(&headers)) {
        return failed(e);
    }
    match ctx.db.counts() {
        Ok((scene, player, env)) => json_ok(
            StatusCode::OK,
            json!({ "merged": merge, "counts": { "scene": scene, "player": player, "env": env } }),
        ),
        Err(e) => failed(e),
    }
}

// ---------------------------------------------------------------- target

#[derive(serde::Deserialize)]
pub(super) struct TargetForm {
    pub(super) token: String,
    #[serde(default)]
    pub(super) upstream: Option<String>,
    #[serde(default)]
    pub(super) service: Option<String>,
    #[serde(default)]
    pub(super) url: Option<String>,
    #[serde(default)]
    pub(super) tab: Option<String>,
}

/// `POST /storage/target`: the switch the page draws. Off is the local
/// database; on names the public service (org or zone) or a custom URL.
pub(super) async fn set_target(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(form): Form<TargetForm>,
) -> Response {
    if let Some(refused) = post_gate(
        &st,
        false,
        peer,
        &headers,
        &form.token,
        "the storage target only changes on the machine hosting this preview",
        "stale or missing token \u{2014} reload /storage and try again",
    ) {
        return refused;
    }
    let target = if form.upstream.is_none() {
        Target::Local
    } else {
        match form.service.as_deref() {
            Some("org") => Target::Org,
            Some("zone") => Target::Zone,
            Some("custom") => match Target::parse(form.url.as_deref().unwrap_or_default()) {
                Ok(Target::Local) => {
                    return reply(StatusCode::BAD_REQUEST, "a custom target needs its URL")
                }
                Ok(target) => target,
                Err(why) => return reply(StatusCode::BAD_REQUEST, &why),
            },
            _ => return reply(StatusCode::BAD_REQUEST, "pick org, zone or a custom URL"),
        }
    };
    let ctx = match open_ctx(&st) {
        Ok(ctx) => ctx,
        Err(r) => return r,
    };
    if let Err(e) = ctx.db.set_target(&target) {
        return failed(e);
    }
    let tab = form
        .tab
        .as_deref()
        .filter(|t| matches!(*t, "player" | "env"))
        .unwrap_or("scene");
    Redirect::to(&format!("{}/storage?tab={tab}", forwarded_prefix(&headers))).into_response()
}

// ---------------------------------------------------------------- the page

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Scene,
    Player,
    Env,
}

impl Tab {
    fn key(self) -> &'static str {
        match self {
            Tab::Scene => "scene",
            Tab::Player => "player",
            Tab::Env => "env",
        }
    }
}

#[derive(serde::Deserialize, Default)]
pub(super) struct PageQuery {
    pub(super) tab: Option<String>,
    pub(super) player: Option<String>,
}

/// One value as the page draws it.
struct Row {
    key: String,
    /// What the editor opens with: pretty JSON, or the plain string of a
    /// variable.
    editable: String,
    /// What the row shows: the value, shortened when long, or dots for a
    /// variable until revealed.
    shown: String,
    meta: String,
    secret: bool,
}

/// A tab's listing, wherever it came from.
struct Listing {
    rows: Vec<Row>,
    total: usize,
    /// Why the listing is short or empty when the service refused.
    problem: Option<String>,
}

impl Listing {
    fn empty() -> Self {
        Listing {
            rows: Vec::new(),
            total: 0,
            problem: None,
        }
    }
}

fn row_from_entry(e: &Entry) -> Row {
    Row {
        key: e.key.clone(),
        editable: serde_json::to_string_pretty(&e.value).unwrap_or_default(),
        shown: shown_value(&e.value),
        meta: meta_line(e.updated_at, &e.source),
        secret: false,
    }
}

fn row_from_env(key: &str, value: &str, source: Option<EnvSource>) -> Row {
    Row {
        key: key.to_string(),
        editable: value.to_string(),
        shown: "\u{2022}".repeat(8),
        meta: match source {
            Some(EnvSource::DotEnv) => "from .env".to_string(),
            Some(EnvSource::Runtime) => "runtime override".to_string(),
            None => String::new(),
        },
        secret: true,
    }
}

/// Strings verbatim, everything else pretty when short and compact and cut
/// when long; the editor still opens the whole value.
fn shown_value(value: &Value) -> String {
    const CUT: usize = 400;
    let text = match value {
        Value::String(s) => s.clone(),
        other => {
            let pretty = serde_json::to_string_pretty(other).unwrap_or_default();
            if pretty.len() <= CUT {
                return pretty;
            }
            other.to_string()
        }
    };
    if text.chars().count() <= CUT {
        return text;
    }
    let cut: String = text.chars().take(CUT).collect();
    format!("{cut}\u{2026} ({} bytes)", text.len())
}

fn meta_line(updated_at: i64, source: &str) -> String {
    if updated_at <= 0 {
        return String::new();
    }
    match source {
        "" => ago(updated_at),
        source => format!("{} \u{b7} {source}", ago(updated_at)),
    }
}

fn ago(at_ms: i64) -> String {
    let secs = ((crate::deploy::now_ms() - at_ms) / 1000).max(0);
    match secs {
        s if s < 5 => "just now".to_string(),
        s if s < 60 => format!("{s}s ago"),
        s if s < 3600 => format!("{}m ago", s / 60),
        s if s < 86_400 => format!("{}h ago", s / 3600),
        s => format!("{}d ago", s / 86_400),
    }
}

/// A service's `{ data: [{ key, value }] }`, or why it would not list.
fn listing_from_reply(reply: Result<Reply, anyhow::Error>) -> Listing {
    match reply {
        Ok(r) if r.status == 200 => {
            let body = r.json().unwrap_or(Value::Null);
            let rows: Vec<Row> = body
                .get("data")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|item| {
                    // `GET /env` and `GET /players` list names alone
                    if let Some(name) = item.as_str() {
                        return Some(Row {
                            key: name.to_string(),
                            editable: String::new(),
                            shown: "\u{2022}".repeat(8),
                            meta: "write-only on the service".to_string(),
                            secret: false,
                        });
                    }
                    let key = item.get("key")?.as_str()?;
                    let value = item.get("value").cloned().unwrap_or(Value::Null);
                    Some(Row {
                        key: key.to_string(),
                        editable: serde_json::to_string_pretty(&value).unwrap_or_default(),
                        shown: shown_value(&value),
                        meta: String::new(),
                        secret: false,
                    })
                })
                .collect();
            let total = body
                .pointer("/pagination/total")
                .and_then(Value::as_u64)
                .map(|n| n as usize)
                .unwrap_or(rows.len());
            Listing {
                rows,
                total,
                problem: None,
            }
        }
        Ok(r) => Listing {
            rows: Vec::new(),
            total: 0,
            problem: Some(format!(
                "the service answered HTTP {}: {}",
                r.status,
                r.message()
            )),
        },
        Err(e) => Listing {
            rows: Vec::new(),
            total: 0,
            problem: Some(format!("{e:#}")),
        },
    }
}

async fn fetch_listing(st: &AppState, ctx: &Ctx, path: &str, query: Option<&str>) -> Listing {
    let Some(base) = ctx.target.url() else {
        return Listing::empty();
    };
    let metadata = SceneMetadata::from_scene_json(&ctx.project.scene_json);
    let reply = remote::forward(
        client(),
        base,
        signer(st).as_ref(),
        &metadata,
        Outgoing {
            method: "GET",
            path,
            query,
            body: None,
            confirm_all: false,
            source: "ui",
        },
    )
    .await;
    listing_from_reply(reply)
}

/// `GET /storage`.
pub(super) async fn page(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(q): Query<PageQuery>,
) -> Response {
    let prefix = forwarded_prefix(&headers);
    let local = !remote_peer(false, peer, &headers);
    let Some(project) = st.first_project() else {
        return deploy_document(
            &st,
            "storage",
            &prefix,
            "storage",
            r#"<main class="dash"><section id="storage" class="sec"><div class="panel">
              <span class="note">No scene is loaded, so there is no storage to show.</span>
            </div></section></main>"#,
        );
    };
    let scene_title = crate::joinblock::scene_title(&project.scene_json);
    let ctx = match storage::open(&project.root).and_then(|db| {
        let target = db.target()?;
        Ok(Ctx {
            project: project.clone(),
            db,
            target,
        })
    }) {
        Ok(ctx) => ctx,
        Err(e) => {
            let body = format!(
                r#"<main class="dash"><section id="storage" class="sec"><div class="panel panel--warn"><h2>Storage</h2><span class="note">{}</span></div></section></main>"#,
                esc(&format!("{e:#}"))
            );
            return deploy_document(
                &st,
                &format!("storage {scene_title}"),
                &prefix,
                "storage",
                &body,
            );
        }
    };
    let tab = match q.tab.as_deref() {
        Some("player") => Tab::Player,
        Some("env") => Tab::Env,
        _ => Tab::Scene,
    };
    let player = q
        .player
        .as_deref()
        .and_then(|p| storage::normalize_address(p).ok());
    let view = gather(&st, &ctx, tab, player.as_deref()).await;
    let body = render(&st, &ctx, &prefix, local, tab, player.as_deref(), &view);
    deploy_document(
        &st,
        &format!("storage {scene_title}"),
        &prefix,
        "storage",
        &body,
    )
}

/// Everything the active tab shows.
struct View {
    listing: Listing,
    counts: Option<(usize, usize, usize)>,
    /// Who has player values, with how many when the local database says.
    players: Vec<(String, Option<usize>)>,
    activity: Vec<Activity>,
}

async fn gather(st: &AppState, ctx: &Ctx, tab: Tab, player: Option<&str>) -> View {
    if ctx.target.is_local() {
        let listing = match tab {
            Tab::Scene => local_listing(ctx.db.list(Scope::Scene, None, LISTED, 0)),
            Tab::Player => match player {
                Some(address) => {
                    local_listing(ctx.db.list(Scope::Player(address), None, LISTED, 0))
                }
                None => Listing::empty(),
            },
            Tab::Env => match storage::env_entries(&ctx.project.root, &ctx.db) {
                Ok(entries) => Listing {
                    total: entries.len(),
                    rows: entries
                        .iter()
                        .map(|e| row_from_env(&e.key, &e.value, Some(e.source)))
                        .collect(),
                    problem: None,
                },
                Err(e) => Listing {
                    rows: Vec::new(),
                    total: 0,
                    problem: Some(format!("{e:#}")),
                },
            },
        };
        return View {
            listing,
            counts: ctx.db.counts().ok(),
            players: ctx
                .db
                .players()
                .unwrap_or_default()
                .into_iter()
                .map(|(address, count)| (address, Some(count)))
                .collect(),
            activity: ctx.db.activity(ACTIVITY_SHOWN).unwrap_or_default(),
        };
    }
    let limit = format!("limit={LISTED}");
    let mut players = Vec::new();
    let listing = match tab {
        Tab::Scene => fetch_listing(st, ctx, "/values", Some(&limit)).await,
        Tab::Player => match player {
            Some(address) => {
                fetch_listing(
                    st,
                    ctx,
                    &format!("/players/{}/values", crate::deploy::encode_segment(address)),
                    Some(&limit),
                )
                .await
            }
            None => {
                // the service lists who has values, not how many each holds
                let listed = fetch_listing(st, ctx, "/players", Some(&limit)).await;
                players = listed.rows.iter().map(|r| (r.key.clone(), None)).collect();
                Listing {
                    rows: Vec::new(),
                    total: 0,
                    problem: listed.problem,
                }
            }
        },
        Tab::Env => fetch_listing(st, ctx, "/env", Some(&limit)).await,
    };
    View {
        listing,
        counts: None,
        players,
        activity: Vec::new(),
    }
}

fn local_listing(page: anyhow::Result<storage::Page>) -> Listing {
    match page {
        Ok(page) => Listing {
            rows: page.data.iter().map(row_from_entry).collect(),
            total: page.total,
            problem: None,
        },
        Err(e) => Listing {
            rows: Vec::new(),
            total: 0,
            problem: Some(format!("{e:#}")),
        },
    }
}

fn render(
    st: &AppState,
    ctx: &Ctx,
    prefix: &str,
    local: bool,
    tab: Tab,
    player: Option<&str>,
    view: &View,
) -> String {
    let notice = remote_notice(
        local,
        "sto__remote",
        if ctx.target.is_local() {
            "editing values only works from there"
        } else {
            "reading and editing values only work from there while a storage service is the target"
        },
    );
    format!(
        r#"<main class="dash"><section id="storage" class="sec" data-prefix="{prefix}" data-target="{kind}">
  <div class="sec__head"><h2>Storage</h2><span class="note">What the scene's server reads and writes through <code>Storage</code> and <code>EnvVar</code> from <code>@dcl/sdk/server</code>.</span></div>
  {notice}
  {target}
  {tabs}
  {tab_panel}
  {activity}
  {tools}
</section></main><script>{script}</script>"#,
        prefix = esc(prefix),
        kind = ctx.target.kind(),
        target = target_panel(st, ctx, prefix, tab),
        tabs = tab_bar(prefix, tab, view.counts, player),
        tab_panel = match tab {
            Tab::Scene => scene_panel(&view.listing),
            Tab::Player => player_panel(prefix, player, &view.players, &view.listing),
            Tab::Env => env_panel(&view.listing, ctx.target.is_local()),
        },
        activity = activity_drawer(&view.activity, ctx.target.is_local()),
        tools = tools_drawer(prefix, ctx.target.is_local()),
        script = SCRIPT,
    )
}

fn target_panel(st: &AppState, ctx: &Ctx, prefix: &str, tab: Tab) -> String {
    let upstream = !ctx.target.is_local();
    let service = ctx.target.kind();
    let custom_url = match &ctx.target {
        Target::Custom(url) => url.as_str(),
        _ => "",
    };
    let radio = |value: &str, label: &str| {
        format!(
            r#"<label class="seg"><input class="seg__r" type="radio" name="service" value="{value}"{checked}>{label}</label>"#,
            checked = if service == value || (!upstream && value == "org") {
                " checked"
            } else {
                ""
            }
        )
    };
    let status = match ctx.target.url() {
        None => format!(
            "Values live in this project's <code>.dcl-one/{}</code>. The scene, this page and <code>dcl-one-sdk storage</code> read and write the same file.",
            storage::DB_FILE
        ),
        Some(url) => {
            let metadata = SceneMetadata::from_scene_json(&ctx.project.scene_json);
            match signer(st) {
                Some(signer) => format!(
                    "Every request goes to <code>{}</code> for {}, signed as {}.",
                    esc(url),
                    esc(&metadata.describe()),
                    esc(&signer.describe())
                ),
                None => format!(
                    "Every request goes to <code>{}</code> for {}, but nothing here can sign them: set <code>DCL_PRIVATE_KEY</code> before <code>dcl-one-sdk start</code>, or use Connect with DCL in the header.",
                    esc(url),
                    esc(&metadata.describe())
                ),
            }
        }
    };
    format!(
        r#"<div class="panel sto__target">
    <h2>Where storage lives</h2>
    <form method="post" action="{action}" data-target-form>
      <input type="hidden" name="token" value="{tok}"><input type="hidden" name="tab" value="{tab}">
      <label class="sto__switch"><input class="sw" type="checkbox" name="upstream"{on}><span class="knob__k">Use upstream storage</span></label>
      <div class="sto__choice" data-choice{hidden_choice}>
        <div class="seg-group">{org}{zone}{custom}</div>
        <input class="sto__url" name="url" placeholder="http://localhost:5199/storage" value="{url}" aria-label="Custom storage service URL"{hidden_url}>
      </div>
      <button class="knob__go" type="submit">Use this</button>
    </form>
    <span class="note sto__status">{status}</span>
  </div>"#,
        action = esc(&format!("{prefix}/storage/target")),
        tok = esc(token(st)),
        tab = tab.key(),
        on = if upstream { " checked" } else { "" },
        hidden_choice = if upstream { "" } else { " hidden" },
        org = radio("org", "decentraland.org"),
        zone = radio("zone", "decentraland.zone"),
        custom = radio("custom", "Custom URL"),
        url = esc(custom_url),
        hidden_url = if matches!(ctx.target, Target::Custom(_)) {
            ""
        } else {
            " hidden"
        },
    )
}

fn tab_bar(
    prefix: &str,
    active: Tab,
    counts: Option<(usize, usize, usize)>,
    player: Option<&str>,
) -> String {
    let count = |n: Option<usize>| match n {
        Some(n) => format!(r#"<span class="sto__n">{n}</span>"#),
        None => String::new(),
    };
    let link = |tab: Tab, label: &str, n: Option<usize>| {
        let mut href = format!("{prefix}/storage?tab={}", tab.key());
        if tab == Tab::Player {
            if let Some(p) = player {
                href.push_str(&format!("&player={}", crate::deploy::encode_segment(p)));
            }
        }
        format!(
            r#"<a class="sto__tab" href="{href}"{current}>{label}{count}</a>"#,
            href = esc(&href),
            current = if tab == active {
                r#" aria-current="page""#
            } else {
                ""
            },
            count = count(n),
        )
    };
    format!(
        r#"<nav class="sto__tabs" aria-label="Storage scopes">{}{}{}</nav>"#,
        link(Tab::Scene, "Scene", counts.map(|c| c.0)),
        link(Tab::Player, "Player", counts.map(|c| c.1)),
        link(Tab::Env, "Environment", counts.map(|c| c.2)),
    )
}

fn problem_note(problem: Option<&str>) -> String {
    match problem {
        Some(why) => format!(r#"<p class="note sto__problem">{}</p>"#, esc(why)),
        None => String::new(),
    }
}

fn row_html(row: &Row) -> String {
    format!(
        r#"<div class="sto__row" data-key="{key}" data-json="{json}">
      <span class="sto__key">{key}</span>
      <span class="sto__val">{shown}</span>
      <span class="sto__acts">{reveal}<button class="sto__btn" type="button" data-act="edit">Edit</button><button class="sto__btn sto__btn--danger" type="button" data-act="delete">Delete</button></span>
      {meta}
    </div>"#,
        key = esc(&row.key),
        json = esc(&row.editable),
        shown = esc(&row.shown),
        reveal = if row.secret {
            r#"<button class="sto__btn" type="button" data-act="reveal">Reveal</button>"#
        } else {
            ""
        },
        meta = if row.meta.is_empty() {
            String::new()
        } else {
            format!(r#"<span class="sto__meta">{}</span>"#, esc(&row.meta))
        },
    )
}

fn rows_html(listing: &Listing, empty: &str) -> String {
    if listing.rows.is_empty() {
        return format!(r#"<p class="sto__empty">{empty}</p>"#);
    }
    let mut out = String::from(r#"<div class="sto__rows">"#);
    for row in &listing.rows {
        out.push_str(&row_html(row));
    }
    out.push_str("</div>");
    if listing.total > listing.rows.len() {
        out.push_str(&format!(
            r#"<p class="note">Showing {} of {} keys; <code>dcl-one-sdk storage scene list --offset {}</code> pages through the rest.</p>"#,
            listing.rows.len(),
            listing.total,
            listing.rows.len()
        ));
    }
    out
}

fn add_form(kind: &str, placeholder: &str) -> String {
    format!(
        r#"<form class="sto__add" data-add>
      <input class="sto__in" name="key" placeholder="key" aria-label="Key" required maxlength="{max}">
      <textarea class="sto__in sto__in--val" name="value" placeholder="{placeholder}" aria-label="Value"></textarea>
      <button class="knob__go" type="submit">Add</button>
    </form>"#,
        max = storage::MAX_KEY,
        placeholder = esc(placeholder),
    )
    .replace("data-add>", &format!(r#"data-add data-kind="{kind}">"#))
}

fn scene_panel(listing: &Listing) -> String {
    format!(
        r#"<div class="panel sto__panel" data-scope="scene" data-address="" data-kind="json">
    <div class="sto__head"><h2>Scene values</h2><span class="note">What <code>Storage.get</code> and <code>Storage.set</code> share across every player.</span>{clear}</div>
    {problem}{rows}{add}
  </div>"#,
        clear = clear_button(!listing.rows.is_empty(), "every scene value"),
        problem = problem_note(listing.problem.as_deref()),
        rows = rows_html(
            listing,
            "No scene values yet. Add one below, or let the scene write one with <code>Storage.set('key', value)</code>."
        ),
        add = add_form("json", "JSON value, or plain text"),
    )
}

fn clear_button(shown: bool, what: &str) -> String {
    if !shown {
        return String::new();
    }
    format!(
        r#"<button class="sto__btn sto__btn--danger sto__clear" type="button" data-act="clear" data-what="{}">Clear all</button>"#,
        esc(what)
    )
}

fn player_panel(
    prefix: &str,
    player: Option<&str>,
    players: &[(String, Option<usize>)],
    listing: &Listing,
) -> String {
    let chips: String = players
        .iter()
        .map(|(address, count)| {
            format!(
                r#"<a class="chip" href="{href}"{current}>{short}{count}</a>"#,
                count = match count {
                    Some(n) => format!(r#" <span class="sto__n">{n}</span>"#),
                    None => String::new(),
                },
                href = esc(&format!(
                    "{prefix}/storage?tab=player&player={}",
                    crate::deploy::encode_segment(address)
                )),
                current = if player == Some(address.as_str()) {
                    r#" aria-current="true""#
                } else {
                    ""
                },
                short = esc(&super::chrome::short_account(address)),
            )
        })
        .collect();
    let body = match player {
        Some(address) => format!(
            r#"<div class="sto__head"><h3 class="sto__who">{address}</h3>{clear}</div>{problem}{rows}{add}"#,
            address = esc(address),
            clear = clear_button(!listing.rows.is_empty(), "every value of this player"),
            problem = problem_note(listing.problem.as_deref()),
            rows = rows_html(
                listing,
                "No values for this player yet. Add one below, or let the scene write one with <code>Storage.player.set(address, 'key', value)</code>."
            ),
            add = add_form("json", "JSON value, or plain text"),
        ),
        None => format!(
            r#"<p class="sto__empty">{}</p>"#,
            if players.is_empty() {
                "Pick a wallet address to see its values. Nothing has written player values yet."
            } else {
                "Pick a wallet address to see its values."
            }
        ),
    };
    format!(
        r#"<div class="panel sto__panel" data-scope="player" data-address="{address}" data-kind="json">
    <div class="sto__head"><h2>Player values</h2><span class="note">What <code>Storage.player.get</code> and <code>Storage.player.set</code> keep per wallet.</span></div>
    <form class="sto__players" method="get" action="{action}"><input type="hidden" name="tab" value="player"><input class="sto__in sto__addr" name="player" placeholder="0x wallet address" aria-label="Wallet address" value="{address}"><button class="sto__btn" type="submit">Show</button></form>
    <div class="sto__players">{chips}</div>
    {body}
  </div>"#,
        address = esc(player.unwrap_or_default()),
        action = esc(&format!("{prefix}/storage")),
    )
}

fn env_panel(listing: &Listing, local: bool) -> String {
    let hint = if local {
        "What <code>EnvVar.get</code> answers: a runtime override set here or by the CLI wins over the scene's <code>.env</code>. Values stay hidden until revealed."
    } else {
        "What <code>EnvVar.get</code> answers from the service. Values stay hidden until revealed."
    };
    format!(
        r#"<div class="panel sto__panel" data-scope="env" data-address="" data-kind="env">
    <div class="sto__head"><h2>Environment</h2><span class="note">{hint}</span>{clear}</div>
    {problem}{rows}{add}
  </div>"#,
        clear = clear_button(
            listing.rows.iter().any(|r| r.meta != "from .env"),
            "every runtime environment override"
        ),
        problem = problem_note(listing.problem.as_deref()),
        rows = rows_html(
            listing,
            "No variables yet. Add one below, or put <code>KEY=VALUE</code> lines in the scene's <code>.env</code>."
        ),
        add = add_form("env", "value (a string)"),
    )
}

fn activity_drawer(activity: &[Activity], local: bool) -> String {
    if !local {
        return String::new();
    }
    let rows: String = activity
        .iter()
        .map(|a| {
            let what = match a.scope.as_str() {
                "player" => format!(
                    "{} {}/{}",
                    a.op,
                    super::chrome::short_account(&a.address),
                    a.key
                ),
                "all" => a.op.clone(),
                scope => format!("{} {scope}/{}", a.op, a.key),
            };
            format!(
                r#"<div><span class="st">{}</span> {} <span class="sto__src">{}</span></div>"#,
                esc(&ago(a.at)),
                esc(&what),
                esc(&a.source)
            )
        })
        .collect();
    format!(
        r#"<details class="drawer sto__activity"><summary>Recent writes{count}</summary><div class="drawer__body"><div class="reqs">{rows}</div>{empty}</div></details>"#,
        count = if activity.is_empty() {
            String::new()
        } else {
            format!(" ({})", activity.len())
        },
        empty = if activity.is_empty() {
            r#"<span class="note">Nothing has written yet. Writes from the scene, the CLI and this page land here with who made them.</span>"#
        } else {
            ""
        },
    )
}

fn tools_drawer(prefix: &str, local: bool) -> String {
    if !local {
        return format!(
            r#"<details class="drawer"><summary>Import and export</summary><div class="drawer__body"><span class="note">Snapshots read and write the local database; switch storage back to local to use them. The CLI reaches the service directly: <code>dcl-one-sdk storage scene list --target {}</code>.</summary></div></details>"#,
            "org"
        )
        .replace("</summary></div>", "</span></div>");
    }
    format!(
        r#"<details class="drawer"><summary>Import and export</summary><div class="drawer__body">
      <span class="note">A snapshot is upstream's <code>server-storage.json</code> (<code>env</code>, <code>world</code>, <code>players</code>), so it drops into an sdk-commands preview as-is.</span>
      <div class="sto__tools"><a class="bar__cta sto__export" href="{export}" download="server-storage.json">Download snapshot</a>
      <form class="sto__tools" data-import><input class="sto__in sto__file" type="file" name="file" accept="application/json,.json" aria-label="Snapshot file"><label class="sto__merge"><input type="checkbox" name="merge"> Merge over what is here</label><button class="sto__btn" type="submit">Import</button></form></div>
    </div></details>"#,
        export = esc(&format!("{prefix}/storage/export")),
    )
}

// ---------------------------------------------------------------- routes

/// Every storage route, mounted in the gated group (after the permissive
/// CORS layer, so a page on another origin cannot drive them).
pub(super) fn routes() -> axum::Router<Arc<AppState>> {
    use axum::routing::{get, post};
    axum::Router::new()
        .route("/storage", get(page))
        .route("/storage/target", post(set_target))
        .route("/storage/export", get(export))
        .route("/storage/import", post(import))
        .route("/values", get(scene_list).delete(scene_clear))
        .route(
            "/values/{key}",
            get(scene_get).put(scene_put).delete(scene_delete),
        )
        .route("/players", get(players).delete(players_clear))
        .route(
            "/players/{address}/values",
            get(player_list).delete(player_clear),
        )
        .route(
            "/players/{address}/values/{key}",
            get(player_get).put(player_put).delete(player_delete),
        )
        .route("/env", get(env_list).delete(env_clear))
        .route("/env/{key}", get(env_get).put(env_put).delete(env_delete))
        .route("/usage/world", get(usage_world))
        .route("/usage/players/{address}", get(usage_player))
        .route("/usage/env", get(usage_env))
}
