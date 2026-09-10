use super::*;
use crate::deploy::WORLDS_CONTENT_SERVER;
use crate::start::deploy_rights::{Holdings, ParcelRight, WorldRow};
use crate::start::deploy_status::*;
use axum::body::to_bytes;
use axum::http::{header, StatusCode};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::net::{Ipv4Addr, SocketAddrV4};

const ADDR: &str = "0x1234567890abcdef1234567890abcdef12345678";
const LAN: SocketAddr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 9), 51000));
const LOCAL: SocketAddr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 51000));

/// A real directory with real files: an earlier version of these tests
/// pointed at a path that did not exist, so nothing was listed and
/// `!contains("somebody")` passed without proving anything.
struct Tree(std::path::PathBuf);

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scene(tag: &str, mut scene_json: serde_json::Value) -> (Tree, Project) {
    let base = std::env::temp_dir().join(format!(
        "dcl-one-sdk-deploy-page-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("somebody-scenes").join("gather");
    if scene_json.get("main").is_none() {
        scene_json["main"] = json!("bin/index.js");
    }
    std::fs::create_dir_all(root.join("assets")).unwrap();
    std::fs::create_dir_all(root.join("bin")).unwrap();
    std::fs::write(root.join("bin/index.js"), "module.exports={}").unwrap();
    std::fs::write(root.join("scene.json"), scene_json.to_string()).unwrap();
    std::fs::write(root.join("assets/model.glb"), vec![7u8; 2048]).unwrap();
    std::fs::write(root.join("README.md"), "notes").unwrap();
    let project = Project {
        root,
        scene_json: scene_json.clone(),
    };
    (Tree(base), project)
}

fn gather(tag: &str) -> (Tree, Project) {
    scene(tag, json!({ "display": { "title": "Gather" } }))
}

fn state(project: Project) -> Arc<AppState> {
    Arc::new(crate::start::testkit::state(vec![project]))
}

async fn body_of(resp: Response) -> String {
    let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

async fn json_of(resp: Response) -> serde_json::Value {
    serde_json::from_slice(&to_bytes(resp.into_body(), usize::MAX).await.unwrap()).unwrap()
}

/// The preview cache is process-wide, so a test that wants a fresh walk
/// drops its own entry and leaves the rest alone.
fn forget(st: &Arc<AppState>) {
    let root = st.projects()[0].root.clone();
    cache(st).retain(|(p, _, _)| *p != root);
}

/// Everything a visitor of `/deploy` receives, fetched the way they fetch it:
/// this suite once rebuilt the page's strings for itself and stayed green
/// while `page` printed the absolute path of the scene.
async fn served(st: &Arc<AppState>) -> String {
    forget(st);
    let resp = route(State(st.clone()), ConnectInfo(LOCAL), HeaderMap::new()).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp
        .headers()
        .get(header::CONTENT_TYPE)
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("text/html"));
    body_of(resp).await
}

async fn served_target(st: &Arc<AppState>) -> String {
    body_of(target_page(st, &HeaderMap::new()).await).await
}

fn status(remote: Remote) -> LiveStatus {
    LiveStatus {
        remote,
        reuse: None,
    }
}

fn reuse(reused_files: usize, reused_bytes: u64, upload_files: usize, upload_bytes: u64) -> Reuse {
    Reuse {
        reused_files,
        reused_bytes,
        upload_files,
        upload_bytes,
    }
}

fn remote_scene(title: &str, coords: Vec<(i64, i64)>) -> RemoteScene {
    RemoteScene {
        title: title.into(),
        parcels: coords.len(),
        coords,
        size: None,
    }
}

fn current_scene(title: &str, timestamp: Option<i64>, coords: Vec<(i64, i64)>) -> CurrentScene {
    CurrentScene {
        title: title.into(),
        timestamp,
        parcels: coords.len(),
        coords,
        size: None,
    }
}

fn world_row(name: &str, scenes: Option<i64>, title: Option<&str>) -> WorldRow {
    WorldRow {
        name: name.to_string(),
        scenes,
        last_deployed: None,
        title: title.map(str::to_string),
        owned: true,
    }
}

fn owner_of(worlds: Vec<WorldRow>) -> Rights {
    Rights {
        verdict: Verdict::May("you own this name".into()),
        worlds,
        ..Rights::unchecked(ADDR, "")
    }
}

fn linker_deploy(target: &str, world: Option<&str>) -> crate::linker::LinkerDeploy {
    crate::linker::LinkerDeploy {
        dir: std::env::temp_dir(),
        prepared: deploy::Prepared {
            files: vec![],
            pointers: vec!["0,0".into()],
            metadata: crate::jsjson::parse("{}").unwrap(),
        },
        target_content: target.into(),
        world: world.map(str::to_string),
        needs_delete: false,
        timestamp_override: None,
        entity_out: None,
        scene_title: "Gather".into(),
        base_parcel: "0,0".into(),
        multi_scene: true,
        gate: crate::deploy::PermissionGate::off(),
    }
}

/// A live signer with no wallet answer yet.
fn parked_signer() -> Arc<crate::linker::LinkerState> {
    crate::linker::new_state(linker_deploy(
        "https://worlds-content-server.decentraland.org",
        Some("w.dcl.eth"),
    ))
    .0
}

/// The loopback gate, which the router tests cannot reach: their client is
/// always 127.0.0.1, so the only honest way to put a non-loopback peer in
/// front of the handler is to hand it one.
#[tokio::test]
async fn a_peer_off_this_machine_cannot_publish() {
    let (_tree, project) = gather("remote");
    let st = state(project);

    let refused = start(
        State(st.clone()),
        ConnectInfo(LAN),
        HeaderMap::new(),
        Form(DeployForm {
            token: token(&st).to_string(),
            fingerprint: String::new(),
        }),
    )
    .await;
    assert_eq!(
        refused.status(),
        StatusCode::FORBIDDEN,
        "a correct token from off-machine is still not allowed to publish"
    );

    let body = body_of(page(&st, &HeaderMap::new(), false).await).await;
    assert!(
        !body.contains(r#"id="publish""#),
        "no publish button for a remote reader"
    );
    assert!(
        body.contains("Publishing runs on the machine"),
        "{body:.400}"
    );
}

/// This page is served to anything that can reach the port, so the one
/// thing it must never answer is where the scene lives on disk.
#[tokio::test]
async fn the_deploy_page_never_names_the_scene_directory() {
    let (dir, project) = gather("names");
    let root = project.root.clone();
    let html = served(&state(project)).await;
    assert!(html.contains("dcl-one-sdk deploy"));
    assert!(html.contains("assets/model.glb"), "the payload is listed");
    assert!(!html.contains("--dir"), "{html}");
    assert!(!html.contains("somebody"), "{html}");
    assert!(!html.contains(&dir.0.display().to_string()));
    assert!(!html.contains(&root.display().to_string()));
    assert!(html.contains("in the scene folder"));
}

/// One landing-language card: the destination as the headline (the ONLY
/// place the card names the target), the publish button in its footer.
#[tokio::test]
async fn the_page_is_one_card_with_the_target_in_its_headline() {
    let _guard = crate::deploy::ENV_LOCK.lock().await;
    let (_dir, project) = gather("card");
    let html = served(&state(project)).await;
    assert!(
        !html.contains(r#"<h1 class="page__title">Deploy</h1>"#),
        "the standalone header folded into the card: {html}"
    );
    assert!(
        html.contains("<h2>Gather \u{2192} ") || html.contains("<h2>Gather → "),
        "the headline says what goes where: {html}"
    );
    assert!(
        !html.contains(r#"<span class="jn2__host">on "#),
        "the redundant host pill stays gone (the nav's server pill is a different span): {html}"
    );
    assert!(
        html.contains(r#"/target">review the target</a>"#),
        "the detail moved to /target and the hint points there: {html}"
    );
    assert!(
        !html.contains(r#"class="u-sr-only""#),
        "no heading on this page is hidden from the screen: {html}"
    );
    assert!(
        !html.contains(r#"<span class="knob__k">On the server now</span>"#),
        "the live/upload split lives on /target now: {html}"
    );
    assert!(
        html.contains(r#"<div class="kvs files">"#) && html.contains(r#"<details class="drawer""#),
        "the payload is a folded list of paths and sizes: {html}"
    );
    assert!(
        html.contains(r#"<button class="jn__cta" type="submit">Publish</button>"#),
        "the primary button is the card footer's: {html}"
    );
    assert!(
        html.matches(r#"id="publish" method="post""#).count() == 1
            && html.matches(r#"method="post""#).count() == 2,
        "the publish form and the bar's connect, nothing more: {html}"
    );
    assert!(
        html.contains(r#"name="token" value=""#) && html.contains(r#"name="fingerprint" value=""#),
        "the form still carries the token and the fingerprint: {html}"
    );
}

/// The page-local sheet adds layout only: a colour, case or tracking of its
/// own would make `/deploy` look like a different server than `/`.
#[test]
fn the_page_local_css_adds_layout_and_never_a_second_palette() {
    for banned in [
        "text-transform",
        "letter-spacing",
        "font-weight",
        "font-family",
        ": #",
        "rgb",
        "opacity",
    ] {
        assert!(
            !PAGE_CSS.contains(banned),
            "page-local css must not carry `{banned}`: {PAGE_CSS}"
        );
    }
    assert!(
        PAGE_CSS.contains("var(--ink-6)"),
        "every colour it does set comes from the shared tokens: {PAGE_CSS}"
    );
}

/// Env override first, then the world name onto the public worlds server,
/// then Genesis — the way the deploy itself resolves it.
#[test]
fn the_destination_resolves_like_the_deploy_will() {
    let world = json!({ "worldConfiguration": { "name": "gather.dcl.eth" } });
    let d = resolve_dest(&world, None, None);
    assert_eq!(d.headline, "World gather.dcl.eth");
    assert!(
        d.server_line.contains("worlds-content-server"),
        "{}",
        d.server_line
    );
    assert_eq!(d.server_line, "on worlds-content-server.decentraland.org");
    assert_eq!(d.read_bases, [WORLDS_CONTENT_SERVER]);
    assert_eq!(
        d.lambdas_base, "https://peer.decentraland.org/lambdas",
        "chain facts read from the public lambdas even for a world deploy"
    );
    assert_eq!(d.worlds_base, WORLDS_CONTENT_SERVER);

    let land =
        json!({ "scene": { "parcels": ["85,40", "86,40", "85,41", "86,41"], "base": "85,40" } });
    let d = resolve_dest(&land, None, None);
    assert_eq!(d.headline, "Parcels 85,40\u{2013}86,41");
    assert!(d.server_line.contains("Genesis City"), "{}", d.server_line);
    assert_eq!(d.server_line, "on a public Genesis City catalyst");
    assert_eq!(d.base_pointer, "85,40");
    assert_eq!(d.read_bases, [GENESIS_READ]);

    let one = json!({ "scene": { "parcels": ["3,-2"], "base": "3,-2" } });
    assert_eq!(resolve_dest(&one, None, None).headline, "Parcel 3,-2");

    let env = resolve_dest(&world, Some("my-server.example.com"), None);
    assert_eq!(env.headline, "World gather.dcl.eth");
    assert!(
        env.server_line.contains("my-server.example.com"),
        "{}",
        env.server_line
    );
    assert!(env.server_line.contains("DCL_ONE_SDK_TARGET_SERVER"));
    assert_eq!(
        env.read_bases,
        [
            "https://my-server.example.com",
            "https://my-server.example.com/content"
        ]
    );
    assert_eq!(
        env.lambdas_base, "https://my-server.example.com/lambdas",
        "the verdict asks the server that will rule on the publish"
    );
    assert_eq!(
        env.chain_lambdas, "https://peer.decentraland.org/lambdas",
        "chain facts always read from Genesis — a self-hosted squid often carries none"
    );
    assert_eq!(env.worlds_base, "https://my-server.example.com");

    let rot = resolve_dest(
        &land,
        None,
        Some(vec!["https://cat.example.com".to_string()]),
    );
    assert_eq!(
        rot.read_bases,
        ["https://cat.example.com/content", "https://cat.example.com"]
    );
    assert!(rot.server_line.contains("DCL_ONE_SDK_CATALYST_ROTATION"));
}

/// The worlds `/scenes` answer in the public server's shape: the scene on
/// our parcels is current, the rest are the neighbours `multi_scene: true`
/// preserves.
#[test]
fn a_world_answer_splits_into_current_and_preserved_scenes() {
    let body = json!({ "scenes": [
        { "parcels": ["0,0", "0,1"], "entity": {
            "timestamp": 1787664148945i64,
            "content": [ { "file": "a.glb", "hash": "bafkaaa" } ],
            "metadata": { "display": { "title": "Gathering Stage" } } } },
        { "parcels": ["5,5"], "size": "2048", "entity": {
            "timestamp": 1787000000000i64,
            "content": [ { "file": "b.glb", "hash": "bafkbbb" } ],
            "metadata": { "display": { "title": "Bazaar" } } } }
    ]});
    let pointers = vec!["0,0".to_string()];
    let Remote::Known(state) = world_remote(&body, &pointers) else {
        panic!("two scenes is a known state");
    };
    let current = state.current.expect("our parcels are occupied");
    assert_eq!(current.title, "Gathering Stage");
    assert_eq!(current.parcels, 2);
    assert_eq!(current.timestamp, Some(1787664148945));
    assert_eq!(
        current.coords,
        [(0, 0), (0, 1)],
        "coords ride along for the map"
    );
    assert_eq!(state.others.len(), 1);
    assert_eq!(state.others[0].title, "Bazaar");
    assert_eq!(
        state.others[0].size,
        Some(2048),
        "the stringified size the worlds server reports is a number here"
    );
    assert!(state.hashes.contains("bafkaaa") && state.hashes.contains("bafkbbb"));

    assert!(matches!(
        world_remote(&json!({ "scenes": [] }), &pointers),
        Remote::Empty
    ));
}

/// The entity on the base parcel headlines; any other entity under the
/// deploying pointers is named as replaced.
#[test]
fn a_genesis_answer_headlines_the_base_parcel_entity() {
    let entities = vec![
        json!({ "pointers": ["86,41"], "timestamp": 1000i64,
            "content": [ { "file": "x", "hash": "bafkxxx" } ],
            "metadata": { "display": { "title": "Neighbour" } } }),
        json!({ "pointers": ["85,40", "86,40"], "timestamp": 2000i64,
            "content": [ { "file": "y", "hash": "bafkyyy" } ],
            "metadata": { "display": { "title": "Ours" } } }),
    ];
    let Remote::Known(state) = genesis_remote(&entities, "85,40") else {
        panic!("two entities is a known state");
    };
    assert_eq!(state.current.as_ref().unwrap().title, "Ours");
    assert_eq!(state.current.as_ref().unwrap().parcels, 2);
    assert_eq!(state.others.len(), 1);
    assert_eq!(state.others[0].title, "Neighbour");
    assert!(matches!(genesis_remote(&[], "85,40"), Remote::Empty));
}

/// Each remote state renders one honest sentence or the kv rows, and the
/// preserved/replaced copy follows the destination: the POST's
/// `multi_scene: true` only holds on a worlds server.
#[test]
fn every_remote_state_renders_and_names_the_fate_of_neighbours() {
    let world = resolve_dest(
        &json!({ "worldConfiguration": { "name": "w.dcl.eth" }, "scene": { "parcels": ["0,0"], "base": "0,0" } }),
        None,
        None,
    );
    let land = resolve_dest(
        &json!({ "scene": { "parcels": ["0,0"], "base": "0,0" } }),
        None,
        None,
    );
    let known = status(Remote::Known(RemoteState {
        current: Some(current_scene(
            "Gathering Stage",
            Some(deploy::now_ms() - 3 * 86_400_000),
            vec![(0, 0), (0, 1), (1, 0), (1, 1)],
        )),
        others: vec![remote_scene("Bazaar", vec![(5, 5), (5, 6), (6, 5)])],
        hashes: HashSet::new(),
    }));
    let html = server_panel(&world, &known);
    assert!(html.contains("Gathering Stage"), "{html}");
    assert!(html.contains("3 days ago"), "{html}");
    assert!(
        html.contains(r#"<span class="k">Parcels</span><span>4</span>"#),
        "{html}"
    );
    assert!(html.contains("Bazaar"), "{html}");
    assert!(html.contains("kept in place by this publish"), "{html}");
    let html = server_panel(&land, &known);
    assert!(html.contains("replaced by this publish"), "{html}");

    let empty = status(Remote::Empty);
    assert!(
        server_panel(&world, &empty).contains("Nothing is deployed here yet"),
        "{}",
        server_panel(&world, &empty)
    );

    let down = status(Remote::Unreachable(
        "could not reach worlds-content-server.decentraland.org".into(),
    ));
    let html = server_panel(&world, &down);
    assert!(html.contains("Could not check what is live"), "{html}");
    assert!(html.contains("Publishing may still work"), "{html}");

    let off = LiveStatus::unknown("Live checks are off for this run");
    assert!(
        server_panel(&world, &off).contains("Live checks are off"),
        "{}",
        server_panel(&world, &off)
    );
}

/// A file the server holds transfers nothing, a file with no hash is an
/// upload, and the bytes follow the files.
#[test]
fn the_reuse_split_counts_files_and_bytes_by_server_hash() {
    let files = vec![
        ("a.glb".to_string(), Some(1000u64)),
        ("b.glb".to_string(), Some(300u64)),
        ("c.glb".to_string(), Some(50u64)),
        ("broken.glb".to_string(), None),
    ];
    let hashes: HashMap<String, String> =
        [("a.glb", "bafka"), ("b.glb", "bafkb"), ("c.glb", "bafkc")]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
    let on_server: HashSet<String> = ["bafka", "bafkc"].iter().map(|s| s.to_string()).collect();
    let r = split_reuse(&files, &hashes, &on_server);
    assert_eq!((r.reused_files, r.reused_bytes), (2, 1050));
    assert_eq!((r.upload_files, r.upload_bytes), (2, 300));

    let none = split_reuse(&files, &hashes, &HashSet::new());
    assert_eq!(none.reused_files, 0);
    assert_eq!(none.upload_files, 4);
}

/// The upload column says the split in one line, and only when there is a
/// split to say.
#[test]
fn the_upload_column_says_the_split_in_one_line() {
    let (_dir, project) = gather("upline");
    let p = deploy::preview(&project).unwrap();
    let with = LiveStatus {
        remote: Remote::Empty,
        reuse: Some(reuse(2, 2000, 1, 79)),
    };
    let html = upload_panel(&p, &with);
    assert!(
        html.contains("2 of 3 files are already on the server"),
        "{html}"
    );
    assert!(html.contains("1 to upload (79 bytes)"), "{html}");

    let fresh = LiveStatus {
        remote: Remote::Empty,
        reuse: Some(reuse(0, 0, 3, 2079)),
    };
    let html = upload_panel(&p, &fresh);
    assert!(html.contains("All 3 files upload"), "{html}");

    let unknown = LiveStatus::unknown("off");
    let html = upload_panel(&p, &unknown);
    assert!(!html.contains("to upload"), "no fake split: {html}");
    assert!(
        html.contains(r#"<span class="datum__num">3</span>"#),
        "{html}"
    );
}

/// The same fixture bytes the deploy golden test pins must hash to the same
/// ids here, or the reuse split would compare apples to invented oranges.
#[tokio::test]
async fn the_reuse_hashes_are_the_publish_time_cids() {
    let (_dir, project) = gather("cids");
    std::fs::write(
        project.root.join("bin/index.js"),
        "console.log(\"golden\");\n",
    )
    .unwrap();
    let p = deploy::preview(&project).unwrap();
    let print = fingerprint(&project.root, &p);
    let rels: Vec<String> = p.files.iter().map(|(rel, _)| rel.clone()).collect();
    let st = state(project.clone());
    let hashes = cached_hashes(&st.deploy.caches, project.root.clone(), print, rels).await;
    let map = hashes.as_ref().as_ref().expect("hashing succeeds");
    assert_eq!(
        map.get("bin/index.js").map(String::as_str),
        Some("bafkreiabpuwsr4w2yzatq6gygbtpx7coohgpsg7tve3msd55odi6b2r5om"),
        "same CID the deploy golden test pins for these bytes"
    );
    assert_eq!(map.len(), p.files.len(), "every readable file is hashed");
}

#[test]
fn ago_reads_like_a_person() {
    let now = 1_787_664_148_945i64;
    assert_eq!(ago(now - 30_000, now), "just now");
    assert_eq!(ago(now - 5 * 60_000, now), "5 minutes ago");
    assert_eq!(ago(now - 3 * 3_600_000, now), "3 hours ago");
    assert_eq!(ago(now - 3 * 86_400_000, now), "3 days ago");
    assert_eq!(
        ago(now + 60_000, now),
        "just now",
        "clock skew is not negative time"
    );
}

/// The `<noscript>` meta refresh rides the Running state only, the region's
/// `data-state`/`data-signing` are the shape the script compares before
/// swapping, and a live signer renders the wallet panel inline.
#[test]
fn the_run_region_marks_its_state_for_the_script_and_the_no_js_refresh() {
    const PANEL: &str =
        r#"<div class="panel" id="sign-panel" data-api="/t/abc/deploy/sign">…</div>"#;
    let mut run = Run {
        signing: Some("/deploy".into()),
        ..Run::new(
            1,
            "https://worlds-content-server.decentraland.org".into(),
            false,
            "abc123".into(),
        )
    };
    let html = run_region_for("/t/abc", Some(&run), None, Some(PANEL));
    assert!(
        html.contains(r#"<noscript><meta http-equiv="refresh" content="2"></noscript>"#),
        "{html}"
    );
    assert!(
        html.contains(r#"id="run-status" data-state="running" data-signing="/t/abc/deploy""#),
        "the signing path is on THIS origin, prefix included: {html}"
    );
    assert!(
        html.contains(r#"id="sign-panel" data-api="/t/abc/deploy/sign""#),
        "the wallet panel is inline, server-rendered: {html}"
    );

    run.signing = None;
    let html = run_region_for("/t/abc", Some(&run), None, None);
    assert!(!html.contains("sign-panel"), "{html}");
    assert!(
        html.contains("wallet hand-off appears here"),
        "building still narrates what comes next: {html}"
    );
    run.signing = Some("/deploy".into());

    run.state = RunState::Done("Deployed bafy (HTTP 200)".into());
    let html = run_region_for(
        "",
        Some(&run),
        Some("https://decentraland.org/play/?realm=w.dcl.eth"),
        None,
    );
    assert!(!html.contains("http-equiv"), "{html}");
    assert!(!html.contains("data-signing"), "{html}");
    assert!(html.contains(r#"data-state="done""#), "{html}");
    assert!(html.contains("Published"), "{html}");
    assert!(html.contains(">Jump in</a>"), "{html}");

    run.state = RunState::Failed("the content server rejected it".into());
    let html = run_region_for("", Some(&run), None, None);
    assert!(!html.contains("http-equiv"), "{html}");
    assert!(html.contains(r#"data-state="failed""#), "{html}");
    assert!(html.contains("Deploy failed"), "{html}");
    assert!(html.contains("panel--warn"), "{html}");

    run.state = RunState::Stale(vec!["assets/model.glb".into()]);
    let html = run_region_for("", Some(&run), None, None);
    assert!(html.contains(r#"data-state="stale""#), "{html}");
    assert!(html.contains("Nothing was published"), "{html}");
    assert!(html.contains("assets/model.glb"), "{html}");
    assert_eq!(
        run_region_for("", None, None, None),
        r#"<div id="run-status" data-state="idle"></div>"#,
        "no run is still a region, so the script always has its element"
    );
}

/// Exactly its own script and nothing else executable — no src, no
/// `javascript:` url, no inline handler, no alert — and the no-JS path keeps
/// its POSTing form with the token and the fingerprint.
#[tokio::test]
async fn the_page_ships_exactly_its_own_script_and_no_handlers() {
    let (_dir, project) = gather("script");
    let html = served(&state(project)).await;
    let lower = html.to_lowercase();
    assert_eq!(lower.matches("<script").count(), 1, "one inline script");
    assert!(
        !lower.contains("<script src"),
        "nothing loads from anywhere"
    );
    assert!(!lower.contains("javascript:"), "no javascript: url");
    assert!(!SCRIPT.contains("alert("), "no alert");
    assert!(
        SCRIPT.contains("querySelectorAll('noscript')"),
        "morphs must strip noscript fallbacks: DOMParser parses with scripting \
         off, so a swapped-in meta refresh would reload the live page"
    );
    for (at, _) in lower.match_indices(" on") {
        let rest = &lower[at + 3..];
        let name_len = rest
            .find(|c: char| !c.is_ascii_alphabetic())
            .unwrap_or(rest.len());
        assert!(
            !(name_len > 0 && rest[name_len..].starts_with('=')),
            "inline event handler: on{}",
            &rest[..name_len]
        );
    }
    assert!(
        html.contains(r#"id="run-status" data-state="#),
        "the script's status region is on the page: {html}"
    );
    assert!(
        html.contains(r#"id="publish" method="post""#),
        "the no-JS form is what the script posts: {html}"
    );
}

/// The destination is said ONCE, in the card's header, and a world deploy's
/// terminal line is the bare command. `None` is passed for the default
/// target on purpose: reading the environment would make this test say
/// something different on a machine that exports `DCL_ONE_SDK_TARGET_SERVER`.
#[tokio::test]
async fn the_card_names_the_destination_once_and_the_bare_command() {
    let (_dir, project) = scene(
        "world",
        json!({ "worldConfiguration": { "name": "my.dcl.eth" } }),
    );
    let dest = resolve_dest(&project.scene_json, None, None);
    let p = deploy::preview(&project).unwrap();
    let status = LiveStatus::unknown("Live checks are off for this run");
    let html = card(
        "", "tok", "Gather", &dest, &p, &status, "fp", None, None, false, "",
    );
    assert!(html.contains("dcl-one-sdk deploy</code>"), "{html}");
    assert!(html.contains("World my.dcl.eth"), "{html}");
    assert!(
        !html.contains(r#"name="target_content""#),
        "the server override field is gone — the header names the server: {html}"
    );
    assert_eq!(
        html.matches("worlds-content-server.decentraland.org")
            .count(),
        0,
        "the server host left the card with the pill — /target owns it: {html}"
    );
    assert!(
        html.contains("<h2>Gather \u{2192} World my.dcl.eth</h2>")
            || html.contains("<h2>Gather → World my.dcl.eth</h2>"),
        "scene and destination share the headline: {html}"
    );
}

/// The publishing docs have two destinations. World details and history
/// must never masquerade as additional destination types.
#[tokio::test]
async fn the_target_page_separates_destinations_from_world_details_and_history() {
    let _guard = crate::deploy::ENV_LOCK.lock().await;
    let (_dir, project) = scene(
        "targetpg",
        json!({ "display": { "title": "Gather" }, "worldConfiguration": { "name": "my.dcl.eth" } }),
    );
    let html = served_target(&state(project)).await;
    assert!(
        !html.contains("<h2>Deploy target</h2>"),
        "the card leads with its tabs, not a header restating the nav: {html}"
    );
    assert!(
        !html.contains(r#"<div class="jn2__head">"#),
        "the target card starts at its tab strip, headless: {html}"
    );
    assert!(
        html.contains(r#"value="world" checked"#),
        "a world scene lands on the World tab: {html}"
    );
    assert_eq!(html.matches("name=\"tgt\"").count(), 2);
    assert!(
        !html.contains("name=\"tgt\" value=\"multi\"")
            && !html.contains("name=\"tgt\" value=\"history\"")
    );
    assert!(html.contains(
        "<details class=\"tgt__advanced\"><summary>Multi-Scene World (Advanced)</summary>"
    ));
    assert!(html.contains("<details class=\"tgt__history\"><summary>Deployment history</summary>"));
    for pane in ["tgt__pane--world", "tgt__pane--land"] {
        assert!(html.contains(pane), "missing {pane}: {html}");
    }
    assert!(
        html.contains(r#"<span class="knob__k">On the server now</span>"#)
            && html.contains(r#"<span class="knob__k">Upload</span>"#),
        "the live/upload split lives here now: {html}"
    );
    assert!(
        html.matches("<script>").count() == 1 && !html.contains("<script src"),
        "one inline script (wallet detection and connect-follow), nothing loaded: {html}"
    );
    assert!(
        html.contains("Select LAND"),
        "a world scene can step back to its parcels in one click: {html}"
    );
}

/// One signing endpoint, hosted by this server and gated like the POST that
/// starts a run; its panel is server-rendered inline once a run hands the
/// signer over.
#[tokio::test]
async fn the_signing_endpoint_is_hosted_gated_and_pending_aware() {
    let _guard = crate::deploy::ENV_LOCK.lock().await;
    let (_dir, project) = gather("signhost");
    let st = state(project);

    let sig_req = || -> Result<
        axum::Json<crate::linker::SignReq>,
        axum::extract::rejection::JsonRejection,
    > {
        Ok(axum::Json(
            serde_json::from_value(json!({
                "address": "0x0", "signature": "0x0", "entityId": "bogus"
            }))
            .unwrap(),
        ))
    };

    let blocked = sign_submit(
        State(st.clone()),
        ConnectInfo(LAN),
        HeaderMap::new(),
        sig_req(),
    )
    .await;
    assert_eq!(
        blocked.status(),
        StatusCode::FORBIDDEN,
        "the wallet signs on the hosting machine"
    );

    *signer_slot(&st) = None;
    assert!(
        pending_sign_panel(&st, "").is_none(),
        "no signer, no panel to draw"
    );
    let idle = sign_submit(
        State(st.clone()),
        ConnectInfo(LOCAL),
        HeaderMap::new(),
        sig_req(),
    )
    .await;
    assert_eq!(
        idle.status(),
        StatusCode::NOT_FOUND,
        "an idle signer refuses instead of pretending"
    );

    adopt_cli_signing(&st, parked_signer());
    let panel = pending_sign_panel(&st, "/t/abc").expect("a live signer renders the panel");
    assert!(
        panel.contains(r#"data-api="/t/abc/deploy/sign""#),
        "the endpoint path carries the prefix: {panel}"
    );
    assert!(
        panel.contains("world w.dcl.eth (multi-scene, additive)"),
        "the facts are server-rendered, not fetched: {panel}"
    );
    assert!(panel.contains("data-entity-id="), "{panel}");

    let html = served(&st).await;
    assert!(
        html.contains(r#"id="sign-panel""#),
        "/deploy draws the live panel inline: {html}"
    );

    forget(&st);
    let remote = body_of(route(State(st.clone()), ConnectInfo(LAN), HeaderMap::new()).await).await;
    assert!(
        !remote.contains(r#"id="sign-panel""#),
        "a peer the signing gate refuses gets no payload facts: {remote}"
    );
    assert!(
        remote.contains(r#"data-state="running""#),
        "the run's liveness still shows: {remote}"
    );
    *signer_slot(&st) = None;
    *runs(&st) = None;
}

/// The count that matters is what `.dclignore` removed from a directory
/// that IS published, not the files under a node_modules the walk never enters.
#[tokio::test]
async fn ignored_names_the_files_you_excluded_not_the_tree_you_never_ship() {
    let (_dir, project) = gather("ignored");
    let modules = project.root.join("node_modules/pkg");
    std::fs::create_dir_all(&modules).unwrap();
    for i in 0..50 {
        std::fs::write(modules.join(format!("f{i}.js")), "x").unwrap();
    }
    let p = deploy::preview(&project).expect("preview");
    assert_eq!(p.ignored, ["README.md"], "node_modules is not enumerated");
    let html = served(&state(project)).await;
    assert!(
        !html.contains(".dclignore") && !html.contains("Left out"),
        "the drawer stopped narrating exclusions: {html}"
    );
    assert!(!html.contains("node_modules"));
}

/// Sizes come from the directory entry, so the totals have to be real
/// without the page ever reading the bytes.
#[test]
fn the_payload_totals_the_files_it_would_upload() {
    let (_dir, project) = gather("totals");
    let p = deploy::preview(&project).expect("preview");
    let listed: u64 = p.files.iter().filter_map(|(_, len)| *len).sum();
    assert_eq!(listed, p.total_bytes);
    assert!(p
        .files
        .iter()
        .any(|(rel, len)| rel == "assets/model.glb" && *len == Some(2048)));
    assert!(p.oversize.is_empty());
    assert!(p.unreadable.is_empty());
    assert_eq!(
        p.files.first().map(|(rel, _)| rel.as_str()),
        Some("assets/model.glb"),
        "largest first"
    );
}

/// The truncated tail folds instead of vanishing: its summary carries the
/// byte sum of the folded files, and one click shows every row.
#[tokio::test]
async fn a_long_payload_lists_eight_and_folds_the_rest() {
    let (_dir, project) = gather("truncate");
    for i in 1..=12u64 {
        std::fs::write(
            project.root.join(format!("assets/a{i:02}.glb")),
            vec![0u8; (13 - i) as usize * 1000],
        )
        .unwrap();
    }
    let p = deploy::preview(&project).unwrap();
    let unlisted: u64 = p
        .files
        .iter()
        .skip(LISTED)
        .filter_map(|(_, len)| *len)
        .sum();
    assert_eq!(
        p.files.len(),
        15,
        "12 assets + model.glb + scene.json + bundle"
    );
    let html = served(&state(project)).await;
    assert_eq!(
        html.matches(r#"class="k k--file""#).count(),
        p.files.len(),
        "every file is a row — the tail is folded, not gone"
    );
    assert!(
        html.contains(&format!(
            "<summary>and 7 more \u{b7} {}</summary>",
            deploy::human_size(unlisted)
        )),
        "the fold's summary sums the files it hides ({unlisted} bytes): {html}"
    );
    let fold = html.find(r#"<details class="files__more""#).unwrap();
    assert!(
        html.find("a09.glb").unwrap() > fold,
        "the ninth file lives inside the fold: {html}"
    );
}

/// A file over the per-file limit is the deploy failing.
#[tokio::test]
async fn an_oversize_file_gets_a_warning_before_the_wallet() {
    let (_dir, project) = gather("oversize");
    let big = project.root.join("assets/huge.glb");
    std::fs::File::create(&big)
        .unwrap()
        .set_len(50_000_001)
        .unwrap();
    let p = deploy::preview(&project).unwrap();
    assert_eq!(p.oversize, ["assets/huge.glb"]);
    let html = served(&state(project)).await;
    assert!(html.contains(r#"class="panel panel--warn""#), "{html}");
    assert!(html.contains("Over the per-file limit"), "{html}");
    assert!(html.contains("assets/huge.glb"), "{html}");
    assert!(html.contains("50.0 MB"), "{html}");
}

/// A publishable file whose size cannot be read stops the deploy after the
/// wallet has signed; reporting it as 0 bytes made the page say all was fine.
#[cfg(unix)]
#[tokio::test]
async fn a_file_that_cannot_be_read_is_not_called_zero_bytes() {
    let (_dir, project) = gather("dangling");
    std::os::unix::fs::symlink(
        project.root.join("assets/gone.bin"),
        project.root.join("assets/dangling.glb"),
    )
    .unwrap();
    let p = deploy::preview(&project).unwrap();
    assert_eq!(p.unreadable, ["assets/dangling.glb"]);
    assert!(p
        .files
        .iter()
        .any(|(rel, len)| rel == "assets/dangling.glb" && len.is_none()));
    let known: u64 = ["scene.json", "assets/model.glb", "bin/index.js"]
        .iter()
        .map(|r| std::fs::metadata(project.root.join(r)).unwrap().len())
        .sum();
    assert_eq!(p.total_bytes, known, "an unknown size adds nothing");
    let html = served(&state(project)).await;
    assert!(html.contains("Size unreadable"), "{html}");
    assert!(html.contains("Cannot be read"), "{html}");
    assert!(
        !html.contains(r#"dangling.glb</span><span class="sz">0 bytes"#),
        "{html}"
    );
}

/// "You have not built yet" is the likeliest reason a real deploy fails,
/// and `prepare` refuses it after the wallet prompt.
#[tokio::test]
async fn a_scene_that_was_never_built_says_so() {
    let (_dir, project) = gather("unbuilt");
    std::fs::remove_file(project.root.join("bin/index.js")).unwrap();
    let p = deploy::preview(&project).unwrap();
    assert_eq!(p.main, MainBundle::Missing("bin/index.js".to_string()));
    let html = served(&state(project)).await;
    assert!(html.contains("Not built yet"), "{html}");
    assert!(html.contains("dcl-one-sdk build"), "{html}");
    assert!(html.contains("bin/index.js"), "{html}");
    assert!(!html.contains("somebody"), "{html}");
}

#[tokio::test]
async fn a_world_section_that_names_no_world_gets_a_warning_before_the_wallet() {
    let (_dir, project) = scene(
        "nameless-world",
        json!({ "display": { "title": "Gather" }, "worldConfiguration": { "name": "" } }),
    );
    let p = deploy::preview(&project).unwrap();
    assert!(p.nameless_world);
    let html = served(&state(project)).await;
    assert!(html.contains(r#"class="panel panel--warn""#), "{html}");
    assert!(html.contains("names no world"), "{html}");
    assert!(html.contains("Genesis City LAND"), "{html}");
}

/// Asserted on the alarm titles rather than the warn class, because the
/// run-status panel is process-global and a parallel test's failed run may
/// legitimately wear it.
#[tokio::test]
async fn a_built_scene_carries_no_alarm() {
    let (_dir, project) = gather("built");
    let p = deploy::preview(&project).unwrap();
    assert_eq!(p.main, MainBundle::Present("bin/index.js".to_string()));
    let html = served(&state(project)).await;
    for alarm in [
        "Not built yet",
        "Over the per-file limit",
        "Cannot be read",
        "Two names a content server reads as one",
    ] {
        assert!(!html.contains(alarm), "{alarm} on a healthy scene: {html}");
    }
}

/// The walk is the expensive half of an unauthenticated route: the answer
/// is reused for a moment rather than redone per request.
#[tokio::test]
async fn the_walk_is_reused_for_a_moment() {
    let (_dir, project) = gather("cache");
    let st = state(project.clone());
    let first = served(&st).await;
    std::fs::write(project.root.join("assets/late.glb"), vec![1u8; 4096]).unwrap();
    let cached =
        body_of(route(State(st.clone()), ConnectInfo(LOCAL), HeaderMap::new()).await).await;
    assert_eq!(cached, first, "the second request did not walk again");
    assert!(!cached.contains("late.glb"));
    let fresh = served(&st).await;
    assert!(fresh.contains("late.glb"), "an expired entry walks again");
}

/// The error branch is a page on the same open port, held to the same rule:
/// say what is wrong, never say where.
#[tokio::test]
async fn the_error_page_names_the_pattern_and_not_the_path() {
    let (dir, project) = gather("badignore");
    std::fs::write(project.root.join(".dclignore"), "assets/[z-a].png\n").unwrap();
    let root = project.root.clone();
    assert!(deploy::preview(&project).is_err(), "the matcher refuses it");
    let html = served(&state(project)).await;
    assert!(html.contains("This scene cannot be"), "{html}");
    assert!(html.contains("assets/[z-a].png"), "{html}");
    assert!(!html.contains("somebody"), "{html}");
    assert!(!html.contains(&root.display().to_string()), "{html}");
    assert!(!html.contains(&dir.0.display().to_string()), "{html}");
}

#[test]
fn a_path_in_an_error_chain_is_taken_back_out() {
    let root = std::path::Path::new("/home/somebody/scenes/gather");
    let scrubbed = scrub_paths(
        "reading /home/somebody/scenes/gather/assets/a.glb failed",
        root,
    );
    assert_eq!(
        scrubbed, "reading the scene folder/assets/a.glb failed",
        "{scrubbed}"
    );
    assert!(!scrub_paths("under /home/somebody/scenes/other", root).contains("somebody"));
}

/// The address form only changes what an open page renders, but a stranger
/// on the LAN does not get to pick whose holdings this preview looks up.
#[tokio::test]
async fn the_address_form_is_gated_like_the_publish_post() {
    let (_dir, project) = gather("addrgate");
    let st = state(project);
    let submit = |peer: SocketAddr, token: &str, address: &str| {
        target_address(
            State(st.clone()),
            ConnectInfo(peer),
            HeaderMap::new(),
            Form(AddressForm {
                token: token.to_string(),
                address: address.to_string(),
            }),
        )
    };

    let refused = submit(LAN, token(&st), ADDR).await;
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);

    let refused = submit(LOCAL, "wrong", ADDR).await;
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);

    let garbage = submit(LOCAL, token(&st), "vitalik.eth").await;
    assert_eq!(
        garbage.status(),
        StatusCode::UNPROCESSABLE_ENTITY,
        "a non-address never becomes a URL"
    );
    assert!(address_slot(&st).is_none());

    let set = submit(LOCAL, token(&st), &ADDR.to_uppercase().replace("0X", "0x")).await;
    assert_eq!(set.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        address_slot(&st).as_deref(),
        Some(ADDR),
        "stored lowercased, the spelling every lookup uses"
    );

    let cleared = submit(LOCAL, token(&st), "  ").await;
    assert_eq!(cleared.status(), StatusCode::SEE_OTHER);
    assert!(address_slot(&st).is_none(), "an empty submit forgets");
}

/// The account lives in the page bar: a split pill when none is known and a
/// pill naming the account once one is. The card adds its own row only when
/// it has something to say; the dry-run seam keeps the checks a sentence.
#[tokio::test]
async fn the_bar_connects_an_account_and_the_page_checks_with_it() {
    let _guard = crate::deploy::ENV_LOCK.lock().await;
    let (_dir, project) = scene(
        "addrpage",
        json!({ "display": { "title": "Gather" }, "worldConfiguration": { "name": "my.dcl.eth" } }),
    );
    let st = state(project);
    let html = served_target(&st).await;
    assert!(
        html.contains(">Connect Wallet</button>")
            && html.contains(">Connect with DCL</button>")
            && html.contains(r#"action="/target/connect""#),
        "the bar's split pill offers the wallet and the DCL sign-in: {html}"
    );
    assert!(
        html.contains(r#"id="bar-wallet" type="button""#),
        "the wallet half is script-armed, never a bare POST: {html}"
    );
    assert!(
        !html.contains(r#"class="jn2__noterow tgt__addr""#),
        "no account, no card row — the bar is the whole story: {html}"
    );
    assert!(
        html.contains("Connect an account above"),
        "the world list says what it is waiting on: {html}"
    );

    *address_slot(&st) = Some(ADDR.to_string());
    let html = served_target(&st).await;
    assert!(
        html.contains("0x1234\u{2026}5678") && html.contains(&format!(r#"title="{ADDR}""#)),
        "the bar pill names the account, full address on hover: {html}"
    );
    assert!(
        !html.contains(">Connect with DCL</button>") && !html.contains(">Connect Wallet</button>"),
        "a known account replaces the split pill: {html}"
    );
    assert!(
        html.contains(r#"title="Disconnect""#),
        "the pill's sliver disconnects: {html}"
    );
    assert!(
        !html.contains(r#"class="jn2__noterow tgt__addr""#),
        "no card row announces the connection — the data below is the answer: {html}"
    );
    assert!(
        html.contains("Live checks are off for this run"),
        "an unchecked verdict is a sentence, not a guess: {html}"
    );
    *address_slot(&st) = None;
}

/// The connect POST shares every gate with the publish button, and the
/// dry-run seam refuses it outright — a test must never mint a real
/// auth-server request.
#[tokio::test]
async fn the_connect_post_is_gated_and_off_in_dry_runs() {
    let (_dir, project) = gather("connectgate");
    let st = state(project);
    let submit = |peer: SocketAddr, token: &str| {
        target_connect(
            State(st.clone()),
            ConnectInfo(peer),
            HeaderMap::new(),
            Form(ConnectForm {
                token: token.to_string(),
            }),
        )
    };

    let refused = submit(LAN, token(&st)).await;
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);

    let refused = submit(LOCAL, "wrong").await;
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);

    let off = submit(LOCAL, token(&st)).await;
    assert_eq!(
        off.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "the dry-run seam stops the request before any network"
    );
    assert!(connect_slot(&st).is_none(), "nothing was left pending");
}

/// A finished run is history: the ring for this process, the JSONL for the
/// next one, and the History tab tells both the same way.
#[tokio::test]
async fn a_finished_run_lands_in_history_and_survives_a_restart() {
    let (_dir, project) = gather("history");
    let root = project.root.clone();
    let st = state(project.clone());
    let id =
        claim(&st, "World w.dcl.eth".into(), false, "abc123".into()).expect("nothing is running");
    finish(&st, id, RunState::Done("Deployed bafy (HTTP 200)".into()));
    let id =
        claim(&st, "World w.dcl.eth".into(), false, "abc123".into()).expect("done is not running");
    finish(&st, id, RunState::Stale(vec![]));
    let id = claim(
        &st,
        "Parcels 2,12\u{2013}7,15".into(),
        false,
        "abc124".into(),
    )
    .expect("stale is not running");
    finish(
        &st,
        id,
        RunState::Failed(
            "the content server rejected this deployment (HTTP 400)\n  \
             {\"error\":\"Bad request\",\"message\":\"must be number\"}\n  \
             \u{2192} try: read the server message above\n  \
             \u{2192} try: re-run with --verbose for the full response\n"
                .into(),
        ),
    );
    {
        let held = history_slot(&st);
        assert_eq!(held.len(), 3);
        assert_eq!(held[0].outcome, "failed", "newest first");
        assert_eq!(held[1].outcome, "nothing published");
        assert_eq!(held[2].outcome, "published");
        assert_eq!(
            held[2].detail.as_deref(),
            Some("Deployed bafy (HTTP 200)"),
            "a publish records what went up"
        );
        assert_eq!(
            held[1].detail.as_deref(),
            Some("the scene changed while the page was open")
        );
    }
    let pane = history_rows_pane(&history_rows(&st, &root));
    assert!(pane.contains("World w.dcl.eth"), "{pane}");
    assert!(pane.contains("published"), "{pane}");
    assert!(pane.contains("Deployed bafy (HTTP 200)"), "{pane}");
    assert!(
        pane.contains("rejected this deployment (HTTP 400)") && pane.contains("must be number"),
        "a failure row says why: {pane}"
    );
    assert!(
        !pane.contains("try:"),
        "the try steps are advice for the moment, not history: {pane}"
    );

    assert_eq!(
        std::fs::read_to_string(root.join(".dcl-one").join(".gitignore")).unwrap(),
        "*\n",
        "the record's directory ignores itself, whatever the scene's .gitignore says"
    );

    let fresh = state(project);
    let rows = history_rows(&fresh, &root);
    assert_eq!(rows.len(), 3, "the on-disk record outlives the process");
    assert_eq!(rows[0].outcome, "failed");
    assert!(
        rows[0]
            .detail
            .as_deref()
            .is_some_and(|d| d.contains("HTTP 400")),
        "the why survives the restart: {:?}",
        rows[0].detail
    );
    assert_eq!(rows[1].outcome, "nothing published");
    assert!(
        history_rows_pane(&[]).contains("No deployments yet"),
        "no history is still a pane"
    );
}

/// Ours in the accent with the base marked, neighbours kept, the replaced
/// footprint dashed — and past the span cap, no grid at all.
#[test]
fn the_after_map_draws_kept_replaced_and_ours() {
    let state = RemoteState {
        current: Some(current_scene("Old", None, vec![(0, 0), (1, 0)])),
        others: vec![remote_scene("Bazaar", vec![(2, 0), (2, 1)])],
        hashes: HashSet::new(),
    };
    let ours = [(0, 0), (0, 1)];
    let html = after_map(&state, &ours, (0, 0));
    assert_eq!(html.matches("lay__cell--base").count(), 1, "{html}");
    assert_eq!(
        html.matches(r#"class="lay__cell lay__cell--in""#).count(),
        1,
        "0,1 is ours and not the base: {html}"
    );
    assert_eq!(
        html.matches("dep__cell--kept").count(),
        2 + 1,
        "two kept cells and the legend swatch: {html}"
    );
    assert_eq!(
        html.matches(r#"class="lay__cell dep__cell--was""#).count(),
        1,
        "only 1,0 keeps the replaced mark — 0,0 is ours now: {html}"
    );
    assert!(html.contains(r#"style="--lay-cols:3""#), "{html}");

    let sprawling = RemoteState {
        current: None,
        others: vec![remote_scene("Far", vec![(0, 0), (40, 40)])],
        hashes: HashSet::new(),
    };
    assert_eq!(
        after_map(&sprawling, &ours, (0, 0)),
        "",
        "past the span cap the rows tell the story alone"
    );
}

/// The multiscene pane sums only sizes the server reported, and each row
/// names its fate.
#[test]
fn the_multiscene_pane_sums_only_reported_sizes() {
    let body = json!({ "scenes": [
        { "parcels": ["0,0"], "size": "2048", "entity": {
            "metadata": { "display": { "title": "Ours Before" } } } },
        { "parcels": ["3,3"], "size": "4096", "entity": {
            "metadata": { "display": { "title": "Bazaar" } } } },
        { "parcels": ["4,4"], "entity": {
            "metadata": { "display": { "title": "Sizeless" } } } }
    ]});
    let dest = resolve_dest(
        &json!({ "worldConfiguration": { "name": "w.dcl.eth" }, "scene": { "parcels": ["0,0"], "base": "0,0" } }),
        None,
        None,
    );
    let status = LiveStatus {
        remote: world_remote(&body, &dest.pointers),
        reuse: Some(reuse(0, 0, 1, 500)),
    };
    let html = multiscene_pane(&dest, &status);
    assert!(
        html.contains(&format!(
            "Scenes in this world hold {}",
            deploy::human_size(2048 + 4096)
        )),
        "the sum is only what was reported: {html}"
    );
    assert!(
        html.contains(&format!(
            "uploads {} of new content",
            deploy::human_size(500)
        )),
        "{html}"
    );
    assert!(html.contains("replaced by this publish"), "{html}");
    assert!(html.contains("kept"), "{html}");
    assert!(
        html.contains("After this publish"),
        "the map column is there: {html}"
    );
}

/// Pointing the scene rewrites the file a deploy reads: a world name lands
/// in worldConfiguration.name, an empty value strips it — and the gates are
/// the scene editors', never opened by --allow-remote-deploy.
#[tokio::test]
async fn pointing_the_scene_rewrites_its_destination_in_scene_json() {
    let (_dir, project) = gather("point");
    let root = project.root.clone();
    let st = state(project);
    let submit = |peer: SocketAddr, world: &str| {
        target_point(
            State(st.clone()),
            ConnectInfo(peer),
            HeaderMap::new(),
            Form(PointForm {
                token: token(&st).to_string(),
                world: world.to_string(),
            }),
        )
    };

    let refused = submit(LAN, "w.dcl.eth").await;
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);

    let garbage = submit(LOCAL, "javascript:alert(1)").await;
    assert_eq!(
        garbage.status(),
        StatusCode::UNPROCESSABLE_ENTITY,
        "only a world name may enter the file"
    );

    let ok = submit(LOCAL, "Gather.DCL.eth").await;
    assert_eq!(ok.status(), StatusCode::SEE_OTHER);
    let on_disk: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("scene.json")).unwrap()).unwrap();
    assert_eq!(
        on_disk["worldConfiguration"]["name"], "gather.dcl.eth",
        "lowercased, the worlds tier's spelling"
    );
    assert_eq!(
        st.first_project().unwrap().scene_json["worldConfiguration"]["name"],
        "gather.dcl.eth",
        "the running preview follows the file"
    );

    let back = submit(LOCAL, "  ").await;
    assert_eq!(back.status(), StatusCode::SEE_OTHER);
    let on_disk: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("scene.json")).unwrap()).unwrap();
    assert!(
        on_disk.get("worldConfiguration").is_none(),
        "an empty point returns the scene to its parcels"
    );
}

/// Every world that is not already the target carries the re-aim button;
/// the target itself does not — the row says so instead.
#[test]
fn the_world_rows_offer_the_re_aim() {
    let dest = resolve_dest(
        &json!({ "worldConfiguration": { "name": "my.dcl.eth" } }),
        None,
        None,
    );
    let row = |name: &str| world_row(name, Some(1), Some("My World"));
    let rights = owner_of(vec![row("my.dcl.eth"), row("other.dcl.eth")]);
    let html = your_worlds("/t", "tok", &dest, Some(&rights));
    let my = html.find("my.dcl.eth").unwrap();
    let other = html.find("other.dcl.eth").unwrap();
    assert!(
        html[..other].contains("Current target") || html[my..].contains("Current target"),
        "{html}"
    );
    assert_eq!(
        html.matches(r#"action="/t/target/point""#).count(),
        1,
        "one re-aim form, on the row that is not the target: {html}"
    );
    assert!(
        html.contains(r#"name="world" value="other.dcl.eth""#)
            && html.contains(">Select World</button>"),
        "{html}"
    );
}

/// The current target leads the list, so the listing cap can never be the
/// reason it is missing: eleven other worlds sort ahead of it alphabetically.
#[test]
fn the_current_target_leads_your_worlds() {
    let dest = resolve_dest(
        &json!({ "worldConfiguration": { "name": "zz-last.dcl.eth" } }),
        None,
        None,
    );
    let mut worlds: Vec<WorldRow> = (0..11)
        .map(|i| world_row(&format!("w{i:02}.dcl.eth"), None, None))
        .collect();
    worlds.push(world_row("zz-last.dcl.eth", None, None));
    let rights = owner_of(worlds);
    let html = your_worlds("/t", "tok", &dest, Some(&rights));
    let target = html.find("zz-last.dcl.eth").expect("the target is listed");
    let first_other = html.find("w00.dcl.eth").expect("others still listed");
    assert!(target < first_other, "the target leads: {html}");
    assert!(html.contains("Current target"), "{html}");
}

/// An auto-started run nobody signed vanishes: no failure panel, no history
/// row. The same timeout on a button-press run keeps its failure.
#[tokio::test]
async fn an_unsigned_auto_run_ends_quietly() {
    let (_dir, project) = gather("autoquiet");
    let root = project.root.clone();
    let st = state(project);
    const TIMEOUT: &str = "no signature arrived within 10 minutes \u{2014} deployment abandoned";
    let id = claim(&st, "World w.dcl.eth".into(), true, "abc123".into()).expect("free slot");
    finish(&st, id, RunState::Failed(TIMEOUT.into()));
    assert!(runs(&st).is_none(), "the slot cleared for the next visit");
    assert!(history_rows(&st, &root).is_empty(), "nothing is recorded");

    let id = claim(&st, "World w.dcl.eth".into(), false, "abc123".into()).expect("slot is free");
    finish(&st, id, RunState::Failed(TIMEOUT.into()));
    assert!(
        matches!(
            runs(&st).as_ref().map(|r| &r.state),
            Some(RunState::Failed(_))
        ),
        "a button-press run keeps its failure"
    );
}

/// A pending run whose payload moved after its build finished re-mints —
/// new id, current fingerprint, the signer slot emptied — and keeps its
/// provenance.
#[tokio::test]
async fn a_drifted_pending_run_is_reminted_before_the_wallet_sees_it() {
    let (_dir, project) = gather("drift");
    let st = state(project);
    let old = claim(&st, "World w.dcl.eth".into(), true, "aaaa".into()).expect("free slot");
    runs(&st).as_mut().expect("just claimed").signing = Some("/deploy".into());
    *signer_slot(&st) = Some(parked_signer());

    let new = drift_reclaim(&st, "World w.dcl.eth".into(), "bbbb").expect("drift re-mints");
    assert_ne!(old, new, "a re-mint is a new claim, not a touch-up");
    assert!(
        signer_slot(&st).is_none(),
        "the stale entity's signer is gone"
    );
    let slot = runs(&st);
    let r = slot.as_ref().expect("the slot is held");
    assert_eq!(r.print, "bbbb");
    assert!(r.auto, "provenance survives the re-mint");
    assert!(r.signing.is_none(), "the new build has not registered yet");
}

/// ...and it declines everywhere a re-mint would be wrong: a build still
/// running, an unchanged payload, a wallet that already answered, and an
/// adopted CLI signing.
#[tokio::test]
async fn the_remint_declines_matching_midbuild_cli_and_signed_runs() {
    let (_dir, project) = gather("driftno");
    let st = state(project);

    claim(&st, "World w.dcl.eth".into(), true, "aaaa".into()).expect("free slot");
    assert!(
        drift_reclaim(&st, "World w.dcl.eth".into(), "bbbb").is_none(),
        "mid-build: the release tree is being written"
    );

    runs(&st).as_mut().expect("held").signing = Some("/deploy".into());
    assert!(
        drift_reclaim(&st, "World w.dcl.eth".into(), "aaaa").is_none(),
        "an unchanged payload is not drift"
    );

    let signed = parked_signer();
    signed.note_signer_for_tests("0xe7f7000000000000000000000000000000000000");
    *signer_slot(&st) = Some(signed);
    assert!(
        drift_reclaim(&st, "World w.dcl.eth".into(), "bbbb").is_none(),
        "a signed deploy is past recall"
    );

    *runs(&st) = None;
    *signer_slot(&st) = None;
    adopt_cli_signing(&st, parked_signer());
    assert!(
        drift_reclaim(&st, "World w.dcl.eth".into(), "bbbb").is_none(),
        "a CLI publish is a terminal's, not this page's"
    );
}

/// A superseded deploy's tail writes no state onto a slot it no longer owns,
/// wipes no signer the newer run registered, and records no history.
#[tokio::test]
async fn a_superseded_deploy_cannot_touch_the_newer_run() {
    let (_dir, project) = gather("driftsup");
    let root = project.root.clone();
    let st = state(project);
    let old = claim(&st, "World w.dcl.eth".into(), true, "aaaa".into()).expect("free slot");
    runs(&st).as_mut().expect("held").signing = Some("/deploy".into());
    let new = drift_reclaim(&st, "World w.dcl.eth".into(), "bbbb").expect("drift re-mints");
    *signer_slot(&st) = Some(parked_signer());

    finish(
        &st,
        old,
        RunState::Failed("no signature arrived within 10 minutes".into()),
    );
    {
        let slot = runs(&st);
        let r = slot.as_ref().expect("the newer run holds the slot");
        assert_eq!(r.id, new);
        assert!(matches!(r.state, RunState::Running), "still pending");
    }
    assert!(
        signer_slot(&st).is_some(),
        "the newer run's signer survives"
    );
    assert!(
        history_rows(&st, &root).is_empty(),
        "a superseded tail is not history"
    );
}

/// A cold cache never makes the page wait: the first render answers NOW
/// with "checking" placeholders and the warming mark, and because every
/// fetch outcome is cached, failures included, the reloads converge on a
/// final render with no mark in bounded time.
#[tokio::test]
async fn a_cold_cache_renders_now_and_the_reloads_converge() {
    let (_dir, project) = gather("warming");
    let mut st = crate::start::testkit::state(vec![project.clone()]);
    st.deploy_dry_run = false;
    let st = Arc::new(st);
    *address_slot(&st) = Some("0x00000000000000000000000000000000000000ab".into());
    // Every base aims at a port nothing listens on, so the spawned warm-up
    // fails fast into a cached sentence without leaving the machine.
    let dest = resolve_dest(&project.scene_json, Some("http://127.0.0.1:9"), None);
    let preview = cached_preview(&st, &project).await;

    let (status, rights, warming) = status_and_rights(&st, &project, &dest, &preview, "aaaa").await;
    assert!(warming, "cold caches mean a warming render");
    assert!(
        matches!(&status.remote, Remote::Unknown(why) if why.contains("Checking the destination")),
        "the status placeholder says checking, not broken"
    );
    let rights = rights.expect("a known wallet still gets a rights row");
    assert!(
        matches!(&rights.verdict, Verdict::Unchecked(why) if why.contains("Checking what this wallet")),
        "the rights placeholder says checking, not nothing"
    );
    assert_eq!(rights.address, "0x00000000000000000000000000000000000000ab");

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let (status, _, warming) = status_and_rights(&st, &project, &dest, &preview, "aaaa").await;
        if !warming {
            assert!(
                !matches!(&status.remote, Remote::Unknown(why) if why.contains("Checking the destination")),
                "the settled answer is the cached outcome, not the placeholder"
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the warming loop must converge once the fetches cached their outcomes"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        warming_marker(true).contains(r#"id="page-warming""#)
            && warming_marker(true).contains("<noscript>"),
        "the mark and its no-JS fallback"
    );
    assert!(warming_marker(false).is_empty());
    assert!(
        include_str!("page_common.js").contains("page-warming"),
        "the shared script knows the mark"
    );
}

/// `live_identity` hands back an unexpired identity and forgets an expired
/// one, so a deploy signs itself for the hour and falls back to the wallet after.
#[test]
fn a_delegated_identity_is_used_while_live_and_dropped_when_it_lapses() {
    let (_dir, project) = gather("identity");
    let st = state(project);
    let mk = |exp_ms: i64| deploy::DeployIdentity {
        signer: "0xe7f78d2c9a9375153476834d2db32632384b01e1".into(),
        ephemeral_key: "0x0000000000000000000000000000000000000000000000000000000000000042".into(),
        delegation_payload: "Decentraland Login\nEphemeral address: x\nExpiration: y".into(),
        delegation_signature: "0xsig".into(),
        expiration_ms: exp_ms,
    };
    *identity_slot(&st) = Some(mk(deploy::now_ms() + 3_600_000));
    let live = live_identity(&st).expect("an unexpired identity is live");
    assert_eq!(live.signer, "0xe7f78d2c9a9375153476834d2db32632384b01e1");
    assert!(
        identity_slot(&st).is_some(),
        "and still held for the next deploy"
    );

    *identity_slot(&st) = Some(mk(deploy::now_ms() - 1));
    assert!(
        live_identity(&st).is_none(),
        "an expired identity is not used"
    );
    assert!(
        identity_slot(&st).is_none(),
        "and is dropped so the wallet takes over"
    );
}

/// The preflight is the refusal moved in front of the signature: gated like
/// the signing routes, honest about a dry run, never an answer for a string
/// that is not an address — and a shrug for a target the oracle has no
/// authority over (a self-hosted node with its own deploy policy), never a
/// mainnet-keyed refusal that would block a server about to say yes.
#[tokio::test]
async fn the_preflight_answers_before_the_wallet_signs() {
    let (_dir, project) = gather("preflight");
    let st = state(project);
    let ask = |peer: SocketAddr, addr: &str| {
        preflight(
            State(st.clone()),
            ConnectInfo(peer),
            HeaderMap::new(),
            Ok(axum::Json(
                serde_json::from_value(json!({ "address": addr })).unwrap(),
            )),
        )
    };

    let blocked = ask(LAN, ADDR).await;
    assert_eq!(
        blocked.status(),
        StatusCode::FORBIDDEN,
        "the check runs where the signature runs"
    );

    let garbage = ask(LOCAL, "vitalik.eth").await;
    assert_eq!(garbage.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let off = ask(LOCAL, ADDR).await;
    assert_eq!(off.status(), StatusCode::OK);
    let v = json_of(off).await;
    assert_eq!(
        v["verdict"], "unchecked",
        "a dry run says so instead of guessing: {v}"
    );
    assert!(
        SCRIPT.contains("/preflight") && SCRIPT.contains("may_not"),
        "the sign flow asks before personal_sign and stops on a refusal"
    );

    let (foreign, _rx) =
        crate::linker::new_state(linker_deploy("https://my-node.example.com/content", None));
    *signer_slot(&st) = Some(foreign);
    let v = json_of(ask(LOCAL, ADDR).await).await;
    assert_eq!(v["verdict"], "unchecked", "{v}");
    assert!(
        v["why"].as_str().unwrap().contains("my-node.example.com"),
        "the shrug names the server that decides: {v}"
    );

    let (upstream, _rx) =
        crate::linker::new_state(linker_deploy("https://interconnected.online/content", None));
    *signer_slot(&st) = Some(upstream);
    let v = json_of(ask(LOCAL, ADDR).await).await;
    assert_eq!(
        v["why"], "live checks are off for this run",
        "a Genesis-network target is the oracle's to judge (the dry-run seam answers here): {v}"
    );
    *signer_slot(&st) = None;
}

#[test]
fn translate_footprint_moves_the_whole_shape() {
    let mut scene = json!({
        "scene": { "parcels": ["0,0", "1,0", "0,1"], "base": "0,0" }
    });
    translate_footprint(&mut scene, (20, -30)).unwrap();
    assert_eq!(scene["scene"]["base"], json!("20,-30"));
    assert_eq!(
        scene["scene"]["parcels"],
        json!(["20,-30", "21,-30", "20,-29"]),
        "every parcel rides the same delta, shape intact"
    );
}

#[test]
fn translate_footprint_refuses_to_leave_the_genesis_map() {
    let mut scene = json!({
        "scene": { "parcels": ["0,0", "5,0"], "base": "0,0" }
    });
    let why = translate_footprint(&mut scene, (160, 0)).unwrap_err();
    assert!(
        why.contains("165,0"),
        "the error names the parcel that falls off the map: {why}"
    );
    assert_eq!(
        scene["scene"]["parcels"],
        json!(["0,0", "5,0"]),
        "a refused move changes nothing"
    );
    translate_footprint(&mut scene, (158, 163)).unwrap();
    assert_eq!(
        scene["scene"]["base"],
        json!("158,163"),
        "the expansion districts past 150 are still the map"
    );
}

#[test]
fn translate_footprint_same_base_is_a_no_op() {
    let mut scene = json!({
        "scene": { "parcels": ["3,4"], "base": "3,4" }
    });
    translate_footprint(&mut scene, (3, 4)).unwrap();
    assert_eq!(scene["scene"]["parcels"], json!(["3,4"]));
}

#[test]
fn a_failure_detail_is_the_why_without_the_advice() {
    let why = "the content server rejected this deployment (HTTP 400)\n  \
               {\"error\":\"Bad request\"}\n  \u{2192} try: read the server message above\n\n";
    assert_eq!(
        failure_detail(why),
        "the content server rejected this deployment (HTTP 400)\n  {\"error\":\"Bad request\"}"
    );
    let long = "x".repeat(DETAIL_CAP + 50);
    let cut = failure_detail(&long);
    assert!(cut.ends_with('\u{2026}') && cut.chars().count() == DETAIL_CAP + 1);

    let legacy =
        json!({ "at_ms": 1, "target": "World w.dcl.eth", "signer": null, "outcome": "published" });
    let (_dir, project) = scene("legacy-history", json!({}));
    let path = history_path(&project.root);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, format!("{legacy}\n")).unwrap();
    let rows = history_rows(&state(project.clone()), &project.root);
    assert_eq!(rows.len(), 1);
    assert!(rows[0].detail.is_none());
    let pane = history_rows_pane(&rows);
    assert!(
        pane.contains("World w.dcl.eth") && pane.contains("tgt__history-status--ok\">published"),
        "{pane}"
    );
}

#[test]
fn the_rights_column_lists_the_parcels_the_wallet_may_publish_to() {
    let mut owned: Vec<(i64, i64)> = (0..30).map(|i| (100 + i, 7)).collect();
    owned.insert(0, (5, -3));
    owned.insert(1, (6, -3));
    let rights = Rights {
        verdict: Verdict::May("rights held on all 2 declared parcels".into()),
        holdings: Some(Holdings {
            parcels: 32,
            estates: 0,
            operated: 1,
            coords: owned.iter().copied().chain([(9, 9)]).collect(),
            owned: owned.clone(),
            operated_coords: vec![(9, 9)],
        }),
        parcel_rights: vec![
            ParcelRight { pointer: "5,-3".into(), leg: Some("owner") },
            ParcelRight { pointer: "6,-3".into(), leg: Some("owner") },
        ],
        parcels_note: Some(
            "worlds-content-server.decentraland.org answered HTTP 404 \u{2014} parcel rights read from peer.decentraland.org"
                .into(),
        ),
        ..Rights::unchecked(ADDR, "")
    };
    let html = land_rights_col(Some(&rights));
    assert!(html.contains("Parcels you may publish to"), "{html}");
    assert!(
        html.contains("5,-3 \u{b7} 6,-3 \u{b7} 100,7"),
        "owned, in order: {html}"
    );
    assert!(
        html.contains("and 8 more"),
        "past the cap the rest is a count: {html}"
    );
    assert!(html.contains("Operated") && html.contains("9,9"), "{html}");
    assert!(
        html.contains("parcel rights read from peer.decentraland.org"),
        "the rows say whose answer they are: {html}"
    );

    let none = Rights {
        holdings: Some(Holdings {
            parcels: 0,
            estates: 0,
            operated: 0,
            coords: Vec::new(),
            owned: Vec::new(),
            operated_coords: Vec::new(),
        }),
        parcels_note: None,
        ..rights
    };
    let html = land_rights_col(Some(&none));
    assert!(
        !html.contains("Parcels you may publish to"),
        "no rights, no list: {html}"
    );
    assert!(!html.contains("parcel rights read from"), "{html}");
}

#[test]
fn target_design_keeps_live_actions_and_distinguishes_denied_rights() {
    let coords: Vec<_> = (12..16).flat_map(|y| (2..8).map(move |x| (x, y))).collect();
    let pointers: Vec<_> = coords.iter().map(|(x, y)| format!("{x},{y}")).collect();
    let (_dir, project) = scene(
        "target-design",
        json!({
            "display": { "title": "Hexabricks" },
            "scene": { "base": "2,12", "parcels": pointers }
        }),
    );
    let dest = resolve_dest(&project.scene_json, None, None);
    let preview = deploy::preview(&project).unwrap();
    let mut live = status(Remote::Known(RemoteState {
        current: Some(current_scene(
            "Hexabricks",
            Some(deploy::now_ms() - 18_000_000),
            coords.clone(),
        )),
        others: vec![],
        hashes: HashSet::new(),
    }));
    live.reuse = Some(reuse(2, 1_800, 1, 317));
    let mut shared = world_row("shared.dcl.eth", Some(2), Some("Shared stage"));
    shared.owned = false;
    let mut rights = Rights {
        verdict: Verdict::May("rights held on all 24 declared parcels".into()),
        worlds: vec![
            world_row("owned.dcl.eth", Some(1), Some("Owned stage")),
            world_row("empty.dcl.eth", Some(0), None),
            shared,
        ],
        holdings: Some(Holdings {
            parcels: 2,
            estates: 1,
            operated: 24,
            coords: vec![(2, 12), (20, -4), (75, -144)],
            owned: vec![(75, -144)],
            operated_coords: coords.clone(),
        }),
        parcel_rights: pointers
            .iter()
            .map(|p| ParcelRight {
                pointer: p.clone(),
                leg: Some("update manager"),
            })
            .collect(),
        ..Rights::unchecked(ADDR, "")
    };
    let history = vec![PastRun {
        at_ms: deploy::now_ms() - 18_000_000,
        target: "Genesis City 2,12".into(),
        signer: Some(ADDR.into()),
        outcome: "published".into(),
        detail: Some("Deployed bafy-test (HTTP 200)".into()),
    }];
    for denied in [false, true] {
        if denied {
            rights.verdict = Verdict::MayNot {
                why: "No update rights on 1 of 24 parcels".into(),
                remedy: "Ask the owner for update-operator rights or move the footprint.".into(),
            };
            rights.parcel_rights[0].leg = None;
        }
        let card = target_card(
            "Hexabricks",
            "/t/demo",
            "test-token",
            &dest,
            &preview,
            &live,
            Some(&rights),
            None,
            &history,
        );
        let land = card
            .split("tgt__pane--land\">")
            .nth(1)
            .unwrap()
            .split("<details class=\"tgt__history\">")
            .next()
            .unwrap();
        assert!(land.contains("Selected destination: LAND · Base 2,12"));
        assert!(land.contains("action=\"/t/demo/target/base\""));
        assert!(land.contains("name=\"token\" value=\"test-token\""));
        assert_eq!(land.contains("href=\"/t/demo/deploy\""), !denied);
        assert_eq!(land.contains("Deploy blocked"), denied);
        assert_eq!(
            land.contains("tgt__cell--missing\" title=\"2,12 — no update rights"),
            denied
        );
        assert!(
            card.contains("Worlds you collaborate on")
                && card.contains("Show 1 world with no scenes yet")
        );
        // Optional snapshots for browser review, using the real renderer and CSS.
        if let Ok(dir) = std::env::var("DCL_TARGET_DESIGN_CAPTURE") {
            let nav = super::super::chrome::Nav {
                active: "target",
                badge: "live",
                host: "127.0.0.1:8001",
                account: Some(ADDR.into()),
                token: "test-token",
            };
            let body = format!(
                r#"<main class="dash"><section id="target" class="sec">{card}</section></main><script>{TARGET_SCRIPT}</script>"#
            );
            let html = document(
                "Target",
                "/t/demo",
                target_css(),
                "#target",
                "Skip to target",
                Some(&nav),
                &body,
            );
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                Path::new(&dir).join(if denied { "denied.html" } else { "land.html" }),
                html,
            )
            .unwrap();
        }
    }
    let unchecked = target_card(
        "<Scene>",
        "",
        "tok",
        &dest,
        &preview,
        &LiveStatus::unknown("Checking server"),
        None,
        None,
        &[],
    );
    assert!(!unchecked.contains("Deploy blocked"));
    assert!(unchecked.contains("&lt;Scene&gt;") && unchecked.contains("Connect an account"));
    assert!(!unchecked.contains("HTTP 200"));
}

#[test]
fn world_groups_keep_empty_current_targets_visible_and_unknown_counts_honest() {
    let dest = resolve_dest(
        &json!({"worldConfiguration": {"name": "current.dcl.eth"}}),
        None,
        None,
    );
    let rights = owner_of(vec![
        world_row("empty.dcl.eth", Some(0), None),
        world_row("current.dcl.eth", Some(0), None),
        world_row("unknown.dcl.eth", None, Some("<Unknown>")),
    ]);
    let html = your_worlds("", "tok", &dest, Some(&rights));
    let fold = html.find("<details").unwrap();
    assert!(html[..fold].contains("current.dcl.eth"));
    assert!(html[..fold].contains("scene count unavailable"));
    assert!(html[fold..].contains("empty.dcl.eth"));
    assert!(html.contains("&lt;Unknown&gt;"));
    assert!(!html.contains("value=\"current.dcl.eth\""));
}

#[test]
fn every_overlapping_world_scene_is_shown_as_replaced_in_full() {
    let dest = resolve_dest(
        &json!({
            "worldConfiguration": {"name": "w.dcl.eth"},
            "scene": {"base": "0,0", "parcels": ["0,0", "1,0"]}
        }),
        None,
        None,
    );
    let body = json!({"scenes": [
        {"parcels": ["0,0", "0,1"], "entity": {"metadata": {"display": {"title": "First overlap"}}}},
        {"parcels": ["1,0", "1,1"], "entity": {"metadata": {"display": {"title": "Second overlap"}}}},
        {"parcels": ["4,0"], "entity": {"metadata": {"display": {"title": "Neighbor"}}}}
    ]});
    let live = status(world_remote(&body, &dest.pointers));
    let Remote::Known(remote) = &live.remote else {
        panic!("known world")
    };
    let map = after_map(remote, &[(0, 0), (1, 0)], (0, 0));
    assert!(map.contains(r#"class="lay__cell dep__cell--was" title="Replaced 0,1""#));
    assert!(map.contains(r#"class="lay__cell dep__cell--was" title="Replaced 1,1""#));
    assert!(map.contains(r#"class="lay__cell dep__cell--kept" title="Kept 4,0""#));
    let panel = server_panel(&dest, &live);
    assert!(panel.contains("Second overlap — replaced by this publish"));
    assert!(panel.contains("Neighbor — kept in place by this publish"));
    let details = multiscene_pane(&dest, &live);
    assert_eq!(
        details
            .matches("2 parcels · replaced by this publish")
            .count(),
        2
    );
    assert!(details.contains("including its parcels outside the footprint"));
}

#[test]
fn unavailable_world_layout_is_not_presented_as_an_empty_world() {
    let dest = resolve_dest(
        &json!({"worldConfiguration": {"name": "w.dcl.eth"}}),
        None,
        None,
    );
    for remote in [
        Remote::Unknown("Still checking".into()),
        Remote::Unreachable("HTTP 503".into()),
    ] {
        let html = multiscene_pane(&dest, &status(remote));
        assert!(html.contains("World layout unavailable"));
        assert!(!html.contains("No scenes published") && !html.contains("No other scenes"));
    }
    assert!(multiscene_pane(&dest, &status(Remote::Empty)).contains("No scenes published"));
}

#[test]
fn world_selection_shows_coordinates_server_and_review_without_inferring_a_mode() {
    let (_dir, project) = scene(
        "world-selection-docs",
        json!({
            "worldConfiguration": {"name": "example.eth"},
            "scene": {"base": "0,0", "parcels": ["0,0"]}
        }),
    );
    let dest = resolve_dest(
        &project.scene_json,
        Some("https://custom.example.org"),
        None,
    );
    let preview = deploy::preview(&project).unwrap();
    let live = LiveStatus {
        remote: Remote::Known(RemoteState {
            current: None,
            others: vec![remote_scene("One neighbor", vec![(4, 4)])],
            hashes: HashSet::new(),
        }),
        reuse: Some(reuse(3, 1000, 0, 0)),
    };
    let rights = owner_of(vec![world_row(
        "example.eth",
        Some(1),
        Some("One neighbor"),
    )]);
    let html = target_card(
        "My scene",
        "/t/demo",
        "token",
        &dest,
        &preview,
        &live,
        Some(&rights),
        None,
        &[],
    );
    assert!(html.contains("Selected destination: World example.eth · Base 0,0"));
    assert!(html.contains("Publishing on custom.example.org"));
    assert!(html.contains(r#"href="/t/demo/deploy">Review deployment"#));
    assert!(!html.contains("Deploy 0 changes"));
    assert!(!html.contains("· Multiscene World"));
    assert!(html.contains("assigned coordinates") && html.contains("Permission to visit a World"));
    assert!(html.contains(r#"href="/t/demo/scene"#));
    assert!(
        !html.contains(r#"action="/t/demo/deploy""#),
        "selection never publishes"
    );
    if let Ok(dir) = std::env::var("DCL_TARGET_DESIGN_CAPTURE") {
        let nav = super::super::chrome::Nav {
            active: "target",
            badge: "",
            host: "127.0.0.1:8001",
            account: Some(ADDR.into()),
            token: "token",
        };
        let body = format!(
            r#"<main class="dash"><section id="target" class="sec">{html}</section></main><script>{TARGET_SCRIPT}</script>"#
        );
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            Path::new(&dir).join("world.html"),
            document(
                "Target",
                "/t/demo",
                target_css(),
                "#target",
                "Skip to target",
                Some(&nav),
                &body,
            ),
        )
        .unwrap();
    }
}

#[tokio::test]
async fn reviewing_with_a_delegated_identity_waits_for_the_publish_button() {
    let _guard = crate::deploy::ENV_LOCK.lock().await;
    let (_dir, project) = gather("delegated-review");
    let preview = deploy::preview(&project).unwrap();
    let print = fingerprint(&project.root, &preview);
    let dest = scene_dest(&project);
    let mut st = state(project);
    Arc::get_mut(&mut st).unwrap().deploy_dry_run = false;
    *identity_slot(&st) = Some(deploy::DeployIdentity {
        signer: ADDR.into(),
        ephemeral_key: "0x0000000000000000000000000000000000000000000000000000000000000042".into(),
        delegation_payload: "test delegation".into(),
        delegation_signature: "0xsig".into(),
        expiration_ms: deploy::now_ms() + 3_600_000,
    });
    // Leave the read-only status placeholder pending without external I/O.
    warm_slot(&st).insert(format!("status|{}|{print}", dest.headline));
    let html = served(&st).await;
    assert!(
        runs(&st).is_none(),
        "a GET must not claim a signed publication"
    );
    assert!(signer_slot(&st).is_none());
    assert!(html.contains(r#"id="publish" method="post" action="/deploy""#));
    assert!(html.contains(r#">Publish</button>"#));
    assert!(
        live_identity(&st).is_some(),
        "the explicit Publish can still use the session"
    );
}
