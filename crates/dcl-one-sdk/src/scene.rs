use crate::ux::{TrySteps, UserError};
use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Project {
    pub root: PathBuf,
    pub scene_json: Value,
}

impl Project {
    pub fn load(dir: &Path) -> Result<Self> {
        if !dir.is_dir() {
            return Err(UserError::new(
                format!("the directory {} does not exist", dir.display()),
                TrySteps::one("check the path passed to --dir")
                    .and("run the command from inside your scene folder"),
            )
            .into());
        }
        let root = dunce::canonicalize(dir)
            .with_context(|| format!("resolving project dir {}", dir.display()))?;
        let scene_path = root.join("scene.json");
        if !scene_path.is_file() {
            if root.join(crate::workspace::WORKSPACE_FILE).is_file() {
                return Err(UserError::new(
                    "this directory is a workspace root, not a single scene",
                    TrySteps::one(
                        "run this command from inside one of the folders listed in dcl-workspace.json",
                    )
                    .and("build and start understand workspaces \u{2014} run them here to cover every member"),
                )
                .why(format!(
                    "{} exists but scene.json does not",
                    root.join(crate::workspace::WORKSPACE_FILE).display()
                ))
                .into());
            }
            return Err(UserError::new(
                "this directory is not a Decentraland scene",
                TrySteps::one("cd into your scene folder, or pass --dir <path>")
                    .and("start a new scene with: dcl-one-sdk init"),
            )
            .why(format!("no scene.json in {}", root.display()))
            .into());
        }
        let bytes = std::fs::read(&scene_path)
            .with_context(|| format!("reading {}", scene_path.display()))?;
        let scene_json: Value = serde_json::from_slice(&bytes).map_err(|e| {
            UserError::new(
                format!(
                    "scene.json is not valid JSON (line {}, column {})",
                    e.line(),
                    e.column()
                ),
                TrySteps::one(format!(
                    "fix the syntax error at scene.json:{}:{}",
                    e.line(),
                    e.column()
                ))
                .and("validate the file with a JSON linter"),
            )
            .caused_by(e)
        })?;
        let runtime = scene_json
            .get("runtimeVersion")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if runtime != "7" {
            let why = if runtime.is_empty() {
                "scene.json has no runtimeVersion; this tool builds SDK 7 scenes only".to_string()
            } else {
                format!(
                    "scene.json runtimeVersion is \"{runtime}\"; this tool builds SDK 7 scenes only"
                )
            };
            return Err(UserError::new(
                "this scene targets SDK 6, which dcl-one-sdk cannot build",
                TrySteps::one("follow the SDK 7 migration guide in the creator docs")
                    .and("after migrating, set \"runtimeVersion\": \"7\" in scene.json"),
            )
            .why(why)
            .into());
        }
        if let Some(warning) = min_cli_warning(&root) {
            tracing::warn!("{warning}");
        }
        Ok(Self { root, scene_json })
    }

    pub fn main_output(&self) -> Result<String> {
        let main = self
            .scene_json
            .get("main")
            .and_then(|m| m.as_str())
            .unwrap_or_default();
        if main.is_empty() {
            return Err(UserError::new(
                "scene.json is missing \"main\"",
                TrySteps::one("add \"main\": \"bin/index.js\" to scene.json"),
            )
            .why("\"main\" names the bundle file the explorer loads")
            .into());
        }
        if self.root.join(main).is_dir() {
            return Err(UserError::new(
                format!("scene.json \"main\" points at \"{main}\", which is a directory"),
                TrySteps::one("set \"main\": \"bin/index.js\" in scene.json"),
            )
            .why("\"main\" must be the bundle output file")
            .into());
        }
        if !main.ends_with(".js") {
            return Err(UserError::new(
                format!("scene.json \"main\" must be a .js bundle path (got \"{main}\")"),
                TrySteps::one("set \"main\": \"bin/index.js\" in scene.json"),
            )
            .into());
        }
        Ok(main.to_string())
    }

