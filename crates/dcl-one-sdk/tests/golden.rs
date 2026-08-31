//! Golden scene snapshots: one built scene, rendered whole, compared as text.
//!
//! Two tiers per fixture, both landing in the same
//! `testdata/golden/<fixture>.<mode>.golden`:
//!
//! * STATIC — `init --node-modules-only` into a shared tree, the fixture
//!   overlaid on it, `build::build` run IN PROCESS, then everything read back
//!   off disk plus `deploy::prepare` with a pinned timestamp. The build's own
//!   stdout is deliberately not captured or parsed: its step lines carry
//!   elapsed times ("Scene chunk saved bin/scene.js (4.99 ms)") and must never
//!   reach a golden.
//! * RUNTIME — `scripts/golden-runtime.mjs`, which loads the built scene in a
//!   node sandbox over a mocked `~system/*` table and runs upstream's short
//!   frame loop. Its stdout is appended verbatim. That stdout includes the
//!   scene's own console output as `CONSOLE(<level>):` lines, which is the only
//!   thing that catches a scene throwing on startup: the generated entrypoint
//!   catches everything `main()` throws and reports it with `console.error`, so
//!   the run still exits zero and can emit byte-identical CRDT traffic.
//!
//! What is NOT here is a CPU metric. Upstream's opcode and malloc counters come
//! from a patched QuickJS published only as an npm package, and this toolchain
//! has no npm step by design. The cost axis is what is deterministic and free:
//! exact per-artifact bytes, deploy total, per-run CRDT message/byte counts,
//! host-call counts and console output. A regression that changes none of those
//! will not be caught here, and no line in the format pretends otherwise.
//!
//! Stale golden: `UPDATE_GOLDEN=1 cargo test -p dcl-one-sdk --test golden`, or
//! scripts/update-goldens.sh. A MISSING golden fails too — upstream writes one
//! silently, but this repo's CI backstop counts passing tests and never checks
//! for a dirty tree, so an auto-written golden would report green having
//! asserted nothing.

use dcl_one_sdk::build::{self, BuildOptions};
use dcl_one_sdk::{crdt_gen, deploy, entrypoint, init, split};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Pinned in Cargo.toml as `rolldown = "=1.2.4"`. Repeated here on purpose: a
/// bump has to be copied into this line, which invalidates every golden in one
/// visible place instead of scattering unexplained byte diffs.
const BUNDLER: &str = "rolldown-1.2.4";

/// The CLI stamps the wall clock into the deploy entity, which would put a
/// fresh CID in every run. Per-file CIDv1s are timestamp-independent, so only
/// the entity id needs freezing.
const PINNED_TIMESTAMP: i64 = 0;

/// The runtime tier, relative to the crate root. Resolved through
/// CARGO_MANIFEST_DIR rather than cwd: `cargo test` runs from the workspace
/// root and `cargo test -p` from wherever the caller stood.
const RUNTIME_SCRIPT: &str = "scripts/golden-runtime.mjs";

#[derive(Clone, Copy)]
struct Fixture {
    /// Directory under `testdata/golden/`.
    name: &'static str,
    production: bool,
    custom_entry_point: bool,
    /// Files staged from `testdata/` rather than kept in the fixture dir, so
    /// there is one source of truth: (path under testdata/, path in the scene).
    staged: &'static [(&'static str, &'static str)],
}

impl Fixture {
    const fn new(name: &'static str) -> Self {
        Fixture {
            name,
            production: true,
            custom_entry_point: false,
            staged: &[],
        }
    }

    const fn development(mut self) -> Self {
        self.production = false;
        self
    }

    const fn custom_entry(mut self) -> Self {
        self.custom_entry_point = true;
        self
    }

    const fn staging(mut self, staged: &'static [(&'static str, &'static str)]) -> Self {
        self.staged = staged;
        self
    }

    fn mode(&self) -> &'static str {
        if self.production {
            "production"
        } else {
            "development"
        }
    }

    fn stem(&self) -> String {
        format!("{}.{}", self.name, self.mode())
    }
}

/// The composite fixture reuses the crdt encoder's own reference composite, so
/// a change to it shows up in both suites rather than only in the one nobody
/// remembers to update.
const OPERA: &[(&str, &str)] = &[("opera-main.composite", "assets/scene/main.composite")];

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cube_production() {
    golden(Fixture::new("cube")).await;
}

