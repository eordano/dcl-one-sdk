//! `/deploy`: where this scene publishes, what is live there, what an upload
//! would actually move, and the button that does it. Remote knowledge is
//! fetched server-side, cached, and allowed to fail into a sentence.
//!
//! Publishing is the one world-changing action on an unauthenticated port
//! bound to every interface; the POST is gated on a loopback peer, this
//! process's token (readable only from the page body), and the payload still
//! fingerprinting as drawn — the watcher rebuilds while the page is open, so
//! without that check the approved payload and the uploaded bytes are only
//! incidentally the same thing. The same drift is watched after the claim:
//! a pending run whose payload moves before the wallet answers is re-minted
//! (see [`drift_reclaim`]), so the signature only ever lands on the tree as
//! it is.

use super::chrome::{document, esc, kv};
use super::deploy_rights::{self, Rights, Verdict};
use super::deploy_status::{
    self, ago, cached_status, resolve_dest, Dest, LiveStatus, Remote, RemoteScene, RemoteState,
};
use super::{cross_origin_refusal, forwarded_prefix, remote_peer, AppState};
use crate::deploy::{self, MainBundle};
use crate::scene::Project;
use axum::extract::{ConnectInfo, Form, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::Json;
use std::collections::{HashSet, VecDeque};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

pub(super) async fn route(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    page(&st, &headers, peer.ip().is_loopback()).await
}

pub(super) async fn target_route(State(st): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    target_page(&st, &headers).await
}

/// The deploy entry's badge in the section nav, read off the run slot so a
/// publish's liveness shows from every page, not just this one.
pub(super) fn nav_badge(st: &AppState) -> &'static str {
    match runs(st).as_ref().map(|r| &r.state) {
        Some(RunState::Running) => "signing\u{2026}",
        Some(RunState::Done) => "live",
        _ => "",
    }
}

/// The shapes this page needs that the shared sheet has no rule for: the file
/// list whose paths take the width while sizes align on a column of their
/// own. Tokens only — no colour of its own, no size off the scale, no case
/// change — so the page cannot drift into looking like a different server
/// than `/`.
const PAGE_CSS: &str = "
#deploy, #target { gap: var(--s-5); }
.jn2__col > .knob__k + * { margin-top: calc(-1 * var(--s-2)); }
.datum__unit + .datum__num { margin-left: var(--s-3); }
.jn2__foot .note { flex-basis: 100%; }
.files .kv { grid-template-columns: minmax(0, 1fr) 8.5rem; }
.files .k--file, .files .sz { font-size: var(--fs-13); }
.files .sz {
  color: var(--ink-6); font-variant-numeric: tabular-nums;
  text-align: right; white-space: nowrap;
}
.tgt__addr { display: flex; align-items: center; gap: var(--s-3); flex-wrap: wrap; }
.tgt__base { display: inline-flex; align-items: center; gap: var(--s-3); flex-wrap: wrap; }
.tgt__base input[name=\"base\"] {
  height: 36px; width: 9ch; padding: 0 var(--s-2-5); background: var(--panel);
  border: 1px solid var(--line-ctl); border-radius: var(--r-control);
  color: var(--text); font: inherit; text-align: center;
}
.tgt__addr .note { flex-basis: 100%; }
.dep__cell--kept { background: var(--fill-3); border: 1px solid var(--line-ctl); }
.dep__cell--was { border: 1px dashed var(--line-ctl); }
.dep__cell--own { background: var(--brand-wash); border: 1px solid var(--brand-line); }
.wl { display: flex; flex-direction: column; }
.wl__r {
  display: grid; grid-template-columns: minmax(0, 13rem) minmax(0, 1fr) auto;
  gap: var(--s-3); align-items: center; padding: var(--s-2-5) 0;
  border-top: 1px solid var(--line);
}
.wl__r:first-child { border-top: 0; }
.wl__n { display: inline-flex; align-items: center; gap: var(--s-2); min-width: 0; overflow-wrap: anywhere; }
.wl__d { color: var(--ink-6); font-size: var(--fs-13); min-width: 0; }
.files__more > summary { cursor: pointer; padding: var(--s-2-5) 0; color: var(--ink-6); font-size: var(--fs-13); border-top: 1px solid var(--line); }
.dep__err { white-space: pre-wrap; overflow-wrap: anywhere; max-height: 24rem; overflow-y: auto; }
.panel--ok { border-color: var(--success); }
.panel--ok > h2 { color: var(--success); }
.dep__ok { color: var(--success); }
.jn #run-status .panel--ok { border-top-color: var(--success); background: var(--success-fill, var(--fill-1)); }
.dep__wait { display: flex; flex-direction: column; align-items: center; gap: var(--s-3); padding: var(--s-6) 0; text-align: center; }
.jn #run-status .panel { border: 0; border-top: 1px solid var(--line); border-radius: 0; background: var(--fill-1); }
.jn #run-status { display: flex; flex-direction: column; }
.spin {
  width: 28px; height: 28px; border-radius: var(--r-pill);
  border: 3px solid var(--fill-4); border-top-color: var(--brand);
  animation: dep-spin .9s linear infinite;
}
@keyframes dep-spin { to { transform: rotate(360deg); } }
";

/// Files listed individually before the rest is summed into one line. Long
/// enough to cover the assets that actually decide an upload's size, short
/// enough that a scene with a thousand textures does not render a thousand
/// rows.
const LISTED: usize = 8;

/// How long a walk answers for. The route is unauthenticated and bound to
/// every interface, and the walk is the whole scene tree: without this, a
/// handful of concurrent refreshes park that many tokio workers in `read_dir`
/// and every other request on the server queues behind them. Same span as
/// `start`'s `ENTITY_CACHE_TTL`, for the same reason — long enough to absorb a
/// burst, short enough that a rebuild shows up on the next refresh.
const PREVIEW_CACHE_TTL: Duration = Duration::from_millis(500);

/// The preview, or the message its failure would print. `anyhow::Error` is not
/// `Clone`, and the error text is all the page ever wanted from it.
type PreviewResult = Result<deploy::DeployPreview, String>;

/// Everything a deploy run keeps between requests, on `AppState` rather than
/// in statics so state cannot bleed across tests (or, one day, servers).
#[derive(Default)]
pub(super) struct DeployState {
    caches: deploy_status::StatusCaches,
    run: Mutex<Option<Run>>,
    token: OnceLock<String>,
    signer: Mutex<Option<Arc<crate::linker::LinkerState>>>,
    preview: Mutex<Vec<(PathBuf, Instant, Arc<PreviewResult>)>>,
    /// The wallet whose rights the pages check: the last signer, a connected
    /// account, or whatever the /target form was told. Read-only knowledge —
    /// nothing ever signs with it.
    address: Mutex<Option<String>>,
    rights: deploy_rights::RightsCache,
    /// Publishes this preview has seen, newest first.
    history: Mutex<VecDeque<PastRun>>,
    /// A Decentraland-account sign-in in flight, if any.
    connect: Mutex<Option<Connect>>,
    /// Cache keys a background warm-up is already fetching, so the reloads a
    /// warming page makes do not each start another fetch of the same thing.
    warming: Mutex<HashSet<String>>,
    /// The delegated deploy key the Connect-with-DCL flow minted, kept in
    /// memory: while it is present and unexpired, a publish signs itself with
    /// no wallet prompt. Dropped when the process stops.
    identity: Mutex<Option<deploy::DeployIdentity>>,
}

fn identity_slot(st: &AppState) -> std::sync::MutexGuard<'_, Option<deploy::DeployIdentity>> {
    st.deploy
        .identity
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// The live delegated identity, or None if none was minted or it lapsed —
/// an expired one is dropped here so the page falls back to the wallet.
fn live_identity(st: &AppState) -> Option<deploy::DeployIdentity> {
    let mut slot = identity_slot(st);
    match slot.as_ref() {
        Some(id) if id.expired(deploy::now_ms()) => {
            *slot = None;
            None
        }
        other => other.cloned(),
    }
}

fn connect_slot(st: &AppState) -> std::sync::MutexGuard<'_, Option<Connect>> {
    st.deploy
        .connect
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

fn warm_slot(st: &AppState) -> std::sync::MutexGuard<'_, HashSet<String>> {
    st.deploy
        .warming
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// A "connect a Decentraland account" hand-off: the authorize page the
/// person approves on, and what became of it. The proven address lands in
/// the address slot; this records only the wait.
#[derive(Clone)]
pub(super) struct Connect {
    url: String,
    state: ConnectState,
}

#[derive(Clone)]
enum ConnectState {
    Waiting,
    Failed(String),
}

/// How long a sign-in may stay pending before the page stops waiting on it.
const CONNECT_WINDOW: Duration = Duration::from_secs(300);

fn address_slot(st: &AppState) -> std::sync::MutexGuard<'_, Option<String>> {
    st.deploy
        .address
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

fn history_slot(st: &AppState) -> std::sync::MutexGuard<'_, VecDeque<PastRun>> {
    st.deploy
        .history
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// One finished publish, as the History tab tells it.
#[derive(Clone)]
pub(super) struct PastRun {
    at_ms: i64,
    target: String,
    signer: Option<String>,
    outcome: String,
}

/// The in-memory ring is this deep; the on-disk record keeps everything.
const HISTORY_KEPT: usize = 20;

/// Where a scene's publishes are recorded, beside its build artifacts.
fn history_path(root: &std::path::Path) -> PathBuf {
    root.join(".dcl-one").join("publishes.jsonl")
}

/// Appends to the scene's own record and the in-memory ring. Best-effort on
/// disk: a read-only checkout loses persistence, not publishing.
fn record_history(st: &AppState, past: PastRun) {
    if let Some(project) = st.first_project() {
        let line = serde_json::json!({
            "at_ms": past.at_ms,
            "target": past.target,
            "signer": past.signer,
            "outcome": past.outcome,
        });
        let path = history_path(&project.root);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            use std::io::Write;
            let _ = writeln!(f, "{line}");
        }
    }
    let mut h = history_slot(st);
    h.push_front(past);
    h.truncate(HISTORY_KEPT);
}

/// The rows the History tab draws: this process's ring, or — for a preview
/// that just started — the scene's on-disk record from earlier runs.
fn history_rows(st: &AppState, root: &std::path::Path) -> Vec<PastRun> {
    let held: Vec<PastRun> = history_slot(st).iter().cloned().collect();
    if !held.is_empty() {
        return held;
    }
    let Ok(text) = std::fs::read_to_string(history_path(root)) else {
        return Vec::new();
    };
    let mut out: Vec<PastRun> = text
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| {
            Some(PastRun {
                at_ms: v.get("at_ms")?.as_i64()?,
                target: v.get("target")?.as_str()?.to_string(),
                signer: v.get("signer").and_then(|s| s.as_str()).map(str::to_string),
                outcome: v.get("outcome")?.as_str()?.to_string(),
            })
        })
        .collect();
    out.reverse();
    out.truncate(HISTORY_KEPT);
    out
}

fn cache(st: &AppState) -> std::sync::MutexGuard<'_, Vec<(PathBuf, Instant, Arc<PreviewResult>)>> {
    st.deploy
        .preview
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// Walks the scene off the async worker, at most once per [`PREVIEW_CACHE_TTL`]
/// per scene.
async fn cached_preview(st: &AppState, project: &Project) -> Arc<PreviewResult> {
    let root = project.root.clone();
    // While a publish is running the walk's answer is irrelevant (and the
    // page polls every ~2s, faster than the TTL): serve whatever is held
    // rather than re-walking the tree beside the deploy's own build.
    let stale_ok = matches!(runs(st).as_ref().map(|r| &r.state), Some(RunState::Running));
    let hit = cache(st)
        .iter()
        .find(|(p, at, _)| *p == root && (stale_ok || at.elapsed() < PREVIEW_CACHE_TTL))
        .map(|(_, _, v)| v.clone());
    if let Some(hit) = hit {
        return hit;
    }
    let owned = project.clone();
    let computed =
        tokio::task::spawn_blocking(move || deploy::preview(&owned).map_err(|e| format!("{e:#}")))
            .await
            .unwrap_or_else(|e| Err(format!("the scene walk did not finish ({e})")));
    let entry = Arc::new(computed);
    let mut c = cache(st);
    c.retain(|(p, at, _)| *p != root && at.elapsed() < PREVIEW_CACHE_TTL);
    c.push((root, Instant::now(), entry.clone()));
    entry
}

