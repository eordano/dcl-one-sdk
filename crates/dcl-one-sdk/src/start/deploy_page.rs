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
use super::deploy_rights::{self, Holdings, Rights, Verdict};
use super::deploy_status::{
    self, ago, cache_get, cache_put, cached_status, lock, plural, resolve_dest, Dest, LiveStatus,
    Remote, RemoteScene, RemoteState,
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
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

pub(super) async fn route(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    page(&st, &headers, peer.ip().is_loopback()).await
}

pub(super) async fn target_route(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    target_page(&st, &headers, peer.ip().is_loopback()).await
}

/// The deploy entry's badge in the section nav, so a publish's liveness
/// shows from every page.
pub(super) fn nav_badge(st: &AppState) -> &'static str {
    match runs(st).as_ref().map(|r| &r.state) {
        Some(RunState::Running) => "signing\u{2026}",
        Some(RunState::Done(_)) => "live",
        _ => "",
    }
}

/// Layout the shared sheet has no rule for. Tokens only — no colour, size or
/// case of its own — so the page cannot drift into looking like a different
/// server than `/`.
const PAGE_CSS: &str = "
#deploy, #target { gap: var(--s-5); }
.jn2__col > .knob__k + * { margin-top: calc(-1 * var(--s-2)); }
.datum__unit + .datum__num { margin-left: var(--s-3); }
.jn2__foot .note { flex-basis: 100%; }
.jn2__foot .knob__go { align-self: auto; }
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
.dep__err { white-space: pre; max-height: 24rem; overflow: auto; }
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

/// Files listed individually before the rest is summed into one line.
const LISTED: usize = 8;

/// How long a walk answers for. The route is unauthenticated and the walk is
/// the whole scene tree: without this, a burst of refreshes parks that many
/// tokio workers in `read_dir`. Same span as `start`'s `ENTITY_CACHE_TTL`.
const PREVIEW_CACHE_TTL: Duration = Duration::from_millis(500);

/// `anyhow::Error` is not `Clone`, and the text is all the page wants.
type PreviewResult = Result<deploy::DeployPreview, String>;

/// Everything a deploy run keeps between requests, on `AppState` rather than
/// in statics so state cannot bleed across tests.
#[derive(Default)]
pub(super) struct DeployState {
    caches: deploy_status::StatusCaches,
    run: Mutex<Option<Run>>,
    token: OnceLock<String>,
    signer: Mutex<Option<Arc<crate::linker::LinkerState>>>,
    preview: Mutex<Vec<(PathBuf, Instant, Arc<PreviewResult>)>>,
    /// The wallet whose rights the pages check: the last signer, a connected
    /// account, or the /target form's. Read-only — nothing signs with it.
    address: Mutex<Option<String>>,
    rights: deploy_rights::RightsCache,
    /// Publishes this preview has seen, newest first.
    history: Mutex<VecDeque<PastRun>>,
    connect: Mutex<Option<Connect>>,
    /// Cache keys a background warm-up is already fetching, so a warming
    /// page's reloads do not each start another fetch.
    warming: Mutex<HashSet<String>>,
    /// The delegated deploy key the Connect-with-DCL flow minted: while
    /// present and unexpired, a publish signs itself with no wallet prompt.
    identity: Mutex<Option<deploy::DeployIdentity>>,
}

/// One accessor per [`DeployState`] slot, each holding its lock.
macro_rules! slot {
    ($($name:ident: $field:ident => $ty:ty;)*) => {$(
        fn $name(st: &AppState) -> MutexGuard<'_, $ty> {
            lock(&st.deploy.$field)
        }
    )*};
}
slot! {
    identity_slot: identity => Option<deploy::DeployIdentity>;
    connect_slot: connect => Option<Connect>;
    warm_slot: warming => HashSet<String>;
    address_slot: address => Option<String>;
    history_slot: history => VecDeque<PastRun>;
    cache: preview => Vec<(PathBuf, Instant, Arc<PreviewResult>)>;
    signer_slot: signer => Option<Arc<crate::linker::LinkerState>>;
    runs: run => Option<Run>;
}

/// The live delegated identity; an expired one is dropped here so the page
/// falls back to the wallet.
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

/// A "connect a Decentraland account" hand-off: the authorize page and what
/// became of it. The proven address lands in the address slot.
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

#[derive(Clone)]
pub(super) struct PastRun {
    at_ms: i64,
    target: String,
    signer: Option<String>,
    outcome: String,
    /// The upload line, the failure's why lines, or the moved files; absent
    /// on rows from before it was kept.
    detail: Option<String>,
}

/// The why lines without the "try" steps, capped: diagnostics can run pages.
const DETAIL_CAP: usize = 800;

fn failure_detail(why: &str) -> String {
    let why = &super::ansi::strip(why);
    let mut kept: Vec<&str> = why
        .lines()
        .filter(|l| !l.trim_start().starts_with("\u{2192} try:"))
        .collect();
    while kept.last().is_some_and(|l| l.trim().is_empty()) {
        kept.pop();
    }
    let joined = kept.join("\n");
    match joined.char_indices().nth(DETAIL_CAP) {
        Some((cut, _)) => format!("{}\u{2026}", joined[..cut].trim_end()),
        None => joined,
    }
}

/// The in-memory ring is this deep; the on-disk record keeps everything.
const HISTORY_KEPT: usize = 20;

fn history_path(root: &Path) -> PathBuf {
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
            "detail": past.detail,
        });
        let path = history_path(&project.root);
        let _ = crate::scene::work_dir(&project.root);
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

/// This process's ring, or — for a preview that just started — the scene's
/// on-disk record from earlier runs.
fn history_rows(st: &AppState, root: &Path) -> Vec<PastRun> {
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
                detail: v.get("detail").and_then(|d| d.as_str()).map(str::to_string),
            })
        })
        .collect();
    out.reverse();
    out.truncate(HISTORY_KEPT);
    out
}

/// Walks the scene off the async worker, at most once per [`PREVIEW_CACHE_TTL`]
/// per scene.
async fn cached_preview(st: &AppState, project: &Project) -> Arc<PreviewResult> {
    let root = project.root.clone();
    let ttl = match runs(st).as_ref().map(|r| &r.state) {
        Some(RunState::Running) => Duration::MAX,
        _ => PREVIEW_CACHE_TTL,
    };
    if let Some(hit) = cache_get(&cache(st), &root, ttl) {
        return hit;
    }
    let owned = project.clone();
    let computed =
        tokio::task::spawn_blocking(move || deploy::preview(&owned).map_err(|e| format!("{e:#}")))
            .await
            .unwrap_or_else(|e| Err(format!("the scene walk did not finish ({e})")));
    let entry = Arc::new(computed);
    cache_put(&mut cache(st), root, entry.clone(), PREVIEW_CACHE_TTL);
    entry
}

/// A CLI publish hosting itself on this server: the signer is live from the
/// first request, and the run slot says so.
pub(super) fn adopt_cli_signing(st: &AppState, signer: Arc<crate::linker::LinkerState>) {
    let target = signer.target_content().to_string();
    *signer_slot(st) = Some(signer);
    *runs(st) = Some(Run {
        signing: Some("/deploy".to_string()),
        ..Run::new(0, target, false, String::new())
    });
}

/// The signing panel when a publish is waiting on a wallet, minted fresh per
/// render: the id drawn is the id the wallet signs.
pub(super) fn pending_sign_panel(st: &AppState, prefix: &str) -> Option<String> {
    let state = signer_slot(st).clone()?;
    Some(crate::linker::sign_section(
        &state,
        &format!("{prefix}/deploy/sign"),
    ))
}

