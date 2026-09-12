//! Destination resolution mirrors `deploy::net::resolve_target_from`.

use super::landing::parse_parcels;
pub(super) use crate::deploy::Reuse;
use crate::deploy::{self, WORLDS_CONTENT_SERVER};
use crate::scene::Project;
use serde::de::DeserializeOwned;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

/// Where "what is live on Genesis" is read from when nothing configured a
/// server; reads are network-wide consistent, so any catalyst answers the same.
pub(super) const GENESIS_READ: &str = "https://peer.decentraland.org/content";

/// Where "who owns what" (chain state, network-wide consistent) is read from.
pub(super) const GENESIS_LAMBDAS: &str = "https://peer.decentraland.org/lambdas";

pub(super) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(super) fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

#[derive(Clone)]
pub(super) struct Dest {
    pub(super) world: Option<String>,
    pub(super) pointers: Vec<String>,
    pub(super) base_pointer: String,
    /// Status endpoints, tried in order: a raw content server answers on the
    /// bare base, a catalyst domain under `/content`.
    pub(super) read_bases: Vec<String>,
    /// The target's own lambdas: the verdict must ask the server that will
    /// actually rule on the publish.
    pub(super) lambdas_base: String,
    /// Always the public Genesis lambdas: chain facts are network-wide
    /// consistent and a self-hosted realm's squid often carries none.
    pub(super) chain_lambdas: String,
    /// The worlds service itself (the explorer-api gateway does not proxy
    /// `/worlds`): the deploy target when configured, else the public one.
    pub(super) worlds_base: String,
    pub(super) headline: String,
    pub(super) server_line: String,
}

pub(super) fn host_of(url: &str) -> String {
    deploy::host_of(url).unwrap_or_else(|| url.to_string())
}

pub(super) fn parse_coords(pointers: &[String]) -> Vec<(i64, i64)> {
    pointers
        .iter()
        .filter_map(|p| catalyrst_auth_chain::pointer::parse_pointer(p))
        .collect()
}

pub(super) fn parcel_span(parcels: &[(i64, i64)]) -> String {
    match parcels {
        [] => "No parcels declared".to_string(),
        [(x, y)] => format!("Parcel {x},{y}"),
        _ => {
            let xs = parcels.iter().map(|(x, _)| *x);
            let ys = parcels.iter().map(|(_, y)| *y);
            format!(
                "Parcels {},{}\u{2013}{},{}",
                xs.clone().min().unwrap(),
                ys.clone().min().unwrap(),
                xs.max().unwrap(),
                ys.max().unwrap()
            )
        }
    }
}

/// Pure so a test can drive it without the process environment; the page
/// passes `deploy::configured_target_server()` and
/// `deploy::configured_catalyst_rotation()` in.
pub(super) fn resolve_dest(
    scene_json: &serde_json::Value,
    default_target: Option<&str>,
    rotation: Option<Vec<String>>,
) -> Dest {
    let (parcels, base) = parse_parcels(scene_json);
    let pointers: Vec<String> = parcels.iter().map(|(x, y)| format!("{x},{y}")).collect();
    let base_pointer = format!("{},{}", base.0, base.1);
    let world = crate::joinblock::world_name(scene_json);
    let headline = match &world {
        Some(w) => format!("World {w}"),
        None => parcel_span(&parcels),
    };
    let public_worlds = WORLDS_CONTENT_SERVER.to_string();
    let (read_bases, lambdas_base, worlds_base, server_line) =
        if let Some(t) = default_target.map(str::trim).filter(|t| !t.is_empty()) {
            let base_url = deploy::sanitize_catalyst_url(t);
            let root = base_url.trim_end_matches("/content").to_string();
            let read_bases = match base_url.ends_with("/content") {
                true => vec![base_url.clone()],
                false => vec![base_url.clone(), format!("{base_url}/content")],
            };
            (
                read_bases,
                format!("{root}/lambdas"),
                root,
                format!(
                    "on {} \u{2014} DCL_ONE_SDK_TARGET_SERVER",
                    host_of(&base_url)
                ),
            )
        } else if world.is_some() {
            (
                vec![public_worlds.clone()],
                GENESIS_LAMBDAS.to_string(),
                public_worlds,
                format!("on {}", host_of(WORLDS_CONTENT_SERVER)),
            )
        } else {
            match rotation.as_deref().and_then(<[String]>::first) {
                Some(b) => (
                    vec![format!("{b}/content"), b.clone()],
                    format!("{b}/lambdas"),
                    public_worlds,
                    format!("on {} \u{2014} DCL_ONE_SDK_CATALYST_ROTATION", host_of(b)),
                ),
                None => (
                    vec![GENESIS_READ.to_string()],
                    GENESIS_LAMBDAS.to_string(),
                    public_worlds,
                    format!("on {}", host_of(deploy::DEFAULT_GENESIS_TARGET_SERVER)),
                ),
            }
        };
    Dest {
        world,
        pointers,
        base_pointer,
        read_bases,
        lambdas_base,
        chain_lambdas: GENESIS_LAMBDAS.to_string(),
        worlds_base,
        headline,
        server_line,
    }
}