fn signer_slot(
    st: &AppState,
) -> std::sync::MutexGuard<'_, Option<Arc<crate::linker::LinkerState>>> {
    st.deploy
        .signer
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// A CLI publish hosting itself on this server: the signer is live from the
/// first request, and the run slot says so, so the badge and /deploy tell the
/// same story the terminal does.
pub(super) fn adopt_cli_signing(st: &AppState, signer: Arc<crate::linker::LinkerState>) {
    let target = signer.target_content().to_string();
    *signer_slot(st) = Some(signer);
    *runs(st) = Some(Run {
        id: 0,
        started: Instant::now(),
        target,
        signing: Some("/deploy".to_string()),
        auto: false,
        print: String::new(),
        state: RunState::Running,
    });
}

/// The signing panel when a publish is waiting on a wallet, minted fresh per
/// render: each GET is a new candidate entity, and the id drawn is the id the
/// wallet signs. `None` when nothing is pending.
pub(super) fn pending_sign_panel(st: &AppState, prefix: &str) -> Option<String> {
    let state = signer_slot(st).clone()?;
    Some(crate::linker::sign_section(
        &state,
        &format!("{prefix}/deploy/sign"),
    ))
}

/// The account every page's bar shows, when one is known.
pub(super) fn known_account(st: &AppState) -> Option<String> {
    address_slot(st).clone()
}

/// Minted once per process and rendered only into the page body. An attacker
/// page cannot read it (same-origin) and cannot guess it (128 random bits), so
/// together with the loopback check it is what keeps `POST /deploy` from being
/// a publish button for anything that can reach the port.
pub(super) fn token(st: &AppState) -> &str {
    st.deploy.token.get_or_init(|| {
        let bytes: [u8; 16] = rand::random();
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    })
}

fn runs(st: &AppState) -> std::sync::MutexGuard<'_, Option<Run>> {
    st.deploy.run.lock().unwrap_or_else(PoisonError::into_inner)
}

struct Run {
    /// Which claim this is. The completion path presents it before writing a
    /// terminal state, so a slow deploy cannot stamp its outcome onto a run
    /// that replaced it.
    id: u64,
    started: Instant,
    target: String,
    /// The wallet-signing page's URL, knowable up front because the POST
    /// handler picks the port and hands it to the deploy: the linker binds it
    /// once the build finishes, and until then the link simply refuses to
    /// connect, which the copy says.
    signing: Option<String>,
    /// Started by opening /deploy rather than pressing the button. The build
    /// is small, so entering the page IS the intent to publish — but a run
    /// nobody signed ends quietly instead of wearing a failure panel.
    auto: bool,
    /// The payload fingerprint this run was claimed against, so the page can
    /// tell when the tree moved under a pending signature. Empty for an
    /// adopted CLI signing — that publish belongs to a terminal, and the
    /// page never re-mints it.
    print: String,
    state: RunState,
}

/// One counter for every way a run comes to exist, so a re-mint can never
/// collide with a claim.
static NEXT_RUN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

enum RunState {
    Running,
    Done,
    Failed(String),
    /// The payload moved under the page. Nothing was published.
    Stale(Vec<String>),
}

#[derive(serde::Deserialize)]
pub(super) struct DeployForm {
    token: String,
    #[serde(default)]
    fingerprint: String,
}

/// What the page drew, in one line: every publishable path with its size and
/// mtime, plus the bundle's state. Size alone is not enough — the edit that
/// changes a character and not the length is the common one — so the mtime the
/// walk already had to stat for goes in too.
fn fingerprint(root: &std::path::Path, p: &deploy::DeployPreview) -> String {
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    digest.update(format!("{:?}\n", p.main).as_bytes());
    for (rel, len) in &p.files {
        let mtime = std::fs::metadata(root.join(rel))
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis())
            .unwrap_or(0);
        digest.update(format!("{rel}\u{1f}{len:?}\u{1f}{mtime}\n").as_bytes());
    }
    digest
        .finalize()
        .iter()
        .take(12)
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The paths whose size or mtime moved between two walks, so the page can name
/// what changed rather than only that something did.
/// Names what changed, for the refusal message. It does NOT decide whether
/// anything changed — the digest does, at the call site. This only knows about
/// files that still exist and were touched recently, so as a decision it fails
/// open on a deletion or an edit older than the window.
fn moved_since(root: &std::path::Path, p: &deploy::DeployPreview) -> Vec<String> {
    let mut out: Vec<String> = p
        .files
        .iter()
        .filter(|(rel, _)| {
            std::fs::metadata(root.join(rel))
                .and_then(|m| m.modified())
                .map(|t| {
                    t.elapsed()
                        .map(|e| e < Duration::from_secs(300))
                        .unwrap_or(false)
                })
                .unwrap_or(false)
        })
        .map(|(rel, _)| rel.clone())
        .collect();
    out.truncate(6);
    out
}

/// `POST /deploy`, five gates in order, each one a found bypass: loopback
/// peer that is not a tunnel replay (the agent stamps
/// [`crate::tunnel::FORWARDED_HEADER`]); same origin, because with CORS the
/// token was fetchable out of the page HTML; the token itself; one run
/// claimed in the lock that checks (two builds over one `bin/` can sign
/// mid-write bytes); and the payload fingerprint the page drew. Refuses
/// `DCL_PRIVATE_KEY` signing (one click must not be an unattended publish).
/// Replies with a redirect so refresh cannot resubmit. `multi_scene: true`
/// is load-bearing (false silently deletes the world's other scenes), and
/// the inner spawn's JoinHandle is awaited so a deploy panic still reaches a
/// terminal state.
pub(super) async fn start(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(form): Form<DeployForm>,
) -> Response {
    let prefix = forwarded_prefix(&headers);
    let back = Redirect::to(&format!("{prefix}/deploy"));
    let refuse = |why: &str| (StatusCode::FORBIDDEN, format!("{why}\n")).into_response();

    if remote_peer(st.allow_remote_deploy, peer, &headers) {
        return refuse(
            "publishing runs on the machine hosting this preview; start it with --allow-remote-deploy to publish from elsewhere",
        );
    }
    if let Some(why) = cross_origin_refusal(&headers) {
        return refuse(why);
    }
    if form.token != token(&st) {
        return refuse(
            "stale or missing deploy token \u{2014} reload /deploy and press the button there",
        );
    }

    if !st.deploy_dry_run && std::env::var_os("DCL_PRIVATE_KEY").is_some() {
        return refuse(
            "DCL_PRIVATE_KEY is set, so this deploy would upload with no wallet prompt; run dcl-one-sdk deploy in a terminal instead",
        );
    }

    let Some(project) = st.first_project() else {
        return back.into_response();
    };

    let dest = resolve_dest(
        &project.scene_json,
        deploy::env_default_target().as_deref(),
        deploy::configured_catalyst_rotation(),
    );
    let Some(id) = claim(&st, dest.headline.clone(), false, form.fingerprint.clone()) else {
        return back.into_response();
    };
    let root = project.root.clone();
    let owned = project.clone();
    let fresh = match tokio::task::spawn_blocking(move || deploy::preview(&owned)).await {
        Ok(Ok(p)) => p,
        _ => {
            finish(
                &st,
                id,
                RunState::Failed("the scene could not be read".into()),
            );
            return back.into_response();
        }
    };

    if form.fingerprint.is_empty() || fingerprint(&root, &fresh) != form.fingerprint {
        finish(&st, id, RunState::Stale(moved_since(&root, &fresh)));
        return back.into_response();
    }
    launch(st, root, id);
    back.into_response()
}

/// Everything past the gates, shared by the button and the page's own
/// auto-start: the linker hands its signing state to THIS server — a
/// page-driven publish involves exactly one server, and the signing URL is a
/// path on this origin, set on the run only once the signer is actually
/// live — and the deploy runs to a terminal state.
fn launch(st: Arc<AppState>, root: PathBuf, id: u64) {
    // A live delegated identity signs the deploy itself — no browser panel,
    // no signature wait: the run builds and uploads straight through. Only
    // when there is none does the deploy host a wallet signing page.
    let identity = live_identity(&st);
    let host_signer = match identity.is_some() {
        true => None,
        false => {
            let register_st = st.clone();
            Some(crate::linker::HostSigner {
                register: Arc::new(move |state| {
                    // Only the run that still owns the slot gets to install
                    // its signer: a build that finished after its run was
                    // re-minted must not hand the wallet panel a stale
                    // entity. Lock order runs → signer, shared with
                    // [`drift_reclaim`].
                    let mut slot = runs(&register_st);
                    if let Some(r) = slot.as_mut() {
                        if r.id == id {
                            r.signing = Some("/deploy".to_string());
                            *signer_slot(&register_st) = Some(state);
                        }
                    }
                }),
                url: format!("http://127.0.0.1:{}/deploy", st.port),
            })
        }
    };
    let opts = deploy::DeployOptions {
        dir: root.clone(),
        target: None,
        target_content: None,
        sign_key: None,
        skip_build: false,
        dry_run: st.deploy_dry_run,
        timestamp: None,
        entity_out: None,
        multi_scene: true,
        yes: true,
        no_browser: true,
        ci: false,
        port: None,
        quiet: true,
        host_signer,
        identity,
    };
    tokio::spawn(async move {
        let handle = tokio::spawn(async move { deploy::deploy(&opts).await });
        let state = match handle.await {
            Ok(Ok(())) => RunState::Done,
            Ok(Err(e)) => {
                // The page is the terminal for a page-driven publish: the
                // fully rendered error — the why lines (a type check's
                // diagnostics live there) and the try steps — lands in the
                // panel, not just the headline of the chain.
                let rendered = crate::ux::render(&e, false, false);
                let rendered = rendered
                    .trim_start()
                    .strip_prefix("Error:")
                    .map(str::trim_start)
                    .unwrap_or(&rendered)
                    .to_string();
                RunState::Failed(scrub_paths(&rendered, &root))
            }
            Err(_) => RunState::Failed("the deploy did not finish".into()),
        };
        finish(&st, id, state);
    });
}

#[derive(serde::Deserialize)]
pub(super) struct PreflightReq {
    address: String,
}

/// Whether the rights oracle actually speaks for `target_content`: any host
/// of the public Genesis network (its reads are network-wide consistent),
/// the public worlds server, or the very server the destination reads from.
fn oracle_speaks_for(dest: &Dest, target_content: &str) -> bool {
    let Some(host) = deploy::host_of(target_content) else {
        return false;
    };
    let matches = |url: &str| deploy::host_of(url).is_some_and(|h| h.eq_ignore_ascii_case(&host));
    deploy::UPSTREAM_CATALYST_HOSTS.iter().any(|u| matches(u))
        || matches(deploy::WORLDS_CONTENT_SERVER)
        || dest.read_bases.iter().any(|b| matches(b))
}

