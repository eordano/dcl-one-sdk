use super::*;
use crate::start::{
    external_debug_state::DebugState,
    testkit::{scene, Tmp},
};
use tokio_tungstenite::tungstenite::Message as Wire;

#[test]
fn debug_launch_links_preserve_native_and_mobile_syntax_without_duplicate_flags() {
    let native = crate::joinblock::desktop_deep_link(
        "http://127.0.0.1:8000",
        (4, -8),
        None,
        "&multi-instance=false",
    );
    let native = add_param(
        &native,
        "scene-inspector",
        "ws://127.0.0.1:8000/scene-inspector?token=test",
    );
    let native = add_param(&native, "multi-instance", "true");
    assert!(native.starts_with("decentraland://realm="));
    assert!(!native.contains('?'));
    assert_eq!(native.matches("multi-instance=").count(), 1);
    let params: std::collections::HashMap<_, _> =
        url::form_urlencoded::parse(native.strip_prefix("decentraland://").unwrap().as_bytes())
            .collect();
    assert_eq!(
        params["scene-inspector"],
        "ws://127.0.0.1:8000/scene-inspector?token=test"
    );
    let mobile = add_param(
        &crate::joinblock::mobile_deep_link("http://192.0.2.4:8000", (0, 0)),
        "scene-inspector",
        "ws://192.0.2.4:8000/scene-inspector?token=test",
    );
    assert!(mobile.starts_with("decentraland://open?preview="));
    assert_eq!(
        url::Url::parse(&mobile)
            .unwrap()
            .query_pairs()
            .find(|(k, _)| k == "preview")
            .unwrap()
            .1,
        "http://192.0.2.4:8000"
    );
}

#[test]
fn telemetry_retains_identity_and_cannot_ack_another_clients_command() {
    let state = DebugState::default();
    let (tx, _) = mpsc::channel(1);
    let id = state.connect(tx).unwrap();
    state.receive(id,json!({"type":"SCENE_INSPECTOR","payload":{"sessionId":"phone","entries":[{"type":"session_start","device_name":"Phone"},{"type":"log","message":"Hello"}]}}));
    assert_eq!(state.sessions()[0]["deviceName"], "Phone");
    assert_eq!(state.sessions()[0]["messageCount"], 2);
    let (tx, _) = mpsc::channel(1);
    let other = state.connect(tx).unwrap();
    let (reply, mut rx) = oneshot::channel();
    state
        .store
        .lock()
        .unwrap()
        .sessions
        .get_mut(&id)
        .unwrap()
        .pending
        .insert("cmd-one".into(), reply);
    state.receive(
        other,
        json!({"type":"SCENE_INSPECTOR_CMD_ACK","id":"cmd-one","ok":true}),
    );
    assert!(rx.try_recv().is_err());
    state.receive(
        id,
        json!({"type":"SCENE_INSPECTOR_CMD_ACK","id":"cmd-one","ok":true,"data":{"paused":true}}),
    );
    assert_eq!(rx.try_recv().unwrap()["data"]["paused"], true);
    state.disconnect(id);
    assert_eq!(state.sessions()[0]["status"], "ended");
    assert!(!token_matches("wrong", &state.token));
    assert!(token_matches(&state.token, &state.token));
}

#[tokio::test]
async fn real_debug_wire_receives_telemetry_and_returns_acknowledged_command_results() {
    let t = Tmp::new("external-debug");
    let project = scene(&t.0, "demo", &["0,0"], "compiled");
    let state = Arc::new(crate::start::testkit::state(vec![project]));
    let app =
        crate::start::build_router(state.clone(), Arc::new(crate::comms::CommsState::default()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let client = reqwest::Client::new();
    let descriptor: Value = client
        .get(format!("{base}/api/project/debug"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(descriptor["nativeUrl"]
        .as_str()
        .unwrap()
        .contains("scene-inspector="));
    assert!(descriptor["eventsUrl"]
        .as_str()
        .unwrap()
        .ends_with("/events"));
    assert!(
        tokio_tungstenite::connect_async(format!("ws://{addr}/scene-inspector?token=wrong"))
            .await
            .is_err()
    );
    let mut updates = state.external_debug.events.subscribe();
    let (mut device, _) = tokio_tungstenite::connect_async(format!(
        "ws://{addr}/scene-inspector?token={}",
        state.external_debug.token
    ))
    .await
    .unwrap();
    device.send(Wire::Text(json!({"type":"SCENE_INSPECTOR","payload":{"sessionId":"fixture","entries":[{"type":"session_start","device_name":"Fake phone"},{"type":"log","message":"Ready"}]}}).to_string().into())).await.unwrap();
    let id = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let event = updates.recv().await.unwrap();
            if event["type"] == "entries" {
                break event["sessionId"].as_u64().unwrap();
            }
        }
    })
    .await
    .unwrap();
    let request = client
        .post(format!("{base}/api/project/debug/command"))
        .json(&json!({"sessionId":id,"cmd":"pause"}));
    let response = tokio::spawn(async move { request.send().await.unwrap() });
    let command: Value =
        serde_json::from_str(&device.next().await.unwrap().unwrap().into_text().unwrap()).unwrap();
    assert_eq!(command["type"], "SCENE_INSPECTOR_CMD");
    assert_eq!(command["cmd"], "pause");
    device.send(Wire::Text(json!({"type":"SCENE_INSPECTOR_CMD_ACK","id":command["id"],"ok":true,"data":{"paused":true}}).to_string().into())).await.unwrap();
    let response: Value = response.await.unwrap().json().await.unwrap();
    assert_eq!(response["ok"], true);
    assert_eq!(response["data"]["paused"], true);
    let denied = client
        .post(format!("{base}/api/project/debug/command"))
        .header("Origin", "https://untrusted.invalid")
        .json(&json!({"sessionId":id,"cmd":"pause"}))
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    let invalid = client
        .post(format!("{base}/api/project/debug/command"))
        .json(&json!({"sessionId":id,"cmd":"arbitrary-shell"}))
        .send()
        .await
        .unwrap();
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    device.close(None).await.unwrap();
    server.abort();
}

#[tokio::test]
async fn abandoned_command_does_not_leave_a_pending_slot() {
    let state = Arc::new(crate::start::testkit::state(vec![]));
    let (send, mut commands) = mpsc::channel(1);
    let id = state.external_debug.connect(send).unwrap();
    let task_state = state.clone();
    let task = tokio::spawn(async move {
        command(
            State(task_state),
            Json(Command {
                session_id: id,
                cmd: "pause".into(),
                args: json!({}),
            }),
        )
        .await
    });
    let sent = commands.recv().await.unwrap();
    assert_eq!(sent["cmd"], "pause");
    task.abort();
    let _ = task.await;
    assert!(state.external_debug.store.lock().unwrap().sessions[&id]
        .pending
        .is_empty());
}