pub(super) fn known_account(st: &AppState) -> Option<String> {
    address_slot(st).clone()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Minted once per process and rendered only into the page body: an attacker
/// page can neither read it (same-origin) nor guess it, so with the loopback
/// check it keeps `POST /deploy` from being anyone's publish button.
pub(super) fn token(st: &AppState) -> &str {
    st.deploy
        .token
        .get_or_init(|| hex(&rand::random::<[u8; 16]>()))
}

struct Run {
    /// Which claim this is; the completion path presents it before writing a
    /// terminal state, so a slow deploy cannot stamp a run that replaced it.
    id: u64,
    started: Instant,
    target: String,
    /// The wallet-signing page's URL, set once the linker binds it.
    signing: Option<String>,
    /// Started by opening /deploy rather than pressing the button: a run
    /// nobody signed ends quietly instead of wearing a failure panel.
    auto: bool,
    /// The payload fingerprint this run was claimed against. Empty for an
    /// adopted CLI signing, which the page never re-mints.
    print: String,
    /// How many drift re-mints led to this run; [`MAX_REMINTS`] ends the chase.
    remints: u32,
    state: RunState,
}

impl Run {
    fn new(id: u64, target: String, auto: bool, print: String) -> Self {
        Run {
            id,
            started: Instant::now(),
            target,
            signing: None,
            auto,
            print,
            remints: 0,
            state: RunState::Running,
        }
    }
}

/// Re-mints one pending run may go through. A payload that moves on every
/// poll is not an author editing — it is a build writing into its own
/// fingerprint, and one more build will not settle it — so past this the run
/// keeps the entity it has, and the POST's own fingerprint check still
/// refuses a stale page.
const MAX_REMINTS: u32 = 5;

/// One counter for every way a run comes to exist, so a re-mint can never
/// collide with a claim.
static NEXT_RUN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn next_run_id() -> u64 {
    NEXT_RUN.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

enum RunState {
    Running,
    Done(String),
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

/// Size and mtime (unix ms) of one file, or nothing when it is not there.
fn stamp(path: &Path) -> Option<(u64, u128)> {
    let m = std::fs::metadata(path).ok()?;
    let mtime = m
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis());
    Some((m.len(), mtime))
}

/// The two files a payload path may live in: the tree's own copy and its
/// release twin under `RELEASE_OUT`. Both are stamped, so an edit the
/// watcher lands in the tree moves the print (a pending run re-mints on
/// that) and so does a fresh release build (the forecast's hashes are keyed
/// on it, and they read the twin). Both come from disk, never from a held
/// preview, so two prints of one tree agree whichever walk drew them.
fn copies(root: &Path, rel: &str) -> [Option<(u64, u128)>; 2] {
    [
        stamp(&root.join(rel)),
        stamp(&root.join(crate::build::RELEASE_OUT).join(rel)),
    ]
}

/// What the page drew, in one line: every publishable path with the size and
/// mtime of each copy it has, plus the bundle's state. Size alone misses the
/// edit that changes a character and not the length.
fn fingerprint(root: &Path, p: &deploy::DeployPreview) -> String {
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    digest.update(format!("{:?}\n", p.main).as_bytes());
    for (rel, _) in &p.files {
        digest.update(format!("{rel}\u{1f}{:?}\n", copies(root, rel)).as_bytes());
    }
    hex(&digest.finalize()[..12])
}

/// Names what changed, for the refusal message. It does NOT decide whether
/// anything changed — the digest does: this only sees files that still exist
/// and were touched recently.
fn moved_since(root: &Path, p: &deploy::DeployPreview) -> Vec<String> {
    let mut out: Vec<String> = p
        .files
        .iter()
        .filter(|(rel, _)| {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis());
            copies(root, rel)
                .iter()
                .flatten()
                .any(|(_, mtime)| now.saturating_sub(*mtime) < 300_000)
        })
        .map(|(rel, _)| rel.clone())
        .collect();
    out.truncate(6);
    out
}

fn reply(status: StatusCode, why: &str) -> Response {
    (status, format!("{why}\n")).into_response()
}

fn forbidden(why: &str) -> Response {
    reply(StatusCode::FORBIDDEN, why)
}

/// The gates every mutating POST shares, each one a found bypass: a loopback
/// peer that is not a tunnel replay (the agent stamps
/// [`crate::tunnel::FORWARDED_HEADER`]); same origin, because with CORS the
/// token was fetchable out of the page HTML; and the token itself.
fn post_gate(
    st: &AppState,
    allow_remote: bool,
    peer: SocketAddr,
    headers: &HeaderMap,
    tok: &str,
    remote_why: &str,
    token_why: &str,
) -> Option<Response> {
    if remote_peer(allow_remote, peer, headers) {
        return Some(forbidden(remote_why));
    }
    if let Some(why) = cross_origin_refusal(headers) {
        return Some(forbidden(why));
    }
    (tok != token(st)).then(|| forbidden(token_why))
}

fn scene_dest(project: &Project) -> Dest {
    resolve_dest(
        &project.scene_json,
        deploy::configured_target_server().as_deref(),
        deploy::configured_catalyst_rotation(),
    )
}

/// `POST /deploy`: the shared gates, then one run claimed in the lock that
/// checks (two builds over one `bin/` can sign mid-write bytes), and the
/// payload fingerprint the page drew. Refuses `DCL_PRIVATE_KEY` signing (one
/// click must not be an unattended publish). Replies with a redirect so
/// refresh cannot resubmit.
pub(super) async fn start(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(form): Form<DeployForm>,
) -> Response {
    let back = Redirect::to(&format!("{}/deploy", forwarded_prefix(&headers))).into_response();
    if let Some(refused) = post_gate(
        &st,
        st.allow_remote_deploy,
        peer,
        &headers,
        &form.token,
        "publishing runs on the machine hosting this preview; start it with --allow-remote-deploy to publish from elsewhere",
        "stale or missing deploy token \u{2014} reload /deploy and press the button there",
    ) {
        return refused;
    }
    if !st.deploy_dry_run && std::env::var_os("DCL_PRIVATE_KEY").is_some() {
        return forbidden(
            "DCL_PRIVATE_KEY is set, so this deploy would upload with no wallet prompt; run dcl-one-sdk deploy in a terminal instead",
        );
    }
    let Some(project) = st.first_project() else {
        return back;
    };
    let dest = scene_dest(&project);
    let Some(id) = claim(&st, dest.headline.clone(), false, form.fingerprint.clone()) else {
        return back;
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
            return back;
        }
    };
    if form.fingerprint.is_empty() || fingerprint(&root, &fresh) != form.fingerprint {
        finish(&st, id, RunState::Stale(moved_since(&root, &fresh)));
        return back;
    }
    launch(st, root, id);
    back
}

/// A fresh walk of the tree as a build left it, handed to the polls: the
/// preview held for a run's duration is what every poll's print is drawn
/// from, so it must list the files the build wrote (a release-only chunk is
/// absent from a walk that predates the first build) or the print the run is
/// re-anchored to and the print the polls draw never meet, and the run
/// re-mints on every poll. Returns the print of that walk.
fn reanchor_preview(st: &AppState) -> Option<String> {
    let p = st.first_project()?;
    let fresh = deploy::preview(&p).ok()?;
    let print = fingerprint(&p.root, &fresh);
    cache_put(
        &mut cache(st),
        p.root.clone(),
        Arc::new(Ok(fresh)),
        PREVIEW_CACHE_TTL,
    );
    Some(print)
}