/// `POST /deploy/preflight` — the verdict for one address against the
/// scene's declared target, asked by the sign panel in the moment between
/// "which wallet" and "sign": the refusal a catalyst would issue after the
/// signature is said before it, to the exact address about to sign. Gated
/// like every signing route, and the answer rides the same cache the target
/// page reads.
pub(super) async fn preflight(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Result<Json<PreflightReq>, axum::extract::rejection::JsonRejection>,
) -> Response {
    if let Some(blocked) = signing_blocked(&st, peer, &headers) {
        return blocked;
    }
    if let Some(why) = cross_origin_refusal(&headers) {
        return (StatusCode::FORBIDDEN, format!("{why}\n")).into_response();
    }
    let Ok(Json(req)) = body else {
        return (StatusCode::UNPROCESSABLE_ENTITY, "not an address\n").into_response();
    };
    let address = req.address.trim().to_lowercase();
    if !deploy_rights::valid_address(&address) {
        return (StatusCode::UNPROCESSABLE_ENTITY, "not an address\n").into_response();
    }
    let Some(project) = st.first_project() else {
        return (StatusCode::NOT_FOUND, "no scene loaded\n").into_response();
    };
    let dest = resolve_dest(
        &project.scene_json,
        deploy::env_default_target().as_deref(),
        deploy::configured_catalyst_rotation(),
    );
    // The subject is what the pending signature would actually upload to: a
    // CLI-hosted run may carry an explicit --target-content the rights
    // oracle has no authority over (a self-hosted node with its own deploy
    // policy), and a mainnet-keyed refusal must never block a server that
    // would say yes. Hard verdicts only where the oracle speaks for the
    // target; elsewhere, an honest shrug and the server decides.
    if let Some(signer) = signer_slot(&st).clone() {
        let actual = signer.target_content().to_string();
        if !oracle_speaks_for(&dest, &actual) {
            return Json(serde_json::json!({
                "verdict": "unchecked",
                "why": format!(
                    "this publish goes to {}, whose deploy policy this preview cannot read \u{2014} that server decides",
                    deploy_status::host_of(&actual)
                ),
                "remedy": null,
            }))
            .into_response();
        }
    }
    if st.deploy_dry_run {
        return Json(serde_json::json!({
            "verdict": "unchecked",
            "why": "live checks are off for this run",
            "remedy": null,
        }))
        .into_response();
    }
    let rights = deploy_rights::cached_rights(&st.deploy.rights, &dest, &address).await;
    let (verdict, why, remedy) = match &rights.verdict {
        Verdict::May(w) => ("may", w.clone(), None),
        Verdict::MayNot { why, remedy } => ("may_not", why.clone(), Some(remedy.clone())),
        Verdict::Unchecked(w) => ("unchecked", w.clone(), None),
    };
    Json(serde_json::json!({ "verdict": verdict, "why": why, "remedy": remedy })).into_response()
}

/// The gate every signing route shares with `start`'s first one: the wallet
/// signs on the hosting machine, so only a loopback peer that is not a tunnel
/// replay reaches the signer — unless --allow-remote-deploy opened it up.
fn signing_blocked(st: &AppState, peer: SocketAddr, headers: &HeaderMap) -> Option<Response> {
    remote_peer(st.allow_remote_deploy, peer, headers).then(|| {
        (
            StatusCode::FORBIDDEN,
            "signing happens on the machine hosting this preview\n",
        )
            .into_response()
    })
}

/// `POST /deploy/sign` — the one signing endpoint: the panel it answers is
/// server-rendered inline on `/` and `/deploy`, so there is no signing page of
/// its own.
pub(super) async fn sign_submit(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Result<Json<crate::linker::SignReq>, axum::extract::rejection::JsonRejection>,
) -> Response {
    if let Some(blocked) = signing_blocked(&st, peer, &headers) {
        return blocked;
    }
    if let Some(why) = cross_origin_refusal(&headers) {
        return (StatusCode::FORBIDDEN, format!("{why}\n")).into_response();
    }
    let Ok(body) = body else {
        return (StatusCode::UNPROCESSABLE_ENTITY, "not a signature\n").into_response();
    };
    let state = signer_slot(&st).clone();
    match state {
        Some(state) => crate::linker::sign(State(state), body)
            .await
            .into_response(),
        None => (StatusCode::NOT_FOUND, "no signature is pending\n").into_response(),
    }
}

/// Check and claim in one acquisition, returning the id the completion path
/// must present before it may write a terminal state.
fn claim(st: &AppState, target: String, auto: bool, print: String) -> Option<u64> {
    let mut slot = runs(st);
    if matches!(slot.as_ref().map(|r| &r.state), Some(RunState::Running)) {
        return None;
    }
    let id = NEXT_RUN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    *slot = Some(Run {
        id,
        started: Instant::now(),
        target,
        signing: None,
        auto,
        print,
        state: RunState::Running,
    });
    Some(id)
}

/// A pending run whose payload moved is a signature waiting to be wrong: the
/// entity was minted from the tree as it was, and the wallet panel would
/// happily sign it against the tree as it is. This replaces such a run — new
/// id, current fingerprint, signer slot emptied so the panel cannot route to
/// the stale entity — but only in the window where a re-mint is safe: the
/// old build finished (its signer registered — two builds over one release
/// tree can sign mid-write bytes), the wallet has not answered (a signed
/// deploy is past recall), and the run is the page's own (an adopted CLI
/// signing carries no fingerprint and belongs to a terminal). The provenance
/// survives: an auto run re-mints auto, a button run stays a button run.
fn drift_reclaim(st: &AppState, target: String, print: &str) -> Option<u64> {
    let mut slot = runs(st);
    let auto = match slot.as_ref() {
        Some(r)
            if matches!(r.state, RunState::Running)
                && r.signing.is_some()
                && !r.print.is_empty()
                && r.print != print =>
        {
            r.auto
        }
        _ => return None,
    };
    {
        // Lock order runs → signer, same as the register closure in
        // [`launch`], so the two cannot deadlock.
        let mut signer = signer_slot(st);
        if signer
            .as_ref()
            .is_some_and(|s| s.signer_address().is_some())
        {
            return None;
        }
        *signer = None;
    }
    let id = NEXT_RUN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    *slot = Some(Run {
        id,
        started: Instant::now(),
        target,
        signing: None,
        auto,
        print: print.to_string(),
        state: RunState::Running,
    });
    Some(id)
}

/// Write a terminal state, but only onto the run that claimed the slot: a
/// finishing deploy must not stamp `Done` over a later run's `Running`, nor
/// over the refusal that replaced it. A terminal state also retires the
/// live-status cache, so the page's "on the server now" reflects the publish
/// on the next refresh instead of thirty seconds later — and it is the one
/// moment the signer names its wallet, so the address and the history row
/// are harvested here, before the signer slot empties.
fn finish(st: &AppState, id: u64, state: RunState) {
    // An auto-started run nobody signed is not news: the page was opened and
    // left, that is all. The slot clears so the next visit builds afresh, and
    // neither a failure panel nor a history row claims something happened.
    if let RunState::Failed(why) = &state {
        if why.contains("no signature arrived") {
            let mut slot = runs(st);
            if slot.as_ref().is_some_and(|r| r.id == id && r.auto) {
                *slot = None;
                drop(slot);
                *signer_slot(st) = None;
                return;
            }
        }
    }
    let outcome = match &state {
        RunState::Done => "published",
        RunState::Failed(_) => "failed",
        RunState::Stale(_) => "nothing published",
        RunState::Running => "",
    };
    let target = {
        let mut slot = runs(st);
        match slot.as_mut() {
            Some(r) if r.id == id => {
                let target = r.target.clone();
                r.state = state;
                Some(target)
            }
            _ => None,
        }
    };
    // A superseded run's tail: the slot belongs to a newer claim, and the
    // signer waiting there is the newer run's — touch nothing.
    let Some(target) = target else {
        return;
    };
    let signer = signer_slot(st).as_ref().and_then(|s| s.signer_address());
    if let Some(addr) = &signer {
        *address_slot(st) = Some(addr.to_lowercase());
    }
    if !outcome.is_empty() {
        record_history(
            st,
            PastRun {
                at_ms: deploy::now_ms(),
                target,
                signer,
                outcome: outcome.to_string(),
            },
        );
    }
    *signer_slot(st) = None;
    st.deploy.caches.clear();
}

#[derive(serde::Deserialize)]
pub(super) struct AddressForm {
    token: String,
    #[serde(default)]
    address: String,
}

/// `POST /target/address` — which wallet the pages check rights for. Gated
/// like the publish POST (loopback peer, same origin, the page token): the
/// address is only read, but it decides what an open page renders, and a
/// stranger on the LAN does not get to choose that. An empty address forgets.
pub(super) async fn target_address(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(form): Form<AddressForm>,
) -> Response {
    let prefix = forwarded_prefix(&headers);
    let refuse = |why: &str| (StatusCode::FORBIDDEN, format!("{why}\n")).into_response();
    if remote_peer(st.allow_remote_deploy, peer, &headers) {
        return refuse("the wallet this preview checks is chosen on the machine hosting it");
    }
    if let Some(why) = cross_origin_refusal(&headers) {
        return refuse(why);
    }
    if form.token != token(&st) {
        return refuse("stale or missing token \u{2014} reload /target and use the form there");
    }
    let trimmed = form.address.trim();
    if trimmed.is_empty() {
        *address_slot(&st) = None;
    } else if deploy_rights::valid_address(trimmed) {
        *address_slot(&st) = Some(trimmed.to_lowercase());
    } else {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            "that is not an Ethereum address\n",
        )
            .into_response();
    }
    Redirect::to(&format!("{prefix}/target")).into_response()
}

#[derive(serde::Deserialize)]
pub(super) struct ConnectForm {
    token: String,
}

/// `POST /target/connect` — starts a Decentraland-account sign-in: the same
/// gates as every mutating POST here, then the browser is bounced onto the
/// authorize page — the configured target's own domain, or catalyst.example.com — with a
/// throwaway session key and this process's own id in the query. The person
/// approves in the browser they are already signed into, the page relays
/// the signed delegation, and a background task picks it up from the
/// single-read relay and remembers the address the signature proves. No
/// server is asked anything up front, so there is nothing here to fail.
pub(super) async fn target_connect(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(form): Form<ConnectForm>,
) -> Response {
    let refuse = |why: &str| (StatusCode::FORBIDDEN, format!("{why}\n")).into_response();
    if remote_peer(st.allow_remote_deploy, peer, &headers) {
        return refuse("accounts connect on the machine hosting this preview");
    }
    if let Some(why) = cross_origin_refusal(&headers) {
        return refuse(why);
    }
    if form.token != token(&st) {
        return refuse("stale or missing token \u{2014} reload /target and use the button there");
    }
    if st.deploy_dry_run {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "sign-in is off for this run\n",
        )
            .into_response();
    }
    let bases = deploy_rights::working_auth_bases(deploy::env_default_target().as_deref()).await;
    // A real, throwaway session key. Its secret is kept — in memory, on this
    // slot — so a deploy can sign with it for the delegation's hour without
    // asking the wallet again; the wallet's signature over its address is
    // what the relay returns.
    let (ephemeral, ephemeral_key) = loop {
        let raw: [u8; 32] = rand::random();
        let hex: String = raw.iter().map(|b| format!("{b:02x}")).collect();
        if let Ok(w) = catalyrst_crypto::Wallet::from_hex(&hex) {
            break (w.address().to_lowercase(), hex);
        }
    };
    let id: String = {
        let raw: [u8; 16] = rand::random();
        raw.iter().map(|b| format!("{b:02x}")).collect()
    };
    // Millisecond ISO, because the page round-trips it through a JS Date and
    // the signed text must come out byte-identical.
    let expiration = (chrono::Utc::now() + chrono::Duration::hours(1))
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string();
    let url = format!(
        "{}?id={id}&ephemeral={ephemeral}&expiration={expiration}",
        bases.page
    );
    *connect_slot(&st) = Some(Connect {
        url: url.clone(),
        state: ConnectState::Waiting,
    });
    spawn_connect_poll(
        st.clone(),
        bases.relay,
        id,
        ephemeral,
        ephemeral_key,
        expiration,
    );
    Redirect::to(&url).into_response()
}

