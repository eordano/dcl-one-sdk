mod net;
mod run;
mod unpublish;
mod world_gate;

#[cfg(test)]
pub(crate) use net::ENV_LOCK;

pub use net::{
    build_delete_payload, configured_target_server, encode_segment, forget_remembered_target,
    jump_in_url, non_upstream_note, play_url, sanitize_catalyst_url, scenes_on_other_parcels,
    send_world_delete, simple_auth_chain, upload_entity, PermissionGate, WorldScene,
    WORLDS_CONTENT_SERVER,
};
pub(crate) use net::{
    client, denied_parcels_in, deployment_permission_in_doc, entity_content_hashes, entity_title,
    host_of, parse_world_scenes, read_server_message, refusal, send_text, unreachable_server,
    upload_entity_to, with_headers, DocAnswer, UploadDestination, VERBOSE_HINT,
};
pub use run::{deploy, load_signer};
pub use unpublish::{unpublish, UnpublishOptions};
pub use world_gate::{nameless_world_section, refuse_nameless_world};

use crate::jsjson::{self, JsValue};
use crate::scene::Project;
use crate::ux::{TrySteps, UserError};
use anyhow::{Context, Result};
use catalyrst_hashing::hash_bytes_v1;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::SystemTime;

pub struct DeployOptions {
    pub dir: PathBuf,
    pub target: Option<String>,
    pub target_content: Option<String>,
    pub sign_key: Option<PathBuf>,
    pub skip_build: bool,
    pub dry_run: bool,
    pub timestamp: Option<i64>,
    pub entity_out: Option<PathBuf>,
    pub multi_scene: bool,
    /// Ask the worlds server whether the wallet may publish before uploading;
    /// see [`PermissionGate`] for why it is opt-in.
    pub check_permissions: bool,
    pub yes: bool,
    pub no_browser: bool,
    pub ci: bool,
    pub port: Option<u16>,
    /// A caller hosting the signing routes on its own server (the preview
    /// server's `/deploy/sign/`); `None` (the CLI) serves the page itself.
    pub host_signer: Option<crate::linker::HostSigner>,
    /// No terminal narration: a page-driven publish tells its story on the
    /// page. Errors still print.
    pub quiet: bool,
    /// A delegated identity that signs headlessly instead of hosting a
    /// browser signing page, while unexpired.
    pub identity: Option<DeployIdentity>,
}

/// A throwaway key the wallet authorized once, and the proof it did; signs
/// deploys as the wallet until the delegation expires.
#[derive(Clone)]
pub struct DeployIdentity {
    pub signer: String,
    /// Hex; in memory only, never written.
    pub ephemeral_key: String,
    /// The exact `Decentraland Login\n…` text the wallet signed.
    pub delegation_payload: String,
    pub delegation_signature: String,
    pub expiration_ms: i64,
}

impl DeployIdentity {
    pub fn expired(&self, now_ms: i64) -> bool {
        now_ms >= self.expiration_ms
    }
}

const MAX_FILE_SIZE_BYTES: usize = 50_000_000;

/// The public Genesis City network: the classifier behind `non_upstream_note`
/// and the rotation `deploy` falls back to.
pub const UPSTREAM_CATALYST_HOSTS: [&str; 8] = [
    "https://interconnected.online",
    "https://peer-ec2.decentraland.org",
    "https://peer.melonwave.com",
    "https://peer-ec1.decentraland.org",
    "https://peer-ap1.decentraland.org",
    "https://peer.uadevops.com",
    "https://peer.dclnodes.io",
    "https://peer-eu1.decentraland.org",
];

/// This build's own default Genesis City catalyst: tried first, ahead of
/// `UPSTREAM_CATALYST_HOSTS`, whenever nobody configured their own rotation.
/// A fork wanting a different one edits this constant; the health check
/// (`rotation_content_url`) still moves on to the rest of the rotation if
/// it does not answer.
pub const DEFAULT_GENESIS_TARGET_SERVER: &str = "https://peer.decentraland.org";

/// The rotation named by DCL_ONE_SDK_CATALYST_ROTATION (comma-separated), or
/// `None`. Callers that must not reach a public catalyst on their own read
/// this rather than `catalyst_rotation`.
pub fn configured_catalyst_rotation() -> Option<Vec<String>> {
    let rotation: Vec<String> = std::env::var("DCL_ONE_SDK_CATALYST_ROTATION")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .collect();
    (!rotation.is_empty()).then_some(rotation)
}

/// Catalysts `deploy` picks from when given no target; the implicit choice
/// is announced and confirmed at the call site.
pub fn catalyst_rotation() -> Vec<String> {
    configured_catalyst_rotation().unwrap_or_else(|| {
        std::iter::once(DEFAULT_GENESIS_TARGET_SERVER.to_string())
            .chain(UPSTREAM_CATALYST_HOSTS.iter().map(|h| h.to_string()))
            .collect()
    })
}

/// Built-in ignore rules over developer files: source, build configuration
/// and output, docs, scripts, source maps, tool state. Scene code can name
/// one of these — a bundler's module-path comment, a `require('../package.json')`
/// — without the explorer ever fetching it, so such a mention is not a lost
/// asset.
const DEVELOPER_DCL_IGNORE: [&str; 22] = [
    ".*",
    "package.json",
    "package-lock.json",
    "yarn-lock.json",
    "build.json",
    "export",
    "tsconfig.json",
    "tslint.json",
    "node_modules",
    "dclcontext",
    "sdk-skills",
    "**/*.ts",
    "**/*.tsx",
    "Dockerfile",
    "dist",
    "README.md",
    // Non-asset developer files. `*.html` earns its place twice: a DCL scene
    // is ECS/JS rendered in the 3D client, never HTML, AND a Cloudflare-
    // fronted content server's WAF reads raw HTML in the upload body as an
    // injection attack and 403-challenges the whole deploy. `*.sh`/`*.cjs`/
    // `*.md`/`*.mdc` are scripts and docs that ride along the same way.
    "*.html",
    "*.sh",
    "*.cjs",
    "*.md",
    "*.mdc",
    "*.map",
];

/// Built-in ignore rules over files scene code could otherwise name as
/// content: source art, archives, and the project's own Creator Hub asset
/// previews. A bundle naming one of these has lost an asset, and preview and
/// deploy warn about it.
const SOURCE_ASSET_DCL_IGNORE: [&str; 5] = [
    // Root-anchored: the project's own thumbnails/ holds Creator Hub asset
    // previews; a thumbnails/ nested anywhere else is scene content.
    "/thumbnails",
    "*.blend",
    "*.fbx",
    "*.zip",
    "*.rar",
];

const EXTRA_DCL_IGNORE: [&str; 6] = [
    ".*",
    "node_modules",
    "**/*.ts",
    "**/*.tsx",
    "node_modules/**",
    "*.md",
];

