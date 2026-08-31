pub(crate) mod chrome;
mod content_cache;
mod deploy_page;
mod deploy_rights;
mod deploy_status;
mod edit;
mod editor;
mod http;
mod landing;
pub(crate) mod scene_logs;

/// Whether a worlds host is configured, for callers outside this module.
///
/// `joinblock` needs it to decide whether advertising the `/world/…` mirror is
/// honest: the mirror answers 501 when this is false (see `proxy::world_base`).
pub fn world_base_configured() -> bool {
    proxy::world_base().is_some()
}
pub(crate) mod proxy;

use crate::build::{self, BuildOptions};
use crate::data_layer::{self, DataLayerState};
use crate::joinblock::{self, JoinBlock, QrMode};
use crate::live_reload::{self, ReloadEvent, ReloadFrame};
use crate::netinfo::{self, Iface, IfaceClass};
use crate::scene::{b64_hash, machine_id, Project};
use crate::ux::{self, TrySteps, UserError};
use crate::watch::{FsWatcher, WatchSession};
use crate::workspace::Workspace;
use anyhow::{Context, Result};
use axum::{
    extract::Request,
    http::{header, HeaderMap},
    middleware::{self, Next},
    response::Response,
    routing::{any, get, post},
    Router,
};
use editor::{data_layer_ws, inspector_asset, inspector_index, inspector_redirect, mobile_preview};
use http::{
    about, contents, entities_active, entities_scene, feature_flags, preview_wearables, root,
    scene_id_for, scene_json, scenes,
};
use proxy::{
    catalyst_proxy, lambdas_contracts_servers, lambdas_explore_realms, world_about, world_content,
};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

pub struct StartOptions {
    pub dir: PathBuf,
    /// None picks 8000, or the next free port when 8000 is taken.
    pub port: Option<u16>,
    pub skip_build: bool,
    /// Type checking runs beside the watch loop, not in front of it, so it never
    /// delays a reload; this turns it off entirely.
    pub skip_type_check: bool,
    pub no_watch: bool,
    pub ignore_composite: bool,
    pub offline_comms: bool,
    pub mobile: bool,
    /// Run the local abgen conversion sidecar. On unless --no-asset-bundles.
    pub ab_sidecar: bool,
    /// Forward `local-ab=true` in the desktop deep link (tracks `ab_sidecar`).
    /// The client then fetches `{realm}/optimized-assets`, which this server
    /// proxies to the sidecar. Not an option: naming the sidecar via
    /// `optimized-assets-url` was the old default and the launcher drops that
    /// param before the explorer sees it.
    pub local_ab: bool,
    pub mcp: bool,
    /// Let a non-loopback peer press Deploy. Off by default: the publish
    /// button signs with the wallet of the machine hosting the preview, and
    /// the port is otherwise unauthenticated.
    pub allow_remote_deploy: bool,
    /// Already defaulted by the caller, so the deep link and the log reader
    /// cannot disagree about which port the client opened.
    pub mcp_port: u16,
    /// How much of the developer's source to quote around a scene error.
    pub source_context: SourceContext,
    /// Raw tokens after a standalone `--`, forwarded into the desktop deep
    /// link as query params.
    pub explorer_params: Vec<String>,
    pub data_layer: bool,
    pub tunnel: Option<String>,
    pub tunnel_token: Option<String>,
    /// Skip the authoritative-server isolate a scene.json
    /// `authoritativeMultiplayer: true` would otherwise attach (upstream's
    /// preview auto-starts the server role; so does this one).
    pub no_host: bool,
}

/// Extra source lines quoted either side of the line a scene error points at.
#[derive(Clone, Copy)]
pub struct SourceContext {
    pub before: u32,
    pub after: u32,
}

impl SourceContext {
    /// `--error-source-lines-context` sets both sides; the per-side flags win
    /// over it, so `--error-source-lines-context=4 --error-source-lines-after=0`
    /// is meaningful.
    ///
    /// Defaults to 0: the line that threw is the answer, and neighbours are
    /// padding the reader has to skip past on every error.
    pub fn resolve(context: Option<u32>, before: Option<u32>, after: Option<u32>) -> Self {
        const DEFAULT: u32 = 0;
        SourceContext {
            before: before.or(context).unwrap_or(DEFAULT),
            after: after.or(context).unwrap_or(DEFAULT),
        }
    }
}

impl Default for SourceContext {
    fn default() -> Self {
        SourceContext::resolve(None, None, None)
    }
}

