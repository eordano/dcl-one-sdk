//! What is at the deploy destination, and what of the payload it already
//! holds: destination resolution (mirroring `deploy::net::resolve_target_from`),
//! the remote entity lookups, the CID/reuse split, and their caches.

use super::landing::parse_parcels;
use crate::deploy::{self, WORLDS_CONTENT_SERVER};
use crate::scene::Project;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

/// Where "what is live on Genesis" is read from when nothing configured a
/// server: the round-robin alias over the public network. Reading is
/// network-wide consistent, so any catalyst answers the same.
pub(super) const GENESIS_READ: &str = "https://peer.decentraland.org/content";

/// Where "who owns what" is read from when nothing configured a server: the
/// lambdas tier of the same public catalyst [`GENESIS_READ`] reads content
/// from. Names and LAND rights are chain state, network-wide consistent.
pub(super) const GENESIS_LAMBDAS: &str = "https://peer.decentraland.org/lambdas";

#[derive(Clone)]
pub(super) struct Dest {
    pub(super) world: Option<String>,
    pub(super) pointers: Vec<String>,
    pub(super) base_pointer: String,
    /// Status endpoints, tried in order: a raw content server answers on the
    /// bare base, a catalyst domain under `/content`.
    pub(super) read_bases: Vec<String>,
    /// Where the deploy-deciding lambdas live — the target's own, because
    /// the verdict must ask the server that will actually rule on the
    /// publish (its batch parcel-permissions route included).
    pub(super) lambdas_base: String,
    /// Where address-scoped chain facts are read for display: names owned,
    /// LAND held. Chain state is network-wide consistent and a self-hosted
    /// realm's squid often deliberately carries none of it, so this is
    /// always the public Genesis lambdas.
    pub(super) chain_lambdas: String,
    /// Where world-scoped questions are asked (`/worlds`, `/world/{name}/…`).
    /// The explorer-api gateway does not proxy these, so this is the worlds
    /// service itself — the deploy target when one is configured, else the
    /// public worlds server.
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
        .filter_map(|p| catalyrst_types::pointer::parse_pointer(p))
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

/// Pure so a test can drive it without touching the process environment; the
/// page passes `deploy::env_default_target()` and
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

    if let Some(t) = default_target.map(str::trim).filter(|t| !t.is_empty()) {
        let base_url = deploy::sanitize_catalyst_url(t);
        let host = host_of(&base_url);
        let root = base_url.trim_end_matches("/content").to_string();
        let read_bases = match base_url.ends_with("/content") {
            true => vec![base_url.clone()],
            false => vec![base_url.clone(), format!("{base_url}/content")],
        };
        let headline = match &world {
            Some(w) => format!("World {w}"),
            None => parcel_span(&parcels),
        };
        return Dest {
            world,
            pointers,
            base_pointer,
            read_bases,
            lambdas_base: format!("{root}/lambdas"),
            chain_lambdas: GENESIS_LAMBDAS.to_string(),
            worlds_base: root,
            headline,
            server_line: format!("on {host} \u{2014} DCL_ONE_SDK_DEFAULT_TARGET"),
        };
    }
    if let Some(w) = world {
        return Dest {
            headline: format!("World {w}"),
            server_line: format!("on {}", host_of(WORLDS_CONTENT_SERVER)),
            world: Some(w),
            pointers,
            base_pointer,
            read_bases: vec![WORLDS_CONTENT_SERVER.to_string()],
            lambdas_base: GENESIS_LAMBDAS.to_string(),
            chain_lambdas: GENESIS_LAMBDAS.to_string(),
            worlds_base: WORLDS_CONTENT_SERVER.to_string(),
        };
    }
    let (read_bases, lambdas_base, server_line) =
        match rotation.as_deref().and_then(<[String]>::first) {
            Some(b) => (
                vec![format!("{b}/content"), b.clone()],
                format!("{b}/lambdas"),
                format!("on {} \u{2014} DCL_ONE_SDK_CATALYST_ROTATION", host_of(b)),
            ),
            None => (
                vec![GENESIS_READ.to_string()],
                GENESIS_LAMBDAS.to_string(),
                "on a public Genesis City catalyst".to_string(),
            ),
        };
    Dest {
        world: None,
        headline: parcel_span(&parcels),
        server_line,
        pointers,
        base_pointer,
        read_bases,
        lambdas_base,
        chain_lambdas: GENESIS_LAMBDAS.to_string(),
        worlds_base: WORLDS_CONTENT_SERVER.to_string(),
    }
}

/// A short, cached look at the target. 3 seconds is long enough for a public
/// catalyst on a bad day and short enough that a cold page is not a hung page.
pub(super) const STATUS_TIMEOUT: Duration = Duration::from_secs(3);
pub(super) const STATUS_TTL: Duration = Duration::from_secs(30);