/// Waits on the relay for the approval. Success stores the proven address
/// and clears the wait; a forged or mismatched entry becomes a sentence the
/// account row shows. The deadline is this task's, not the relay's: a page
/// abandoned mid-approval stops costing requests after [`CONNECT_WINDOW`].
fn spawn_connect_poll(
    st: Arc<AppState>,
    relay: String,
    id: String,
    ephemeral: String,
    ephemeral_key: String,
    expiration: String,
) {
    tokio::spawn(async move {
        let deadline = Instant::now() + CONNECT_WINDOW;
        let fail = |st: &AppState, why: String| {
            if let Some(c) = connect_slot(st).as_mut() {
                c.state = ConnectState::Failed(why);
            }
        };
        loop {
            if Instant::now() >= deadline {
                fail(
                    &st,
                    "the sign-in expired \u{2014} connect again".to_string(),
                );
                return;
            }
            let resp = deploy_status::status_client()
                .get(format!("{relay}?id={id}"))
                .send()
                .await;
            if let Ok(resp) = resp {
                let status = resp.status().as_u16();
                if status == 200 {
                    let Ok(v) = resp.json::<serde_json::Value>().await else {
                        fail(&st, "the relay sent an unreadable answer".to_string());
                        return;
                    };
                    match deploy_rights::relayed_address(&ephemeral, &expiration, &v) {
                        Ok(addr) => {
                            // The same verified approval that names the
                            // address also delegates the ephemeral key: keep
                            // both, and deploys sign themselves for the hour.
                            if let Some(sig) = v.get("signature").and_then(|s| s.as_str()) {
                                *identity_slot(&st) = Some(deploy::DeployIdentity {
                                    signer: addr.clone(),
                                    ephemeral_key: ephemeral_key.clone(),
                                    delegation_payload: deploy_rights::ephemeral_message(
                                        &ephemeral,
                                        &expiration,
                                    ),
                                    delegation_signature: sig.to_string(),
                                    expiration_ms: chrono::DateTime::parse_from_rfc3339(
                                        &expiration,
                                    )
                                    .map(|t| t.timestamp_millis())
                                    .unwrap_or_else(|_| deploy::now_ms()),
                                });
                            }
                            *address_slot(&st) = Some(addr);
                            *connect_slot(&st) = None;
                        }
                        Err(why) => fail(&st, why),
                    }
                    return;
                }
                // 204 is "nothing yet"; a 4xx would repeat forever, so it
                // fails the wait once instead.
                if (400..500).contains(&status) {
                    fail(&st, format!("the relay refused the wait (HTTP {status})"));
                    return;
                }
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });
}

#[derive(serde::Deserialize)]
pub(super) struct PointForm {
    token: String,
    #[serde(default)]
    world: String,
}

/// A world name the pointer will write into scene.json: the worlds tier's
/// own dialect, nothing else.
fn valid_world_name(w: &str) -> bool {
    w.len() <= 100
        && w.ends_with(".eth")
        && w.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-')
}

/// `POST /target/point` — re-aims the scene. A world name writes
/// `worldConfiguration.name` into scene.json; an empty value removes
/// `worldConfiguration`, and the scene deploys to its declared parcels on
/// Genesis City. The file is what changes, because the file is what a deploy
/// reads — the page never publishes anywhere it does not say. Gated like the
/// scene editors, and like them never opened by --allow-remote-deploy:
/// scene.json is the developer's file.
pub(super) async fn target_point(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(form): Form<PointForm>,
) -> Response {
    let prefix = forwarded_prefix(&headers);
    let refuse = |why: &str| (StatusCode::FORBIDDEN, format!("{why}\n")).into_response();
    if remote_peer(false, peer, &headers) {
        return refuse("the scene's destination is chosen on the machine hosting this preview");
    }
    if let Some(why) = cross_origin_refusal(&headers) {
        return refuse(why);
    }
    if form.token != token(&st) {
        return refuse("stale or missing token \u{2014} reload /target and use the button there");
    }
    let Some(project) = st.first_project() else {
        return (StatusCode::NOT_FOUND, "no scene loaded\n").into_response();
    };
    let world = form.world.trim().to_lowercase();
    if !world.is_empty() && !valid_world_name(&world) {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            "that is not a world name\n",
        )
            .into_response();
    }
    let write_st = st.clone();
    let root = project.root.clone();
    let w = world.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        super::edit::edit_scene_json(&write_st, &root, |scene| {
            let obj = scene.as_object_mut().expect("edit_scene_json checked");
            if w.is_empty() {
                obj.remove("worldConfiguration");
                return Ok(());
            }
            match obj
                .entry("worldConfiguration")
                .or_insert_with(|| serde_json::json!({}))
                .as_object_mut()
            {
                Some(wc) => {
                    wc.insert("name".to_string(), serde_json::Value::String(w));
                }
                None => return Err("worldConfiguration in scene.json is not an object".into()),
            }
            Ok(())
        })
    })
    .await;
    match outcome {
        Ok(Ok(_)) => {
            // The destination moved, so everything keyed on it is stale now,
            // not in thirty seconds.
            st.deploy.caches.clear();
            st.deploy
                .rights
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clear();
            Redirect::to(&format!("{prefix}/target")).into_response()
        }
        Ok(Err((status, why))) => (status, format!("{why}\n")).into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "the change did not finish\n",
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
pub(super) struct BaseForm {
    token: String,
    #[serde(default)]
    base: String,
}

/// The Genesis map's coordinate clamp — the LAND contract's own bounds,
/// district expansions included. Worlds lay their scenes out in the same
/// coordinate space, so one range serves both destination shapes.
fn in_genesis(p: (i64, i64)) -> bool {
    let r = -150..=163;
    r.contains(&p.0) && r.contains(&p.1)
}

/// The whole footprint moved so the base parcel lands on `new_base`. The
/// shape survives: every parcel translates by the same delta, and spawn
/// points ride along because they are metres from the base, not map
/// coordinates.
fn translate_footprint(scene: &mut serde_json::Value, new_base: (i64, i64)) -> Result<(), String> {
    let obj = scene.as_object_mut().expect("edit_scene_json checked");
    let sc = obj
        .get_mut("scene")
        .and_then(|s| s.as_object_mut())
        .ok_or_else(|| "scene.json has no scene object".to_string())?;
    let parcels: Vec<(i64, i64)> = sc
        .get("parcels")
        .and_then(|p| p.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .filter_map(catalyrst_types::pointer::parse_pointer)
                .collect()
        })
        .unwrap_or_default();
    if parcels.is_empty() {
        return Err("scene.json declares no parcels to move".into());
    }
    let old_base = sc
        .get("base")
        .and_then(|b| b.as_str())
        .and_then(catalyrst_types::pointer::parse_pointer)
        .unwrap_or(parcels[0]);
    let (dx, dy) = (new_base.0 - old_base.0, new_base.1 - old_base.1);
    if (dx, dy) == (0, 0) {
        return Ok(());
    }
    let moved: Vec<(i64, i64)> = parcels.iter().map(|(x, y)| (x + dx, y + dy)).collect();
    if let Some((ox, oy)) = moved.iter().find(|p| !in_genesis(**p)) {
        return Err(format!(
            "moving there puts parcel {ox},{oy} outside the Genesis map"
        ));
    }
    sc.insert(
        "parcels".into(),
        serde_json::json!(moved
            .iter()
            .map(|(x, y)| format!("{x},{y}"))
            .collect::<Vec<_>>()),
    );
    sc.insert(
        "base".into(),
        serde_json::json!(format!("{},{}", new_base.0, new_base.1)),
    );
    Ok(())
}

/// `POST /target/base` — moves the scene's whole footprint so its base
/// parcel lands where the form says (see [`translate_footprint`]). Gated
/// exactly like `/target/point`: scene.json is the developer's file.
pub(super) async fn target_base(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(form): Form<BaseForm>,
) -> Response {
    let prefix = forwarded_prefix(&headers);
    let refuse = |why: &str| (StatusCode::FORBIDDEN, format!("{why}\n")).into_response();
    if remote_peer(false, peer, &headers) {
        return refuse("the scene's base parcel is chosen on the machine hosting this preview");
    }
    if let Some(why) = cross_origin_refusal(&headers) {
        return refuse(why);
    }
    if form.token != token(&st) {
        return refuse("stale or missing token \u{2014} reload /target and use the form there");
    }
    let Some(project) = st.first_project() else {
        return (StatusCode::NOT_FOUND, "no scene loaded\n").into_response();
    };
    let Some(new_base) = catalyrst_types::pointer::parse_pointer(form.base.trim()) else {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            "that is not an x,y parcel\n",
        )
            .into_response();
    };
    if !in_genesis(new_base) {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            "that parcel is outside the Genesis map\n",
        )
            .into_response();
    }
    let write_st = st.clone();
    let root = project.root.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        super::edit::edit_scene_json(&write_st, &root, |scene| {
            translate_footprint(scene, new_base)
        })
    })
    .await;
    match outcome {
        Ok(Ok(_)) => {
            // The footprint moved, so everything keyed on the pointers is
            // stale now, not in thirty seconds.
            st.deploy.caches.clear();
            st.deploy
                .rights
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clear();
            Redirect::to(&format!("{prefix}/target")).into_response()
        }
        Ok(Err((status, why))) => (status, format!("{why}\n")).into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "the change did not finish\n",
        )
            .into_response(),
    }
}

/// The rights view for the remembered address, or `None` when no wallet is
/// known yet. The dry-run seam answers without the network, like the live
/// status does.
async fn page_rights(st: &AppState, dest: &Dest) -> Option<Arc<Rights>> {
    let address = address_slot(st).clone()?;
    if st.deploy_dry_run {
        return Some(Arc::new(Rights::unchecked(
            &address,
            "Live checks are off for this run",
        )));
    }
    Some(deploy_rights::cached_rights(&st.deploy.rights, dest, &address).await)
}

/// The live status and the wallet's rights, without ever making the page
/// wait for a server: warm caches answer as before, and a cold one comes
/// back as a "checking now" placeholder while a spawned task fetches the
/// real answer into the cache — the page renders NOW and reloads itself
/// into the data (`warming` is the third return, and it puts the marker the
/// reload script keys on into the body). Every fetch outcome is a cached
/// value — failures are sentences, not retries — so the reload loop always
/// lands. The warming set keeps the reloads themselves from each starting
/// another fetch of the same key.
async fn status_and_rights(
    st: &Arc<AppState>,
    project: &Project,
    dest: &Dest,
    preview: &Arc<PreviewResult>,
    print: &str,
) -> (Arc<LiveStatus>, Option<Arc<Rights>>, bool) {
    if st.deploy_dry_run {
        return (
            Arc::new(LiveStatus::unknown("Live checks are off for this run")),
            page_rights(st, dest).await,
            false,
        );
    }
    let address = address_slot(st).clone();
    let status = deploy_status::status_peek(&st.deploy.caches, dest, print);
    let rights = address
        .as_deref()
        .and_then(|a| deploy_rights::rights_peek(&st.deploy.rights, dest, a));
    let cold_status = status.is_none();
    let cold_rights = address.is_some() && rights.is_none();
    if !cold_status && !cold_rights {
        return (status.expect("not cold"), rights, false);
    }
    {
        let mut warming = warm_slot(st);
        if cold_status {
            let key = format!("status|{}|{print}", dest.headline);
            if warming.insert(key.clone()) {
                let st = st.clone();
                let project = project.clone();
                let dest = dest.clone();
                let preview = preview.clone();
                let print = print.to_string();
                tokio::spawn(async move {
                    if let Ok(p) = &*preview {
                        cached_status(&st.deploy.caches, &project, &dest, p, &print).await;
                    }
                    warm_slot(&st).remove(&key);
                });
            }
        }
        if let (true, Some(address)) = (cold_rights, address.clone()) {
            let key = format!("rights|{address}|{}", dest.headline);
            if warming.insert(key.clone()) {
                let st = st.clone();
                let dest = dest.clone();
                tokio::spawn(async move {
                    deploy_rights::cached_rights(&st.deploy.rights, &dest, &address).await;
                    warm_slot(&st).remove(&key);
                });
            }
        }
    }
    let status =
        status.unwrap_or_else(|| Arc::new(LiveStatus::unknown("Checking the destination now")));
    // The synthesized rights keep the card from telling a connected wallet
    // it has nothing while the real answer is still in flight.
    let rights = rights.or_else(|| {
        address.map(|a| {
            Arc::new(Rights::unchecked(
                &a,
                "Checking what this wallet may publish",
            ))
        })
    });
    (status, rights, true)
}

