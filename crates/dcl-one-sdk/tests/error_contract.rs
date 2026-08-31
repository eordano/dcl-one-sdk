use std::io::Read;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_dcl-one-sdk");

/// Every child this file spawns is bounded by this.
///
/// Not a nicety: the product's own waits are user-scale, not test-scale.
/// `deploy` without a key hands off to the browser linker and waits
/// `DCL_ONE_SDK_LINKER_TIMEOUT_SECS`, which defaults to **600 seconds** — so one
/// test that reaches that path by mistake turns this file into a twelve-minute
/// CI outage that still ends green. `Command::output()` has no timeout at all,
/// so nothing used to stop that. Now it fails in a minute, naming the argv that
/// hung, which is a bug report instead of an outage.
const CHILD_TIMEOUT: Duration = Duration::from_secs(60);

const SCENE_OK: &str =
    r#"{"main":"bin/index.js","runtimeVersion":"7","scene":{"parcels":["0,0"],"base":"0,0"}}"#;
const SCENE_WORLD: &str = r#"{"main":"bin/index.js","runtimeVersion":"7","scene":{"parcels":["0,0"],"base":"0,0"},"worldConfiguration":{"name":"example.dcl.eth"}}"#;
const TSCONFIG: &str = r#"{"compilerOptions":{}}"#;
const WEARABLE_OK: &str = r#"{"id":"0f0e0d0c-0b0a-4900-8807-060504030201","name":"Test Glasses","description":"t","rarity":"mythic","data":{"replaces":[],"hides":[],"tags":[],"category":"eyewear","representations":[{"bodyShapes":["urn:decentraland:off-chain:base-avatars:BaseMale"],"mainFile":"model.glb","contents":["model.glb"],"overrideHides":[],"overrideReplaces":[]}]}}"#;

struct Fixture(PathBuf);

impl Fixture {
    fn new(tag: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("dcl-one-sdk-errgate-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Fixture(dir)
    }

    fn write(&self, rel: &str, contents: &str) {
        self.write_bytes(rel, contents.as_bytes());
    }

    fn write_bytes(&self, rel: &str, contents: &[u8]) {
        let p = self.0.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, contents).unwrap();
    }