/// Everything past the gates, shared by the button and the page's own
/// auto-start. A live delegated identity signs the deploy itself; otherwise
/// the linker hands its signing state to THIS server, and the signing URL is
/// set on the run only once the signer is live. `multi_scene: true` is
/// load-bearing (false silently deletes the world's other scenes), and the
/// inner JoinHandle is awaited so a deploy panic still reaches a terminal
/// state.
fn launch(st: Arc<AppState>, root: PathBuf, id: u64) {
    let automatic = runs(&st).as_ref().is_some_and(|r| r.id == id && r.auto);
    let identity = if automatic { None } else { live_identity(&st) };
    let host_signer = identity.is_none().then(|| {
        let register_st = st.clone();
        crate::linker::HostSigner {
            register: Arc::new(move |state| {
                let rebuilt = reanchor_preview(&register_st);
                let mut slot = runs(&register_st);
                if let Some(r) = slot.as_mut() {
                    if r.id == id {
                        r.signing = Some("/deploy".to_string());
                        if let Some(print) = rebuilt {
                            r.print = print;
                        }
                        *signer_slot(&register_st) = Some(state);
                    }
                }
            }),
            url: format!("http://127.0.0.1:{}/deploy", st.port),
        }
    });
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
        check_permissions: false,
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
            Ok(Ok(message)) => RunState::Done(message),
            Ok(Err(e)) => {
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

/// Whether the rights oracle speaks for `target_content`: any public Genesis
/// host, the public worlds server, or the server the destination reads from.
/// Elsewhere (a self-hosted node with its own policy) a mainnet-keyed refusal
/// must never block a server that would say yes.
fn oracle_speaks_for(dest: &Dest, target_content: &str) -> bool {
    let Some(host) = deploy::host_of(target_content) else {
        return false;
    };
    let matches = |url: &str| deploy::host_of(url).is_some_and(|h| h.eq_ignore_ascii_case(&host));
    deploy::UPSTREAM_CATALYST_HOSTS.iter().any(|u| matches(u))
        || matches(deploy::WORLDS_CONTENT_SERVER)
        || dest.read_bases.iter().any(|b| matches(b))
}

fn verdict_json(verdict: &str, why: &str, remedy: Option<&str>) -> Response {
    Json(serde_json::json!({ "verdict": verdict, "why": why, "remedy": remedy })).into_response()
}

/// `POST /deploy/preflight` — the refusal a catalyst would issue after the
/// signature, said before it to the exact address about to sign. Gated like
/// every signing route; the answer rides the cache the target page reads.
pub(super) async fn preflight(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Result<Json<PreflightReq>, axum::extract::rejection::JsonRejection>,
) -> Response {
    if let Some(blocked) = signing_blocked(&st, peer, &headers) {
        return blocked;
    }
    let address = match body {
        Ok(Json(req)) => req.address.trim().to_lowercase(),
        Err(_) => String::new(),
    };
    if !deploy_rights::valid_address(&address) {
        return reply(StatusCode::UNPROCESSABLE_ENTITY, "not an address");
    }
    let Some(project) = st.first_project() else {
        return reply(StatusCode::NOT_FOUND, "no scene loaded");
    };
    let dest = scene_dest(&project);
    if let Some(signer) = signer_slot(&st).clone() {
        let actual = signer.target_content().to_string();
        if !oracle_speaks_for(&dest, &actual) {
            return verdict_json(
                "unchecked",
                &format!(
                    "this publish goes to {}, whose deploy policy this preview cannot read \u{2014} that server decides",
                    deploy_status::host_of(&actual)
                ),
                None,
            );
        }
    }
    if st.deploy_dry_run {
        return verdict_json("unchecked", "live checks are off for this run", None);
    }
    let rights = deploy_rights::cached_rights(&st.deploy.rights, &dest, &address).await;
    match &rights.verdict {
        Verdict::May(w) => verdict_json("may", w, None),
        Verdict::MayNot { why, remedy } => verdict_json("may_not", why, Some(remedy.as_str())),
        Verdict::Unchecked(w) => verdict_json("unchecked", w, None),
    }
}

/// The gate every signing route shares: the wallet signs on the hosting
/// machine, so only a same-origin loopback peer that is not a tunnel replay
/// reaches the signer — unless --allow-remote-deploy opened it up.
fn signing_blocked(st: &AppState, peer: SocketAddr, headers: &HeaderMap) -> Option<Response> {
    if remote_peer(st.allow_remote_deploy, peer, headers) {
        return Some(forbidden(
            "signing happens on the machine hosting this preview",
        ));
    }
    cross_origin_refusal(headers).map(forbidden)
}

/// `POST /deploy/sign` — the one signing endpoint; its panel is
/// server-rendered inline on `/` and `/deploy`.
pub(super) async fn sign_submit(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Result<Json<crate::linker::SignReq>, axum::extract::rejection::JsonRejection>,
) -> Response {
    if let Some(blocked) = signing_blocked(&st, peer, &headers) {
        return blocked;
    }
    let Ok(body) = body else {
        return reply(StatusCode::UNPROCESSABLE_ENTITY, "not a signature");
    };
    let state = signer_slot(&st).clone();
    match state {
        Some(state) => crate::linker::sign(State(state), body)
            .await
            .into_response(),
        None => reply(StatusCode::NOT_FOUND, "no signature is pending"),
    }
}

/// `GET /deploy/progress` — how far the signed upload has got, for the
/// panel's bar; `idle` when nothing is signing.
pub(super) async fn sign_progress(State(st): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let state = signer_slot(&st).clone();
    Json(match state {
        Some(s) => serde_json::to_value(s.progress()).unwrap_or_default(),
        None => serde_json::json!({ "phase": "idle" }),
    })
}

/// Check and claim in one acquisition, returning the id the completion path
/// must present before it may write a terminal state.
fn claim(st: &AppState, target: String, auto: bool, print: String) -> Option<u64> {
    let mut slot = runs(st);
    if matches!(slot.as_ref().map(|r| &r.state), Some(RunState::Running)) {
        return None;
    }
    let id = next_run_id();
    *slot = Some(Run::new(id, target, auto, print));
    Some(id)
}

/// A pending run whose payload moved is a signature waiting to be wrong.
/// This replaces such a run — new id, current fingerprint, signer slot
/// emptied — but only where a re-mint is safe: the old build finished (its
/// signer registered), the wallet has not answered (a signed deploy is past
/// recall), and the run is the page's own (an adopted CLI signing carries no
/// fingerprint). The provenance survives: an auto run re-mints auto.
fn drift_reclaim(st: &AppState, target: String, print: &str) -> Option<u64> {
    let mut slot = runs(st);
    let (auto, remints) = match slot.as_ref() {
        Some(r)
            if matches!(r.state, RunState::Running)
                && r.signing.is_some()
                && !r.print.is_empty()
                && r.print != print =>
        {
            (r.auto, r.remints)
        }
        _ => return None,
    };
    if remints >= MAX_REMINTS {
        if remints == MAX_REMINTS {
            tracing::warn!(
                "the payload moved on {MAX_REMINTS} consecutive polls; keeping the entity as built"
            );
            slot.as_mut().expect("matched above").remints += 1;
        }
        return None;
    }
    {
        let mut signer = signer_slot(st);
        if signer
            .as_ref()
            .is_some_and(|s| s.signer_address().is_some())
        {
            return None;
        }
        *signer = None;
    }
    let id = next_run_id();
    *slot = Some(Run {
        remints: remints + 1,
        ..Run::new(id, target, auto, print.to_string())
    });
    Some(id)
}

/// Writes a terminal state, but only onto the run that claimed the slot. It
/// also retires the live-status cache, and — the one moment the signer names
/// its wallet — harvests the address and the history row before the signer
/// slot empties.
fn finish(st: &AppState, id: u64, state: RunState) {
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
    let (outcome, detail) = match &state {
        RunState::Done(message) => ("published", Some(message.clone())),
        RunState::Failed(why) => ("failed", Some(failure_detail(why))),
        RunState::Stale(paths) => (
            "nothing published",
            Some(match paths.is_empty() {
                true => "the scene changed while the page was open".to_string(),
                false => format!(
                    "the scene changed while the page was open: {}",
                    paths.join(", ")
                ),
            }),
        ),
        RunState::Running => ("", None),
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
                detail,
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
/// like the publish POST: the address is only read, but a stranger on the
/// LAN does not get to choose what an open page renders. Empty forgets.
pub(super) async fn target_address(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(form): Form<AddressForm>,
) -> Response {
    if let Some(refused) = post_gate(
        &st,
        st.allow_remote_deploy,
        peer,
        &headers,
        &form.token,
        "the wallet this preview checks is chosen on the machine hosting it",
        "stale or missing token \u{2014} reload /target and use the form there",
    ) {
        return refused;
    }
    let trimmed = form.address.trim();
    if trimmed.is_empty() {
        *address_slot(&st) = None;
    } else if deploy_rights::valid_address(trimmed) {
        *address_slot(&st) = Some(trimmed.to_lowercase());
    } else {
        return reply(
            StatusCode::UNPROCESSABLE_ENTITY,
            "that is not an Ethereum address",
        );
    }
    Redirect::to(&format!("{}/target", forwarded_prefix(&headers))).into_response()
}

#[derive(serde::Deserialize)]
pub(super) struct ConnectForm {
    token: String,
}

/// `POST /target/connect` — starts a Decentraland-account sign-in: the
/// browser is bounced onto the authorize page (the configured target's own
/// domain, or example.com) with a throwaway session key and this process's id in
/// the query; a background task picks the signed delegation up from the
/// single-read relay and remembers the address the signature proves.
pub(super) async fn target_connect(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(form): Form<ConnectForm>,
) -> Response {
    if let Some(refused) = post_gate(
        &st,
        st.allow_remote_deploy,
        peer,
        &headers,
        &form.token,
        "accounts connect on the machine hosting this preview",
        "stale or missing token \u{2014} reload /target and use the button there",
    ) {
        return refused;
    }
    if st.deploy_dry_run {
        return reply(
            StatusCode::SERVICE_UNAVAILABLE,
            "sign-in is off for this run",
        );
    }
    let bases =
        deploy_rights::working_auth_bases(deploy::configured_target_server().as_deref()).await;
    let (ephemeral, ephemeral_key) = loop {
        let key = hex(&rand::random::<[u8; 32]>());
        if let Ok(w) = catalyrst_crypto::Wallet::from_hex(&key) {
            break (w.address().to_lowercase(), key);
        }
    };
    let id = hex(&rand::random::<[u8; 16]>());
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

/// Waits on the relay for the approval: success stores the proven address
/// and clears the wait, a refusal becomes the sentence the account row shows.
/// The deadline is this task's, so an abandoned approval stops costing
/// requests after [`CONNECT_WINDOW`].
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
        let outcome = loop {
            if Instant::now() >= deadline {
                break Err("the sign-in expired \u{2014} connect again".to_string());
            }
            let resp = deploy_status::status_client()
                .get(format!("{relay}?id={id}"))
                .send()
                .await;
            if let Ok(resp) = resp {
                let status = resp.status().as_u16();
                if status == 200 {
                    break match resp.json::<serde_json::Value>().await {
                        Ok(v) => adopt_approval(&st, &v, &ephemeral, &ephemeral_key, &expiration),
                        Err(_) => Err("the relay sent an unreadable answer".to_string()),
                    };
                }
                if (400..500).contains(&status) {
                    break Err(format!("the relay refused the wait (HTTP {status})"));
                }
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        };
        match outcome {
            Ok(()) => *connect_slot(&st) = None,
            Err(why) => {
                if let Some(c) = connect_slot(&st).as_mut() {
                    c.state = ConnectState::Failed(why);
                }
            }
        }
    });
}

/// Believes a relayed approval only through its signature, then keeps both
/// the address it proves and the session key it delegates, so deploys sign
/// themselves for the hour.
fn adopt_approval(
    st: &AppState,
    entry: &serde_json::Value,
    ephemeral: &str,
    ephemeral_key: &str,
    expiration: &str,
) -> Result<(), String> {
    let addr = deploy_rights::relayed_address(ephemeral, expiration, entry)?;
    if let Some(sig) = entry.get("signature").and_then(|s| s.as_str()) {
        *identity_slot(st) = Some(deploy::DeployIdentity {
            signer: addr.clone(),
            ephemeral_key: ephemeral_key.to_string(),
            delegation_payload: deploy_rights::ephemeral_message(ephemeral, expiration),
            delegation_signature: sig.to_string(),
            expiration_ms: chrono::DateTime::parse_from_rfc3339(expiration)
                .map(|t| t.timestamp_millis())
                .unwrap_or_else(|_| deploy::now_ms()),
        });
    }
    *address_slot(st) = Some(addr);
    Ok(())
}

#[derive(serde::Deserialize)]
pub(super) struct PointForm {
    token: String,
    #[serde(default)]
    world: String,
}

/// The worlds tier's own dialect, nothing else.
fn valid_world_name(w: &str) -> bool {
    w.len() <= 100
        && w.ends_with(".eth")
        && w.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-')
}

/// Applies `change` to scene.json off the worker, retires every cache keyed
/// on the destination, and answers the form.
async fn edit_scene(
    st: &Arc<AppState>,
    prefix: &str,
    root: PathBuf,
    change: impl FnOnce(&mut serde_json::Value) -> Result<(), String> + Send + 'static,
) -> Response {
    let write_st = st.clone();
    let outcome =
        tokio::task::spawn_blocking(move || super::edit::edit_scene_json(&write_st, &root, change))
            .await;
    match outcome {
        Ok(Ok(_)) => {
            st.deploy.caches.clear();
            lock(&st.deploy.rights).clear();
            Redirect::to(&format!("{prefix}/target")).into_response()
        }
        Ok(Err((status, why))) => reply(status, &why),
        Err(_) => reply(
            StatusCode::INTERNAL_SERVER_ERROR,
            "the change did not finish",
        ),
    }
}

/// `POST /target/point` — re-aims the scene: a world name writes
/// `worldConfiguration.name` into scene.json, an empty value removes the
/// section and the scene deploys to its parcels on Genesis City. Gated like
/// the scene editors, never opened by --allow-remote-deploy: scene.json is
/// the developer's file.
pub(super) async fn target_point(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(form): Form<PointForm>,
) -> Response {
    if let Some(refused) = post_gate(
        &st,
        false,
        peer,
        &headers,
        &form.token,
        "the scene's destination is chosen on the machine hosting this preview",
        "stale or missing token \u{2014} reload /target and use the button there",
    ) {
        return refused;
    }
    let Some(project) = st.first_project() else {
        return reply(StatusCode::NOT_FOUND, "no scene loaded");
    };
    let world = form.world.trim().to_lowercase();
    if !world.is_empty() && !valid_world_name(&world) {
        return reply(StatusCode::UNPROCESSABLE_ENTITY, "that is not a world name");
    }
    edit_scene(
        &st,
        &forwarded_prefix(&headers),
        project.root.clone(),
        move |scene| {
            let obj = scene.as_object_mut().expect("edit_scene_json checked");
            if world.is_empty() {
                obj.remove("worldConfiguration");
                return Ok(());
            }
            match obj
                .entry("worldConfiguration")
                .or_insert_with(|| serde_json::json!({}))
                .as_object_mut()
            {
                Some(wc) => {
                    wc.insert("name".to_string(), serde_json::Value::String(world));
                    Ok(())
                }
                None => Err("worldConfiguration in scene.json is not an object".into()),
            }
        },
    )
    .await
}

#[derive(serde::Deserialize)]
pub(super) struct BaseForm {
    token: String,
    #[serde(default)]
    base: String,
}

/// The LAND contract's own bounds, district expansions included; worlds lay
/// their scenes out in the same coordinate space.
fn in_genesis(p: (i64, i64)) -> bool {
    let r = -150..=163;
    r.contains(&p.0) && r.contains(&p.1)
}

/// Translates every parcel by the same delta so the base lands on `new_base`;
/// spawn points ride along because they are metres from the base.
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
                .filter_map(catalyrst_auth_chain::pointer::parse_pointer)
                .collect()
        })
        .unwrap_or_default();
    if parcels.is_empty() {
        return Err("scene.json declares no parcels to move".into());
    }
    let old_base = sc
        .get("base")
        .and_then(|b| b.as_str())
        .and_then(catalyrst_auth_chain::pointer::parse_pointer)
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

/// `POST /target/base` — moves the whole footprint so its base parcel lands
/// where the form says (see [`translate_footprint`]). Gated like `/target/point`.
pub(super) async fn target_base(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(form): Form<BaseForm>,
) -> Response {
    if let Some(refused) = post_gate(
        &st,
        false,
        peer,
        &headers,
        &form.token,
        "the scene's base parcel is chosen on the machine hosting this preview",
        "stale or missing token \u{2014} reload /target and use the form there",
    ) {
        return refused;
    }
    let Some(project) = st.first_project() else {
        return reply(StatusCode::NOT_FOUND, "no scene loaded");
    };
    let Some(new_base) = catalyrst_auth_chain::pointer::parse_pointer(form.base.trim()) else {
        return reply(
            StatusCode::UNPROCESSABLE_ENTITY,
            "that is not an x,y parcel",
        );
    };
    if !in_genesis(new_base) {
        return reply(
            StatusCode::UNPROCESSABLE_ENTITY,
            "that parcel is outside the Genesis map",
        );
    }
    edit_scene(
        &st,
        &forwarded_prefix(&headers),
        project.root.clone(),
        move |scene| translate_footprint(scene, new_base),
    )
    .await
}

/// The rights view for the remembered address; the dry-run seam answers
/// without the network.
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

/// Spawns `fetch` unless a warm-up for `key` is already in flight.
fn warm<F: std::future::Future<Output = ()> + Send + 'static>(
    st: &Arc<AppState>,
    key: String,
    fetch: F,
) {
    if !warm_slot(st).insert(key.clone()) {
        return;
    }
    let st = st.clone();
    tokio::spawn(async move {
        fetch.await;
        warm_slot(&st).remove(&key);
    });
}