/// What a warming render adds to the body: the marker the shared script's
/// reload keys on, and the no-JS fallback that does the same job slower.
fn warming_marker(warming: bool) -> &'static str {
    match warming {
        true => {
            r#"<div id="page-warming" hidden></div><noscript><meta http-equiv="refresh" content="2"></noscript>"#
        }
        false => "",
    }
}

// Destination: resolved the way `deploy::net::resolve_target_from` will
// resolve it when the button is pressed — env override first, then the world
// name onto the public worlds server, then the Genesis rotation.

/// The page's one script, inlined like the landing page's editor: it posts
/// the same form the no-JS page posts and re-fetches this page's own HTML to
/// follow the run, so the server stays the single renderer. Without it, the
/// plain form POST and the `<noscript>` meta refresh do the same job slower.
const SCRIPT: &str = concat!(
    include_str!("page_common.js"),
    include_str!("sign_flow.js"),
    include_str!("deploy_page.js")
);

/// The status region: always rendered, so the script has one stable element
/// to swap and one `data-state` to read. While a run is in flight the region
/// carries a `<noscript>` meta refresh — the no-JS way to follow the run —
/// and the signing path as a data attribute: part of the shape the script
/// compares before swapping, so a live wallet panel is never wiped mid-flow.
/// `show_sign` carries the signing gate's answer for the requesting peer:
/// the panel (payload facts, entity id, wallet button) renders only for a
/// peer `sign_submit` would accept — the posture the old standalone sign
/// page held with its 403.
fn run_region(st: &AppState, prefix: &str, jump: Option<&str>, show_sign: bool) -> String {
    let sign_panel = show_sign.then(|| pending_sign_panel(st, prefix)).flatten();
    run_region_for(prefix, runs(st).as_ref(), jump, sign_panel.as_deref())
}