    fn mkdir(&self, rel: &str) {
        std::fs::create_dir_all(self.0.join(rel)).unwrap();
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn dir_arg(&self) -> String {
        self.0.display().to_string()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn cli(args: &[&str], envs: &[(&str, &str)]) -> Command {
    let mut cmd = Command::new(BIN);
    cmd.args(args);
    for k in [
        "DCL_PRIVATE_KEY",
        "RUST_LOG",
        "NO_COLOR",
        "DCL_ONE_SDK_DEFAULT_TARGET",
        "DCL_ONE_SDK_LINKER_TIMEOUT_SECS",
    ] {
        cmd.env_remove(k);
    }
    for (k, v) in envs {
        cmd.env(k, v);
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

/// A child's piped stream, drained by a thread so it is readable *while* the
/// child is still running and can never fill its pipe and deadlock.
struct Tail {
    buf: Arc<Mutex<Vec<u8>>>,
    reader: Option<std::thread::JoinHandle<()>>,
}

impl Tail {
    fn spawn(mut src: impl Read + Send + 'static) -> Self {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&buf);
        let reader = std::thread::spawn(move || {
            let mut chunk = [0u8; 4096];
            while let Ok(n) = src.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                sink.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .extend_from_slice(&chunk[..n]);
            }
        });
        Tail {
            buf,
            reader: Some(reader),
        }
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.buf.lock().unwrap_or_else(PoisonError::into_inner))
            .into_owned()
    }

    /// Everything, after the writer end is gone. Only valid once the child has
    /// exited; before that use [`Tail::text`].
    fn finish(&mut self) -> Vec<u8> {
        if let Some(r) = self.reader.take() {
            let _ = r.join();
        }
        self.buf
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

fn wait_bounded(child: &mut Child, limit: Duration) -> Option<ExitStatus> {
    let deadline = Instant::now() + limit;
    let mut nap = Duration::from_millis(1);
    loop {
        match child.try_wait().expect("waiting on the CLI child") {
            Some(status) => return Some(status),
            None if Instant::now() >= deadline => return None,
            None => {
                std::thread::sleep(nap);
                nap = (nap * 2).min(Duration::from_millis(50));
            }
        }
    }
}

/// SIGTERM first, SIGKILL only as a backstop.
///
/// `start` tears its abgen sidecar down on SIGTERM
/// (`asset_bundles::kill_sidecar_group`). A SIGKILLed preview server runs no
/// such handler, and because the sidecar is spawned into its own process group
/// it does not die with the parent either — so it is orphaned onto the
/// developer's machine, holding a port and three quarters of the CPUs' worth of
/// worker threads, once for every run of this file.
fn terminate(child: &mut Child) -> ExitStatus {
    #[cfg(unix)]
    unsafe {
        libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
    }
    if let Some(status) = wait_bounded(child, Duration::from_secs(15)) {
        return status;
    }
    let _ = child.kill();
    child.wait().expect("reaping the CLI child")
}

fn run(args: &[&str], envs: &[(&str, &str)]) -> Output {
    run_with_limit(args, envs, CHILD_TIMEOUT)
}

fn run_with_limit(args: &[&str], envs: &[(&str, &str)], limit: Duration) -> Output {
    let mut child = cli(args, envs).spawn().unwrap();
    let mut out = Tail::spawn(child.stdout.take().unwrap());
    let mut err = Tail::spawn(child.stderr.take().unwrap());
    let Some(status) = wait_bounded(&mut child, limit) else {
        let stderr = err.text();
        let stdout = out.text();
        terminate(&mut child);
        panic!(
            "`dcl-one-sdk {}` was still running after {limit:?} and was killed.\n\
             A CLI error path must not wait on a user: check for a fall-through to the \
             browser linker, whose default wait is {}s.\nstdout so far: {stdout}\n\
             stderr so far: {stderr}",
            args.join(" "),
            dcl_one_sdk::linker::DEFAULT_TIMEOUT_SECS,
        );
    };
    Output {
        status,
        stdout: out.finish(),
        stderr: err.finish(),
    }
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// The bound on [`run`], exercised on the one CLI path that really does wait on
/// a human: `deploy` with no key hands off to the browser linker and sits there
/// for `DCL_ONE_SDK_LINKER_TIMEOUT_SECS`, default 600. A test that lands on that
/// path by accident used to take the ten minutes and then pass, which is how a
/// six-second file turns into a twelve-minute CI outage nobody can attribute.
#[test]
fn a_cli_run_that_waits_on_a_human_fails_the_test_instead_of_waiting_with_it() {
    let f = Fixture::new("bound");
    f.write("scene.json", SCENE_OK);
    f.write("bin/index.js", "module.exports = {}\n");
    let dir = f.dir_arg();
    let args = [
        "deploy",
        "--dir",
        &dir,
        "--skip-build",
        "--target-content",
        "http://127.0.0.1:9",
        "--no-browser",
    ];
    // Long enough that a `run` with no bound would sit here, short enough that
    // this test still finishes if the bound is ever removed again.
    let envs = [("DCL_ONE_SDK_LINKER_TIMEOUT_SECS", "25")];

    let began = Instant::now();
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_with_limit(&args, &envs, Duration::from_secs(2))
    }));
    std::panic::set_hook(hook);
    let elapsed = began.elapsed();

    let panic = outcome.err().unwrap_or_else(|| {
        panic!(
            "run_with_limit waited out a child that hangs for 25s under a 2s bound \
             and returned normally after {elapsed:?}: the bound is not being applied"
        )
    });
    let message = panic
        .downcast_ref::<String>()
        .cloned()
        .unwrap_or_else(|| "<non-string panic>".to_string());
    assert!(
        elapsed < Duration::from_secs(20),
        "the bound fired only after {elapsed:?}"
    );
    assert!(
        message.contains("still running after"),
        "the failure must say the child was killed, not just fail: {message}"
    );
    assert!(
        message.contains("--target-content"),
        "the failure must name the argv that hung: {message}"
    );
    assert!(
        message.contains(&dcl_one_sdk::linker::DEFAULT_TIMEOUT_SECS.to_string()),
        "the failure must point at the product default that causes this: {message}"
    );
}

fn assert_contract(out: &Output, tmp: &Path, what_contains: &str) {
    let err = stderr_of(out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !out.status.success(),
        "expected failure, got {:?}\nstdout: {stdout}\nstderr: {err}",
        out.status
    );
    let error_lines: Vec<&str> = err.lines().filter(|l| l.starts_with("Error: ")).collect();
    assert_eq!(error_lines.len(), 1, "stderr: {err}");
    assert!(
        error_lines[0].contains(what_contains),
        "wanted {what_contains:?} in {:?}\nstderr: {err}",
        error_lines[0]
    );
    assert!(
        err.lines()
            .any(|l| l.trim_start().starts_with("\u{2192} try: ")),
        "no try line\nstderr: {err}"
    );
    assert!(!err.contains('\u{1b}'), "ANSI leaked\nstderr: {err}");
    assert!(!err.contains("os error"), "os error leaked\nstderr: {err}");
    assert!(!err.contains("Caused by"), "stderr: {err}");
    assert!(!err.contains("caused by:"), "stderr: {err}");
    let tmp_str = tmp.to_string_lossy();
    for line in err
        .lines()
        .filter(|l| l.trim_start().starts_with("\u{2192} try:"))
    {
        assert!(
            !line.contains(tmp_str.as_ref()),
            "try line leaks the fixture path: {line}"
        );
    }
}

fn assert_verbose_chain(args: &[&str], envs: &[(&str, &str)]) {
    let mut with_v: Vec<&str> = args.to_vec();
    with_v.push("--verbose");
    let out = run(&with_v, envs);
    assert!(!out.status.success());
    assert!(
        stderr_of(&out).contains("caused by:"),
        "stderr: {}",
        stderr_of(&out)
    );
}

#[test]
fn g1_not_a_scene() {
    let f = Fixture::new("g1");
    let dir = f.dir_arg();
    let args = ["build", "--dir", &dir];
    let out = run(&args, &[]);
    assert_contract(&out, f.path(), "not a Decentraland scene");
    assert_verbose_chain(&args, &[]);
}

#[test]
fn g2_malformed_scene_json() {
    let f = Fixture::new("g2");
    f.write("scene.json", "{oops");
    let dir = f.dir_arg();
    let args = ["build", "--dir", &dir];
    let out = run(&args, &[]);
    assert_contract(&out, f.path(), "scene.json is not valid JSON");
    assert_verbose_chain(&args, &[]);
}

#[test]
fn g3_sdk6_scene() {
    let f = Fixture::new("g3");
    f.write("scene.json", r#"{"main":"bin/index.js"}"#);
    let dir = f.dir_arg();
    let args = ["build", "--dir", &dir];
    let out = run(&args, &[]);
    assert_contract(&out, f.path(), "targets SDK 6");
    assert_verbose_chain(&args, &[]);
}

#[test]
fn g4_missing_main() {
    let f = Fixture::new("g4");
    f.write("scene.json", r#"{"runtimeVersion":"7"}"#);
    let dir = f.dir_arg();
    let args = ["build", "--dir", &dir];
    let out = run(&args, &[]);
    assert_contract(&out, f.path(), "scene.json is missing \"main\"");
    assert!(stderr_of(&out).contains("add \"main\": \"bin/index.js\""));
    assert_verbose_chain(&args, &[]);
}

#[test]
fn g5_missing_tsconfig() {
    let f = Fixture::new("g5");
    f.write("scene.json", SCENE_OK);
    let dir = f.dir_arg();
    let args = ["build", "--dir", &dir];
    let out = run(&args, &[]);
    assert_contract(&out, f.path(), "no tsconfig.json");
    assert!(stderr_of(&out).contains("tsconfig.ecs7.json"));
    assert_verbose_chain(&args, &[]);
}

#[test]
fn g6_missing_node_modules() {
    let f = Fixture::new("g6");
    f.write("scene.json", SCENE_OK);
    f.write("tsconfig.json", TSCONFIG);
    let dir = f.dir_arg();
    let args = ["build", "--dir", &dir];
    let out = run(&args, &[]);
    assert_contract(&out, f.path(), "@dcl/sdk is not installed");
    assert!(stderr_of(&out).contains("npm install"));
    assert_verbose_chain(&args, &[]);
}

#[test]
fn g7_main_is_a_directory() {
    let f = Fixture::new("g7");
    f.write("scene.json", r#"{"main":"bin","runtimeVersion":"7"}"#);
    f.mkdir("bin");
    let dir = f.dir_arg();
    let args = ["build", "--dir", &dir];
    let out = run(&args, &[]);
    assert_contract(&out, f.path(), "which is a directory");
    assert!(stderr_of(&out).contains("set \"main\": \"bin/index.js\""));
    assert_verbose_chain(&args, &[]);
}

#[test]
fn g8_port_already_in_use() {
    let f = Fixture::new("g8");
    f.write("scene.json", SCENE_OK);
    let holder = TcpListener::bind("0.0.0.0:0").unwrap();
    let port = holder.local_addr().unwrap().port().to_string();
    let dir = f.dir_arg();
    let args = [
        "start",
        "--dir",
        &dir,
        "--port",
        &port,
        "--skip-build",
        "--no-watch",
    ];
    let out = run(&args, &[]);
    assert_contract(&out, f.path(), &format!("port {port} is already in use"));
    assert!(stderr_of(&out).contains("--port"));
    assert_verbose_chain(&args, &[]);
}

#[test]
fn g9_headless_key_requires_explicit_target() {
    let f = Fixture::new("g9");
    f.write("scene.json", SCENE_OK);
    f.write("bin/index.js", "module.exports = {}\n");
    let key = (
        "DCL_PRIVATE_KEY",
        "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d",
    );
    let dir = f.dir_arg();
    let args = ["deploy", "--dir", &dir, "--skip-build"];
    let out = run(&args, &[key]);
    assert_contract(&out, f.path(), "no deploy target given");
    let err = stderr_of(&out);
    assert!(err.contains("--target-content"));
    assert!(err.contains("DCL_ONE_SDK_DEFAULT_TARGET"));
    assert_verbose_chain(&args, &[key]);
}

#[test]
fn g13_linker_always_prints_the_signing_url() {
    let f = Fixture::new("g13");
    f.write("scene.json", SCENE_OK);
    f.write("bin/index.js", "module.exports = {}\n");
    let envs = [("DCL_ONE_SDK_LINKER_TIMEOUT_SECS", "1")];
    let dir = f.dir_arg();
    let args = [
        "deploy",
        "--dir",
        &dir,
        "--skip-build",
        "--target-content",
        "http://127.0.0.1:9",
        "--no-browser",
    ];
    let out = run(&args, &envs);
    assert_contract(&out, f.path(), "no signature arrived");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("http://localhost:"),
        "signing URL missing from stdout: {stdout}"
    );
    assert!(stderr_of(&out).contains("DCL_ONE_SDK_LINKER_TIMEOUT_SECS"));
}

#[test]
fn g10_content_server_unreachable() {
    let f = Fixture::new("g10");
    f.write("scene.json", SCENE_OK);
    f.write("bin/index.js", "module.exports = {}\n");
    let key = (
        "DCL_PRIVATE_KEY",
        "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d",
    );
    let dir = f.dir_arg();
    let args = [
        "deploy",
        "--dir",
        &dir,
        "--skip-build",
        "--target-content",
        "http://127.0.0.1:9",
    ];
    let out = run(&args, &[key]);
    assert_contract(&out, f.path(), "could not reach the content server");
    assert!(stderr_of(&out).contains("--target-content"));
    assert_verbose_chain(&args, &[key]);
}

#[test]
fn g11_no_parcels() {
    let f = Fixture::new("g11");
    f.write(
        "scene.json",
        r#"{"main":"bin/index.js","runtimeVersion":"7"}"#,
    );
    f.write("bin/index.js", "module.exports = {}\n");
    let dir = f.dir_arg();
    let args = ["deploy", "--dir", &dir, "--skip-build", "--dry-run"];
    let out = run(&args, &[]);
    assert_contract(&out, f.path(), "declares no parcels");
    assert!(stderr_of(&out).contains("\"parcels\": [\"0,0\"]"));
    assert_verbose_chain(&args, &[]);
}

#[test]
fn g14_init_non_empty_dir() {
    let f = Fixture::new("g14");
    f.write("existing.txt", "already here");
    let dir = f.dir_arg();
    let args = ["init", "--dir", &dir];
    let out = run(&args, &[]);
    assert_contract(&out, f.path(), "the target directory is not empty");
    let err = stderr_of(&out);
    assert!(err.contains("--yes"), "stderr: {err}");
    assert!(err.contains("mkdir my-scene"), "stderr: {err}");
    assert_verbose_chain(&args, &[]);
}

#[test]
fn g15_init_target_is_a_file() {
    let f = Fixture::new("g15");
    f.write("blocker.txt", "x");
    let target = f.path().join("blocker.txt").display().to_string();
    let args = ["init", "--dir", &target];
    let out = run(&args, &[]);
    assert_contract(&out, f.path(), "is a file, not a directory");
    assert_verbose_chain(&args, &[]);
}

#[test]
fn g16_pack_needs_a_wearable_json() {
    let f = Fixture::new("g16");
    f.write("scene.json", SCENE_OK);
    let dir = f.dir_arg();
    let args = ["pack", "--dir", &dir, "--skip-build"];
    let out = run(&args, &[]);
    assert_contract(&out, f.path(), "not a smart wearable");
    assert!(
        stderr_of(&out).contains("init --project smart-wearable"),
        "stderr: {}",
        stderr_of(&out)
    );
    assert_verbose_chain(&args, &[]);
}

#[test]
fn g17_pack_malformed_wearable_json() {
    let f = Fixture::new("g17");
    f.write("scene.json", SCENE_OK);
    f.write("wearable.json", "{oops");
    let dir = f.dir_arg();
    let args = ["pack", "--dir", &dir, "--skip-build"];
    let out = run(&args, &[]);
    assert_contract(&out, f.path(), "wearable.json is not valid JSON");
    assert_verbose_chain(&args, &[]);
}

#[test]
fn g18_pack_names_the_broken_field() {
    let f = Fixture::new("g18");
    f.write("scene.json", SCENE_OK);
    f.write(
        "wearable.json",
        r#"{"name":"Test","rarity":"shiny","data":{"category":"eyewear","representations":[]}}"#,
    );
    let dir = f.dir_arg();
    let args = ["pack", "--dir", &dir, "--skip-build"];
    let out = run(&args, &[]);
    assert_contract(&out, f.path(), "\"shiny\", which is not a rarity");
    assert!(
        stderr_of(&out).contains("unique, mythic"),
        "stderr: {}",
        stderr_of(&out)
    );
    assert_verbose_chain(&args, &[]);
}

#[test]
fn g19_pack_oversize_warns_but_still_packs() {
    let f = Fixture::new("g19");
    f.write("scene.json", SCENE_OK);
    f.write("wearable.json", WEARABLE_OK);
    f.write_bytes("model.glb", &vec![0x47u8; 3_000_000]);
    f.write("bin/index.js", "module.exports = {}\n");
    let dir = f.dir_arg();
    let args = ["pack", "--dir", &dir, "--skip-build"];
    let out = run(&args, &[]);
    let err = stderr_of(&out);
    assert!(
        out.status.success(),
        "pack should still succeed\nstderr: {err}"
    );
    assert!(err.contains("2097152"), "stderr: {err}");
    assert!(err.contains(".dclignore"), "stderr: {err}");
    assert!(f.path().join("smart-wearable.zip").is_file());
}

#[test]
fn g20_workspace_missing_member() {
    let f = Fixture::new("g20");
    f.write("dcl-workspace.json", r#"{"folders":[{"path":"missing"}]}"#);
    let dir = f.dir_arg();
    let args = ["build", "--dir", &dir];
    let out = run(&args, &[]);
    assert_contract(&out, f.path(), "does not exist");
    assert_verbose_chain(&args, &[]);
}

#[test]
fn g21_workspace_empty_folders() {
    let f = Fixture::new("g21");
    f.write("dcl-workspace.json", r#"{"folders":[]}"#);
    let dir = f.dir_arg();
    let args = ["build", "--dir", &dir];
    let out = run(&args, &[]);
    assert_contract(&out, f.path(), "must list at least one folder");
    assert!(
        stderr_of(&out).contains(r#"{ "folders": [ { "path": "scene-a" }"#),
        "stderr: {}",
        stderr_of(&out)
    );
    assert_verbose_chain(&args, &[]);
}

#[test]
fn g22_deploy_at_workspace_root() {
    let f = Fixture::new("g22");
    f.write("dcl-workspace.json", r#"{"folders":[{"path":"scene-a"}]}"#);
    f.write("scene-a/scene.json", SCENE_OK);
    let dir = f.dir_arg();
    let args = ["deploy", "--dir", &dir, "--skip-build", "--dry-run"];
    let out = run(&args, &[]);
    assert_contract(&out, f.path(), "workspace root, not a single scene");
    assert_verbose_chain(&args, &[]);
}

#[test]
fn g23_tunnel_url_invalid() {
    let f = Fixture::new("g23");
    f.write("scene.json", SCENE_OK);
    let dir = f.dir_arg();
    let args = [
        "start",
        "--dir",
        &dir,
        "--skip-build",
        "--no-watch",
        "--tunnel",
        "ftp://x",
    ];
    let out = run(&args, &[]);
    assert_contract(&out, f.path(), "invalid --tunnel URL");
    assert!(
        stderr_of(&out).contains("--tunnel help"),
        "stderr: {}",
        stderr_of(&out)
    );
    assert_verbose_chain(&args, &[]);
}

/// pids of every abgen running out of `root`, which is one preview server's
/// private TMPDIR — so the answer is that server's sidecars and nobody else's.
#[cfg(unix)]
fn sidecars_under(root: &Path) -> Vec<i32> {
    let needle = root.to_string_lossy().into_owned();
    let out = Command::new("ps")
        .args(["-Ao", "pid=,command="])
        .output()
        .expect("ps");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| l.contains(&needle) && l.contains("abgen"))
        .filter_map(|l| l.split_whitespace().next()?.parse().ok())
        .collect()
}

#[test]
fn g24_tunnel_unreachable_warns_and_keeps_serving() {
    let f = Fixture::new("g24");
    f.write("scene.json", SCENE_OK);
    // Its own TMPDIR. Two reasons: the preview server inflates its embedded
    // abgen there, so the copy is guaranteed cold and this test exercises the
    // slow start rather than depending on a warm machine; and the sidecar is
    // then identifiable by path, which is what lets the orphan assertion below
    // tell this run's sidecar from every other one on the box.
    let tmpdir = f.path().join("tmp");
    std::fs::create_dir_all(&tmpdir).unwrap();
    let dir = f.dir_arg();
    let mut cmd = cli(
        &[
            "start",
            "--dir",
            &dir,
            "--port",
            "0",
            "--skip-build",
            "--no-watch",
            "--tunnel",
            "ws://127.0.0.1:9",
        ],
        &[],
    );
    cmd.env("TMPDIR", &tmpdir);
    let mut child = cmd.spawn().unwrap();
    let mut out = Tail::spawn(child.stdout.take().unwrap());
    let mut err = Tail::spawn(child.stderr.take().unwrap());

    // Poll for the warning; do not sleep a fixed interval and hope.
    //
    // The tunnel warning is printed after the join block, which is printed
    // after the abgen sidecar reports ready, which on a cold machine is behind
    // a 36 MB inflate out of the binary — 6.9s when this was written. The
    // fixed 6s sleep this replaces therefore failed outright on any machine
    // that had not already unpacked this exact build's sidecar, and passed only
    // because CI happened to be warm.
    let deadline = Instant::now() + CHILD_TIMEOUT;
    while !err.text().contains("warning: tunnel connection failed") {
        assert!(
            child.try_wait().unwrap().is_none(),
            "the preview server must keep serving while the tunnel retries; it exited\n\
             stdout: {}\nstderr: {}",
            out.text(),
            err.text()
        );
        assert!(
            Instant::now() < deadline,
            "no tunnel warning within {CHILD_TIMEOUT:?}\nstdout: {}\nstderr: {}",
            out.text(),
            err.text()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        child.try_wait().unwrap().is_none(),
        "the preview server must keep serving while the tunnel retries"
    );

    #[cfg(unix)]
    let sidecars = {
        let pids = sidecars_under(&tmpdir);
        assert!(
            !pids.is_empty(),
            "no abgen sidecar under {}: the orphan check below would assert nothing. \
             If `start` legitimately stopped spawning one, delete this block on purpose.\n\
             stdout: {}",
            tmpdir.display(),
            out.text()
        );
        pids
    };

    terminate(&mut child);
    let err = String::from_utf8_lossy(&err.finish()).into_owned();
    let _ = out.finish();

    // The sidecar must die with the server it belongs to. It is spawned into
    // its own process group, so nothing in the OS does this for us: `start`
    // has to do it on the way out, and if it stops, every preview a developer
    // or a CI job ever ran stays resident.
    #[cfg(unix)]
    {
        let gone = Instant::now() + Duration::from_secs(20);
        while !sidecars_under(&tmpdir).is_empty() {
            assert!(
                Instant::now() < gone,
                "abgen {:?} outlived the preview server it belongs to (still {:?} after SIGTERM)",
                sidecars,
                sidecars_under(&tmpdir)
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    assert!(
        err.contains("warning: tunnel connection failed"),
        "stderr: {err}"
    );
    assert!(
        err.lines()
            .any(|l| l.trim_start().starts_with("\u{2192} try: ")),
        "no try line\nstderr: {err}"
    );
    assert!(err.contains("--tunnel help"), "stderr: {err}");
    assert!(!err.contains('\u{1b}'), "ANSI leaked\nstderr: {err}");
    assert!(!err.contains("os error"), "os error leaked\nstderr: {err}");
}

#[test]
fn start_tolerates_skip_install_flag() {
    let f = Fixture::new("skipinstall");
    let dir = f.dir_arg();
    let out = run(
        &[
            "start",
            "--dir",
            &dir,
            "--skip-install",
            "--skip-build",
            "--no-watch",
            "--port",
            "0",
        ],
        &[],
    );
    let err = stderr_of(&out);
    assert!(
        !err.contains("unexpected argument"),
        "clap rejected --skip-install\nstderr: {err}"
    );
    assert!(err.contains("not a Decentraland scene"), "stderr: {err}");
}

#[test]
fn g25_target_and_target_content_conflict() {
    let f = Fixture::new("g25");
    f.write("scene.json", SCENE_OK);
    f.write("bin/index.js", "module.exports = {}\n");
    let dir = f.dir_arg();
    let args = [
        "deploy",
        "--dir",
        &dir,
        "--skip-build",
        "--target",
        "peer.decentraland.org",
        "--target-content",
        "http://127.0.0.1:9",
    ];
    let out = run(&args, &[]);
    assert_contract(&out, f.path(), "not both");
    assert_verbose_chain(&args, &[]);
}

#[test]
fn g26_catalyst_about_probe_failure() {
    let f = Fixture::new("g26");
    f.write("scene.json", SCENE_OK);
    f.write("bin/index.js", "module.exports = {}\n");
    let dir = f.dir_arg();
    let args = [
        "deploy",
        "--dir",
        &dir,
        "--skip-build",
        "--target",
        "http://127.0.0.1:9",
    ];
    let out = run(&args, &[]);
    assert_contract(&out, f.path(), "could not resolve the catalyst");
    assert!(
        stderr_of(&out).contains("--target-content"),
        "stderr: {}",
        stderr_of(&out)
    );
    assert_verbose_chain(&args, &[]);
}

#[test]
fn g27_malformed_private_key_env() {
    let f = Fixture::new("g27");
    f.write("scene.json", SCENE_OK);
    f.write("bin/index.js", "module.exports = {}\n");
    let key = ("DCL_PRIVATE_KEY", "not-a-key");
    let dir = f.dir_arg();
    let args = [
        "deploy",
        "--dir",
        &dir,
        "--skip-build",
        "--target-content",
        "http://127.0.0.1:9",
    ];
    let out = run(&args, &[key]);
    assert_contract(&out, f.path(), "DCL_PRIVATE_KEY is not a valid private key");
    assert!(
        stderr_of(&out).contains("64 hex chars"),
        "stderr: {}",
        stderr_of(&out)
    );
    assert_verbose_chain(&args, &[key]);
}

#[test]
fn g28_sign_key_file_missing() {
    let f = Fixture::new("g28");
    f.write("scene.json", SCENE_OK);
    f.write("bin/index.js", "module.exports = {}\n");
    let missing = f.path().join("no-such-key").display().to_string();
    let dir = f.dir_arg();
    let args = [
        "deploy",
        "--dir",
        &dir,
        "--skip-build",
        "--target-content",
        "http://127.0.0.1:9",
        "--sign-key",
        &missing,
    ];
    let out = run(&args, &[]);
    assert_contract(&out, f.path(), "could not read the key file");
    assert!(
        stderr_of(&out).contains("--sign-key"),
        "stderr: {}",
        stderr_of(&out)
    );
    assert_verbose_chain(&args, &[]);
}

#[test]
fn g29_main_is_not_a_js_bundle() {
    let f = Fixture::new("g29");
    f.write(
        "scene.json",
        r#"{"main":"src/index.ts","runtimeVersion":"7"}"#,
    );
    let dir = f.dir_arg();
    let args = ["build", "--dir", &dir];
    let out = run(&args, &[]);
    assert_contract(&out, f.path(), "must be a .js bundle path");
    assert!(
        stderr_of(&out).contains("bin/index.js"),
        "stderr: {}",
        stderr_of(&out)
    );
    assert_verbose_chain(&args, &[]);
}

#[cfg(not(feature = "rolldown"))]
#[test]
fn g30_rolldown_backend_needs_the_feature() {
    let f = Fixture::new("g30");
    f.write("scene.json", SCENE_OK);
    f.write("tsconfig.json", TSCONFIG);
    f.mkdir("node_modules/@dcl/sdk");
    let dir = f.dir_arg();
    let args = ["build", "--dir", &dir];
    let out = run(&args, &[]);
    assert_contract(&out, f.path(), "without the rolldown backend");
    assert!(
        stderr_of(&out).contains("--features rolldown"),
        "stderr: {}",
        stderr_of(&out)
    );
    assert_verbose_chain(&args, &[]);
}

/// A world scene with no target resolves to the public worlds server instead
/// of refusing. The additive default keeps the run hermetic (no overwrite
/// probe of the real server) and the 1s linker timeout is what ends it.
#[test]
fn g31_world_deploy_defaults_to_the_public_worlds_server() {
    let f = Fixture::new("g31");
    f.write("scene.json", SCENE_WORLD);
    f.write("bin/index.js", "module.exports = {}\n");
    let envs = [("DCL_ONE_SDK_LINKER_TIMEOUT_SECS", "1")];
    let dir = f.dir_arg();
    let args = ["deploy", "--dir", &dir, "--skip-build", "--no-browser"];
    let out = run(&args, &envs);
    assert_contract(&out, f.path(), "no signature arrived");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("worlds-content-server.decentraland.org"),
        "worlds default missing from stdout: {stdout}"
    );
}

#[test]
fn g32_dir_does_not_exist() {
    let f = Fixture::new("g32");
    let missing = f.path().join("nope").display().to_string();
    let args = ["build", "--dir", &missing];
    let out = run(&args, &[]);
    assert_contract(&out, f.path(), "does not exist");
    assert!(
        stderr_of(&out).contains("--dir"),
        "stderr: {}",
        stderr_of(&out)
    );
    assert_verbose_chain(&args, &[]);
}

/// The scene checkout t2/t6 borrow a real `node_modules` from. Goes through the
/// testgate rather than `expect`, so `--include-ignored` without the variable
/// fails naming the variable and pointing at the opt-out, instead of unwrapping.
fn provisioned_scene() -> Option<PathBuf> {
    catalyrst_testgate::require_env("DCL_ONE_SDK_TEST_SCENE").map(PathBuf::from)
}

#[test]
#[ignore = "needs DCL_ONE_SDK_TEST_SCENE: an installed scene checkout to borrow node_modules from, so the real compiler runs; see docs/testing.md"]
fn t2_syntax_error_renders_code_frame() {
    let Some(src) = provisioned_scene() else {
        return;
    };
    let f = Fixture::new("t2");
    f.write("scene.json", SCENE_OK);
    f.write("tsconfig.json", TSCONFIG);
    f.write("src/index.ts", "export function main() { const x = = 1 }\n");
    std::os::unix::fs::symlink(src.join("node_modules"), f.path().join("node_modules")).unwrap();
    let dir = f.dir_arg();
    let args = ["build", "--dir", &dir];
    let out = run(&args, &[]);
    assert_contract(&out, f.path(), "build failed");
    assert!(
        stderr_of(&out).contains('^'),
        "no caret frame: {}",
        stderr_of(&out)
    );
}

#[test]
#[ignore = "needs DCL_ONE_SDK_TEST_SCENE: an installed scene checkout to borrow node_modules from, so the real compiler runs; see docs/testing.md"]
fn t6_type_error_framing_and_skip_tip() {
    let Some(src) = provisioned_scene() else {
        return;
    };
    let f = Fixture::new("t6");
    f.write("scene.json", SCENE_OK);
    f.write("tsconfig.json", TSCONFIG);
    f.write(
        "src/index.ts",
        "export function main() { const x: number = 'nope'; return x }\n",
    );
    std::os::unix::fs::symlink(src.join("node_modules"), f.path().join("node_modules")).unwrap();
    let dir = f.dir_arg();
    let args = ["build", "--dir", &dir];
    let out = run(&args, &[]);
    assert_contract(&out, f.path(), "type check failed");
    assert!(stderr_of(&out).contains("--skip-type-check"));
}