/// Every effective ignore line in matching order, with where it came from:
/// the project's `.dclignore` for its own lines, `None` for the built-in
/// defaults. A line the project repeats counts as the project's.
fn dcl_ignore_lines(root: &Path) -> Vec<(Option<PathBuf>, String)> {
    let dclignore = root.join(".dclignore");
    let user = std::fs::read_to_string(&dclignore).unwrap_or_default();
    let mut seen = HashSet::new();
    user.split('\n')
        .map(|p| (Some(dclignore.clone()), p))
        .chain(
            DEVELOPER_DCL_IGNORE
                .into_iter()
                .chain(SOURCE_ASSET_DCL_IGNORE)
                .chain(EXTRA_DCL_IGNORE)
                .map(|p| (None, p)),
        )
        .filter(|(_, p)| !p.is_empty() && seen.insert(p.to_string()))
        .map(|(from, p)| (from, p.to_string()))
        .collect()
}

pub fn dcl_ignore_patterns(root: &Path) -> Vec<String> {
    dcl_ignore_lines(root).into_iter().map(|(_, p)| p).collect()
}

/// A `map_err` closure: the user-facing error wrapping the underlying cause.
pub(crate) fn caused<E>(what: impl Into<String>, steps: TrySteps) -> impl FnOnce(E) -> anyhow::Error
where
    E: Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
{
    move |e| UserError::new(what, steps).caused_by(e).into()
}

fn build_matcher(root: &Path) -> Result<Gitignore> {
    let mut b = GitignoreBuilder::new(root);
    b.case_insensitive(true).context("matcher options")?;
    for (from, p) in dcl_ignore_lines(root) {
        b.add_line(from, &p).map_err(caused(
            format!(".dclignore line {p:?} is not a valid pattern"),
            TrySteps::one("fix or delete that line (gitignore syntax)"),
        ))?;
    }
    b.build().context("building ignore matcher")
}

/// `readdir` already answered this, so the stat behind `Path::is_dir` is only
/// paid where `d_type` is unknown — and for symlinks, whose own type says
/// nothing about the target (a symlinked directory is descended into).
fn entry_is_dir(entry: &std::fs::DirEntry, path: &Path) -> bool {
    match entry.file_type() {
        Ok(ft) if !ft.is_symlink() => ft.is_dir(),
        _ => path.is_dir(),
    }
}

/// One walk, two lists: what a deploy uploads and what `.dclignore` kept out.
/// An ignored DIRECTORY is not descended into, so nothing under it lands in
/// either list; dot-entries are never publishable and skipped outright.
fn walk(dir: &Path, root: &Path, gi: &Gitignore, out: &mut Vec<String>, ignored: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(String, String)> = Vec::new();
    let mut dirs: Vec<(String, PathBuf)> = Vec::new();
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        let rel = rel.to_string_lossy().replace('\\', "/");
        let is_dir = entry_is_dir(&entry, &path);
        if gi.matched(&rel, is_dir).is_ignore() {
            if !is_dir {
                ignored.push(rel);
            }
            continue;
        }
        match is_dir {
            true => dirs.push((name, path)),
            false => files.push((name, rel)),
        }
    }
    files.sort_by(|a, b| b.0.cmp(&a.0));
    dirs.sort_by(|a, b| b.0.cmp(&a.0));
    out.extend(files.into_iter().map(|(_, rel)| rel));
    for (_, path) in dirs {
        walk(&path, root, gi, out, ignored);
    }
}

pub fn collect_publishable_files(root: &Path) -> Result<Vec<String>> {
    Ok(collect_files(root)?.0)
}

fn collect_files(root: &Path) -> Result<(Vec<String>, Vec<String>)> {
    let gi = build_matcher(root)?;
    let (mut out, mut ignored) = (Vec::new(), Vec::new());
    walk(root, root, &gi, &mut out, &mut ignored);
    warn_default_ignored_named_by_bundle(root, &gi, &ignored);
    Ok((out, ignored))
}

/// The chunk files a dcl-one-sdk loader stub evaluates: every
/// `var __dclOne…ChunkPath = '<rel>'` declaration with a non-empty path
/// (see `split::loader_stub`). Any other entry point declares none.
fn stub_chunk_paths(entry: &str) -> Vec<String> {
    entry
        .lines()
        .filter_map(|line| {
            let rest = line.trim_start().strip_prefix("var __dclOne")?;
            let (_, rest) = rest.split_once("ChunkPath = '")?;
            let (rel, _) = rest.split_once('\'')?;
            (!rel.is_empty()).then(|| rel.to_string())
        })
        .collect()
}

/// A bundle file as last seen on disk; a moved stamp means re-reading it.
#[derive(Clone, Debug, PartialEq)]
struct Stamp {
    rel: String,
    modified: Option<SystemTime>,
    len: u64,
}

impl Stamp {
    fn of(root: &Path, rel: &str) -> Self {
        let meta = std::fs::metadata(root.join(rel)).ok();
        Stamp {
            rel: rel.to_string(),
            modified: meta.as_ref().and_then(|m| m.modified().ok()),
            len: meta.map_or(0, |m| m.len()),
        }
    }
}

struct Bundle {
    stamp: Stamp,
    text: String,
}

/// Stamped before it is read, so a rewrite racing the read moves the stamp
/// and the next scan reads again. Bytes, so a non-UTF-8 stretch does not
/// read as a missing file.
fn read_bundle(root: &Path, rel: &str) -> Option<Bundle> {
    let stamp = Stamp::of(root, rel);
    let bytes = std::fs::read(root.join(rel)).ok()?;
    Some(Bundle {
        stamp,
        text: String::from_utf8_lossy(&bytes).into_owned(),
    })
}