/// The one fixture snapshotted twice: dev-vs-prod is a build FLAG, not a
/// property of a scene, so it costs one extra golden rather than a directory.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cube_development() {
    golden(Fixture::new("cube").development()).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn composite_production() {
    golden(Fixture::new("composite").staging(OPERA)).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dynamic_import_production() {
    golden(Fixture::new("dynamic-import")).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ui_production() {
    golden(Fixture::new("ui")).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn smart_item_production() {
    golden(Fixture::new("smart-item")).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn custom_entry_production() {
    golden(Fixture::new("custom-entry").custom_entry()).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_main_production() {
    golden(Fixture::new("no-main")).await;
}

async fn golden(fixture: Fixture) {
    let Some(node) = node_bin() else {
        return;
    };
    let root = stage(&fixture);
    let built = build::build(&BuildOptions {
        dir: root.clone(),
        production: fixture.production,
        ignore_composite: false,
        custom_entry_point: fixture.custom_entry_point,
        skip_type_check: false,
        out_root: None,
        quiet: false,
    })
    .await
    .unwrap_or_else(|e| panic!("{} failed to build: {e:#}", fixture.stem()));

    let mut text = render_static(&fixture, &built);
    text.push_str(&run_runtime(&node, &built.project.root));
    compare(&fixture, &text);
}

fn node_bin() -> Option<PathBuf> {
    match build::find_node() {
        Some(p) => Some(p),
        // The build already needs node for the type check and the data-layer
        // fallback, so this adds no requirement the suite did not have.
        None => catalyrst_testgate::unavailable(
            "node",
            "install node; the build's type check and data-layer fallback need it too",
        ),
    }
}

fn manifest_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn testdata() -> PathBuf {
    manifest_dir().join("testdata")
}

fn work_dir() -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).join("golden")
}

/// The vendored toolchain, extracted once per test binary.
///
/// `OnceLock::get_or_init` is the lock: cargo runs these `#[test]`s in
/// parallel, and every one of them calls this before touching anything else,
/// so the environment scrub below lands before any build reads it.
fn node_modules_src() -> &'static Path {
    static SRC: OnceLock<PathBuf> = OnceLock::new();
    SRC.get_or_init(|| {
        // Same list tests/error_contract.rs scrubs from its child processes.
        // These builds run in-process, so the variables have to leave this
        // process instead — a developer with RUST_LOG or a default deploy
        // target set must get the same bytes CI does.
        for key in [
            "DCL_PRIVATE_KEY",
            "RUST_LOG",
            "NO_COLOR",
            "DCL_ONE_SDK_DEFAULT_TARGET",
            "DCL_ONE_SDK_LINKER_TIMEOUT_SECS",
        ] {
            std::env::remove_var(key);
        }
        let dir = work_dir().join("node_modules-src");
        std::fs::create_dir_all(&dir).expect("creating the shared node_modules dir");
        init::init(&init::InitOptions {
            dir: dir.clone(),
            project: None,
            yes: true,
            node_modules_only: true,
        })
        .expect("extracting the vendored node_modules");
        dir
    })
}

/// A private copy of the fixture with the shared toolchain symlinked in.
fn stage(fixture: &Fixture) -> PathBuf {
    let modules = node_modules_src().join("node_modules");
    let root = work_dir().join(fixture.stem());
    let _ = std::fs::remove_dir_all(&root);
    copy_tree(&testdata().join("golden").join(fixture.name), &root);
    for (from, to) in fixture.staged {
        let dst = root.join(to);
        std::fs::create_dir_all(dst.parent().expect("staged path has a parent")).unwrap();
        std::fs::copy(testdata().join(from), &dst)
            .unwrap_or_else(|e| panic!("staging {from}: {e}"));
    }
    std::os::unix::fs::symlink(modules, root.join("node_modules")).expect("linking node_modules");
    root
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap_or_else(|e| panic!("creating {}: {e}", to.display()));
    for entry in std::fs::read_dir(from)
        .unwrap_or_else(|e| panic!("reading fixture {}: {e}", from.display()))
        .flatten()
    {
        let src = entry.path();
        let dst = to.join(entry.file_name());
        if src.is_dir() {
            copy_tree(&src, &dst);
        } else {
            std::fs::copy(&src, &dst).unwrap_or_else(|e| panic!("copying {}: {e}", src.display()));
        }
    }
}

fn render_static(fixture: &Fixture, built: &build::Built) -> String {
    let root = &built.project.root;
    let main = built.project.main_output().expect("scene.json main");
    let (sdk_rel, scene_rel) = split::chunk_rel_paths(&main);
    let smart_rel = split::smart_chunk_rel_path(&main);

    let mut chunks: Vec<(String, Vec<u8>)> = Vec::new();
    for rel in [main.clone(), scene_rel.clone(), sdk_rel.clone(), smart_rel] {
        if let Ok(bytes) = std::fs::read(root.join(&rel)) {
            chunks.push((rel, strip_sourcemap(&bytes).to_vec()));
        }
    }

    let mut out = String::new();
    let _ = writeln!(
        out,
        "(dcl-one-sdk golden v1 {} {})",
        fixture.name,
        fixture.mode()
    );
    let _ = writeln!(
        out,
        "TOOLCHAIN sdk={} ecs={} bundler={BUNDLER} blob=sha256:{}",
        package_version(root, "@dcl/sdk"),
        package_version(root, "@dcl/ecs"),
        short_sha256(&std::fs::read(manifest_dir().join("src/vendor/node_modules.zip")).unwrap()),
    );

    // Upstream's spelling and unit, so a reviewer can put our number next to
    // theirs; the sum is over every emitted JS chunk, sourcemaps excluded.
    let total: usize = chunks.iter().map(|(_, b)| b.len()).sum();
    let _ = writeln!(
        out,
        "SCENE_COMPILED_JS_SIZE_{}={} bytes",
        if fixture.production { "PROD" } else { "DEV" },
        kilobytes(total)
    );
    if !fixture.production {
        // Upstream's literal line. The sources array only: sourcesContent
        // base64-embeds the absolute scene root, which is why the map is
        // stripped before every size and hash above and below.
        let _ = writeln!(out, "THE BUNDLE HAS SOURCEMAPS");
        for source in sourcemap_sources(&std::fs::read(root.join(&scene_rel)).unwrap()) {
            let _ = writeln!(out, "  SOURCE: {source}");
        }
    }
    for (rel, bytes) in &chunks {
        // Exact bytes, not upstream's 0.1k rounding: upstream rounds because
        // esbuild's output is not byte-stable across machines and ours is.
        let _ = writeln!(
            out,
            "  ARTIFACT {:<22}{:>9} sha256={}",
            rel,
            bytes.len(),
            short_sha256(bytes)
        );
    }

    let loader = std::fs::read_to_string(root.join(&main)).expect("reading the loader stub");
    let _ = writeln!(
        out,
        "LOADER sdk={} smart={} scene={} max_composite_entity={}",
        loader_var(&loader, "__dclOneSdkChunkPath"),
        dash_if_empty(&loader_var(&loader, "__dclOneSmartChunkPath")),
        loader_var(&loader, "__dclOneSceneChunkPath"),
        loader_number(&loader, "globalThis.DCL_MAX_COMPOSITE_ENTITY"),
    );

    render_entrypoint(&mut out, root, fixture, &scene_rel);
    render_entity_names(&mut out, root);
    render_main_crdt(&mut out, root);
    if fixture.production {
        render_deploy(&mut out, built);
    } else {
        // A dev bundle's inline sourcemap embeds the absolute scene root, so
        // its CIDv1 — and with it the entity id and the total — is a function
        // of where the scene happens to sit. Nothing deploys a dev build, so
        // the section is left out rather than faked stable.
        let _ = writeln!(
            out,
            "DEPLOY skipped (a dev bundle's sourcemap is path-bound)"
        );
    }
    out
}

fn render_entrypoint(out: &mut String, root: &Path, fixture: &Fixture, scene_rel: &str) {
    let dir = root.join(".dcl-one");
    let generated = std::fs::read_to_string(dir.join("entrypoint.ts")).expect("entrypoint.ts");
    let composites = std::fs::read_to_string(dir.join("all-composites.js"))
        .map(|s| s.matches(".composite':").count())
        .unwrap_or(0);
    let script_utils = match std::fs::read_to_string(dir.join("script-utils.js")).as_deref() {
        Ok(entrypoint::SCRIPT_UTILS_STUB) => "stub",
        Ok(_) => "runtime",
        Err(_) => "-",
    };
    // `main=guarded` is the generated entrypoint's runtime
    // `if (entrypoint.main !== undefined)` startup-system guard; a custom entry
    // point generates nothing and re-exports scene.json's main verbatim.
    let _ = writeln!(
        out,
        "ENTRYPOINT .dcl-one/entrypoint.ts main={} composites={composites} script-utils={script_utils}",
        if fixture.custom_entry_point { "custom-entry" } else { "guarded" },
    );
    indent_into(out, &scrub_root(&generated, root));
    // The registry keys the scene chunk actually resolves. Sorted, because this
    // is a set: the emission order is rolldown's business, not the contract's.
    let mut keys = require_specifiers(&std::fs::read_to_string(root.join(scene_rel)).unwrap());
    keys.sort();
    keys.dedup();
    for key in keys {
        let _ = writeln!(out, "  REQUIRE(scene): {key}");
    }
}

fn render_entity_names(out: &mut String, root: &Path) {
    let rel = dcl_one_sdk::entity_names::OUTPUT_PATH;
    let Ok(names) = std::fs::read_to_string(root.join(rel)) else {
        return;
    };
    let _ = writeln!(out, "ENTITY_NAMES {rel}");
    indent_into(out, &names);
}

/// Quote a file under its section header. Blank lines stay blank rather than
/// becoming two spaces: trailing whitespace is the first thing an editor or a
/// pre-commit hook silently eats, and a golden it can edit is a golden that
/// fails for no reason.
fn indent_into(out: &mut String, body: &str) {
    for line in body.lines() {
        if line.is_empty() {
            out.push('\n');
        } else {
            let _ = writeln!(out, "  {line}");
        }
    }
}

fn render_main_crdt(out: &mut String, root: &Path) {
    let Ok(bytes) = std::fs::read(root.join("main.crdt")) else {
        let _ = writeln!(out, "MAIN_CRDT none (no composite)");
        return;
    };
    // Which generator produced it, re-derived rather than scraped from the
    // build's stdout: the native encoder refuses composites carrying their own
    // jsonSchema, and those fall back to the node data layer.
    let origin = match crdt_gen::generate(root) {
        Ok(Some(g)) => format!(
            "native, {} composite{}",
            g.composites,
            if g.composites == 1 { "" } else { "s" }
        ),
        Ok(None) => "native, 0 composites".to_string(),
        Err(_) => "node data-layer".to_string(),
    };
    let _ = writeln!(
        out,
        "MAIN_CRDT main.crdt {} bytes sha256={} ({origin})",
        bytes.len(),
        short_sha256(&bytes)
    );
}

fn render_deploy(out: &mut String, built: &build::Built) {
    let prepared = deploy::prepare(&built.project).expect("deploy::prepare");
    let (entity, _) =
        deploy::build_entity(&prepared, PINNED_TIMESTAMP).expect("deploy::build_entity");
    let total: usize = prepared.files.iter().map(|(_, _, b)| b.len()).sum();
    let _ = writeln!(
        out,
        "DEPLOY entity={entity} files={} total={} bytes",
        prepared.files.len(),
        kilobytes(total)
    );
    // deploy::collect_publishable_files' order, already asserted deterministic
    // by src/deploy/mod.rs; keeping it means the golden also pins that order.
    for (rel, hash, bytes) in &prepared.files {
        let _ = writeln!(out, "  {hash} {rel} {} bytes", bytes.len());
    }
}

fn run_runtime(node: &Path, root: &Path) -> String {
    let script = manifest_dir().join(RUNTIME_SCRIPT);
    let out = std::process::Command::new(node)
        .arg(&script)
        .arg(root)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap_or_else(|e| panic!("running {}: {e}", script.display()));
    assert!(
        out.status.success(),
        "{} exited {:?}\n{}{}",
        script.display(),
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    scrub_root(&String::from_utf8_lossy(&out.stdout), root)
}

fn compare(fixture: &Fixture, actual: &str) {
    let path = testdata()
        .join("golden")
        .join(format!("{}.golden", fixture.stem()));
    let actual = normalize_newlines(actual);

    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, &actual)
            .unwrap_or_else(|e| panic!("writing {}: {e}", path.display()));
        return;
    }

    let Ok(expected) = std::fs::read_to_string(&path) else {
        panic!(
            "no golden at {}\n\n{actual}\n\nwrite it with:\n  {}",
            path.display(),
            update_command(fixture)
        );
    };
    let expected = normalize_newlines(&expected);
    if expected == actual {
        return;
    }
    let dump = work_dir().join(format!("{}.actual", fixture.stem()));
    let _ = std::fs::create_dir_all(work_dir());
    let _ = std::fs::write(&dump, &actual);
    panic!(
        "{} is stale\n{}\nfull output: {}\nregenerate with:\n  {}",
        path.display(),
        first_difference(&expected, &actual),
        dump.display(),
        update_command(fixture)
    );
}

fn update_command(fixture: &Fixture) -> String {
    format!(
        "UPDATE_GOLDEN=1 cargo test -p dcl-one-sdk --test golden -- {}",
        fixture.stem().replace(['.', '-'], "_")
    )
}

/// The first line the two texts disagree on, as a `-`/`+` pair.
fn first_difference(expected: &str, actual: &str) -> String {
    let (want, got): (Vec<&str>, Vec<&str>) =
        (expected.lines().collect(), actual.lines().collect());
    for i in 0..want.len().max(got.len()) {
        let (w, g) = (want.get(i), got.get(i));
        if w == g {
            continue;
        }
        return format!(
            "line {}:\n  - {}\n  + {}",
            i + 1,
            w.copied().unwrap_or("<end of golden>"),
            g.copied().unwrap_or("<end of output>")
        );
    }
    "the two differ only in trailing newlines".to_string()
}

fn normalize_newlines(s: &str) -> String {
    s.replace("\r\n", "\n")
}

/// Everything up to the inline sourcemap. Dev bundles carry the absolute scene
/// root inside the map's base64 `sourcesContent`, so hashing or sizing the raw
/// file would make the golden a function of where the temp dir landed.
fn strip_sourcemap(bytes: &[u8]) -> &[u8] {
    const MARKER: &[u8] = b"//# sourceMappingURL=";
    match bytes
        .windows(MARKER.len())
        .position(|window| window == MARKER)
    {
        Some(at) => &bytes[..at],
        None => bytes,
    }
}

fn sourcemap_sources(bytes: &[u8]) -> Vec<String> {
    use base64::Engine as _;
    let text = String::from_utf8_lossy(bytes);
    const MARKER: &str = "base64,";
    let Some(at) = text.find("//# sourceMappingURL=").and_then(|start| {
        text[start..]
            .find(MARKER)
            .map(|offset| start + offset + MARKER.len())
    }) else {
        return Vec::new();
    };
    let raw = base64::engine::general_purpose::STANDARD
        .decode(text[at..].trim())
        .expect("decoding the inline sourcemap");
    let map: serde_json::Value =
        serde_json::from_slice(&raw).expect("parsing the inline sourcemap");
    map.get("sources")
        .and_then(|s| s.as_array())
        .map(|entries| {
            entries
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn short_sha256(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    let digest = sha2::Sha256::digest(bytes);
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// Upstream's unit: thousands of bytes with one decimal.
fn kilobytes(bytes: usize) -> String {
    format!("{:.1}k", bytes as f64 / 1000.0)
}

fn package_version(root: &Path, package: &str) -> String {
    let path = root.join("node_modules").join(package).join("package.json");
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return "-".to_string();
    };
    serde_json::from_str::<serde_json::Value>(&raw)
        .ok()
        .and_then(|v| {
            v.get("version")
                .and_then(|x| x.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "-".to_string())
}

fn loader_var(loader: &str, name: &str) -> String {
    let needle = format!("{name} = '");
    let Some(at) = loader.find(&needle) else {
        return "?".to_string();
    };
    let rest = &loader[at + needle.len()..];
    rest[..rest.find('\'').unwrap_or(0)].to_string()
}

fn loader_number(loader: &str, name: &str) -> String {
    let needle = format!("{name} = ");
    let Some(at) = loader.find(&needle) else {
        return "?".to_string();
    };
    let rest = &loader[at + needle.len()..];
    rest.chars().take_while(char::is_ascii_digit).collect()
}

fn dash_if_empty(value: &str) -> String {
    if value.is_empty() {
        "-".to_string()
    } else {
        value.to_string()
    }
}

/// `require("x")` / `require('x')` literals in an emitted chunk.
fn require_specifiers(code: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = code.as_bytes();
    let mut i = 0;
    while let Some(pos) = code[i..].find("require(") {
        let start = i + pos + "require(".len();
        i = start;
        let Some(&quote) = bytes.get(start) else {
            break;
        };
        if quote != b'"' && quote != b'\'' {
            continue;
        }
        let Some(end) = code[start + 1..].find(quote as char) else {
            break;
        };
        if bytes.get(start + 2 + end) == Some(&b')') {
            out.push(code[start + 1..start + 1 + end].to_string());
        }
        i = start + 1 + end;
    }
    out
}

/// The generated entrypoint embeds the absolute scene root twice, and node
/// stack traces would carry it too; `<SCENE>` is the only thing that makes the
/// text portable between a worktree, a checkout and CI.
fn scrub_root(text: &str, root: &Path) -> String {
    text.replace(&root.display().to_string(), "<SCENE>")
}
