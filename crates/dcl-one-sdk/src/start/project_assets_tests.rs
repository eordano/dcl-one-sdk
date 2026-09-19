use super::*;
use crate::start::testkit::{scene, Tmp};

#[tokio::test]
async fn binary_assets_round_trip_with_revision_safe_cleanup_and_watch_events() {
    let t = Tmp::new("asset-api");
    let p = scene(&t.0, "demo", &["0,0"], "compiled");
    let st = Arc::new(crate::start::testkit::state(vec![p.clone()]));
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
    let endpoint = format!("{origin}/api/project/asset?path=assets/model.glb");
    let bytes = vec![0x67, 0x6c, 0x54, 0x46, 0, 255, 128, 0, 42];
    let mut watcher = crate::watch::FsWatcher::new(&p.root).unwrap();
    let save = client
        .put(&endpoint)
        .header("Origin", &origin)
        .header("If-None-Match", "*")
        .body(bytes.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(save.status(), StatusCode::OK);
    let saved: Value = save.json().await.unwrap();
    let batch = tokio::time::timeout(std::time::Duration::from_secs(5), watcher.next_batch())
        .await
        .unwrap()
        .unwrap();
    assert!(batch.contains(&p.root.join("assets/model.glb")));
    let head = client
        .head(&endpoint)
        .header("Origin", &origin)
        .send()
        .await
        .unwrap();
    assert_eq!(head.status(), StatusCode::OK);
    assert_eq!(
        head.headers()["etag"],
        format!("\"{}\"", saved["revision"].as_str().unwrap())
    );
    assert!(head.headers()["access-control-expose-headers"]
        .to_str()
        .unwrap()
        .contains("etag"));
    assert!(head.bytes().await.unwrap().is_empty());
    let read = client
        .get(&endpoint)
        .header("Origin", &origin)
        .send()
        .await
        .unwrap();
    let revision = read.headers()["etag"].clone();
    assert_eq!(read.bytes().await.unwrap().as_ref(), bytes.as_slice());
    let list: Value = client
        .get(format!("{origin}/api/project/assets"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(list["files"][0]["path"], "assets/model.glb");
    std::fs::write(p.root.join("assets/model.glb"), b"external change").unwrap();
    let stale = client
        .delete(&endpoint)
        .header("If-Match", revision)
        .send()
        .await
        .unwrap();
    assert_eq!(stale.status(), StatusCode::CONFLICT);
    let fresh = client.head(&endpoint).send().await.unwrap();
    let deleted = client
        .delete(&endpoint)
        .header("If-Match", fresh.headers()["etag"].clone())
        .send()
        .await
        .unwrap();
    assert_eq!(deleted.status(), StatusCode::OK);
    assert!(!p.root.join("assets/model.glb").exists());
    assert_eq!(
        client
            .put(&endpoint)
            .body(bytes)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::PRECONDITION_REQUIRED
    );
    let denied = client
        .delete(&endpoint)
        .header("Origin", "https://untrusted.invalid")
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    server.abort();
}

#[test]
fn asset_paths_cannot_write_code_or_follow_symlinks() {
    for path in [
        "src/index.ts",
        "scene.json",
        ".env.png",
        "../escape.glb",
        "node_modules/model.glb",
        "bin/image.png",
    ] {
        assert!(!asset(path), "{path}");
    }
    for path in [
        "models/chair.GLB",
        "textures/sky.ktx2",
        "assets/sound.ogg",
        "assets/buffer.bin",
    ] {
        assert!(asset(path), "{path}");
    }
    let t = Tmp::new("asset-symlink");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("/tmp", t.0.join("assets")).unwrap();
        assert!(files::safe_path_for(&t.0, "assets/escape.glb", asset).is_err());
    }
    let mut headers = HeaderMap::new();
    headers.insert(header::IF_MATCH, "*".parse().unwrap());
    assert!(expected(&headers, false).is_err());
}
