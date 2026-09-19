use super::*;
use crate::start::testkit::Tmp;

#[test]
fn four_providers_keep_permission_policy_and_use_the_project() {
    let temp = Tmp::new("assistant-args");
    for (id, _, _) in provider::PROVIDERS {
        let invocation =
            provider::invocation(id, "Build a chair", None, None, &temp.0, None).unwrap();
        let args = invocation.args.join(" ");
        for bypass in [
            "bypassPermissions",
            "danger-full-access",
            "--yolo",
            "--force",
            "-f ",
        ] {
            assert!(!args.contains(bypass), "{id}: {args}");
        }
        assert!(args.contains("json"));
        assert!(args.contains("Build a chair") || invocation.stdin.contains("Build a chair"));
    }
    let codex =
        provider::invocation("codex", "Continue", Some("thread-1"), None, &temp.0, None).unwrap();
    assert_eq!(&codex.args[..3], ["exec", "resume", "thread-1"]);
    assert!(
        provider::invocation("gemini", "Continue", Some("unstable"), None, &temp.0, None).is_err()
    );
    assert!(!identifier("--danger"));
}

#[test]
fn provider_streams_normalize_text_tools_sessions_and_failures_without_final_duplicates() {
    assert_eq!(
        provider::events("codex", r#"{"type":"thread.started","thread_id":"one"}"#)[0]["sessionId"],
        "one"
    );
    assert_eq!(
        provider::events(
            "codex",
            r#"{"type":"item.completed","item":{"type":"agent_message","text":"Done"}}"#
        )[0]["text"],
        "Done"
    );
    assert_eq!(
        provider::events(
            "codex",
            r#"{"type":"turn.failed","error":{"message":"Sign in"}}"#
        )[0]["message"],
        "Sign in"
    );
    for id in ["claude", "cursor"] {
        assert_eq!(
            provider::events(
                id,
                r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Hello"}]}}"#
            )[0]["text"],
            "Hello"
        );
        let result = provider::events(
            id,
            r#"{"type":"result","session_id":"one","result":"Hello"}"#,
        );
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["type"], "session");
    }
    assert_eq!(
        provider::events(
            "claude",
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read","input":{"path":"src/index.ts"}}]}}"#
        )[0]["type"],
        "tool"
    );
    assert_eq!(
        provider::events(
            "cursor",
            r#"{"type":"tool_call","subtype":"started","tool_call":{"readToolCall":{"args":{"path":"src/index.ts"}}}}"#
        )[0]["type"],
        "tool"
    );
    assert_eq!(
        provider::events(
            "gemini",
            r#"{"type":"message","role":"assistant","content":"Hello"}"#
        )[0]["text"],
        "Hello"
    );
    assert!(provider::events(
        "gemini",
        r#"{"type":"message","role":"user","content":"Hello"}"#
    )
    .is_empty());
    assert_eq!(
        provider::events(
            "gemini",
            r#"{"type":"result","status":"error","error":{"message":"No credentials"}}"#
        )[0]["message"],
        "No credentials"
    );
}

#[test]
fn scene_tool_config_uses_sdk_proxy_without_credentials_and_restores_user_settings() {
    let temp = Tmp::new("assistant-mcp");
    for (id, path) in [
        ("cursor", ".cursor/mcp.json"),
        ("gemini", ".gemini/settings.json"),
    ] {
        let path = temp.0.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = r#"{"mcpServers":{"mine":{"command":"my-server"}},"theme":"dark"}"#;
        std::fs::write(&path, original).unwrap();
        {
            let call = provider::invocation(
                id,
                "Hello",
                None,
                None,
                &temp.0,
                Some("http://localhost:8000/api/project/assistant/mcp"),
            )
            .unwrap();
            assert_eq!(call.overlays.len(), 1);
            let data: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            assert_eq!(data["theme"], "dark");
            assert_eq!(data["mcpServers"]["mine"]["command"], "my-server");
            assert!(data["mcpServers"]["creator-hub"]["headers"].is_null());
        }
        assert_eq!(std::fs::read_to_string(path).unwrap(), original);
    }
    let codex = provider::invocation(
        "codex",
        "Hello",
        None,
        None,
        &temp.0,
        Some("http://localhost:8000/api/project/assistant/mcp"),
    )
    .unwrap();
    assert!(codex
        .args
        .iter()
        .any(|a| a.contains("mcp_servers.creator-hub.url")));
    assert!(!codex.args.iter().any(|a| a.contains("bearer")));
}

