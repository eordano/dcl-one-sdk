//! `/deploy` — what publishing this scene would do, and the button that does it.
//!
//! The page answers the questions you would otherwise only learn *after* the
//! wallet prompt — where it goes, how much is going, what is being left out —
//! and then publishes, so the answer and the action are not two programs.
//!
//! Publishing is the only thing this server does that changes the world, and
//! the port is unauthenticated and bound to every interface. Three things gate
//! it, and all three must hold:
//!
//! 1. the POST comes from a **loopback peer** (`--allow-remote-deploy` opts
//!    out, for someone driving the preview from another machine on purpose);
//! 2. it carries this **process's token**, which appears nowhere but inside the
//!    page body — a page you have open somewhere else can read neither the
//!    token nor originate from 127.0.0.1, so it can forge neither half;
//! 3. the payload still **fingerprints the same** as the one the page drew.
//!
//! The third is not about attackers. The preview server watches the scene and
//! rebuilds while you are reading this page, so without it the payload you
//! approved and the bytes that go up are only incidentally the same thing.

use super::landing::{deploy_target, document, esc, kv};
use super::{forwarded_prefix, AppState};
use crate::deploy::{self, MainBundle};
use crate::scene::Project;
use axum::extract::{ConnectInfo, Form, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

pub(super) async fn route(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    page(&st, &headers, peer.ip().is_loopback()).await
}

/// What to prefill the content-server field with: the flag `deploy_target`
/// already decided this scene needs, so a world arrives with its worlds server
/// filled in and Genesis City arrives empty (the rotation picks a healthy one).
fn suggested_target(flags: &str, default_target: Option<&str>) -> String {
    if let Some(t) = flags.trim().strip_prefix("--target-content ") {
        return t.to_string();
    }
    default_target.unwrap_or_default().to_string()
}

/// The three shapes this page needs that the shared sheet has no rule for: a
/// row of datums, the gap that keeps two readings inside one datum from
/// running together, and a file list whose paths take the width while the
/// sizes align on a column of their own. Tokens only — no colour of its own,
/// no size off the scale, no case change — so the page cannot drift into
/// looking like a different server than `/`.
const PAGE_CSS: &str = "
#deploy { gap: var(--s-6); }
.dt { display: flex; flex-wrap: wrap; gap: var(--s-4) var(--s-10); }
.dt .page__sub { color: var(--text); overflow-wrap: anywhere; }
.panel > .cmd { align-self: stretch; max-width: var(--measure); }
.datum__unit + .datum__num { margin-left: var(--s-3); }
.files .kv { grid-template-columns: minmax(0, 1fr) 8.5rem; }
.files .k--file, .files .sz { font-size: var(--fs-13); }
.files .sz {
  color: var(--ink-6); font-variant-numeric: tabular-nums;
  text-align: right; white-space: nowrap;
}
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

static PREVIEW_CACHE: Mutex<Vec<(PathBuf, Instant, Arc<PreviewResult>)>> = Mutex::new(Vec::new());

fn cache() -> std::sync::MutexGuard<'static, Vec<(PathBuf, Instant, Arc<PreviewResult>)>> {
    PREVIEW_CACHE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Walks the scene off the async worker, at most once per [`PREVIEW_CACHE_TTL`]
/// per scene.
async fn cached_preview(project: &Project) -> Arc<PreviewResult> {
    let root = project.root.clone();
    let hit = cache()
        .iter()
        .find(|(p, at, _)| *p == root && at.elapsed() < PREVIEW_CACHE_TTL)
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
    let mut c = cache();
    c.retain(|(p, at, _)| *p != root && at.elapsed() < PREVIEW_CACHE_TTL);
    c.push((root, Instant::now(), entry.clone()));
    entry
}

/// The run in flight, if any, and the token that authorises starting one.
/// Process-global rather than `AppState` state because a deploy outlives the
/// request that began it and there is only ever one preview server per process.
static DEPLOY: Mutex<Option<Run>> = Mutex::new(None);
static TOKEN: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Minted once per process and rendered only into the page body. An attacker
/// page cannot read it (same-origin) and cannot guess it (128 random bits), so
/// together with the loopback check it is what keeps `POST /deploy` from being
/// a publish button for anything that can reach the port.
fn token() -> &'static str {
    TOKEN.get_or_init(|| {
        let bytes: [u8; 16] = rand::random();
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    })
}

fn runs() -> std::sync::MutexGuard<'static, Option<Run>> {
    DEPLOY.lock().unwrap_or_else(PoisonError::into_inner)
}

struct Run {
    /// Which claim this is. The completion path presents it before writing a
    /// terminal state, so a slow deploy cannot stamp its outcome onto a run
    /// that replaced it.
    id: u64,
    started: Instant,
    target: String,
    state: RunState,
}

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
    target_content: String,
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