/// `available-content` is a GET with one `cid` pair per hash: batches keep the
/// URL under proxy header limits, and the cap keeps a thousand-file scene from
/// turning one page load into a dozen requests — past it, reuse falls back to
/// the active entity's own manifest and simply undercounts.
pub(super) const AVAILABILITY_BATCH: usize = 80;
pub(super) const AVAILABILITY_CAP: usize = 240;

pub(super) struct CurrentScene {
    pub(super) title: String,
    pub(super) timestamp: Option<i64>,
    pub(super) parcels: usize,
    /// The parcels themselves, parsed — what the after-map draws. The count
    /// above stays separate because a pointer that fails to parse still
    /// counts.
    pub(super) coords: Vec<(i64, i64)>,
    /// Deployed bytes, where the server says (the worlds server does; a
    /// Genesis entity does not).
    pub(super) size: Option<u64>,
}

pub(super) struct RemoteScene {
    pub(super) title: String,
    pub(super) parcels: usize,
    pub(super) coords: Vec<(i64, i64)>,
    pub(super) size: Option<u64>,
}

pub(super) struct RemoteState {
    /// The scene occupying the parcels this deploy writes to, if any.
    pub(super) current: Option<CurrentScene>,
    /// Scenes on the target that this deploy does not touch (worlds: kept by
    /// `multi_scene: true`; Genesis: other entities under the same pointers,
    /// which this publish replaces).
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

/// `GET {server}/world/{name}/scenes` → what the world holds. The scene on our
/// parcels is the one this publish replaces; the rest are the neighbours
/// `multi_scene: true` exists to preserve.
pub(super) fn world_remote(body: &serde_json::Value, pointers: &[String]) -> Remote {
    let scenes = deploy::parse_world_scenes(body);
    if scenes.is_empty() {
        return Remote::Empty;
    }
    let ours: HashSet<&str> = pointers.iter().map(String::as_str).collect();
    let mut current = None;
    let mut others = Vec::new();
    let mut hashes = HashSet::new();
    for scene in scenes {
        hashes.extend(scene.content_hashes);
        let overlaps = scene.parcels.iter().any(|p| ours.contains(p.as_str()));
        if overlaps && current.is_none() {
            current = Some(CurrentScene {
                title: scene.title,
                timestamp: scene.timestamp,
                parcels: scene.parcels.len(),
                coords: parse_coords(&scene.parcels),
                size: scene.size,
            });
        } else {
            others.push(RemoteScene {
                title: scene.title,
                parcels: scene.parcels.len(),
                coords: parse_coords(&scene.parcels),
                size: scene.size,
            });
        }
    }
    Remote::Known(RemoteState {
        current,
        others,
        hashes,
    })
}

/// `POST {content}/entities/active` → the entities under our pointers. The one
/// holding the base parcel is the scene this publish replaces; any other
/// entity under the remaining pointers is also replaced, and is named so the
/// page never deletes something it did not show.
pub(super) fn genesis_remote(entities: &[serde_json::Value], base_pointer: &str) -> Remote {
    if entities.is_empty() {
        return Remote::Empty;
    }
    let mut hashes = HashSet::new();
    let mut current = None;
    let mut others = Vec::new();
    for e in entities {
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
        let parcels = pointers.len();
        let on_base = pointers.iter().any(|p| p == base_pointer);
        let scene = CurrentScene {
            title: deploy::entity_title(e),
            timestamp: e.get("timestamp").and_then(|t| t.as_i64()),
            parcels,
            coords: parse_coords(&pointers),
            size: None,
        };
        if on_base && current.is_none() {
            current = Some(scene);
        } else {
            others.push(RemoteScene {
                title: scene.title,
                parcels,
                coords: parse_coords(&pointers),
                size: None,
            });
        }
    }
    if current.is_none() && !others.is_empty() {
        // Without an entity on the base parcel, the first one found is still
        // the most useful thing to headline.
        let first = others.remove(0);
        current = Some(CurrentScene {
            title: first.title,
            timestamp: entities
                .first()
                .and_then(|e| e.get("timestamp"))
                .and_then(|t| t.as_i64()),
            parcels: first.parcels,
            coords: first.coords,
            size: first.size,
        });
    }
    Remote::Known(RemoteState {
        current,
        others,
        hashes,
    })
}

pub(super) fn status_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        deploy::client(Duration::from_secs(2), STATUS_TIMEOUT).expect("building the status client")
    })
}

