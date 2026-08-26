mod content_cache;
mod deploy_page;
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
    /// Forward `local-ab=true` in the desktop deep link. Tracks `ab_sidecar`.
    ///
    /// This does NOT hand conversion to the explorer, which is what upstream
    /// uses the flag for. Per `AppArgsFlags.LOCAL_AB` in unity-explorer it
    /// "carries no URL or port": the client appends
    /// `RealmLaunchSettings.OPTIMIZED_ASSETS_PATH` to the realm it already has
    /// and fetches `{realm}/optimized-assets`, which this server proxies to the
    /// sidecar — so our sidecar still does every conversion.
    ///
    /// It is not an option because the alternative does not work. Naming the
    /// sidecar directly with `optimized-assets-url` was the old default, and
    /// the launcher drops that param before the explorer ever sees it (see the
    /// route comment below). Going through the realm also costs one port
    /// instead of two: one firewall approval, and a LAN or tunnel guest needs
    /// no second reachable address.
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

struct AppState {
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
    /// Pre-encoded `&key=value...` appended to every desktop deep link
    /// (local-ab/--mcp/--mcp-port and `--` passthrough params).
    deep_link_extra: String,
    /// The ingredients of the line above, kept so the landing page can rebuild
    /// it for a knob the visitor turns — the mcp pair on or off, extra params
    /// typed into the page — instead of trying to edit the encoded string.
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

