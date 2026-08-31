mod net;
mod run;
mod unpublish;

#[cfg(test)]
pub(crate) use net::ENV_LOCK;

pub use net::{
    build_delete_payload, encode_segment, enforce_world_permission, env_default_target,
    jump_in_url, non_upstream_note, play_url, sanitize_catalyst_url, scenes_on_other_parcels,
    send_world_delete, simple_auth_chain, sticky_default_target, upload_entity, WorldScene,
    WORLDS_CONTENT_SERVER,
};
pub(crate) use net::{
    client, denied_parcels_in, deployment_permission_in_doc, entity_content_hashes, entity_title,
    host_of, parse_world_scenes, unreachable_server, DocAnswer,
};
pub use run::{deploy, load_signer};
pub use unpublish::{unpublish, UnpublishOptions};

use crate::jsjson::{self, JsValue};
use crate::scene::Project;
use crate::ux::{TrySteps, UserError};
use anyhow::{Context, Result};
use catalyrst_hashing::hash_bytes_v1;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
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
    pub yes: bool,
    pub no_browser: bool,
    pub ci: bool,
    pub port: Option<u16>,
    /// A caller that hosts the signing routes on its own server — the preview
    /// server's `/deploy/sign/` — so a page-driven deploy binds no second
    /// listener. `None` (the CLI) serves the signing page itself.
    pub host_signer: Option<crate::linker::HostSigner>,
    /// No terminal narration — build steps, file listing, target note, jump
    /// link. A page-driven publish tells its whole story on the page, and its
    /// prints would land in the preview terminal as noise. Errors still print.
    pub quiet: bool,
    /// A delegated identity that signs this deploy with no wallet prompt: the
    /// ephemeral key the Connect-with-DCL flow minted, kept in memory. When
    /// set (and unexpired), the deploy signs headlessly with it instead of
    /// hosting a browser signing page.
    pub identity: Option<DeployIdentity>,
}

/// The in-memory delegated identity: a throwaway key the wallet authorized
/// once, and the proof it did. It signs deploys as the wallet until the
/// delegation expires — the wallet itself is not asked again.
#[derive(Clone)]
pub struct DeployIdentity {
    /// The wallet the deploy publishes as.
    pub signer: String,
    /// The ephemeral private key, hex. In memory only — never written.
    pub ephemeral_key: String,
    /// The exact `Decentraland Login\n…` text the wallet signed.
    pub delegation_payload: String,
    /// The wallet's signature over that text.
    pub delegation_signature: String,
    /// When the delegation lapses (ms). Past it, the identity is dropped and
    /// the deploy falls back to the wallet.
    pub expiration_ms: i64,
}

impl DeployIdentity {
    pub fn expired(&self, now_ms: i64) -> bool {
        now_ms >= self.expiration_ms
    }
}

const MAX_FILE_SIZE_BYTES: usize = 50_000_000;

/// The hosts that make up the public Genesis City network: both the classifier
/// behind `non_upstream_note` and the rotation `deploy` falls back to, because
/// publishing a scene there is what this CLI is for.
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

/// The rotation named by DCL_ONE_SDK_CATALYST_ROTATION (comma-separated), or
/// `None` when the caller never named one. Callers that must not reach a
/// public catalyst on their own read this rather than `catalyst_rotation`.
pub fn configured_catalyst_rotation() -> Option<Vec<String>> {
    let rotation: Vec<String> = std::env::var("DCL_ONE_SDK_CATALYST_ROTATION")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .collect();
    (!rotation.is_empty()).then_some(rotation)
}

/// Catalysts `deploy` picks from when given no target. Defaults to the public
/// network; the implicit choice is announced and confirmed at the call site,
/// so it is never silent.
pub fn catalyst_rotation() -> Vec<String> {
    configured_catalyst_rotation().unwrap_or_else(|| {
        UPSTREAM_CATALYST_HOSTS
            .iter()
            .map(|h| h.to_string())
            .collect()
    })
}

const DEFAULT_DCL_IGNORE: [&str; 27] = [
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
    // The SDK's own AI-context install and its skill docs — never scene
    // runtime, and their prose/markup is upload bloat at best.
    "sdk-skills",
    "**/*.ts",
    "**/*.tsx",
    "Dockerfile",
    "thumbnails",
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
    "*.blend",
    "*.fbx",
    "*.zip",
    "*.rar",
    "*.map",
];

const EXTRA_DCL_IGNORE: [&str; 6] = [
    ".*",
    "node_modules",
    "**/*.ts",
    "**/*.tsx",
    "node_modules/**",
    "*.md",
];