pub(crate) struct AppState {
    /// Behind a lock because the landing page's editors rewrite scene.json at
    /// runtime: every reader takes a snapshot, and `set_scene_json` /
    /// `refresh_scene_json` are the only writers.
    projects: std::sync::RwLock<Vec<Project>>,
    machine: String,
    reload_tx: broadcast::Sender<ReloadFrame>,
    offline_comms: bool,
    port: u16,
    data_layer: Option<DataLayerState>,
    entity_cache: Mutex<HashMap<PathBuf, (Instant, Value)>>,
    /// The sidecar's own address, set once abgen reports ready. This is what
    /// `/optimized-assets/*` forwards to and what the landing page reports —
    /// NOT something to put in a deep link; see `local_ab`.
    optimized_assets_url: std::sync::OnceLock<String>,
    /// Whether deep links carry `local-ab=true`. Mirrors `Opts::local_ab` so
    /// the landing page builds the same link the terminal banner prints: with
    /// this on, a link must NOT also name the sidecar directly, since the
    /// explorer treats `optimized-assets-url` as an override of the
    /// realm-derived base and the two would cancel out.
    local_ab: bool,
    mcp: bool,
    mcp_port: u16,
    allow_remote_deploy: bool,
    /// Publish as a dry run: build, pack and mint the entity id, then stop
    /// before signing or uploading. Only the test suite sets it, and it exists
    /// because every gate test works by BREAKING a gate — without this, a test
    /// that neuters the token check falls through to a real deploy against a
    /// real content server. The suite must not be one deleted line away from
    /// publishing.
    deploy_dry_run: bool,
    explorer_params: Vec<String>,
    /// Ring buffer of the latest requests, shown on the landing page.
    recent_requests: Mutex<VecDeque<(String, u16, Instant)>>,
    /// The publish run, its signer, and the /deploy caches.
    deploy: deploy_page::DeployState,
}

impl AppState {
    /// A snapshot, not a guard: no handler holds the lock across an await, and
    /// the vec is a handful of small documents.
    fn projects(&self) -> Vec<Project> {
        self.projects
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn first_project(&self) -> Option<Project> {
        self.projects
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .first()
            .cloned()
    }

    fn set_scene_json(&self, root: &std::path::Path, scene_json: Value) {
        let mut projects = self
            .projects
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(project) = projects.iter_mut().find(|p| p.root == root) {
            project.scene_json = scene_json;
        }
    }

    /// Re-read scene.json off disk, keeping the last good copy through a
    /// mid-edit syntax error. Called on every watch batch, so a hand edit
    /// reaches the landing page and the entity metadata like a page edit does.
    fn refresh_scene_json(&self, root: &std::path::Path) {
        if let Ok(bytes) = std::fs::read(root.join("scene.json")) {
            if let Ok(scene_json) = serde_json::from_slice(&bytes) {
                self.set_scene_json(root, scene_json);
            }
        }
    }
}

async fn scene_route(
    axum::extract::State(st): axum::extract::State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> Response {
    landing::scene_page(&st, &headers)
}

/// One `AppState` for every test in this tree — the per-test differences are
/// field mutations at the call site, so a new field is added exactly once.
#[cfg(test)]
pub(crate) mod testkit {
    use super::*;

    pub(crate) fn state(projects: Vec<Project>) -> AppState {
        let (reload_tx, _) = broadcast::channel(4);
        AppState {
            projects: std::sync::RwLock::new(projects),
            machine: "test-machine".to_string(),
            reload_tx,
            offline_comms: true,
            port: 0,
            data_layer: None,
            entity_cache: Mutex::new(HashMap::new()),
            optimized_assets_url: std::sync::OnceLock::new(),
            local_ab: true,
            mcp: true,
            mcp_port: crate::joinblock::DEFAULT_EXPLORER_MCP_PORT,
            allow_remote_deploy: false,
            deploy_dry_run: true,
            explorer_params: Vec::new(),
            recent_requests: Mutex::new(VecDeque::new()),
            deploy: deploy_page::DeployState::default(),
        }
    }
}

/// Refuses a mutating request whose peer is not this machine: a loopback
/// socket that is a tunnel replay counts as remote (the agent stamps
/// [`crate::tunnel::FORWARDED_HEADER`]).
fn remote_peer(allow_remote: bool, peer: std::net::SocketAddr, headers: &HeaderMap) -> bool {
    !allow_remote
        && (headers.contains_key(crate::tunnel::FORWARDED_HEADER) || !peer.ip().is_loopback())
}

/// The same-origin half of the write gate: `Sec-Fetch-Site` when the browser
/// sends it, then `Origin` against `Host`. One copy, because the routes that
/// mutate (scene edits, the publish POST, the signature POST) must agree on
/// what "our own page" means.
fn cross_origin_refusal(headers: &HeaderMap) -> Option<&'static str> {
    const WHY: &str = "this request did not come from the preview's own page";
    if let Some(site) = headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()) {
        if site != "same-origin" && site != "none" {
            return Some(WHY);
        }
    }
    if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        let host = headers
            .get(header::HOST)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        if host.is_empty() || !origin.ends_with(host) {
            return Some(WHY);
        }
    }
    None
}

/// The buffer is what bounds what is held; `RECENT_REQUESTS_SHOWN` bounds what
/// is drawn. A few hundred short lines is nothing to hold and covers a whole
/// scene load, which is the run someone opening the drawer reads it to
/// understand.
const RECENT_REQUESTS_CAP: usize = 200;