/// The live status and the wallet's rights without ever making the page wait:
/// a cold cache comes back as a "checking now" placeholder while a spawned
/// task fetches the real answer, and the third return says so (the reload
/// script keys on the marker it puts in the body). Every fetch outcome is a
/// cached value — failures are sentences — so the reload loop always lands.
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
    if cold_status {
        let (st2, project, dest2, preview, print2) = (
            st.clone(),
            project.clone(),
            dest.clone(),
            preview.clone(),
            print.to_string(),
        );
        warm(
            st,
            format!("status|{}|{print}", dest.headline),
            async move {
                if let Ok(p) = &*preview {
                    cached_status(&st2.deploy.caches, &project, &dest2, p, &print2).await;
                }
            },
        );
    }
    if let (true, Some(address)) = (cold_rights, address.clone()) {
        let (st2, dest2) = (st.clone(), dest.clone());
        warm(
            st,
            format!("rights|{address}|{}", dest.headline),
            async move {
                deploy_rights::cached_rights(&st2.deploy.rights, &dest2, &address).await;
            },
        );
    }
    let status =
        status.unwrap_or_else(|| Arc::new(LiveStatus::unknown("Checking the destination now")));
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

/// The marker the shared script's reload keys on, and the no-JS fallback.
fn warming_marker(warming: bool) -> &'static str {
    match warming {
        true => {
            r#"<div id="page-warming" hidden></div><noscript><meta http-equiv="refresh" content="2"></noscript>"#
        }
        false => "",
    }
}

/// Posts the same form the no-JS page posts and re-fetches this page's own
/// HTML to follow the run, so the server stays the single renderer.
const SCRIPT: &str = concat!(
    include_str!("page_common.js"),
    include_str!("sign_flow.js"),
    include_str!("deploy_page.js")
);

/// The status region: always rendered, so the script has one stable element
/// to swap and one `data-state` to read. `show_sign` is the signing gate's
/// answer for the requesting peer: the wallet panel renders only for a peer
/// `sign_submit` would accept.
fn run_region(st: &AppState, prefix: &str, show_sign: bool) -> String {
    let sign_panel = show_sign.then(|| pending_sign_panel(st, prefix)).flatten();
    run_region_for(prefix, runs(st).as_ref(), sign_panel.as_deref())
}

/// Pure over the run state, so a test can render every state without
/// mutating the slot other tests read through `served`. While running, the
/// region carries a `<noscript>` meta refresh and the signing path as a data
/// attribute — part of the shape the script compares before swapping, so a
/// live wallet panel is never wiped mid-flow.
fn run_region_for(prefix: &str, run: Option<&Run>, sign_panel: Option<&str>) -> String {
    let state = match run.map(|r| &r.state) {
        None => "idle",
        Some(RunState::Running) => "running",
        Some(RunState::Done(_)) => "done",
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
        panel = run_panel_for(run, sign_panel)
    )
}

