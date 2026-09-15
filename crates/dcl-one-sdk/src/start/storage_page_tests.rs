//! The storage routes and page, called as the router would call them: a
//! loopback peer, same-origin headers, a temp scene whose `.dcl-one` holds
//! the database.

use super::storage_page::{self as sp, SOURCE_HEADER};
use super::testkit::{body_text, scene, state, Tmp};
use super::AppState;
use crate::storage::{self, Scope, Target};
use axum::body::Bytes;
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode, Uri};
use axum::response::Response;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

const LOCAL: SocketAddr =
    SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), 41000);
const REMOTE: SocketAddr = SocketAddr::new(
    std::net::IpAddr::V4(std::net::Ipv4Addr::new(10, 0, 0, 9)),
    41000,
);

fn headers(source: &str) -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert(header::HOST, HeaderValue::from_static("127.0.0.1:8000"));
    h.insert(SOURCE_HEADER, HeaderValue::from_str(source).unwrap());
    h
}

fn confirm(mut h: HeaderMap) -> HeaderMap {
    h.insert("x-confirm-delete-all", HeaderValue::from_static("true"));
    h
}

fn uri(s: &str) -> Uri {
    s.parse().unwrap()
}

/// Keys travel percent-encoded in the path, as the browser and the host send them.
fn enc(s: &str) -> String {
    crate::deploy::encode_segment(s)
}

fn st(tmp: &Tmp) -> Arc<AppState> {
    Arc::new(state(vec![scene(&tmp.0, "shop", &["1,2"], "")]))
}

fn root(st: &AppState) -> std::path::PathBuf {
    st.first_project().unwrap().root
}

async fn json_of(resp: Response) -> (u16, Value) {
    let status = resp.status().as_u16();
    let text = body_text(resp).await;
    let value = serde_json::from_str(&text).unwrap_or(Value::String(text));
    (status, value)
}

async fn put_scene(st: &Arc<AppState>, key: &str, body: &str, source: &str) -> Response {
    sp::scene_put(
        State(st.clone()),
        ConnectInfo(LOCAL),
        headers(source),
        uri(&format!("/values/{}", enc(key))),
        Path(key.to_string()),
        Bytes::from(body.to_string()),
    )
    .await
}

async fn get_scene(st: &Arc<AppState>, key: &str, peer: SocketAddr) -> Response {
    sp::scene_get(
        State(st.clone()),
        ConnectInfo(peer),
        headers("test"),
        uri(&format!("/values/{}", enc(key))),
        Path(key.to_string()),
    )
    .await
}

fn page_query(query: &str) -> sp::PageQuery {
    let get = |name: &str| {
        query
            .split('&')
            .find_map(|pair| pair.strip_prefix(&format!("{name}=")))
            .map(str::to_string)
    };
    sp::PageQuery {
        tab: get("tab"),
        player: get("player"),
    }
}

fn list_query(query: &str) -> sp::ListQuery {
    let get = |name: &str| {
        query
            .split('&')
            .find_map(|pair| pair.strip_prefix(&format!("{name}=")))
            .map(str::to_string)
    };
    sp::ListQuery {
        prefix: get("prefix"),
        limit: get("limit").map(|n| n.parse().unwrap()),
        offset: get("offset").map(|n| n.parse().unwrap()),
    }
}

/// `token=..&upstream=on&service=..&url=..&tab=..` as the browser posts it.
fn target_form(form: &str) -> sp::TargetForm {
    let get = |name: &str| {
        form.split('&')
            .find_map(|pair| pair.strip_prefix(&format!("{name}=")))
            .map(|v| v.replace("%3A", ":").replace("%2F", "/"))
    };
    sp::TargetForm {
        token: get("token").unwrap_or_default(),
        upstream: get("upstream"),
        service: get("service"),
        url: get("url"),
        tab: get("tab"),
    }
}

async fn page(st: &Arc<AppState>, query: &str) -> String {
    let q = page_query(query);
    body_text(
        sp::page(
            State(st.clone()),
            ConnectInfo(LOCAL),
            headers("test"),
            Query(q),
        )
        .await,
    )
    .await
}