/// How much of a request path the log keeps.
///
/// The path is a string a stranger chose — any LAN or tunnel peer can put one
/// in this buffer just by asking for it — and `RECENT_REQUESTS_CAP` of them are
/// held in memory and re-rendered into every page this server serves. Without a
/// cap, one request with a 7000-character path is retained and echoed whole,
/// and 200 of them are megabytes of attacker-chosen text on every render. A
/// real path is a scene file; anything longer is not information the reader
/// loses by having it cut.
const MAX_LOGGED_PATH: usize = 120;

const ENTITY_CACHE_TTL: Duration = Duration::from_millis(500);

fn lock_cache(st: &AppState) -> std::sync::MutexGuard<'_, HashMap<PathBuf, (Instant, Value)>> {
    st.entity_cache
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

pub async fn start(opts: StartOptions) -> Result<()> {
    let trunk_url = opts
        .tunnel
        .as_deref()
        .map(crate::tunnel::normalize_trunk_url)
        .transpose()?;
    let workspace = Workspace::load(&opts.dir)?;
    let first = workspace.projects[0].clone();
    crate::deploy::sticky_default_target(&first.root);
    let (port, listener) = bind_preview_port(opts.port).await?;

    let data_layer = if opts.data_layer {
        let public_dir = data_layer::locate_inspector_public(&first.root)?;
        if public_dir.is_none() {
            tracing::info!(
                "no @dcl/inspector UI installed \u{2014} serving the data layer on /data-layer only \
                 (npm install --save-dev @dcl/inspector to get /inspector/ too)"
            );
        }
        let port_rx = data_layer::spawn(&first.root).await?;
        Some(DataLayerState {
            port_rx,
            public_dir,
        })
    } else {
        None
    };

    let (reload_tx, _) = broadcast::channel::<ReloadFrame>(32);
    let state = Arc::new(AppState {
        projects: std::sync::RwLock::new(workspace.projects.clone()),
        machine: machine_id(),
        reload_tx: reload_tx.clone(),
        offline_comms: opts.offline_comms,
        port,
        data_layer,
        entity_cache: Mutex::new(HashMap::new()),
        optimized_assets_url: std::sync::OnceLock::new(),
        local_ab: opts.local_ab,
        mcp: opts.mcp,
        mcp_port: opts.mcp_port,
        allow_remote_deploy: opts.allow_remote_deploy,
        deploy_dry_run: false,
        explorer_params: opts.explorer_params.clone(),
        recent_requests: Mutex::new(VecDeque::new()),
        deploy: deploy_page::DeployState::default(),
    });
    match scene_log_port(opts.mcp, opts.mcp_port, port) {
        Some(mcp_port) => {
            scene_logs::spawn(mcp_port, workspace.projects.clone(), opts.source_context)
        }
        None if opts.mcp => ux::report_watch(&mcp_port_clash(port).into()),
        None => {}
    }

    let comms_state = Arc::new(crate::comms::CommsState::default());

    let mut steps = if workspace.is_multi() {
        prepare_members(&opts, &workspace, &state, &reload_tx).await?
    } else {
        prepare_single(&opts, first.clone(), &state, &reload_tx).await?
    };

    let app = build_router(state.clone(), comms_state);

    // upstream parity: authoritativeMultiplayer in scene.json auto-starts
    // the server isolate beside the preview. Held here so its stdin
    // lifeline closes with this process however it dies; failure to spawn
    // degrades the preview, it does not kill it.
    let _host_isolate = if !opts.no_host && crate::entrypoint::authoritative_multiplayer(&first) {
        match crate::host::spawn_isolate(&first.root, &format!("http://127.0.0.1:{port}"), "room-1")
        {
            Ok(isolate) => {
                ux::note_arrow(
                    "authoritative host attached (scene.json authoritativeMultiplayer; --no-host to skip)",
                );
                Some(isolate)
            }
            Err(e) => {
                ux::report_watch(&e);
                None
            }
        }
    } else {
        None
    };

    let mut sidecar = if opts.ab_sidecar {
        crate::asset_bundles::spawn_sidecar(port, &first.root)
    } else {
        None
    };
    let banner_state = state.clone();
    let scene_count = workspace.projects.len();
    let is_multi = workspace.is_multi();
    let scene_json = first.scene_json.clone();
    let mobile = opts.mobile;
    let local_ab = opts.local_ab;
    let tunnel_token = opts.tunnel_token.clone();
    tokio::spawn(async move {
        let optimized_assets_url = match sidecar.as_mut() {
            Some(s) => {
                if s.wait_ready().await {
                    ux::note_arrow(format!(
                        "Selected abgen backend: {} at {}",
                        s.backend_label(),
                        s.url
                    ));
                    let _ = banner_state.optimized_assets_url.set(s.url.clone());
                    Some(s.url.clone())
                } else {
                    None
                }
            }
            None => None,
        };
        let ifaces = netinfo::enumerate();
        let unreachable = probe_unreachable(&ifaces, port).await;
        let block = JoinBlock {
            title: joinblock::scene_title(&scene_json),
            position: joinblock::base_coords(&scene_json),
            port,
            ifaces,
            web_explorer: joinblock::web_explorer_base(),
            qr: if mobile { QrMode::Print } else { QrMode::Hint },
            unreachable,
            tunnel_hint: trunk_url.is_none(),
            editor: banner_state.data_layer.is_some(),
            optimized_assets_url: banner_ab_url(local_ab, optimized_assets_url),
            deep_link_extra: joinblock::deep_link_extra(
                banner_state.local_ab,
                banner_state.mcp,
                banner_state.mcp.then_some(banner_state.mcp_port),
                // The terminal link carries the same fresh-page defaults the
                // join card starts with; they ride AFTER the user's own
                // params, so an explicit `--multi-instance=false` still wins
                // the dedup.
                &{
                    let mut params = banner_state.explorer_params.clone();
                    for flag in landing::DEFAULT_ON {
                        params.push(format!("--{flag}=true"));
                    }
                    params
                },
            ),
            native_hud: true,
            native_bin: joinblock::detect_native_bin(),
        };
        if is_multi {
            ux::note(format!(
                "workspace preview: {scene_count} scenes served in one realm"
            ));
        }
        steps.done(block.heading());
        if ux::verbose() {
            println!("{}", block.body());
        } else {
            println!("{}", block.compact_body());
        }
        if let Some(trunk_url) = trunk_url {
            let events = crate::tunnel::spawn(crate::tunnel::AgentConfig {
                trunk_url,
                token: tunnel_token,
                local_port: port,
            });
            spawn_tunnel_printer(events, block.clone());
        }
        ux::set_session_note(session_note(port));
    });
    let result = tokio::select! {
        r = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        ) => r.context("serving"),
        _ = shutdown_signal() => Ok(()),
    };
    crate::asset_bundles::kill_sidecar_group();
    result
}