#[cfg(unix)]
fn fake_child(script: &str, root: &std::path::Path) -> tokio::process::Child {
    use std::os::unix::process::CommandExt;
    let mut command = tokio::process::Command::new("sh");
    command
        .args(["-c", script])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    command.as_std_mut().process_group(0);
    command.spawn().unwrap()
}

#[cfg(unix)]
#[tokio::test]
async fn fake_cli_writes_project_streams_output_and_reports_exit() {
    let temp = Tmp::new("assistant-stream");
    let child=fake_child("printf authored > result.txt; printf '%s\\n' '{\"type\":\"thread.started\",\"thread_id\":\"test-session\"}' '{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"Saved\"}}'",&temp.0);
    let (_cancel, rx) = watch::channel(false);
    let (tx, mut output) = mpsc::channel(64);
    run_child(child, "codex", rx, tx).await;
    let mut events = Vec::new();
    while let Some(event) = output.recv().await {
        events.push(event);
    }
    assert_eq!(events[0]["sessionId"], "test-session");
    assert_eq!(events[1]["text"], "Saved");
    assert_eq!(
        events.last().unwrap(),
        &json!({"type":"done","exitCode":0,"cancelled":false})
    );
    assert_eq!(
        std::fs::read_to_string(temp.0.join("result.txt")).unwrap(),
        "authored"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn cancelling_a_fake_cli_kills_the_turn_and_its_child_processes() {
    let temp = Tmp::new("assistant-cancel");
    let child=fake_child("printf '%s\\n' '{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"Ready\"}}'; sleep 5; printf leaked > leaked.txt",&temp.0);
    let (cancel, rx) = watch::channel(false);
    let (tx, mut output) = mpsc::channel(64);
    let task = tokio::spawn(async move {
        run_child(child, "codex", rx, tx).await;
    });
    assert_eq!(output.recv().await.unwrap()["text"], "Ready");
    cancel.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(output.recv().await.unwrap()["cancelled"], true);
    assert!(!temp.0.join("leaked.txt").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn closing_the_stream_cancels_the_cli() {
    let temp = Tmp::new("assistant-disconnect");
    let child = fake_child("sleep 5", &temp.0);
    let (_cancel, rx) = watch::channel(false);
    let (tx, output) = mpsc::channel(64);
    drop(output);
    tokio::time::timeout(Duration::from_secs(2), run_child(child, "codex", rx, tx))
        .await
        .unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn cancellation_works_when_a_browser_stops_reading_output() {
    let temp = Tmp::new("assistant-backpressure");
    let child=fake_child("i=0; while [ $i -lt 100 ]; do printf '%s\\n' '{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"More\"}}'; i=$((i+1)); done; sleep 5",&temp.0);
    let (cancel, rx) = watch::channel(false);
    let (tx, mut output) = mpsc::channel(1);
    let task = tokio::spawn(async move {
        run_child(child, "codex", rx, tx).await;
    });
    assert_eq!(output.recv().await.unwrap()["text"], "More");
    tokio::time::sleep(Duration::from_millis(20)).await;
    cancel.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn cancel_endpoint_returns_the_agreed_json_and_releases_the_active_turn() {
    let state = Arc::new(crate::start::testkit::state(vec![]));
    let (tx, rx) = watch::channel(false);
    *state.assistant.active.lock().unwrap() = Some(("turn-1".into(), tx));
    let (status, Json(body)) = cancel(State(state.clone()), Path("turn-1".into()))
        .await
        .unwrap();
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(body, json!({"turnId":"turn-1","cancelled":true}));
    assert!(*rx.borrow());
    drop(TurnGuard {
        state: state.clone(),
        id: "turn-1".into(),
    });
    assert!(state.assistant.active.lock().unwrap().is_none());
}

#[test]
fn scene_tools_cannot_implicitly_connect_to_remote_or_credential_bearing_urls() {
    for allowed in [
        "http://localhost:5196/mcp",
        "http://127.0.0.1:5196/mcp",
        "http://[::1]:5196/mcp",
    ] {
        assert!(local_mcp_url(allowed).is_some());
    }
    for denied in [
        "https://remote.example/mcp",
        "http://localhost.evil/mcp",
        "http://user:secret@localhost:5196/mcp",
        "http://localhost:5196/mcp?token=secret",
        "file:///tmp/mcp",
    ] {
        assert!(local_mcp_url(denied).is_none(), "{denied}");
    }
}
