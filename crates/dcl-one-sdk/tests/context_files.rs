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
    assert!(stdout.contains("\u{2713} Valid Scene project"), "{stdout}");
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

#[test]
fn unreachable_listing_is_a_user_error() {
    let f = Fixture::new("down");
    f.make_project();
    let dir = f.dir_arg();
    let out = run(
        &["get-context-files", "--dir", &dir],
        &[("DCL_ONE_SDK_CONTEXT_API", "http://127.0.0.1:9/api/root")],
    );
    assert!(!out.status.success());
    let err = stderr_of(&out);
    let error_lines: Vec<&str> = err.lines().filter(|l| l.starts_with("Error: ")).collect();
    assert_eq!(error_lines.len(), 1, "stderr: {err}");
    assert!(
        error_lines[0].contains("could not list the AI context files"),
        "{err}"
    );
    assert!(err.contains("\u{2192} try: "), "{err}");
    assert!(!err.contains('\u{1b}'), "ANSI leaked: {err}");
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
    assert!(stdout_of(&out).contains("Valid Smart Wearable project"));
}