/// The JS the explorer evaluates, plus the stamp of every file it may live
/// in: the entry point — scene.json's `main`, else the SDK7 then the SDK6
/// default name — and, when that entry is a loader stub, each chunk it
/// declares (stamped even while absent, so its arrival is noticed). Both
/// empty until an entry point is built.
fn built_bundles(root: &Path) -> (Vec<Stamp>, Vec<Bundle>) {
    let main = std::fs::read(root.join("scene.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .and_then(|v| v.get("main")?.as_str().map(str::to_string));
    let Some(entry) = main
        .as_deref()
        .into_iter()
        .chain(["bin/index.js", "bin/game.js"])
        .find_map(|rel| read_bundle(root, rel))
    else {
        return (Vec::new(), Vec::new());
    };
    let chunks = stub_chunk_paths(&entry.text);
    let mut stamps = vec![entry.stamp.clone()];
    let mut bundles = vec![entry];
    for rel in &chunks {
        match read_bundle(root, rel) {
            Some(chunk) => {
                stamps.push(chunk.stamp.clone());
                bundles.push(chunk);
            }
            None => stamps.push(Stamp::of(root, rel)),
        }
    }
    (stamps, bundles)
}

/// Whether `text` names `rel` the way scene code names an asset: quoted,
/// with at most a `./` or `/` prefix, and closed by the quote (also when
/// escaped inside an embedding string) or by a `?`/`#` suffix. A path inside
/// a URL, a bundler's module-path comment or a `require('../x')` is not
/// that. Both sides come lowercased.
fn names_path(text: &str, rel: &str) -> bool {
    text.match_indices(rel).any(|(i, _)| {
        let before = &text[..i];
        let before = before
            .strip_suffix("./")
            .or_else(|| before.strip_suffix('/'))
            .unwrap_or(before);
        let after = &text[i + rel.len()..];
        before.ends_with(['"', '\'', '`'])
            && (after.starts_with(['"', '\'', '`', '?', '#'])
                || after.starts_with("\\\"")
                || after.starts_with("\\'"))
    })
}

/// Whether a built-in source-asset rule — not the project's `.dclignore`,
/// not a developer-file rule — is what keeps `rel` out of the upload.
fn kept_out_by_source_asset_default(gi: &Gitignore, rel: &str) -> bool {
    gi.matched(rel, false)
        .inner()
        .is_some_and(|g| g.from().is_none() && SOURCE_ASSET_DCL_IGNORE.contains(&g.original()))
}

/// `(file, bundle)` for each candidate some bundle names: the explorer will
/// ask the content server for a file it never received. Case-insensitive on
/// both sides, like the matcher and the content servers.
fn named_by_bundles(candidates: &[String], bundles: &[Bundle]) -> Vec<(String, String)> {
    let texts: Vec<(&str, String)> = bundles
        .iter()
        .map(|b| (b.stamp.rel.as_str(), b.text.to_lowercase()))
        .collect();
    candidates
        .iter()
        .filter_map(|rel| {
            let lower = rel.to_lowercase();
            let (bundle, _) = texts.iter().find(|(_, text)| names_path(text, &lower))?;
            Some((rel.clone(), bundle.to_string()))
        })
        .collect()
}

/// One project's last scan: the bundle files it read (by stamp), the
/// candidates it checked, and what it found.
struct BundleScan {
    stamps: Vec<Stamp>,
    candidates: Vec<String>,
    named: Vec<(String, String)>,
}

/// The `(file, bundle)` pairs to warn about now, or `None`. The content map
/// is rebuilt on every stale entity request, so a project is rescanned only
/// when its candidate list or a bundle file's stamp moved, and the result is
/// returned only when it is non-empty and differs from the last one: one
/// WARN per change, not one per poll.
fn changed_named_by_bundles(
    root: &Path,
    gi: &Gitignore,
    ignored: &[String],
) -> Option<Vec<(String, String)>> {
    static SCANS: OnceLock<Mutex<HashMap<PathBuf, BundleScan>>> = OnceLock::new();
    let candidates: Vec<String> = ignored
        .iter()
        .filter(|rel| kept_out_by_source_asset_default(gi, rel))
        .cloned()
        .collect();
    let mut scans = SCANS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if candidates.is_empty() {
        scans.remove(root);
        return None;
    }
    if scans.get(root).is_some_and(|last| {
        last.candidates == candidates
            && last
                .stamps
                .iter()
                .all(|stamp| Stamp::of(root, &stamp.rel) == *stamp)
    }) {
        return None;
    }
    let previous = scans.remove(root);
    let (stamps, bundles) = built_bundles(root);
    if bundles.is_empty() {
        return None;
    }
    let named = named_by_bundles(&candidates, &bundles);
    let changed = !named.is_empty() && previous.is_none_or(|last| last.named != named);
    scans.insert(
        root.to_path_buf(),
        BundleScan {
            stamps,
            candidates,
            named: named.clone(),
        },
    );
    changed.then_some(named)
}

fn warn_default_ignored_named_by_bundle(root: &Path, gi: &Gitignore, ignored: &[String]) {
    let Some(named) = changed_named_by_bundles(root, gi, ignored) else {
        return;
    };
    let mut bundles: Vec<&str> = named.iter().map(|(_, bundle)| bundle.as_str()).collect();
    bundles.sort_unstable();
    bundles.dedup();
    let files: Vec<&str> = named.iter().map(|(file, _)| file.as_str()).collect();
    tracing::warn!(
        "the default ignore rules keep {} file(s) that {} names out of the upload; the explorer will not find them: {}",
        named.len(),
        bundles.join(", "),
        files.join(", ")
    );
}

/// What a deploy would upload, without reading a byte: the same walk and
/// `.dclignore` rules as [`prepare`], with sizes from the directory entry, so
/// a page can render it on every refresh.
pub struct DeployPreview {
    /// Publishable files, largest first; `None` when the entry could not be
    /// stat'd (see `unreadable`).
    pub files: Vec<(String, Option<u64>)>,
    pub total_bytes: u64,
    /// Files `.dclignore` keeps out, from directories that are themselves
    /// published — the texture excluded by accident, not the seventeen
    /// thousand files under node_modules.
    pub ignored: Vec<String>,
    /// Over the per-file limit; `prepare` would refuse them.
    pub oversize: Vec<String>,
    /// Most often a dangling symlink: `prepare` reads every file, so these
    /// abort the deploy *after* the wallet prompt unless named here.
    pub unreadable: Vec<String>,
    pub main: MainBundle,
    /// Pairs differing only in case; content servers are case-insensitive.
    pub collisions: Vec<(String, String)>,
    pub nameless_world: bool,
}

/// The one file a scene cannot be published without.
#[derive(Debug, PartialEq, Eq)]
pub enum MainBundle {
    Present(String),
    /// Not built, or `.dclignore` excludes it.
    Missing(String),
    /// scene.json's `"main"` is itself unusable; the string says why.
    Unusable(String),
}

/// Names a content server would read as one file, paired with the first name
/// they collide with. Over names, not the filesystem: the pair cannot exist
/// on a case-insensitive volume.
fn case_collisions(rels: &[String]) -> Vec<(String, String)> {
    let mut seen: HashMap<String, String> = HashMap::new();
    let mut out = Vec::new();
    for rel in rels {
        if let Some(first) = seen.insert(rel.to_lowercase(), rel.clone()) {
            out.push((rel.clone(), first));
        }
    }
    out
}

pub fn preview(project: &Project) -> Result<DeployPreview> {
    let root = &project.root;
    let (publishable, mut ignored) = collect_files(root)?;
    ignored.sort();
    let main = match project.main_output() {
        Err(e) => MainBundle::Unusable(format!("{e}")),
        Ok(main) => match publishable.iter().any(|r| r == &main) {
            true => MainBundle::Present(main),
            false => MainBundle::Missing(main),
        },
    };
    let collisions = case_collisions(&publishable);
    let mut files: Vec<(String, Option<u64>)> = publishable
        .iter()
        .map(|rel| {
            let len = std::fs::metadata(root.join(rel)).ok().map(|m| m.len());
            (rel.clone(), len)
        })
        .collect();
    files.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let total_bytes = files.iter().filter_map(|(_, len)| *len).sum();
    let named = |keep: fn(Option<u64>) -> bool| -> Vec<String> {
        files
            .iter()
            .filter(|(_, len)| keep(*len))
            .map(|(rel, _)| rel.clone())
            .collect()
    };
    let oversize = named(|len| len.is_some_and(|l| l > MAX_FILE_SIZE_BYTES as u64));
    let unreadable = named(|len| len.is_none());
    let nameless_world = project.scene_json.get("worldConfiguration").is_some()
        && crate::joinblock::world_name(&project.scene_json).is_none();
    Ok(DeployPreview {
        ignored,
        files,
        total_bytes,
        oversize,
        unreadable,
        main,
        collisions,
        nameless_world,
    })
}

pub struct Prepared {
    pub files: Vec<(String, String, Vec<u8>)>,
    pub pointers: Vec<String>,
    pub metadata: JsValue,
}

fn resolve_sdk_version(root: &Path) -> String {
    let mut dir = Some(root);
    while let Some(d) = dir {
        let pkg = d.join("node_modules/@dcl/sdk/package.json");
        if let Ok(raw) = std::fs::read_to_string(&pkg) {
            return serde_json::from_str::<serde_json::Value>(&raw)
                .ok()
                .and_then(|v| v.get("version")?.as_str().map(str::to_string))
                .unwrap_or_else(|| "unknown".to_string());
        }
        dir = d.parent();
    }
    "unknown".to_string()
}

pub fn build_metadata(project: &Project) -> Result<JsValue> {
    let scene_path = project.root.join("scene.json");
    let raw = std::fs::read_to_string(&scene_path)
        .with_context(|| format!("reading {}", scene_path.display()))?;
    let scene = jsjson::parse(&raw).map_err(|e| {
        anyhow::Error::from(
            UserError::new(
                format!("scene.json is not valid JSON ({e})"),
                TrySteps::one("fix the syntax at the position named above"),
            )
            .why("deploy uses a strict parser to hash-match the upstream toolchain"),
        )
    })?;
    let JsValue::Object(entries) = scene else {
        return Err(UserError::new(
            "scene.json must be a JSON object",
            TrySteps::one("wrap the contents in { \u{2026} } \u{2014} see the scene.json reference in the creator docs"),
        )
        .into());
    };
    let mut obj = vec![(
        "sdkVersion".to_string(),
        JsValue::String(resolve_sdk_version(&project.root)),
    )];
    for (k, v) in entries {
        jsjson::set(&mut obj, k, v);
    }
    Ok(JsValue::Object(obj))
}

pub fn extract_pointers(metadata: &JsValue) -> Result<Vec<String>> {
    let parcels = metadata.get("scene").and_then(|s| s.get("parcels"));
    let Some(JsValue::Array(arr)) = parcels else {
        return Err(no_parcels());
    };
    let mut out = Vec::new();
    for v in arr {
        match v.as_str() {
            Some(s) => out.push(s.to_string()),
            None => {
                return Err(UserError::new(
                    "scene.parcels entries must be strings",
                    TrySteps::one("write parcels as strings: \"0,0\" not [0,0]"),
                )
                .into())
            }
        }
    }
    if out.is_empty() {
        return Err(no_parcels());
    }
    Ok(out)
}

fn no_parcels() -> anyhow::Error {
    UserError::new(
        "scene.json declares no parcels",
        TrySteps::one("add \"scene\": { \"parcels\": [\"0,0\"], \"base\": \"0,0\" } to scene.json"),
    )
    .into()
}

pub fn world_name(metadata: &JsValue) -> Option<String> {
    metadata
        .get("worldConfiguration")
        .and_then(|w| w.get("name"))
        .and_then(|n| n.as_str())
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_string)
}

pub fn scene_title(metadata: &JsValue) -> String {
    metadata
        .get("display")
        .and_then(|d| d.get("title"))
        .and_then(|t| t.as_str())
        .unwrap_or("Untitled")
        .to_string()
}

pub fn base_parcel(metadata: &JsValue, pointers: &[String]) -> String {
    metadata
        .get("scene")
        .and_then(|s| s.get("base"))
        .and_then(|b| b.as_str())
        .map(str::to_string)
        .or_else(|| pointers.first().cloned())
        .unwrap_or_else(|| "0,0".to_string())
}

/// Every file under the release artifact root, as scene-relative paths. It
/// holds only what a release build wrote, so `.dclignore` is not consulted.
fn release_rel_files(release_root: &Path) -> Vec<String> {
    fn descend(dir: &Path, base: &Path, out: &mut Vec<String>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in rd.flatten() {
            let path = entry.path();
            if path.is_dir() {
                descend(&path, base, out);
            } else if let Ok(rel) = path.strip_prefix(base) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let mut out = Vec::new();
    descend(release_root, release_root, &mut out);
    out.sort();
    out
}

pub fn prepare(project: &Project) -> Result<Prepared> {
    let mut rel_paths = collect_publishable_files(&project.root)?;
    // rustc keeps debug and release artifacts apart, and so does this tree:
    // the watcher owns the in-place dev bundle, a deploy's production build
    // lands under RELEASE_OUT, and the payload prefers the release copy of
    // any path that has one. The two builds stop clobbering one file — and a
    // publish stops rewriting the very tree the page just fingerprinted.
    let release_root = project.root.join(crate::build::RELEASE_OUT);
    for rel in release_rel_files(&release_root) {
        if !rel_paths.contains(&rel) {
            rel_paths.push(rel);
        }
    }
    let main = project.main_output()?;
    if !rel_paths.iter().any(|r| r == &main) {
        return Err(UserError::new(
            format!("the bundle {main} does not exist yet"),
            TrySteps::one("run dcl-one-sdk build (or drop --skip-build)")
                .and(format!("check .dclignore does not exclude {main}")),
        )
        .into());
    }
    if let Some((rel, _)) = case_collisions(&rel_paths).first() {
        return Err(UserError::new(
            format!("the file {rel} collides case-insensitively with another content file"),
            TrySteps::one(
                "rename one of the two files \u{2014} content servers treat names case-insensitively",
            ),
        )
        .into());
    }
    let hashed = crate::scene::parallel_map(&rel_paths, |rel| -> Result<_> {
        let release = release_root.join(rel);
        let p = match release.is_file() {
            true => release,
            false => project.root.join(rel),
        };
        let bytes =
            std::fs::read(&p).with_context(|| format!("reading content file {}", p.display()))?;
        if bytes.len() > MAX_FILE_SIZE_BYTES {
            return Err(UserError::new(
                format!(
                    "{rel} is {}, over the 50 MB per-file limit",
                    human_size(bytes.len() as u64)
                ),
                TrySteps::one("compress or split the asset (GLB textures are usually the culprit)")
                    .and("exclude it via .dclignore if it is not needed in-world"),
            )
            .into());
        }
        let hash = hash_bytes_v1(&bytes);
        Ok((rel.clone(), hash, bytes))
    });
    let files = hashed.into_iter().collect::<Result<Vec<_>>>()?;
    let metadata = build_metadata(project)?;
    let pointers = extract_pointers(&metadata)?;
    Ok(Prepared {
        files,
        pointers,
        metadata,
    })
}

pub fn build_entity(p: &Prepared, timestamp: i64) -> Result<(String, Vec<u8>)> {
    let s = |v: &str| JsValue::String(v.to_string());
    let content = JsValue::Array(
        p.files
            .iter()
            .map(|(f, h, _)| {
                JsValue::Object(vec![("file".to_string(), s(f)), ("hash".to_string(), s(h))])
            })
            .collect(),
    );
    let pointers = JsValue::Array(p.pointers.iter().map(|v| s(v)).collect());
    let entity = JsValue::Object(vec![
        ("version".to_string(), s("v3")),
        ("type".to_string(), s("scene")),
        ("pointers".to_string(), pointers),
        ("timestamp".to_string(), JsValue::Number(timestamp as f64)),
        ("content".to_string(), content),
        ("metadata".to_string(), p.metadata.clone()),
    ]);
    let entity_bytes = jsjson::stringify(&entity)
        .map_err(|e| {
            anyhow::Error::from(
                UserError::new(
                    "scene.json contains a number this tool cannot serialize byte-identically",
                    TrySteps::one(
                        "rewrite the value in plain decimal notation within [1e-6, 1e21) in scene.json",
                    ),
                )
                .why(format!("{e}")),
            )
        })?
        .into_bytes();
    let entity_id = hash_bytes_v1(&entity_bytes);
    Ok((entity_id, entity_bytes))
}

/// The one size formatter every page and printout shares, so a payload reads
/// as the same number everywhere. Decimal units, one decimal.
pub fn human_size(bytes: u64) -> String {
    const MB: f64 = 1_000_000.0;
    const KB: f64 = 1_000.0;
    if bytes as f64 >= MB {
        format!("{:.1} MB", bytes as f64 / MB)
    } else if bytes as f64 >= KB {
        format!("{:.1} KB", bytes as f64 / KB)
    } else {
        format!("{bytes} bytes")
    }
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// A per-test scratch directory, removed on drop.
#[cfg(test)]
pub(crate) struct TempTree(pub PathBuf);

#[cfg(test)]
impl TempTree {
    pub fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "dcl-one-sdk-deploy-test-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempTree(dir)
    }

    pub fn write(&self, rel: &str, contents: &str) {
        let p = self.0.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, contents).unwrap();
    }

    pub fn write_all(&self, rels: &[&str]) {
        for rel in rels {
            self.write(rel, "x");
        }
    }
}

#[cfg(test)]
impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ux;
    use catalyrst_crypto::Wallet;

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn glob9_order_files_desc_then_dirs_desc_depth_first() {
        let t = TempTree::new("order1");
        t.write_all(&["zz.png", "z/1.png", "mid.png", "AA.png", "a/2.png"]);
        let got = collect_publishable_files(&t.0).unwrap();
        assert_eq!(
            got,
            vec!["zz.png", "mid.png", "AA.png", "z/1.png", "a/2.png"]
        );

        let t2 = TempTree::new("order2");
        t2.write_all(&["top.png", "c/m.png", "b/z.png", "b/a.png", "b/inner/q.png"]);
        let got2 = collect_publishable_files(&t2.0).unwrap();
        assert_eq!(
            got2,
            vec!["top.png", "c/m.png", "b/z.png", "b/a.png", "b/inner/q.png"]
        );
    }

    /// Every non-dot file the walk reaches is in exactly one of the two lists.
    /// The check is deliberately independent of `walk`: it enumerates the
    /// tree with no rules at all and asks the matcher directly.
    #[test]
    fn one_walk_partitions_the_tree_into_published_and_ignored() {
        let t = TempTree::new("partition");
        t.write_all(&[
            "scene.json",
            "bin/index.js",
            "bin/index.js.map",
            "README.md",
            "notes.md",
            "src/game.ts",
            "src/tex.png",
            "assets/model.glb",
            "assets/model.fbx",
            "node_modules/pkg/a.js",
            "node_modules/pkg/b.js",
            "thumbnails/t.png",
            ".hidden/x.png",
        ]);

        fn every_file(dir: &Path, root: &Path, out: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                if entry.file_name().to_string_lossy().starts_with('.') {
                    continue;
                }
                let path = entry.path();
                let rel = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                match path.is_dir() {
                    true => every_file(&path, root, out),
                    false => out.push(rel),
                }
            }
        }

        let (published, ignored) = collect_files(&t.0).unwrap();
        let gi = build_matcher(&t.0).unwrap();
        let mut all = Vec::new();
        every_file(&t.0, &t.0, &mut all);
        let mut reachable: Vec<String> = all
            .into_iter()
            .filter(|rel| {
                let parts: Vec<&str> = rel.split('/').collect();
                !(1..parts.len()).any(|n| gi.matched(parts[..n].join("/"), true).is_ignore())
            })
            .collect();
        let mut got: Vec<String> = published.iter().chain(ignored.iter()).cloned().collect();
        got.sort();
        reachable.sort();
        assert_eq!(got, reachable, "the two lists are the whole tree");
        assert!(
            published.iter().all(|p| !ignored.contains(p)),
            "and they do not overlap"
        );
        assert!(published.contains(&"assets/model.glb".to_string()));
        assert!(ignored.contains(&"assets/model.fbx".to_string()));
        assert!(
            !got.iter().any(|r| r.starts_with("node_modules/")),
            "an ignored directory is not descended into, in either direction"
        );
    }

    /// `entry.file_type()` answers about the LINK; deleting the `is_dir`
    /// fallback loses whole subtrees silently.
    #[cfg(unix)]
    #[test]
    fn a_symlinked_directory_is_still_walked_into() {
        let t = TempTree::new("symdir");
        t.write("scene.json", "{}");
        t.write("real/tex.png", "x");
        std::os::unix::fs::symlink(t.0.join("real"), t.0.join("linked")).unwrap();
        let got = collect_publishable_files(&t.0).unwrap();
        assert!(got.contains(&"linked/tex.png".to_string()), "{got:?}");
        assert!(got.contains(&"real/tex.png".to_string()), "{got:?}");
    }

    #[test]
    fn names_that_differ_only_in_case_are_paired_up() {
        assert_eq!(
            case_collisions(&strings(&["a/X.png", "b.js", "a/x.png", "a/x.PNG"])),
            vec![
                ("a/x.png".to_string(), "a/X.png".to_string()),
                ("a/x.PNG".to_string(), "a/x.png".to_string()),
            ]
        );
        assert!(case_collisions(&strings(&["a/x.png", "b/x.png"])).is_empty());
    }

    #[test]
    fn default_ignore_semantics() {
        let t = TempTree::new("ignore1");
        t.write("scene.json", "{}");
        t.write_all(&[
            "bin/index.js",
            "bin/index.js.map",
            "yarn.lock",
            "builder.json",
            "package.json",
            "package-lock.json",
            "README.md",
            "Readme.MD",
            "notes.md",
            "src/game.ts",
            "src/tex.png",
            "node_modules/foo/bar.js",
            "sub/node_modules/baz.js",
            "thumbnails/t.png",
            "dclcontext/c.json",
            "assets/model.fbx",
            "assets/model.glb",
            ".dclignore-not-really/x.png",
            ".hidden.png",
        ]);
        let got = collect_publishable_files(&t.0).unwrap();
        assert_eq!(
            got,
            vec![
                "yarn.lock",
                "scene.json",
                "builder.json",
                "src/tex.png",
                "bin/index.js",
                "assets/model.glb"
            ]
        );
    }

    /// `thumbnails` is root-anchored: the project's own thumbnails/ stays
    /// home, a thumbnails/ under any other directory is scene content.
    #[test]
    fn only_the_root_thumbnails_directory_is_ignored() {
        let t = TempTree::new("thumbs");
        t.write("scene.json", "{}");
        t.write_all(&[
            "thumbnails/asset.png",
            "images/thumbnails/casino.png",
            "models/Thumbnails/deep/x.png",
        ]);
        let got = collect_publishable_files(&t.0).unwrap();
        assert_eq!(
            got,
            vec![
                "scene.json",
                "models/Thumbnails/deep/x.png",
                "images/thumbnails/casino.png"
            ]
        );
        let gi = build_matcher(&t.0).unwrap();
        assert!(gi.matched("thumbnails", true).is_ignore());
        assert!(
            gi.matched("THUMBNAILS", true).is_ignore(),
            "case-insensitive"
        );
        assert!(!gi.matched("images/thumbnails", true).is_ignore());
        assert!(!gi
            .matched("images/thumbnails/casino.png", false)
            .is_ignore());
    }

    /// The entry point is scene.json's `main`, else the SDK7 then the SDK6
    /// default name; a loader stub adds the chunks it declares, stamped even
    /// while a chunk is still missing.
    #[test]
    fn the_bundles_are_the_entry_point_and_the_chunks_a_stub_declares() {
        let t = TempTree::new("bundle");
        assert!(built_bundles(&t.0).1.is_empty());
        t.write("bin/game.js", "six");
        let (_, bundles) = built_bundles(&t.0);
        assert_eq!(bundles[0].stamp.rel, "bin/game.js");
        assert_eq!(bundles[0].text, "six");
        t.write("bin/index.js", "seven");
        assert_eq!(built_bundles(&t.0).1[0].stamp.rel, "bin/index.js");
        t.write("scene.json", "{\"main\":\"out/main.js\"}");
        t.write("out/main.js", "custom");
        let (stamps, bundles) = built_bundles(&t.0);
        assert_eq!(bundles[0].stamp.rel, "out/main.js");
        assert_eq!(stamps.len(), 1, "an esbuild bundle declares no chunks");

        t.write(
            "out/main.js",
            &crate::split::loader_stub("out/sdk-runtime.js", None, "out/scene.js", 0, false),
        );
        t.write("out/scene.js", "scene");
        let (stamps, bundles) = built_bundles(&t.0);
        let read: Vec<&str> = bundles.iter().map(|b| b.stamp.rel.as_str()).collect();
        assert_eq!(read, vec!["out/main.js", "out/scene.js"]);
        let stamped: Vec<&str> = stamps.iter().map(|s| s.rel.as_str()).collect();
        assert_eq!(
            stamped,
            vec!["out/main.js", "out/sdk-runtime.js", "out/scene.js"]
        );
        assert_eq!(stamps[1].modified, None, "the sdk chunk is not on disk yet");
        assert_eq!(
            stub_chunk_paths(&crate::split::loader_stub(
                "bin/sdk-runtime.js",
                Some("bin/sdk-smart-items.js"),
                "bin/scene.js",
                0,
                false
            )),
            vec![
                "bin/sdk-runtime.js",
                "bin/sdk-smart-items.js",
                "bin/scene.js"
            ]
        );
    }

    #[test]
    fn a_path_is_named_only_as_a_quoted_asset_path() {
        let rel = "models/rig.fbx";
        for yes in [
            "\"models/rig.fbx\"",
            "'./models/rig.fbx'",
            "`/models/rig.fbx?v=2`",
            "x(\\\"models/rig.fbx\\\")",
            "\"models/rig.fbx#a\"",
        ] {
            assert!(names_path(yes, rel), "{yes}");
        }
        for no in [
            "// models/rig.fbx",
            "//#region models/rig.fbx",
            "\"https://cdn/models/rig.fbx\"",
            "\"models/rig.fbx.bak\"",
            "\"xmodels/rig.fbx\"",
            "\"../models/rig.fbx\"",
            "models/rig.fbx",
        ] {
            assert!(!names_path(no, rel), "{no}");
        }
    }

    /// The WARN's input: a file a built-in source-asset rule keeps out while
    /// a bundle names it as an asset path. Lines from the project's own
    /// `.dclignore` are its choice; developer files a bundle names — a
    /// bundler's module-path comment, a `require('../package.json')`, even a
    /// quoted README — are never fetched by the explorer.
    #[test]
    fn source_assets_a_bundle_names_are_listed_developer_files_never() {
        let t = TempTree::new("named");
        t.write("scene.json", "{\"main\":\"bin/game.js\"}");
        t.write(".dclignore", "*.zip\n");
        t.write(
            "bin/game.js",
            concat!(
                "// src/index.ts\n",
                "//#region src/loop.ts\n",
                "eval(\"f.exports={version:e(\\\"../package.json\\\").version}\");\n",
                "load('README.md');load('images/Help.MD');\n",
                "load('assets/Model.FBX');load('packs/level.zip');\n",
                "load('https://cdn/art/logo.blend');\n",
            ),
        );
        t.write_all(&[
            "src/index.ts",
            "src/loop.ts",
            "package.json",
            "README.md",
            "images/help.md",
            "assets/model.fbx",
            "assets/other.fbx",
            "packs/level.zip",
            "art/logo.blend",
        ]);
        let gi = build_matcher(&t.0).unwrap();
        let (_, ignored) = collect_files(&t.0).unwrap();
        for dev in [
            "src/index.ts",
            "package.json",
            "README.md",
            "images/help.md",
        ] {
            assert!(ignored.contains(&dev.to_string()), "{dev}");
            assert!(!kept_out_by_source_asset_default(&gi, dev), "{dev}");
        }
        assert!(kept_out_by_source_asset_default(&gi, "assets/model.fbx"));
        assert!(kept_out_by_source_asset_default(&gi, "art/logo.blend"));
        assert!(
            !kept_out_by_source_asset_default(&gi, "packs/level.zip"),
            "the project's own rule"
        );
        let candidates: Vec<String> = ignored
            .iter()
            .filter(|rel| kept_out_by_source_asset_default(&gi, rel))
            .cloned()
            .collect();
        let (_, bundles) = built_bundles(&t.0);
        assert_eq!(
            named_by_bundles(&candidates, &bundles),
            vec![("assets/model.fbx".to_string(), "bin/game.js".to_string())]
        );
    }

    /// One WARN per change: the same tree and bundles never re-warn, a
    /// rebuilt chunk naming the same files does not either, and a chunk that
    /// stops naming them then names them again does. The scene code lives
    /// in the chunk the stub declares, not in the entry point.
    #[test]
    fn the_warn_fires_once_per_change() {
        let t = TempTree::new("rescan");
        t.write("scene.json", "{\"main\":\"bin/index.js\"}");
        t.write(
            "bin/index.js",
            &crate::split::loader_stub("bin/sdk-runtime.js", None, "bin/scene.js", 0, false),
        );
        t.write("bin/scene.js", "load('models/rig.fbx')");
        t.write_all(&["models/rig.fbx", "models/rig.glb"]);
        let gi = build_matcher(&t.0).unwrap();
        let (mut out, mut ignored) = (Vec::new(), Vec::new());
        walk(&t.0, &t.0, &gi, &mut out, &mut ignored);
        let named = vec![("models/rig.fbx".to_string(), "bin/scene.js".to_string())];
        assert_eq!(
            changed_named_by_bundles(&t.0, &gi, &ignored),
            Some(named.clone())
        );
        assert_eq!(
            changed_named_by_bundles(&t.0, &gi, &ignored),
            None,
            "nothing moved"
        );
        t.write("bin/scene.js", "load('models/rig.fbx');/* rebuilt */");
        assert_eq!(
            changed_named_by_bundles(&t.0, &gi, &ignored),
            None,
            "rebuilt, same list"
        );
        t.write("bin/scene.js", "load('models/rig.glb');");
        assert_eq!(
            changed_named_by_bundles(&t.0, &gi, &ignored),
            None,
            "nothing named"
        );
        t.write(
            "bin/scene.js",
            "load('models/rig.fbx');load('models/rig.glb');",
        );
        assert_eq!(changed_named_by_bundles(&t.0, &gi, &ignored), Some(named));
    }

    #[test]
    fn user_dclignore_lines_are_respected() {
        let t = TempTree::new("ignore2");
        t.write(".dclignore", "ignored-dir\n*.secret\n\n");
        t.write("scene.json", "{}");
        t.write_all(&[
            "bin/index.js",
            "ignored-dir/x.txt",
            "top.secret",
            "keep.txt",
        ]);
        let got = collect_publishable_files(&t.0).unwrap();
        assert_eq!(got, vec!["scene.json", "keep.txt", "bin/index.js"]);
    }

    #[test]
    fn dry_run_entity_is_frozen() {
        let t = TempTree::new("golden");
        t.write(
            "scene.json",
            "{\"runtimeVersion\":\"7\",\"main\":\"bin/index.js\",\"display\":{\"title\":\"Parity Guard\"},\"scene\":{\"parcels\":[\"52,-52\",\"52,-53\"],\"base\":\"52,-52\"}}",
        );
        t.write("bin/index.js", "console.log(\"golden\");\n");
        t.write("assets/Model.glb", "GLBBINARYFIXTURE0123456789");
        t.write("notes.md", "not deployed");
        let project = Project::load(&t.0).unwrap();
        let prepared = prepare(&project).unwrap();
        let (entity_id, _) = build_entity(&prepared, 1751900000000).unwrap();
        assert_eq!(
            entity_id,
            "bafkreigndax3hlj5fa4alog7573u5jvoo2lqxwdlsvfths2pdcvrg2veae"
        );
        let listing: Vec<(String, String)> = prepared
            .files
            .iter()
            .map(|(f, h, _)| (f.clone(), h.clone()))
            .collect();
        assert_eq!(
            listing,
            vec![
                (
                    "scene.json".to_string(),
                    "bafkreifhurehzptgrhsjgb3ey6ugoohxf7xcok4jiy2sxlsgkasubry2ya".to_string()
                ),
                (
                    "bin/index.js".to_string(),
                    "bafkreiabpuwsr4w2yzatq6gygbtpx7coohgpsg7tve3msd55odi6b2r5om".to_string()
                ),
                (
                    "assets/Model.glb".to_string(),
                    "bafkreiczplgxt7awmu3kwydlegs266nsooijxjc7svtgy6rkrgia65fft4".to_string()
                ),
            ]
        );
    }

    #[test]
    fn out_of_range_number_maps_to_user_error() {
        let t = TempTree::new("bignum");
        t.write(
            "scene.json",
            "{\"runtimeVersion\":\"7\",\"main\":\"bin/index.js\",\"display\":{\"title\":\"X\",\"big\":1e21},\"scene\":{\"parcels\":[\"0,0\"],\"base\":\"0,0\"}}",
        );
        t.write("bin/index.js", "console.log(\"x\");\n");
        let project = Project::load(&t.0).unwrap();
        let prepared = prepare(&project).unwrap();
        let err = build_entity(&prepared, 1751900000000).unwrap_err();
        let rendered = ux::render(&err, false, false);
        assert!(
            rendered.contains("cannot serialize byte-identically"),
            "rendered: {rendered}"
        );
        assert!(
            rendered.lines().any(|l| l
                .trim_start()
                .starts_with("\u{2192} try: rewrite the value in plain decimal")),
            "rendered: {rendered}"
        );
        assert!(!rendered.contains("caused by:"), "rendered: {rendered}");
    }

    #[test]
    fn world_metadata_helpers() {
        let meta = jsjson::parse(
            "{\"display\":{\"title\":\"My World\"},\"scene\":{\"parcels\":[\"0,0\"],\"base\":\"0,0\"},\"worldConfiguration\":{\"name\":\"Example.dcl.eth\"}}",
        )
        .unwrap();
        assert_eq!(world_name(&meta).as_deref(), Some("Example.dcl.eth"));
        assert_eq!(scene_title(&meta), "My World");
        assert_eq!(base_parcel(&meta, &["9,9".to_string()]), "0,0");
        let bare = jsjson::parse("{}").unwrap();
        assert_eq!(world_name(&bare), None);
        assert_eq!(scene_title(&bare), "Untitled");
        assert_eq!(base_parcel(&bare, &["9,9".to_string()]), "9,9");
        let blank = jsjson::parse("{\"worldConfiguration\":{\"name\":\"\"}}").unwrap();
        assert_eq!(world_name(&blank), None);
        let padded = jsjson::parse("{\"worldConfiguration\":{\"name\":\" x.dcl.eth \"}}").unwrap();
        assert_eq!(world_name(&padded).as_deref(), Some("x.dcl.eth"));
    }

    #[test]
    fn delete_payload_shape_matches_upstream() {
        let p = build_delete_payload("MyWorld.dcl.eth");
        assert!(p.starts_with("delete:/entities/myworld.dcl.eth:"));
        assert!(p.ends_with(":{}"));
        let parts: Vec<&str> = p.split(':').collect();
        assert_eq!(parts.len(), 4);
        assert!(parts[2].chars().all(|c| c.is_ascii_digit()));
        assert_eq!(p, p.to_lowercase());
    }

    #[test]
    fn rotation_defaults_to_the_public_network_and_yields_to_the_env() {
        let mut public = vec![DEFAULT_GENESIS_TARGET_SERVER.to_string()];
        public.extend(strings(&UPSTREAM_CATALYST_HOSTS));

        std::env::remove_var("DCL_ONE_SDK_CATALYST_ROTATION");
        assert_eq!(configured_catalyst_rotation(), None);
        assert_eq!(catalyst_rotation(), public);
        assert_eq!(
            catalyst_rotation().first().unwrap(),
            DEFAULT_GENESIS_TARGET_SERVER,
            "this build's own default is tried first"
        );

        std::env::set_var(
            "DCL_ONE_SDK_CATALYST_ROTATION",
            " https://catalyst.example.com/ , ,https://second.example.com ",
        );
        let configured = configured_catalyst_rotation().unwrap();
        assert_eq!(
            configured,
            strings(&["https://catalyst.example.com", "https://second.example.com"])
        );
        assert_eq!(catalyst_rotation(), configured);

        std::env::set_var("DCL_ONE_SDK_CATALYST_ROTATION", "  ");
        assert_eq!(configured_catalyst_rotation(), None);
        assert_eq!(catalyst_rotation(), public);

        std::env::remove_var("DCL_ONE_SDK_CATALYST_ROTATION");
    }

    #[test]
    fn network_scope_note_fires_only_off_the_upstream_rotation() {
        assert_eq!(
            non_upstream_note("https://peer-ec2.decentraland.org/content"),
            None
        );
        assert_eq!(
            non_upstream_note("https://interconnected.online/content"),
            None
        );
        let dclone = non_upstream_note("https://catalyst.example.com/content").unwrap();
        assert!(
            dclone.contains("publishing to catalyst.example.com"),
            "{dclone}"
        );
        assert!(
            dclone.contains("not Genesis City on decentraland.org"),
            "{dclone}"
        );
        let local = non_upstream_note("http://127.0.0.1:5198/content").unwrap();
        assert!(local.contains("127.0.0.1:5198"), "{local}");
    }

    #[test]
    fn base_url_path_extraction() {
        assert_eq!(net::url_path("http://127.0.0.1:5198/content"), "/content");
        assert_eq!(net::url_path("http://127.0.0.1:5142"), "");
        assert_eq!(
            net::url_path("https://catalyst.example.com/content"),
            "/content"
        );
    }

    #[test]
    fn segment_encoding_is_uri_component_like() {
        assert_eq!(encode_segment("my-world.dcl.eth"), "my-world.dcl.eth");
        assert_eq!(encode_segment("a b/c"), "a%20b%2Fc");
    }

    #[test]
    fn other_parcel_scenes_are_detected() {
        let scene = |title: &str, parcels: &[&str]| WorldScene {
            title: title.into(),
            parcels: strings(parcels),
            timestamp: None,
            content_hashes: vec![],
            size: None,
        };
        let existing = vec![scene("same", &["0,0", "0,1"]), scene("other", &["5,5"])];
        let others = scenes_on_other_parcels(&existing, &strings(&["0,0", "0,1"]));
        assert_eq!(others.len(), 1);
        assert_eq!(others[0].title, "other");
    }

    #[test]
    fn catalyst_url_sanitizing_prepends_https() {
        assert_eq!(
            sanitize_catalyst_url("peer.decentraland.org/"),
            "https://peer.decentraland.org"
        );
        assert_eq!(
            sanitize_catalyst_url("http://127.0.0.1:5142"),
            "http://127.0.0.1:5142"
        );
    }

    #[test]
    fn sign_key_flag_wins_over_env_private_key() {
        const KEY_FLAG: &str = "0000000000000000000000000000000000000000000000000000000000000001";
        const KEY_ENV: &str = "0000000000000000000000000000000000000000000000000000000000000002";
        let addr_flag = Wallet::from_hex(KEY_FLAG).unwrap().address();
        let addr_env = Wallet::from_hex(KEY_ENV).unwrap().address();
        assert_ne!(addr_flag, addr_env);
        let t = TempTree::new("signerprec");
        t.write("key.txt", KEY_FLAG);
        let key_path = t.0.join("key.txt");
        std::env::set_var("DCL_PRIVATE_KEY", KEY_ENV);
        let picked = load_signer(Some(&key_path)).unwrap().unwrap();
        assert_eq!(picked.address(), addr_flag);
        let picked_env = load_signer(None).unwrap().unwrap();
        assert_eq!(picked_env.address(), addr_env);
        std::env::remove_var("DCL_PRIVATE_KEY");
        assert!(load_signer(None).unwrap().is_none());
        let picked_flag_only = load_signer(Some(&key_path)).unwrap().unwrap();
        assert_eq!(picked_flag_only.address(), addr_flag);
    }

    /// A release artifact shadows its dev-tree twin, a release-only chunk
    /// still ships, and everything else reads from the tree.
    #[test]
    fn release_artifacts_shadow_the_dev_tree_in_prepare() {
        let t = TempTree::new("release");
        t.write(
            "scene.json",
            "{\"runtimeVersion\":\"7\",\"main\":\"bin/index.js\",\"display\":{\"title\":\"P\"},\"scene\":{\"parcels\":[\"0,0\"],\"base\":\"0,0\"}}",
        );
        t.write("bin/index.js", "dev");
        t.write("asset.glb", "asset");
        t.write(".dcl-one/release/bin/index.js", "release");
        t.write(".dcl-one/release/bin/scene.js", "release-only");
        let project = Project::load(&t.0).unwrap();
        let prepared = prepare(&project).unwrap();
        let bytes = |rel: &str| {
            prepared
                .files
                .iter()
                .find(|(r, _, _)| r == rel)
                .map(|(_, _, b)| b.clone())
                .unwrap_or_else(|| panic!("{rel} missing from the payload"))
        };
        assert_eq!(
            bytes("bin/index.js").as_slice(),
            b"release",
            "the release copy wins"
        );
        assert_eq!(
            bytes("bin/scene.js").as_slice(),
            b"release-only",
            "a release-only chunk still ships"
        );
        assert_eq!(
            bytes("asset.glb").as_slice(),
            b"asset",
            "the tree serves the rest"
        );
        assert!(
            !prepared
                .files
                .iter()
                .any(|(r, _, _)| r.contains(".dcl-one")),
            "artifact paths never leak into the payload listing"
        );
    }
}