pub fn dcl_ignore_patterns(root: &Path) -> Vec<String> {
    let user = std::fs::read_to_string(root.join(".dclignore")).ok();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let user_lines = user.as_deref().map(|s| s.split('\n').collect::<Vec<_>>());
    for p in user_lines
        .unwrap_or_default()
        .into_iter()
        .chain(DEFAULT_DCL_IGNORE)
        .chain(EXTRA_DCL_IGNORE)
    {
        if !p.is_empty() && seen.insert(p.to_string()) {
            out.push(p.to_string());
        }
    }
    out
}

fn build_matcher(root: &Path) -> Result<Gitignore> {
    let mut b = GitignoreBuilder::new(root);
    b.case_insensitive(true).context("matcher options")?;
    for p in dcl_ignore_patterns(root) {
        b.add_line(None, &p).map_err(|e| {
            anyhow::Error::from(
                UserError::new(
                    format!(".dclignore line {p:?} is not a valid pattern"),
                    TrySteps::one("fix or delete that line (gitignore syntax)"),
                )
                .caused_by(e),
            )
        })?;
    }
    b.build().context("building ignore matcher")
}

/// `readdir` already answered this on every platform this ships to, so the
/// stat behind `Path::is_dir` is only paid for the entries where `d_type` is
/// genuinely unknown — and for symlinks, whose own type says nothing about
/// what they point at (a symlinked directory has always been descended into).
fn entry_is_dir(entry: &std::fs::DirEntry, path: &Path) -> bool {
    match entry.file_type() {
        Ok(ft) if !ft.is_symlink() => ft.is_dir(),
        _ => path.is_dir(),
    }
}

/// One walk, two lists: what a deploy would upload and what `.dclignore` kept
/// out of it. They are produced together because they are the same decision
/// read in both directions — a second, hand-inverted walk drifts the moment
/// this one gains a rule, and silently reports a partition that is not one.
///
/// An ignored DIRECTORY is not descended into, so nothing under it lands in
/// either list. Dot-entries are skipped outright: they are never publishable,
/// so calling them "excluded by .dclignore" would report a decision nobody
/// made.
fn walk(dir: &Path, root: &Path, gi: &Gitignore, out: &mut Vec<String>, ignored: &mut Vec<String>) {
    let rd = match std::fs::read_dir(dir) {
        Ok(x) => x,
        Err(_) => return,
    };
    let mut files: Vec<(String, String)> = Vec::new();
    let mut dirs: Vec<(String, PathBuf)> = Vec::new();
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let rel = match path.strip_prefix(root) {
            Ok(r) => r.to_string_lossy().replace('\\', "/"),
            Err(_) => continue,
        };
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
    for (_, rel) in files {
        out.push(rel);
    }
    for (_, path) in dirs {
        walk(&path, root, gi, out, ignored);
    }
}

pub fn collect_publishable_files(root: &Path) -> Result<Vec<String>> {
    Ok(collect_files(root)?.0)
}

/// The publishable files and the ignored ones, from a single walk.
fn collect_files(root: &Path) -> Result<(Vec<String>, Vec<String>)> {
    let gi = build_matcher(root)?;
    let (mut out, mut ignored) = (Vec::new(), Vec::new());
    walk(root, root, &gi, &mut out, &mut ignored);
    Ok((out, ignored))
}

/// What a deploy would upload, answered without reading a byte of it.
///
/// [`prepare`] reads and hashes every file, which is the right thing when the
/// bytes are about to be signed and sent, and the wrong thing for a page that
/// renders on every refresh: a scene with a few hundred megabytes of GLBs
/// would re-hash all of it. The walk and the `.dclignore` rules here are the
/// same ones `prepare` uses, so the file list matches; only the sizes are
/// taken from the directory entry instead.
pub struct DeployPreview {
    /// Publishable files, largest first. The size is `None` when the directory
    /// entry could not be stat'd — see [`DeployPreview::unreadable`].
    pub files: Vec<(String, Option<u64>)>,
    /// The sum of the sizes that could be read. Files in `unreadable`
    /// contribute nothing, because nothing about them is known.
    pub total_bytes: u64,
    /// Files `.dclignore` keeps out of the upload, from directories that are
    /// themselves published. An ignored DIRECTORY is not descended into, so
    /// this counts the texture somebody excluded by accident and not the
    /// seventeen thousand files under node_modules — a number that is true,
    /// useless, and alarming.
    pub ignored: Vec<String>,
    /// Files over the per-file limit, which `prepare` would refuse. Named here
    /// so the answer arrives before the wallet prompt rather than after it.
    pub oversize: Vec<String>,
    /// Publishable files whose size could not be read — most often a dangling
    /// symlink, which the walk sees as a non-directory and therefore publishes.
    /// `prepare` reads every file, so these abort the deploy *after* the wallet
    /// prompt: reporting them as 0 bytes would hide the exact failure this page
    /// exists to move earlier.
    pub unreadable: Vec<String>,
    /// Whether the bundle `prepare` refuses to deploy without is in the walk.
    pub main: MainBundle,
    /// Pairs that differ only in case. A content server treats names
    /// case-insensitively, so `prepare` refuses them.
    pub collisions: Vec<(String, String)>,
}