/// Long enough for a public catalyst on a bad day, short enough that a cold
/// page is not a hung page.
pub(super) const STATUS_TIMEOUT: Duration = Duration::from_secs(3);
pub(super) const STATUS_TTL: Duration = Duration::from_secs(30);

pub(super) struct CurrentScene {
    pub(super) title: String,
    pub(super) timestamp: Option<i64>,
    /// Counts pointers that fail to parse too, unlike `coords`.
    pub(super) parcels: usize,
    pub(super) coords: Vec<(i64, i64)>,
    /// Deployed bytes where the server says (worlds do, Genesis does not).
    pub(super) size: Option<u64>,
}

impl CurrentScene {
    fn demote(self) -> RemoteScene {
        RemoteScene {
            title: self.title,
            parcels: self.parcels,
            coords: self.coords,
            size: self.size,
        }
    }
}

pub(super) struct RemoteScene {
    pub(super) title: String,
    pub(super) parcels: usize,
    pub(super) coords: Vec<(i64, i64)>,
    pub(super) size: Option<u64>,
}

pub(super) struct RemoteState {
    pub(super) current: Option<CurrentScene>,
    /// Scenes this deploy does not touch (worlds: kept by `multi_scene: true`;
    /// Genesis: other entities under the same pointers, which it replaces).
    pub(super) others: Vec<RemoteScene>,
    /// Every content hash the target is known to hold, for the reuse split.
    pub(super) hashes: HashSet<String>,
}

pub(super) enum Remote {
    Known(RemoteState),
    /// The server answered and holds nothing at this target.
    Empty,
    Unreachable(String),
    /// Never asked: the dry-run test seam, or a scene with nothing to ask for.
    Unknown(String),
}

/// The first scene marked `ours` headlines as `current` — or, when
/// `headline_first`, the first scene at all; the rest are `others`.
fn known(
    mut scenes: Vec<(bool, CurrentScene)>,
    hashes: HashSet<String>,
    headline_first: bool,
) -> Remote {
    let pick = scenes
        .iter()
        .position(|(ours, _)| *ours)
        .or(headline_first.then_some(0));
    let current = pick.map(|i| scenes.remove(i).1);
    let others = scenes.into_iter().map(|(_, s)| s.demote()).collect();
    Remote::Known(RemoteState {
        current,
        others,
        hashes,
    })
}

/// `GET {server}/world/{name}/scenes`: the scene on our parcels is the one
/// this publish replaces, the rest are the neighbours `multi_scene: true`
/// preserves.
pub(super) fn world_remote(body: &serde_json::Value, pointers: &[String]) -> Remote {
    let scenes = deploy::parse_world_scenes(body);
    if scenes.is_empty() {
        return Remote::Empty;
    }
    let ours: HashSet<&str> = pointers.iter().map(String::as_str).collect();
    let mut hashes = HashSet::new();
    let scenes = scenes
        .into_iter()
        .map(|scene| {
            hashes.extend(scene.content_hashes);
            let overlaps = scene.parcels.iter().any(|p| ours.contains(p.as_str()));
            let current = CurrentScene {
                title: scene.title,
                timestamp: scene.timestamp,
                parcels: scene.parcels.len(),
                coords: parse_coords(&scene.parcels),
                size: scene.size,
            };
            (overlaps, current)
        })
        .collect();
    known(scenes, hashes, false)
}

/// `POST {content}/entities/active`: the entity on the base parcel headlines
/// (without one, the first found does); every other entity under the
/// pointers is also replaced, and named so the page never deletes something
/// it did not show.
pub(super) fn genesis_remote(entities: &[serde_json::Value], base_pointer: &str) -> Remote {
    if entities.is_empty() {
        return Remote::Empty;
    }
    let mut hashes = HashSet::new();
    let scenes = entities
        .iter()
        .map(|e| {
            hashes.extend(deploy::entity_content_hashes(e));
            let pointers: Vec<String> = e
                .get("pointers")
                .and_then(|p| p.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let on_base = pointers.iter().any(|p| p == base_pointer);
            let current = CurrentScene {
                title: deploy::entity_title(e),
                timestamp: e.get("timestamp").and_then(|t| t.as_i64()),
                parcels: pointers.len(),
                coords: parse_coords(&pointers),
                size: None,
            };
            (on_base, current)
        })
        .collect();
    known(scenes, hashes, true)
}

pub(super) fn status_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        deploy::client(Duration::from_secs(2), STATUS_TIMEOUT).expect("building the status client")
    })
}

