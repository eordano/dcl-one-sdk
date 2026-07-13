mod editor;
mod http;

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
    routing::{get, post},
    Router,
};
use editor::{data_layer_ws, inspector_asset, inspector_index, inspector_redirect, mobile_preview};
use http::{about, contents, entities_active, entities_scene, root, scene_id_for, scenes};
use serde_json::Value;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

pub struct StartOptions {
    pub dir: PathBuf,
    pub port: u16,
    pub skip_build: bool,
    pub no_watch: bool,
    pub ignore_composite: bool,
    pub offline_comms: bool,
    pub mobile: bool,
    pub asset_bundles: bool,
    pub data_layer: bool,
    pub tunnel: Option<String>,
    pub tunnel_token: Option<String>,
}

struct AppState {
    projects: Vec<Project>,
    machine: String,
    reload_tx: broadcast::Sender<ReloadFrame>,
    offline_comms: bool,
    port: u16,
    base: (i64, i64),
    data_layer: Option<DataLayerState>,
    entity_cache: Mutex<HashMap<PathBuf, (Instant, Value)>>,
}

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

    let data_layer = if opts.data_layer {
        let public_dir = data_layer::locate_inspector_public(&first.root)?;
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
        projects: workspace.projects.clone(),
        machine: machine_id(),
        reload_tx: reload_tx.clone(),
        offline_comms: opts.offline_comms,
        port: opts.port,
        base: joinblock::base_coords(&first.scene_json),
        data_layer,
        entity_cache: Mutex::new(HashMap::new()),
    });
    let comms_state = Arc::new(crate::comms::CommsState::default());

    let mut steps = if workspace.is_multi() {
        prepare_members(&opts, &workspace, &state, &reload_tx).await?
    } else {
        prepare_single(&opts, first.clone(), &state, &reload_tx).await?
    };

    let app = Router::new()
        .route("/", get(root))
        .route("/about", get(about))
        .route("/scenes", get(scenes))
        .route("/content/contents/{hash}", get(contents).head(contents))
        .route("/content/entities/active", post(entities_active))
        .route("/content/entities/scene", get(entities_scene))
        .route("/mobile-preview", get(mobile_preview))
        .route("/data-layer", get(data_layer_ws))
        .route("/inspector", get(inspector_redirect))
        .route("/inspector/", get(inspector_index))
        .route("/inspector/{*path}", get(inspector_asset))
        .with_state(state.clone())
        .merge(crate::comms::routes(comms_state))
        .layer(middleware::from_fn(access_log))
        .layer(tower_http::cors::CorsLayer::permissive());

    let addr = SocketAddr::from(([0, 0, 0, 0], opts.port));
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => return Err(bind_error(opts.port, addr, e)),
    };
    let mut sidecar = if opts.asset_bundles {
        crate::asset_bundles::spawn_sidecar(opts.port, &first.root)
    } else {
        None
    };
    let banner_state = state.clone();
    let scene_count = workspace.projects.len();
    let is_multi = workspace.is_multi();
    let scene_json = first.scene_json.clone();
    let port = opts.port;
    let mobile = opts.mobile;
    let tunnel_token = opts.tunnel_token.clone();
    tokio::spawn(async move {
        let optimized_assets_url = match sidecar.as_mut() {
            Some(s) => {
                if s.wait_ready().await {
                    ux::note(format!("Serving asset bundles (abgen JIT): {}", s.url));
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
            position: banner_state.base,
            port,
            ifaces,
            web_explorer: joinblock::web_explorer_base(),
            qr: if mobile { QrMode::Print } else { QrMode::Hint },
            unreachable,
            tunnel_hint: trunk_url.is_none(),
            editor: banner_state.data_layer.is_some(),
            optimized_assets_url,
        };
        if is_multi {
            ux::note(format!(
                "workspace preview: {scene_count} scenes served in one realm"
            ));
        }
        steps.done(block.heading());
        println!("{}", block.body());
        if let Some(trunk_url) = trunk_url {
            let events = crate::tunnel::spawn(crate::tunnel::AgentConfig {
                trunk_url,
                token: tunnel_token,
                local_port: port,
            });
            spawn_tunnel_printer(events, block.clone());
        }
    });
    axum::serve(listener, app).await.context("serving")
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

async fn prepare_single(
    opts: &StartOptions,
    project: Project,
    state: &Arc<AppState>,
    reload_tx: &broadcast::Sender<ReloadFrame>,
) -> Result<ux::Steps> {
    let build_opts = BuildOptions {
        dir: opts.dir.clone(),
        production: false,
        ignore_composite: opts.ignore_composite,
        custom_entry_point: false,
        skip_type_check: true,
    };

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
        let fs = FsWatcher::new(&project.root)?;
        let root = project.root.clone();
        let session =
            WatchSession::create(project, &build_opts, !opts.skip_build, &mut steps).await?;
        if !opts.skip_build {
            ux::note("type check skipped (--skip-type-check)");
        }
        let scene = b64_hash(&root.display().to_string(), &state.machine);
        spawn_watch(session, fs, root, scene, state.clone(), reload_tx.clone());
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
        let build_opts = BuildOptions {
            dir: project.root.clone(),
            production: false,
            ignore_composite: opts.ignore_composite,
            custom_entry_point: false,
            skip_type_check: true,
        };
        if opts.no_watch {
            if !opts.skip_build {
                build::build(&build_opts).await?;
            }
            continue;
        }
        let chunk = if opts.skip_build { 0 } else { 3 };
        let mut steps = ux::Steps::new(chunk);
        let fs = FsWatcher::new(&project.root)?;
        let session =
            WatchSession::create(project.clone(), &build_opts, !opts.skip_build, &mut steps)
                .await?;
        let scene = scene_id_for(project, &state.machine);
        spawn_watch(
            session,
            fs,
            project.root.clone(),
            scene,
            state.clone(),
            reload_tx.clone(),
        );
    }
    if opts.no_watch {
        Ok(ux::Steps::new(1))
    } else {
        if !opts.skip_build {
            ux::note("type check skipped (--skip-type-check)");
        }
        let mut steps = ux::Steps::new(2);
        steps.done("Watching for changes");
        Ok(steps)
    }
}

fn spawn_watch(
    session: WatchSession,
    fs: FsWatcher,
    root: PathBuf,
    scene: String,
    state: Arc<AppState>,
    tx: broadcast::Sender<ReloadFrame>,
) {
    tokio::spawn(async move {
        let notify = move |event: ReloadEvent| {
            lock_cache(&state).remove(&root);
            for frame in live_reload::reload_frames(&root, &scene, &state.machine, &event) {
                let _ = tx.send(frame);
            }
            tracing::info!("scene update pushed");
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
    });
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

async fn access_log(req: Request, next: Next) -> Response {
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
    tracing::info!(target: "access", "{method} {path} {status} {len}");
    resp
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
        .filter(|p| p.starts_with('/'))
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

    fn state(projects: Vec<Project>) -> AppState {
        let (reload_tx, _) = broadcast::channel(4);
        AppState {
            projects,
            machine: "test-machine".to_string(),
            reload_tx,
            offline_comms: true,
            port: 0,
            base: (0, 0),
            data_layer: None,
            entity_cache: Mutex::new(HashMap::new()),
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

    fn state_with_data_layer(public_dir: PathBuf) -> AppState {
        let (reload_tx, _) = broadcast::channel(4);
        let (_tx, port_rx) = tokio::sync::watch::channel(1234u16);
        std::mem::forget(_tx);
        AppState {
            projects: vec![],
            machine: "test-machine".to_string(),
            reload_tx,
            offline_comms: true,
            port: 0,
            base: (0, 0),
            data_layer: Some(DataLayerState {
                port_rx,
                public_dir,
            }),
            entity_cache: Mutex::new(HashMap::new()),
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
                    == json!(b64_hash(
                        &b.root.join("bin/index.js").display().to_string(),
                        "test-machine"
                    ))
        }));
    }
}