    pub fn parcels(&self) -> Vec<String> {
        self.scene_json
            .get("scene")
            .and_then(|s| s.get("parcels"))
            .and_then(|p| p.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn node_module(&self, rel: &str) -> Option<PathBuf> {
        let p = self.root.join("node_modules").join(rel);
        p.exists().then_some(p)
    }

    pub fn require_node_module(&self, rel: &str) -> Result<PathBuf> {
        match self.node_module(rel) {
            Some(p) => Ok(p),
            None => Err(UserError::new(
                format!("{rel} is not installed in this scene"),
                TrySteps::one("run dcl-one-sdk init --node-modules-only to restore the vendored node_modules (or npm install)"),
            )
            .why(format!(
                "{} does not exist",
                self.root.join("node_modules").join(rel).display()
            ))
            .into()),
        }
    }

    pub fn is_editor_scene(&self) -> bool {
        let Ok(raw) = std::fs::read(self.root.join("assets/scene/main.composite")) else {
            return false;
        };
        let Ok(json) = serde_json::from_slice::<Value>(&raw) else {
            return false;
        };
        json.get("components")
            .and_then(|c| c.as_array())
            .is_some_and(|comps| {
                comps.iter().any(|c| {
                    c.get("name").and_then(|n| n.as_str()).is_some_and(|n| {
                        n.starts_with("asset-packs::") && n != "asset-packs::Script"
                    })
                })
            })
    }

    pub fn tsconfig(&self) -> Result<PathBuf> {
        let p = self.root.join("tsconfig.json");
        if !p.exists() {
            return Err(UserError::new(
                "this scene has no tsconfig.json",
                TrySteps::one(
                    "create tsconfig.json containing: { \"extends\": \"@dcl/sdk/types/tsconfig.ecs7.json\" }",
                ),
            )
            .why("the bundler and type checker both require it")
            .into());
        }
        Ok(p)
    }
}

pub const TRACKED_MIN_CLI: &str = "3.14.1";

pub fn min_cli_warning(root: &Path) -> Option<String> {
    let declared = package_min_cli(&root.join("package.json"))
        .or_else(|| package_min_cli(&root.join("node_modules/@dcl/sdk/package.json")))?;
    let min = parse_semver(&declared)?;
    let tracked = parse_semver(TRACKED_MIN_CLI)?;
    if min > tracked {
        Some(format!(
            "this project asks for CLI version >= {declared}, newer than the {TRACKED_MIN_CLI} level dcl-one-sdk tracks (@dcl/sdk-commands 7.26.0) \u{2014} if a command misbehaves, cross-check with npx @dcl/sdk-commands"
        ))
    } else {
        None
    }
}

fn package_min_cli(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let v: Value = serde_json::from_slice(&bytes).ok()?;
    if let Some(s) = v.get("minCliVersion").and_then(|s| s.as_str()) {
        return Some(s.to_string());
    }
    v.get("engines")
        .and_then(|e| e.get("minCliVersion"))
        .and_then(|s| s.as_str())
        .map(str::to_string)
}

fn parse_semver(s: &str) -> Option<(u64, u64, u64)> {
    let core = s
        .trim()
        .trim_start_matches(['>', '=', '~', '^', 'v', ' '])
        .split(['-', '+'])
        .next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

pub fn machine_id() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .filter(|h| !h.is_empty())
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "dcl-one".to_string())
}

/// A project root as an opaque, stable id. Everything a preview addresses lives
/// under a root, so the root is the only part of a path that has to be hidden —
/// the part below it is already public in every entity's `content[].file`.
/// Machine-scoped like the hash it goes into, so two machines previewing the
/// same folder never mint the same id.
pub fn root_tag(root: &Path, machine: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    digest.update(root.display().to_string().as_bytes());
    digest.update(b"-");
    digest.update(machine.as_bytes());
    digest
        .finalize()
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The nearest ancestor holding a scene.json — [`Project::load`]'s rule read
/// backwards, so a hash can be built from an absolute path alone by a caller
/// that has nothing else to go on.
///
/// This is a guess, and it costs a `stat` per ancestor on every call. Anything
/// that already knows which project a file belongs to — the entity builder, the
/// wearable list, the watcher — must say so with [`b64_hash_in_root`] instead:
/// faster, and it cannot mistake a nested `scene.json` (a vendored scene, a
/// second scene folder inside the published tree) for the root the file is
/// actually served under.
fn project_root_of(path: &Path) -> Option<&Path> {
    path.ancestors().find(|a| a.join("scene.json").is_file())
}

/// [`b64_hash`] for a caller that already holds the project's [`root_tag`] and
/// the path inside it — which is every caller that walks a project.
///
/// `root_tag` is a pure function of (root, machine), so it is computed once per
/// project instead of once per file, and no filesystem probe is needed to
/// rediscover a root the caller passed in. The payload is byte-for-byte what
/// [`b64_hash`] builds for the same file, so hashes minted through either door
/// resolve identically — and, as there, it is the tag and the path inside the
/// root, never the path on disk.
///
/// `rel` is not validated: minting is not authorization. The read side already
/// treats every relative half as attacker-supplied, whoever minted it — see
/// [`b64_unhash`] and the canonicalize-and-contain check in
/// `start::http::contents`.
pub fn b64_hash_in_root(root_tag: &str, rel: &str) -> String {
    use base64::Engine;
    let unique = format!("{root_tag}/{}", rel.replace('\\', "/"));
    format!(
        "b64-{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(unique.as_bytes())
    )
}

/// [`b64_content_hash`] for a caller that already knows the root — see
/// [`b64_hash_in_root`]. `abs` is only read for the digest; the identity half
/// comes from `root_tag` and `rel`.
pub fn b64_content_hash_in_root(root_tag: &str, rel: &str, abs: &Path) -> String {
    let base = b64_hash_in_root(root_tag, rel);
    match content_tag(abs) {
        Some(tag) => format!("{base}{CONTENT_TAG}{tag}"),
        None => base,
    }
}

/// The identity of a path in a preview: reversible, so `/content/contents/{hash}`
/// finds the file again without a side table. Used bare only for things that ARE
/// a path and have no bytes; real files go through [`b64_content_hash`].
///
/// The payload is `{root tag}/{path inside the project}`, never the absolute
/// path. Base64 is not concealment, and the pages carrying these hashes — the
/// landing page, `/content/entities/active` — are unauthenticated and reachable
/// over the LAN or any tunnel, so an absolute path here handed every visitor the
/// OS username and the layout of the disk. A path under no project root is
/// tagged whole with no relative part: no root can match that tag, so it 404s
/// rather than travelling in the clear.
pub fn b64_hash(path_str: &str, machine: &str) -> String {
    let path = Path::new(path_str);
    match project_root_of(path) {
        Some(root) => b64_hash_in_root(
            &root_tag(root, machine),
            &path.strip_prefix(root).unwrap_or(path).to_string_lossy(),
        ),
        None => b64_hash_in_root(&root_tag(path, machine), ""),
    }
}

/// Separates the path part of a hash from its content tag. Not in the base64url
/// alphabet, so splitting on it can never cut into the encoded path, and an
/// untagged hash still decodes.
const CONTENT_TAG: char = '.';

/// [`b64_hash`] plus a digest of the file's bytes. This is content addressing
/// on the WRITE side only, and the distinction matters enough to spell out.
///
/// What holds: an edit always changes the hash. A client that cached the old
/// name asks for a name it has never seen and refetches just that asset,
/// instead of dropping every cached asset on reload the way a path-only hash
/// forced it to.
///
/// What does NOT hold, and why this is not called a content address: the name
/// does not pin the bytes. `/content/contents/{hash}` resolves on
/// [`hash_path_part`] alone and never looks at the digest, so a request
/// carrying a superseded digest is answered 200 with whatever that file holds
/// NOW — not 404, and not the bytes the digest names. Nothing here keeps old
/// versions, so those bytes are gone and there is nothing else to serve; and
/// failing the request instead would break a fetch already in flight when the
/// file changed under it. Pinned by
/// `start::http::tests::a_stale_digest_serves_the_current_bytes`.
///
/// An unreadable file falls back to the path-only hash — the request for it is
/// going to fail anyway.
pub fn b64_content_hash(abs_path: &str, machine: &str) -> String {
    let base = b64_hash(abs_path, machine);
    match content_tag(Path::new(abs_path)) {
        Some(tag) => format!("{base}{CONTENT_TAG}{tag}"),
        None => base,
    }
}

/// How old an mtime has to be before (mtime, len) is a safe cache key. Filesystem
/// mtime granularity runs as coarse as 2s (FAT, older NFS), so two same-length
/// writes inside one tick are indistinguishable — the make/rsync guard.
const MTIME_SETTLED: std::time::Duration = std::time::Duration::from_secs(2);

/// A short digest of a file, memoised on (mtime, len): the content mapping is
/// rebuilt on every entity request, so re-reading would push the whole scene
/// through sha256 on a timer. A file touched within [`MTIME_SETTLED`] is hashed
/// every time instead — its stamp cannot yet tell one edit from the next, and a
/// stale tag would be cached under it forever.
fn content_tag(path: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    type Stamp = (std::time::SystemTime, u64);
    static SEEN: std::sync::OnceLock<std::sync::Mutex<HashMap<PathBuf, (Stamp, String)>>> =
        std::sync::OnceLock::new();
    let cache = SEEN.get_or_init(Default::default);
    let lock = || {
        cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    };

    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let modified = meta.modified().ok()?;
    let stamp = (modified, meta.len());
    if let Some((seen, tag)) = lock().get(path) {
        if *seen == stamp {
            return Some(tag.clone());
        }
    }
    let bytes = std::fs::read(path).ok()?;
    let tag: String = Sha256::digest(&bytes)
        .iter()
        .take(6)
        .map(|b| format!("{b:02x}"))
        .collect();
    let settled = std::time::SystemTime::now()
        .duration_since(modified)
        .is_ok_and(|age| age >= MTIME_SETTLED);
    if settled {
        lock().insert(path.to_path_buf(), (stamp, tag.clone()));
    }
    Some(tag)
}

/// Bounded parallel map, input order preserved. The per-file read+hash work in
/// deploy and the preview content mappings is independent blocking I/O, and
/// upstream walks project files with a concurrency of 32 (js-sdk-toolchain
/// b7a44a20); one worker per item up to that same cap.
pub(crate) fn parallel_map<T, U>(items: &[T], f: impl Fn(&T) -> U + Sync) -> Vec<U>
where
    T: Sync,
    U: Send,
{
    let workers = std::thread::available_parallelism()
        .map(|n| n.get() * 2)
        .unwrap_or(8)
        .clamp(1, 32)
        .min(items.len().max(1));
    if workers <= 1 {
        return items.iter().map(f).collect();
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::scope(|s| {
        let (next, f) = (&next, &f);
        for _ in 0..workers {
            let tx = tx.clone();
            s.spawn(move || loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(item) = items.get(i) else { break };
                let _ = tx.send((i, f(item)));
            });
        }
    });
    drop(tx);
    let mut slots: Vec<Option<U>> = std::iter::repeat_with(|| None).take(items.len()).collect();
    for (i, v) in rx {
        slots[i] = Some(v);
    }
    slots
        .into_iter()
        .map(|v| v.expect("every index mapped"))
        .collect()
}

/// The part of a hash that identifies WHICH file, not which version of it —
/// and the only part anything resolving a hash compares on, which is what
/// makes the read side path-addressed rather than content-addressed. See
/// [`b64_content_hash`].
pub fn hash_path_part(hash: &str) -> &str {
    hash.rsplit_once(CONTENT_TAG).map_or(hash, |(path, _)| path)
}

/// Splits a hash back into the [`root_tag`] it was minted under and the path
/// inside that root. The caller owns the roots, so it does the matching; the
/// relative half is attacker-controlled (anyone can mint a hash for a tag they
/// read off the page) and must not be joined to a root unchecked.
pub fn b64_unhash(hash: &str) -> Option<(String, String)> {
    use base64::Engine;
    let b = hash.strip_prefix("b64-")?;
    let b = hash_path_part(b);
    let normalized = b.trim_end_matches('=').replace('+', "-").replace('/', "_");
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(normalized.as_bytes())
        .ok()?;
    let s = String::from_utf8(decoded).ok()?;
    let (tag, rel) = s.split_once('/')?;
    Some((tag.to_string(), rel.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tmp(PathBuf);

    impl Tmp {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("dcl-one-sdk-mincli-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Tmp(dir)
        }

        fn write(&self, rel: &str, contents: &str) {
            let p = self.0.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, contents).unwrap();
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn no_package_json_is_silent() {
        let t = Tmp::new("none");
        assert_eq!(min_cli_warning(&t.0), None);
    }

    #[test]
    fn editor_scene_requires_a_runtime_asset_packs_component() {
        let t = Tmp::new("editorscene");
        let project = |root: &Path| Project {
            root: root.to_path_buf(),
            scene_json: serde_json::json!({}),
        };
        assert!(!project(&t.0).is_editor_scene());
        t.write("assets/scene/main.composite", "not json");
        assert!(!project(&t.0).is_editor_scene());
        t.write(
            "assets/scene/main.composite",
            r#"{"version":1,"components":[{"name":"asset-packs::Script","data":{}},{"name":"core::Transform","data":{}}]}"#,
        );
        assert!(!project(&t.0).is_editor_scene());
        t.write(
            "assets/scene/main.composite",
            r#"{"version":1,"components":[{"name":"asset-packs::Actions","data":{}}]}"#,
        );
        assert!(project(&t.0).is_editor_scene());
    }

    #[test]
    fn tracked_level_is_silent() {
        let t = Tmp::new("ok");
        t.write("package.json", r#"{"minCliVersion":"3.14.1"}"#);
        assert_eq!(min_cli_warning(&t.0), None);
    }

    #[test]
    fn newer_min_warns_and_names_both_versions() {
        let t = Tmp::new("newer");
        t.write("package.json", r#"{"minCliVersion":"3.15.0"}"#);
        let w = min_cli_warning(&t.0).unwrap();
        assert!(w.contains("3.15.0"));
        assert!(w.contains(TRACKED_MIN_CLI));
        assert!(w.contains("@dcl/sdk-commands"));
    }

    #[test]
    fn engines_form_and_range_prefixes_parse() {
        let t = Tmp::new("engines");
        t.write("package.json", r#"{"engines":{"minCliVersion":">=4.0.0"}}"#);
        assert!(min_cli_warning(&t.0).is_some());
    }

    #[test]
    fn installed_sdk_manifest_is_the_fallback_source() {
        let t = Tmp::new("sdkfall");
        t.write(
            "node_modules/@dcl/sdk/package.json",
            r#"{"minCliVersion":"3.99.0"}"#,
        );
        assert!(min_cli_warning(&t.0).is_some());
        t.write("package.json", r#"{"minCliVersion":"3.0.0"}"#);
        assert_eq!(min_cli_warning(&t.0), None);
    }

    #[test]
    fn unparseable_versions_stay_silent() {
        let t = Tmp::new("garbage");
        t.write("package.json", r#"{"minCliVersion":"latest"}"#);
        assert_eq!(min_cli_warning(&t.0), None);
    }

    #[test]
    fn a_hash_carries_the_path_inside_the_project_never_the_path_on_disk() {
        let t = Tmp::new("roothash");
        t.write("scene.json", "{}");
        t.write("assets/tree.glb", "glb");
        let root = t.0.clone();
        let hash = b64_content_hash(&root.join("assets/tree.glb").display().to_string(), "m");

        let (tag, rel) = b64_unhash(&hash).unwrap();
        assert_eq!(tag, root_tag(&root, "m"));
        assert_eq!(rel, "assets/tree.glb");
        assert!(
            !format!("{tag}/{rel}").contains(root.to_str().unwrap()),
            "the decoded hash still spells out where the scene lives"
        );

        assert_eq!(
            b64_unhash(&b64_hash(&root.display().to_string(), "m")).unwrap(),
            (root_tag(&root, "m"), String::new()),
            "the scene entity id must decode to its root and an empty path"
        );
        assert_ne!(root_tag(&root, "m"), root_tag(&root, "other-machine"));
        assert_ne!(root_tag(&root, "m"), root_tag(&root.join("nested"), "m"));
        assert_eq!(b64_unhash("QmSomeIpfsHash"), None);
    }

    #[test]
    fn a_caller_that_knows_the_root_mints_the_same_hash_without_looking_for_one() {
        let t = Tmp::new("knownroot");
        t.write("scene.json", "{}");
        t.write("assets/tree.glb", "glb");
        let root = t.0.clone();
        let abs = root.join("assets/tree.glb");
        let tag = root_tag(&root, "m");

        assert_eq!(
            b64_content_hash_in_root(&tag, "assets/tree.glb", &abs),
            b64_content_hash(&abs.display().to_string(), "m"),
            "the two doors onto a file's hash have to agree, or a client asks \
             for a name the entity never advertised"
        );
        assert_eq!(
            b64_hash_in_root(&tag, ""),
            b64_hash(&root.display().to_string(), "m"),
            "the scene entity id is the root's tag with an empty path"
        );
        assert_eq!(
            b64_unhash(&b64_hash_in_root(&tag, "assets/tree.glb")).unwrap(),
            (tag.clone(), "assets/tree.glb".to_string())
        );
        assert_eq!(
            b64_hash_in_root(&tag, "assets\\tree.glb"),
            b64_hash_in_root(&tag, "assets/tree.glb"),
            "a windows separator must normalise the same way it does when the \
             root is discovered"
        );

        t.write("sub/scene.json", "{}");
        t.write("sub/model.glb", "glb");
        let nested = root.join("sub/model.glb");
        assert_eq!(
            b64_unhash(&b64_hash(&nested.display().to_string(), "m")).unwrap(),
            (root_tag(&root.join("sub"), "m"), "model.glb".to_string()),
            "the path-only door guesses the nearest scene.json"
        );
        assert_eq!(
            b64_unhash(&b64_hash_in_root(&tag, "sub/model.glb")).unwrap(),
            (tag, "sub/model.glb".to_string())
        );
    }

    fn set_mtime(p: &Path, ts: std::time::SystemTime) {
        std::fs::File::options()
            .write(true)
            .open(p)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(ts))
            .unwrap();
    }

    #[test]
    fn a_same_length_edit_inside_one_mtime_tick_still_changes_the_content_tag() {
        let t = Tmp::new("tagtick");
        let p = t.0.join("main.composite");
        let abs = p.display().to_string();
        std::fs::write(&p, r#"{"n":1}"#).unwrap();
        let first = b64_content_hash(&abs, "m");

        let tick = std::fs::metadata(&p).unwrap().modified().unwrap();
        std::fs::write(&p, r#"{"n":2}"#).unwrap();
        set_mtime(&p, tick);
        assert_eq!(std::fs::metadata(&p).unwrap().modified().unwrap(), tick);
        assert_eq!(std::fs::metadata(&p).unwrap().len(), 7);

        let second = b64_content_hash(&abs, "m");
        assert_ne!(
            first, second,
            "a second edit of the same length in the same mtime tick kept the old tag"
        );
        assert_eq!(hash_path_part(&first), hash_path_part(&second));
    }

    #[test]
    fn a_settled_mtime_is_still_memoised() {
        let t = Tmp::new("tagmemo");
        let p = t.0.join("tree.glb");
        let abs = p.display().to_string();
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(600);
        std::fs::write(&p, "aaaa").unwrap();
        set_mtime(&p, old);
        let first = b64_content_hash(&abs, "m");

        std::fs::write(&p, "bbbb").unwrap();
        set_mtime(&p, old);
        assert_eq!(
            first,
            b64_content_hash(&abs, "m"),
            "the memo is gone: every entity request would re-read the whole scene"
        );
    }

    #[test]
    fn semver_compare_is_numeric_not_lexical() {
        assert!(parse_semver("3.9.0").unwrap() < parse_semver("3.14.1").unwrap());
        assert!(parse_semver("10.0.0").unwrap() > parse_semver("9.9.9").unwrap());
        assert_eq!(
            parse_semver("7.22.6-25007982108.commit-83012ab").unwrap(),
            (7, 22, 6)
        );
    }

    /// More items than the worker cap, so the work-stealing index actually
    /// wraps threads; order must still be the input's.
    #[test]
    fn parallel_map_preserves_order_and_covers_every_item() {
        let items: Vec<usize> = (0..257).collect();
        let expected: Vec<usize> = items.iter().map(|n| n * 2).collect();
        assert_eq!(parallel_map(&items, |n| n * 2), expected);
        assert!(parallel_map(&Vec::<usize>::new(), |n| *n).is_empty());
        assert_eq!(parallel_map(&[7usize], |n| n + 1), vec![8]);
    }
}