fn run_panel_for(run: Option<&Run>, sign_panel: Option<&str>) -> String {
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
        RunState::Done(message) => {
            let detail = match deployed_parts(message) {
                Some((entity, server, _)) => format!(
                    r#"<p class="note">{}{}</p><span class="dep__cid">{}</span>"#,
                    esc(&r.target),
                    server
                        .map(|s| format!(" on {}", esc(s)))
                        .unwrap_or_default(),
                    esc(entity)
                ),
                None => format!(r#"<p class="note">{}</p>"#, esc(message)),
            };
            ("Published", detail, "", " panel--ok")
        }
        RunState::Failed(why) => (
            "Deploy failed",
            format!(
                r#"<pre class="note dep__err">{}</pre>"#,
                super::ansi::to_html(why)
            ),
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

fn deploy_document(st: &AppState, title: &str, prefix: &str, active: &str, body: &str) -> Response {
    super::chrome::html(document(
        title,
        prefix,
        if active == "target" {
            target_css()
        } else {
            PAGE_CSS
        },
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

/// Keep the design fonts self-contained, including behind a preview tunnel.
fn target_css() -> &'static str {
    static CSS: OnceLock<String> = OnceLock::new();
    CSS.get_or_init(|| {
        use base64::Engine;
        let mut css = PAGE_CSS.to_string();
        for (weight, bytes) in [
            (400, include_bytes!("fonts/Inter-UI-Regular.otf").as_slice()),
            (600, include_bytes!("fonts/Inter-UI-SemiBold.otf").as_slice()),
            (700, include_bytes!("fonts/Inter-UI-Bold.otf").as_slice()),
        ] {
            let data = base64::engine::general_purpose::STANDARD.encode(bytes);
            css.push_str(&format!("@font-face{{font-family:'Inter UI';src:url(data:font/otf;base64,{data}) format('opentype');font-weight:{weight};font-display:swap;}}"));
        }
        css.push_str(include_str!("target.css"));
        css
    })
}

/// The error branch both pages share, held to the same rule as the rest:
/// say what is wrong, never where the scene lives.
fn cannot_package(section: &str, scene_title: &str, why: &str, root: &Path) -> String {
    format!(
        r#"<main class="dash"><section id="{section}" class="sec">
              <div class="panel"><h2>{title}</h2><span class="note">This scene cannot be
              packaged yet: {why}</span></div></section></main>"#,
        title = esc(scene_title),
        why = esc(&scrub_paths(why, root))
    )
}

/// What a scene page has in hand once the walk and the live checks are in.
struct Drawn<'a> {
    prefix: &'a str,
    project: &'a Project,
    scene_title: &'a str,
    dest: &'a Dest,
    p: &'a deploy::DeployPreview,
    print: String,
    status: Arc<LiveStatus>,
    rights: Option<Arc<Rights>>,
    warming: bool,
}

/// The frame `/deploy` and `/target` share: no scene says `nothing`, a scene
/// that cannot package says why, and one that can is `render`ed.
async fn scene_page(
    st: &Arc<AppState>,
    headers: &HeaderMap,
    active: &str,
    nothing: &str,
    render: impl FnOnce(&Drawn<'_>) -> String,
) -> Response {
    let prefix = forwarded_prefix(headers);
    let Some(project) = st.first_project() else {
        return deploy_document(
            st,
            active,
            &prefix,
            active,
            &format!(
                r#"<main class="dash"><section id="{active}" class="sec"><div class="panel">
              <span class="note">{nothing}</span>
            </div></section></main>"#
            ),
        );
    };
    let scene_title = crate::joinblock::scene_title(&project.scene_json);
    let dest = scene_dest(&project);
    let preview = cached_preview(st, &project).await;
    let body = match &*preview {
        Ok(p) => {
            let print = fingerprint(&project.root, p);
            let (status, rights, warming) =
                status_and_rights(st, &project, &dest, &preview, &print).await;
            render(&Drawn {
                prefix: &prefix,
                project: &project,
                scene_title: &scene_title,
                dest: &dest,
                p,
                print,
                status,
                rights,
                warming,
            })
        }
        Err(e) => cannot_package(active, &scene_title, e, &project.root),
    };
    deploy_document(
        st,
        &format!("{active} {scene_title}"),
        &prefix,
        active,
        &body,
    )
}

async fn page(st: &Arc<AppState>, headers: &HeaderMap, local: bool) -> Response {
    let may_publish = st.allow_remote_deploy || local;
    let blocked = match may_publish {
        true => None,
        false => Some(
            "Publishing runs on the machine hosting this preview, because signing opens its wallet. \
             Open this page there, or start the preview with --allow-remote-deploy.",
        ),
    };
    scene_page(
        st,
        headers,
        "deploy",
        "No scene is loaded, so there is nothing to publish.",
        |d| {
            let p = d.p;
            let jump = deploy::play_url(d.dest.world.as_deref(), &d.dest.base_pointer);
            let clean = matches!(p.main, MainBundle::Present(_))
                && p.oversize.is_empty()
                && p.unreadable.is_empty()
                && p.collisions.is_empty()
                && !p.nameless_world;
            if clean
                && may_publish
                && !st.deploy_dry_run
                && std::env::var_os("DCL_PRIVATE_KEY").is_none()
                && live_identity(st).is_none()
            {
                let vacant = runs(st).is_none();
                let id = match vacant {
                    true => claim(st, d.dest.headline.clone(), true, d.print.clone()),
                    false => drift_reclaim(st, d.dest.headline.clone(), &d.print),
                };
                if let Some(id) = id {
                    launch(st.clone(), d.project.root.clone(), id);
                }
            }
            let phase = Phase::of(runs(st).as_ref(), Some(jump.as_str()));
            format!(
                r#"<main class="dash"><section id="deploy" class="sec">{alarms}{verdict}{card}{drawer}{warm}</section></main><script>{script}</script>"#,
                warm = warming_marker(d.warming),
                alarms = alarms(p),
                verdict = verdict_warn(d.rights.as_deref()),
                card = card(
                    d.prefix,
                    token(st),
                    d.scene_title,
                    d.dest,
                    p,
                    &d.status,
                    &d.print,
                    blocked,
                    d.rights.as_deref(),
                    phase,
                    &run_region(st, d.prefix, may_publish),
                ),
                drawer = payload_drawer(p),
                script = SCRIPT,
            )
        },
    )
    .await
}

/// The error text is served to anyone who can reach the port, and an anyhow
/// chain from further down may carry a path: taken back out here rather than
/// trusting every layer below to stay quiet.
fn scrub_paths(msg: &str, root: &Path) -> String {
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

/// `human_size`'s number apart from its unit, for the caption/number/unit stack.
fn split_size(bytes: u64) -> (String, String) {
    let whole = deploy::human_size(bytes);
    match whole.rsplit_once(' ') {
        Some((n, unit)) => (n.to_string(), unit.to_string()),
        None => (whole, String::new()),
    }
}

fn warn(title: &str, body: String) -> String {
    format!(
        r#"<div class="panel panel--warn"><h2>{}</h2><span class="note">{body}</span></div>"#,
        esc(title)
    )
}

pub(super) fn note_span(text: &str) -> String {
    format!(r#"<span class="note">{}</span>"#, esc(text))
}

/// The refusals a real deploy would hit, moved in front of the wallet prompt.
fn alarms(p: &deploy::DeployPreview) -> String {
    let mut out = match &p.main {
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
    if !p.oversize.is_empty() {
        out += &warn(
            "Over the per-file limit",
            format!(
                "a content server refuses a file over 50 MB, so this deploy would fail on: {}. \
                 Compress or split it, or exclude it in .dclignore.",
                esc(&p.oversize.join(", "))
            ),
        );
    }
    if !p.unreadable.is_empty() {
        out += &warn(
            "Cannot be read",
            format!(
                "these files are in the payload but their size could not be read: {}. deploy reads \
                 every file it uploads, so it would stop on them \u{2014} after your wallet had \
                 signed. A link pointing at something that is no longer there is the usual cause.",
                esc(&p.unreadable.join(", "))
            ),
        );
    }
    if !p.collisions.is_empty() {
        out += &warn(
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
        );
    }
    if p.nameless_world {
        out += &warn(
            "A world section that names no world",
            "scene.json has a worldConfiguration section without a name, and no server takes \
             that: a World needs the name, and a Genesis City catalyst refuses the section. \
             deploy refuses it before the wallet prompt. On /target, point the scene at Genesis \
             City LAND to remove the section, or pick one of your worlds."
                .to_string(),
        );
    }
    out
}

fn named_scenes(scenes: &[&RemoteScene]) -> String {
    let named: Vec<String> = scenes.iter().take(3).map(|s| s.title.clone()).collect();
    let tail = match scenes.len() > named.len() {
        true => format!(" and {} more", scenes.len() - named.len()),
        false => String::new(),
    };
    format!("{}{tail}", esc(&named.join(", ")))
}

/// The left column: what is on the target right now.
fn server_panel(dest: &Dest, status: &LiveStatus) -> String {
    let (ours, _) = footprint(dest);
    let others_row = |others: &[RemoteScene]| {
        let (replaced, kept): (Vec<_>, Vec<_>) = others
            .iter()
            .partition(|scene| dest.world.is_none() || scene_overlaps(&scene.coords, &ours));
        let row = |label: &str, scenes: &[&RemoteScene], fate: &str| {
            if scenes.is_empty() {
                return String::new();
            }
            kv(
                label,
                format!(
                    r#"<span class="note">{} — {fate}</span>"#,
                    named_scenes(scenes)
                ),
            )
        };
        format!(
            "{}{}",
            row("Also replaced", &replaced, "replaced by this publish"),
            row("Other scenes", &kept, "kept in place by this publish")
        )
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
        s = plural(p.files.len()),
    );
    let reuse_line = match &status.reuse {
        Some(r) => format!(r#"<span class="note">{}.</span>"#, esc(&r.sentence())),
        None => String::new(),
    };
    let meter = status.reuse.as_ref().map(|r| {
        let total = r.reused_files + r.upload_files;
        let percent = (r.reused_files * 100).checked_div(total).unwrap_or(0);
        format!(r#"<div class="tgt__reuse" role="meter" aria-label="Files already on the server" aria-valuemin="0" aria-valuemax="100" aria-valuenow="{percent}"><span style="width:{percent}%"></span></div>"#)
    }).unwrap_or_default();
    format!("{datum}{meter}{reuse_line}")
}

/// The card's footer: the primary button and the terminal line — or, for a
/// reader off the hosting machine, the refusal said out loud instead of a
/// button whose POST would be refused anyway.
fn foot(prefix: &str, tok: &str, print: &str, blocked: Option<&str>, phase: Phase) -> String {
    if let Some(why) = blocked {
        return format!(
            r#"<div class="jn2__foot"><span class="note">{}</span></div>"#,
            esc(why)
        );
    }
    let actions = match phase {
        Phase::Done(Some(jump)) => format!(
            r#"<a class="jn__cta" href="{}">Jump in</a>
          <button class="knob__go" type="submit">Publish again</button>"#,
            esc(jump)
        ),
        Phase::Done(None) => r#"<button class="jn__cta" type="submit">Publish again</button>"#.to_string(),
        _ => r#"<button class="jn__cta" type="submit">Publish</button>
          <span class="note">Publish signs with your connected session, or asks your wallet for a signature.
            You can also run <code>dcl-one-sdk deploy</code> in the scene folder.</span>"#
            .to_string(),
    };
    format!(
        r#"<form class="jn2__foot" id="publish" method="post" action="{prefix_esc}/deploy">
          <input type="hidden" name="token" value="{tok}">
          <input type="hidden" name="fingerprint" value="{print_esc}">
          {actions}
        </form>"#,
        prefix_esc = esc(prefix),
        tok = esc(tok),
        print_esc = esc(print),
    )
}

/// Where the card's run stands, as far as the head line and footer care.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase<'a> {
    /// No run, or one that failed or went stale: the button starts one.
    Idle,
    /// Building or signing: no button, the region carries the hand-off.
    Running,
    /// Published: the head says so, Jump in (at the realm URL, when there
    /// is one) is the call to action, and another run is the quiet button.
    Done(Option<&'a str>),
}

impl<'a> Phase<'a> {
    fn of(run: Option<&Run>, jump: Option<&'a str>) -> Self {
        match run.map(|r| &r.state) {
            Some(RunState::Running) => Phase::Running,
            Some(RunState::Done(_)) => Phase::Done(jump),
            _ => Phase::Idle,
        }
    }
}

/// `0x` plus enough hex to recognize a wallet without a full line of it.
pub(super) fn short_addr(addr: &str) -> String {
    match addr.len() > 12 {
        true => format!("{}\u{2026}{}", &addr[..6], &addr[addr.len() - 4..]),
        false => addr.to_string(),
    }
}

/// The warn panel a refusing verdict earns on /deploy: the deploy's own
/// message, moved before the wallet prompt.
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

/// The slim publish card: one line saying what goes where, the run region
/// (spinner, wallet panel, outcome) INSIDE the card where the publish was
/// clicked, and the foot — which disappears while a run is live, since the
/// region above is the story.
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
    phase: Phase,
    run: &str,
) -> String {
    let (size_num, size_unit) = split_size(p.total_bytes);
    let state_word = match (phase, &status.remote) {
        (Phase::Done(_), _) => "just published",
        (_, Remote::Known(state)) if state.current.is_some() => "updates the live scene",
        (_, Remote::Known(_) | Remote::Empty) => "first publish",
        (_, Remote::Unreachable(_) | Remote::Unknown(_)) => "live state unknown",
    };
    let parcels = dest.pointers.len().max(1);
    let payload = match &status.reuse {
        Some(r) => esc(&r.sentence()),
        None => format!(
            "{} file{} · {size_num} {size_unit}",
            p.files.len(),
            plural(p.files.len())
        ),
    };
    format!(
        r#"<div class="jn">
      <div class="jn2__head"><div class="jn2__title">
        <h2>{title} → {headline}</h2>
        <span class="jn__hint">{parcels} parcel{ps} · {payload} · {state_word}{verdict} — <a href="{prefix_esc}/target">review the target</a></span></div></div>
      {run}
      {foot}
    </div>"#,
        title = esc(scene_title),
        headline = esc(&dest.headline),
        ps = plural(parcels),
        verdict = verdict_bit(rights),
        prefix_esc = esc(prefix),
        foot = match phase {
            Phase::Running => String::new(),
            _ => foot(prefix, tok, print, blocked, phase),
        },
    )
}

fn col(head: &str, body: &str) -> String {
    format!(r#"<div class="jn2__col"><span class="knob__k">{head}</span>{body}</div>"#)
}

fn empty_col(head: &str, note: &str) -> String {
    col(head, &format!(r#"<span class="note">{note}</span>"#))
}

/// An empty state whose remedy is connecting: the bar's split pill again,
/// right where the page says it is waiting on an account, so nobody has to
/// hunt for it.
fn connect_col(head: &str, note: &str, prefix: &str, tok: &str) -> String {
    col(
        head,
        &format!(
            r#"<span class="note">{note}</span>{}"#,
            connect_buttons(prefix, tok)
        ),
    )
}

/// The bar's two doors, again: the browser wallet (script-armed through
/// `data-wallet`, inert without one) and the Decentraland sign-in form.
pub(super) fn connect_buttons(prefix: &str, tok: &str) -> String {
    format!(
        r#"<span class="bar__acct--split tgt__connect"><button class="bar__cta" type="button" data-wallet>Connect Wallet</button><form method="post" action="{action}"><input type="hidden" name="token" value="{tok}"><button class="bar__cta" type="submit">Connect with DCL</button></form></span>"#,
        action = esc(&format!("{prefix}/target/connect")),
        tok = esc(tok),
    )
}

const HELP_ACTION: &str = r#"<a class="jn__cta" href="https://docs.decentraland.org/creator/scene-editor/publish/publish-scene" target="_blank" rel="noopener">Publishing guide <span aria-hidden="true">↗</span></a>"#;

/// Guides under [`DOCS`], shown as their path.
const DOCS: &str = "https://docs.decentraland.org/";
const HELP_GUIDES: &[(&str, &str)] = &[
    (
        "Publish your scene",
        "creator/scene-editor/publish/publish-scene#publish-your-scene",
    ),
    (
        "Kinds of projects",
        "creator/scenes-sdk7/kinds-of-projects/kinds-of-project",
    ),
    (
        "Multi-scene Worlds",
        "creator/scene-editor/publish/publish-scene#multi-scene-worlds",
    ),
    (
        "Collaborators",
        "creator/scene-editor/publish/publish-scene#adding-collaborators-to-a-multi-scene-world",
    ),
    (
        "Scene metadata",
        "creator/scenes-sdk7/projects/scene-metadata",
    ),
    (
        "Scene overwriting",
        "creator/scene-editor/publish/publish-scene#scene-overwriting",
    ),
    (
        "Custom servers",
        "creator/scene-editor/publish/publish-scene#custom-servers",
    ),
];

fn help_rows() -> String {
    let rows: String = HELP_GUIDES
        .iter()
        .map(|(label, path)| {
            kv(
                label,
                format!(
                    r#"<a href="{DOCS}{path}" target="_blank" rel="noopener">{}</a>"#,
                    esc(path)
                ),
            )
        })
        .collect();
    format!(r#"<div class="kvs">{rows}</div>"#)
}

/// What an off-host viewer gets instead of silent 403s: this page's forms
/// only act from the machine hosting the preview.
pub(super) fn remote_notice(local: bool, class: &str, clause: &str) -> String {
    if local {
        return String::new();
    }
    format!(
        r#"<p class="note {class}">This preview is hosted on another machine, so {clause}. Open this page on the hosting machine to make changes.</p>"#
    )
}

/// The account's row inside the card, drawn only when there is something to
/// say beyond the bar's pill: the wait on a pending sign-in, or why the last
/// one failed. The connect buttons live in the bar, and again inside the
/// empty columns waiting on an account — the account is a server-wide fact,
/// not this card's field.
fn account_row(rights: Option<&Rights>, connect: Option<&Connect>) -> String {
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

/// The shared bar behaviour plus following a pending sign-in by reloading.
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

/// The base parcel as a chip with "Move scene" beside it; opening it reveals
/// the one-row form that moves the scene's footprint. Land only — a world
/// positions its scenes internally.
fn base_form(prefix: &str, tok: &str, dest: &Dest) -> String {
    format!(
        r#"<div class="tgt__base-row"><span class="knob__k">Base parcel</span><details class="tgt__move"><summary><span class="tgt__coord tgt__coord--base">{base_esc}</span><span class="deep__copy">Move scene</span></summary><form class="tgt__base" method="post" action="{prefix_esc}/target/base"><input type="hidden" name="token" value="{tok_esc}"><input name="base" value="{base_esc}" aria-label="base parcel x,y" spellcheck="false" autocomplete="off"><button class="deep__copy" type="submit">Set base parcel</button><span class="jn__hint">Shifts this project's entire footprint. Publish to apply the new location.</span></form></details></div>"#,
        prefix_esc = esc(prefix),
        tok_esc = esc(tok),
        base_esc = esc(&dest.base_pointer),
    )
}

/// The worlds the wallet can deploy to, each row saying what is there now
/// (from the one /worlds answer) and, unless it is the target, the button
/// that points the scene at it. The current target leads the list so the
/// cap can never be the reason it is missing.
fn your_worlds(prefix: &str, tok: &str, dest: &Dest, rights: Option<&Rights>) -> String {
    let Some(r) = rights else {
        return connect_col(
            "Your worlds",
            "Connect an account to list its Decentraland NAMEs, ENS domains, and Worlds where it has deployment permission.",
            prefix,
            tok,
        );
    };
    if let Verdict::Unchecked(why) = &r.verdict {
        if r.worlds.is_empty() {
            return empty_col("Your worlds", &esc(why));
        }
    }
    let note = r.worlds_note.as_deref().map(note_span).unwrap_or_default();
    if r.worlds.is_empty() {
        return format!(
            "{}{note}",
            empty_col(
                "Your worlds",
                &format!(
                    "No Worlds found for {}. Use an account that owns a Decentraland NAME or ENS domain, or has been granted deployment permission.",
                    esc(&short_addr(&r.address))
                ),
            )
        );
    }
    let mut listed: Vec<_> = r.worlds.iter().collect();
    listed.sort_by_key(|w| dest.world.as_deref() != Some(w.name.as_str()));
    let render_row = |w: &&deploy_rights::WorldRow| {
        let target = dest.world.as_deref() == Some(w.name.as_str());
        let mut bits = Vec::new();
        if let Some(t) = &w.title {
            bits.push(esc(t));
        }
        match w.scenes {
            Some(n) if n > 1 => bits.push(format!("{n} scenes")),
            Some(1) => bits.push("1 scene".into()),
            Some(0) => bits.push("no scenes yet".into()),
            _ => bits.push("scene count unavailable".into()),
        }
        if w.scenes.unwrap_or(0) > 0 {
            if let Some(ts) = w.last_deployed {
                bits.push(format!("updated {}", ago(ts, deploy::now_ms())));
            }
        }
        if !w.owned {
            bits.push("Collaborator · deployment permission".into());
        }
        let action = if target {
            r#"<span class="tgt__badge">Current target</span>"#.to_string()
        } else {
            point_form(prefix, tok, &w.name, "Select World")
        };
        format!(
            r#"<div class="wl__r"><span class="wl__n">{}</span><span class="wl__d">{}</span>{action}</div>"#,
            esc(&w.name),
            bits.join(" · ")
        )
    };
    let group = |owned: bool| {
        let (empty, populated): (Vec<_>, Vec<_>) = listed
            .iter()
            .copied()
            .filter(|w| w.owned == owned)
            .partition(|w| w.scenes == Some(0) && dest.world.as_deref() != Some(w.name.as_str()));
        let rows: String = populated.iter().map(&render_row).collect();
        let folded = if empty.is_empty() {
            String::new()
        } else {
            let rows: String = empty.iter().map(&render_row).collect();
            format!(
                r#"<details class="tgt__empty"><summary>Show {} world{} with no scenes yet</summary><div class="wl">{rows}</div></details>"#,
                empty.len(),
                plural(empty.len())
            )
        };
        let nothing = if populated.is_empty() && empty.is_empty() {
            r#"<span class="note">No worlds to show.</span>"#
        } else {
            ""
        };
        col(
            if owned {
                "Your worlds"
            } else {
                "Shared with you"
            },
            &format!(r#"<div class="wl">{rows}</div>{folded}{nothing}"#),
        )
    };
    format!("{}{}{}{note}", group(true), group(false), note_span("A collaborator may publish to all parcels or only assigned coordinates. Permission to visit a World does not grant permission to publish."))
}

fn span(vs: impl Iterator<Item = i64>) -> (i64, i64) {
    vs.fold((i64::MAX, i64::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)))
}

/// The legend and cell grid both maps share — the layout card's own cell
/// idiom, so the maps read as one language. `classify` names a cell's
/// modifier class and title.
fn parcel_grid(
    keys: &[(&str, &str)],
    label: &str,
    xs: (i64, i64),
    ys: (i64, i64),
    classify: impl Fn((i64, i64)) -> (&'static str, String),
) -> String {
    format!(
        r#"<div class="lay__legend">{}</div>{}"#,
        parcel_legend(keys),
        parcel_cells(label, xs, ys, false, classify)
    )
}

/// The swatch keys alone, so a card can seat them in its own header row.
fn parcel_legend(keys: &[(&str, &str)]) -> String {
    keys.iter()
        .map(|(class, name)| {
            format!(r#"<span class="lay__key"><i class="lay__swatch {class}"></i>{name}</span>"#)
        })
        .collect()
}

/// The cell grid alone. `coords` prints each parcel's coordinates inside its
/// cell, the target map's idiom; the after-map keeps its cells bare.
fn parcel_cells(
    label: &str,
    (x0, x1): (i64, i64),
    (y0, y1): (i64, i64),
    coords: bool,
    classify: impl Fn((i64, i64)) -> (&'static str, String),
) -> String {
    let mut cells = String::new();
    for y in (y0..=y1).rev() {
        for x in x0..=x1 {
            let (class, title) = classify((x, y));
            let inner = if coords {
                format!("<span>{x},{y}</span>")
            } else {
                String::new()
            };
            cells.push_str(&format!(
                r#"<div class="lay__cell{class}" title="{title}">{inner}</div>"#
            ));
        }
    }
    format!(
        r#"<div class="lay__map" style="--lay-cols:{cols}"><div class="lay__grid" role="img" aria-label="{label}">{cells}</div></div>"#,
        cols = x1 - x0 + 1
    )
}

/// The declared footprint in the accent, every parcel the wallet owns or
/// operates lit up around it. The window centres on the footprint; holdings
/// beyond it are a count, never silently gone.
fn land_map(
    declared: &[(i64, i64)],
    base: (i64, i64),
    owned: &[(i64, i64)],
    missing: &HashSet<(i64, i64)>,
) -> String {
    let head = |legend: &str| {
        format!(
            r#"<div class="tgt__map-head"><span class="knob__k">Map</span><div class="lay__legend">{legend}</div></div>"#
        )
    };
    if declared.is_empty() {
        return head("");
    }
    let (dx0, dx1) = span(declared.iter().map(|p| p.0));
    let (dy0, dy1) = span(declared.iter().map(|p| p.1));
    if dx1 - dx0 > 63 || dy1 - dy0 > 63 {
        return format!(
            "{}{}",
            head(""),
            note_span(
                "This footprint is too spread out for the map. Review the parcel list below.",
            )
        );
    }
    let pad_x = (10 - (dx1 - dx0 + 1)).max(2) / 2;
    let pad_y = (8 - (dy1 - dy0 + 1)).max(2) / 2;
    let (x0, x1) = (dx0 - pad_x, dx1 + pad_x);
    let (y0, y1) = (dy0 - pad_y, dy1 + pad_y);
    let mine: HashSet<(i64, i64)> = declared.iter().copied().collect();
    let yours: HashSet<(i64, i64)> = owned.iter().copied().collect();
    let off_map = owned
        .iter()
        .filter(|(x, y)| *x < x0 || *x > x1 || *y < y0 || *y > y1)
        .count();
    let mut keys = vec![
        ("lay__swatch--base", "Base"),
        ("lay__swatch--in", "This scene"),
    ];
    if !owned.is_empty() {
        keys.push(("dep__cell--own", "Yours"));
    }
    if !missing.is_empty() {
        keys.push(("tgt__cell--missing", "No rights"));
    }
    let cells = parcel_cells(
        "your land around this scene",
        (x0, x1),
        (y0, y1),
        true,
        |p| {
            let (x, y) = p;
            if missing.contains(&p) {
                (" tgt__cell--missing", format!("{x},{y} — no update rights"))
            } else if p == base {
                (" lay__cell--base", format!("Base parcel {x},{y}"))
            } else if mine.contains(&p) {
                (" lay__cell--in", format!("This scene {x},{y}"))
            } else if yours.contains(&p) {
                (" dep__cell--own", format!("Yours {x},{y}"))
            } else {
                ("", format!("{x},{y}"))
            }
        },
    );
    let off = match off_map {
        0 => String::new(),
        n => format!(
            r#"<span class="note">and {n} of your parcel{s} beyond this window</span>"#,
            s = plural(n),
        ),
    };
    format!("{}{cells}{off}", head(&parcel_legend(&keys)))
}

/// The declared parcels and the base among them (the first parcel, or 0,0,
/// when the base does not parse).
pub(super) fn footprint(dest: &Dest) -> (Vec<(i64, i64)>, (i64, i64)) {
    let declared = deploy_status::parse_coords(&dest.pointers);
    let base = catalyrst_auth_chain::pointer::parse_pointer(&dest.base_pointer)
        .unwrap_or_else(|| declared.first().copied().unwrap_or((0, 0)));
    (declared, base)
}

/// One row per declared parcel with the strongest right the wallet holds on
/// it, and the holdings line under them.
fn land_rights_col(prefix: &str, tok: &str, rights: Option<&Rights>) -> String {
    let Some(r) = rights else {
        return connect_col(
            "Your rights here",
            "Connect an account to check every declared parcel against its on-chain rights.",
            prefix,
            tok,
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
    let note = r.parcels_note.as_deref().map(note_span).unwrap_or_default();
    let body = match (rows.is_empty(), &r.verdict) {
        (true, Verdict::Unchecked(why)) => note_span(why),
        (true, _) => r#"<span class="note">No declared parcel to check.</span>"#.to_string(),
        (false, _) => {
            let unchecked = match r.unchecked_parcels {
                0 => String::new(),
                n => format!(
                    r#"<span class="note">and {n} more parcel{} unchecked</span>"#,
                    plural(n)
                ),
            };
            format!(r#"<div class="kvs">{rows}</div>{unchecked}{note}"#)
        }
    };
    let holdings = match r.holdings.as_ref() {
        Some(h) => format!(
            r#"<span class="note">{} holds {} parcel{}, {} estate{} and operates {} more.</span>{}"#,
            esc(&short_addr(&r.address)),
            h.parcels,
            plural(h.parcels as usize),
            h.estates,
            plural(h.estates as usize),
            h.operated,
            permitted_parcels(h)
        ),
        None => String::new(),
    };
    col("Your rights here", &format!("{body}{holdings}"))
}

const PARCELS_LISTED: usize = 24;

/// Owned then operated coordinates, each capped with the remainder counted.
fn permitted_parcels(h: &Holdings) -> String {
    let spell = |coords: &[(i64, i64)]| -> String {
        let named: Vec<String> = coords
            .iter()
            .take(PARCELS_LISTED)
            .map(|(x, y)| format!("{x},{y}"))
            .collect();
        let rest = coords.len().saturating_sub(PARCELS_LISTED);
        match rest {
            0 => named.join(" \u{b7} "),
            n => format!("{} and {n} more", named.join(" \u{b7} ")),
        }
    };
    if h.owned.is_empty() && h.operated_coords.is_empty() {
        return String::new();
    }
    let mut rows = String::new();
    if !h.owned.is_empty() {
        rows.push_str(&kv(
            "Owned",
            format!("<span>{}</span>", esc(&spell(&h.owned))),
        ));
    }
    if !h.operated_coords.is_empty() {
        rows.push_str(&kv(
            "Operated",
            format!("<span>{}</span>", esc(&spell(&h.operated_coords))),
        ));
    }
    format!(
        r#"<span class="knob__k">Parcels you may publish to</span><div class="kvs">{rows}</div>"#
    )
}

/// Two destination views, matching the publishing docs. Multi-scene details
/// belong to the selected World; history is independent of selection.
#[allow(clippy::too_many_arguments)]
fn target_card(
    title: &str,
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
    let blocked = matches!(rights.map(|r| &r.verdict), Some(Verdict::MayNot { .. }));
    let deploy_action = if blocked {
        r#"<span class="tgt__blocked">Deploy blocked — resolve rights below</span>"#.to_string()
    } else {
        format!(
            r#"<a class="jn__cta" href="{}/deploy">{} <span aria-hidden="true">→</span></a>"#,
            esc(prefix),
            "Deploy"
        )
    };
    let current = match world {
        Some(w) => format!(
            "Selected destination: World {w} · Base {}",
            dest.base_pointer
        ),
        None => format!("Selected destination: LAND · Base {}", dest.base_pointer),
    };
    let server = dest
        .server_line
        .split(" — ")
        .next()
        .unwrap_or(&dest.server_line);
    let header = |overline: &str, action: &str| {
        format!(
            r#"<div class="tgt__head"><div class="tgt__title"><span class="knob__k">{}</span><h1>{}</h1><span class="tgt__current"><i></i>{}</span><span class="note">Publishing {}</span></div>{action}</div>"#,
            esc(overline),
            esc(title),
            esc(&current),
            esc(server)
        )
    };
    let missing: Vec<&str> = match (world, rights) {
        (None, Some(r)) if blocked => r
            .parcel_rights
            .iter()
            .filter(|pr| pr.leg.is_none())
            .map(|pr| pr.pointer.as_str())
            .collect(),
        _ => Vec::new(),
    };
    let verdict = match rights.map(|r| &r.verdict) {
        Some(Verdict::MayNot { why, remedy }) => {
            let (title, why_line) = if missing.is_empty() {
                (
                    "You can't publish here".to_string(),
                    format!("<p>{}</p>", esc(why)),
                )
            } else {
                let total = rights.map(|r| r.parcel_rights.len()).unwrap_or(0);
                (
                    format!(
                        "You can't publish to {} of {total} parcel{}",
                        missing.len(),
                        plural(total)
                    ),
                    String::new(),
                )
            };
            let rows: String = missing
                .iter()
                .map(|pointer| {
                    format!(
                        r#"<div class="tgt__missing-row"><span class="tgt__coord">{}</span><span>No update rights on this parcel</span></div>"#,
                        esc(pointer)
                    )
                })
                .collect();
            format!(
                r#"<div class="panel tgt__rights-failure"><h2><i class="tgt__bang" aria-hidden="true">!</i>{title}</h2>{why_line}{rows}<p class="note">{}</p></div>"#,
                esc(remedy)
            )
        }
        Some(Verdict::Unchecked(why)) => note_span(why),
        _ => String::new(),
    };
    let summary = format!(
        r#"<div class="tgt__summary">{}{}</div>"#,
        col("Upload", &upload_panel(p, status)),
        col("On the server now", &server_panel(dest, status))
    );
    let land_action = if world.is_some() || p.nameless_world {
        point_form(prefix, tok, "", "Select LAND")
    } else {
        deploy_action.clone()
    };
    let land_pane = if let Some(world) = world {
        format!(
            r#"{}<p class="tgt__inactive"><b aria-hidden="true">·</b><span>The deploy target is World {}. Select LAND to deploy to Genesis City with this scene's parcel coordinates. You need deployment permission on every parcel.</span></p><div class="tgt__map-panel tgt__map-panel--solo">{}</div>"#,
            header("Publish to LAND", &land_action),
            esc(world),
            super::land_picker::land_picker(prefix, tok, dest, rights, status)
        )
    } else {
        format!(
            r#"{}{}<div class="tgt__land">{summary}<div class="tgt__map-panel">{}{}</div></div>"#,
            header("Publish to LAND", &land_action),
            verdict,
            target_land_map(prefix, tok, dest, rights, status),
            format_args!(
                r#"<details class="tgt__rights-detail"><summary>Parcel rights and holdings</summary>{}</details>"#,
                land_rights_col(prefix, tok, rights)
            )
        )
    };
    let world_action = if world.is_some() {
        deploy_action.clone()
    } else {
        note_span("Select a World below")
    };
    let record_action = if world.is_some() || !p.nameless_world {
        deploy_action.clone()
    } else {
        note_span("Select a World or LAND first")
    };
    let multi_pane = match world {
        Some(_) => format!(
            r#"{}<div class="tgt__advanced-body"><p class="note">A World can contain multiple scenes. Publishing from this preview preserves scenes that do not overlap this project's parcels. World settings and collaborator permissions are managed by the World owner.</p><p class="note">These coordinates are inside the selected World; they are not Genesis City LAND. <a href="{}/scene">Edit this scene's parcel layout</a>.</p>{}<a href="https://docs.decentraland.org/creator/scene-editor/publish/publish-scene#multi-scene-worlds">About Multi-Scene Worlds</a></div>"#,
            header("Multiscene world", &record_action),
            esc(prefix),
            multiscene_pane(dest, status)
        ),
        None => format!(
            r#"{}<p class="tgt__inactive"><b aria-hidden="true">·</b><span>The deploy target is Genesis City LAND. Multiscene applies to Worlds: select a World first.</span></p>"#,
            header("Multiscene world", &record_action)
        ),
    };
    let history_pane = format!(
        "{}{}",
        header("Deployment history", &record_action),
        history_rows_pane(history)
    );
    let help_pane = format!(
        "{}{}",
        header("Help", HELP_ACTION),
        col("Guides", &help_rows())
    );
    let world_pane = format!(
        r#"{}{}{}<div id="target-worlds" class="tgt__worlds">{}</div>"#,
        header("Publish to a World", &world_action),
        if world.is_some() {
            verdict.as_str()
        } else {
            ""
        },
        if world.is_some() {
            format!(r#"<div class="tgt__world-summary">{summary}</div>"#)
        } else {
            String::new()
        },
        your_worlds(prefix, tok, dest, rights)
    );
    let tabs = format!(
        "{}{}{}{}{}",
        tab("world", "World", world.is_some()),
        tab("land", "LAND (Genesis City)", world.is_none()),
        tab("multi", "Multiscene world", false),
        tab("history", "History", false),
        tab("help", "Help", false)
    );
    format!(
        r#"<div class="jn tgt" data-target-kind="{target_kind}">
      {addr}
      <fieldset class="knob knob--tabs"><legend class="knob__k u-sr-only">Browse publishing destinations</legend><div class="jn2__tabs">{tabs}</div></fieldset>
      <div class="tgt__pane tgt__pane--world">{world_pane}</div>
      <div class="tgt__pane tgt__pane--land">{land_pane}</div>
      <div class="tgt__pane tgt__pane--multi">{multi_pane}</div>
      <div class="tgt__pane tgt__pane--history">{history_pane}</div>
      <div class="tgt__pane tgt__pane--help">{help_pane}</div>
    </div>"#,
        addr = account_row(rights, connect),
        target_kind = if world.is_some() { "world" } else { "land" },
    )
}

/// The map and move actions use real coordinates; mock estate titles and
/// owner identities from the design must never stand in for chain data.
fn target_land_map(
    prefix: &str,
    tok: &str,
    dest: &Dest,
    rights: Option<&Rights>,
    status: &LiveStatus,
) -> String {
    let (declared, base) = footprint(dest);
    let owned = rights
        .and_then(|r| r.holdings.as_ref())
        .map(|h| h.coords.as_slice())
        .unwrap_or_default();
    let missing: HashSet<_> = rights
        .into_iter()
        .flat_map(|r| &r.parcel_rights)
        .filter(|pr| pr.leg.is_none())
        .filter_map(|pr| catalyrst_auth_chain::pointer::parse_pointer(&pr.pointer))
        .collect();
    let map = land_map(&declared, base, owned, &missing);
    format!(
        "{map}{}{}",
        base_form(prefix, tok, dest),
        super::land_picker::land_picker(prefix, tok, dest, rights, status)
    )
}

/// The deploy's own success line, `Deployed <entity> to <server> (HTTP <n>)`,
/// taken apart as (entity, server, status); `None` for any other wording.
fn deployed_parts(detail: &str) -> Option<(&str, Option<&str>, &str)> {
    let rest = detail.strip_prefix("Deployed ")?;
    let (body, tail) = rest.rsplit_once(" (HTTP ")?;
    let http = tail.strip_suffix(')')?;
    if http.is_empty() || !http.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let (entity, server) = match body.split_once(" to ") {
        Some((e, s)) => (e, Some(s)),
        None => (body, None),
    };
    if entity.is_empty() || entity.contains(' ') {
        return None;
    }
    Some((entity, server, http))
}

/// The success line as the design's three chips; any other detail stays a
/// plain note.
fn published_detail(detail: &str) -> Option<String> {
    let (entity, server, http) = deployed_parts(detail)?;
    let server = server
        .map(|s| format!(r#"<span class="note">to <b>{}</b></span>"#, esc(s)))
        .unwrap_or_default();
    Some(format!(
        r#"<div class="tgt__history-detail"><span class="tgt__history-cid" title="{e}">{e}</span>{server}<span class="tgt__history-http">HTTP {}</span></div>"#,
        esc(http),
        e = esc(entity)
    ))
}

/// Deployment history: what this preview (and, through the on-disk record,
/// earlier previews of this scene) actually published.
fn history_rows_pane(history: &[PastRun]) -> String {
    if history.is_empty() {
        return empty_col(
            "No deployments yet",
            "Nothing has been published from this preview — publishes from here will list with age, signer and what came of them.",
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
            let detail = match h.detail.as_deref().filter(|d| !d.trim().is_empty()) {
                Some(d) if d.contains('\n') => {
                    format!(r#"<pre class="note dep__err">{}</pre>"#, esc(d))
                }
                Some(d) => published_detail(d).unwrap_or_else(|| note_span(d)),
                None => String::new(),
            };
            let tone = match h.outcome.as_str() {
                "published" => "ok",
                "failed" => "failed",
                _ => "neutral",
            };
            format!(r#"<div class="tgt__history-row"><div class="tgt__history-head"><span class="tgt__history-age">{}</span><span class="tgt__history-target">{}{}</span><span class="tgt__history-status tgt__history-status--{tone}">{}</span></div>{detail}</div>"#,
                esc(&ago(h.at_ms, deploy::now_ms())), esc(&h.target), esc(&by), esc(&h.outcome))

        })
        .collect();
    col(
        "Published from here",
        &format!(r#"<div class="kvs">{rows}</div>"#),
    )
}

/// How many cells a side the after-map will draw; past it the scene rows
/// tell the story and the grid would be a wall of unreadable pixels.
const AFTER_MAP_SPAN: i64 = 24;

/// A scene is indivisible: touching one parcel replaces the whole entity.
fn scene_overlaps(scene: &[(i64, i64)], footprint: &[(i64, i64)]) -> bool {
    scene.iter().any(|p| footprint.contains(p))
}

/// The world as this publish leaves it: the scenes that stay in the neutral
/// swatch, the replaced scene's old footprint dashed, this scene's parcels
/// in the accent.
fn after_map(remote: &RemoteState, ours: &[(i64, i64)], base: (i64, i64)) -> String {
    let kept: HashSet<(i64, i64)> = remote
        .others
        .iter()
        .filter(|s| !scene_overlaps(&s.coords, ours))
        .flat_map(|s| s.coords.iter().copied())
        .collect();
    let was: HashSet<(i64, i64)> = remote
        .current
        .iter()
        .flat_map(|c| c.coords.iter().copied())
        .chain(
            remote
                .others
                .iter()
                .filter(|s| scene_overlaps(&s.coords, ours))
                .flat_map(|s| s.coords.iter().copied()),
        )
        .collect();
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
    let xs = span(all.iter().map(|p| p.0));
    let ys = span(all.iter().map(|p| p.1));
    if xs.1 - xs.0 + 1 > AFTER_MAP_SPAN || ys.1 - ys.0 + 1 > AFTER_MAP_SPAN {
        return String::new();
    }
    parcel_grid(
        &[
            ("lay__swatch--base", "Base"),
            ("lay__swatch--in", "This scene"),
            ("dep__cell--kept", "Kept"),
            ("dep__cell--was", "Replaced"),
        ],
        "world parcels after this publish",
        xs,
        ys,
        |p| {
            let (x, y) = p;
            if p == base && mine.contains(&p) {
                (" lay__cell--base", format!("Base parcel {x},{y}"))
            } else if mine.contains(&p) {
                (" lay__cell--in", format!("This scene {x},{y}"))
            } else if kept.contains(&p) {
                (" dep__cell--kept", format!("Kept {x},{y}"))
            } else if was.contains(&p) {
                (" dep__cell--was", format!("Replaced {x},{y}"))
            } else {
                ("", format!("{x},{y}"))
            }
        },
    )
}

fn scene_row(swatch: &str, title: &str, parcels: usize, size: Option<u64>, fate: &str) -> String {
    let size = size
        .map(|b| format!(" \u{b7} {}", deploy::human_size(b)))
        .unwrap_or_default();
    format!(
        "<div class=\"lay__srow\"><i class=\"lay__swatch {swatch}\"></i><span class=\"lay__srow-t\"><span class=\"lay__srow-n\">{}</span><code class=\"lay__srow-c\">{parcels} parcel{}{size} \u{b7} {fate}</code></span></div>",
        esc(title),
        plural(parcels),
    )
}

/// The multiscene pane: what already lives in this world beside the upload,
/// the after-map, and what the world's scenes hold in bytes — said only from
/// sizes the server actually reported.
fn multiscene_pane(dest: &Dest, status: &LiveStatus) -> String {
    let Some(w) = dest.world.as_deref() else {
        return empty_col(
            "Worlds only",
            "Multi-scene publishes stack scenes inside one world. This scene targets Genesis City, where every parcel set is its own deployment.",
        );
    };
    let state = match &status.remote {
        Remote::Known(state) => state,
        Remote::Empty => return empty_col("No scenes published", "The selected World has no published scenes yet."),
        Remote::Unknown(why) | Remote::Unreachable(why) => return empty_col(
            "World layout unavailable", &format!("Could not check this World's published scenes: {}. Review the layout before publishing.", esc(why))),
    };
    let (ours, base) = footprint(dest);
    let was = |title: &str, parcels, size| {
        scene_row(
            "dep__cell--was",
            title,
            parcels,
            size,
            "replaced by this publish",
        )
    };
    let rows: String = state
        .others
        .iter()
        .map(|s| match scene_overlaps(&s.coords, &ours) {
            true => was(&s.title, s.parcels, s.size),
            false => scene_row("dep__cell--kept", &s.title, s.parcels, s.size, "kept"),
        })
        .collect();
    let replaced_row = state
        .current
        .as_ref()
        .map(|c| was(&c.title, c.parcels, c.size))
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
    format!(
        "<div class=\"jn2__noterow\"><span class=\"jn__hint\">Publishing to {} replaces every scene that overlaps this footprint, including its parcels outside the footprint. Non-overlapping scenes stay published.</span></div>\n        <div class=\"jn2__body\">{}\n        {}</div>",
        esc(w),
        col("After this publish", &after_map(state, &ours, base)),
        col(
            "In this world now",
            &format!(r#"<div class="lay__srows">{rows}{replaced_row}</div>{storage}"#),
        ),
    )
}

/// `/target` — the destination detail that used to crowd the publish card.
async fn target_page(st: &Arc<AppState>, headers: &HeaderMap, local: bool) -> Response {
    scene_page(
        st,
        headers,
        "target",
        "No scene is loaded, so there is nowhere to publish to.",
        |d| {
            let history = history_rows(st, &d.project.root);
            let connect = connect_slot(st).clone();
            format!(
                r#"<main class="dash"><section id="target" class="sec">{remote}{card}{warm}</section></main><script>{script}</script>"#,
                remote = remote_notice(
                    local,
                    "tgt__remote",
                    if st.allow_remote_deploy {
                        "choosing a destination only works from there"
                    } else {
                        "choosing a destination and connecting an account only work from there"
                    },
                ),
                warm = warming_marker(d.warming),
                card = target_card(
                    d.scene_title,
                    d.prefix,
                    token(st),
                    d.dest,
                    d.p,
                    &d.status,
                    d.rights.as_deref(),
                    connect.as_ref(),
                    &history
                ),
                script = TARGET_SCRIPT,
            )
        },
    )
    .await
}

/// Rows past [`LISTED`] fold instead of vanishing — but a click is still a
/// page, so past this many even the fold ends in a sum.
const LISTED_EXPANDED: usize = 400;

fn payload_drawer(p: &deploy::DeployPreview) -> String {
    // A size the walk could not read is said so, in the size cell: `prepare`
    // reads every file it uploads, so that file stops the deploy after signing.
    let row = |(rel, len): &(String, Option<u64>)| {
        format!(
            r#"<div class="kv"><span class="k k--file">{}</span><span class="sz">{}</span></div>"#,
            esc(rel),
            match *len {
                Some(n) => deploy::human_size(n),
                None => "Size unreadable".to_string(),
            }
        )
    };
    let listed: String = p.files.iter().take(LISTED).map(row).collect();
    let rest = p.files.len().saturating_sub(LISTED);
    let rest_fold = match rest {
        0 => String::new(),
        n => {
            let beyond_listed = || p.files.iter().skip(LISTED);
            let rest_size = deploy::human_size(beyond_listed().filter_map(|(_, len)| *len).sum());
            let rows: String = beyond_listed().take(LISTED_EXPANDED).map(row).collect();
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
        s = plural(p.files.len()),
        mdot = "\u{b7}",
    )
}

#[cfg(test)]
#[path = "deploy_page_tests.rs"]
mod tests;