/// A CLI publish waiting on its wallet signature, served as the preview
/// interface rather than a one-off page: the printed URL is /deploy, which
/// carries the signing panel beside the payload and its alarms while the
/// signature is pending — and the server under it is the normal one, so the
/// scene about to go up can be walked from the same origin while you decide.
/// Resolves when the signature lands or the wait runs out, and the server
/// goes down with it.
pub(crate) async fn serve_signing(
    dir: &std::path::Path,
    port: Option<u16>,
    open_browser: bool,
    timeout: Duration,
    signer: Arc<crate::linker::LinkerState>,
    rx: tokio::sync::oneshot::Receiver<Result<String>>,
) -> Result<String> {
    let workspace = Workspace::load(dir)?;
    let (port, listener) = bind_preview_port(port).await?;
    // DCL_ONE_SDK_LINKER_HOST=0.0.0.0 keeps its old meaning: sign from
    // another device. The listener is bound wide either way (it is the normal
    // preview listener); the env var opens the signing gate.
    let allow_remote = crate::linker::linker_bind_host() != "127.0.0.1";
    let (reload_tx, _) = broadcast::channel::<ReloadFrame>(32);
    let state = Arc::new(AppState {
        projects: std::sync::RwLock::new(workspace.projects.clone()),
        machine: machine_id(),
        reload_tx,
        offline_comms: false,
        port,
        data_layer: None,
        entity_cache: Mutex::new(HashMap::new()),
        optimized_assets_url: std::sync::OnceLock::new(),
        local_ab: false,
        mcp: false,
        mcp_port: joinblock::DEFAULT_EXPLORER_MCP_PORT,
        allow_remote_deploy: allow_remote,
        deploy_dry_run: false,
        explorer_params: Vec::new(),
        recent_requests: Mutex::new(VecDeque::new()),
        deploy: deploy_page::DeployState::default(),
    });
    deploy_page::adopt_cli_signing(&state, signer);
    let app = build_router(state, Arc::new(crate::comms::CommsState::default()));
    let url = format!("http://localhost:{port}/deploy");
    println!();
    println!("Sign the deployment with your wallet in a browser:");
    println!("  {url}");
    ux::note("the same server previews the scene at / while you decide");
    if !allow_remote {
        ux::note("to sign from another device, re-run with DCL_ONE_SDK_LINKER_HOST=0.0.0.0");
    } else {
        ux::note("from another device, replace localhost with this machine's address");
    }
    if open_browser {
        crate::linker::spawn_browser(&url);
    } else {
        ux::note("browser auto-open disabled \u{2014} open the URL manually");
    }
    let serve = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    );
    tokio::select! {
        r = serve => {
            r.context("serving the signing page")?;
            Err(UserError::new(
                "the signing page stopped before a signature arrived",
                TrySteps::one("re-run dcl-one-sdk deploy"),
            )
            .into())
        }
        outcome = crate::linker::await_outcome(rx, timeout, &url) => outcome,
    }
}