/// Pure over the run state, so every state can be rendered by a test without
/// mutating the process-wide slot other tests read through `served`.
fn run_region_for(
    prefix: &str,
    run: Option<&Run>,
    jump: Option<&str>,
    sign_panel: Option<&str>,
) -> String {
    let state = match run.map(|r| &r.state) {
        None => "idle",
        Some(RunState::Running) => "running",
        Some(RunState::Done) => "done",
        Some(RunState::Failed(_)) => "failed",
        Some(RunState::Stale(_)) => "stale",
    };
    let signing = run
        .filter(|r| matches!(r.state, RunState::Running))
        .and_then(|r| r.signing.as_deref())
        .map(|path| format!(r#" data-signing="{}""#, esc(&format!("{prefix}{path}"))))
        .unwrap_or_default();
    format!(
        r#"<div id="run-status" data-state="{state}"{signing}>{panel}</div>"#,
        panel = run_panel_for(run, jump, sign_panel)
    )
}

fn run_panel_for(run: Option<&Run>, jump: Option<&str>, sign_panel: Option<&str>) -> String {
    let Some(r) = run else {
        return String::new();
    };
    let (title, body, refresh, tone) = match &r.state {
        RunState::Running => {
            let refresh = r#"<noscript><meta http-equiv="refresh" content="2"></noscript>"#;
            if let Some(panel) = sign_panel {
                return format!("{refresh}{panel}");
            }
            (
                "Publishing",
                format!(
                    "<div class=\"dep__wait\"><div class=\"spin\" role=\"status\" aria-label=\"building\"></div>\
                     <p class=\"note\">Building \u{2014} running for {}s. The wallet hand-off appears here once \
                     the build finishes; nothing uploads until your wallet answers.</p></div>",
                    r.started.elapsed().as_secs()
                ),
                refresh,
                "",
            )
        }
        RunState::Done => {
            let jump = match jump {
                Some(url) => format!(r#" <a class="knob__go" href="{}">Jump in</a>"#, esc(url)),
                None => String::new(),
            };
            (
                "Published",
                format!(
                    r#"<p class="note dep__ok">Uploaded to {}.</p>{jump}"#,
                    esc(&r.target)
                ),
                "",
                " panel--ok",
            )
        }
        RunState::Failed(why) => (
            "Deploy failed",
            format!(r#"<pre class="note dep__err">{}</pre>"#, esc(why)),
            "",
            " panel--warn",
        ),
        RunState::Stale(paths) => (
            "Nothing was published",
            format!(
                "<p class=\"note\">The scene changed while this page was open, so the payload \
                 you saw is not the one that would have gone up: {}. Check the numbers and \
                 press the button again.</p>",
                esc(&paths.join(", "))
            ),
            "",
            " panel--warn",
        ),
    };
    format!(r#"{refresh}<div class="panel{tone}"><h2>{title}</h2>{body}</div>"#)
}

/// The chrome every /deploy and /target page variant shares.
fn deploy_document(st: &AppState, title: &str, prefix: &str, active: &str, body: &str) -> Response {
    super::chrome::html(document(
        title,
        prefix,
        PAGE_CSS,
        &format!("#{active}"),
        "Skip to the section",
        Some(&super::chrome::Nav {
            active,
            badge: nav_badge(st),
            host: &format!("127.0.0.1:{}", st.port),
            account: known_account(st),
            token: token(st),
        }),
        body,
    ))
}

async fn page(st: &Arc<AppState>, headers: &HeaderMap, local: bool) -> Response {
    let prefix = forwarded_prefix(headers);
    let Some(project) = st.first_project() else {
        return deploy_document(
            st,
            "deploy",
            &prefix,
            "deploy",
            r#"<main class="dash"><section id="deploy" class="sec">
              <div class="panel">
              <span class="note">No scene is loaded, so there is nothing to publish.</span>
            </div></section></main>"#,
        );
    };
    let scene_title = crate::joinblock::scene_title(&project.scene_json);
    let default_target = deploy::env_default_target();
    let dest = resolve_dest(
        &project.scene_json,
        default_target.as_deref(),
        deploy::configured_catalyst_rotation(),
    );
    let jump = deploy::play_url(dest.world.as_deref(), &dest.base_pointer);

    let blocked = match st.allow_remote_deploy || local {
        true => None,
        false => Some(
            "Publishing runs on the machine hosting this preview, because signing opens its wallet. \
             Open this page there, or start the preview with --allow-remote-deploy.",
        ),
    };
    let preview = cached_preview(st, &project).await;
    let body = match &*preview {
        Ok(p) => {
            let print = fingerprint(&project.root, p);
            let (status, rights, warming) =
                status_and_rights(st, &project, &dest, &preview, &print).await;
            // The build is small, so entering this page IS the intent to
            // publish: a clean payload starts building immediately for a
            // local reader, and the wallet panel is ready by the time a
            // button would have been found. Only an empty slot starts one —
            // a terminal state (published, failed, stale) holds the floor
            // until the button asks again, so polling never rebuilds and a
            // finished publish never re-prompts on its own.
            let clean = matches!(p.main, MainBundle::Present(_))
                && p.oversize.is_empty()
                && p.unreadable.is_empty()
                && p.collisions.is_empty();
            if clean
                && (st.allow_remote_deploy || local)
                && !st.deploy_dry_run
                && std::env::var_os("DCL_PRIVATE_KEY").is_none()
            {
                // An empty slot claims afresh; a pending run whose payload
                // moved re-mints, so the wallet only ever signs the tree as
                // it is. Terminal states still hold the floor.
                // The emptiness check is a `let`, NOT a match scrutinee: a
                // scrutinee's MutexGuard lives to the end of the match, and
                // claim/drift_reclaim retake the same lock -- as a match
                // this self-deadlocked and starved the whole server.
                let vacant = runs(st).is_none();
                let id = match vacant {
                    true => claim(st, dest.headline.clone(), true, print.clone()),
                    false => drift_reclaim(st, dest.headline.clone(), &print),
                };
                if let Some(id) = id {
                    launch(st.clone(), project.root.clone(), id);
                }
            }
            let signing_now =
                matches!(runs(st).as_ref().map(|r| &r.state), Some(RunState::Running));
            format!(
                r#"<main class="dash"><section id="deploy" class="sec">{alarms}{verdict}{card}{drawer}{warm}</section></main><script>{script}</script>"#,
                warm = warming_marker(warming),
                alarms = alarms(p),
                verdict = verdict_warn(rights.as_deref()),
                card = card(
                    &prefix,
                    token(st),
                    &scene_title,
                    &dest,
                    p,
                    &status,
                    &print,
                    blocked,
                    rights.as_deref(),
                    signing_now,
                    &run_region(
                        st,
                        &prefix,
                        Some(jump.as_str()),
                        st.allow_remote_deploy || local,
                    ),
                ),
                drawer = payload_drawer(p),
                script = SCRIPT,
            )
        }
        Err(e) => format!(
            r#"<main class="dash"><section id="deploy" class="sec">
              <div class="panel"><h2>{title}</h2><span class="note">This scene cannot be
              packaged yet: {why}</span></div></section></main>"#,
            title = esc(&scene_title),
            why = esc(&scrub_paths(e, &project.root))
        ),
    };
    deploy_document(
        st,
        &format!("deploy {scene_title}"),
        &prefix,
        "deploy",
        &body,
    )
}

/// The error text on this page is served to anyone who can reach the port, and
/// an anyhow chain assembled further down may carry a path the page has no
/// business printing. Rather than trust every layer below to stay quiet about
/// where the scene lives, the path is taken back out here.
fn scrub_paths(msg: &str, root: &std::path::Path) -> String {
    let mut out = msg.to_string();
    let root_str = root.display().to_string();
    if root_str.len() > 1 {
        out = out.replace(&root_str, "the scene folder");
    }
    if let Some(parent) = root.parent() {
        let parent = parent.display().to_string();
        if parent.len() > 1 {
            out = out.replace(&parent, "\u{2026}");
        }
    }
    out
}

/// `human_size` returns one string; the caption/number/unit stack wants the
/// number apart from its unit, so the reading carries the weight and the unit
/// stays out of its way.
fn split_size(bytes: u64) -> (String, String) {
    let whole = deploy::human_size(bytes);
    match whole.rsplit_once(' ') {
        Some((n, unit)) => (n.to_string(), unit.to_string()),
        None => (whole, String::new()),
    }
}

/// A size the walk could not read is worth saying so, in the cell where its
/// size would be: `prepare` reads every file it uploads, so a file with no
/// readable size is a deploy that stops after the wallet has signed.
fn size_cell(len: Option<u64>) -> String {
    let text = match len {
        Some(n) => deploy::human_size(n),
        None => "Size unreadable".to_string(),
    };
    format!(r#"<span class="sz">{text}</span>"#)
}

fn warn(title: &str, body: String) -> String {
    format!(
        r#"<div class="panel panel--warn"><h2>{}</h2><span class="note">{body}</span></div>"#,
        esc(title)
    )
}

/// The refusals a real deploy would hit, moved in front of the wallet prompt.
fn alarms(p: &deploy::DeployPreview) -> String {
    let main = match &p.main {
        MainBundle::Present(_) => String::new(),
        MainBundle::Missing(m) => warn(
            "Not built yet",
            format!(
                "publishing needs the bundle <code>{}</code>, and it is not in the payload. \
                 Run <code>dcl-one-sdk build</code> first: deploy refuses this, and it refuses it \
                 after the wallet prompt. If the bundle does exist, .dclignore is excluding it.",
                esc(m)
            ),
        ),
        MainBundle::Unusable(why) => warn(
            "scene.json names no bundle",
            format!(
                "{}. deploy cannot package a scene until this is fixed.",
                esc(why)
            ),
        ),
    };
    let oversize = match p.oversize.is_empty() {
        true => String::new(),
        false => warn(
            "Over the per-file limit",
            format!(
                "a content server refuses a file over 50 MB, so this deploy would fail on: {}. \
                 Compress or split it, or exclude it in .dclignore.",
                esc(&p.oversize.join(", "))
            ),
        ),
    };
    let unreadable = match p.unreadable.is_empty() {
        true => String::new(),
        false => warn(
            "Cannot be read",
            format!(
                "these files are in the payload but their size could not be read: {}. deploy reads \
                 every file it uploads, so it would stop on them \u{2014} after your wallet had \
                 signed. A link pointing at something that is no longer there is the usual cause.",
                esc(&p.unreadable.join(", "))
            ),
        ),
    };
    let collisions = match p.collisions.is_empty() {
        true => String::new(),
        false => warn(
            "Two names a content server reads as one",
            format!(
                "{}. A content server matches file names case-insensitively, so deploy refuses \
                 this \u{2014} rename one of each pair.",
                esc(&p
                    .collisions
                    .iter()
                    .map(|(a, b)| format!("{a} collides with {b}"))
                    .collect::<Vec<_>>()
                    .join("; "))
            ),
        ),
    };
    format!("{main}{oversize}{unreadable}{collisions}")
}

fn named_scenes(scenes: &[RemoteScene]) -> String {
    let named: Vec<String> = scenes.iter().take(3).map(|s| s.title.clone()).collect();
    let tail = match scenes.len() > named.len() {
        true => format!(" and {} more", scenes.len() - named.len()),
        false => String::new(),
    };
    format!("{}{tail}", esc(&named.join(", ")))
}

/// The left column: what is on the target right now.
fn server_panel(dest: &Dest, status: &LiveStatus) -> String {
    let others_row = |others: &[RemoteScene]| match others.is_empty() {
        true => String::new(),
        false => {
            let fate = match dest.world.is_some() {
                true => "kept in place by this publish",
                false => "replaced by this publish",
            };
            kv(
                "Also here",
                format!(
                    "<span class=\"note\">{} \u{2014} {fate}</span>",
                    named_scenes(others)
                ),
            )
        }
    };
    match &status.remote {
        Remote::Known(state) => {
            let mut rows = String::new();
            match &state.current {
                Some(c) => {
                    rows.push_str(&kv("Scene", format!("<span>{}</span>", esc(&c.title))));
                    if let Some(ts) = c.timestamp {
                        rows.push_str(&kv(
                            "Deployed",
                            format!("<span>{}</span>", ago(ts, deploy::now_ms())),
                        ));
                    }
                    rows.push_str(&kv("Parcels", format!("<span>{}</span>", c.parcels)));
                }
                None => rows.push_str(r#"<span class="note">Nothing on these parcels yet.</span>"#),
            }
            rows.push_str(&others_row(&state.others));
            format!(r#"<div class="kvs">{rows}</div>"#)
        }
        Remote::Empty => {
            "<span class=\"note\">Nothing is deployed here yet \u{2014} this publish is the first.</span>"
                .to_string()
        }
        Remote::Unreachable(why) => format!(
            r#"<span class="note">Could not check what is live: {}. Publishing may still work.</span>"#,
            esc(why)
        ),
        Remote::Unknown(why) => format!(r#"<span class="note">{}.</span>"#, esc(why)),
    }
}

/// The right column: the payload totals and, when the server answered, how
/// much of it transfers at all.
fn upload_panel(p: &deploy::DeployPreview, status: &LiveStatus) -> String {
    let (size_num, size_unit) = split_size(p.total_bytes);
    let datum = format!(
        r#"<div class="datum"><div class="datum__v"><span class="datum__num">{files}</span><span
          class="datum__unit">file{s}</span><span class="datum__num">{size_num}</span><span
          class="datum__unit">{size_unit}</span></div></div>"#,
        files = p.files.len(),
        s = if p.files.len() == 1 { "" } else { "s" },
    );
    let reuse_line = match &status.reuse {
        Some(r) if r.reused_files > 0 => format!(
            "<span class=\"note\">{reused} of {total} files are already on the server \u{2014} \
             {up} to upload ({size}).</span>",
            reused = r.reused_files,
            total = r.reused_files + r.upload_files,
            up = r.upload_files,
            size = deploy::human_size(r.upload_bytes),
        ),
        Some(r) => format!(
            "<span class=\"note\">All {up} file{s} upload ({size}) \u{2014} the server has none \
             of them yet.</span>",
            up = r.upload_files,
            s = if r.upload_files == 1 { "" } else { "s" },
            size = deploy::human_size(r.upload_bytes),
        ),
        None => String::new(),
    };
    format!("{datum}{reuse_line}")
}

/// The card's footer: the primary button and the terminal line for people who
/// want it — the destination named once, in the card's own header. Or, for a
/// reader off the hosting machine, the refusal said out loud instead of a
/// button whose POST would be refused anyway.
fn foot(prefix: &str, tok: &str, print: &str, blocked: Option<&str>) -> String {
    if let Some(why) = blocked {
        return format!(
            r#"<div class="jn2__foot"><span class="note">{}</span></div>"#,
            esc(why)
        );
    }
    format!(
        r#"<form class="jn2__foot" id="publish" method="post" action="{prefix_esc}/deploy">
          <input type="hidden" name="token" value="{tok}">
          <input type="hidden" name="fingerprint" value="{print_esc}">
          <button class="jn__cta" type="submit">Publish</button>
          <span class="note">Signing happens in your wallet {mdash} nothing uploads until it
            answers. Or run <code>dcl-one-sdk deploy</code> in the scene folder.</span>
        </form>"#,
        prefix_esc = esc(prefix),
        tok = esc(tok),
        print_esc = esc(print),
        mdash = "\u{2014}",
    )
}

/// `0x` plus enough hex to recognize a wallet without a full line of it.
fn short_addr(addr: &str) -> String {
    match addr.len() > 12 {
        true => format!("{}\u{2026}{}", &addr[..6], &addr[addr.len() - 4..]),
        false => addr.to_string(),
    }
}

/// The one warn panel a refusing verdict earns on /deploy: the same message
/// the deploy itself would print after the wallet prompt, moved before it.
fn verdict_warn(rights: Option<&Rights>) -> String {
    let Some(Rights {
        address,
        verdict: Verdict::MayNot { why, remedy },
        ..
    }) = rights
    else {
        return String::new();
    };
    warn(
        "This wallet cannot publish here",
        format!(
            "{}: {}. {}. Signing with a different wallet still works.",
            esc(&short_addr(address)),
            esc(why),
            esc(remedy)
        ),
    )
}

/// The verdict as the card-hint fragment: one glyph, one short clause.
fn verdict_bit(rights: Option<&Rights>) -> String {
    match rights.map(|r| (&r.verdict, r.address.as_str())) {
        Some((Verdict::May(_), a)) => {
            format!(" · Publishing as {}", esc(&short_addr(a)))
        }
        Some((Verdict::MayNot { .. }, a)) => {
            format!(" · \u{2717} {} may not publish", esc(&short_addr(a)))
        }
        _ => String::new(),
    }
}

/// The slim publish card: one line saying what goes where, the button, and a
/// pointer at /target for the detail that used to crowd this page. The run
/// region — building spinner, wallet panel, outcome — rides INSIDE the card,
/// between head and foot: the publish was clicked here, so here is where it
/// unfolds, not in a second box above. While a run is live the foot simply
/// disappears — the region above IS the story, and a caption restating it
/// would be the page talking over itself.
#[allow(clippy::too_many_arguments)]
fn card(
    prefix: &str,
    tok: &str,
    scene_title: &str,
    dest: &Dest,
    p: &deploy::DeployPreview,
    status: &LiveStatus,
    print: &str,
    blocked: Option<&str>,
    rights: Option<&Rights>,
    running: bool,
    run: &str,
) -> String {
    let (size_num, size_unit) = split_size(p.total_bytes);
    let state_word = match &status.remote {
        Remote::Known(state) if state.current.is_some() => "updates the live scene",
        Remote::Known(_) | Remote::Empty => "first publish",
        Remote::Unreachable(_) | Remote::Unknown(_) => "live state unknown",
    };
    format!(
        r#"<div class="jn">
      <div class="jn2__head"><div class="jn2__title">
        <h2>{title} → {headline}</h2>
        <span class="jn__hint">{parcels} parcel{ps} · {files} file{fs} · {size_num} {size_unit} · {state_word}{verdict} — <a href="{prefix_esc}/target">review the target</a></span></div></div>
      {run}
      {foot}
    </div>"#,
        title = esc(scene_title),
        headline = esc(&dest.headline),
        parcels = dest.pointers.len().max(1),
        ps = if dest.pointers.len().max(1) == 1 {
            ""
        } else {
            "s"
        },
        files = p.files.len(),
        fs = if p.files.len() == 1 { "" } else { "s" },
        verdict = verdict_bit(rights),
        prefix_esc = esc(prefix),
        foot = match running {
            true => String::new(),
            false => foot(prefix, tok, print, blocked),
        },
    )
}

/// The pane fragment every empty state shares.
fn empty_col(head: &str, note: &str) -> String {
    format!(
        r#"<div class="jn2__col"><span class="knob__k">{head}</span><span class="note">{note}</span></div>"#
    )
}

/// The account's row inside the card, drawn only when there is something to
/// say beyond the bar's pill: the wait on a pending sign-in, why the last
/// one failed, or the connected account's cost line and its Forget. The
/// connect button itself lives in the page bar — the account is a
/// server-wide fact, not this card's field. (The auth page handles MetaMask
/// and Decentraland accounts alike; `POST /target/address` stays for the
/// person who scripts a bare address in.)
fn account_row(rights: Option<&Rights>, connect: Option<&Connect>) -> String {
    // A connected account needs no row at all: the bar pill names it and the
    // panes below ARE the answer — announcing that the check succeeded would
    // be the page congratulating itself.
    if rights.is_some() {
        return String::new();
    }
    match connect {
        Some(Connect {
            url,
            state: ConnectState::Waiting,
        }) => {
            format!(
                r#"<div class="jn2__noterow tgt__addr" id="connect-pending"><span class="knob__k">Account</span>
          <span class="note">Authorize the connection on the page that opened — this page follows along. <a class="knob__go" href="{url_esc}">Reopen the authorize page</a></span>
          <noscript><meta http-equiv="refresh" content="3"></noscript></div>"#,
                url_esc = esc(url),
            )
        }
        Some(Connect {
            state: ConnectState::Failed(why),
            ..
        }) => warn(
            "The sign-in did not finish",
            format!(
                "{} \u{2014} the bar's Connect button starts another.",
                esc(why)
            ),
        ),
        _ => String::new(),
    }
}

/// The target page's one script: the shared bar behaviour plus following a
/// pending sign-in by reloading — the server is still the only renderer.
const TARGET_SCRIPT: &str = concat!(
    include_str!("page_common.js"),
    r#"(() => {
  if (document.getElementById('connect-pending')) setTimeout(() => location.reload(), 3000);
})();"#
);

/// The one-line form that re-aims the scene at `world` (empty: at its
/// parcels on Genesis City).
fn point_form(prefix: &str, tok: &str, world: &str, label: &str) -> String {
    format!(
        r#"<form method="post" action="{prefix_esc}/target/point"><input type="hidden" name="token" value="{tok_esc}"><input type="hidden" name="world" value="{world_esc}"><button class="deep__copy" type="submit">{label}</button></form>"#,
        prefix_esc = esc(prefix),
        tok_esc = esc(tok),
        world_esc = esc(world),
        label = esc(label),
    )
}

/// The one-row form that moves the scene's footprint: its base parcel,
/// editable to anywhere on the Genesis map. Land only — a world positions
/// its scenes internally, so the map coordinate means nothing there.
fn base_form(prefix: &str, tok: &str, dest: &Dest) -> String {
    format!(
        r#"<div class="jn2__noterow"><form class="tgt__base" method="post" action="{prefix_esc}/target/base"><input type="hidden" name="token" value="{tok_esc}"><span class="jn__hint">Base parcel</span><input name="base" value="{base_esc}" aria-label="base parcel x,y" spellcheck="false" autocomplete="off"><button class="deep__copy" type="submit">Move scene</button><span class="jn__hint">anywhere on the Genesis map {mdash} the whole footprint moves with it</span></form></div>"#,
        prefix_esc = esc(prefix),
        tok_esc = esc(tok),
        base_esc = esc(&dest.base_pointer),
        mdash = '\u{2014}',
    )
}

/// The worlds the wallet can deploy to, each row saying what is there now —
/// and, for every world that is not already the target, the button that
/// points the scene at it. A scene mostly publishes to one place; the rows
/// make the exception one click instead of a scene.json edit.
fn your_worlds(prefix: &str, tok: &str, dest: &Dest, rights: Option<&Rights>) -> String {
    let Some(r) = rights else {
        return empty_col(
            "Your worlds",
            "Connect an account above to list the worlds it owns or holds a deploy grant on.",
        );
    };
    if let Verdict::Unchecked(why) = &r.verdict {
        if r.worlds.is_empty() {
            return empty_col("Your worlds", &esc(why));
        }
    }
    let note = r
        .worlds_note
        .as_deref()
        .map(|n| format!(r#"<span class="note">{}</span>"#, esc(n)))
        .unwrap_or_default();
    if r.worlds.is_empty() {
        return format!(
            "{}{note}",
            empty_col(
                "Your worlds",
                &format!(
                    "{} owns no name and holds no deploy grant \u{2014} claim a NAME to get a world.",
                    esc(&short_addr(&r.address))
                ),
            )
        );
    }
    const WORLDS_LISTED: usize = 10;
    // The current target leads the list — it is the row the eye came for,
    // and the cap below must never be the reason it is missing.
    let mut listed: Vec<_> = r.worlds.iter().collect();
    listed.sort_by_key(|w| dest.world.as_deref() != Some(w.name.as_str()));
    let rows: String = listed
        .iter()
        .take(WORLDS_LISTED)
        .map(|w| {
            let target = dest.world.as_deref() == Some(w.name.as_str());
            // The one /worlds answer already carries the world's title,
            // scene count and last deploy — every row says what is live
            // there without a single extra request, and every row says it
            // in the same order.
            let mut bits: Vec<String> = Vec::new();
            if let Some(t) = &w.title {
                bits.push(esc(t));
            }
            match w.scenes {
                Some(n) if n > 1 => {
                    bits.push("multiscene".to_string());
                    bits.push(format!("{n} scenes"));
                }
                Some(1) => bits.push("1 scene".to_string()),
                _ => bits.push("no scenes yet".to_string()),
            }
            if w.scenes.unwrap_or(0) > 0 {
                if let Some(ts) = w.last_deployed {
                    bits.push(format!("updated {}", ago(ts, deploy::now_ms())));
                }
            }
            if !w.owned {
                bits.push("granted".to_string());
            }
            let action = match target {
                true => r#"<span class="note">current target</span>"#.to_string(),
                false => point_form(prefix, tok, &w.name, "Point scene here"),
            };
            format!(
                r#"<div class="wl__r"><span class="wl__n">{dot}{name}</span><span class="wl__d">{data}</span>{action}</div>"#,
                dot = match target {
                    true => r#"<i class="lay__swatch lay__swatch--base"></i>"#,
                    false => "",
                },
                name = esc(&w.name),
                data = bits.join(" \u{b7} "),
            )
        })
        .collect();
    let tail = match r.worlds.len() > WORLDS_LISTED {
        true => format!(
            r#"<span class="note">and {} more</span>"#,
            r.worlds.len() - WORLDS_LISTED
        ),
        false => String::new(),
    };
    format!(
        r#"<div class="jn2__col"><span class="knob__k">Your worlds</span><div class="wl">{rows}</div>{tail}{note}</div>"#
    )
}

/// The land around the scene, as a map: the declared footprint in the
/// accent, every parcel the wallet owns or operates lit up around it — the
/// glanceable answer to "where could this scene go instead". The window
/// stays readable by centring on the footprint; holdings beyond it are a
/// count, never silently gone.
fn land_map(declared: &[(i64, i64)], base: (i64, i64), owned: &[(i64, i64)]) -> String {
    if declared.is_empty() {
        return String::new();
    }
    let span = |vs: &mut dyn Iterator<Item = i64>| {
        vs.fold((i64::MAX, i64::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)))
    };
    let (dx0, dx1) = span(&mut declared.iter().map(|p| p.0));
    let (dy0, dy1) = span(&mut declared.iter().map(|p| p.1));
    let pad_x = (AFTER_MAP_SPAN - (dx1 - dx0 + 1)).max(0) / 2;
    let pad_y = (AFTER_MAP_SPAN - (dy1 - dy0 + 1)).max(0) / 2;
    let (x0, x1) = (dx0 - pad_x, dx1 + pad_x);
    let (y0, y1) = (dy0 - pad_y, dy1 + pad_y);
    let mine: HashSet<(i64, i64)> = declared.iter().copied().collect();
    let yours: HashSet<(i64, i64)> = owned.iter().copied().collect();
    let off_map = owned
        .iter()
        .filter(|(x, y)| *x < x0 || *x > x1 || *y < y0 || *y > y1)
        .count();
    let cols = x1 - x0 + 1;
    let mut cells = String::new();
    for y in (y0..=y1).rev() {
        for x in x0..=x1 {
            let p = (x, y);
            let (class, title) = if p == base {
                (" lay__cell--base", format!("Base parcel {x},{y}"))
            } else if mine.contains(&p) {
                (" lay__cell--in", format!("This scene {x},{y}"))
            } else if yours.contains(&p) {
                (" dep__cell--own", format!("Yours {x},{y}"))
            } else {
                ("", format!("{x},{y}"))
            };
            cells.push_str(&format!(
                r#"<div class="lay__cell{class}" title="{title}"></div>"#
            ));
        }
    }
    let off = match off_map {
        0 => String::new(),
        n => format!(
            r#"<span class="note">and {n} of your parcel{s} beyond this window</span>"#,
            s = if n == 1 { "" } else { "s" },
        ),
    };
    format!(
        r#"<div class="lay__legend"><span class="lay__key"><i class="lay__swatch lay__swatch--base"></i>Base</span><span class="lay__key"><i class="lay__swatch lay__swatch--in"></i>This scene</span><span class="lay__key"><i class="lay__swatch dep__cell--own"></i>Yours</span></div><div class="lay__map" style="--lay-cols:{cols}"><div class="lay__grid" role="img" aria-label="your land around this scene">{cells}</div></div>{off}"#
    )
}

/// One row per declared parcel with the strongest right the wallet holds on
/// it, the map of the wallet's land around the footprint, and the holdings
/// line under them.
fn land_rights_col(dest: &Dest, rights: Option<&Rights>) -> String {
    let Some(r) = rights else {
        return empty_col(
            "Your rights here",
            "Connect an account above to check every declared parcel against its on-chain rights.",
        );
    };
    let mut rows = String::new();
    for pr in &r.parcel_rights {
        let cell = match pr.leg {
            Some(leg) => format!("<span>\u{2713} {leg}</span>"),
            None => "<span>\u{2717} no rights \u{2014} deploy will refuse</span>".to_string(),
        };
        rows.push_str(&kv(&pr.pointer, cell));
    }
    let body = match (rows.is_empty(), &r.verdict) {
        (true, Verdict::Unchecked(why)) => {
            format!(r#"<span class="note">{}</span>"#, esc(why))
        }
        (true, _) => r#"<span class="note">No declared parcel to check.</span>"#.to_string(),
        (false, _) => {
            let unchecked = match r.unchecked_parcels {
                0 => String::new(),
                n => format!(
                    r#"<span class="note">and {n} more parcel{} unchecked</span>"#,
                    if n == 1 { "" } else { "s" }
                ),
            };
            format!(r#"<div class="kvs">{rows}</div>{unchecked}"#)
        }
    };
    let (map, holdings) = match r.holdings.as_ref() {
        Some(h) => {
            let declared = deploy_status::parse_coords(&dest.pointers);
            let base = catalyrst_types::pointer::parse_pointer(&dest.base_pointer)
                .unwrap_or_else(|| declared.first().copied().unwrap_or((0, 0)));
            (
                land_map(&declared, base, &h.coords),
                format!(
                    r#"<span class="note">{} holds {} parcel{}, {} estate{} and operates {} more.</span>"#,
                    esc(&short_addr(&r.address)),
                    h.parcels,
                    if h.parcels == 1 { "" } else { "s" },
                    h.estates,
                    if h.estates == 1 { "" } else { "s" },
                    h.operated,
                ),
            )
        }
        None => (String::new(), String::new()),
    };
    format!(
        r#"<div class="jn2__col"><span class="knob__k">Your rights here</span>{body}{map}{holdings}</div>"#
    )
}

/// The target card: where a publish sends this scene, one sub-tab per shape
/// the destination can take. The tabs are CSS-only radios — the same idiom as
/// the join card — so the page needs no script at all.
#[allow(clippy::too_many_arguments)]
fn target_card(
    prefix: &str,
    tok: &str,
    dest: &Dest,
    p: &deploy::DeployPreview,
    status: &LiveStatus,
    rights: Option<&Rights>,
    connect: Option<&Connect>,
    history: &[PastRun],
) -> String {
    let world = dest.world.as_deref();
    let tab = |value: &str, label: &str, checked: bool| {
        super::chrome::radio_tab("tgt", value, label, checked)
    };
    let verdict_row = match rights.map(|r| (&r.verdict, r.address.as_str())) {
        Some((Verdict::May(reason), a)) => format!(
            "<div class=\"jn2__noterow\"><span class=\"jn__hint\">Publishing as {} \u{2014} {}</span></div>",
            esc(&short_addr(a)),
            esc(reason)
        ),
        Some((Verdict::MayNot { why, remedy }, a)) => format!(
            "<div class=\"jn2__noterow\"><span class=\"jn__hint\">\u{2717} {}: {} \u{2014} {}</span></div>",
            esc(&short_addr(a)),
            esc(why),
            esc(remedy)
        ),
        Some((Verdict::Unchecked(why), _)) => format!(
            r#"<div class="jn2__noterow"><span class="jn__hint">{}</span></div>"#,
            esc(why)
        ),
        None => String::new(),
    };
    let detail = format!(
        r#"<div class="jn2__noterow"><span class="jn__hint">{line}</span></div>{verdict_row}<div class="jn2__body">
      <div class="jn2__col"><span class="knob__k">On the server now</span>{server}</div>
      <div class="jn2__col"><span class="knob__k">This upload</span>{upload}</div>
    </div>"#,
        line = esc(&format!("{} {}", dest.headline, dest.server_line)),
        server = server_panel(dest, status),
        upload = upload_panel(p, status),
    );
    let world_pane = format!(
        "{}{}",
        match world {
            Some(_) => detail.clone(),
            None => empty_col(
                "No world target",
                "This scene deploys to Genesis City LAND \u{2014} point it at one of your worlds below to publish there instead.",
            ),
        },
        your_worlds(prefix, tok, dest, rights)
    );
    let land_pane = match world {
        None => format!(
            "{detail}{}{}",
            base_form(prefix, tok, dest),
            land_rights_col(dest, rights)
        ),
        Some(w) => format!(
            r#"<div class="jn2__col"><span class="knob__k">No LAND target</span><span class="note">This scene targets the world <code>{}</code>; pointed back at its parcels, it deploys to Genesis City instead.</span>{}</div>"#,
            esc(w),
            point_form(prefix, tok, "", "Point at Genesis City LAND"),
        ),
    };
    let multi_pane = multiscene_pane(dest, status);
    let history_pane = history_rows_pane(history);
    let in_world = match &status.remote {
        Remote::Known(state) => state.others.len() + usize::from(state.current.is_some()),
        _ => 0,
    };
    let multi_label = match in_world {
        0 => "Multiscene world".to_string(),
        n => format!(r#"Multiscene world <span class="jn2__tab-n">{n}</span>"#),
    };
    let tabs = format!(
        "{}{}{}{}",
        tab("world", "World", world.is_some()),
        tab("land", "Land", world.is_none()),
        tab("multi", &multi_label, false),
        tab("history", "History", false),
    );
    format!(
        r#"<div class="jn tgt">
      {addr}
      <fieldset class="knob knob--tabs"><legend class="knob__k u-sr-only">target type</legend><div class="jn2__tabs">{tabs}</div></fieldset>
      <div class="tgt__pane tgt__pane--world">{world_pane}</div>
      <div class="tgt__pane tgt__pane--land">{land_pane}</div>
      <div class="tgt__pane tgt__pane--multi">{multi_pane}</div>
      <div class="tgt__pane tgt__pane--history">{history_pane}</div>
    </div>"#,
        addr = account_row(rights, connect),
    )
}

/// The History tab: what this preview (and, through the on-disk record,
/// earlier previews of this scene) actually published.
fn history_rows_pane(history: &[PastRun]) -> String {
    if history.is_empty() {
        return empty_col(
            "No deployments yet",
            "Nothing has been published from this preview — publishes from here will list with age and signer.",
        );
    }
    let rows: String = history
        .iter()
        .map(|h| {
            let by = h
                .signer
                .as_deref()
                .map(|s| format!(" by {}", short_addr(s)))
                .unwrap_or_default();
            kv(
                &ago(h.at_ms, deploy::now_ms()),
                format!(
                    "<span>{} \u{2014} {}{}</span>",
                    esc(&h.target),
                    esc(&h.outcome),
                    esc(&by)
                ),
            )
        })
        .collect();
    format!(
        r#"<div class="jn2__col"><span class="knob__k">Published from here</span><div class="kvs">{rows}</div></div>"#
    )
}

/// How many cells a side the after-map will draw. Past it, the scene rows
/// still tell the story and the grid would be a wall of unreadable pixels.
const AFTER_MAP_SPAN: i64 = 24;

/// The world as this publish leaves it, as a parcel grid: the scenes that
/// stay in the neutral swatch, the replaced scene's old footprint dashed,
/// this scene's parcels in the accent — the same cell idiom as the layout
/// card, so the two maps read as one language.
fn after_map(remote: &RemoteState, ours: &[(i64, i64)], base: (i64, i64)) -> String {
    let kept: HashSet<(i64, i64)> = remote
        .others
        .iter()
        .flat_map(|s| s.coords.iter().copied())
        .collect();
    let was: HashSet<(i64, i64)> = remote
        .current
        .as_ref()
        .map(|c| c.coords.iter().copied().collect())
        .unwrap_or_default();
    let mine: HashSet<(i64, i64)> = ours.iter().copied().collect();
    let all: Vec<(i64, i64)> = kept
        .iter()
        .chain(was.iter())
        .chain(mine.iter())
        .copied()
        .collect();
    if all.is_empty() {
        return String::new();
    }
    let span = |vs: &mut dyn Iterator<Item = i64>| {
        vs.fold((i64::MAX, i64::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)))
    };
    let (x0, x1) = span(&mut all.iter().map(|p| p.0));
    let (y0, y1) = span(&mut all.iter().map(|p| p.1));
    let (cols, rows) = (x1 - x0 + 1, y1 - y0 + 1);
    if cols > AFTER_MAP_SPAN || rows > AFTER_MAP_SPAN {
        return String::new();
    }
    let mut cells = String::new();
    for y in (y0..=y1).rev() {
        for x in x0..=x1 {
            let p = (x, y);
            let (class, title) = if p == base && mine.contains(&p) {
                (" lay__cell--base", format!("Base parcel {x},{y}"))
            } else if mine.contains(&p) {
                (" lay__cell--in", format!("This scene {x},{y}"))
            } else if kept.contains(&p) {
                (" dep__cell--kept", format!("Kept {x},{y}"))
            } else if was.contains(&p) {
                (" dep__cell--was", format!("Replaced {x},{y}"))
            } else {
                ("", format!("{x},{y}"))
            };
            cells.push_str(&format!(
                r#"<div class="lay__cell{class}" title="{title}"></div>"#
            ));
        }
    }
    format!(
        r#"<div class="lay__legend"><span class="lay__key"><i class="lay__swatch lay__swatch--base"></i>Base</span><span class="lay__key"><i class="lay__swatch lay__swatch--in"></i>This scene</span><span class="lay__key"><i class="lay__swatch dep__cell--kept"></i>Kept</span><span class="lay__key"><i class="lay__swatch dep__cell--was"></i>Replaced</span></div><div class="lay__map" style="--lay-cols:{cols}"><div class="lay__grid" role="img" aria-label="world parcels after this publish">{cells}</div></div>"#
    )
}

/// The multiscene pane: what already lives in this world beside the upload,
/// the after-map, and what the world's scenes hold in bytes — said only from
/// sizes the server actually reported.
fn multiscene_pane(dest: &Dest, status: &LiveStatus) -> String {
    let Some(w) = dest.world.as_deref() else {
        return r#"<div class="jn2__col"><span class="knob__k">Worlds only</span><span class="note">Multi-scene publishes stack scenes inside one world. This scene targets Genesis City, where every parcel set is its own deployment.</span></div>"#.to_string();
    };
    let Remote::Known(state) = &status.remote else {
        return format!(
            r#"<div class="jn2__col"><span class="knob__k">Nothing else here</span><span class="note">No other scenes are live in <code>{}</code> — a multi-scene publish would add this one beside them when there are.</span></div>"#,
            esc(w)
        );
    };
    if state.others.is_empty() {
        return format!(
            r#"<div class="jn2__col"><span class="knob__k">Nothing else here</span><span class="note">No other scenes are live in <code>{}</code> — a multi-scene publish would add this one beside them when there are.</span></div>"#,
            esc(w)
        );
    }
    let rows: String = state
        .others
        .iter()
        .map(|scene| {
            let size = scene
                .size
                .map(|b| format!(" \u{b7} {}", deploy::human_size(b)))
                .unwrap_or_default();
            format!(
                "<div class=\"lay__srow\"><i class=\"lay__swatch dep__cell--kept\"></i><span class=\"lay__srow-t\"><span class=\"lay__srow-n\">{}</span><code class=\"lay__srow-c\">{} parcel{}{size} \u{b7} kept</code></span></div>",
                esc(&scene.title),
                scene.parcels,
                if scene.parcels == 1 { "" } else { "s" },
            )
        })
        .collect();
    let replaced_row = state
        .current
        .as_ref()
        .map(|c| {
            let size = c
                .size
                .map(|b| format!(" \u{b7} {}", deploy::human_size(b)))
                .unwrap_or_default();
            format!(
                "<div class=\"lay__srow\"><i class=\"lay__swatch dep__cell--was\"></i><span class=\"lay__srow-t\"><span class=\"lay__srow-n\">{}</span><code class=\"lay__srow-c\">{} parcel{}{size} \u{b7} replaced by this publish</code></span></div>",
                esc(&c.title),
                c.parcels,
                if c.parcels == 1 { "" } else { "s" },
            )
        })
        .unwrap_or_default();
    let held: u64 = state
        .others
        .iter()
        .filter_map(|s| s.size)
        .chain(state.current.as_ref().and_then(|c| c.size))
        .sum();
    let storage = match held {
        0 => String::new(),
        b => {
            let adds = status
                .reuse
                .as_ref()
                .map(|r| {
                    format!(
                        " \u{2014} this publish uploads {} of new content",
                        deploy::human_size(r.upload_bytes)
                    )
                })
                .unwrap_or_default();
            format!(
                r#"<span class="note">Scenes in this world hold {}{adds}.</span>"#,
                deploy::human_size(b)
            )
        }
    };
    let ours = deploy_status::parse_coords(&dest.pointers);
    let base = catalyrst_types::pointer::parse_pointer(&dest.base_pointer)
        .unwrap_or_else(|| ours.first().copied().unwrap_or((0, 0)));
    format!(
        r#"<div class="jn2__noterow"><span class="jn__hint">A multi-scene publish adds this scene to {w} — only overlapping parcels change hands, everything else stays live</span></div>
        <div class="jn2__body"><div class="jn2__col"><span class="knob__k">After this publish</span>{map}</div>
        <div class="jn2__col"><span class="knob__k">In this world now</span><div class="lay__srows">{rows}{replaced_row}</div>{storage}</div></div>"#,
        w = esc(w),
        map = after_map(state, &ours, base),
    )
}

/// `/target` — the destination detail that used to crowd the publish card.
async fn target_page(st: &Arc<AppState>, headers: &HeaderMap) -> Response {
    let prefix = forwarded_prefix(headers);
    let Some(project) = st.first_project() else {
        return deploy_document(
            st,
            "target",
            &prefix,
            "target",
            r#"<main class="dash"><section id="target" class="sec"><div class="panel">
              <span class="note">No scene is loaded, so there is nowhere to publish to.</span>
            </div></section></main>"#,
        );
    };
    let scene_title = crate::joinblock::scene_title(&project.scene_json);
    let dest = resolve_dest(
        &project.scene_json,
        deploy::env_default_target().as_deref(),
        deploy::configured_catalyst_rotation(),
    );
    let preview = cached_preview(st, &project).await;
    let body = match &*preview {
        Ok(p) => {
            let print = fingerprint(&project.root, p);
            let (status, rights, warming) =
                status_and_rights(st, &project, &dest, &preview, &print).await;
            let history = history_rows(st, &project.root);
            let connect = connect_slot(st).clone();
            format!(
                r#"<main class="dash"><section id="target" class="sec">{card}{warm}</section></main><script>{script}</script>"#,
                warm = warming_marker(warming),
                card = target_card(
                    &prefix,
                    token(st),
                    &dest,
                    p,
                    &status,
                    rights.as_deref(),
                    connect.as_ref(),
                    &history
                ),
                script = TARGET_SCRIPT,
            )
        }
        Err(e) => format!(
            r#"<main class="dash"><section id="target" class="sec">
              <div class="panel"><h2>{title}</h2><span class="note">This scene cannot be
              packaged yet: {why}</span></div></section></main>"#,
            title = esc(&scene_title),
            why = esc(&scrub_paths(e, &project.root))
        ),
    };
    deploy_document(
        st,
        &format!("target {scene_title}"),
        &prefix,
        "target",
        &body,
    )
}

/// The full payload, folded: the reuse split already answered "how much", so
/// the file-by-file listing is detail on demand.
/// Rows past [`LISTED`] are one click away, not gone — but a click is still
/// a page, so past this many even the fold ends in a sum instead of ten
/// thousand rows.
const LISTED_EXPANDED: usize = 400;

fn payload_drawer(p: &deploy::DeployPreview) -> String {
    let row = |(rel, len): &(String, Option<u64>)| {
        format!(
            r#"<div class="kv"><span class="k k--file">{}</span>{}</div>"#,
            esc(rel),
            size_cell(*len)
        )
    };
    let listed: String = p.files.iter().take(LISTED).map(row).collect();
    let rest = p.files.len().saturating_sub(LISTED);
    let rest_fold = match rest {
        0 => String::new(),
        n => {
            let rest_size = deploy::human_size(
                p.files
                    .iter()
                    .skip(LISTED)
                    .filter_map(|(_, len)| *len)
                    .sum(),
            );
            let rows: String = p
                .files
                .iter()
                .skip(LISTED)
                .take(LISTED_EXPANDED)
                .map(row)
                .collect();
            let beyond = match n.saturating_sub(LISTED_EXPANDED) {
                0 => String::new(),
                m => format!(
                    r#"<span class="note">and {m} more beyond this listing (the sizes above count them)</span>"#
                ),
            };
            format!(
                r#"<details class="files__more"><summary>and {n} more {mdot} {rest_size}</summary><div class="kvs files">{rows}</div>{beyond}</details>"#,
                mdot = "\u{b7}",
            )
        }
    };
    let (size_num, size_unit) = split_size(p.total_bytes);
    format!(
        r#"<details class="drawer"><summary>Payload {mdot} {files} file{s} {mdot} {size_num} {size_unit}</summary>
      <div class="drawer__body"><div class="kvs files">{listed}</div>{rest_fold}</div></details>"#,
        files = p.files.len(),
        s = if p.files.len() == 1 { "" } else { "s" },
        mdot = "\u{b7}",
    )
}

#[cfg(test)]
#[path = "deploy_page_tests.rs"]
mod tests;