#[tokio::test]
async fn scene_values_round_trip_over_upstreams_routes() {
    let tmp = Tmp::new("sto-scene");
    let st = st(&tmp);
    let (status, body) =
        json_of(put_scene(&st, "score", r#"{"value":{"a":[1,true]}}"#, "scene").await).await;
    assert_eq!(
        (status, body),
        (200, json!({ "value": { "a": [1, true] } }))
    );
    let (status, body) = json_of(get_scene(&st, "score", LOCAL).await).await;
    assert_eq!(
        (status, body),
        (200, json!({ "value": { "a": [1, true] } }))
    );
    let (status, body) = json_of(get_scene(&st, "missing", LOCAL).await).await;
    assert_eq!(status, 404);
    assert_eq!(body["error"], "Not Found");
    assert_eq!(body["message"], "Value not found");
    put_scene(&st, "scratch", r#"{"value":7}"#, "scene").await;
    put_scene(&st, "second", r#"{"value":"two"}"#, "cli").await;
    let list = |q: &str| {
        let st = st.clone();
        let q = list_query(q);
        async move {
            json_of(
                sp::scene_list(
                    State(st),
                    ConnectInfo(LOCAL),
                    headers("test"),
                    uri("/values"),
                    Query(q),
                )
                .await,
            )
            .await
        }
    };
    let (status, body) = list("prefix=sc&limit=1&offset=1").await;
    assert_eq!(status, 200);
    assert_eq!(
        body,
        json!({ "data": [{ "key": "scratch", "value": 7 }], "pagination": { "limit": 1, "offset": 1, "total": 2 } })
    );
    let all = list("").await.1;
    assert_eq!(
        all["pagination"],
        json!({ "limit": 100, "offset": 0, "total": 3 })
    );
    assert_eq!(
        list("limit=500").await.1["pagination"]["limit"],
        100,
        "the limit is clamped, as the service clamps it"
    );
    let del = sp::scene_delete(
        State(st.clone()),
        ConnectInfo(LOCAL),
        headers("ui"),
        uri("/values/scratch"),
        Path("scratch".to_string()),
    )
    .await;
    assert_eq!(del.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        get_scene(&st, "scratch", LOCAL).await.status(),
        StatusCode::NOT_FOUND
    );
    let clear = |h: HeaderMap| {
        let st = st.clone();
        async move { sp::scene_clear(State(st), ConnectInfo(LOCAL), h, uri("/values")).await }
    };
    let (status, body) = json_of(clear(headers("cli")).await).await;
    assert_eq!(status, 400, "a clear needs the confirm header");
    assert_eq!(body["error"], "Bad request");
    assert_eq!(
        body["message"],
        "Missing required header: X-Confirm-Delete-All"
    );
    assert_eq!(
        clear(confirm(headers("cli"))).await.status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(list("").await.1["pagination"]["total"], 0);
    let db = storage::open(&root(&st)).unwrap();
    let log = db.activity(10).unwrap();
    assert_eq!(
        (log[0].op.as_str(), log[0].source.as_str()),
        ("clear", "cli")
    );
    assert!(log.iter().any(|a| a.source == "scene" && a.key == "score"));
}

#[tokio::test]
async fn player_values_are_keyed_by_lowercase_address_and_players_are_listed() {
    let tmp = Tmp::new("sto-player");
    let st = st(&tmp);
    let put = sp::player_put(
        State(st.clone()),
        ConnectInfo(LOCAL),
        headers("scene"),
        uri("/players/0xABCDEFabcdefABCDEFabcdefABCDEFabcdefABCD/values/score"),
        Path((
            "0xABCDEFabcdefABCDEFabcdefABCDEFabcdefABCD".to_string(),
            "score".to_string(),
        )),
        Bytes::from(r#"{"value":3}"#),
    )
    .await;
    assert_eq!(put.status(), StatusCode::OK);
    let (status, body) = json_of(
        sp::player_get(
            State(st.clone()),
            ConnectInfo(LOCAL),
            headers("test"),
            uri("/players/0xabcdefabcdefabcdefabcdefabcdefabcdefabcd/values/score"),
            Path((
                "0xabcdefabcdefabcdefabcdefabcdefabcdefabcd".to_string(),
                "score".to_string(),
            )),
        )
        .await,
    )
    .await;
    assert_eq!((status, body), (200, json!({ "value": 3 })));
    let (status, body) = json_of(
        sp::players(
            State(st.clone()),
            ConnectInfo(LOCAL),
            headers("test"),
            uri("/players"),
            Query(sp::ListQuery::default()),
        )
        .await,
    )
    .await;
    assert_eq!(
        (status, body),
        (
            200,
            json!({ "data": ["0xabcdefabcdefabcdefabcdefabcdefabcdefabcd"], "pagination": { "limit": 100, "offset": 0, "total": 1 } })
        ),
        "the production shape: address strings"
    );
    let (status, body) = json_of(
        sp::player_list(
            State(st.clone()),
            ConnectInfo(LOCAL),
            headers("test"),
            uri("/players/0xABCDEFabcdefABCDEFabcdefABCDEFabcdefABCD/values"),
            Path("0xABCDEFabcdefABCDEFabcdefABCDEFabcdefABCD".to_string()),
            Query(sp::ListQuery::default()),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["data"][0]["key"], "score");
    let (status, body) = json_of(
        sp::player_get(
            State(st.clone()),
            ConnectInfo(LOCAL),
            headers("test"),
            uri("/players/0xabc/values/score"),
            Path(("0xabc".to_string(), "score".to_string())),
        )
        .await,
    )
    .await;
    assert_eq!(
        (status, body),
        (
            400,
            json!({ "error": "Bad request", "message": "Invalid player address" })
        )
    );
    let (status, body) = json_of(
        sp::usage_player(
            State(st.clone()),
            ConnectInfo(LOCAL),
            headers("test"),
            uri("/usage/players/0xabcdefabcdefabcdefabcdefabcdefabcdefabcd"),
            Path("0xabcdefabcdefabcdefabcdefabcdefabcdefabcd".to_string()),
        )
        .await,
    )
    .await;
    assert_eq!(
        (status, body),
        (
            200,
            json!({ "usedBytes": 1, "maxTotalSizeBytes": storage::MAX_PLAYER_TOTAL_BYTES })
        )
    );
    let clear = sp::player_clear(
        State(st.clone()),
        ConnectInfo(LOCAL),
        confirm(headers("ui")),
        uri("/players/0xabcdefabcdefabcdefabcdefabcdefabcdefabcd/values"),
        Path("0xabcdefabcdefabcdefabcdefabcdefabcdefabcd".to_string()),
    )
    .await;
    assert_eq!(clear.status(), StatusCode::NO_CONTENT);
    assert!(storage::open(&root(&st))
        .unwrap()
        .players()
        .unwrap()
        .is_empty());
    for address in [
        "0x1111111111111111111111111111111111111111",
        "0x2222222222222222222222222222222222222222",
    ] {
        sp::player_put(
            State(st.clone()),
            ConnectInfo(LOCAL),
            headers("scene"),
            uri(&format!("/players/{address}/values/k")),
            Path((address.to_string(), "k".to_string())),
            Bytes::from(r#"{"value":true}"#),
        )
        .await;
    }
    let clear_all = |h: HeaderMap| {
        let st = st.clone();
        async move { sp::players_clear(State(st), ConnectInfo(LOCAL), h, uri("/players")).await }
    };
    assert_eq!(
        clear_all(headers("ui")).await.status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        clear_all(confirm(headers("ui"))).await.status(),
        StatusCode::NO_CONTENT,
        "DELETE /players clears every player"
    );
    let db = storage::open(&root(&st)).unwrap();
    assert!(db.players().unwrap().is_empty());
    assert_eq!(db.activity(1).unwrap()[0].scope, "players");
}

#[tokio::test]
async fn env_reads_overlay_dotenv_and_writes_must_be_strings() {
    let tmp = Tmp::new("sto-env");
    let st = st(&tmp);
    std::fs::write(root(&st).join(".env"), "API=file\nOTHER=x\n").unwrap();
    let get = |key: &str| {
        let st = st.clone();
        let key = key.to_string();
        async move {
            json_of(
                sp::env_get(
                    State(st),
                    ConnectInfo(LOCAL),
                    headers("scene"),
                    uri(&format!("/env/{}", enc(&key))),
                    Path(key),
                )
                .await,
            )
            .await
        }
    };
    assert_eq!(get("API").await, (200, json!({ "value": "file" })));
    assert_eq!(get("NOPE").await.0, 404);
    let put = |body: &str| {
        let st = st.clone();
        let body = body.to_string();
        async move {
            sp::env_put(
                State(st),
                ConnectInfo(LOCAL),
                headers("cli"),
                uri("/env/API"),
                Path("API".to_string()),
                Bytes::from(body),
            )
            .await
        }
    };
    assert_eq!(
        put(r#"{"value":"runtime"}"#).await.status(),
        StatusCode::NO_CONTENT
    );
    let (status, body) = json_of(put(r#"{"value":1}"#).await).await;
    assert_eq!(status, 400);
    assert_eq!(body["message"], "Environment variables are strings");
    assert_eq!(get("API").await, (200, json!({ "value": "runtime" })));
    let (status, body) = json_of(
        sp::env_list(
            State(st.clone()),
            ConnectInfo(LOCAL),
            headers("test"),
            uri("/env"),
            Query(sp::ListQuery::default()),
        )
        .await,
    )
    .await;
    assert_eq!(
        (status, body),
        (
            200,
            json!({ "data": ["API", "OTHER"], "pagination": { "limit": 100, "offset": 0, "total": 2 } })
        ),
        "the production shape: the names of runtime and .env variables alike"
    );
    let (status, body) = json_of(
        sp::usage_env(
            State(st.clone()),
            ConnectInfo(LOCAL),
            headers("test"),
            uri("/usage/env"),
        )
        .await,
    )
    .await;
    assert_eq!(
        (status, body),
        (
            200,
            json!({ "usedBytes": 9, "maxTotalSizeBytes": storage::MAX_ENV_TOTAL_BYTES })
        ),
        "the runtime override's JSON text; .env is the scene's file, not storage"
    );
    let (status, body) = json_of(
        put(&format!(
            r#"{{"value":"{}"}}"#,
            "x".repeat(storage::MAX_ENV_VALUE_BYTES)
        ))
        .await,
    )
    .await;
    assert_eq!(status, 400);
    assert_eq!(
        body["message"],
        format!(
            "Value size ({} bytes) exceeds the maximum allowed size ({} bytes)",
            storage::MAX_ENV_VALUE_BYTES + 2,
            storage::MAX_ENV_VALUE_BYTES
        )
    );
    let del = sp::env_delete(
        State(st.clone()),
        ConnectInfo(LOCAL),
        headers("ui"),
        uri("/env/API"),
        Path("API".to_string()),
    )
    .await;
    assert_eq!(del.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        get("API").await,
        (200, json!({ "value": "file" })),
        ".env shows through again"
    );
    let clear = sp::env_clear(
        State(st.clone()),
        ConnectInfo(LOCAL),
        confirm(headers("ui")),
        uri("/env"),
    )
    .await;
    assert_eq!(clear.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn writes_are_refused_off_machine_and_cross_origin_while_reads_stay_open() {
    let tmp = Tmp::new("sto-gate");
    let st = st(&tmp);
    let remote = sp::scene_put(
        State(st.clone()),
        ConnectInfo(REMOTE),
        headers("scene"),
        uri("/values/k"),
        Path("k".to_string()),
        Bytes::from(r#"{"value":1}"#),
    )
    .await;
    let (status, body) = json_of(remote).await;
    assert_eq!(status, 403);
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .contains("machine hosting"),
        "{body}"
    );
    let mut foreign = headers("ui");
    foreign.insert(
        header::ORIGIN,
        HeaderValue::from_static("http://evil.example"),
    );
    let cross = sp::scene_put(
        State(st.clone()),
        ConnectInfo(LOCAL),
        foreign,
        uri("/values/k"),
        Path("k".to_string()),
        Bytes::from(r#"{"value":1}"#),
    )
    .await;
    assert_eq!(cross.status(), StatusCode::FORBIDDEN);
    put_scene(&st, "k", r#"{"value":1}"#, "scene").await;
    assert_eq!(
        get_scene(&st, "k", REMOTE).await.status(),
        StatusCode::OK,
        "local reads are open"
    );
    storage::open(&root(&st))
        .unwrap()
        .set_target(&Target::Zone)
        .unwrap();
    assert_eq!(
        get_scene(&st, "k", REMOTE).await.status(),
        StatusCode::FORBIDDEN,
        "a service target signs as the developer, so even reads stay on the machine"
    );
}

#[tokio::test]
async fn bodies_must_carry_a_value_and_keys_and_sizes_are_bounded() {
    let tmp = Tmp::new("sto-body");
    let st = st(&tmp);
    let (status, body) = json_of(put_scene(&st, "k", "[]", "scene").await).await;
    assert_eq!(status, 400);
    assert!(
        body["message"].as_str().unwrap().contains("\"value\""),
        "{body}"
    );
    let (status, _) = json_of(put_scene(&st, "k", "not json", "scene").await).await;
    assert_eq!(status, 400);
    let huge = format!(
        r#"{{"value":"{}"}}"#,
        "x".repeat(storage::MAX_WORLD_VALUE_BYTES)
    );
    let (status, body) = json_of(put_scene(&st, "k", &huge, "scene").await).await;
    assert_eq!(
        (status, body["message"].as_str().unwrap_or_default()),
        (
            400,
            format!(
                "Value size ({} bytes) exceeds the maximum allowed size ({} bytes)",
                storage::MAX_WORLD_VALUE_BYTES + 2,
                storage::MAX_WORLD_VALUE_BYTES
            )
            .as_str()
        ),
        "the service's rule, in its words"
    );
    let (status, body) =
        json_of(put_scene(&st, "nul", r#"{"value":"a\u0000b"}"#, "scene").await).await;
    assert_eq!(status, 400);
    assert_eq!(
        body["message"],
        "Values must not contain the \\u0000 (NUL) character"
    );
    let (status, body) =
        json_of(put_scene(&st, &"k".repeat(256), r#"{"value":1}"#, "scene").await).await;
    assert_eq!(status, 400);
    assert_eq!(body["message"], "Key must be between 1 and 255 characters");
    assert_eq!(
        get_scene(&st, "k", LOCAL).await.status(),
        StatusCode::NOT_FOUND,
        "nothing was stored"
    );
}

#[tokio::test]
async fn the_page_draws_the_switch_the_tabs_and_the_rows() {
    let tmp = Tmp::new("sto-page");
    let st = st(&tmp);
    put_scene(&st, "score", r#"{"value":{"best":9}}"#, "scene").await;
    put_scene(&st, "<name>", r#"{"value":"a & b"}"#, "cli").await;
    std::fs::write(root(&st).join(".env"), "SECRET=hunter2\n").unwrap();
    sp::player_put(
        State(st.clone()),
        ConnectInfo(LOCAL),
        headers("scene"),
        uri("/players/0xABCDEFabcdefABCDEFabcdefABCDEFabcdefABCD/values/lives"),
        Path((
            "0xABCDEFabcdefABCDEFabcdefABCDEFabcdefABCD".to_string(),
            "lives".to_string(),
        )),
        Bytes::from(r#"{"value":3}"#),
    )
    .await;

    let html = page(&st, "").await;
    assert_eq!(html.matches(r#"class="pgnav__lnk""#).count(), 5);
    assert!(
        html.contains(r#"href="/storage" aria-current="page">Storage"#),
        "{html}"
    );
    assert!(html.contains(r#"class="sto__tab" href="/storage?tab=scene" aria-current="page">Scene<span class="sto__n">2</span>"#), "{html}");
    assert!(
        html.contains(r#"href="/storage?tab=player">Player<span class="sto__n">1</span>"#),
        "{html}"
    );
    assert!(
        html.contains(
            r#"data-key="score" data-json="{
  &quot;best&quot;: 9
}""#
        ),
        "the editor opens with pretty JSON: {html}"
    );
    assert!(
        html.contains(r#"data-key="&lt;name&gt;" data-json="&quot;a &amp; b&quot;""#),
        "escaped: {html}"
    );
    assert!(
        html.contains("<span class=\"sto__val\">a &amp; b</span>"),
        "strings show verbatim: {html}"
    );
    assert!(
        html.contains(r#"name="upstream"><span class="knob__k">Use upstream storage</span>"#),
        "{html}"
    );
    assert!(!html.contains(r#"name="upstream" checked"#));
    assert!(
        html.contains(r#"<div class="sto__choice" data-choice hidden>"#),
        "the service choice hides while local"
    );
    assert!(
        html.contains("Values live in this project's <code>.dcl-one/storage.sqlite</code>"),
        "{html}"
    );
    assert!(html.contains("Recent writes (3)"), "{html}");
    assert!(
        html.contains(r#"scene/score <span class="sto__src">scene</span>"#),
        "{html}"
    );
    assert!(html.contains(r#"href="/storage/export" download="server-storage.json""#));
    assert!(html.contains(r#"data-act="clear" data-what="every scene value""#));

    let env = page(&st, "tab=env").await;
    assert!(
        env.contains(r#"href="/storage?tab=env" aria-current="page">Environment"#),
        "{env}"
    );
    assert!(
        env.contains(r#"data-key="SECRET" data-json="hunter2""#),
        "{env}"
    );
    assert!(env.contains("<span class=\"sto__val\">\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}</span>"), "masked: {env}");
    assert!(env.contains(r#"data-act="reveal""#));
    assert!(
        env.contains(r#"<span class="sto__meta">from .env</span>"#),
        "{env}"
    );
    assert!(
        !env.contains(r#"data-what="every runtime environment override""#),
        "nothing to clear: only .env rows"
    );

    let players = page(&st, "tab=player").await;
    assert!(
        players.contains(
            r#"href="/storage?tab=player&amp;player=0xabcdefabcdefabcdefabcdefabcdefabcdefabcd">0xabcd…abcd <span class="sto__n">1</span>"#
        ),
        "{players}"
    );
    assert!(players.contains("Pick a wallet address"));
    let one = page(
        &st,
        "tab=player&player=0xABCDEFabcdefABCDEFabcdefABCDEFabcdefABCD",
    )
    .await;
    assert!(
        one.contains(
            r#"data-scope="player" data-address="0xabcdefabcdefabcdefabcdefabcdefabcdefabcd""#
        ),
        "{one}"
    );
    assert!(one.contains(r#"data-key="lives" data-json="3""#), "{one}");
    assert!(one.contains(r#"data-what="every value of this player""#));
    assert!(
        one.contains(r#"href="/storage?tab=player&amp;player=0xabcdefabcdefabcdefabcdefabcdefabcdefabcd" aria-current="true""#),
        "{one}"
    );
}

#[tokio::test]
async fn the_target_switch_persists_and_the_page_reflects_it() {
    let _guard = crate::deploy::ENV_LOCK.lock().await;
    std::env::remove_var("DCL_PRIVATE_KEY");
    let tmp = Tmp::new("sto-target");
    let st = st(&tmp);
    let tok = super::deploy_page::token(&st).to_string();
    let submit = |form: &str| {
        let st = st.clone();
        let form = target_form(form);
        async move {
            sp::set_target(
                State(st),
                ConnectInfo(LOCAL),
                headers("ui"),
                axum::Form(form),
            )
            .await
        }
    };
    let resp = submit(&format!("token={tok}&upstream=on&service=zone&tab=env")).await;
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    assert_eq!(resp.headers()[header::LOCATION], "/storage?tab=env");
    assert_eq!(
        storage::open(&root(&st)).unwrap().target().unwrap(),
        Target::Zone
    );

    let html = page(&st, "tab=env").await;
    assert!(html.contains(r#"name="upstream" checked"#), "{html}");
    assert!(
        html.contains(r#"<div class="sto__choice" data-choice>"#),
        "the choice shows"
    );
    assert!(html.contains(r#"value="zone" checked"#), "{html}");
    assert!(html.contains(r#"data-target="zone""#));
    assert!(html.contains("Every request goes to <code>https://storage.decentraland.zone</code> for parcel 1,2, but nothing here can sign them"), "{html}");
    assert!(!html.contains("Recent writes"), "the log is local-only");
    assert!(
        html.contains("switch storage back to local to use them"),
        "{html}"
    );

    let bad = submit(&format!(
        "token={tok}&upstream=on&service=custom&url=ftp%3A%2F%2Fx"
    ))
    .await;
    assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
    let none = submit(&format!("token={tok}&upstream=on&service=custom&url=")).await;
    assert_eq!(none.status(), StatusCode::BAD_REQUEST);
    let custom = submit(&format!(
        "token={tok}&upstream=on&service=custom&url=http%3A%2F%2Flocalhost%3A5199%2Fstorage%2F"
    ))
    .await;
    assert_eq!(custom.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        storage::open(&root(&st)).unwrap().target().unwrap(),
        Target::Custom("http://localhost:5199/storage".into())
    );
    let html = page(&st, "").await;
    assert!(html.contains(r#"value="custom" checked"#), "{html}");
    assert!(
        html.contains(
            r#"value="http://localhost:5199/storage" aria-label="Custom storage service URL">"#
        ),
        "the url input is shown: {html}"
    );

    let back = submit(&format!("token={tok}&service=org")).await;
    assert_eq!(back.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        storage::open(&root(&st)).unwrap().target().unwrap(),
        Target::Local
    );
    let stale = submit("token=nope&upstream=on&service=org").await;
    assert_eq!(stale.status(), StatusCode::FORBIDDEN);
    let remote = sp::set_target(
        State(st.clone()),
        ConnectInfo(REMOTE),
        headers("ui"),
        axum::Form(target_form(&format!("token={tok}&upstream=on&service=org"))),
    )
    .await;
    assert_eq!(remote.status(), StatusCode::FORBIDDEN);
}

type Seen = Arc<Mutex<Vec<(String, String, HeaderMap, String)>>>;

/// A stand-in storage service that records every request and answers with
/// a fixed body, so the proxy's signing and relaying can be read back.
async fn mock_service(status: StatusCode, body: &'static str) -> (String, Seen) {
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let app = axum::Router::new().fallback(
        move |method: axum::http::Method, uri: Uri, headers: HeaderMap, body_in: String| {
            let log = log.clone();
            async move {
                log.lock()
                    .unwrap()
                    .push((method.to_string(), uri.to_string(), headers, body_in));
                (status, [(header::CONTENT_TYPE, "application/json")], body)
            }
        },
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), seen)
}

#[tokio::test]
async fn a_service_target_forwards_signed_requests_and_relays_the_answer() {
    let _guard = crate::deploy::ENV_LOCK.lock().await;
    std::env::set_var(
        "DCL_PRIVATE_KEY",
        "0x0123456789012345678901234567890123456789012345678901234567890123",
    );
    let tmp = Tmp::new("sto-proxy");
    let st = st(&tmp);
    let (base, seen) = mock_service(StatusCode::CREATED, r#"{"value":"from the service"}"#).await;
    storage::open(&root(&st))
        .unwrap()
        .set_target(&Target::Custom(format!("{base}/storage")))
        .unwrap();

    let (status, body) =
        json_of(put_scene(&st, "hi there", r#"{"value":[1]}"#, "scene").await).await;
    assert_eq!(
        (status, body),
        (201, json!({ "value": "from the service" })),
        "relayed as answered"
    );
    {
        let seen = seen.lock().unwrap();
        let (method, uri, headers, body) = &seen[0];
        assert_eq!(
            (method.as_str(), uri.as_str()),
            ("PUT", "/storage/values/hi%20there")
        );
        assert_eq!(body, r#"{"value":[1]}"#);
        assert_eq!(headers[SOURCE_HEADER], "scene");
        assert_eq!(headers["x-identity-metadata"], r#"{"parcel":"1,2"}"#);
        assert!(headers.contains_key("x-identity-timestamp"));
        let link: Value =
            serde_json::from_str(headers["x-identity-auth-chain-1"].to_str().unwrap()).unwrap();
        assert_eq!(link["type"], "ECDSA_SIGNED_ENTITY");
        assert!(
            link["payload"]
                .as_str()
                .unwrap()
                .starts_with("put:/storage/values/hi%20there:"),
            "{link}"
        );
        assert!(!headers.contains_key("x-confirm-delete-all"));
    }
    assert!(
        storage::open(&root(&st))
            .unwrap()
            .get(Scope::Scene, "hi there")
            .unwrap()
            .is_none(),
        "nothing lands locally while a service is the target"
    );

    let clear = sp::scene_clear(
        State(st.clone()),
        ConnectInfo(LOCAL),
        confirm(headers("cli")),
        uri("/values"),
    )
    .await;
    assert_eq!(clear.status(), StatusCode::CREATED);
    {
        let seen = seen.lock().unwrap();
        let (method, uri, headers, _) = &seen[1];
        assert_eq!(
            (method.as_str(), uri.as_str()),
            ("DELETE", "/storage/values")
        );
        assert_eq!(
            headers["x-confirm-delete-all"], "true",
            "the confirm header rides along"
        );
    }
    let list = sp::scene_list(
        State(st.clone()),
        ConnectInfo(LOCAL),
        headers("test"),
        uri("/values?prefix=a&limit=5"),
        Query(sp::ListQuery::default()),
    )
    .await;
    assert_eq!(list.status(), StatusCode::CREATED);
    assert_eq!(
        seen.lock().unwrap()[2].1,
        "/storage/values?prefix=a&limit=5",
        "the query is forwarded verbatim"
    );

    let html = page(&st, "").await;
    assert!(html.contains("signed as 0x"), "{html}");
    assert!(html.contains("(DCL_PRIVATE_KEY)"), "{html}");
    assert!(
        html.contains(r#"<p class="note sto__problem">the service answered HTTP 201"#),
        "a listing that is not a 200 is reported: {html}"
    );

    std::env::remove_var("DCL_PRIVATE_KEY");
    storage::open(&root(&st))
        .unwrap()
        .set_target(&Target::Custom("http://127.0.0.1:1".into()))
        .unwrap();
    let (status, body) = json_of(get_scene(&st, "k", LOCAL).await).await;
    assert_eq!(status, 502);
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .contains("could not reach the storage service"),
        "{body}"
    );
    let (status, body) = json_of(sp::export(State(st.clone())).await).await;
    assert_eq!(status, 409, "{body}");
}

#[tokio::test]
async fn a_service_listing_fills_the_page() {
    let _guard = crate::deploy::ENV_LOCK.lock().await;
    std::env::remove_var("DCL_PRIVATE_KEY");
    let tmp = Tmp::new("sto-proxy-page");
    let st = st(&tmp);
    let (base, seen) = mock_service(
        StatusCode::OK,
        r#"{"data":[{"key":"remote","value":{"n":1}}],"pagination":{"offset":0,"total":40}}"#,
    )
    .await;
    storage::open(&root(&st))
        .unwrap()
        .set_target(&Target::Custom(base))
        .unwrap();
    let html = page(
        &st,
        "tab=player&player=0xABCDEFabcdefABCDEFabcdefABCDEFabcdefABCD",
    )
    .await;
    assert!(html.contains(r#"data-key="remote""#), "{html}");
    assert!(html.contains("Showing 1 of 40 keys"), "{html}");
    assert!(html.contains("nothing here can sign them"), "{html}");
    let seen = seen.lock().unwrap();
    assert_eq!(
        seen[0].1,
        "/players/0xabcdefabcdefabcdefabcdefabcdefabcdefabcd/values?limit=100"
    );
    assert!(
        !seen[0].2.contains_key("x-identity-auth-chain-0"),
        "unsigned when nobody can sign"
    );
    assert_eq!(seen[0].2[SOURCE_HEADER], "ui");
}

#[tokio::test]
async fn snapshots_export_and_import_through_the_routes() {
    let tmp = Tmp::new("sto-snapshot");
    let st = st(&tmp);
    put_scene(&st, "w", r#"{"value":1}"#, "scene").await;
    let resp = sp::export(State(st.clone())).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()[header::CONTENT_DISPOSITION],
        "attachment; filename=\"server-storage.json\""
    );
    let snapshot: Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert_eq!(
        snapshot,
        json!({ "env": {}, "world": { "w": 1 }, "players": {} })
    );

    let import = |query: &str, body: &str, peer: SocketAddr| {
        let st = st.clone();
        let q = sp::ImportQuery {
            merge: query.strip_prefix("merge=").map(str::to_string),
        };
        let body = body.to_string();
        async move {
            json_of(
                sp::import(
                    State(st),
                    ConnectInfo(peer),
                    headers("ui"),
                    Query(q),
                    Bytes::from(body),
                )
                .await,
            )
            .await
        }
    };
    let (status, body) = import(
        "merge=1",
        r#"{"world":{"x":2},"players":{"0xA":{"k":"v"}}}"#,
        LOCAL,
    )
    .await;
    assert_eq!(
        (status, body.clone()),
        (
            200,
            json!({ "merged": true, "counts": { "scene": 2, "player": 1, "env": 0 } })
        ),
        "{body}"
    );
    let (status, body) = import("", r#"{"world":{"only":true}}"#, LOCAL).await;
    assert_eq!(status, 200);
    assert_eq!(body["counts"], json!({ "scene": 1, "player": 0, "env": 0 }));
    assert_eq!(import("", "[]", LOCAL).await.0, 400);
    assert_eq!(import("", "nope", LOCAL).await.0, 400);
    assert_eq!(import("", "{}", REMOTE).await.0, 403);
    let db = storage::open(&root(&st)).unwrap();
    assert_eq!(db.get(Scope::Scene, "only").unwrap(), Some(json!(true)));
    assert_eq!(db.activity(3).unwrap()[0].op, "import");
}

#[tokio::test]
async fn without_a_scene_the_routes_say_so() {
    let st = Arc::new(state(Vec::new()));
    let (status, body) = json_of(get_scene(&st, "k", LOCAL).await).await;
    assert_eq!(status, 503);
    assert!(body["message"]
        .as_str()
        .unwrap()
        .contains("no scene is loaded"));
    let html = page(&st, "").await;
    assert!(html.contains("No scene is loaded, so there is no storage to show."));
}

#[test]
fn the_start_line_names_the_target_the_signer_and_the_switch() {
    let base = "http://127.0.0.1:8000/";
    assert_eq!(
        sp::start_line(&Target::Local, None, base),
        "Storage: local SQLite (.dcl-one/storage.sqlite); switch at http://127.0.0.1:8000/storage"
    );
    assert_eq!(
        sp::start_line(&Target::Zone, None, base),
        "Storage: storage.decentraland.zone, unsigned until DCL_PRIVATE_KEY is set or a wallet connects on http://127.0.0.1:8000/deploy; switch at http://127.0.0.1:8000/storage"
    );
    let key = crate::storage_remote::Signer::Key(
        "0x0123456789012345678901234567890123456789012345678901234567890123".to_string(),
    );
    let line = sp::start_line(
        &Target::Custom("http://localhost:5199/storage".into()),
        Some(&key),
        base,
    );
    assert!(
        line.starts_with("Storage: http://localhost:5199/storage, signed by 0x"),
        "{line}"
    );
    assert!(
        line.ends_with("(DCL_PRIVATE_KEY); switch at http://127.0.0.1:8000/storage"),
        "{line}"
    );
}