/// The line the watch session re-floats: the address someone else on the
/// network can actually reach, not the loopback one they cannot.
fn session_note(port: u16) -> String {
    let host = netinfo::share_ip(&netinfo::enumerate())
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "127.0.0.1".to_string());
    format!("you are running the dcl-one-sdk at http://{host}:{port}")
}

/// Everything this server answers, in one place a test can drive.
///
/// Built here rather than inline in `start` so the routing table is reachable
/// without a scene build, a watcher and a tunnel: a route registered but never
/// fetched is a page whose disappearance no test can notice, and every page
/// this server draws carries a header button pointing at `/deploy`.
fn build_router(state: Arc<AppState>, comms_state: Arc<crate::comms::CommsState>) -> Router {
    Router::new()
        .route("/", get(root))
        .route("/about", get(about))
        .route("/scenes", get(scenes))
        .route("/scene.json", get(scene_json))
        .route("/preview-wearables", get(preview_wearables))
        .route("/feature-flags/{file}", get(feature_flags))
        .route("/content/contents/{hash}", get(contents).head(contents))
        .route("/content/entities/active", post(entities_active))
        .route("/content/entities/scene", get(entities_scene))
        .route("/content/entities", post(catalyst_proxy))
        .route("/lambdas/explore/realms", get(lambdas_explore_realms))
        .route("/lambdas/contracts/servers", get(lambdas_contracts_servers))
        .route("/lambdas/{*path}", any(catalyst_proxy))
        .route("/explorer/{*path}", any(catalyst_proxy))
        .route("/world/{name}/about", get(world_about))
        .route(
            "/optimized-assets/{*path}",
            any(crate::start::proxy::optimized_assets),
        )
        .route(
            "/world-content/{name}/contents/{hash}",
            get(world_content).head(world_content),
        )
        .route("/mobile-preview", get(mobile_preview))
        .route("/data-layer", get(data_layer_ws))
        .route("/inspector", get(inspector_redirect))
        .route("/inspector/", get(inspector_index))
        .route("/inspector/{*path}", get(inspector_asset))
        .with_state(state.clone())
        .merge(crate::comms::routes(comms_state))
        .layer(tower_http::cors::CorsLayer::permissive())
        .merge(
            Router::new()
                .route("/deploy", get(deploy_page::route).post(deploy_page::start))
                .route("/target", get(deploy_page::target_route))
                .route("/target/address", post(deploy_page::target_address))
                .route("/target/connect", post(deploy_page::target_connect))
                .route("/target/point", post(deploy_page::target_point))
                .route("/target/base", post(deploy_page::target_base))
                .route("/deploy/preflight", post(deploy_page::preflight))
                .route("/scene", get(scene_route))
                .route("/deploy/sign", post(deploy_page::sign_submit))
                .route("/scene-json", post(edit::scene_json))
                .route("/scene-thumbnail", post(edit::thumbnail))
                .with_state(state.clone()),
        )
        .layer(middleware::from_fn_with_state(state, access_log))
}

/// The `optimized-assets-url` the join block should advertise, given whether the
/// deep link already carries `local-ab=true`.
///
/// The two are alternatives, never both: the explorer treats
/// `optimized-assets-url` as an OVERRIDE of the realm-derived base
/// (`DecentralandUrlsSource::ResolveOptimizedAssetsUrl`), so emitting it
/// alongside `local-ab=true` would silently defeat the flag. Since `local_ab`
/// now tracks the sidecar, in practice this returns None whenever there is a
/// sidecar at all — but the pairing is what matters, so it stays explicit.
fn banner_ab_url(local_ab: bool, sidecar_url: Option<String>) -> Option<String> {
    match local_ab {
        true => None,
        false => sidecar_url,
    }
}

/// The scene-log poller POSTs `127.0.0.1:{mcp_port}` every 700ms; when that
/// port IS this server (`start --port 8123`), the loop would 404 against
/// itself and fill the request log — skipped and reported instead.
fn scene_log_port(mcp: bool, mcp_port: u16, server_port: u16) -> Option<u16> {
    match mcp && mcp_port != server_port {
        true => Some(mcp_port),
        false => None,
    }
}

/// What to print when the client's MCP port is this server's own port.
fn mcp_port_clash(port: u16) -> UserError {
    let other = port.saturating_add(1).max(1024);
    UserError::new(
        format!(
            "scene errors will not print \u{2014} --mcp-port {port} is the port this preview bound"
        ),
        TrySteps::one(format!(
            "dcl-one-sdk start --port {port} --mcp-port {other}"
        ))
        .and(format!(
            "or move the preview instead \u{2014} dcl-one-sdk start --port {other}"
        ))
        .and("or turn the reader off \u{2014} dcl-one-sdk start --no-mcp"),
    )
    .why(format!(
        "the reader would poll http://127.0.0.1:{port}/unity-explorer-mcp, which is this server: \
         it would answer its own polls 404 several times a second and fill the landing page's \
         request log with them, and the client cannot open that port while this server holds it"
    ))
}

