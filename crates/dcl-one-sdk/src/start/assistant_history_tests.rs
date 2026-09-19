use super::*;
use crate::start::testkit::{scene, Tmp};

#[test]
fn project_history_survives_reopen_with_resume_context_and_provider_isolation() {
    let a = Tmp::new("assistant-history-a");
    let b = Tmp::new("assistant-history-b");
    let selected = [EntityContext {
        id: "42".into(),
        name: "Oak 🌳".into(),
    }];
    {
        let mut history = History::open(&a.0).unwrap();
        history
            .begin("chat-a", "codex", "Grow this tree", &selected)
            .unwrap();
        for raw in [
            r#"{"type":"thread.started","thread_id":"provider-thread"}"#,
            r#"{"type":"item.completed","item":{"type":"agent_message","text":"Tree grown"}}"#,
        ] {
            for event in crate::start::assistant_provider::events("codex", raw) {
                history.record("chat-a", &event).unwrap();
            }
        }
        history
            .record(
                "chat-a",
                &json!({"type":"done","exitCode":0,"cancelled":false}),
            )
            .unwrap();
    }
    let reopened = History::open(&a.0).unwrap();
    let (id, resume) = reopened
        .resolve(Some("chat-a"), "codex", None)
        .unwrap()
        .unwrap();
    assert_eq!(id, "chat-a");
    let invoke = crate::start::assistant_provider::invocation(
        "codex",
        "Continue",
        resume.as_deref(),
        None,
        &a.0,
        None,
    )
    .unwrap();
    assert_eq!(&invoke.args[..3], ["exec", "resume", "provider-thread"]);
    let saved = reopened.get(&id).unwrap();
    assert_eq!(saved["events"][0]["selectedEntities"][0]["name"], "Oak 🌳");
    assert_eq!(saved["events"][1]["text"], "Tree grown");
    assert!(!saved.to_string().contains("provider-thread"));
    assert_eq!(
        reopened
            .resolve(Some("chat-a"), "claude", None)
            .unwrap_err()
            .0,
        StatusCode::CONFLICT
    );
    let other = History::open(&b.0).unwrap();
    assert_eq!(other.get("chat-a").unwrap_err().0, StatusCode::NOT_FOUND);
    assert_eq!(
        other
            .resolve(None, "codex", Some("provider-thread"))
            .unwrap_err()
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        other.list().unwrap()["conversations"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    let context = prompt("Grow it", &selected).unwrap();
    assert!(context.contains("verify these current entities"));
    assert!(context.contains("Oak 🌳"));
    assert!(prompt(
        "Hi",
        &[EntityContext {
            id: "../file".into(),
            name: "X".into()
        }]
    )
    .is_err());
}

#[test]
fn history_is_bounded_and_excluded_from_project_asset_and_deployment_walkers() {
    let t = Tmp::new("assistant-history-private");
    let mut history = History::open(&t.0).unwrap();
    for i in 0..23 {
        history
            .begin(&format!("chat-{i}"), "codex", "Hello", &[])
            .unwrap();
    }
    assert_eq!(
        history.list().unwrap()["conversations"]
            .as_array()
            .unwrap()
            .len(),
        20
    );
    assert!(history.get("chat-0").is_err());
    history
        .record(
            "chat-22",
            &json!({"type":"text","text":"x".repeat(MAX_BYTES)}),
        )
        .unwrap();
    assert_eq!(history.get("chat-22").unwrap()["truncated"], true);
    history
        .record(
            "chat-22",
            &json!({"type":"session","sessionId":"still-resumes"}),
        )
        .unwrap();
    assert_eq!(
        history
            .resolve(Some("chat-22"), "codex", None)
            .unwrap()
            .unwrap()
            .1
            .as_deref(),
        Some("still-resumes")
    );
    let mut files = vec![];
    project_bridge::list_matching(&t.0, &t.0, &mut files, |_| true).unwrap();
    assert!(
        files.is_empty(),
        "Even an all-files project/asset scanner must exclude private state"
    );
    std::fs::write(t.0.join(".dclignore"), "!.dcl-one/**\n").unwrap();
    assert!(crate::deploy::collect_publishable_files(&t.0)
        .unwrap()
        .is_empty());
    history.remove("chat-22").unwrap();
    assert!(history.get("chat-22").is_err());
    assert_eq!(
        history
            .conn
            .query_row(
                "SELECT count(*) FROM events WHERE conversation='chat-22'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn history_routes_use_existing_project_origin_guard_and_never_serve_database() {
    let t = Tmp::new("assistant-history-api");
    let p = scene(&t.0, "demo", &["0,0"], "compiled");
    History::open(&p.root)
        .unwrap()
        .begin("saved", "codex", "Saved privately", &[])
        .unwrap();
    let st = Arc::new(crate::start::testkit::state(vec![p]));
    let app = crate::start::build_router(st, Arc::new(crate::comms::CommsState::default()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let client = reqwest::Client::new();
    let endpoint = format!("{origin}/api/project/assistant/conversations");
    assert_eq!(
        client
            .get(&endpoint)
            .header("Origin", "https://untrusted.invalid")
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let saved: Value = client
        .get(format!("{endpoint}/saved"))
        .header("Origin", &origin)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(saved["events"][0]["text"], "Saved privately");
    for path in [
        "/.dcl-one/assistant.sqlite",
        "/api/project/file?path=.dcl-one/assistant.sqlite",
        "/api/project/asset?path=.dcl-one/assistant.sqlite",
    ] {
        assert!(
            !client
                .get(format!("{origin}{path}"))
                .send()
                .await
                .unwrap()
                .status()
                .is_success(),
            "{path}"
        );
    }
    server.abort();
}

#[tokio::test]
async fn streamed_events_are_durable_before_delivery_and_browser_disconnect_closes_provider_stream()
{
    let t = Tmp::new("assistant-stream-history");
    let mut history = History::open(&t.0).unwrap();
    history.begin("chat", "codex", "Continue", &[]).unwrap();
    let (provider, events) = tokio::sync::mpsc::channel(4);
    let (client, mut received) = tokio::sync::mpsc::channel(4);
    let worker = tokio::spawn(async move {
        record_stream(&mut history, "chat", events, &client).await;
    });
    provider
        .send(json!({"type":"session","sessionId":"saved-thread"}))
        .await
        .unwrap();
    provider
        .send(json!({"type":"text","text":"Durable reply"}))
        .await
        .unwrap();
    assert_eq!(received.recv().await.unwrap()["type"], "session");
    assert_eq!(received.recv().await.unwrap()["text"], "Durable reply");
    let reopened = History::open(&t.0).unwrap();
    assert_eq!(
        reopened.get("chat").unwrap()["events"][1]["text"],
        "Durable reply"
    );
    assert_eq!(
        reopened
            .resolve(Some("chat"), "codex", None)
            .unwrap()
            .unwrap()
            .1
            .as_deref(),
        Some("saved-thread")
    );
    drop(received);
    tokio::time::timeout(Duration::from_secs(1), worker)
        .await
        .unwrap()
        .unwrap();
    assert!(
        provider.is_closed(),
        "Closing the browser must let run_child observe a closed stream and kill the provider"
    );
}