pub(super) enum ProbeFail {
    Http(u16),
    Unreachable,
    Unreadable,
}

impl ProbeFail {
    /// One failure sentence per way a probe can go wrong, naming `url`'s host.
    pub(super) fn sentence(&self, url: &str) -> String {
        let host = host_of(url);
        match self {
            ProbeFail::Http(status) => format!("{host} answered HTTP {status}"),
            ProbeFail::Unreachable => format!("could not reach {host}"),
            ProbeFail::Unreadable => format!("{host} sent an unreadable answer"),
        }
    }
}

/// Sends `req` and parses a 2xx body; anything else is a [`ProbeFail`].
pub(super) async fn fetch_json<T: DeserializeOwned>(
    req: reqwest::RequestBuilder,
) -> Result<T, ProbeFail> {
    let resp = req.send().await.map_err(|_| ProbeFail::Unreachable)?;
    if !resp.status().is_success() {
        return Err(ProbeFail::Http(resp.status().as_u16()));
    }
    resp.json().await.map_err(|_| ProbeFail::Unreadable)
}

/// One look at the target, plus the base that answered (for the availability
/// check). Never an error: everything unreachable becomes a sentence.
pub(super) async fn fetch_remote(dest: &Dest) -> (Remote, Option<String>) {
    if let Some(w) = &dest.world {
        let base = &dest.read_bases[0];
        let url = format!("{base}/world/{}/scenes", deploy::encode_segment(w));
        return match fetch_json::<serde_json::Value>(status_client().get(&url)).await {
            Ok(body) => (world_remote(&body, &dest.pointers), Some(base.clone())),
            Err(ProbeFail::Http(404)) => (Remote::Empty, Some(base.clone())),
            Err(e) => (Remote::Unreachable(e.sentence(base)), None),
        };
    }
    if dest.pointers.is_empty() {
        return (
            Remote::Unknown(
                "scene.json declares no parcels, so there is nothing to look up".into(),
            ),
            None,
        );
    }
    let mut last = String::new();
    for base in &dest.read_bases {
        let req = status_client()
            .post(format!("{base}/entities/active"))
            .json(&serde_json::json!({ "pointers": dest.pointers }));
        match fetch_json::<Vec<serde_json::Value>>(req).await {
            Ok(entities) => {
                return (
                    genesis_remote(&entities, &dest.base_pointer),
                    Some(base.clone()),
                )
            }
            Err(e) => last = e.sentence(base),
        }
    }
    (Remote::Unreachable(last), None)
}

/// The publish-time CIDs (`hash_bytes_v1` over the bytes `deploy::prepare`
/// signs, release copy first) of the payload, cached against the payload
/// fingerprint so they are paid once per edit. A file that cannot be read is
/// absent and counts as an upload.
pub(super) type HashResult = Arc<Result<HashMap<String, String>, String>>;

pub(super) async fn cached_hashes(
    caches: &StatusCaches,
    root: PathBuf,
    print: String,
    rels: Vec<String>,
) -> HashResult {
    if let Some((r, f, v)) = lock(&caches.hashes).as_ref() {
        if *r == root && *f == print {
            return v.clone();
        }
    }
    let hash_root = root.clone();
    let computed = tokio::task::spawn_blocking(move || {
        let mut out = HashMap::new();
        for rel in &rels {
            if let Ok(bytes) = std::fs::read(deploy::payload_path(&hash_root, rel)) {
                out.insert(rel.clone(), catalyrst_hashing::hash_bytes_v1(&bytes));
            }
        }
        Ok(out)
    })
    .await
    .unwrap_or_else(|e| Err(format!("hashing did not finish ({e})")));
    let entry: HashResult = Arc::new(computed);
    *lock(&caches.hashes) = Some((root, print, entry.clone()));
    entry
}

/// The forecast's split over the preview's names and sizes: a file whose
/// hash the server holds transfers nothing; a file with no hash (unreadable,
/// or hashing failed) counts as an upload.
pub(super) fn split_reuse(
    files: &[(String, Option<u64>)],
    hashes: &HashMap<String, String>,
    on_server: &HashSet<String>,
) -> Reuse {
    Reuse::tally(files.iter().map(|(rel, len)| {
        let held = hashes.get(rel).is_some_and(|h| on_server.contains(h));
        (held, len.unwrap_or(0))
    }))
}

pub(super) struct LiveStatus {
    pub(super) remote: Remote,
    pub(super) reuse: Option<Reuse>,
}