/// One failure sentence per way a probe can go wrong, shared by both target
/// shapes.
pub(super) fn probe_failure(base: &str, outcome: &Result<reqwest::StatusCode, ()>) -> String {
    match outcome {
        Ok(status) => format!("{} answered HTTP {}", host_of(base), status.as_u16()),
        Err(()) => format!("could not reach {}", host_of(base)),
    }
}

pub(super) fn unreadable(base: &str) -> String {
    format!("{} sent an unreadable answer", host_of(base))
}

/// One look at the target, plus the base that answered (for the availability
/// check). Never an error: everything unreachable becomes a sentence.
pub(super) async fn fetch_remote(dest: &Dest) -> (Remote, Option<String>) {
    if let Some(w) = &dest.world {
        let base = &dest.read_bases[0];
        let url = format!("{base}/world/{}/scenes", deploy::encode_segment(w));
        return match status_client().get(&url).send().await {
            Ok(resp) if resp.status().as_u16() == 404 => (Remote::Empty, Some(base.clone())),
            Ok(resp) if resp.status().is_success() => match resp.json().await {
                Ok(body) => (world_remote(&body, &dest.pointers), Some(base.clone())),
                Err(_) => (Remote::Unreachable(unreadable(base)), None),
            },
            Ok(resp) => (
                Remote::Unreachable(probe_failure(base, &Ok(resp.status()))),
                None,
            ),
            Err(_) => (Remote::Unreachable(probe_failure(base, &Err(()))), None),
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
        let url = format!("{base}/entities/active");
        match status_client()
            .post(&url)
            .json(&serde_json::json!({ "pointers": dest.pointers }))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(entities) = resp.json::<Vec<serde_json::Value>>().await {
                    return (
                        genesis_remote(&entities, &dest.base_pointer),
                        Some(base.clone()),
                    );
                }
                last = unreadable(base);
            }
            Ok(resp) => last = probe_failure(base, &Ok(resp.status())),
            Err(_) => last = probe_failure(base, &Err(())),
        }
    }
    (Remote::Unreachable(last), None)
}

/// Which of `cids` the server already stores, by the same `available-content`
/// check the upload protocol itself runs. `None` means the question went
/// unanswered and reuse falls back to the entity manifest.
pub(super) async fn available_on_server(base: &str, cids: &[String]) -> Option<HashSet<String>> {
    let batches = cids.chunks(AVAILABILITY_BATCH).map(|batch| {
        let query: String = batch
            .iter()
            .map(|c| format!("cid={c}"))
            .collect::<Vec<_>>()
            .join("&");
        let url = format!("{base}/available-content?{query}");
        async move {
            let resp = status_client().get(&url).send().await.ok()?;
            if !resp.status().is_success() {
                return None;
            }
            resp.json::<Vec<serde_json::Value>>().await.ok()
        }
    });
    let mut have = HashSet::new();
    for body in futures::future::join_all(batches).await {
        have.extend(body?.iter().filter_map(|e| {
            let available = e.get("available").and_then(|a| a.as_bool())?;
            available
                .then(|| e.get("cid").and_then(|c| c.as_str()).map(str::to_string))
                .flatten()
        }));
    }
    Some(have)
}

/// The real content hashes (CIDs) of the current payload — the same
/// `hash_bytes_v1` that `deploy::prepare` signs at publish time — computed off
/// the async worker and cached against the payload fingerprint, so they are
/// paid once per edit rather than once per refresh. A file that cannot be
/// read is simply absent from the map and counts as an upload.
pub(super) type HashResult = Arc<Result<HashMap<String, String>, String>>;

pub(super) async fn cached_hashes(
    caches: &StatusCaches,
    root: PathBuf,
    print: String,
    rels: Vec<String>,
) -> HashResult {
    {
        let guard = caches.hashes.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((r, f, v)) = guard.as_ref() {
            if *r == root && *f == print {
                return v.clone();
            }
        }
    }
    let hash_root = root.clone();
    let computed = tokio::task::spawn_blocking(move || {
        let mut out = HashMap::new();
        for rel in &rels {
            if let Ok(bytes) = std::fs::read(hash_root.join(rel)) {
                out.insert(rel.clone(), catalyrst_hashing::hash_bytes_v1(&bytes));
            }
        }
        Ok(out)
    })
    .await
    .unwrap_or_else(|e| Err(format!("hashing did not finish ({e})")));
    let entry: HashResult = Arc::new(computed);
    *caches.hashes.lock().unwrap_or_else(PoisonError::into_inner) =
        Some((root, print, entry.clone()));
    entry
}