/// Resolves on SIGINT (ctrl-c) or, on unix, SIGTERM.
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut term =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).ok();
        let term = async {
            match term.as_mut() {
                Some(t) => {
                    t.recv().await;
                }
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

fn spawn_tunnel_printer(
    mut events: tokio::sync::mpsc::UnboundedReceiver<crate::tunnel::AgentEvent>,
    block: JoinBlock,
) {
    tokio::spawn(async move {
        let mut announced: Option<String> = None;
        let mut warned = false;
        while let Some(event) = events.recv().await {
            match event {
                crate::tunnel::AgentEvent::Connected { public_url } => {
                    warned = false;
                    if announced.as_deref() == Some(public_url.as_str()) {
                        ux::note("tunnel reconnected \u{2014} public URL unchanged");
                    } else {
                        println!("{}", block.internet_section(&public_url));
                        announced = Some(public_url);
                    }
                }
                crate::tunnel::AgentEvent::ConnectFailed { error } => {
                    if !warned {
                        warned = true;
                        ux::report_watch(
                            &UserError::new(
                                "tunnel connection failed \u{2014} retrying in background; the local and LAN links above still work",
                                TrySteps::one(
                                    "check the tunnel URL/service \u{2014} dcl-one-sdk start --tunnel help",
                                )
                                .and(
                                    "re-run with --verbose to log every retry attempt with its full cause",
                                ),
                            )
                            .why(error)
                            .into(),
                        );
                    }
                }
                crate::tunnel::AgentEvent::Disconnected { error } => {
                    ux::note(format!(
                        "tunnel disconnected ({error}) \u{2014} reconnecting"
                    ));
                }
            }
        }
    });
}

/// The `BuildOptions` every preview build uses: never production/minified, entry point always
/// generated (never the scene's own `main`), differing only in which project dir to build.
fn preview_build_opts(opts: &StartOptions, dir: PathBuf) -> BuildOptions {
    BuildOptions {
        dir,
        production: false,
        ignore_composite: opts.ignore_composite,
        custom_entry_point: false,
        skip_type_check: opts.skip_type_check,
        out_root: None,
        quiet: false,
    }
}

async fn prepare_single(
    opts: &StartOptions,
    project: Project,
    state: &Arc<AppState>,
    reload_tx: &broadcast::Sender<ReloadFrame>,
) -> Result<ux::Steps> {
    let build_opts = preview_build_opts(opts, opts.dir.clone());

    let total = if opts.no_watch {
        1
    } else {
        let chunk = if opts.skip_build { 0 } else { 3 };
        chunk + 2
    };
    let mut steps = ux::Steps::new(total);

    if opts.no_watch {
        if !opts.skip_build {
            build::build(&build_opts).await?;
        }
    } else {
        let root = project.root.clone();
        let scene = b64_hash(&root.display().to_string(), &state.machine);
        watch_or_retry(
            project,
            build_opts,
            !opts.skip_build,
            &mut steps,
            scene,
            state.clone(),
            reload_tx.clone(),
        )
        .await?;
        steps.done("Watching for changes");
    }
    Ok(steps)
}

async fn prepare_members(
    opts: &StartOptions,
    workspace: &Workspace,
    state: &Arc<AppState>,
    reload_tx: &broadcast::Sender<ReloadFrame>,
) -> Result<ux::Steps> {
    for (i, project) in workspace.projects.iter().enumerate() {
        if let Some(header) = workspace.member_header(i) {
            ux::note(header);
        }
        let build_opts = preview_build_opts(opts, project.root.clone());
        if opts.no_watch {
            if !opts.skip_build {
                build::build(&build_opts).await?;
            }
            continue;
        }
        let chunk = if opts.skip_build { 0 } else { 3 };
        let mut steps = ux::Steps::new(chunk);
        let scene = scene_id_for(project, &state.machine);
        watch_or_retry(
            project.clone(),
            build_opts,
            !opts.skip_build,
            &mut steps,
            scene,
            state.clone(),
            reload_tx.clone(),
        )
        .await?;
    }
    if opts.no_watch {
        Ok(ux::Steps::new(1))
    } else {
        let mut steps = ux::Steps::new(2);
        steps.done("Watching for changes");
        Ok(steps)
    }
}

/// Start the watch loop for one project. A failed INITIAL build must not kill
/// `start`: the server can still serve and the watcher is what picks up the
/// fix, so scene-content errors get the same report-and-recover contract
/// re-builds have always had. Config errors (scene.json main, tsconfig) stay
/// fatal, pre-checked here — upstream dies on those before bundling too.
async fn watch_or_retry(
    project: Project,
    build_opts: BuildOptions,
    initial_build: bool,
    steps: &mut ux::Steps,
    scene: String,
    state: Arc<AppState>,
    tx: broadcast::Sender<ReloadFrame>,
) -> Result<()> {
    project.main_output()?;
    project.tsconfig()?;
    let fs = FsWatcher::new(&project.root)?;
    let root = project.root.clone();
    match WatchSession::create(project.clone(), &build_opts, initial_build, steps).await {
        Ok(session) => {
            tokio::spawn(run_watch(session, fs, root, scene, state, tx));
        }
        Err(e) => {
            report_initial_failure(&e);
            tokio::spawn(retry_initial_build(
                project,
                build_opts,
                initial_build,
                fs,
                root,
                scene,
                state,
                tx,
            ));
        }
    }
    Ok(())
}

/// Reports the build error itself (matching the re-build loop, so the compiler
/// diagnostic in the inner UserError's `why` is preserved) before noting that
/// the session survived it.
fn report_initial_failure(e: &anyhow::Error) {
    ux::report_watch(e);
    ux::note(
        "the preview server and watcher are still running \u{2014} save any file to retry the initial build",
    );
}

/// The recover half of the initial-build contract: every watch batch retries
/// the initial build (with the same skip-build choice the session started
/// with) until one succeeds, then hands the watcher to the normal re-build
/// loop.
#[allow(clippy::too_many_arguments)]
async fn retry_initial_build(
    project: Project,
    build_opts: BuildOptions,
    initial_build: bool,
    mut fs: FsWatcher,
    root: PathBuf,
    scene: String,
    state: Arc<AppState>,
    tx: broadcast::Sender<ReloadFrame>,
) {
    loop {
        if fs.next_batch().await.is_none() {
            return;
        }
        let mut steps = ux::Steps::new(if initial_build { 3 } else { 0 });
        match WatchSession::create(project.clone(), &build_opts, initial_build, &mut steps).await {
            Ok(session) => {
                notify_reload(&root, &scene, &state, &tx, ReloadEvent::Scene);
                run_watch(session, fs, root, scene, state, tx).await;
                return;
            }
            Err(e) => report_initial_failure(&e),
        }
    }
}

/// Push the change and say what it achieved: the client routes UpdateModel
/// and UpdateScene alike into TryReloadSceneAsync, so an asset save reloads
/// the whole scene, and `send`'s receiver count is the difference between
/// "reloaded" and "nothing was listening".
fn notify_reload(
    root: &std::path::Path,
    scene: &str,
    state: &AppState,
    tx: &broadcast::Sender<ReloadFrame>,
    event: ReloadEvent,
) {
    state.refresh_scene_json(root);
    lock_cache(state).remove(root);
    let mut clients = 0;
    for frame in live_reload::reload_frames(root, scene, &state.machine, &event) {
        clients = tx.send(frame).unwrap_or(0);
    }
    match clients {
        0 => ux::note_absent(reload_note(0)),
        n => ux::note_arrow(reload_note(n)),
    }
    tracing::info!("scene update pushed to {clients} client(s)");
}

/// What the push actually achieved, in the words the reader needs.
fn reload_note(clients: usize) -> String {
    match clients {
        0 => "no client connected".to_string(),
        1 => "reload issued".to_string(),
        n => format!("reload issued to {n} clients"),
    }
}

async fn run_watch(
    session: WatchSession,
    fs: FsWatcher,
    root: PathBuf,
    scene: String,
    state: Arc<AppState>,
    tx: broadcast::Sender<ReloadFrame>,
) {
    let notify = {
        let root = root.clone();
        move |event: ReloadEvent| notify_reload(&root, &scene, &state, &tx, event)
    };
    if let Err(e) = session.run(fs, notify).await {
        tracing::error!("watch loop stopped: {e:#}");
        ux::report_watch(
            &UserError::new(
                "live reload stopped",
                TrySteps::one(
                    "restart dcl-one-sdk start to resume hot reload (the server is still serving the last build)",
                ),
            )
            .why(format!("{e:#}"))
            .into(),
        );
    }
}

async fn probe_unreachable(ifaces: &[Iface], port: u16) -> Vec<std::net::Ipv4Addr> {
    let mut out = Vec::new();
    for i in ifaces {
        if matches!(i.class, IfaceClass::Loopback | IfaceClass::LinkLocal) {
            continue;
        }
        let reachable = tokio::time::timeout(
            Duration::from_millis(400),
            tokio::net::TcpStream::connect(SocketAddr::from((i.ip, port))),
        )
        .await
        .map(|r| r.is_ok())
        .unwrap_or(false);
        if !reachable {
            out.push(i.ip);
        }
    }
    out
}

/// Bind the preview listener. An explicit port must bind exactly (the error
/// explains the conflict); the default scans 8000 upward and falls back to an
/// ephemeral port, so `start` never dies just because 8000 is taken.
async fn bind_preview_port(
    requested: Option<u16>,
) -> Result<(u16, tokio::net::TcpListener), anyhow::Error> {
    const DEFAULT_PORT: u16 = 8000;
    const SCAN: u16 = 20;
    if let Some(port) = requested {
        let addr = SocketAddr::from(([0, 0, 0, 0], port));
        return match tokio::net::TcpListener::bind(addr).await {
            Ok(l) => Ok((port, l)),
            Err(e) => Err(bind_error(port, addr, e)),
        };
    }
    for port in DEFAULT_PORT..DEFAULT_PORT + SCAN {
        let addr = SocketAddr::from(([0, 0, 0, 0], port));
        match tokio::net::TcpListener::bind(addr).await {
            Ok(l) => {
                if port != DEFAULT_PORT {
                    ux::note(format!("port {DEFAULT_PORT} is busy — serving on {port}"));
                }
                return Ok((port, l));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => continue,
            Err(e) => return Err(bind_error(port, addr, e)),
        }
    }
    let addr = SocketAddr::from(([0, 0, 0, 0], 0));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| bind_error(0, addr, e))?;
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    ux::note(format!(
        "ports {DEFAULT_PORT}\u{2013}{} are busy — serving on {port}",
        DEFAULT_PORT + SCAN - 1
    ));
    Ok((port, listener))
}

fn bind_error(port: u16, addr: SocketAddr, e: std::io::Error) -> anyhow::Error {
    let next = port.checked_add(1).unwrap_or(8001);
    match e.kind() {
        std::io::ErrorKind::AddrInUse => UserError::new(
            format!("port {port} is already in use"),
            TrySteps::one(format!("dcl-one-sdk start --port {next}"))
                .and(format!("or stop the other process (lsof -i :{port})")),
        )
        .why(format!("something else is listening on {addr}"))
        .caused_by(e)
        .into(),
        std::io::ErrorKind::PermissionDenied => UserError::new(
            format!("port {port} cannot be opened"),
            TrySteps::one(
                "ports below 1024 need elevated rights \u{2014} pick a higher port with --port",
            )
            .and("dcl-one-sdk start --port 8001"),
        )
        .why(format!("binding {addr} was denied"))
        .caused_by(e)
        .into(),
        _ => anyhow::Error::from(e).context(format!("binding {addr}")),
    }
}

async fn access_log(
    axum::extract::State(st): axum::extract::State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Response {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let resp = next.run(req).await;
    let status = resp.status().as_u16();
    let len = resp
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-")
        .to_string();
    let line = log_line(&method, &path);
    tracing::info!(target: "access", "{line} {status} {len}");
    record_request(&st, line, status);
    resp
}

/// The one line the request log keeps for a request, cut to
/// [`MAX_LOGGED_PATH`] on a char boundary so a multi-byte path is trimmed
/// rather than split.
fn log_line(method: &axum::http::Method, path: &str) -> String {
    if path.len() <= MAX_LOGGED_PATH {
        return format!("{method} {path}");
    }
    let mut cut = MAX_LOGGED_PATH;
    while !path.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{method} {}\u{2026}", &path[..cut])
}

/// Push one line into the ring buffer, dropping the oldest past the cap. A
/// poisoned lock loses the line rather than the request: nothing here is worth
/// failing a response over.
fn record_request(st: &AppState, line: String, status: u16) {
    if let Ok(mut recent) = st.recent_requests.lock() {
        recent.push_back((line, status, Instant::now()));
        while recent.len() > RECENT_REQUESTS_CAP {
            recent.pop_front();
        }
    }
}

fn forwarded_proto(headers: &HeaderMap) -> &'static str {
    match headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
    {
        Some(p) if p.trim().eq_ignore_ascii_case("https") => "https",
        _ => "http",
    }
}

fn forwarded_prefix(headers: &HeaderMap) -> String {
    headers
        .get("x-forwarded-prefix")
        .and_then(|v| v.to_str().ok())
        .map(|p| p.trim().trim_end_matches('/'))
        .filter(|p| {
            p.starts_with('/') && !p.starts_with("//") && !p.contains(':') && !p.contains('\\')
        })
        .map(str::to_string)
        .unwrap_or_default()
}

fn forwarded_host(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-forwarded-host")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|h| !h.is_empty())
        .map(str::to_string)
}

fn authority_of(origin: &str) -> Option<String> {
    let after = origin
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(origin);
    let authority = after.split(['/', '?', '#']).next().unwrap_or("");
    (!authority.is_empty()).then(|| authority.to_ascii_lowercase())
}

fn allowed_editor_origins() -> Vec<String> {
    std::env::var("DCL_ONE_SDK_ALLOWED_ORIGINS")
        .ok()
        .map(|v| {
            v.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn data_layer_origin_allowed(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else {
        return true;
    };
    let origin = origin.trim();
    if origin.is_empty() || origin.eq_ignore_ascii_case("null") {
        return true;
    }
    let Some(origin_authority) = authority_of(origin) else {
        return false;
    };
    let request_authority = forwarded_host(headers)
        .or_else(|| {
            headers
                .get(header::HOST)
                .and_then(|h| h.to_str().ok())
                .map(str::to_string)
        })
        .map(|h| h.to_ascii_lowercase());
    if request_authority.as_deref() == Some(origin_authority.as_str()) {
        return true;
    }
    allowed_editor_origins()
        .iter()
        .any(|a| a.eq_ignore_ascii_case(&origin_authority) || a.eq_ignore_ascii_case(origin))
}

#[cfg(test)]
#[path = "start_tests.rs"]
mod tests;