impl LiveStatus {
    pub(super) fn unknown(why: &str) -> Self {
        LiveStatus {
            remote: Remote::Unknown(why.to_string()),
            reuse: None,
        }
    }
}

#[derive(Default)]
pub(super) struct StatusCaches {
    hashes: Mutex<Option<(PathBuf, String, HashResult)>>,
    status: Mutex<Vec<(String, Instant, Arc<LiveStatus>)>>,
}

impl StatusCaches {
    pub(super) fn clear(&self) {
        lock(&self.status).clear();
    }
}

/// A TTL cache row lookup: one row per key, invisible once older than `ttl`.
pub(super) fn cache_get<K: PartialEq, V>(
    rows: &[(K, Instant, Arc<V>)],
    key: &K,
    ttl: Duration,
) -> Option<Arc<V>> {
    rows.iter()
        .find(|(k, at, _)| k == key && at.elapsed() < ttl)
        .map(|(_, _, v)| v.clone())
}

/// Replaces `key`'s row and sweeps every expired one.
pub(super) fn cache_put<K: PartialEq, V>(
    rows: &mut Vec<(K, Instant, Arc<V>)>,
    key: K,
    value: Arc<V>,
    ttl: Duration,
) {
    rows.retain(|(k, at, _)| *k != key && at.elapsed() < ttl);
    rows.push((key, Instant::now(), value));
}

/// One row per (target, payload) pair: the fingerprint rides the key so an
/// edit recomputes the reuse split on the next refresh, not thirty seconds later.
fn status_key(dest: &Dest, print: &str) -> String {
    format!(
        "{}|{}|{print}",
        dest.read_bases[0],
        dest.world
            .clone()
            .unwrap_or_else(|| dest.pointers.join(";"))
    )
}

/// The cached answer if still warm, never fetching: the no-wait read the
/// instant page render uses while a background task warms the cache.
pub(super) fn status_peek(
    caches: &StatusCaches,
    dest: &Dest,
    print: &str,
) -> Option<Arc<LiveStatus>> {
    cache_get(&lock(&caches.status), &status_key(dest, print), STATUS_TTL)
}

/// The network look and the hash split, at most once per [`STATUS_TTL`] per
/// (target, payload) pair.
pub(super) async fn cached_status(
    caches: &StatusCaches,
    project: &Project,
    dest: &Dest,
    p: &deploy::DeployPreview,
    print: &str,
) -> Arc<LiveStatus> {
    if let Some(hit) = status_peek(caches, dest, print) {
        return hit;
    }
    let (remote, base) = fetch_remote(dest).await;
    let reuse = match &remote {
        Remote::Known(_) | Remote::Empty => {
            reuse_split(caches, project, p, print, &remote, base).await
        }
        _ => None,
    };
    let entry = Arc::new(LiveStatus { remote, reuse });
    cache_put(
        &mut lock(&caches.status),
        status_key(dest, print),
        entry.clone(),
        STATUS_TTL,
    );
    entry
}

/// The local CIDs against what the server holds: the entity manifests, plus
/// the upload's own `available-content` question for the rest, so the
/// forecast and the upload agree.
async fn reuse_split(
    caches: &StatusCaches,
    project: &Project,
    p: &deploy::DeployPreview,
    print: &str,
    remote: &Remote,
    base: Option<String>,
) -> Option<Reuse> {
    let rels = p.files.iter().map(|(rel, _)| rel.clone()).collect();
    let hashes = cached_hashes(caches, project.root.clone(), print.to_string(), rels).await;
    let map = (*hashes).as_ref().ok()?;
    let mut on_server = match remote {
        Remote::Known(state) => state.hashes.clone(),
        _ => HashSet::new(),
    };
    if let Some(b) = base {
        let unknown: Vec<&str> = map
            .values()
            .filter(|h| !on_server.contains(*h))
            .map(String::as_str)
            .collect();
        on_server.extend(deploy::stored_cids(&b, &unknown).await);
    }
    Some(split_reuse(&p.files, map, &on_server))
}

pub(super) fn ago(ts_ms: i64, now_ms: i64) -> String {
    let mins = (now_ms.saturating_sub(ts_ms)).max(0) / 60_000;
    match mins {
        0..=1 => "just now".to_string(),
        2..=119 => format!("{mins} minutes ago"),
        _ => {
            let hours = mins / 60;
            let days = hours / 24;
            match hours {
                2..=47 => format!("{hours} hours ago"),
                _ if days < 60 => format!("{days} days ago"),
                _ if days < 730 => format!("{} months ago", days / 30),
                _ => format!("{} years ago", days / 365),
            }
        }
    }
}