/// The reuse split, over whatever the two sides actually know: a file whose
/// hash the server holds transfers nothing, a file with no hash (unreadable,
/// or hashing failed) is counted as an upload rather than guessed at.
pub(super) struct Reuse {
    pub(super) reused_files: usize,
    pub(super) reused_bytes: u64,
    pub(super) upload_files: usize,
    pub(super) upload_bytes: u64,
}

pub(super) fn split_reuse(
    files: &[(String, Option<u64>)],
    hashes: &HashMap<String, String>,
    on_server: &HashSet<String>,
) -> Reuse {
    let mut r = Reuse {
        reused_files: 0,
        reused_bytes: 0,
        upload_files: 0,
        upload_bytes: 0,
    };
    for (rel, len) in files {
        let reused = hashes.get(rel).is_some_and(|h| on_server.contains(h));
        match reused {
            true => {
                r.reused_files += 1;
                r.reused_bytes += len.unwrap_or(0);
            }
            false => {
                r.upload_files += 1;
                r.upload_bytes += len.unwrap_or(0);
            }
        }
    }
    r
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

/// The two remote-knowledge caches, one field on `DeployState`.
#[derive(Default)]
pub(super) struct StatusCaches {
    hashes: Mutex<Option<(PathBuf, String, HashResult)>>,
    status: Mutex<Vec<(String, Instant, Arc<LiveStatus>)>>,
}

impl StatusCaches {
    pub(super) fn clear(&self) {
        status_cache(self).clear();
    }
}

pub(super) fn status_cache(
    caches: &StatusCaches,
) -> std::sync::MutexGuard<'_, Vec<(String, Instant, Arc<LiveStatus>)>> {
    caches.status.lock().unwrap_or_else(PoisonError::into_inner)
}

/// One cache row per (target, payload) pair; the fingerprint rides the key
/// so an edit recomputes the reuse split on the next refresh instead of
/// thirty seconds later.
fn status_key(dest: &Dest, print: &str) -> String {
    format!(
        "{}|{}|{print}",
        dest.read_bases[0],
        dest.world
            .clone()
            .unwrap_or_else(|| dest.pointers.join(";"))
    )
}

/// The cached answer if it is still warm, without ever fetching: the no-wait
/// read the instant page render uses while a background task warms the cache.
pub(super) fn status_peek(
    caches: &StatusCaches,
    dest: &Dest,
    print: &str,
) -> Option<Arc<LiveStatus>> {
    let key = status_key(dest, print);
    status_cache(caches)
        .iter()
        .find(|(k, at, _)| *k == key && at.elapsed() < STATUS_TTL)
        .map(|(_, _, v)| v.clone())
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
    let key = status_key(dest, print);
    let hit = status_cache(caches)
        .iter()
        .find(|(k, at, _)| *k == key && at.elapsed() < STATUS_TTL)
        .map(|(_, _, v)| v.clone());
    if let Some(hit) = hit {
        return hit;
    }
    let (remote, base) = fetch_remote(dest).await;
    let reuse = match &remote {
        Remote::Known(_) | Remote::Empty => {
            let rels: Vec<String> = p.files.iter().map(|(rel, _)| rel.clone()).collect();
            let hashes = cached_hashes(caches, project.root.clone(), print.to_string(), rels).await;
            match &*hashes {
                Ok(map) => {
                    let mut on_server = match &remote {
                        Remote::Known(state) => state.hashes.clone(),
                        _ => HashSet::new(),
                    };
                    if let Some(b) = base {
                        let unknown: Vec<String> = {
                            let mut u: Vec<String> = map
                                .values()
                                .filter(|h| !on_server.contains(*h))
                                .cloned()
                                .collect();
                            u.sort();
                            u.dedup();
                            u
                        };
                        if !unknown.is_empty() && unknown.len() <= AVAILABILITY_CAP {
                            if let Some(have) = available_on_server(&b, &unknown).await {
                                on_server.extend(have);
                            }
                        }
                    }
                    Some(split_reuse(&p.files, map, &on_server))
                }
                Err(_) => None,
            }
        }
        _ => None,
    };
    let entry = Arc::new(LiveStatus { remote, reuse });
    let mut c = status_cache(caches);
    c.retain(|(k, at, _)| *k != key && at.elapsed() < STATUS_TTL);
    c.push((key, Instant::now(), entry.clone()));
    entry
}

pub(super) fn ago(ts_ms: i64, now_ms: i64) -> String {
    let mins = (now_ms.saturating_sub(ts_ms)).max(0) / 60_000;
    match mins {
        0..=1 => "just now".to_string(),
        2..=119 => format!("{mins} minutes ago"),
        _ => {
            let hours = mins / 60;
            match hours {
                2..=47 => format!("{hours} hours ago"),
                _ => format!("{} days ago", hours / 24),
            }
        }
    }
}
