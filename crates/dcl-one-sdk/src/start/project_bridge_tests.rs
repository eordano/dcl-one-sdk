use super::*;
use crate::start::testkit::{scene, Tmp};

fn headers(host: &str, origin: Option<&str>) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(header::HOST, host.parse().unwrap());
    if let Some(origin) = origin {
        headers.insert(header::ORIGIN, origin.parse().unwrap());
    }
    headers
}

#[test]
fn authoring_requires_local_peer_and_explicit_cross_origin_trust() {
    let local = "127.0.0.1:8000".parse().unwrap();
    let remote = "192.0.2.1:8000".parse().unwrap();
    let trusted = vec!["https://catalyst.example.com".into()];
    assert!(allowed(&headers("localhost:8000", None), local, &[]));
    assert!(allowed(
        &headers("localhost:8000", Some("http://localhost:8000")),
        local,
        &[]
    ));
    assert!(!allowed(
        &headers("localhost:8000", Some("null")),
        local,
        &trusted
    ));
    assert!(!allowed(
        &headers("localhost:8000", Some("https://catalyst.example.com")),
        local,
        &[]
    ));
    assert!(allowed(
        &headers("localhost:8000", Some("https://catalyst.example.com")),
        local,
        &trusted
    ));
    assert!(!allowed(
        &headers("localhost:8000", Some("https://catalyst.example.com")),
        remote,
        &trusted
    ));
    assert!(!allowed(
        &headers("evil.test:8000", Some("http://evil.test:8000")),
        local,
        &[]
    ));
    assert!(!allowed(
        &headers(
            "localhost:8000",
            Some("https://catalyst.example.com.evil.test")
        ),
        local,
        &trusted
    ));
    let mut h = headers("localhost:8000", Some("https://catalyst.example.com"));
    h.insert(crate::tunnel::FORWARDED_HEADER, "1".parse().unwrap());
    assert!(!allowed(&h, local, &trusted));
    h.remove(crate::tunnel::FORWARDED_HEADER);
    h.insert("x-forwarded-for", "203.0.113.2".parse().unwrap());
    assert!(!allowed(&h, local, &trusted));
}

#[test]
fn only_project_source_and_composites_are_exposed() {
    let t = Tmp::new("creator-paths");
    std::fs::create_dir_all(t.0.join("src")).unwrap();
    for bad in [
        "../secret.ts",
        "src/../../secret.ts",
        "/etc/passwd",
        "src/.env",
        ".dcl-one/storage.json",
        "node_modules/p/index.ts",
        "bin/index.js",
        "src\\index.ts",
        "package-lock.json",
    ] {
        assert!(safe_path(&t.0, bad).is_err(), "{bad}");
    }
    assert!(safe_path(&t.0, "main.composite").is_ok());
    assert!(safe_path(&t.0, "src/index.ts").is_ok());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&t.0, t.0.join("src/escape")).unwrap();
        std::os::unix::fs::symlink("/etc/passwd", t.0.join("src/secret.ts")).unwrap();
        assert!(safe_path(&t.0, "src/escape/secret.ts").is_err());
        assert!(safe_path(&t.0, "src/secret.ts").is_err());
    }
}

#[test]
fn saves_preserve_external_edits_and_create_composites() {
    let t = Tmp::new("creator-save");
    std::fs::create_dir_all(t.0.join("src")).unwrap();
    std::fs::write(t.0.join("src/index.ts"), "before").unwrap();
    let original = read_file(&t.0, "src/index.ts").unwrap();
    std::fs::write(t.0.join("src/index.ts"), "external edit").unwrap();
    let err = write_file(
        &t.0,
        "src/index.ts",
        FileWrite {
            content: "browser edit".into(),
            revision: original["revision"].as_str().map(String::from),
        },
    )
    .unwrap_err();
    assert_eq!(err.0, StatusCode::CONFLICT);
    assert_eq!(
        std::fs::read_to_string(t.0.join("src/index.ts")).unwrap(),
        "external edit"
    );
    let now = read_file(&t.0, "src/index.ts").unwrap();
    let saved = write_file(
        &t.0,
        "src/index.ts",
        FileWrite {
            content: "browser edit".into(),
            revision: now["revision"].as_str().map(String::from),
        },
    )
    .unwrap();
    assert_eq!(saved["revision"], revision(b"browser edit"));
    write_file(
        &t.0,
        "assets/custom-items/chair.composite",
        FileWrite {
            content: "{}".into(),
            revision: None,
        },
    )
    .unwrap();
    assert!(t.0.join("assets/custom-items/chair.composite").is_file());
    let composite = r#"{"version":1,"components":[],"entities":[]}"#;
    write_file(
        &t.0,
        "main.composite",
        FileWrite {
            content: composite.into(),
            revision: None,
        },
    )
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(t.0.join("main.composite")).unwrap(),
        composite
    );
    assert_eq!(
        write_file(
            &t.0,
            "main.composite",
            FileWrite {
                content: composite.into(),
                revision: None
            }
        )
        .unwrap_err()
        .0,
        StatusCode::CONFLICT
    );
    assert!(write_file(
        &t.0,
        "scene.json",
        FileWrite {
            content: "{".into(),
            revision: None
        }
    )
    .is_err());
}