/// `POST /deploy` — the only request this server answers that changes the
/// world. Always replies with a redirect, so the browser lands on the status
/// page whether the run started or was refused: a POST that rendered its own
/// body would leave a resubmit-on-refresh trap on a route that publishes.
///
/// Five gates, in order, every one of which a review found a way past before it
/// was written this way:
///
/// 1. **Loopback, and not merely a loopback socket.** The tunnel agent replays
///    trunk requests through `http://127.0.0.1:{port}`, so a POST from the
///    public internet arrives with a loopback peer; it stamps
///    [`crate::tunnel::FORWARDED_HEADER`] so this gate can tell them apart.
/// 2. **Same origin, then the token.** The token alone is not enough: while the
///    permissive CORS layer covered this route, any page could `fetch` the HTML
///    and read both the token and the fingerprint out of it. CORS is scoped off
///    `/deploy` now, and `Origin`/`Sec-Fetch-Site` are the belt to that brace.
/// 3. **A human signs.** With `DCL_PRIVATE_KEY` exported the deploy signs
///    headlessly, which turns one click into an unattended publish and makes
///    the page's own promise of a wallet prompt false. Refused instead.
/// 4. **One run, claimed in the lock that checks.** Two accepted POSTs would
///    run two builds over one `bin/`, and each hashes those files to sign them:
///    one can publish an entity assembled from bytes the other was mid-write.
/// 5. **The payload the page drew.** Compared on the digest, so an absent,
///    empty or wrong fingerprint all refuse alike.
///
/// Two more decisions live in the body and are load-bearing: `multi_scene: true`
/// (the alternative silently deletes a world's other scenes, from a list the
/// page never showed), and the inner [`tokio::spawn`] with its `JoinHandle`
/// awaited, so a panic inside `deploy` reaches a terminal state instead of
/// wedging the button as `Running` for the life of the process.
pub(super) async fn start(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(form): Form<DeployForm>,
) -> Response {
    let prefix = forwarded_prefix(&headers);
    let back = Redirect::to(&format!("{prefix}/deploy"));
    let refuse = |why: &str| (StatusCode::FORBIDDEN, format!("{why}\n")).into_response();

    let forwarded = headers.contains_key(crate::tunnel::FORWARDED_HEADER);
    if !st.allow_remote_deploy && (forwarded || !peer.ip().is_loopback()) {
        return refuse(
            "publishing runs on the machine hosting this preview; start it with --allow-remote-deploy to publish from elsewhere",
        );
    }

    if let Some(site) = headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()) {
        if site != "same-origin" && site != "none" {
            return refuse("this deploy did not come from the preview's own page");
        }
    }
    if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        let host = headers
            .get(header::HOST)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        if !origin.ends_with(host) || host.is_empty() {
            return refuse("this deploy did not come from the preview's own page");
        }
    }
    if form.token != token() {
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
    let target_content = match validated_target(form.target_content.trim()) {
        Ok(t) => t,
        Err(why) => return refuse(&why),
    };

    let Some(id) = claim() else {
        return back.into_response();
    };
    let root = project.root.clone();
    let owned = project.clone();
    let fresh = match tokio::task::spawn_blocking(move || deploy::preview(&owned)).await {
        Ok(Ok(p)) => p,
        _ => {
            finish(id, RunState::Failed("the scene could not be read".into()));
            return back.into_response();
        }
    };

    let (root_fp, fresh_fp) = (&root, &fresh);
    if form.fingerprint.is_empty() || fingerprint(root_fp, fresh_fp) != form.fingerprint {
        finish(id, RunState::Stale(moved_since(root_fp, fresh_fp)));
        return back.into_response();
    }

    let (shown, _) = deploy_target(&project.scene_json, deploy::env_default_target().as_deref());
    if let Some(r) = runs().as_mut() {
        r.target = target_content.clone().unwrap_or(shown);
    }
    let opts = deploy::DeployOptions {
        dir: root.clone(),
        target: None,
        target_content,
        sign_key: None,
        skip_build: false,
        dry_run: st.deploy_dry_run,
        timestamp: None,
        entity_out: None,
        multi_scene: true,
        yes: true,
        no_browser: false,
        ci: false,
        port: None,
    };
    tokio::spawn(async move {
        let handle = tokio::spawn(async move { deploy::deploy(&opts).await });
        let state = match handle.await {
            Ok(Ok(())) => RunState::Done,
            Ok(Err(e)) => RunState::Failed(scrub_paths(&format!("{e:#}"), &root)),
            Err(_) => RunState::Failed("the deploy did not finish".into()),
        };
        finish(id, state);
    });
    back.into_response()
}