/// The state of the one file a scene cannot be published without — the
/// `prepare` refusal a real deploy hits most often, because "I have not run
/// build yet" is the most common reason a deploy fails.
#[derive(Debug, PartialEq, Eq)]
pub enum MainBundle {
    /// Declared by scene.json and present in the payload.
    Present(String),
    /// Declared, but the walk did not find it: not built, or `.dclignore`
    /// excludes it.
    Missing(String),
    /// scene.json's `"main"` is itself unusable; the string says why.
    Unusable(String),
}

/// Names a content server would read as one file, paired with the name they
/// collide with — the same refusal `prepare` raises, which is only actionable
/// once you know which two files it means. Kept off the filesystem on purpose:
/// the pair cannot even exist on a case-insensitive volume, where this would
/// otherwise be untestable.
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
        .map(|rel| (rel.clone(), std::fs::metadata(root.join(rel)).ok()))
        .map(|(rel, meta)| (rel, meta.map(|m| m.len())))
        .collect();
    files.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let total_bytes = files.iter().filter_map(|(_, len)| *len).sum();
    let oversize = files
        .iter()
        .filter(|(_, len)| len.is_some_and(|l| l > MAX_FILE_SIZE_BYTES as u64))
        .map(|(rel, _)| rel.clone())
        .collect();
    let unreadable = files
        .iter()
        .filter(|(_, len)| len.is_none())
        .map(|(rel, _)| rel.clone())
        .collect();
    Ok(DeployPreview {
        ignored,
        files,
        total_bytes,
        oversize,
        unreadable,
        main,
        collisions,
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
                .and_then(|v| {
                    v.get("version")
                        .and_then(|x| x.as_str())
                        .map(str::to_string)
                })
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

/// Every file under the release artifact root, as scene-relative paths. The
/// dir holds only what a release build wrote (bundle chunks under `bin/`),
/// so the walk is a handful of entries and needs none of `.dclignore`.
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

    let mut seen_lower = HashSet::new();
    for rel in &rel_paths {
        if !seen_lower.insert(rel.to_lowercase()) {
            return Err(UserError::new(
                format!("the file {rel} collides case-insensitively with another content file"),
                TrySteps::one(
                    "rename one of the two files \u{2014} content servers treat names case-insensitively",
                ),
            )
            .into());
        }
    }
    // Read+hash in parallel; results stay in rel_paths order, so the first
    // failing file (by that order) is still the one reported.
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
    let mut files = Vec::with_capacity(hashed.len());
    for entry in hashed {
        files.push(entry?);
    }

    let metadata = build_metadata(project)?;
    let pointers = extract_pointers(&metadata)?;

    Ok(Prepared {
        files,
        pointers,
        metadata,
    })
}

pub fn build_entity(p: &Prepared, timestamp: i64) -> Result<(String, Vec<u8>)> {
    let content = JsValue::Array(
        p.files
            .iter()
            .map(|(f, h, _)| {
                JsValue::Object(vec![
                    ("file".to_string(), JsValue::String(f.clone())),
                    ("hash".to_string(), JsValue::String(h.clone())),
                ])
            })
            .collect(),
    );
    let pointers = JsValue::Array(
        p.pointers
            .iter()
            .map(|s| JsValue::String(s.clone()))
            .collect(),
    );
    let entity = JsValue::Object(vec![
        ("version".to_string(), JsValue::String("v3".to_string())),
        ("type".to_string(), JsValue::String("scene".to_string())),
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

/// The one size formatter every page and printout shares — a payload must
/// read as the same number on the sign panel, the /deploy hint and the
/// /target datum. Decimal units, one decimal, no six-digit byte counts.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ux;
    use catalyrst_crypto::Wallet;

    struct TempTree(PathBuf);

    impl TempTree {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "dcl-one-sdk-deploy-test-{tag}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TempTree(dir)
        }

        fn write(&self, rel: &str, contents: &str) {
            let p = self.0.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, contents).unwrap();
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn glob9_order_files_desc_then_dirs_desc_depth_first() {
        let t = TempTree::new("order1");
        for f in ["zz.png", "z/1.png", "mid.png", "AA.png", "a/2.png"] {
            t.write(f, "x");
        }
        let got = collect_publishable_files(&t.0).unwrap();
        assert_eq!(
            got,
            vec!["zz.png", "mid.png", "AA.png", "z/1.png", "a/2.png"]
        );

        let t2 = TempTree::new("order2");
        for f in ["top.png", "c/m.png", "b/z.png", "b/a.png", "b/inner/q.png"] {
            t2.write(f, "x");
        }
        let got2 = collect_publishable_files(&t2.0).unwrap();
        assert_eq!(
            got2,
            vec!["top.png", "c/m.png", "b/z.png", "b/a.png", "b/inner/q.png"]
        );
    }

    /// Every non-dot file the walk reaches is in exactly one of the two lists.
    /// The lists used to come from two walks — one written forwards and one by
    /// hand backwards — with nothing holding them to the same tree, so a rule
    /// added to one and not the other would have made the page quietly lie.
    /// The check below is deliberately independent of `walk`: it enumerates
    /// the tree with no rules at all and asks the matcher directly.
    #[test]
    fn one_walk_partitions_the_tree_into_published_and_ignored() {
        let t = TempTree::new("partition");
        for f in [
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
        ] {
            t.write(f, "x");
        }
        t.write(".hidden/x.png", "x");

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

    /// `entry.file_type()` answers "directory?" from readdir, but it answers
    /// it about the LINK, and a symlinked directory has always been walked
    /// into. Deleting that fallback loses whole subtrees silently.
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

    /// A content server matches names case-insensitively, so these two are one
    /// file to it and `prepare` refuses them. They cannot both exist on a
    /// case-insensitive volume, which is why the check is over names.
    #[test]
    fn names_that_differ_only_in_case_are_paired_up() {
        let rels = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            case_collisions(&rels(&["a/X.png", "b.js", "a/x.png", "a/x.PNG"])),
            vec![
                ("a/x.png".to_string(), "a/X.png".to_string()),
                ("a/x.PNG".to_string(), "a/x.png".to_string()),
            ]
        );
        assert!(case_collisions(&rels(&["a/x.png", "b/x.png"])).is_empty());
    }

    #[test]
    fn default_ignore_semantics() {
        let t = TempTree::new("ignore1");
        t.write("scene.json", "{}");
        t.write("bin/index.js", "x");
        t.write("bin/index.js.map", "x");
        t.write("yarn.lock", "x");
        t.write("builder.json", "x");
        t.write("package.json", "x");
        t.write("package-lock.json", "x");
        t.write("README.md", "x");
        t.write("Readme.MD", "x");
        t.write("notes.md", "x");
        t.write("src/game.ts", "x");
        t.write("src/tex.png", "x");
        t.write("node_modules/foo/bar.js", "x");
        t.write("sub/node_modules/baz.js", "x");
        t.write("thumbnails/t.png", "x");
        t.write("dclcontext/c.json", "x");
        t.write("assets/model.fbx", "x");
        t.write("assets/model.glb", "x");
        t.write(".dclignore-not-really/x.png", "x");
        t.write(".hidden.png", "x");
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

    #[test]
    fn user_dclignore_lines_are_respected() {
        let t = TempTree::new("ignore2");
        t.write(".dclignore", "ignored-dir\n*.secret\n\n");
        t.write("scene.json", "{}");
        t.write("bin/index.js", "x");
        t.write("ignored-dir/x.txt", "x");
        t.write("top.secret", "x");
        t.write("keep.txt", "x");
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
        let public: Vec<String> = UPSTREAM_CATALYST_HOSTS
            .iter()
            .map(|h| h.to_string())
            .collect();

        std::env::remove_var("DCL_ONE_SDK_CATALYST_ROTATION");
        assert_eq!(configured_catalyst_rotation(), None);
        assert_eq!(catalyst_rotation(), public);

        std::env::set_var(
            "DCL_ONE_SDK_CATALYST_ROTATION",
            " https://catalyst.example.com/ , ,https://second.example.com ",
        );
        let configured = configured_catalyst_rotation().unwrap();
        assert_eq!(
            configured,
            vec![
                "https://catalyst.example.com".to_string(),
                "https://second.example.com".to_string()
            ]
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
        let existing = vec![
            WorldScene {
                title: "same".into(),
                parcels: vec!["0,0".into(), "0,1".into()],
                timestamp: None,
                content_hashes: vec![],
                size: None,
            },
            WorldScene {
                title: "other".into(),
                parcels: vec!["5,5".into()],
                timestamp: None,
                content_hashes: vec![],
                size: None,
            },
        ];
        let deploying = vec!["0,0".to_string(), "0,1".to_string()];
        let others = scenes_on_other_parcels(&existing, &deploying);
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

    /// rustc-style profiles: a release artifact shadows its dev-tree twin in
    /// the payload, a release-only chunk still ships, and everything without
    /// a release copy reads from the tree as ever.
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
