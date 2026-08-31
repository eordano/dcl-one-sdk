use super::http::{build_scene_entity, entities_for, project_for, scene_id_for};
use super::*;
use axum::extract::{Path as AxPath, State};
use axum::http::StatusCode;
use axum::Json;
use serde_json::json;

struct Tmp(PathBuf);

impl Tmp {
    fn new(tag: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("dcl-one-sdk-startws-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Tmp(dir)
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn member(tmp: &Tmp, name: &str, parcels: &[&str]) -> Project {
    let root = tmp.0.join(name);
    std::fs::create_dir_all(root.join("bin")).unwrap();
    std::fs::write(root.join("bin/index.js"), "module.exports={}").unwrap();
    let scene_json = json!({
        "main": "bin/index.js",
        "runtimeVersion": "7",
        "scene": { "parcels": parcels, "base": parcels[0] }
    });
    std::fs::write(root.join("scene.json"), scene_json.to_string()).unwrap();
    Project {
        root: root.canonicalize().unwrap(),
        scene_json,
    }
}

#[test]
fn a_deep_link_never_carries_both_ab_forms() {
    let sidecar = || Some("http://127.0.0.1:5147".to_string());
    assert_eq!(banner_ab_url(true, sidecar()), None);
    assert_eq!(banner_ab_url(true, None), None);
    assert_eq!(banner_ab_url(false, None), None);
    assert_eq!(banner_ab_url(false, sidecar()), sidecar());

    assert_eq!(
        joinblock::deep_link_extra(true, false, None, &[]),
        "&local-ab=true"
    );
    assert_eq!(joinblock::deep_link_extra(false, false, None, &[]), "");
}

fn state(projects: Vec<Project>) -> AppState {
    testkit::state(projects)
}

#[test]
fn entities_union_serves_every_member() {
    let tmp = Tmp::new("union");
    let a = member(&tmp, "scene-a", &["0,0"]);
    let b = member(&tmp, "scene-b", &["1,0", "1,1"]);
    let st = state(vec![a.clone(), b.clone()]);
    let all = entities_for(&st, &[]);
    assert_eq!(all.len(), 2);
    assert_eq!(all[0]["id"], json!(scene_id_for(&a, "test-machine")));
    assert_eq!(all[1]["id"], json!(scene_id_for(&b, "test-machine")));
    assert_eq!(all[1]["pointers"], json!(["1,0", "1,1"]));
}

#[test]
fn entities_filter_by_pointer_returns_only_matches() {
    let tmp = Tmp::new("filter");
    let a = member(&tmp, "scene-a", &["0,0"]);
    let b = member(&tmp, "scene-b", &["1,0"]);
    let st = state(vec![a, b.clone()]);
    let hit = entities_for(&st, &["1,0".to_string()]);
    assert_eq!(hit.len(), 1);
    assert_eq!(hit[0]["id"], json!(scene_id_for(&b, "test-machine")));
    let both = entities_for(&st, &["0,0".to_string(), "1,0".to_string()]);
    assert_eq!(both.len(), 2);
    let miss = entities_for(&st, &["9,9".to_string()]);
    assert!(miss.is_empty());
}

#[test]
fn project_for_maps_paths_to_the_owning_member() {
    let tmp = Tmp::new("owner");
    let a = member(&tmp, "scene-a", &["0,0"]);
    let b = member(&tmp, "scene-b", &["1,0"]);
    let st = state(vec![a.clone(), b.clone()]);
    let inside_b = b.root.join("bin/index.js");
    assert_eq!(project_for(&st, &inside_b).unwrap().root, b.root);
    assert_eq!(project_for(&st, &a.root).unwrap().root, a.root);
    let outside = tmp.0.canonicalize().unwrap();
    assert!(project_for(&st, &outside).is_none());
}

#[tokio::test]
async fn about_honors_x_forwarded_proto_host_prefix() {
    let tmp = Tmp::new("fwd");
    let a = member(&tmp, "scene-a", &["0,0"]);
    let mut st = state(vec![a]);
    st.offline_comms = false;
    let req = axum::extract::Request::builder()
        .uri("/about")
        .header("host", "127.0.0.1:8000")
        .header("x-forwarded-proto", "https")
        .header("x-forwarded-host", "tunnel.example")
        .header("x-forwarded-prefix", "/t/abc123defg/")
        .body(axum::body::Body::empty())
        .unwrap();
    let Json(v) = about(State(Arc::new(st)), req).await;
    assert_eq!(
        v["comms"]["fixedAdapter"],
        json!("ws-room:wss://tunnel.example/t/abc123defg/mini-comms/room-1")
    );
    assert_eq!(
        v["content"]["publicUrl"],
        json!("https://tunnel.example/t/abc123defg/content")
    );
    assert_eq!(
        v["lambdas"]["publicUrl"],
        json!("https://tunnel.example/t/abc123defg/lambdas")
    );
    assert!(v["configurations"]["scenesUrn"][0]
        .as_str()
        .unwrap()
        .contains("baseUrl=https://tunnel.example/t/abc123defg/content/contents/"));
}

#[tokio::test]
async fn about_without_forwarding_headers_stays_plain_http() {
    let tmp = Tmp::new("nofwd");
    let a = member(&tmp, "scene-a", &["0,0"]);
    let mut st = state(vec![a]);
    st.offline_comms = false;
    let req = axum::extract::Request::builder()
        .uri("/about")
        .header("host", "10.1.2.20:8000")
        .body(axum::body::Body::empty())
        .unwrap();
    let Json(v) = about(State(Arc::new(st)), req).await;
    assert_eq!(
        v["comms"]["fixedAdapter"],
        json!("ws-room:ws://10.1.2.20:8000/mini-comms/room-1")
    );
    assert_eq!(
        v["content"]["publicUrl"],
        json!("http://10.1.2.20:8000/content")
    );
}

#[tokio::test]
async fn root_redirect_honors_forwarded_prefix() {
    let tmp = Tmp::new("redir");
    let a = member(&tmp, "scene-a", &["0,0"]);
    let st = Arc::new(state(vec![a]));
    let req = axum::extract::Request::builder()
        .uri("/")
        .header("x-forwarded-prefix", "/t/abc123defg")
        .body(axum::body::Body::empty())
        .unwrap();
    let resp = root(State(st.clone()), req).await;
    assert_eq!(
        resp.headers().get(header::LOCATION).unwrap(),
        "/t/abc123defg/about"
    );
    let req = axum::extract::Request::builder()
        .uri("/")
        .body(axum::body::Body::empty())
        .unwrap();
    let resp = root(State(st), req).await;
    assert_eq!(resp.headers().get(header::LOCATION).unwrap(), "/about");
}

/// The landing page exactly as a browser receives it: through `root`, with
/// the headers and the query string a visitor would send. Tests that
/// rebuild the page's strings for themselves prove nothing about the page.
async fn scene_body(st: &Arc<AppState>, headers: &[(&str, &str)]) -> String {
    let mut h = HeaderMap::new();
    for (k, v) in headers {
        h.insert(
            axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
            v.parse().unwrap(),
        );
    }
    let resp = super::landing::scene_page(st, &h);
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

async fn landing_body(st: &Arc<AppState>, uri: &str, headers: &[(&str, &str)]) -> String {
    let mut req = axum::extract::Request::builder()
        .uri(uri)
        .header("accept", "text/html,application/xhtml+xml");
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let resp = root(
        State(st.clone()),
        req.body(axum::body::Body::empty()).unwrap(),
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
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8(body.to_vec()).unwrap()
}

/// `/deploy` holds the landing page's script posture: at most its own one
/// inline script, nothing loaded from anywhere, no `javascript:` url and
/// no inline handler beside the button that publishes.
fn assert_no_javascript(body: &str) {
    let body = body.to_lowercase();
    assert!(
        body.matches("<script").count() <= 1,
        "at most the page's own inline script"
    );
    assert!(
        !body.contains("<script src"),
        "no script loads from anywhere"
    );
    assert!(!body.contains("javascript:"), "no javascript: url");
    let posts = body.matches(r#"method="post""#).count();
    if posts > 0 {
        assert!(
            posts <= 2,
            "the publish button and the bar's connect, nothing more"
        );
        assert!(
            body.matches(r#"name="token""#).count() >= posts,
            "every POST carries the token, or it would be forgeable cross-origin"
        );
        assert!(
            body.contains(r#"name="fingerprint""#),
            "a POST without the payload fingerprint could publish something \
             other than what the page showed"
        );
    }
    assert_no_inline_handlers(&body);
}

/// The landing page ships exactly its own two scripts — the `#edit-data`
/// JSON blob and the inline editor — and nothing else executable: no third
/// script however hostile the input, nothing loaded from anywhere, no
/// `javascript:` url, no inline handler, and exactly one POSTing form (the
/// bar's connect button; the scene's mutations stay fetches).
fn assert_only_the_landing_scripts(body: &str) {
    let lower = body.to_lowercase();
    assert_eq!(
        lower.matches("<script").count(),
        2,
        "the data blob and the editor script, nothing more"
    );
    assert!(body.contains(r#"<script type="application/json" id="edit-data">"#));
    assert!(
        !lower.contains("<script src"),
        "no script loads from anywhere"
    );
    assert!(!lower.contains("javascript:"), "no javascript: url");
    assert_eq!(
        lower.matches(r#"method="post""#).count(),
        1,
        "the bar's connect button is this page's one form POST; the scene's \
         mutations stay fetches to the gated routes"
    );
    assert_no_inline_handlers(&lower);
}

fn assert_no_inline_handlers(lower: &str) {
    for (attr, _) in lower.match_indices(" on") {
        let rest = &lower[attr + 3..];
        let name_len = rest
            .find(|c: char| !c.is_ascii_alphabetic())
            .unwrap_or(rest.len());
        assert!(
            !(name_len > 0 && rest[name_len..].starts_with('=')),
            "inline event handler: on{}",
            &rest[..name_len]
        );
    }
}

/// A form on any page here must be a GET aimed back at the page that drew
/// it: a GET carries no side effect, so a page you happen to have open
/// still cannot make this server do anything, which is what the blanket
/// no-form rule used to buy. `action` is the whole expected attribute
/// value, so behind a proxy it has to carry the forwarded prefix. The one
/// standing exception is the bar's connect button — a POST, but aimed at a
/// route gated like the publish button and carrying the page token, so an
/// open page still cannot make this server act cross-origin.
fn assert_forms_are_gets(body: &str, action: &str, count: usize) {
    let mut connects = 0;
    for form in body.match_indices("<form").map(|(i, _)| &body[i..]) {
        let tag = &form[..form.find('>').expect("unterminated <form")];
        if tag.contains("/target/connect") || tag.contains("/target/address") {
            connects += 1;
            assert!(
                tag.contains(r#"method="post""#),
                "the bar's form is the connect (or disconnect) POST and nothing else: {tag}"
            );
            let scope = &form[..form.find("</form>").unwrap_or(form.len())];
            assert!(
                scope.contains(r#"name="token""#),
                "the connect POST carries the token: {tag}"
            );
            continue;
        }
        assert!(
            tag.contains(r#"method="get""#),
            "every form must be a GET: {tag}"
        );
        assert!(
            tag.contains(&format!(r#"action="{action}""#)),
            "a form may only target this page (expected {action}): {tag}"
        );
    }
    assert!(connects <= 1, "one connect form at most");
    assert_eq!(
        body.matches("<form").count() - connects,
        count,
        "the page draws exactly {count} form(s) beside the bar's connect"
    );
}

#[tokio::test]
async fn root_serves_a_landing_page_to_browsers() {
    let tmp = Tmp::new("landing");
    let a = member(&tmp, "scene-a", &["0,0"]);
    let st = Arc::new(state(vec![a]));
    let body = landing_body(&st, "/", &[("host", "127.0.0.1:8000")]).await;
    assert!(body.contains("dcl-one-sdk"));
    assert!(body.contains("decentraland://realm=http%3A%2F%2F127.0.0.1%3A8000"));
    assert_eq!(
        body.matches(r#"id="launch""#).count(),
        1,
        "one launch button, for the selected target"
    );
    assert!(
        !body.contains("https://decentraland.org/bevy-web/"),
        "the web target is not the selected one"
    );
    let web = landing_body(&st, "/?where=web", &[("host", "127.0.0.1:8000")]).await;
    assert!(web.contains(
        "https://decentraland.org/bevy-web/?preview=true&amp;realm=http://127.0.0.1:8000"
    ));
    assert!(
        !web.contains("decentraland://realm="),
        "and then the desktop card is the one that is gone"
    );
    assert_eq!(web.matches(r#"id="launch""#).count(), 1);

    let must = |needle: &str| {
        body.find(needle)
            .unwrap_or_else(|| panic!("missing {needle}"))
    };
    assert!(
        must(r#"class="pgnav""#) < must(r#"id="join""#),
        "the section nav leads the page"
    );
    assert_eq!(
        body.matches(r#"class="pgnav__lnk""#).count(),
        4,
        "Preview, Scene, Target, Deploy: {body}"
    );
    assert!(
        body.contains(r#"href="/" aria-current="page">Preview"#),
        "the current section is marked: {body}"
    );
    assert!(
        !body.contains(r#"id="scene-layout""#),
        "the layout card lives on /scene now"
    );
    assert!(
        must(r#"id="requests""#) > must(r#"id="join""#),
        "the request log is the last section"
    );

    let sc = scene_body(&st, &[("host", "127.0.0.1:8000")]).await;
    let smust = |needle: &str| {
        sc.find(needle)
            .unwrap_or_else(|| panic!("missing on /scene: {needle}"))
    };
    let layout = smust(r#"<div class="jn lay lay--info" id="scene-layout">"#);
    assert!(
        layout < smust(r#"data-laytab="info""#)
            && smust(r#"data-laytab="info""#) < smust(r#"data-laytab="parcels""#)
            && smust(r#"data-laytab="parcels""#) < smust(r#"data-laytab="spawns""#)
            && smust(r#"data-laytab="spawns""#) < smust(r#"data-laytab="perms""#),
        "one layout card, four tabs in order"
    );
    assert!(
        smust(r#"id="lay-grid""#) < smust(r#"data-pane="info""#),
        "the shared grid pane sits left of the rail panes"
    );
    assert!(
        sc.contains(r#"<h1 class="scene__title" id="edit-title""#),
        "the visible title is the h1, and it is the title editor: {sc}"
    );
    assert!(
        sc.contains(r#"href="/scene" aria-current="page">Scene"#),
        "the scene page marks its own nav entry: {sc}"
    );
    assert!(
        !body.contains(r#"id="deploy""#),
        "and the landing page no longer carries a deploy section"
    );
    assert!(
        !body.contains("--dir"),
        "the page never names the scene path"
    );
    for page in [
        "/about",
        "/scenes",
        "/scene.json",
        "/preview-wearables",
        "/deploy",
    ] {
        assert!(body.contains(&format!(r#"href="{page}""#)), "{page} linked");
    }
    assert_forms_are_gets(&body, "/", 1);
    let form = body
        .find(r#"<form class="side""#)
        .expect("the knob form (the bar's connect form is not it)");
    let end = body[form..].find("</form>").expect("unterminated form") + form;
    for knob in [r#"name="where""#, r#"name="mcp""#, r#"name="opt""#] {
        let at = body.find(knob).unwrap_or_else(|| panic!("missing {knob}"));
        assert!(at > form && at < end, "{knob} must ride inside the form");
    }
    for gone in [
        "comms ws-room",
        "abgen ready",
        "bar__realm",
        ">setup<",
        ">server<",
        "snapshot",
        "/inspector/",
        "class=\"foot\"",
    ] {
        assert!(!body.contains(gone), "{gone} should be gone");
    }
    assert!(!body.contains(&format!("dcl-one-sdk {}", env!("CARGO_PKG_VERSION"))));
}

#[tokio::test]
async fn landing_page_honors_forwarded_headers_and_escapes_titles() {
    let tmp = Tmp::new("landing-fwd");
    let root_dir = tmp.0.join("scene-x");
    std::fs::create_dir_all(root_dir.join("bin")).unwrap();
    std::fs::write(root_dir.join("bin/index.js"), "module.exports={}").unwrap();
    let scene_json = json!({
        "main": "bin/index.js",
        "display": { "title": "a <script> title" },
        "scene": { "parcels": ["0,0"], "base": "0,0" }
    });
    std::fs::write(root_dir.join("scene.json"), scene_json.to_string()).unwrap();
    let project = Project {
        root: root_dir.canonicalize().unwrap(),
        scene_json,
    };
    let st = Arc::new(state(vec![project]));
    let fwd = [
        ("host", "127.0.0.1:8000"),
        ("x-forwarded-proto", "https"),
        ("x-forwarded-host", "tunnel.example"),
        ("x-forwarded-prefix", "/t/abc123defg/"),
    ];
    let body = landing_body(&st, "/", &fwd).await;
    assert!(body.contains("https%3A%2F%2Ftunnel.example%2Ft%2Fabc123defg"));
    assert!(!body.contains("a <script> title"));
    assert!(body.contains("a &lt;script&gt; title"));
    let web = landing_body(&st, "/?where=web", &fwd).await;
    assert!(web.contains("https://tunnel.example/t/abc123defg"));
    assert!(!web.contains("a <script> title"));

    assert!(
        body.contains(r#"<a class="pgnav__lnk" href="/t/abc123defg/deploy""#),
        "the section nav keeps the forwarded prefix"
    );
    assert_forms_are_gets(&body, "/t/abc123defg/", 1);
    for page in ["/about", "/scenes", "/scene.json", "/deploy"] {
        assert!(
            body.contains(&format!(r#"href="/t/abc123defg{page}""#)),
            "{page} keeps the forwarded prefix"
        );
    }
    assert!(
        !body.contains(r#"href="/deploy""#),
        "and no link is left pointing at the unprefixed root"
    );
}

/// The landing page ships its editor script, and that is the whole
/// JavaScript budget: however hostile the query string, nothing else
/// executable may appear — not a third script, not a handler attribute,
/// not a `javascript:` href — because everything reflected into the page
/// is either allowlisted or escaped.
#[tokio::test]
async fn the_landing_page_ships_only_its_own_script() {
    let tmp = Tmp::new("landing-nojs");
    let root_dir = tmp.0.join("scene-x");
    std::fs::create_dir_all(root_dir.join("bin")).unwrap();
    std::fs::write(root_dir.join("bin/index.js"), "module.exports={}").unwrap();
    let scene_json = json!({
        "main": "bin/index.js",
        "display": { "title": "Plain" },
        "scene": { "parcels": ["0,0"], "base": "0,0" },
        "requiredPermissions": ["USE_FETCH"],
        "spawnPoints": [{ "name": "spawn", "default": true,
                          "position": { "x": 8, "y": 0, "z": 8 } }]
    });
    std::fs::write(root_dir.join("scene.json"), scene_json.to_string()).unwrap();
    let project = Project {
        root: root_dir.canonicalize().unwrap(),
        scene_json,
    };
    let st = Arc::new(state(vec![project]));
    let body = landing_body(&st, "/", &[("host", "127.0.0.1:8000")]).await;
    assert_only_the_landing_scripts(&body);
    assert_forms_are_gets(&body, "/", 1);

    let hostile = concat!(
        "/?spawn=%3Cscript%3Ealert%281%29%3C%2Fscript%3E",
        "&opt=%22+onload%3Dalert%281%29",
        "&where=%22%3E%3Cimg+src%3Dx+onerror%3Dalert%281%29%3E",
        "&mcp=javascript%3Aalert%281%29",
        "&args=--gatekeeper-url%3Dhttps%3A%2F%2Fevil.example",
        "&%3Cscript%3E=%3Cscript%3E",
    );
    let poisoned = landing_body(&st, hostile, &[("host", "127.0.0.1:8000")]).await;
    assert_only_the_landing_scripts(&poisoned);
    assert_forms_are_gets(&poisoned, "/", 1);
    for smuggled in ["alert(1)", "gatekeeper-url", "evil.example", "onerror"] {
        assert!(
            !poisoned.contains(smuggled),
            "attacker-chosen {smuggled:?} reached the page"
        );
    }
    assert!(poisoned.contains(r#"id="launch""#));
    assert!(poisoned.contains(r#"name="spawn""#), "this scene names one");
}

/// The request log is the one part of this page built out of strings a
/// stranger chose: any LAN or tunnel peer puts one in the buffer just by
/// asking for it. Every `AppState` constructor here starts the buffer
/// empty, so until this test `id="requests"` was asserted present while
/// every row was the empty string and the `esc()` on the way in was never
/// once executed.
#[tokio::test]
async fn the_request_log_replays_a_hostile_path_escaped_and_newest_first() {
    let tmp = Tmp::new("reqlog");
    let a = member(&tmp, "scene-a", &["0,0"]);
    let st = Arc::new(state(vec![a]));
    let method = axum::http::Method::GET;
    record_request(&st, log_line(&method, "/<script>alert(1)</script>"), 404);
    record_request(&st, log_line(&method, "/newest.glb"), 200);

    let body = landing_body(&st, "/", &[("host", "127.0.0.1:8000")]).await;
    assert!(
        body.contains("GET /&lt;script&gt;alert(1)&lt;/script&gt;"),
        "the path is replayed escaped"
    );
    assert_only_the_landing_scripts(&body);
    assert!(
        body.find("/newest.glb").unwrap() < body.find("&lt;script&gt;").unwrap(),
        "newest first, so the log reads as a tail of what just happened"
    );
    assert!(
        body.contains("<summary>Recent requests \u{b7} 2</summary>"),
        "the count is the buffer's, plain text on the drawer that holds it: {body}"
    );
    assert!(
        body.contains(r#"<b class="st st--warn">404</b>"#)
            && body.contains(r#"<b class="st st--ok">200</b>"#),
        "each row carries its own status tone"
    );
}

/// The buffer bounds the section, so the eviction is the only thing
/// keeping a long-running preview's page from growing without limit.
#[test]
fn the_request_log_forgets_the_oldest_past_its_cap() {
    let tmp = Tmp::new("reqcap");
    let a = member(&tmp, "scene-a", &["0,0"]);
    let st = state(vec![a]);
    let method = axum::http::Method::GET;
    for i in 0..RECENT_REQUESTS_CAP + 5 {
        record_request(&st, log_line(&method, &format!("/{i}.glb")), 200);
    }
    let recent = st.recent_requests.lock().unwrap();
    assert_eq!(recent.len(), RECENT_REQUESTS_CAP);
    assert_eq!(
        recent.front().unwrap().0,
        "GET /5.glb",
        "the oldest five go"
    );
    assert_eq!(
        recent.back().unwrap().0,
        format!("GET /{}.glb", RECENT_REQUESTS_CAP + 4)
    );
}

/// A path is attacker-chosen, unbounded, retained, and re-rendered into
/// every response. Cut it on the way in rather than on the way out, so the
/// unbounded version is never held at all.
#[test]
fn a_path_too_long_to_be_a_path_is_cut_before_it_is_kept() {
    let method = axum::http::Method::GET;
    assert_eq!(
        log_line(&method, "/about"),
        "GET /about",
        "short paths whole"
    );

    let long = format!("/{}", "a".repeat(7000));
    let line = log_line(&method, &long);
    assert!(
        line.len() < MAX_LOGGED_PATH + 8,
        "a 7000-character path is not retained: kept {} bytes",
        line.len()
    );
    assert!(line.starts_with("GET /aaa") && line.ends_with('\u{2026}'));

    let wide = format!("/{}", "\u{2764}".repeat(2000));
    let cut = log_line(&method, &wide);
    assert!(cut.ends_with('\u{2026}'));
    assert!(
        cut.trim_start_matches("GET /")
            .trim_end_matches('\u{2026}')
            .chars()
            .all(|c| c == '\u{2764}'),
        "no replacement or split character: {cut}"
    );
}

/// The whole app on a real socket, so a test can ask it the way a browser
/// does — through the routing table and the access-log layer, instead of
/// reaching past both to call one handler.
async fn serve(st: &Arc<AppState>) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let app = build_router(st.clone(), Arc::new(crate::comms::CommsState::default()));
    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await;
    });
    (addr, handle)
}

/// `/deploy` is registered in the router `start` builds, and every page
/// this server draws carries a header button pointing at it — but no test
/// ever fetched it, so deleting the route would have left the suite green
/// behind a button that 404s. Fetched here through the real router, not by
/// calling the handler, because the registration is the untested half.
/// The note has to distinguish a push that landed from one that went
/// nowhere: with no client attached, `broadcast::send` reports zero
/// receivers, and a developer staring at "reload issued" while nothing
/// moves is the exact confusion this line exists to prevent.
#[test]
fn the_reload_note_says_whether_anything_received_it() {
    assert_eq!(reload_note(1), "reload issued");
    assert_eq!(reload_note(3), "reload issued to 3 clients");
    let none = reload_note(0);
    assert!(none.contains("no client connected"), "{none}");
    assert!(!none.contains("reload issued"), "{none}");
}

/// A model save is not a targeted refetch, whatever the frame implies:
/// the client routes UpdateModel and UpdateScene alike into
/// TryReloadSceneAsync. Both events must therefore report a reload, and
/// this pins the pair so a future "only the asset reloaded" claim has to
/// change a test rather than just the copy.
#[tokio::test]
async fn both_a_scene_and_a_model_change_report_a_reload() {
    let tmp = Tmp::new("reloadnote");
    let project = member(&tmp, "scene-a", &["0,0"]);
    let root = project.root.clone();
    let st = Arc::new(state(vec![project]));
    let (tx, _keep) = broadcast::channel::<ReloadFrame>(8);
    let mut rx = tx.subscribe();

    for event in [
        ReloadEvent::Scene,
        ReloadEvent::Model {
            path: root.join("assets/spiral.glb"),
            removed: false,
        },
    ] {
        notify_reload(&root, "scene-a", &st, &tx, event);
        assert!(rx.try_recv().is_ok(), "text frame");
        assert!(rx.try_recv().is_ok(), "binary frame");
    }
}

#[tokio::test]
async fn deploy_is_a_page_this_server_actually_serves() {
    let tmp = Tmp::new("deployroute");
    let a = member(&tmp, "scene-a", &["0,0"]);
    let scene_dir = a.root.display().to_string();
    let st = Arc::new(state(vec![a]));
    let (addr, server) = serve(&st).await;
    let resp = reqwest::Client::new()
        .get(format!("http://{addr}/deploy"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200, "the header button leads here");
    assert!(resp
        .headers()
        .get(header::CONTENT_TYPE)
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("text/html"));
    let body = resp.text().await.unwrap();
    server.abort();
    assert!(
        body.contains("dcl-one-sdk deploy"),
        "the page still prints the command, for anyone who wants to run it"
    );
    assert!(
        body.contains(r#"method="post""#) && body.contains(r#"name="token""#),
        "the publish button is live for a loopback caller"
    );
    assert_no_javascript(&body);
    assert!(
        !body.contains(&scene_dir) && !body.contains("deployroute"),
        "the page never names the scene directory"
    );
}

/// The publish gates, driven through the real router. Each of the three is
/// asserted by breaking it and watching the POST be refused — a deploy
/// that ran here would build and sign a scene, so these are the tests that
/// keep an unauthenticated port from being a publish button.
#[tokio::test]
async fn publishing_refuses_a_forged_or_stale_post() {
    let tmp = Tmp::new("deploypost");
    let a = member(&tmp, "scene-a", &["0,0"]);
    let st = Arc::new(state(vec![a]));
    let (addr, server) = serve(&st).await;
    let client = reqwest::Client::new();

    let page = client
        .get(format!("http://{addr}/deploy"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let field = |name: &str| {
        let at = page
            .find(&format!(r#"name="{name}" value=""#))
            .unwrap_or_else(|| panic!("no {name} field on the page"));
        let from = page[at..].find("value=\"").unwrap() + at + 7;
        page[from..][..page[from..].find('"').unwrap()].to_string()
    };
    let token = field("token");
    let print = field("fingerprint");
    assert!(!token.is_empty() && !print.is_empty());

    let poster = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let post = |form: Vec<(&'static str, String)>| {
        let c = poster.clone();
        async move {
            c.post(format!("http://{addr}/deploy"))
                .form(&form)
                .send()
                .await
                .unwrap()
                .status()
                .as_u16()
        }
    };

    assert_eq!(
        post(vec![
            ("token", "not-the-token".to_string()),
            ("fingerprint", print.clone()),
        ])
        .await,
        403,
        "a forged token must not start a deploy"
    );

    assert_eq!(
        post(vec![
            ("token", token.clone()),
            ("fingerprint", "0000stale0000".to_string()),
        ])
        .await,
        303,
        "a stale fingerprint redirects rather than publishing"
    );
    let after = client
        .get(format!("http://{addr}/deploy"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        after.contains("Nothing was published"),
        "and it says so: {after:.400}"
    );
    server.abort();
}

/// The editors' write path end to end, through the real router: a
/// same-origin POST from loopback rewrites scene.json on disk, and every
/// read surface — the landing page, `/scene.json`, `/about`'s parcel list
/// — serves the edit at once, because the state behind them was updated
/// too, not just the file.
#[tokio::test]
async fn a_scene_edit_reaches_disk_state_and_every_page() {
    let tmp = Tmp::new("editflow");
    let a = member(&tmp, "scene-a", &["0,0"]);
    let root = a.root.clone();
    let st = Arc::new(state(vec![a]));
    let (addr, server) = serve(&st).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("http://{addr}/scene-json"))
        .json(&json!({
            "title": "Renamed Stage",
            "description": "Now with a description.",
            "tags": ["events", "theatre"],
            "parcels": ["0,0", "1,0"],
        }))
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    let echoed: Value = resp.json().await.unwrap();
    assert_eq!(status, 200, "{echoed}");
    assert_eq!(echoed["display"]["title"], json!("Renamed Stage"));

    let disk: Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("scene.json")).unwrap()).unwrap();
    assert_eq!(disk["display"]["title"], json!("Renamed Stage"));
    assert_eq!(disk["scene"]["parcels"], json!(["0,0", "1,0"]));
    assert_eq!(
        disk["main"],
        json!("bin/index.js"),
        "untouched keys survive"
    );

    let page = client
        .get(format!("http://{addr}/scene"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(page.contains("Renamed Stage"));
    assert!(page.contains("2 parcels"));
    assert!(page.contains("events"));

    let served: Value = client
        .get(format!("http://{addr}/scene.json"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(served["display"]["title"], json!("Renamed Stage"));

    let about: Value = client
        .get(format!("http://{addr}/about"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        about["configurations"]["localSceneParcels"],
        json!(["0,0", "1,0"]),
        "the realm advertises the new layout"
    );
    server.abort();
}

/// Every way a stranger's page could aim a POST at the editor, refused
/// before it touches disk: a cross-origin `Origin`, a cross-site
/// `Sec-Fetch-Site`, a tunnel replay, and a body field the page never
/// drew an editor for.
#[tokio::test]
async fn a_forged_scene_edit_never_touches_disk() {
    let tmp = Tmp::new("editforge");
    let a = member(&tmp, "scene-a", &["0,0"]);
    let root = a.root.clone();
    let before = std::fs::read_to_string(root.join("scene.json")).unwrap();
    let st = Arc::new(state(vec![a]));
    let (addr, server) = serve(&st).await;
    let client = reqwest::Client::new();
    let post = |headers: Vec<(&'static str, &'static str)>, body: Value| {
        let client = client.clone();
        let url = format!("http://{addr}/scene-json");
        async move {
            let mut req = client.post(url).json(&body);
            for (k, v) in headers {
                req = req.header(k, v);
            }
            req.send().await.unwrap().status().as_u16()
        }
    };

    let title = json!({ "title": "Forged" });
    assert_eq!(
        post(vec![("origin", "https://evil.example")], title.clone()).await,
        403,
        "a cross-origin POST is refused"
    );
    assert_eq!(
        post(vec![("sec-fetch-site", "cross-site")], title.clone()).await,
        403
    );
    assert_eq!(
        post(vec![(crate::tunnel::FORWARDED_HEADER, "1")], title.clone()).await,
        403,
        "a tunnel replay arrives on a loopback socket and is still refused"
    );
    assert_eq!(
        post(vec![], json!({ "realm": "https://evil.example" })).await,
        422,
        "a field the page has no editor for is refused, not ignored"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("scene.json")).unwrap(),
        before,
        "nothing above touched the file"
    );

    let own: &'static str = format!("http://{addr}").leak();
    assert_eq!(
        post(vec![("origin", own)], title).await,
        200,
        "the page's own origin is the one that works"
    );
    server.abort();
}

/// The thumbnail route writes the image where scene.json will find it,
/// repoints `display.navmapThumbnail`, and refuses bytes that are not the
/// image type they claim.
#[tokio::test]
async fn a_thumbnail_upload_lands_in_the_scene_and_scene_json() {
    let tmp = Tmp::new("editthumb");
    let a = member(&tmp, "scene-a", &["0,0"]);
    let root = a.root.clone();
    let st = Arc::new(state(vec![a]));
    let (addr, server) = serve(&st).await;
    let client = reqwest::Client::new();

    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend_from_slice(&[0u8; 24]);
    let resp = client
        .post(format!("http://{addr}/scene-thumbnail"))
        .header("content-type", "image/png")
        .body(png.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let answer: Value = resp.json().await.unwrap();
    assert_eq!(answer["navmapThumbnail"], json!("scene-thumbnail.png"));
    assert_eq!(
        std::fs::read(root.join("scene-thumbnail.png")).unwrap(),
        png
    );
    let disk: Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("scene.json")).unwrap()).unwrap();
    assert_eq!(
        disk["display"]["navmapThumbnail"],
        json!("scene-thumbnail.png")
    );

    let forged = client
        .post(format!("http://{addr}/scene-thumbnail"))
        .header("content-type", "image/png")
        .body(b"GIF89a not a png".to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(
        forged.status().as_u16(),
        415,
        "the magic bytes decide, not the header"
    );
    server.abort();
}

/// `access_log` is a layer, so every test that calls a handler directly
/// walks straight past it. Drive the real server and look at what it
/// retained: the buffer is unauthenticated, attacker-writable state that
/// every page re-renders.
#[tokio::test]
async fn the_access_log_keeps_one_bounded_line_per_request() {
    let tmp = Tmp::new("accesslog");
    let a = member(&tmp, "scene-a", &["0,0"]);
    let st = Arc::new(state(vec![a]));
    let (addr, server) = serve(&st).await;
    let client = reqwest::Client::new();
    let long = format!("/{}", "a".repeat(4000));
    client.get(format!("http://{addr}{long}")).send().await.ok();
    client.get(format!("http://{addr}/about")).send().await.ok();
    server.abort();

    let recent: Vec<(String, u16)> = st
        .recent_requests
        .lock()
        .unwrap()
        .iter()
        .map(|(line, status, _)| (line.clone(), *status))
        .collect();
    assert_eq!(recent.len(), 2, "one line per request: {recent:?}");
    assert!(
        recent[0].0.len() < MAX_LOGGED_PATH + 8,
        "a 4000-character path must not be retained whole, kept {} bytes",
        recent[0].0.len()
    );
    assert!(recent[0].0.ends_with('\u{2026}'), "{}", recent[0].0);
    assert_eq!(recent[0].1, 404, "and the status it really got");
    assert_eq!(recent[1], ("GET /about".to_string(), 200));
}

/// `--mcp` is on by default and its default port is 8123, so
/// `start --port 8123` aimed the scene-log poller at this very server: a
/// 404 POST to itself several times a second, which fills a 200-entry
/// request log in about seven minutes and buries every real client.
#[test]
fn the_scene_log_reader_never_polls_this_server() {
    let mcp = crate::joinblock::DEFAULT_EXPLORER_MCP_PORT;
    assert_eq!(
        scene_log_port(true, mcp, 8000),
        Some(mcp),
        "the normal case"
    );
    assert_eq!(
        scene_log_port(true, mcp, mcp),
        None,
        "start --port {mcp} must not start a poller aimed at itself"
    );
    assert_eq!(scene_log_port(false, mcp, 8000), None, "--no-mcp");
    assert_eq!(scene_log_port(false, mcp, mcp), None);

    let said = ux::render(&mcp_port_clash(mcp).into(), false, false);
    assert!(said.contains(&format!("--mcp-port {mcp}")), "{said}");
    assert!(said.contains("--mcp-port 8124"), "a way out: {said}");
    assert!(said.contains("--no-mcp"), "and a way to silence it: {said}");
}

fn state_with_data_layer(public_dir: PathBuf) -> AppState {
    let (_tx, port_rx) = tokio::sync::watch::channel(1234u16);
    std::mem::forget(_tx);
    let mut st = testkit::state(vec![]);
    st.data_layer = Some(DataLayerState {
        port_rx,
        public_dir: Some(public_dir),
    });
    st
}

#[tokio::test]
async fn contents_refuses_dclignored_files() {
    let tmp = Tmp::new("dclignore");
    let a = member(&tmp, "scene-a", &["0,0"]);
    std::fs::write(a.root.join("package.json"), "{\"secret\":\"key\"}").unwrap();
    let st = Arc::new(state(vec![a.clone()]));

    let pub_hash = b64_hash(
        &a.root.join("bin/index.js").display().to_string(),
        "test-machine",
    );
    let ok = contents(
        axum::http::Method::GET,
        State(st.clone()),
        AxPath(pub_hash),
        HeaderMap::new(),
    )
    .await;
    assert_eq!(ok.status(), StatusCode::OK);

    let ignored_hash = b64_hash(
        &a.root.join("package.json").display().to_string(),
        "test-machine",
    );
    let refused = contents(
        axum::http::Method::GET,
        State(st),
        AxPath(ignored_hash),
        HeaderMap::new(),
    )
    .await;
    assert_eq!(
        refused.status(),
        StatusCode::NOT_FOUND,
        "a .dclignored file must not be byte-served via /content/contents"
    );
}

#[tokio::test]
async fn inspector_asset_refuses_absolute_and_dotdot_paths() {
    let tmp = Tmp::new("inspector");
    let public = tmp.0.join("public");
    std::fs::create_dir_all(&public).unwrap();
    std::fs::write(public.join("app.js"), "console.log(1)").unwrap();
    let secret = tmp.0.join("secret.txt");
    std::fs::write(&secret, "top secret").unwrap();
    let st = Arc::new(state_with_data_layer(public.clone()));

    let ok = inspector_asset(
        State(st.clone()),
        AxPath("app.js".to_string()),
        HeaderMap::new(),
    )
    .await;
    assert_eq!(ok.status(), StatusCode::OK);

    let abs = secret.canonicalize().unwrap().display().to_string();
    let escaped = inspector_asset(State(st.clone()), AxPath(abs), HeaderMap::new()).await;
    assert_eq!(
        escaped.status(),
        StatusCode::NOT_FOUND,
        "an absolute path outside public_dir must be refused"
    );

    let dotdot = inspector_asset(
        State(st),
        AxPath("../secret.txt".to_string()),
        HeaderMap::new(),
    )
    .await;
    assert_eq!(dotdot.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn inspector_serves_the_gzipped_bundle_both_ways() {
    use std::io::Write;
    let tmp = Tmp::new("inspector-gz");
    let public = tmp.0.join("public");
    std::fs::create_dir_all(&public).unwrap();
    let plain = b"globalThis.InspectorConfig\n".repeat(40);
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    enc.write_all(&plain).unwrap();
    std::fs::write(public.join("bundle.js.gz"), enc.finish().unwrap()).unwrap();
    let st = Arc::new(state_with_data_layer(public.clone()));

    let mut gz_headers = HeaderMap::new();
    gz_headers.insert(header::ACCEPT_ENCODING, "gzip, deflate".parse().unwrap());
    let compressed = inspector_asset(
        State(st.clone()),
        AxPath("bundle.js".to_string()),
        gz_headers,
    )
    .await;
    assert_eq!(compressed.status(), StatusCode::OK);
    assert_eq!(
        compressed.headers().get(header::CONTENT_ENCODING).unwrap(),
        "gzip"
    );
    assert_eq!(
        compressed.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/javascript",
        "the mime must come from the request path, not the .gz on disk"
    );
    let body = axum::body::to_bytes(compressed.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(body.len() < plain.len());
    assert_eq!(crate::data_layer::gunzip(&body).unwrap(), plain);

    let expanded =
        inspector_asset(State(st), AxPath("bundle.js".to_string()), HeaderMap::new()).await;
    assert_eq!(expanded.status(), StatusCode::OK);
    assert!(expanded.headers().get(header::CONTENT_ENCODING).is_none());
    let body = axum::body::to_bytes(expanded.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body.as_ref(), plain.as_slice());
}

#[test]
fn data_layer_origin_gate_allows_same_origin_and_native_rejects_cross() {
    let empty = HeaderMap::new();
    assert!(
        data_layer_origin_allowed(&empty),
        "native clients send no Origin"
    );

    let mut null_origin = HeaderMap::new();
    null_origin.insert(header::ORIGIN, "null".parse().unwrap());
    assert!(
        data_layer_origin_allowed(&null_origin),
        "null Origin is native"
    );

    let mut same = HeaderMap::new();
    same.insert(header::HOST, "127.0.0.1:8000".parse().unwrap());
    same.insert(header::ORIGIN, "http://127.0.0.1:8000".parse().unwrap());
    assert!(data_layer_origin_allowed(&same));

    let mut fwd = HeaderMap::new();
    fwd.insert("x-forwarded-host", "tunnel.example".parse().unwrap());
    fwd.insert(header::HOST, "127.0.0.1:8000".parse().unwrap());
    fwd.insert(header::ORIGIN, "https://tunnel.example".parse().unwrap());
    assert!(data_layer_origin_allowed(&fwd), "same-origin behind nginx");

    let mut cross = HeaderMap::new();
    cross.insert(header::HOST, "127.0.0.1:8000".parse().unwrap());
    cross.insert(header::ORIGIN, "https://evil.example".parse().unwrap());
    assert!(!data_layer_origin_allowed(&cross));
}

#[test]
fn scene_entity_content_hashes_are_member_scoped() {
    let tmp = Tmp::new("entity");
    let b = member(&tmp, "scene-b", &["1,0"]);
    let entity = build_scene_entity(&b, "test-machine");
    let content = entity["content"].as_array().unwrap();
    assert!(content.iter().any(|c| {
        c["file"] == json!("bin/index.js")
            && c["hash"]
                == json!(crate::scene::b64_content_hash(
                    &b.root.join("bin/index.js").display().to_string(),
                    "test-machine"
                ))
    }));
}
