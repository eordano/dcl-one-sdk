mod net;
mod run;

pub use net::{
    build_delete_payload, encode_segment, jump_in_url, sanitize_catalyst_url,
    scenes_on_other_parcels, send_world_delete, simple_auth_chain, upload_entity, WorldScene,
};
pub use run::{deploy, load_signer};

use crate::jsjson::{self, JsValue};
use crate::scene::Project;
use crate::ux::{TrySteps, UserError};
use anyhow::{Context, Result};
use catalyrst_hashing::hash_bytes_v1;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::collections::HashSet;
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
}

const MAX_FILE_SIZE_BYTES: usize = 50_000_000;

pub const CATALYST_ROTATION: [&str; 8] = [
    "https://peer-ec2.decentraland.org",
    "https://peer.melonwave.com",
    "https://peer-ec1.decentraland.org",
    "https://peer-ap1.decentraland.org",
    "https://peer.uadevops.com",
    "https://peer.dclnodes.io",
    "https://peer-eu1.decentraland.org",
    "https://interconnected.online",
];

const DEFAULT_DCL_IGNORE: [&str; 20] = [
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
    "**/*.ts",
    "**/*.tsx",
    "Dockerfile",
    "thumbnails",
    "dist",
    "README.md",
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

fn walk(dir: &Path, root: &Path, gi: &Gitignore, out: &mut Vec<String>) {
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
        if path.is_dir() {
            if !gi.matched(&rel, true).is_ignore() {
                dirs.push((name, path));
            }
        } else if !gi.matched(&rel, false).is_ignore() {
            files.push((name, rel));
        }
    }
    files.sort_by(|a, b| b.0.cmp(&a.0));
    dirs.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, rel) in files {
        out.push(rel);
    }
    for (_, path) in dirs {
        walk(&path, root, gi, out);
    }
}

pub fn collect_publishable_files(root: &Path) -> Result<Vec<String>> {
    let gi = build_matcher(root)?;
    let mut out = Vec::new();
    walk(root, root, &gi, &mut out);
    Ok(out)
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

pub fn prepare(project: &Project) -> Result<Prepared> {
    let rel_paths = collect_publishable_files(&project.root)?;
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
    let mut files = Vec::new();
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
        let p = project.root.join(rel);
        let bytes =
            std::fs::read(&p).with_context(|| format!("reading content file {}", p.display()))?;
        if bytes.len() > MAX_FILE_SIZE_BYTES {
            return Err(UserError::new(
                format!(
                    "{rel} is {}, over the 50 MB per-file limit",
                    human_size(bytes.len())
                ),
                TrySteps::one("compress or split the asset (GLB textures are usually the culprit)")
                    .and("exclude it via .dclignore if it is not needed in-world"),
            )
            .into());
        }
        let hash = hash_bytes_v1(&bytes);
        files.push((rel.clone(), hash, bytes));
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

pub fn human_size(bytes: usize) -> String {
    const MB: f64 = 1_000_000.0;
    if bytes as f64 >= MB {
        format!("{:.1} MB", bytes as f64 / MB)
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
                "bin/index.js.map",
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
            },
            WorldScene {
                title: "other".into(),
                parcels: vec!["5,5".into()],
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
}