/// A content server the operator typed. Unvalidated, this string is the base
/// every file and the signed entity are POSTed to, so it chooses where the
/// signature goes — and reaches internal addresses a browser could not.
fn validated_target(raw: &str) -> Result<Option<String>, String> {
    if raw.is_empty() {
        return Ok(None);
    }
    let url = url::Url::parse(raw).map_err(|_| format!("{raw:?} is not a url"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(format!(
            "a content server must be http or https, not {:?}",
            url.scheme()
        ));
    }
    let host = url.host_str().unwrap_or_default().to_string();
    if host.is_empty() {
        return Err("a content server needs a host".to_string());
    }
    let internal = host == "localhost"
        || host == "169.254.169.254"
        || host
            .parse::<std::net::IpAddr>()
            .map(|ip| ip.is_loopback() || ip.is_unspecified())
            .unwrap_or(false);
    if internal {
        return Err(format!(
            "{host} is not a content server this can publish to"
        ));
    }
    Ok(Some(url.as_str().trim_end_matches('/').to_string()))
}

/// Check and claim in one acquisition, returning the id the completion path
/// must present before it may write a terminal state.
fn claim() -> Option<u64> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let mut slot = runs();
    if matches!(slot.as_ref().map(|r| &r.state), Some(RunState::Running)) {
        return None;
    }
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    *slot = Some(Run {
        id,
        started: Instant::now(),
        target: String::new(),
        state: RunState::Running,
    });
    Some(id)
}

/// Write a terminal state, but only onto the run that claimed the slot: a
/// finishing deploy must not stamp `Done` over a later run's `Running`, nor
/// over the refusal that replaced it.
fn finish(id: u64, state: RunState) {
    if let Some(r) = runs().as_mut() {
        if r.id == id {
            r.state = state;
        }
    }
}

/// The status of the last run. While one is in flight it carries a `meta`
/// refresh, so the page follows the deploy without a line of JavaScript; it
/// sits in the body because `document()` owns the head for every page, and
/// browsers honour it either way.
fn run_panel() -> String {
    let run = runs();
    let Some(r) = run.as_ref() else {
        return String::new();
    };
    let (title, body, refresh, tone) = match &r.state {
        RunState::Running => (
            "Publishing",
            format!(
                "Running for {}s. Your wallet opens in a new tab to sign; the terminal has the step by step.",
                r.started.elapsed().as_secs()
            ),
            r#"<meta http-equiv="refresh" content="2">"#,
            "",
        ),
        RunState::Done => (
            "Published",
            format!("Uploaded to {}.", esc(&r.target)),
            "",
            "",
        ),
        RunState::Failed(why) => ("Deploy failed", esc(why), "", " panel--warn"),
        RunState::Stale(paths) => (
            "Nothing was published",
            format!(
                "The scene changed while this page was open, so the payload you saw is not the one that would have gone up: {}. Check the numbers below and press the button again.",
                esc(&paths.join(", "))
            ),
            "",
            " panel--warn",
        ),
    };
    format!(
        r#"{refresh}<div class="panel{tone}"><h2 class="dt__label">{title}</h2>
          <p class="dt__note">{body}</p></div>"#
    )
}

/// The publish control. Disabled, with the reason said out loud, for a caller
/// whose POST would be refused anyway.
fn deploy_form(prefix: &str, suggested: &str, print: &str, blocked: Option<&str>) -> String {
    if let Some(why) = blocked {
        return format!(
            r#"<div class="panel"><h2 class="dt__label">Publish</h2><p class="dt__note">{}</p></div>"#,
            esc(why)
        );
    }
    format!(
        r#"<form class="panel" method="post" action="{prefix_esc}/deploy">
          <h2 class="dt__label">Publish</h2>
          <input type="hidden" name="token" value="{tok}">
          <input type="hidden" name="fingerprint" value="{print_esc}">
          <label class="fieldlabel" for="target_content">Content server</label>
          <input class="knob__text" type="text" id="target_content" name="target_content"
                 value="{target}" autocomplete="off" spellcheck="false">
          <p class="dt__note">Builds, packs and signs, then uploads. Your wallet opens in a new
            tab; nothing leaves this machine until it answers.</p>
          <button class="btn btn--primary" type="submit">Deploy this scene</button>
        </form>"#,
        prefix_esc = esc(prefix),
        tok = esc(token()),
        print_esc = esc(print),
        target = esc(suggested),
    )
}

