use axum::routing::get;
use axum::{Json, Router};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_dcl-one-sdk");

struct Fixture(PathBuf);

impl Fixture {
    fn new(tag: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("dcl-one-sdk-ctxgate-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Fixture(dir)
    }

    fn write(&self, rel: &str, contents: &str) {
        let p = self.0.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, contents).unwrap();
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn dir_arg(&self) -> String {
        self.0.display().to_string()
    }

    fn make_project(&self) {
        self.write("package.json", "{}");
        self.write("scene.json", "{}");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(args: &[&str], envs: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(BIN);
    cmd.args(args).stdin(Stdio::null());
    for k in ["RUST_LOG", "NO_COLOR", "DCL_ONE_SDK_CONTEXT_API"] {
        cmd.env_remove(k);
    }
    for (k, v) in envs {
        cmd.env(k, v);
    }
    cmd.output().unwrap()
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

async fn serve_mock(fail_b: bool) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let root_base = base.clone();
    let sub_base = base.clone();
    let app = Router::new()
        .route(
            "/api/root",
            get(move || {
                let base = root_base.clone();
                async move {
                    Json(serde_json::json!([
                        {
                            "name": "a.md",
                            "path": "ai-sdk-context/a.md",
                            "type": "file",
                            "download_url": format!("{base}/dl/a.md")
                        },
                        {
                            "name": "sub",
                            "path": "ai-sdk-context/sub",
                            "type": "dir",
                            "url": format!("{base}/api/sub")
                        }
                    ]))
                }
            }),
        )
        .route(
            "/api/sub",
            get(move || {
                let base = sub_base.clone();
                async move {
                    Json(serde_json::json!([
                        {
                            "name": "b.md",
                            "path": "ai-sdk-context/sub/b.md",
                            "type": "file",
                            "download_url": format!("{base}/dl/b.md")
                        }
                    ]))
                }
            }),
        )
        .route("/dl/a.md", get(|| async { "alpha" }))
        .route(
            "/dl/b.md",
            get(move || async move {
                if fail_b {
                    Err(axum::http::StatusCode::NOT_FOUND)
                } else {
                    Ok("beta")
                }
            }),
        );
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    base
}

#[test]
fn non_project_directory_exits_zero_with_guidance() {
    let f = Fixture::new("noproj");
    let dir = f.dir_arg();
    let out = run(&["get-context-files", "--dir", &dir], &[]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(stdout.contains("not a Decentraland project"), "{stdout}");
    assert!(stdout.contains("dcl-one-sdk init"), "{stdout}");
    assert!(!f.path().join("dclcontext").exists());
    // The command is scene-scoped; a .claude/skills/ in a random cwd is litter.
    assert!(!f.path().join(".claude").exists());
}

#[tokio::test]
async fn fetches_recursively_flat_and_replaces_old_context() {
    let base = serve_mock(false).await;
    let f = Fixture::new("fetch");
    f.make_project();
    f.write("dclcontext/stale.md", "old");
    let api = format!("{base}/api/root");
    let dir = f.dir_arg();
    let out = tokio::task::spawn_blocking(move || {
        run(
            &["get-context-files", "--dir", &dir],
            &[("DCL_ONE_SDK_CONTEXT_API", api.as_str())],
        )
    })
    .await
    .unwrap();
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(stdout.contains("\u{2713} Scene project"), "{stdout}");
    assert!(
        stdout.contains("\u{2713} Saved ai-sdk-context/a.md"),
        "{stdout}"
    );
    assert!(
        stdout.contains("\u{2713} Saved ai-sdk-context/sub/b.md"),
        "{stdout}"
    );
    assert!(
        stdout.contains("Download complete: 2 successful, 0 failed"),
        "{stdout}"
    );
    assert_eq!(
        std::fs::read_to_string(f.path().join("dclcontext/a.md")).unwrap(),
        "alpha"
    );
    assert_eq!(
        std::fs::read_to_string(f.path().join("dclcontext/b.md")).unwrap(),
        "beta"
    );
    assert!(!f.path().join("dclcontext/stale.md").exists());
    assert!(!f.path().join("dclcontext/sub").exists());
    // Both halves in one run: the embedded skill lands even on the happy path.
    assert!(skill_md(&f).is_file());
}

#[tokio::test]
async fn partial_download_failure_is_reported_not_fatal() {
    let base = serve_mock(true).await;
    let f = Fixture::new("partial");
    f.make_project();
    let api = format!("{base}/api/root");
    let dir = f.dir_arg();
    let out = tokio::task::spawn_blocking(move || {
        run(
            &["get-context-files", "--dir", &dir],
            &[("DCL_ONE_SDK_CONTEXT_API", api.as_str())],
        )
    })
    .await
    .unwrap();
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("\u{2717} Failed to download ai-sdk-context/sub/b.md"),
        "{stdout}"
    );
    assert!(
        stdout.contains("Download complete: 1 successful, 1 failed"),
        "{stdout}"
    );
    assert!(f.path().join("dclcontext/a.md").is_file());
    assert!(!f.path().join("dclcontext/b.md").exists());
}

/// An unreachable GitHub used to be a hard error, which made the offline half
/// of this command unreachable exactly when it was most useful. It is now a
/// note: the bundled skill is installed, `dclcontext/` is left as it was, and
/// the process exits 0.
#[test]
fn unreachable_listing_still_installs_the_skill_and_exits_zero() {
    let f = Fixture::new("down");
    f.make_project();
    f.write("dclcontext/keep.md", "corpus");
    let dir = f.dir_arg();
    let out = run(
        &["get-context-files", "--dir", &dir],
        &[("DCL_ONE_SDK_CONTEXT_API", "http://127.0.0.1:9/api/root")],
    );
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let stdout = stdout_of(&out);
    // Count-independent: the number comes from the build.rs-generated skills
    // table, so pinning it is what made this assertion go stale in cfa0f5a15.
    assert!(stdout.contains("skills into .claude/skills/"), "{stdout}");
    assert!(
        stdout.contains("Could not reach the ai-sdk-context corpus"),
        "{stdout}"
    );
    assert!(skill_md(&f).is_file());
    assert_eq!(
        std::fs::read_to_string(f.path().join("dclcontext/keep.md")).unwrap(),
        "corpus",
        "a failed listing must not touch an existing corpus"
    );
}

fn skill_md(f: &Fixture) -> PathBuf {
    f.path()
        .join(".claude/skills/migrate-smart-items-to-code/SKILL.md")
}

/// The skill comes out of the binary, so it is byte-identical to the crate's
/// `skills/` source with no checkout, no npm and no network in the picture.
#[test]
fn offline_installs_the_skill_from_the_binary() {
    let f = Fixture::new("offline");
    f.make_project();
    let dir = f.dir_arg();
    let out = run(
        &["get-context-files", "--dir", &dir, "--offline"],
        // Unroutable: if --offline dialled out, this would hang, not return.
        &[("DCL_ONE_SDK_CONTEXT_API", "http://198.51.100.1/api/root")],
    );
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(stdout.contains("skipping the GitHub"), "{stdout}");

    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("skills/migrate-smart-items-to-code");
    for rel in [
        "SKILL.md",
        "references/actions.md",
        "references/triggers.md",
    ] {
        let shipped = f
            .path()
            .join(".claude/skills/migrate-smart-items-to-code")
            .join(rel);
        assert_eq!(
            std::fs::read(&shipped).unwrap(),
            std::fs::read(src.join(rel)).unwrap(),
            "{rel} differs from the crate source"
        );
    }
    // Frontmatter is what Claude Code matches a request against.
    let head = std::fs::read_to_string(skill_md(&f)).unwrap();
    assert!(
        head.starts_with("---\nname: migrate-smart-items-to-code\n"),
        "{head:.80}"
    );
    assert!(!f.path().join("dclcontext").exists());
}

#[test]
fn wearable_project_is_recognized() {
    let f = Fixture::new("sw");
    f.write("package.json", "{}");
    f.write("wearable.json", "{}");
    let dir = f.dir_arg();
    let out = run(
        &["get-context-files", "--dir", &dir],
        &[("DCL_ONE_SDK_CONTEXT_API", "http://127.0.0.1:9/api/root")],
    );
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("\u{2713} Smart Wearable project"),
        "{stdout}"
    );
}