#[tokio::test]
async fn manifest_uses_existing_sdk_services_and_preserves_proxy_prefix() {
    let t = Tmp::new("creator-manifest");
    let p = scene(&t.0, "demo", &["0,0"], "export {}");
    std::fs::write(p.root.join("main.composite"), "{}").unwrap();
    let mut st = crate::start::testkit::state(vec![p]);
    st.project_watch = true;
    let mut h = headers("localhost:8000", None);
    h.insert("x-forwarded-prefix", "/local-scene".parse().unwrap());
    let Json(info) = info(State(Arc::new(st)), h).await.unwrap();
    assert_eq!(info["compositePath"], "main.composite");
    assert_eq!(info["capabilities"]["watch"], true);
    assert_eq!(info["links"]["files"], "/local-scene/api/project/files");
    assert_eq!(info["links"]["reload"], "ws://localhost:8000/local-scene/");
    assert_eq!(info["links"]["publish"], "/local-scene/deploy");
    assert!(info["links"]["dataLayer"].is_null());
}

#[tokio::test]
async fn lists_real_files_and_refuses_ambiguous_workspace() {
    let t = Tmp::new("creator-list");
    let p = scene(&t.0, "demo", &["0,0"], "compiled");
    std::fs::create_dir_all(p.root.join("src")).unwrap();
    std::fs::write(p.root.join("src/index.ts"), "source").unwrap();
    std::fs::write(p.root.join(".env"), "secret").unwrap();
    let st = Arc::new(crate::start::testkit::state(vec![p.clone()]));
    let Json(list) = files(State(st)).await.unwrap();
    assert_eq!(
        list["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["path"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["scene.json", "src/index.ts"]
    );
    let st = crate::start::testkit::state(vec![p.clone(), p]);
    assert_eq!(single_project(&st).unwrap_err().0, StatusCode::CONFLICT);
}

#[tokio::test]
async fn http_preflight_and_revision_save_work_through_the_real_router() {
    let t = Tmp::new("creator-http");
    let p = scene(&t.0, "demo", &["0,0"], "compiled");
    std::fs::create_dir_all(p.root.join("src")).unwrap();
    std::fs::write(p.root.join("src/index.ts"), "export const value = 1").unwrap();
    let st = Arc::new(crate::start::testkit::state(vec![p.clone()]));
    let app = crate::start::build_router(st, Arc::new(crate::comms::CommsState::default()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let client = reqwest::Client::new();
    let endpoint = format!("{origin}/api/project/file?path=src%2Findex.ts");
    let preflight = client
        .request(reqwest::Method::OPTIONS, &endpoint)
        .header("Origin", &origin)
        .header("Access-Control-Request-Method", "PUT")
        .header("Access-Control-Request-Headers", "content-type")
        .send()
        .await
        .unwrap();
    assert_eq!(preflight.status(), StatusCode::NO_CONTENT);
    assert_eq!(preflight.headers()["access-control-allow-origin"], origin);
    let read: Value = client
        .get(&endpoint)
        .header("Origin", &origin)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let mut watcher = crate::watch::FsWatcher::new(&p.root).unwrap();
    let save = client
        .put(&endpoint)
        .header("Origin", &origin)
        .json(&json!({"revision":read["revision"],"content":"export const value = 2"}))
        .send()
        .await
        .unwrap();
    assert_eq!(save.status(), StatusCode::OK);
    let batch = tokio::time::timeout(std::time::Duration::from_secs(5), watcher.next_batch())
        .await
        .unwrap()
        .unwrap();
    assert!(
        batch.contains(&p.root.join("src/index.ts")),
        "atomic browser save must reach the SDK watcher: {batch:?}"
    );
    assert_eq!(
        std::fs::read_to_string(p.root.join("src/index.ts")).unwrap(),
        "export const value = 2"
    );
    let stale = client
        .put(&endpoint)
        .header("Origin", &origin)
        .json(&json!({"revision":read["revision"],"content":"stale"}))
        .send()
        .await
        .unwrap();
    assert_eq!(stale.status(), StatusCode::CONFLICT);
    let denied_delete = client.delete(&endpoint).send().await.unwrap();
    assert_eq!(denied_delete.status(), StatusCode::PRECONDITION_REQUIRED);
    let stale_delete = client
        .delete(&endpoint)
        .header("If-Match", read["revision"].as_str().unwrap())
        .send()
        .await
        .unwrap();
    assert_eq!(stale_delete.status(), StatusCode::CONFLICT);
    let deleted = client
        .delete(&endpoint)
        .header("If-Match", revision(b"export const value = 2"))
        .send()
        .await
        .unwrap();
    assert_eq!(deleted.status(), StatusCode::OK);
    assert!(!p.root.join("src/index.ts").exists());
    let protected = client
        .delete(format!("{origin}/api/project/file?path=scene.json"))
        .header("If-Match", "0".repeat(64))
        .send()
        .await
        .unwrap();
    assert_eq!(protected.status(), StatusCode::BAD_REQUEST);
    let composite = client
        .put(format!(
            "{origin}/api/project/file?path=assets/custom-items/chair.composite"
        ))
        .header("Origin", &origin)
        .json(&json!({"revision":null,"content":"{}"}))
        .send()
        .await
        .unwrap();
    assert_eq!(composite.status(), StatusCode::OK);
    let batch = tokio::time::timeout(std::time::Duration::from_secs(5), watcher.next_batch())
        .await
        .unwrap()
        .unwrap();
    assert!(
        batch.contains(&p.root.join("assets/custom-items/chair.composite")),
        "new composite directories must trigger the SDK watcher: {batch:?}"
    );
    let denied = client
        .get(&endpoint)
        .header("Origin", "https://untrusted.invalid")
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    assert!(!denied.headers().contains_key("access-control-allow-origin"));
    server.abort();
}