async fn page(st: &AppState, headers: &HeaderMap, local: bool) -> Response {
    let prefix = forwarded_prefix(headers);
    let Some(project) = st.first_project() else {
        return super::landing::html(document(
            "deploy",
            &prefix,
            PAGE_CSS,
            "#deploy",
            "Skip to deploy",
            ("/", "Back to the preview"),
            r#"<main class="dash"><section id="deploy" class="sec">
              <div class="sec__head"><h1 class="page__title">Deploy</h1></div>
              <div class="panel">
              <span class="note">No scene is loaded, so there is nothing to publish.</span>
            </div></section></main>"#,
        ));
    };
    let scene_title = crate::joinblock::scene_title(&project.scene_json);
    let default_target = deploy::env_default_target();
    let (destination, flags) = deploy_target(&project.scene_json, default_target.as_deref());
    let command = format!("dcl-one-sdk deploy{flags}");

    let blocked = match st.allow_remote_deploy || local {
        true => None,
        false => Some(
            "Publishing runs on the machine hosting this preview, because signing opens its wallet. \
             Open this page there, or start the preview with --allow-remote-deploy.",
        ),
    };
    let body = match &*cached_preview(&project).await {
        Ok(p) => format!(
            "{}{}{}",
            run_panel(),
            rendered(&scene_title, &destination, &command, p),
            deploy_form(
                &prefix,
                &suggested_target(&flags, default_target.as_deref()),
                &fingerprint(&project.root, p),
                blocked,
            ),
        ),
        Err(e) => format!(
            r#"<main class="dash"><section id="deploy" class="sec">{head}
              <div class="panel"><span class="note">This scene cannot be
              packaged yet: {why}</span></div></section></main>"#,
            head = head(&scene_title),
            why = esc(&scrub_paths(e, &project.root))
        ),
    };
    super::landing::html(document(
        &format!("deploy {scene_title}"),
        &prefix,
        PAGE_CSS,
        "#deploy",
        "Skip to deploy",
        ("/", "Back to the preview"),
        &body,
    ))
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

/// The page states its subject the way the landing page states its own: the
/// scene's name at full size, one line under it, and nothing above it. The
/// name is here rather than in a `scene` row because a row would say it twice.
fn head(title: &str) -> String {
    format!(
        r#"<div class="sec__head"><h1 class="page__title">{}</h1>
      <span class="page__sub">What publishing this scene would do, before you run it</span></div>"#,
        esc(title)
    )
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

fn rendered(title: &str, destination: &str, command: &str, p: &deploy::DeployPreview) -> String {
    let listed: String = p
        .files
        .iter()
        .take(LISTED)
        .map(|(rel, len)| {
            format!(
                r#"<div class="kv"><span class="k k--file">{}</span>{}</div>"#,
                esc(rel),
                size_cell(*len)
            )
        })
        .collect();
    let rest = p.files.len().saturating_sub(LISTED);
    let rest_row = match rest {
        0 => String::new(),
        n => kv(
            &format!("and {n} more"),
            size_cell(Some(
                p.files
                    .iter()
                    .skip(LISTED)
                    .filter_map(|(_, len)| *len)
                    .sum(),
            )),
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
    let ignored = match p.ignored.len() {
        0 => "Nothing — no file in the folders being published matches .dclignore".to_string(),
        n => {
            let named: Vec<&str> = p.ignored.iter().take(4).map(String::as_str).collect();
            let tail = match n > named.len() {
                true => format!(" and {} more", n - named.len()),
                false => String::new(),
            };
            format!(
                "{n} file{} by .dclignore: {}{tail}",
                if n == 1 { "" } else { "s" },
                named.join(", ")
            )
        }
    };
    let alarms = format!("{main}{oversize}{unreadable}{collisions}");
    let (size_num, size_unit) = split_size(p.total_bytes);
    format!(
        r##"<main class="dash">
  <section id="deploy" class="sec">
    {head}
    <div class="dt">
      <div class="datum"><span class="datum__cap">Publishes to</span>
        <div class="datum__v"><span class="page__sub">{destination_esc}</span></div></div>
      <div class="datum"><span class="datum__cap">Upload</span>
        <div class="datum__v"><span class="datum__num">{files}</span><span
          class="datum__unit">file{files_plural}</span><span
          class="datum__num">{size_num}</span><span class="datum__unit">{size_unit}</span></div></div>
    </div>
    {alarms}
    <div class="panel">
      <h2>Run it</h2>
      <code class="cmd cmd--hero">{command_esc}</code>
      <span class="note">Click the line to select all of it. Run it from the scene folder —
        deploy publishes the current directory. Signing opens your wallet, so it runs where
        you are: the command starts a signing page of its own on a throwaway port and shuts
        it down once the wallet answers. This server has no publish route.</span>
      <span class="note">Add <code>--dry-run</code> to pack and hash the entity without
        touching the network.</span>
    </div>
  </section>
  <section class="sec">
    <div class="panel">
      <h2>Payload</h2>
      <div class="kvs files">{listed}{rest_row}</div>
      <div class="datum"><span class="datum__cap">Left out</span>
        <span class="note">{ignored_esc}</span></div>
      <span class="note">The entity id is minted when you run the command, not here: the id
        hashes the entity including its timestamp, so any value printed on this page would be
        a different one from the id you get. <code>--dry-run</code> prints it.</span>
    </div>
  </section>
</main>"##,
        head = head(title),
        command_esc = esc(command),
        destination_esc = esc(destination),
        ignored_esc = esc(&ignored),
        files = p.files.len(),
        files_plural = if p.files.len() == 1 { "" } else { "s" },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::{header, StatusCode};
    use serde_json::json;
    use std::collections::{HashMap, VecDeque};
    use std::sync::OnceLock;
    use tokio::sync::broadcast;

    /// A real directory with real files, because the property being tested is
    /// about what a rendered payload list says: the previous version of this
    /// test pointed at a path that did not exist, so nothing was listed and
    /// `!contains("somebody")` passed without proving anything.
    struct Tree(std::path::PathBuf);

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn scene(tag: &str, mut scene_json: serde_json::Value) -> (Tree, Project) {
        let base = std::env::temp_dir().join(format!(
            "dcl-one-sdk-deploy-page-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("somebody-scenes").join("gather");
        if scene_json.get("main").is_none() {
            scene_json["main"] = json!("bin/index.js");
        }
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::write(root.join("bin/index.js"), "module.exports={}").unwrap();
        std::fs::write(root.join("scene.json"), scene_json.to_string()).unwrap();
        std::fs::write(root.join("assets/model.glb"), vec![7u8; 2048]).unwrap();
        std::fs::write(root.join("README.md"), "notes").unwrap();
        let project = Project {
            root,
            scene_json: scene_json.clone(),
        };
        (Tree(base), project)
    }

    /// `start::mod`'s `AppState` is private to `start`, and this module is one
    /// of its children, so the page can be driven through its real route
    /// without touching the file that owns the type.
    /// The loopback gate, which the router tests cannot reach: their client is
    /// always 127.0.0.1, and this machine's firewall refuses a real inbound
    /// connection to its own LAN address, so the only honest way to put a
    /// non-loopback peer in front of the handler is to hand it one.
    #[tokio::test]
    async fn a_peer_off_this_machine_cannot_publish() {
        let (_tree, project) = scene("remote", json!({ "display": { "title": "Gather" } }));
        let st = state(project);
        let lan: SocketAddr = ([192, 168, 1, 9], 51000).into();

        let refused = start(
            State(st.clone()),
            ConnectInfo(lan),
            HeaderMap::new(),
            Form(DeployForm {
                token: token().to_string(),
                target_content: String::new(),
                fingerprint: String::new(),
            }),
        )
        .await;
        assert_eq!(
            refused.status(),
            StatusCode::FORBIDDEN,
            "a correct token from off-machine is still not allowed to publish"
        );

        let page = page(&st, &HeaderMap::new(), false).await;
        let body = axum::body::to_bytes(page.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(
            !body.contains(r#"method="post""#),
            "no publish button for a remote reader"
        );
        assert!(
            body.contains("Publishing runs on the machine"),
            "{body:.400}"
        );
    }

    fn state(project: Project) -> Arc<AppState> {
        let (reload_tx, _) = broadcast::channel(4);
        Arc::new(AppState {
            projects: std::sync::RwLock::new(vec![project]),
            allow_remote_deploy: false,
            deploy_dry_run: true,
            machine: "test-machine".to_string(),
            reload_tx,
            offline_comms: true,
            port: 0,
            data_layer: None,
            entity_cache: Mutex::new(HashMap::new()),
            optimized_assets_url: OnceLock::new(),
            local_ab: true,
            deep_link_extra: String::new(),
            mcp: true,
            mcp_port: crate::joinblock::DEFAULT_EXPLORER_MCP_PORT,
            explorer_params: Vec::new(),
            recent_requests: Mutex::new(VecDeque::new()),
        })
    }

    /// The preview cache is process-wide, so a test that wants a fresh walk
    /// drops its own entry and leaves the rest alone.
    fn forget(st: &Arc<AppState>) {
        let root = st.projects()[0].root.clone();
        cache().retain(|(p, _, _)| *p != root);
    }

    /// Everything a visitor of `/deploy` actually receives, fetched the way
    /// they fetch it. Tests that rebuild the page's strings for themselves
    /// prove nothing about the page: this suite used to do exactly that, and
    /// stayed green while `page` printed the absolute path of the scene.
    async fn served(st: &Arc<AppState>) -> String {
        forget(st);
        let resp = route(
            State(st.clone()),
            ConnectInfo(([127, 0, 0, 1], 0).into()),
            HeaderMap::new(),
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
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    /// This page is served to anything that can reach the port, so the one
    /// thing it must never answer is where the scene lives on disk.
    #[tokio::test]
    async fn the_deploy_page_never_names_the_scene_directory() {
        let (dir, project) = scene("names", json!({ "display": { "title": "Gather" } }));
        let root = project.root.clone();
        let html = served(&state(project)).await;
        assert!(html.contains("dcl-one-sdk deploy"));
        assert!(html.contains("assets/model.glb"), "the payload is listed");
        assert!(html.contains("Publishes to"), "{html}");
        assert!(!html.contains("--dir"), "{html}");
        assert!(!html.contains("somebody"), "{html}");
        assert!(!html.contains(&dir.0.display().to_string()));
        assert!(!html.contains(&root.display().to_string()));
        assert!(html.contains("Run it from the scene folder"));
    }

    /// The answer someone came to this page for is a reading, not a row in a
    /// label/value table, and the scene's name is the page's title rather than
    /// a `scene` row restating what the title already says.
    #[tokio::test]
    async fn the_headline_numbers_are_a_datum_and_the_scene_is_the_title() {
        let (_dir, project) = scene("datum", json!({ "display": { "title": "Gather" } }));
        let html = served(&state(project)).await;
        assert!(
            html.contains(r#"<h1 class="page__title">Gather</h1>"#),
            "the scene names the page, and is its <h1>: {html}"
        );
        assert!(
            !html.contains(r#"class="u-sr-only""#),
            "no heading on this page is hidden from the screen: {html}"
        );
        assert!(
            !html.contains(r#"<span class="k">scene</span>"#),
            "and does not say so a second time in a row of its own: {html}"
        );
        assert!(
            html.contains(r#"<span class="datum__cap">Upload</span>"#)
                && html.contains(r#"<span class="datum__num">"#),
            "the file count and the size are a reading, not a table cell: {html}"
        );
        assert!(
            html.contains(r#"<div class="kvs files">"#),
            "the payload is a list of paths and sizes: {html}"
        );
        assert!(html.contains("<h2>Run it</h2>"), "{html}");
        assert!(
            html.contains(r#"<code class="cmd cmd--hero">"#),
            "the command is the hero of its card: {html}"
        );
        assert!(
            !html.contains(r#"<div class="grid">"#),
            "the entity-id card is a footnote now, so the payload takes the \
             full measure: {html}"
        );
    }

    /// This page adds layout the shared sheet has no rule for, and that is the
    /// whole licence it has: the moment it names a colour, a case or a
    /// tracking of its own, `/deploy` starts looking like a different server
    /// than `/`.
    #[test]
    fn the_page_local_css_adds_layout_and_never_a_second_palette() {
        for banned in [
            "text-transform",
            "letter-spacing",
            "font-weight",
            "font-family",
            ": #",
            "rgb",
            "opacity",
        ] {
            assert!(
                !PAGE_CSS.contains(banned),
                "page-local css must not carry `{banned}`: {PAGE_CSS}"
            );
        }
        assert!(
            PAGE_CSS.contains("var(--ink-6)"),
            "every colour it does set comes from the shared tokens: {PAGE_CSS}"
        );
    }

    /// A world needs `--target-content` or the command the page prints is one
    /// that refuses to run. `None` is passed for the default target on
    /// purpose: reading the environment here would make this test say
    /// something different on a machine that exports
    /// `DCL_ONE_SDK_DEFAULT_TARGET`, which the README tells people to do.
    #[tokio::test]
    async fn the_command_carries_the_flags_the_target_needs() {
        let (_dir, project) = scene(
            "world",
            json!({ "worldConfiguration": { "name": "my.dcl.eth" } }),
        );
        let (destination, flags) = deploy_target(&project.scene_json, None);
        assert!(flags.contains("--target-content"), "{flags}");
        assert!(destination.contains("my.dcl.eth"), "{destination}");
        let command = format!("dcl-one-sdk deploy{flags}");
        let p = deploy::preview(&project).unwrap();
        let html = rendered("Gather", &destination, &command, &p);
        assert!(html.contains(&esc(&command)), "{html}");
        assert!(html.contains(&esc(&destination)), "{html}");
    }

    /// The count that matters is what `.dclignore` removed from a directory
    /// that IS published — not the seventeen thousand files under a
    /// node_modules the walk never enters.
    #[tokio::test]
    async fn ignored_names_the_files_you_excluded_not_the_tree_you_never_ship() {
        let (_dir, project) = scene("ignored", json!({ "display": { "title": "Gather" } }));
        let modules = project.root.join("node_modules/pkg");
        std::fs::create_dir_all(&modules).unwrap();
        for i in 0..50 {
            std::fs::write(modules.join(format!("f{i}.js")), "x").unwrap();
        }
        let p = deploy::preview(&project).expect("preview");
        assert_eq!(p.ignored, ["README.md"], "node_modules is not enumerated");
        let html = served(&state(project)).await;
        assert!(html.contains("1 file by .dclignore: README.md"), "{html}");
        assert!(!html.contains("node_modules"));
    }

    /// Nothing is left out, and saying so has to mean something: the old copy
    /// read "every file beside the published ones is published".
    #[tokio::test]
    async fn nothing_left_out_is_said_without_going_in_a_circle() {
        let (_dir, project) = scene("nothing", json!({ "display": { "title": "Gather" } }));
        std::fs::remove_file(project.root.join("README.md")).unwrap();
        let html = served(&state(project)).await;
        assert!(
            html.contains("Nothing — no file in the folders being published matches .dclignore"),
            "{html}"
        );
        assert!(
            !html.contains("every file beside the published ones"),
            "{html}"
        );
    }

    /// Sizes come from the directory entry, so the totals have to be real
    /// without the page ever reading the bytes.
    #[test]
    fn the_payload_totals_the_files_it_would_upload() {
        let (_dir, project) = scene("totals", json!({ "display": { "title": "Gather" } }));
        let p = deploy::preview(&project).expect("preview");
        let listed: u64 = p.files.iter().filter_map(|(_, len)| *len).sum();
        assert_eq!(listed, p.total_bytes);
        assert!(p
            .files
            .iter()
            .any(|(rel, len)| rel == "assets/model.glb" && *len == Some(2048)));
        assert!(p.oversize.is_empty());
        assert!(p.unreadable.is_empty());
        assert_eq!(
            p.files.first().map(|(rel, _)| rel.as_str()),
            Some("assets/model.glb"),
            "largest first"
        );
    }

    /// The truncated tail is a byte sum of the files that are NOT on the page,
    /// which is the only number that makes the panel add up.
    #[tokio::test]
    async fn a_long_payload_lists_eight_and_sums_the_rest() {
        let (_dir, project) = scene("truncate", json!({ "display": { "title": "Gather" } }));
        for i in 1..=12u64 {
            std::fs::write(
                project.root.join(format!("assets/a{i:02}.glb")),
                vec![0u8; (13 - i) as usize * 1000],
            )
            .unwrap();
        }
        let p = deploy::preview(&project).unwrap();
        let unlisted: u64 = p
            .files
            .iter()
            .skip(LISTED)
            .filter_map(|(_, len)| *len)
            .sum();
        assert_eq!(
            p.files.len(),
            15,
            "12 assets + model.glb + scene.json + bundle"
        );
        let html = served(&state(project)).await;
        assert_eq!(
            html.matches(r#"class="k k--file""#).count(),
            LISTED,
            "only the first {LISTED} rows are listed"
        );
        assert!(html.contains("and 7 more"), "{html}");
        assert!(
            html.contains(&format!(
                r#"<span class="k">and 7 more</span><span class="sz">{}</span>"#,
                deploy::human_size(unlisted)
            )),
            "the tail sums the files it did not list ({unlisted} bytes): {html}"
        );
        assert!(!html.contains("a09.glb"), "the ninth file is not a row");
    }

    /// A file over the per-file limit is the deploy failing, and the panel
    /// saying so has never rendered until this fixture existed.
    #[tokio::test]
    async fn an_oversize_file_gets_a_warning_before_the_wallet() {
        let (_dir, project) = scene("oversize", json!({ "display": { "title": "Gather" } }));
        let big = project.root.join("assets/huge.glb");
        std::fs::File::create(&big)
            .unwrap()
            .set_len(50_000_001)
            .unwrap();
        let p = deploy::preview(&project).unwrap();
        assert_eq!(p.oversize, ["assets/huge.glb"]);
        let html = served(&state(project)).await;
        assert!(html.contains(r#"class="panel panel--warn""#), "{html}");
        assert!(html.contains("Over the per-file limit"), "{html}");
        assert!(html.contains("assets/huge.glb"), "{html}");
        assert!(html.contains("50.0 MB"), "{html}");
    }

    /// A publishable file whose size cannot be read is the deploy stopping
    /// after the wallet has signed. Reporting it as 0 bytes made the page say
    /// the deploy was fine.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_file_that_cannot_be_read_is_not_called_zero_bytes() {
        let (_dir, project) = scene("dangling", json!({ "display": { "title": "Gather" } }));
        std::os::unix::fs::symlink(
            project.root.join("assets/gone.bin"),
            project.root.join("assets/dangling.glb"),
        )
        .unwrap();
        let p = deploy::preview(&project).unwrap();
        assert_eq!(p.unreadable, ["assets/dangling.glb"]);
        assert!(p
            .files
            .iter()
            .any(|(rel, len)| rel == "assets/dangling.glb" && len.is_none()));
        let known: u64 = ["scene.json", "assets/model.glb", "bin/index.js"]
            .iter()
            .map(|r| std::fs::metadata(project.root.join(r)).unwrap().len())
            .sum();
        assert_eq!(p.total_bytes, known, "an unknown size adds nothing");
        let html = served(&state(project)).await;
        assert!(html.contains("Size unreadable"), "{html}");
        assert!(html.contains("Cannot be read"), "{html}");
        assert!(
            !html.contains(r#"dangling.glb</span><span class="sz">0 bytes"#),
            "{html}"
        );
    }

    /// "You have not built yet" is the most likely reason a real deploy fails,
    /// and `prepare` refuses it — after the wallet prompt.
    #[tokio::test]
    async fn a_scene_that_was_never_built_says_so() {
        let (_dir, project) = scene("unbuilt", json!({ "display": { "title": "Gather" } }));
        std::fs::remove_file(project.root.join("bin/index.js")).unwrap();
        let p = deploy::preview(&project).unwrap();
        assert_eq!(p.main, MainBundle::Missing("bin/index.js".to_string()));
        let html = served(&state(project)).await;
        assert!(html.contains("Not built yet"), "{html}");
        assert!(html.contains("dcl-one-sdk build"), "{html}");
        assert!(html.contains("bin/index.js"), "{html}");
        assert!(!html.contains("somebody"), "{html}");
    }

    /// The built scene must NOT carry the alarm, or the one above proves only
    /// that the panel is always there.
    #[tokio::test]
    async fn a_built_scene_carries_no_alarm() {
        let (_dir, project) = scene("built", json!({ "display": { "title": "Gather" } }));
        let p = deploy::preview(&project).unwrap();
        assert_eq!(p.main, MainBundle::Present("bin/index.js".to_string()));
        let html = served(&state(project)).await;
        assert!(
            !html.contains(r#"class="panel panel--warn""#),
            "no warning markup, only the stylesheet rule: {html}"
        );
        assert!(!html.contains("Not built yet"), "{html}");
    }

    /// The walk is the expensive half of this route and the route is
    /// unauthenticated: the answer is reused for a moment rather than redone
    /// per request.
    #[tokio::test]
    async fn the_walk_is_reused_for_a_moment() {
        let (_dir, project) = scene("cache", json!({ "display": { "title": "Gather" } }));
        let st = state(project.clone());
        let first = served(&st).await;
        std::fs::write(project.root.join("assets/late.glb"), vec![1u8; 4096]).unwrap();
        let resp = route(
            State(st.clone()),
            ConnectInfo(([127, 0, 0, 1], 0).into()),
            HeaderMap::new(),
        )
        .await;
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let cached = String::from_utf8(bytes.to_vec()).unwrap();
        assert_eq!(cached, first, "the second request did not walk again");
        assert!(!cached.contains("late.glb"));
        let fresh = served(&st).await;
        assert!(fresh.contains("late.glb"), "an expired entry walks again");
    }

    /// The error branch is a page too, served on the same open port, and it
    /// is held to the same rule: say what is wrong, never say where.
    #[tokio::test]
    async fn the_error_page_names_the_pattern_and_not_the_path() {
        let (dir, project) = scene("badignore", json!({ "display": { "title": "Gather" } }));
        std::fs::write(project.root.join(".dclignore"), "assets/[z-a].png\n").unwrap();
        let root = project.root.clone();
        assert!(deploy::preview(&project).is_err(), "the matcher refuses it");
        let html = served(&state(project)).await;
        assert!(html.contains("This scene cannot be"), "{html}");
        assert!(html.contains("assets/[z-a].png"), "{html}");
        assert!(!html.contains("somebody"), "{html}");
        assert!(!html.contains(&root.display().to_string()), "{html}");
        assert!(!html.contains(&dir.0.display().to_string()), "{html}");
    }

    /// Whatever an error chain from further down carries, the page does not
    /// pass a filesystem path on to the visitor.
    #[test]
    fn a_path_in_an_error_chain_is_taken_back_out() {
        let root = std::path::Path::new("/home/somebody/scenes/gather");
        let scrubbed = scrub_paths(
            "reading /home/somebody/scenes/gather/assets/a.glb failed",
            root,
        );
        assert_eq!(
            scrubbed, "reading the scene folder/assets/a.glb failed",
            "{scrubbed}"
        );
        assert!(!scrub_paths("under /home/somebody/scenes/other", root).contains("somebody"));
    }
}