    /// The first scene's base parcel, read live so a parcels edit moves every
    /// link and page that names a position.
    fn base(&self) -> (i64, i64) {
        self.first_project()
            .map(|p| joinblock::base_coords(&p.scene_json))
            .unwrap_or((0, 0))
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
        deep_link_extra: joinblock::deep_link_extra(
            opts.local_ab,
            opts.mcp,
            opts.mcp.then_some(opts.mcp_port),
            &opts.explorer_params,
        ),
        mcp: opts.mcp,
        mcp_port: opts.mcp_port,
        allow_remote_deploy: opts.allow_remote_deploy,
        deploy_dry_run: false,
        explorer_params: opts.explorer_params.clone(),
        recent_requests: Mutex::new(VecDeque::new()),
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
            position: banner_state.base(),
            port,
            ifaces,
            web_explorer: joinblock::web_explorer_base(),
            qr: if mobile { QrMode::Print } else { QrMode::Hint },
            unreachable,
            tunnel_hint: trunk_url.is_none(),
            editor: banner_state.data_layer.is_some(),
            optimized_assets_url: banner_ab_url(local_ab, optimized_assets_url),
            deep_link_extra: banner_state.deep_link_extra.clone(),
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
        // From here the output is a watch session: events get a clock in the
        // left gutter, and the address re-floats every hundred lines, because
        // by the time anyone wants to open it on a phone this banner is a
        // thousand lines up.
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

/// Which port the scene-log poller should read, or `None` when it must not run
/// at all.
///
/// The poller POSTs `127.0.0.1:{mcp_port}/unity-explorer-mcp` every 700ms. The
/// default client port is 8123 and `--mcp` is on by default, so
/// `start --port 8123` aims that loop at THIS server: every poll is a 404 this
/// process serves itself, and with `RECENT_REQUESTS_CAP` at 200 the whole
/// request log on the landing page turns into self-traffic within minutes.
/// (The link would not work either — the client cannot bind a port this server
/// is already holding.) So the poller is skipped and the clash is reported,
/// rather than quietly drowning the one page that shows what real clients
/// asked for.
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

/// Push the change to whatever clients are listening, and say so.
///
/// The line is not decoration. A model change sends a targeted `UpdateModel`
/// frame naming one file, which reads as if only that asset is refetched — but
/// the client does not act on the distinction: `LocalSceneDevelopmentController`
/// routes BOTH `UpdateScene` and `UpdateModel` into `TryReloadSceneAsync`, over
/// a `TODO` saying discriminating them is still to do. So an asset save reloads
/// the whole scene, and the terminal should not imply otherwise.
///
/// `broadcast::Sender::send` reports how many receivers took the frame, which
/// is the difference between "the scene reloaded" and "nothing was listening" —
/// the case where a developer waits for a change that cannot arrive because no
/// client is connected.
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
mod tests {
    use super::http::{build_scene_entity, entities_for, project_for, scene_id_for};
    use super::*;
    use axum::extract::{Path as AxPath, State};
    use axum::http::StatusCode;
    use axum::Json;
    use serde_json::json;

    struct Tmp(PathBuf);

    impl Tmp {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("dcl-one-sdk-startws-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Tmp(dir)
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn member(tmp: &Tmp, name: &str, parcels: &[&str]) -> Project {
        let root = tmp.0.join(name);
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::write(root.join("bin/index.js"), "module.exports={}").unwrap();
        let scene_json = json!({
            "main": "bin/index.js",
            "runtimeVersion": "7",
            "scene": { "parcels": parcels, "base": parcels[0] }
        });
        std::fs::write(root.join("scene.json"), scene_json.to_string()).unwrap();
        Project {
            root: root.canonicalize().unwrap(),
            scene_json,
        }
    }

    #[test]
    fn a_deep_link_never_carries_both_ab_forms() {
        let sidecar = || Some("http://127.0.0.1:5147".to_string());
        assert_eq!(banner_ab_url(true, sidecar()), None);
        assert_eq!(banner_ab_url(true, None), None);
        assert_eq!(banner_ab_url(false, None), None);
        assert_eq!(banner_ab_url(false, sidecar()), sidecar());

        assert_eq!(
            joinblock::deep_link_extra(true, false, None, &[]),
            "&local-ab=true"
        );
        assert_eq!(joinblock::deep_link_extra(false, false, None, &[]), "");
    }

    fn state(projects: Vec<Project>) -> AppState {
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
            deep_link_extra: String::new(),
            mcp: true,
            mcp_port: crate::joinblock::DEFAULT_EXPLORER_MCP_PORT,
            allow_remote_deploy: false,
            deploy_dry_run: true,
            explorer_params: Vec::new(),
            recent_requests: Mutex::new(VecDeque::new()),
        }
    }

    #[test]
    fn entities_union_serves_every_member() {
        let tmp = Tmp::new("union");
        let a = member(&tmp, "scene-a", &["0,0"]);
        let b = member(&tmp, "scene-b", &["1,0", "1,1"]);
        let st = state(vec![a.clone(), b.clone()]);
        let all = entities_for(&st, &[]);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0]["id"], json!(scene_id_for(&a, "test-machine")));
        assert_eq!(all[1]["id"], json!(scene_id_for(&b, "test-machine")));
        assert_eq!(all[1]["pointers"], json!(["1,0", "1,1"]));
    }

    #[test]
    fn entities_filter_by_pointer_returns_only_matches() {
        let tmp = Tmp::new("filter");
        let a = member(&tmp, "scene-a", &["0,0"]);
        let b = member(&tmp, "scene-b", &["1,0"]);
        let st = state(vec![a, b.clone()]);
        let hit = entities_for(&st, &["1,0".to_string()]);
        assert_eq!(hit.len(), 1);
        assert_eq!(hit[0]["id"], json!(scene_id_for(&b, "test-machine")));
        let both = entities_for(&st, &["0,0".to_string(), "1,0".to_string()]);
        assert_eq!(both.len(), 2);
        let miss = entities_for(&st, &["9,9".to_string()]);
        assert!(miss.is_empty());
    }

    #[test]
    fn project_for_maps_paths_to_the_owning_member() {
        let tmp = Tmp::new("owner");
        let a = member(&tmp, "scene-a", &["0,0"]);
        let b = member(&tmp, "scene-b", &["1,0"]);
        let st = state(vec![a.clone(), b.clone()]);
        let inside_b = b.root.join("bin/index.js");
        assert_eq!(project_for(&st, &inside_b).unwrap().root, b.root);
        assert_eq!(project_for(&st, &a.root).unwrap().root, a.root);
        let outside = tmp.0.canonicalize().unwrap();
        assert!(project_for(&st, &outside).is_none());
    }

    #[tokio::test]
    async fn about_honors_x_forwarded_proto_host_prefix() {
        let tmp = Tmp::new("fwd");
        let a = member(&tmp, "scene-a", &["0,0"]);
        let mut st = state(vec![a]);
        st.offline_comms = false;
        let req = axum::extract::Request::builder()
            .uri("/about")
            .header("host", "127.0.0.1:8000")
            .header("x-forwarded-proto", "https")
            .header("x-forwarded-host", "tunnel.example")
            .header("x-forwarded-prefix", "/t/abc123defg/")
            .body(axum::body::Body::empty())
            .unwrap();
        let Json(v) = about(State(Arc::new(st)), req).await;
        assert_eq!(
            v["comms"]["fixedAdapter"],
            json!("ws-room:wss://tunnel.example/t/abc123defg/mini-comms/room-1")
        );
        assert_eq!(
            v["content"]["publicUrl"],
            json!("https://tunnel.example/t/abc123defg/content")
        );
        assert_eq!(
            v["lambdas"]["publicUrl"],
            json!("https://tunnel.example/t/abc123defg/lambdas")
        );
        assert!(v["configurations"]["scenesUrn"][0]
            .as_str()
            .unwrap()
            .contains("baseUrl=https://tunnel.example/t/abc123defg/content/contents/"));
    }

    #[tokio::test]
    async fn about_without_forwarding_headers_stays_plain_http() {
        let tmp = Tmp::new("nofwd");
        let a = member(&tmp, "scene-a", &["0,0"]);
        let mut st = state(vec![a]);
        st.offline_comms = false;
        let req = axum::extract::Request::builder()
            .uri("/about")
            .header("host", "10.1.2.20:8000")
            .body(axum::body::Body::empty())
            .unwrap();
        let Json(v) = about(State(Arc::new(st)), req).await;
        assert_eq!(
            v["comms"]["fixedAdapter"],
            json!("ws-room:ws://10.1.2.20:8000/mini-comms/room-1")
        );
        assert_eq!(
            v["content"]["publicUrl"],
            json!("http://10.1.2.20:8000/content")
        );
    }

    #[tokio::test]
    async fn root_redirect_honors_forwarded_prefix() {
        let tmp = Tmp::new("redir");
        let a = member(&tmp, "scene-a", &["0,0"]);
        let st = Arc::new(state(vec![a]));
        let req = axum::extract::Request::builder()
            .uri("/")
            .header("x-forwarded-prefix", "/t/abc123defg")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = root(State(st.clone()), req).await;
        assert_eq!(
            resp.headers().get(header::LOCATION).unwrap(),
            "/t/abc123defg/about"
        );
        let req = axum::extract::Request::builder()
            .uri("/")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = root(State(st), req).await;
        assert_eq!(resp.headers().get(header::LOCATION).unwrap(), "/about");
    }

    /// The landing page exactly as a browser receives it: through `root`, with
    /// the headers and the query string a visitor would send. Tests that
    /// rebuild the page's strings for themselves prove nothing about the page.
    async fn landing_body(st: &Arc<AppState>, uri: &str, headers: &[(&str, &str)]) -> String {
        let mut req = axum::extract::Request::builder()
            .uri(uri)
            .header("accept", "text/html,application/xhtml+xml");
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let resp = root(
            State(st.clone()),
            req.body(axum::body::Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(resp
            .headers()
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("text/html"));
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(body.to_vec()).unwrap()
    }

    /// `/deploy`'s posture is still no JavaScript at all: a script or an
    /// inline handler added there sits beside a button that publishes.
    fn assert_no_javascript(body: &str) {
        let body = body.to_lowercase();
        assert!(!body.contains("<script"), "no script element");
        assert!(!body.contains("javascript:"), "no javascript: url");
        let posts = body.matches(r#"method="post""#).count();
        if posts > 0 {
            assert_eq!(posts, 1, "one POST on this server, the publish button");
            assert!(
                body.contains(r#"name="token""#),
                "a POST without the token would be forgeable cross-origin"
            );
            assert!(
                body.contains(r#"name="fingerprint""#),
                "a POST without the payload fingerprint could publish something \
                 other than what the page showed"
            );
        }
        assert_no_inline_handlers(&body);
    }

    /// The landing page ships exactly its own two scripts — the `#edit-data`
    /// JSON blob and the inline editor — and nothing else executable: no third
    /// script however hostile the input, nothing loaded from anywhere, no
    /// `javascript:` url, no inline handler, and still no POSTing form.
    fn assert_only_the_landing_scripts(body: &str) {
        let lower = body.to_lowercase();
        assert_eq!(
            lower.matches("<script").count(),
            2,
            "the data blob and the editor script, nothing more"
        );
        assert!(body.contains(r#"<script type="application/json" id="edit-data">"#));
        assert!(
            !lower.contains("<script src"),
            "no script loads from anywhere"
        );
        assert!(!lower.contains("javascript:"), "no javascript: url");
        assert!(
            !lower.contains(r#"method="post""#),
            "the page's mutations are fetches to the gated routes, not forms"
        );
        assert_no_inline_handlers(&lower);
    }

    fn assert_no_inline_handlers(lower: &str) {
        for (attr, _) in lower.match_indices(" on") {
            let rest = &lower[attr + 3..];
            let name_len = rest
                .find(|c: char| !c.is_ascii_alphabetic())
                .unwrap_or(rest.len());
            assert!(
                !(name_len > 0 && rest[name_len..].starts_with('=')),
                "inline event handler: on{}",
                &rest[..name_len]
            );
        }
    }

    /// A form on any page here must be a GET aimed back at the page that drew
    /// it: a GET carries no side effect, so a page you happen to have open
    /// still cannot make this server do anything, which is what the blanket
    /// no-form rule used to buy. `action` is the whole expected attribute
    /// value, so behind a proxy it has to carry the forwarded prefix.
    fn assert_forms_are_gets(body: &str, action: &str, count: usize) {
        for form in body.match_indices("<form").map(|(i, _)| &body[i..]) {
            let tag = &form[..form.find('>').expect("unterminated <form")];
            assert!(
                tag.contains(r#"method="get""#),
                "every form must be a GET: {tag}"
            );
            assert!(
                tag.contains(&format!(r#"action="{action}""#)),
                "a form may only target this page (expected {action}): {tag}"
            );
        }
        assert_eq!(
            body.matches("<form").count(),
            count,
            "the page draws exactly {count} form(s)"
        );
    }

    #[tokio::test]
    async fn root_serves_a_landing_page_to_browsers() {
        let tmp = Tmp::new("landing");
        let a = member(&tmp, "scene-a", &["0,0"]);
        let st = Arc::new(state(vec![a]));
        let body = landing_body(&st, "/", &[("host", "127.0.0.1:8000")]).await;
        assert!(body.contains("dcl-one-sdk"));
        assert!(body.contains("decentraland://realm=http%3A%2F%2F127.0.0.1%3A8000"));
        assert_eq!(
            body.matches(r#"id="launch""#).count(),
            1,
            "one launch button, for the selected target"
        );
        assert!(
            !body.contains("https://decentraland.org/bevy-web/"),
            "the web target is not the selected one"
        );
        let web = landing_body(&st, "/?where=web", &[("host", "127.0.0.1:8000")]).await;
        assert!(web.contains(
            "https://decentraland.org/bevy-web/?preview=true&amp;realm=http://127.0.0.1:8000"
        ));
        assert!(
            !web.contains("decentraland://realm="),
            "and then the desktop card is the one that is gone"
        );
        assert_eq!(web.matches(r#"id="launch""#).count(), 1);

        let must = |needle: &str| {
            body.find(needle)
                .unwrap_or_else(|| panic!("missing {needle}"))
        };
        assert!(
            must(r#"class="scene""#) < must(r#"id="join""#),
            "the scene card comes before the join cards"
        );
        let panel = must(r#"<div class="panel"><div class="row row--map">"#);
        let next_panel = body[panel + 1..]
            .find(r#"<div class="panel"#)
            .map_or(body.len(), |i| i + panel + 1);
        assert!(
            panel < must("<h2>Parcels</h2>")
                && must("<h2>Parcels</h2>") < must("<h2>Spawn points</h2>")
                && must("<h2>Spawn points</h2>") < next_panel,
            "spawn points fold into the parcels panel"
        );
        assert!(
            must(r#"id="requests""#) > must(r#"id="join""#),
            "the request log is the last section"
        );
        assert!(
            body.contains(r#"<a class="bar__cta" href="/deploy">Deploy</a>"#),
            "the header carries the deploy button"
        );
        assert!(
            body.contains(r#"<h1 class="scene__title" id="edit-title""#)
                && !body.contains(r#"class="u-sr-only""#),
            "the visible title is the h1, and it is the title editor: {body}"
        );
        assert!(
            !body.contains(r#"id="deploy""#),
            "and the landing page no longer carries a deploy section"
        );
        assert!(
            !body.contains("--dir"),
            "the page never names the scene path"
        );
        for page in [
            "/about",
            "/scenes",
            "/scene.json",
            "/preview-wearables",
            "/deploy",
        ] {
            assert!(body.contains(&format!(r#"href="{page}""#)), "{page} linked");
        }
        assert_forms_are_gets(&body, "/", 1);
        let form = body.find("<form").expect("the knob form");
        let end = body[form..].find("</form>").expect("unterminated form") + form;
        for knob in [r#"name="where""#, r#"name="mcp""#, r#"name="opt""#] {
            let at = body.find(knob).unwrap_or_else(|| panic!("missing {knob}"));
            assert!(at > form && at < end, "{knob} must ride inside the form");
        }
        for gone in [
            "comms ws-room",
            "abgen ready",
            "bar__realm",
            ">setup<",
            ">server<",
            "snapshot",
            "/inspector/",
            "class=\"foot\"",
        ] {
            assert!(!body.contains(gone), "{gone} should be gone");
        }
        assert!(!body.contains(&format!("dcl-one-sdk {}", env!("CARGO_PKG_VERSION"))));
    }

    #[tokio::test]
    async fn landing_page_honors_forwarded_headers_and_escapes_titles() {
        let tmp = Tmp::new("landing-fwd");
        let root_dir = tmp.0.join("scene-x");
        std::fs::create_dir_all(root_dir.join("bin")).unwrap();
        std::fs::write(root_dir.join("bin/index.js"), "module.exports={}").unwrap();
        let scene_json = json!({
            "main": "bin/index.js",
            "display": { "title": "a <script> title" },
            "scene": { "parcels": ["0,0"], "base": "0,0" }
        });
        std::fs::write(root_dir.join("scene.json"), scene_json.to_string()).unwrap();
        let project = Project {
            root: root_dir.canonicalize().unwrap(),
            scene_json,
        };
        let st = Arc::new(state(vec![project]));
        let fwd = [
            ("host", "127.0.0.1:8000"),
            ("x-forwarded-proto", "https"),
            ("x-forwarded-host", "tunnel.example"),
            ("x-forwarded-prefix", "/t/abc123defg/"),
        ];
        let body = landing_body(&st, "/", &fwd).await;
        assert!(body.contains("https%3A%2F%2Ftunnel.example%2Ft%2Fabc123defg"));
        assert!(!body.contains("a <script> title"));
        assert!(body.contains("a &lt;script&gt; title"));
        let web = landing_body(&st, "/?where=web", &fwd).await;
        assert!(web.contains("https://tunnel.example/t/abc123defg"));
        assert!(!web.contains("a <script> title"));

        assert!(
            body.contains(r#"<a class="bar__cta" href="/t/abc123defg/deploy">Deploy</a>"#),
            "the header button keeps the forwarded prefix"
        );
        assert_forms_are_gets(&body, "/t/abc123defg/", 1);
        for page in ["/about", "/scenes", "/scene.json", "/deploy"] {
            assert!(
                body.contains(&format!(r#"href="/t/abc123defg{page}""#)),
                "{page} keeps the forwarded prefix"
            );
        }
        assert!(
            !body.contains(r#"href="/deploy""#),
            "and no link is left pointing at the unprefixed root"
        );
    }

    /// The landing page ships its editor script, and that is the whole
    /// JavaScript budget: however hostile the query string, nothing else
    /// executable may appear — not a third script, not a handler attribute,
    /// not a `javascript:` href — because everything reflected into the page
    /// is either allowlisted or escaped.
    #[tokio::test]
    async fn the_landing_page_ships_only_its_own_script() {
        let tmp = Tmp::new("landing-nojs");
        let root_dir = tmp.0.join("scene-x");
        std::fs::create_dir_all(root_dir.join("bin")).unwrap();
        std::fs::write(root_dir.join("bin/index.js"), "module.exports={}").unwrap();
        let scene_json = json!({
            "main": "bin/index.js",
            "display": { "title": "Plain" },
            "scene": { "parcels": ["0,0"], "base": "0,0" },
            "requiredPermissions": ["USE_FETCH"],
            "spawnPoints": [{ "name": "spawn", "default": true,
                              "position": { "x": 8, "y": 0, "z": 8 } }]
        });
        std::fs::write(root_dir.join("scene.json"), scene_json.to_string()).unwrap();
        let project = Project {
            root: root_dir.canonicalize().unwrap(),
            scene_json,
        };
        let st = Arc::new(state(vec![project]));
        let body = landing_body(&st, "/", &[("host", "127.0.0.1:8000")]).await;
        assert_only_the_landing_scripts(&body);
        assert_forms_are_gets(&body, "/", 1);

        let hostile = concat!(
            "/?spawn=%3Cscript%3Ealert%281%29%3C%2Fscript%3E",
            "&opt=%22+onload%3Dalert%281%29",
            "&where=%22%3E%3Cimg+src%3Dx+onerror%3Dalert%281%29%3E",
            "&mcp=javascript%3Aalert%281%29",
            "&args=--gatekeeper-url%3Dhttps%3A%2F%2Fevil.example",
            "&%3Cscript%3E=%3Cscript%3E",
        );
        let poisoned = landing_body(&st, hostile, &[("host", "127.0.0.1:8000")]).await;
        assert_only_the_landing_scripts(&poisoned);
        assert_forms_are_gets(&poisoned, "/", 1);
        for smuggled in ["alert(1)", "gatekeeper-url", "evil.example", "onerror"] {
            assert!(
                !poisoned.contains(smuggled),
                "attacker-chosen {smuggled:?} reached the page"
            );
        }
        assert!(poisoned.contains(r#"id="launch""#));
        assert!(poisoned.contains(r#"name="spawn""#), "this scene names one");
    }

    /// The request log is the one part of this page built out of strings a
    /// stranger chose: any LAN or tunnel peer puts one in the buffer just by
    /// asking for it. Every `AppState` constructor here starts the buffer
    /// empty, so until this test `id="requests"` was asserted present while
    /// every row was the empty string and the `esc()` on the way in was never
    /// once executed.
    #[tokio::test]
    async fn the_request_log_replays_a_hostile_path_escaped_and_newest_first() {
        let tmp = Tmp::new("reqlog");
        let a = member(&tmp, "scene-a", &["0,0"]);
        let st = Arc::new(state(vec![a]));
        let method = axum::http::Method::GET;
        record_request(&st, log_line(&method, "/<script>alert(1)</script>"), 404);
        record_request(&st, log_line(&method, "/newest.glb"), 200);

        let body = landing_body(&st, "/", &[("host", "127.0.0.1:8000")]).await;
        assert!(
            body.contains("GET /&lt;script&gt;alert(1)&lt;/script&gt;"),
            "the path is replayed escaped"
        );
        assert_only_the_landing_scripts(&body);
        assert!(
            body.find("/newest.glb").unwrap() < body.find("&lt;script&gt;").unwrap(),
            "newest first, so the log reads as a tail of what just happened"
        );
        assert!(
            body.contains(r#"<summary>Recent requests<span"#)
                && body.contains(r#"class="sec__count">2</span>"#),
            "the count is the buffer's, on the drawer that holds it: {body}"
        );
        assert!(
            body.contains(r#"<b class="st st--warn">404</b>"#)
                && body.contains(r#"<b class="st st--ok">200</b>"#),
            "each row carries its own status tone"
        );
    }

    /// The buffer bounds the section, so the eviction is the only thing
    /// keeping a long-running preview's page from growing without limit.
    #[test]
    fn the_request_log_forgets_the_oldest_past_its_cap() {
        let tmp = Tmp::new("reqcap");
        let a = member(&tmp, "scene-a", &["0,0"]);
        let st = state(vec![a]);
        let method = axum::http::Method::GET;
        for i in 0..RECENT_REQUESTS_CAP + 5 {
            record_request(&st, log_line(&method, &format!("/{i}.glb")), 200);
        }
        let recent = st.recent_requests.lock().unwrap();
        assert_eq!(recent.len(), RECENT_REQUESTS_CAP);
        assert_eq!(
            recent.front().unwrap().0,
            "GET /5.glb",
            "the oldest five go"
        );
        assert_eq!(
            recent.back().unwrap().0,
            format!("GET /{}.glb", RECENT_REQUESTS_CAP + 4)
        );
    }

    /// A path is attacker-chosen, unbounded, retained, and re-rendered into
    /// every response. Cut it on the way in rather than on the way out, so the
    /// unbounded version is never held at all.
    #[test]
    fn a_path_too_long_to_be_a_path_is_cut_before_it_is_kept() {
        let method = axum::http::Method::GET;
        assert_eq!(
            log_line(&method, "/about"),
            "GET /about",
            "short paths whole"
        );

        let long = format!("/{}", "a".repeat(7000));
        let line = log_line(&method, &long);
        assert!(
            line.len() < MAX_LOGGED_PATH + 8,
            "a 7000-character path is not retained: kept {} bytes",
            line.len()
        );
        assert!(line.starts_with("GET /aaa") && line.ends_with('\u{2026}'));

        let wide = format!("/{}", "\u{2764}".repeat(2000));
        let cut = log_line(&method, &wide);
        assert!(cut.ends_with('\u{2026}'));
        assert!(
            cut.trim_start_matches("GET /")
                .trim_end_matches('\u{2026}')
                .chars()
                .all(|c| c == '\u{2764}'),
            "no replacement or split character: {cut}"
        );
    }

    /// The whole app on a real socket, so a test can ask it the way a browser
    /// does — through the routing table and the access-log layer, instead of
    /// reaching past both to call one handler.
    async fn serve(st: &Arc<AppState>) -> (SocketAddr, tokio::task::JoinHandle<()>) {
        let app = build_router(st.clone(), Arc::new(crate::comms::CommsState::default()));
        let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
            .await
            .unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let _ = axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await;
        });
        (addr, handle)
    }

    /// `/deploy` is registered in the router `start` builds, and every page
    /// this server draws carries a header button pointing at it — but no test
    /// ever fetched it, so deleting the route would have left the suite green
    /// behind a button that 404s. Fetched here through the real router, not by
    /// calling the handler, because the registration is the untested half.
    /// The note has to distinguish a push that landed from one that went
    /// nowhere: with no client attached, `broadcast::send` reports zero
    /// receivers, and a developer staring at "reload issued" while nothing
    /// moves is the exact confusion this line exists to prevent.
    #[test]
    fn the_reload_note_says_whether_anything_received_it() {
        assert_eq!(reload_note(1), "reload issued");
        assert_eq!(reload_note(3), "reload issued to 3 clients");
        let none = reload_note(0);
        assert!(none.contains("no client connected"), "{none}");
        assert!(!none.contains("reload issued"), "{none}");
    }

    /// A model save is not a targeted refetch, whatever the frame implies:
    /// the client routes UpdateModel and UpdateScene alike into
    /// TryReloadSceneAsync. Both events must therefore report a reload, and
    /// this pins the pair so a future "only the asset reloaded" claim has to
    /// change a test rather than just the copy.
    #[tokio::test]
    async fn both_a_scene_and_a_model_change_report_a_reload() {
        let tmp = Tmp::new("reloadnote");
        let project = member(&tmp, "scene-a", &["0,0"]);
        let root = project.root.clone();
        let st = Arc::new(state(vec![project]));
        let (tx, _keep) = broadcast::channel::<ReloadFrame>(8);
        let mut rx = tx.subscribe();

        for event in [
            ReloadEvent::Scene,
            ReloadEvent::Model {
                path: root.join("assets/spiral.glb"),
                removed: false,
            },
        ] {
            notify_reload(&root, "scene-a", &st, &tx, event);
            // Two frames per push: the SCENE_UPDATE text and the binary. Both
            // reach a live subscriber, which is what makes the count non-zero.
            assert!(rx.try_recv().is_ok(), "text frame");
            assert!(rx.try_recv().is_ok(), "binary frame");
        }
    }

    #[tokio::test]
    async fn deploy_is_a_page_this_server_actually_serves() {
        let tmp = Tmp::new("deployroute");
        let a = member(&tmp, "scene-a", &["0,0"]);
        let scene_dir = a.root.display().to_string();
        let st = Arc::new(state(vec![a]));
        let (addr, server) = serve(&st).await;
        let resp = reqwest::Client::new()
            .get(format!("http://{addr}/deploy"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 200, "the header button leads here");
        assert!(resp
            .headers()
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("text/html"));
        let body = resp.text().await.unwrap();
        server.abort();
        assert!(
            body.contains("dcl-one-sdk deploy"),
            "the page still prints the command, for anyone who wants to run it"
        );
        assert!(
            body.contains(r#"method="post""#) && body.contains(r#"name="token""#),
            "the publish button is live for a loopback caller"
        );
        assert_no_javascript(&body);
        assert!(
            !body.contains(&scene_dir) && !body.contains("deployroute"),
            "the page never names the scene directory"
        );
    }

    /// The publish gates, driven through the real router. Each of the three is
    /// asserted by breaking it and watching the POST be refused — a deploy
    /// that ran here would build and sign a scene, so these are the tests that
    /// keep an unauthenticated port from being a publish button.
    #[tokio::test]
    async fn publishing_refuses_a_forged_or_stale_post() {
        let tmp = Tmp::new("deploypost");
        let a = member(&tmp, "scene-a", &["0,0"]);
        let st = Arc::new(state(vec![a]));
        let (addr, server) = serve(&st).await;
        let client = reqwest::Client::new();

        let page = client
            .get(format!("http://{addr}/deploy"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        let field = |name: &str| {
            let at = page
                .find(&format!(r#"name="{name}" value=""#))
                .unwrap_or_else(|| panic!("no {name} field on the page"));
            let from = page[at..].find("value=\"").unwrap() + at + 7;
            page[from..][..page[from..].find('"').unwrap()].to_string()
        };
        let token = field("token");
        let print = field("fingerprint");
        assert!(!token.is_empty() && !print.is_empty());

        let poster = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let post = |form: Vec<(&'static str, String)>| {
            let c = poster.clone();
            async move {
                c.post(format!("http://{addr}/deploy"))
                    .form(&form)
                    .send()
                    .await
                    .unwrap()
                    .status()
                    .as_u16()
            }
        };

        assert_eq!(
            post(vec![
                ("token", "not-the-token".to_string()),
                ("fingerprint", print.clone()),
            ])
            .await,
            403,
            "a forged token must not start a deploy"
        );

        assert_eq!(
            post(vec![
                ("token", token.clone()),
                ("fingerprint", "0000stale0000".to_string()),
            ])
            .await,
            303,
            "a stale fingerprint redirects rather than publishing"
        );
        let after = client
            .get(format!("http://{addr}/deploy"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert!(
            after.contains("Nothing was published"),
            "and it says so: {after:.400}"
        );
        server.abort();
    }

    /// The editors' write path end to end, through the real router: a
    /// same-origin POST from loopback rewrites scene.json on disk, and every
    /// read surface — the landing page, `/scene.json`, `/about`'s parcel list
    /// — serves the edit at once, because the state behind them was updated
    /// too, not just the file.
    #[tokio::test]
    async fn a_scene_edit_reaches_disk_state_and_every_page() {
        let tmp = Tmp::new("editflow");
        let a = member(&tmp, "scene-a", &["0,0"]);
        let root = a.root.clone();
        let st = Arc::new(state(vec![a]));
        let (addr, server) = serve(&st).await;
        let client = reqwest::Client::new();

        let resp = client
            .post(format!("http://{addr}/scene-json"))
            .json(&json!({
                "title": "Renamed Stage",
                "description": "Now with a description.",
                "tags": ["events", "theatre"],
                "parcels": ["0,0", "1,0"],
            }))
            .send()
            .await
            .unwrap();
        let status = resp.status().as_u16();
        let echoed: Value = resp.json().await.unwrap();
        assert_eq!(status, 200, "{echoed}");
        assert_eq!(echoed["display"]["title"], json!("Renamed Stage"));

        let disk: Value =
            serde_json::from_str(&std::fs::read_to_string(root.join("scene.json")).unwrap())
                .unwrap();
        assert_eq!(disk["display"]["title"], json!("Renamed Stage"));
        assert_eq!(disk["scene"]["parcels"], json!(["0,0", "1,0"]));
        assert_eq!(
            disk["main"],
            json!("bin/index.js"),
            "untouched keys survive"
        );

        let page = client
            .get(format!("http://{addr}/"))
            .header("accept", "text/html")
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert!(page.contains("Renamed Stage"));
        assert!(page.contains("2 parcels"));
        assert!(page.contains("events"));

        let served: Value = client
            .get(format!("http://{addr}/scene.json"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(served["display"]["title"], json!("Renamed Stage"));

        let about: Value = client
            .get(format!("http://{addr}/about"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            about["configurations"]["localSceneParcels"],
            json!(["0,0", "1,0"]),
            "the realm advertises the new layout"
        );
        server.abort();
    }

    /// Every way a stranger's page could aim a POST at the editor, refused
    /// before it touches disk: a cross-origin `Origin`, a cross-site
    /// `Sec-Fetch-Site`, a tunnel replay, and a body field the page never
    /// drew an editor for.
    #[tokio::test]
    async fn a_forged_scene_edit_never_touches_disk() {
        let tmp = Tmp::new("editforge");
        let a = member(&tmp, "scene-a", &["0,0"]);
        let root = a.root.clone();
        let before = std::fs::read_to_string(root.join("scene.json")).unwrap();
        let st = Arc::new(state(vec![a]));
        let (addr, server) = serve(&st).await;
        let client = reqwest::Client::new();
        let post = |headers: Vec<(&'static str, &'static str)>, body: Value| {
            let client = client.clone();
            let url = format!("http://{addr}/scene-json");
            async move {
                let mut req = client.post(url).json(&body);
                for (k, v) in headers {
                    req = req.header(k, v);
                }
                req.send().await.unwrap().status().as_u16()
            }
        };

        let title = json!({ "title": "Forged" });
        assert_eq!(
            post(vec![("origin", "https://evil.example")], title.clone()).await,
            403,
            "a cross-origin POST is refused"
        );
        assert_eq!(
            post(vec![("sec-fetch-site", "cross-site")], title.clone()).await,
            403
        );
        assert_eq!(
            post(vec![(crate::tunnel::FORWARDED_HEADER, "1")], title.clone()).await,
            403,
            "a tunnel replay arrives on a loopback socket and is still refused"
        );
        assert_eq!(
            post(vec![], json!({ "realm": "https://evil.example" })).await,
            422,
            "a field the page has no editor for is refused, not ignored"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("scene.json")).unwrap(),
            before,
            "nothing above touched the file"
        );

        let own: &'static str = format!("http://{addr}").leak();
        assert_eq!(
            post(vec![("origin", own)], title).await,
            200,
            "the page's own origin is the one that works"
        );
        server.abort();
    }

    /// The thumbnail route writes the image where scene.json will find it,
    /// repoints `display.navmapThumbnail`, and refuses bytes that are not the
    /// image type they claim.
    #[tokio::test]
    async fn a_thumbnail_upload_lands_in_the_scene_and_scene_json() {
        let tmp = Tmp::new("editthumb");
        let a = member(&tmp, "scene-a", &["0,0"]);
        let root = a.root.clone();
        let st = Arc::new(state(vec![a]));
        let (addr, server) = serve(&st).await;
        let client = reqwest::Client::new();

        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend_from_slice(&[0u8; 24]);
        let resp = client
            .post(format!("http://{addr}/scene-thumbnail"))
            .header("content-type", "image/png")
            .body(png.clone())
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 200);
        let answer: Value = resp.json().await.unwrap();
        assert_eq!(answer["navmapThumbnail"], json!("scene-thumbnail.png"));
        assert_eq!(
            std::fs::read(root.join("scene-thumbnail.png")).unwrap(),
            png
        );
        let disk: Value =
            serde_json::from_str(&std::fs::read_to_string(root.join("scene.json")).unwrap())
                .unwrap();
        assert_eq!(
            disk["display"]["navmapThumbnail"],
            json!("scene-thumbnail.png")
        );

        let forged = client
            .post(format!("http://{addr}/scene-thumbnail"))
            .header("content-type", "image/png")
            .body(b"GIF89a not a png".to_vec())
            .send()
            .await
            .unwrap();
        assert_eq!(
            forged.status().as_u16(),
            415,
            "the magic bytes decide, not the header"
        );
        server.abort();
    }

    /// `access_log` is a layer, so every test that calls a handler directly
    /// walks straight past it. Drive the real server and look at what it
    /// retained: the buffer is unauthenticated, attacker-writable state that
    /// every page re-renders.
    #[tokio::test]
    async fn the_access_log_keeps_one_bounded_line_per_request() {
        let tmp = Tmp::new("accesslog");
        let a = member(&tmp, "scene-a", &["0,0"]);
        let st = Arc::new(state(vec![a]));
        let (addr, server) = serve(&st).await;
        let client = reqwest::Client::new();
        let long = format!("/{}", "a".repeat(4000));
        client.get(format!("http://{addr}{long}")).send().await.ok();
        client.get(format!("http://{addr}/about")).send().await.ok();
        server.abort();

        let recent: Vec<(String, u16)> = st
            .recent_requests
            .lock()
            .unwrap()
            .iter()
            .map(|(line, status, _)| (line.clone(), *status))
            .collect();
        assert_eq!(recent.len(), 2, "one line per request: {recent:?}");
        assert!(
            recent[0].0.len() < MAX_LOGGED_PATH + 8,
            "a 4000-character path must not be retained whole, kept {} bytes",
            recent[0].0.len()
        );
        assert!(recent[0].0.ends_with('\u{2026}'), "{}", recent[0].0);
        assert_eq!(recent[0].1, 404, "and the status it really got");
        assert_eq!(recent[1], ("GET /about".to_string(), 200));
    }

    /// `--mcp` is on by default and its default port is 8123, so
    /// `start --port 8123` aimed the scene-log poller at this very server: a
    /// 404 POST to itself several times a second, which fills a 200-entry
    /// request log in about seven minutes and buries every real client.
    #[test]
    fn the_scene_log_reader_never_polls_this_server() {
        let mcp = crate::joinblock::DEFAULT_EXPLORER_MCP_PORT;
        assert_eq!(
            scene_log_port(true, mcp, 8000),
            Some(mcp),
            "the normal case"
        );
        assert_eq!(
            scene_log_port(true, mcp, mcp),
            None,
            "start --port {mcp} must not start a poller aimed at itself"
        );
        assert_eq!(scene_log_port(false, mcp, 8000), None, "--no-mcp");
        assert_eq!(scene_log_port(false, mcp, mcp), None);

        let said = ux::render(&mcp_port_clash(mcp).into(), false, false);
        assert!(said.contains(&format!("--mcp-port {mcp}")), "{said}");
        assert!(said.contains("--mcp-port 8124"), "a way out: {said}");
        assert!(said.contains("--no-mcp"), "and a way to silence it: {said}");
    }

    fn state_with_data_layer(public_dir: PathBuf) -> AppState {
        let (reload_tx, _) = broadcast::channel(4);
        let (_tx, port_rx) = tokio::sync::watch::channel(1234u16);
        std::mem::forget(_tx);
        AppState {
            projects: std::sync::RwLock::new(vec![]),
            machine: "test-machine".to_string(),
            reload_tx,
            offline_comms: true,
            port: 0,
            data_layer: Some(DataLayerState {
                port_rx,
                public_dir: Some(public_dir),
            }),
            entity_cache: Mutex::new(HashMap::new()),
            optimized_assets_url: std::sync::OnceLock::new(),
            local_ab: true,
            deep_link_extra: String::new(),
            mcp: true,
            mcp_port: crate::joinblock::DEFAULT_EXPLORER_MCP_PORT,
            allow_remote_deploy: false,
            deploy_dry_run: true,
            explorer_params: Vec::new(),
            recent_requests: Mutex::new(VecDeque::new()),
        }
    }

    #[tokio::test]
    async fn contents_refuses_dclignored_files() {
        let tmp = Tmp::new("dclignore");
        let a = member(&tmp, "scene-a", &["0,0"]);
        std::fs::write(a.root.join("package.json"), "{\"secret\":\"key\"}").unwrap();
        let st = Arc::new(state(vec![a.clone()]));

        let pub_hash = b64_hash(
            &a.root.join("bin/index.js").display().to_string(),
            "test-machine",
        );
        let ok = contents(
            axum::http::Method::GET,
            State(st.clone()),
            AxPath(pub_hash),
            HeaderMap::new(),
        )
        .await;
        assert_eq!(ok.status(), StatusCode::OK);

        let ignored_hash = b64_hash(
            &a.root.join("package.json").display().to_string(),
            "test-machine",
        );
        let refused = contents(
            axum::http::Method::GET,
            State(st),
            AxPath(ignored_hash),
            HeaderMap::new(),
        )
        .await;
        assert_eq!(
            refused.status(),
            StatusCode::NOT_FOUND,
            "a .dclignored file must not be byte-served via /content/contents"
        );
    }

    #[tokio::test]
    async fn inspector_asset_refuses_absolute_and_dotdot_paths() {
        let tmp = Tmp::new("inspector");
        let public = tmp.0.join("public");
        std::fs::create_dir_all(&public).unwrap();
        std::fs::write(public.join("app.js"), "console.log(1)").unwrap();
        let secret = tmp.0.join("secret.txt");
        std::fs::write(&secret, "top secret").unwrap();
        let st = Arc::new(state_with_data_layer(public.clone()));

        let ok = inspector_asset(
            State(st.clone()),
            AxPath("app.js".to_string()),
            HeaderMap::new(),
        )
        .await;
        assert_eq!(ok.status(), StatusCode::OK);

        let abs = secret.canonicalize().unwrap().display().to_string();
        let escaped = inspector_asset(State(st.clone()), AxPath(abs), HeaderMap::new()).await;
        assert_eq!(
            escaped.status(),
            StatusCode::NOT_FOUND,
            "an absolute path outside public_dir must be refused"
        );

        let dotdot = inspector_asset(
            State(st),
            AxPath("../secret.txt".to_string()),
            HeaderMap::new(),
        )
        .await;
        assert_eq!(dotdot.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn inspector_serves_the_gzipped_bundle_both_ways() {
        use std::io::Write;
        let tmp = Tmp::new("inspector-gz");
        let public = tmp.0.join("public");
        std::fs::create_dir_all(&public).unwrap();
        let plain = b"globalThis.InspectorConfig\n".repeat(40);
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        enc.write_all(&plain).unwrap();
        std::fs::write(public.join("bundle.js.gz"), enc.finish().unwrap()).unwrap();
        let st = Arc::new(state_with_data_layer(public.clone()));

        let mut gz_headers = HeaderMap::new();
        gz_headers.insert(header::ACCEPT_ENCODING, "gzip, deflate".parse().unwrap());
        let compressed = inspector_asset(
            State(st.clone()),
            AxPath("bundle.js".to_string()),
            gz_headers,
        )
        .await;
        assert_eq!(compressed.status(), StatusCode::OK);
        assert_eq!(
            compressed.headers().get(header::CONTENT_ENCODING).unwrap(),
            "gzip"
        );
        assert_eq!(
            compressed.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/javascript",
            "the mime must come from the request path, not the .gz on disk"
        );
        let body = axum::body::to_bytes(compressed.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(body.len() < plain.len());
        assert_eq!(crate::data_layer::gunzip(&body).unwrap(), plain);

        let expanded =
            inspector_asset(State(st), AxPath("bundle.js".to_string()), HeaderMap::new()).await;
        assert_eq!(expanded.status(), StatusCode::OK);
        assert!(expanded.headers().get(header::CONTENT_ENCODING).is_none());
        let body = axum::body::to_bytes(expanded.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(body.as_ref(), plain.as_slice());
    }

    #[test]
    fn data_layer_origin_gate_allows_same_origin_and_native_rejects_cross() {
        let empty = HeaderMap::new();
        assert!(
            data_layer_origin_allowed(&empty),
            "native clients send no Origin"
        );

        let mut null_origin = HeaderMap::new();
        null_origin.insert(header::ORIGIN, "null".parse().unwrap());
        assert!(
            data_layer_origin_allowed(&null_origin),
            "null Origin is native"
        );

        let mut same = HeaderMap::new();
        same.insert(header::HOST, "127.0.0.1:8000".parse().unwrap());
        same.insert(header::ORIGIN, "http://127.0.0.1:8000".parse().unwrap());
        assert!(data_layer_origin_allowed(&same));

        let mut fwd = HeaderMap::new();
        fwd.insert("x-forwarded-host", "tunnel.example".parse().unwrap());
        fwd.insert(header::HOST, "127.0.0.1:8000".parse().unwrap());
        fwd.insert(header::ORIGIN, "https://tunnel.example".parse().unwrap());
        assert!(data_layer_origin_allowed(&fwd), "same-origin behind nginx");

        let mut cross = HeaderMap::new();
        cross.insert(header::HOST, "127.0.0.1:8000".parse().unwrap());
        cross.insert(header::ORIGIN, "https://evil.example".parse().unwrap());
        assert!(!data_layer_origin_allowed(&cross));
    }

    #[test]
    fn scene_entity_content_hashes_are_member_scoped() {
        let tmp = Tmp::new("entity");
        let b = member(&tmp, "scene-b", &["1,0"]);
        let entity = build_scene_entity(&b, "test-machine");
        let content = entity["content"].as_array().unwrap();
        assert!(content.iter().any(|c| {
            c["file"] == json!("bin/index.js")
                && c["hash"]
                    == json!(crate::scene::b64_content_hash(
                        &b.root.join("bin/index.js").display().to_string(),
                        "test-machine"
                    ))
        }));
    }
}
