use super::*;
use crate::deploy::WORLDS_CONTENT_SERVER;
use crate::start::deploy_status::*;
use axum::body::to_bytes;
use axum::http::{header, StatusCode};
use serde_json::json;
use std::collections::{HashMap, HashSet};

/// A real directory with real files, because the property being tested is
/// about what a rendered payload list says: the previous version of this
/// test pointed at a path that did not exist, so nothing was listed and
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

/// `start::mod`'s `AppState` is private to `start`, and this module is one
/// of its children, so the page can be driven through its real route
/// without touching the file that owns the type.
/// The loopback gate, which the router tests cannot reach: their client is
/// always 127.0.0.1, and this machine's firewall refuses a real inbound
/// connection to its own LAN address, so the only honest way to put a
/// non-loopback peer in front of the handler is to hand it one.
#[tokio::test]
async fn a_peer_off_this_machine_cannot_publish() {
    let (_tree, project) = scene("remote", json!({ "display": { "title": "Gather" } }));
    let st = state(project);
    let lan: SocketAddr = ([192, 168, 1, 9], 51000).into();

    let refused = start(
        State(st.clone()),
        ConnectInfo(lan),
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

    let page = page(&st, &HeaderMap::new(), false).await;
    let body = axum::body::to_bytes(page.into_body(), usize::MAX)
        .await
        .unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(
        !body.contains(r#"id="publish""#),
        "no publish button for a remote reader"
    );
    assert!(
        body.contains("Publishing runs on the machine"),
        "{body:.400}"
    );
}

fn state(project: Project) -> Arc<AppState> {
    Arc::new(crate::start::testkit::state(vec![project]))
}

/// The preview cache is process-wide, so a test that wants a fresh walk
/// drops its own entry and leaves the rest alone.
fn forget(st: &Arc<AppState>) {
    let root = st.projects()[0].root.clone();
    cache(st).retain(|(p, _, _)| *p != root);
}

/// Everything a visitor of `/deploy` actually receives, fetched the way
/// they fetch it. Tests that rebuild the page's strings for themselves
/// prove nothing about the page: this suite used to do exactly that, and
/// stayed green while `page` printed the absolute path of the scene.
async fn served(st: &Arc<AppState>) -> String {
    forget(st);
    let resp = route(
        State(st.clone()),
        ConnectInfo(([127, 0, 0, 1], 0).into()),
        HeaderMap::new(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp
        .headers()
        .get(header::CONTENT_TYPE)
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("text/html"));
    let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

/// This page is served to anything that can reach the port, so the one
/// thing it must never answer is where the scene lives on disk.
#[tokio::test]
async fn the_deploy_page_never_names_the_scene_directory() {
    let (dir, project) = scene("names", json!({ "display": { "title": "Gather" } }));
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

/// The page is one landing-language card: the destination as the card's
/// headline, the live/upload split as its two columns, and the publish
/// button in its footer bar. The scene's name rides the page sub, not a kv
/// row, and the headline is the ONLY place the card names the target — the
/// old host pill duplicated it and is gone.
#[tokio::test]
async fn the_page_is_one_card_with_the_target_in_its_headline() {
    let _guard = crate::deploy::ENV_LOCK.lock().await;
    let (_dir, project) = scene("card", json!({ "display": { "title": "Gather" } }));
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

/// This page adds layout the shared sheet has no rule for, and that is the
/// whole licence it has: the moment it names a colour, a case or a
/// tracking of its own, `/deploy` starts looking like a different server
/// than `/`.
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

/// The destination is resolved the way the deploy itself resolves it: env
/// override first, then the world name onto the public worlds server,
/// then Genesis.
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
    assert!(env.server_line.contains("DCL_ONE_SDK_DEFAULT_TARGET"));
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

/// The worlds `/scenes` answer, in the shape the public server actually
/// returns: the scene on our parcels is the current one, the rest are the
/// neighbours `multi_scene: true` preserves.
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

/// The Genesis answer: the entity on the base parcel headlines, and any
/// other entity under the deploying pointers is named as replaced.
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

/// Each remote state renders one honest sentence or the kv rows — and the
/// preserved/replaced copy follows the destination, because the POST's
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
    let known = LiveStatus {
        remote: Remote::Known(RemoteState {
            current: Some(CurrentScene {
                title: "Gathering Stage".into(),
                timestamp: Some(deploy::now_ms() - 3 * 86_400_000),
                parcels: 4,
                coords: vec![(0, 0), (0, 1), (1, 0), (1, 1)],
                size: None,
            }),
            others: vec![RemoteScene {
                title: "Bazaar".into(),
                parcels: 3,
                coords: vec![(5, 5), (5, 6), (6, 5)],
                size: None,
            }],
            hashes: HashSet::new(),
        }),
        reuse: None,
    };
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

    let empty = LiveStatus {
        remote: Remote::Empty,
        reuse: None,
    };
    assert!(
        server_panel(&world, &empty).contains("Nothing is deployed here yet"),
        "{}",
        server_panel(&world, &empty)
    );

    let down = LiveStatus {
        remote: Remote::Unreachable(
            "could not reach worlds-content-server.decentraland.org".into(),
        ),
        reuse: None,
    };
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

/// The reuse split over synthetic hash sets: a file the server holds
/// transfers nothing, a file with no hash is an upload, and the bytes
/// follow the files.
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
    let (_dir, project) = scene("upline", json!({ "display": { "title": "Gather" } }));
    let p = deploy::preview(&project).unwrap();
    let with = LiveStatus {
        remote: Remote::Empty,
        reuse: Some(Reuse {
            reused_files: 2,
            reused_bytes: 2000,
            upload_files: 1,
            upload_bytes: 79,
        }),
    };
    let html = upload_panel(&p, &with);
    assert!(
        html.contains("2 of 3 files are already on the server"),
        "{html}"
    );
    assert!(html.contains("1 to upload (79 bytes)"), "{html}");

    let fresh = LiveStatus {
        remote: Remote::Empty,
        reuse: Some(Reuse {
            reused_files: 0,
            reused_bytes: 0,
            upload_files: 3,
            upload_bytes: 2079,
        }),
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

/// The local hashes are the publish-time CIDs: the same fixture bytes the
/// deploy golden test pins must hash to the same ids here, or the reuse
/// split would compare apples to invented oranges.
#[tokio::test]
async fn the_reuse_hashes_are_the_publish_time_cids() {
    let (_dir, project) = scene("cids", json!({ "display": { "title": "Gather" } }));
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

/// The page follows a run both ways: the `<noscript>` meta refresh rides
/// the Running state and only the Running state (the no-JS fallback), and
/// the region's `data-state`/`data-signing` are the shape the script compares
/// before swapping. A live signer renders the wallet panel inline — there is
/// no signing page of its own.
#[test]
fn the_run_region_marks_its_state_for_the_script_and_the_no_js_refresh() {
    const PANEL: &str =
        r#"<div class="panel" id="sign-panel" data-api="/t/abc/deploy/sign">…</div>"#;
    let mut run = Run {
        id: 1,
        started: Instant::now(),
        target: "https://worlds-content-server.decentraland.org".into(),
        signing: Some("/deploy".into()),
        auto: false,
        print: "abc123".into(),
        state: RunState::Running,
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

    run.state = RunState::Done;
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

/// The page ships exactly its own script and nothing else executable, the
/// same posture the landing page holds: no src, no `javascript:` url, no
/// inline handler, no alert — and the no-JS path keeps its POSTing form
/// with the token and the fingerprint.
#[tokio::test]
async fn the_page_ships_exactly_its_own_script_and_no_handlers() {
    let (_dir, project) = scene("script", json!({ "display": { "title": "Gather" } }));
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

/// The destination is said ONCE, in the card's header — the old Server
/// field restated it and is gone. A world deploy needs no flag, so the
/// terminal line is the bare command. `None` is passed for the default
/// target on purpose: reading the environment here would make this test
/// say something different on a machine that exports
/// `DCL_ONE_SDK_DEFAULT_TARGET`, which the README tells people to do.
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

/// The target page: one card, one sub-tab per destination shape, with the
/// world tab checked for a world scene and honest empty states elsewhere.
#[tokio::test]
async fn the_target_page_offers_the_four_destination_shapes() {
    let _guard = crate::deploy::ENV_LOCK.lock().await;
    let (_dir, project) = scene(
        "targetpg",
        json!({ "display": { "title": "Gather" }, "worldConfiguration": { "name": "my.dcl.eth" } }),
    );
    let st = state(project);
    let resp = target_page(&st, &HeaderMap::new()).await;
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
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
    for pane in [
        "tgt__pane--world",
        "tgt__pane--land",
        "tgt__pane--multi",
        "tgt__pane--history",
    ] {
        assert!(html.contains(pane), "missing {pane}: {html}");
    }
    assert!(
        html.contains(r#"<span class="knob__k">On the server now</span>"#)
            && html.contains(r#"<span class="knob__k">This upload</span>"#),
        "the live/upload split lives here now: {html}"
    );
    assert!(
        html.matches("<script>").count() == 1 && !html.contains("<script src"),
        "one inline script (wallet detection and connect-follow), nothing loaded: {html}"
    );
    assert!(
        html.contains("Point at Genesis City LAND"),
        "a world scene can step back to its parcels in one click: {html}"
    );
}

/// One signing endpoint, hosted by this server and gated like the POST that
/// starts a run. The panel it answers is server-rendered inline on `/deploy`
/// (and the landing page) once a run hands the signer over — a page-driven
/// publish involves exactly one server and no signing page of its own.
#[tokio::test]
async fn the_signing_endpoint_is_hosted_gated_and_pending_aware() {
    let _guard = crate::deploy::ENV_LOCK.lock().await;
    let (_dir, project) = scene("signhost", json!({ "display": { "title": "Gather" } }));
    let st = state(project);
    let lan: SocketAddr = ([192, 168, 1, 9], 40000).into();
    let local: SocketAddr = ([127, 0, 0, 1], 40000).into();

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
        ConnectInfo(lan),
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
        ConnectInfo(local),
        HeaderMap::new(),
        sig_req(),
    )
    .await;
    assert_eq!(
        idle.status(),
        StatusCode::NOT_FOUND,
        "an idle signer refuses instead of pretending"
    );

    let (state_arc, _rx) = crate::linker::new_state(crate::linker::LinkerDeploy {
        dir: std::env::temp_dir(),
        prepared: deploy::Prepared {
            files: vec![],
            pointers: vec!["0,0".into()],
            metadata: crate::jsjson::parse("{}").unwrap(),
        },
        target_content: "https://worlds-content-server.decentraland.org".into(),
        world: Some("w.dcl.eth".into()),
        needs_delete: false,
        timestamp_override: None,
        entity_out: None,
        scene_title: "Gather".into(),
        base_parcel: "0,0".into(),
        multi_scene: true,
        check_permissions: false,
    });
    adopt_cli_signing(&st, state_arc);
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
    let remote = route(State(st.clone()), ConnectInfo(lan), HeaderMap::new()).await;
    let remote = String::from_utf8_lossy(
        &axum::body::to_bytes(remote.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .into_owned();
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
/// that IS published — not the seventeen thousand files under a
/// node_modules the walk never enters.
#[tokio::test]
async fn ignored_names_the_files_you_excluded_not_the_tree_you_never_ship() {
    let (_dir, project) = scene("ignored", json!({ "display": { "title": "Gather" } }));
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
    let (_dir, project) = scene("totals", json!({ "display": { "title": "Gather" } }));
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
/// byte sum of the folded files (the only number that makes the panel add
/// up), and one click shows every row.
#[tokio::test]
async fn a_long_payload_lists_eight_and_folds_the_rest() {
    let (_dir, project) = scene("truncate", json!({ "display": { "title": "Gather" } }));
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

/// A file over the per-file limit is the deploy failing, and the panel
/// saying so has never rendered until this fixture existed.
#[tokio::test]
async fn an_oversize_file_gets_a_warning_before_the_wallet() {
    let (_dir, project) = scene("oversize", json!({ "display": { "title": "Gather" } }));
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

/// A publishable file whose size cannot be read is the deploy stopping
/// after the wallet has signed. Reporting it as 0 bytes made the page say
/// the deploy was fine.
#[cfg(unix)]
#[tokio::test]
async fn a_file_that_cannot_be_read_is_not_called_zero_bytes() {
    let (_dir, project) = scene("dangling", json!({ "display": { "title": "Gather" } }));
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

/// "You have not built yet" is the most likely reason a real deploy fails,
/// and `prepare` refuses it — after the wallet prompt.
#[tokio::test]
async fn a_scene_that_was_never_built_says_so() {
    let (_dir, project) = scene("unbuilt", json!({ "display": { "title": "Gather" } }));
    std::fs::remove_file(project.root.join("bin/index.js")).unwrap();
    let p = deploy::preview(&project).unwrap();
    assert_eq!(p.main, MainBundle::Missing("bin/index.js".to_string()));
    let html = served(&state(project)).await;
    assert!(html.contains("Not built yet"), "{html}");
    assert!(html.contains("dcl-one-sdk build"), "{html}");
    assert!(html.contains("bin/index.js"), "{html}");
    assert!(!html.contains("somebody"), "{html}");
}

/// The built scene must NOT carry the alarm, or the one above proves only
/// that the panel is always there. Asserted on the alarm titles rather
/// than the warn class, because the run-status panel is process-global and
/// a parallel test's failed run may legitimately wear it.
#[tokio::test]
async fn a_built_scene_carries_no_alarm() {
    let (_dir, project) = scene("built", json!({ "display": { "title": "Gather" } }));
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

/// The walk is the expensive half of this route and the route is
/// unauthenticated: the answer is reused for a moment rather than redone
/// per request.
#[tokio::test]
async fn the_walk_is_reused_for_a_moment() {
    let (_dir, project) = scene("cache", json!({ "display": { "title": "Gather" } }));
    let st = state(project.clone());
    let first = served(&st).await;
    std::fs::write(project.root.join("assets/late.glb"), vec![1u8; 4096]).unwrap();
    let resp = route(
        State(st.clone()),
        ConnectInfo(([127, 0, 0, 1], 0).into()),
        HeaderMap::new(),
    )
    .await;
    let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let cached = String::from_utf8(bytes.to_vec()).unwrap();
    assert_eq!(cached, first, "the second request did not walk again");
    assert!(!cached.contains("late.glb"));
    let fresh = served(&st).await;
    assert!(fresh.contains("late.glb"), "an expired entry walks again");
}

/// The error branch is a page too, served on the same open port, and it
/// is held to the same rule: say what is wrong, never say where.
#[tokio::test]
async fn the_error_page_names_the_pattern_and_not_the_path() {
    let (dir, project) = scene("badignore", json!({ "display": { "title": "Gather" } }));
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

/// Whatever an error chain from further down carries, the page does not
/// pass a filesystem path on to the visitor.
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

/// The address form is gated exactly like the publish POST — it only changes
/// what an open page renders, but a stranger on the LAN does not get to pick
/// whose holdings this preview looks up.
#[tokio::test]
async fn the_address_form_is_gated_like_the_publish_post() {
    let (_dir, project) = scene("addrgate", json!({ "display": { "title": "Gather" } }));
    let st = state(project);
    let lan: SocketAddr = ([192, 168, 1, 9], 51000).into();
    let local: SocketAddr = ([127, 0, 0, 1], 51000).into();
    const ADDR: &str = "0x1234567890abcdef1234567890abcdef12345678";
    let form = |token: &str, address: &str| {
        Form(AddressForm {
            token: token.to_string(),
            address: address.to_string(),
        })
    };

    let refused = target_address(
        State(st.clone()),
        ConnectInfo(lan),
        HeaderMap::new(),
        form(token(&st), ADDR),
    )
    .await;
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);

    let refused = target_address(
        State(st.clone()),
        ConnectInfo(local),
        HeaderMap::new(),
        form("wrong", ADDR),
    )
    .await;
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);

    let garbage = target_address(
        State(st.clone()),
        ConnectInfo(local),
        HeaderMap::new(),
        form(token(&st), "vitalik.eth"),
    )
    .await;
    assert_eq!(
        garbage.status(),
        StatusCode::UNPROCESSABLE_ENTITY,
        "a non-address never becomes a URL"
    );
    assert!(address_slot(&st).is_none());

    let set = target_address(
        State(st.clone()),
        ConnectInfo(local),
        HeaderMap::new(),
        form(token(&st), &ADDR.to_uppercase().replace("0X", "0x")),
    )
    .await;
    assert_eq!(set.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        address_slot(&st).as_deref(),
        Some(ADDR),
        "stored lowercased, the spelling every lookup uses"
    );

    let cleared = target_address(
        State(st.clone()),
        ConnectInfo(local),
        HeaderMap::new(),
        form(token(&st), "  "),
    )
    .await;
    assert_eq!(cleared.status(), StatusCode::SEE_OTHER);
    assert!(address_slot(&st).is_none(), "an empty submit forgets");
}

/// The account lives in the page bar: a split pill when none is known — the
/// browser wallet directly, or the Decentraland sign-in — and a pill naming
/// the account once one is. The card adds its own row only when it has
/// something to say. The dry-run seam keeps the checks an honest sentence
/// instead of a fetch.
#[tokio::test]
async fn the_bar_connects_an_account_and_the_page_checks_with_it() {
    let _guard = crate::deploy::ENV_LOCK.lock().await;
    let (_dir, project) = scene(
        "addrpage",
        json!({ "display": { "title": "Gather" }, "worldConfiguration": { "name": "my.dcl.eth" } }),
    );
    let st = state(project);
    let page = target_page(&st, &HeaderMap::new()).await;
    let html = String::from_utf8(
        axum::body::to_bytes(page.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
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

    const ADDR: &str = "0x1234567890abcdef1234567890abcdef12345678";
    *address_slot(&st) = Some(ADDR.to_string());
    let page = target_page(&st, &HeaderMap::new()).await;
    let html = String::from_utf8(
        axum::body::to_bytes(page.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
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
    let (_dir, project) = scene("connectgate", json!({ "display": { "title": "Gather" } }));
    let st = state(project);
    let lan: SocketAddr = ([192, 168, 1, 9], 51000).into();
    let local: SocketAddr = ([127, 0, 0, 1], 51000).into();
    let form = |token: &str| {
        Form(ConnectForm {
            token: token.to_string(),
        })
    };

    let refused = target_connect(
        State(st.clone()),
        ConnectInfo(lan),
        HeaderMap::new(),
        form(token(&st)),
    )
    .await;
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);

    let refused = target_connect(
        State(st.clone()),
        ConnectInfo(local),
        HeaderMap::new(),
        form("wrong"),
    )
    .await;
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);

    let off = target_connect(
        State(st.clone()),
        ConnectInfo(local),
        HeaderMap::new(),
        form(token(&st)),
    )
    .await;
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
    let (_dir, project) = scene("history", json!({ "display": { "title": "Gather" } }));
    let root = project.root.clone();
    let st = state(project.clone());
    let id =
        claim(&st, "World w.dcl.eth".into(), false, "abc123".into()).expect("nothing is running");
    finish(&st, id, RunState::Done);
    let id =
        claim(&st, "World w.dcl.eth".into(), false, "abc123".into()).expect("done is not running");
    finish(&st, id, RunState::Stale(vec![]));
    {
        let held = history_slot(&st);
        assert_eq!(held.len(), 2);
        assert_eq!(held[0].outcome, "nothing published", "newest first");
        assert_eq!(held[1].outcome, "published");
    }
    let pane = history_rows_pane(&history_rows(&st, &root));
    assert!(pane.contains("World w.dcl.eth"), "{pane}");
    assert!(pane.contains("published"), "{pane}");

    let fresh = state(project);
    let rows = history_rows(&fresh, &root);
    assert_eq!(rows.len(), 2, "the on-disk record outlives the process");
    assert_eq!(rows[0].outcome, "nothing published");
    assert!(
        history_rows_pane(&[]).contains("No deployments yet"),
        "no history is still a pane"
    );
}

/// The after-map: ours in the accent with the base marked, neighbours kept,
/// the replaced footprint dashed — and past the span cap, no grid at all
/// rather than an unreadable one.
#[test]
fn the_after_map_draws_kept_replaced_and_ours() {
    let state = RemoteState {
        current: Some(CurrentScene {
            title: "Old".into(),
            timestamp: None,
            parcels: 2,
            coords: vec![(0, 0), (1, 0)],
            size: None,
        }),
        others: vec![RemoteScene {
            title: "Bazaar".into(),
            parcels: 2,
            coords: vec![(2, 0), (2, 1)],
            size: None,
        }],
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
        others: vec![RemoteScene {
            title: "Far".into(),
            parcels: 2,
            coords: vec![(0, 0), (40, 40)],
            size: None,
        }],
        hashes: HashSet::new(),
    };
    assert_eq!(
        after_map(&sprawling, &ours, (0, 0)),
        "",
        "past the span cap the rows tell the story alone"
    );
}

/// The multiscene pane says what the world's scenes hold only from sizes the
/// server actually reported, and each row names its fate.
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
        reuse: Some(Reuse {
            reused_files: 0,
            reused_bytes: 0,
            upload_files: 1,
            upload_bytes: 500,
        }),
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

/// Pointing the scene rewrites the file a deploy actually reads: a world
/// name lands in worldConfiguration.name, an empty value strips it — and
/// the gates are the scene editors', never opened by --allow-remote-deploy.
#[tokio::test]
async fn pointing_the_scene_rewrites_its_destination_in_scene_json() {
    let (_dir, project) = scene("point", json!({ "display": { "title": "Gather" } }));
    let root = project.root.clone();
    let st = state(project);
    let local: SocketAddr = ([127, 0, 0, 1], 51000).into();
    let lan: SocketAddr = ([192, 168, 1, 9], 51000).into();
    let form = |token: &str, world: &str| {
        Form(PointForm {
            token: token.to_string(),
            world: world.to_string(),
        })
    };

    let refused = target_point(
        State(st.clone()),
        ConnectInfo(lan),
        HeaderMap::new(),
        form(token(&st), "w.dcl.eth"),
    )
    .await;
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);

    let garbage = target_point(
        State(st.clone()),
        ConnectInfo(local),
        HeaderMap::new(),
        form(token(&st), "javascript:alert(1)"),
    )
    .await;
    assert_eq!(
        garbage.status(),
        StatusCode::UNPROCESSABLE_ENTITY,
        "only a world name may enter the file"
    );

    let ok = target_point(
        State(st.clone()),
        ConnectInfo(local),
        HeaderMap::new(),
        form(token(&st), "Gather.DCL.eth"),
    )
    .await;
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

    let back = target_point(
        State(st.clone()),
        ConnectInfo(local),
        HeaderMap::new(),
        form(token(&st), "  "),
    )
    .await;
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
    use crate::start::deploy_rights::{Rights, Verdict, WorldRow};
    let dest = resolve_dest(
        &json!({ "worldConfiguration": { "name": "my.dcl.eth" } }),
        None,
        None,
    );
    let row = |name: &str| WorldRow {
        name: name.to_string(),
        scenes: Some(1),
        last_deployed: None,
        title: Some("My World".to_string()),
        owned: true,
    };
    let rights = Rights {
        address: "0x1234567890abcdef1234567890abcdef12345678".into(),
        verdict: Verdict::May("you own this name".into()),
        worlds: vec![row("my.dcl.eth"), row("other.dcl.eth")],
        worlds_note: None,
        holdings: None,
        parcel_rights: Vec::new(),
        unchecked_parcels: 0,
    };
    let html = your_worlds("/t", "tok", &dest, Some(&rights));
    let my = html.find("my.dcl.eth").unwrap();
    let other = html.find("other.dcl.eth").unwrap();
    assert!(
        html[..other].contains("current target") || html[my..].contains("current target"),
        "{html}"
    );
    assert_eq!(
        html.matches(r#"action="/t/target/point""#).count(),
        1,
        "one re-aim form, on the row that is not the target: {html}"
    );
    assert!(
        html.contains(r#"name="world" value="other.dcl.eth""#)
            && html.contains(">Point scene here</button>"),
        "{html}"
    );
}

/// The current target leads the list — it is the row the eye came for —
/// and the listing cap can never be the reason it is missing.
#[test]
fn the_current_target_leads_your_worlds() {
    use crate::start::deploy_rights::{Rights, Verdict, WorldRow};
    let dest = resolve_dest(
        &json!({ "worldConfiguration": { "name": "zz-last.dcl.eth" } }),
        None,
        None,
    );
    let row = |name: &str| WorldRow {
        name: name.to_string(),
        scenes: None,
        last_deployed: None,
        title: None,
        owned: true,
    };
    // Eleven other worlds ahead of it: alphabetical order alone would push
    // the target past the ten-row cap and off the page entirely.
    let mut worlds: Vec<WorldRow> = (0..11).map(|i| row(&format!("w{i:02}.dcl.eth"))).collect();
    worlds.push(row("zz-last.dcl.eth"));
    let rights = Rights {
        address: "0x1234567890abcdef1234567890abcdef12345678".into(),
        verdict: Verdict::May("you own this name".into()),
        worlds,
        worlds_note: None,
        holdings: None,
        parcel_rights: Vec::new(),
        unchecked_parcels: 0,
    };
    let html = your_worlds("/t", "tok", &dest, Some(&rights));
    let target = html.find("zz-last.dcl.eth").expect("the target is listed");
    let first_other = html.find("w00.dcl.eth").expect("others still listed");
    assert!(target < first_other, "the target leads: {html}");
    assert!(html.contains("current target"), "{html}");
}

/// An auto-started run nobody signed vanishes: no failure panel, no history
/// row — the page was opened and left, which is not an event. The same
/// timeout on a button-press run keeps its failure, because a person asked.
#[tokio::test]
async fn an_unsigned_auto_run_ends_quietly() {
    let (_dir, project) = scene("autoquiet", json!({ "display": { "title": "Gather" } }));
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

/// A live signer with no wallet answer yet, for run-slot tests that need one.
fn parked_signer() -> Arc<crate::linker::LinkerState> {
    let (state, _rx) = crate::linker::new_state(crate::linker::LinkerDeploy {
        dir: std::env::temp_dir(),
        prepared: deploy::Prepared {
            files: vec![],
            pointers: vec!["0,0".into()],
            metadata: crate::jsjson::parse("{}").unwrap(),
        },
        target_content: "https://worlds-content-server.decentraland.org".into(),
        world: Some("w.dcl.eth".into()),
        needs_delete: false,
        timestamp_override: None,
        entity_out: None,
        scene_title: "Gather".into(),
        base_parcel: "0,0".into(),
        multi_scene: true,
        check_permissions: false,
    });
    state
}

/// The wallet only ever signs the tree as it is: a pending run whose payload
/// moved after its build finished re-mints — new id, current fingerprint,
/// the signer slot emptied so the panel cannot route to the stale entity —
/// and keeps its provenance (an auto run re-mints auto).
#[tokio::test]
async fn a_drifted_pending_run_is_reminted_before_the_wallet_sees_it() {
    let (_dir, project) = scene("drift", json!({ "display": { "title": "Gather" } }));
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
/// running (two builds over one release tree can sign mid-write bytes), an
/// unchanged payload, a wallet that already answered (past recall), and an
/// adopted CLI signing (that publish belongs to a terminal).
#[tokio::test]
async fn the_remint_declines_matching_midbuild_cli_and_signed_runs() {
    let (_dir, project) = scene("driftno", json!({ "display": { "title": "Gather" } }));
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

/// A superseded deploy's tail cannot touch the newer run: its timeout writes
/// no state onto a slot it no longer owns, wipes no signer the newer run
/// registered, and records no history.
#[tokio::test]
async fn a_superseded_deploy_cannot_touch_the_newer_run() {
    let (_dir, project) = scene("driftsup", json!({ "display": { "title": "Gather" } }));
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
/// with "checking" placeholders and the warming mark the reload script keys
/// on — a connected wallet reads "checking", never "you have nothing" — and
/// because every fetch outcome is a cached value, failures included, the
/// reloads converge on a final render with no mark in bounded time.
#[tokio::test]
async fn a_cold_cache_renders_now_and_the_reloads_converge() {
    let (_dir, project) = scene("warming", json!({ "display": { "title": "Gather" } }));
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

    // The spawned warm-ups fail fast against the dead port and cache their
    // failure sentences; the "reload" (a second call) lands on them.
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

/// A delegated identity is used while it is live and dropped the moment it
/// lapses: `live_identity` hands back an unexpired one and forgets an
/// expired one, so a deploy signs itself for the hour and falls back to the
/// wallet after.
#[test]
fn a_delegated_identity_is_used_while_live_and_dropped_when_it_lapses() {
    let (_dir, project) = scene("identity", json!({ "display": { "title": "Gather" } }));
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
/// the signing routes, honest about a dry run, and never an answer for a
/// string that is not an address.
#[tokio::test]
async fn the_preflight_answers_before_the_wallet_signs() {
    let (_dir, project) = scene("preflight", json!({ "display": { "title": "Gather" } }));
    let st = state(project);
    let lan: SocketAddr = ([192, 168, 1, 9], 40000).into();
    let local: SocketAddr = ([127, 0, 0, 1], 40000).into();
    let req =
        |addr: &str| -> Result<axum::Json<PreflightReq>, axum::extract::rejection::JsonRejection> {
            Ok(axum::Json(
                serde_json::from_value(json!({ "address": addr })).unwrap(),
            ))
        };

    let blocked = preflight(
        State(st.clone()),
        ConnectInfo(lan),
        HeaderMap::new(),
        req("0x1234567890abcdef1234567890abcdef12345678"),
    )
    .await;
    assert_eq!(
        blocked.status(),
        StatusCode::FORBIDDEN,
        "the check runs where the signature runs"
    );

    let garbage = preflight(
        State(st.clone()),
        ConnectInfo(local),
        HeaderMap::new(),
        req("vitalik.eth"),
    )
    .await;
    assert_eq!(garbage.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let off = preflight(
        State(st.clone()),
        ConnectInfo(local),
        HeaderMap::new(),
        req("0x1234567890abcdef1234567890abcdef12345678"),
    )
    .await;
    assert_eq!(off.status(), StatusCode::OK);
    let body = axum::body::to_bytes(off.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        v["verdict"], "unchecked",
        "a dry run says so instead of guessing: {v}"
    );
    assert!(
        SCRIPT.contains("/preflight") && SCRIPT.contains("may_not"),
        "the sign flow asks before personal_sign and stops on a refusal"
    );

    // A pending run with an explicit target the oracle has no authority
    // over — a self-hosted node with its own deploy policy — gets a shrug,
    // never a mainnet-keyed refusal that would block a server about to
    // say yes.
    let dep = |target: &str| crate::linker::LinkerDeploy {
        dir: std::env::temp_dir(),
        prepared: deploy::Prepared {
            files: vec![],
            pointers: vec!["0,0".into()],
            metadata: crate::jsjson::parse("{}").unwrap(),
        },
        target_content: target.into(),
        world: None,
        needs_delete: false,
        timestamp_override: None,
        entity_out: None,
        scene_title: "Gather".into(),
        base_parcel: "0,0".into(),
        multi_scene: true,
        check_permissions: false,
    };
    let (foreign, _rx) = crate::linker::new_state(dep("https://my-node.example.com/content"));
    *signer_slot(&st) = Some(foreign);
    let shrug = preflight(
        State(st.clone()),
        ConnectInfo(local),
        HeaderMap::new(),
        req("0x1234567890abcdef1234567890abcdef12345678"),
    )
    .await;
    let body = axum::body::to_bytes(shrug.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["verdict"], "unchecked", "{v}");
    assert!(
        v["why"].as_str().unwrap().contains("my-node.example.com"),
        "the shrug names the server that decides: {v}"
    );

    let (upstream, _rx) = crate::linker::new_state(dep("https://interconnected.online/content"));
    *signer_slot(&st) = Some(upstream);
    let judged = preflight(
        State(st.clone()),
        ConnectInfo(local),
        HeaderMap::new(),
        req("0x1234567890abcdef1234567890abcdef12345678"),
    )
    .await;
    let body = axum::body::to_bytes(judged.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        v["why"], "live checks are off for this run",
        "a Genesis-network target is the oracle's to judge (the dry-run seam answers here): {v}"
    );
    *signer_slot(&st) = None;
}

#[test]
fn translate_footprint_moves_the_whole_shape() {
    let mut scene = serde_json::json!({
        "scene": { "parcels": ["0,0", "1,0", "0,1"], "base": "0,0" }
    });
    translate_footprint(&mut scene, (20, -30)).unwrap();
    assert_eq!(scene["scene"]["base"], serde_json::json!("20,-30"));
    assert_eq!(
        scene["scene"]["parcels"],
        serde_json::json!(["20,-30", "21,-30", "20,-29"]),
        "every parcel rides the same delta, shape intact"
    );
}

#[test]
fn translate_footprint_refuses_to_leave_the_genesis_map() {
    let mut scene = serde_json::json!({
        "scene": { "parcels": ["0,0", "5,0"], "base": "0,0" }
    });
    let why = translate_footprint(&mut scene, (160, 0)).unwrap_err();
    assert!(
        why.contains("165,0"),
        "the error names the parcel that falls off the map: {why}"
    );
    assert_eq!(
        scene["scene"]["parcels"],
        serde_json::json!(["0,0", "5,0"]),
        "a refused move changes nothing"
    );
    translate_footprint(&mut scene, (158, 163)).unwrap();
    assert_eq!(
        scene["scene"]["base"],
        serde_json::json!("158,163"),
        "the expansion districts past 150 are still the map"
    );
}

#[test]
fn translate_footprint_same_base_is_a_no_op() {
    let mut scene = serde_json::json!({
        "scene": { "parcels": ["3,4"], "base": "3,4" }
    });
    translate_footprint(&mut scene, (3, 4)).unwrap();
    assert_eq!(scene["scene"]["parcels"], serde_json::json!(["3,4"]));
}
