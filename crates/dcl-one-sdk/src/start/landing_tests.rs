use super::join_card::*;
use super::layout_card::*;
use super::*;
use crate::scene::Project;
use crate::start::chrome::STYLE;
use serde_json::json;
use std::sync::Mutex;

async fn scene_html(st: &AppState) -> String {
    let resp = scene_page(st, &axum::http::HeaderMap::new());
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

fn landing_state(projects: Vec<Project>) -> AppState {
    let mut st = crate::start::testkit::state(projects);
    st.port = 8000;
    st.local_ab = false;
    st
}

fn target(key: &'static str, label: &'static str, url: &str, carry: Carry) -> Target {
    Target {
        key,
        label,
        hint: String::new(),
        url: url.into(),
        qr: String::new(),
        carry,
    }
}

/// The page is one launch card, and every knob is a control inside the one
/// form: a radio that is not submitted is a choice `apply` throws away.
#[test]
fn the_page_renders_one_card_and_submits_every_knob() {
    let targets = vec![
        target(
            WHERE_DESKTOP,
            "this machine",
            "decentraland://a",
            Carry::Loopback,
        ),
        target(
            WHERE_LAN,
            "another device",
            "decentraland://b",
            Carry::Routable,
        ),
    ];
    let html = join_control(&targets, 0, true, true, &Knobs::default(), &[], "");
    assert_eq!(
        html.matches(r#"<div class="jn "#).count() + html.matches(r#"<div class="jn">"#).count(),
        1,
        "one card, the selected target: {html}"
    );
    assert!(html.contains("decentraland://a") && !html.contains("decentraland://b"));
    assert_eq!(
        join_control(&targets, 1, true, true, &Knobs::default(), &[], "",)
            .matches("decentraland://b")
            .count(),
        2,
        "picking the second target renders its card — the href and the url line"
    );
    assert_eq!(html.matches("<form").count(), 1);
    assert!(html.contains(r#"<form class="side" method="get" action="/">"#));
    let form = html.find("<form").expect("no form");
    assert!(
        !html[..form].contains(r#"type="radio""#),
        "a radio outside the form is not submitted: {html}"
    );
    assert_eq!(
        html.matches(r#"type="radio""#).count(),
        targets.len() + 2,
        "one radio per where-choice plus the two mcp states"
    );
    assert_eq!(html.matches(" checked>").count(), 2, "one where, one mcp");
    assert!(html.contains(r#"value="desktop" checked"#));
    assert!(html.contains(r#"value="on" checked"#), "mcp on by default");
    assert!(
        join_control(&targets, 0, false, false, &Knobs::default(), &[], "",)
            .contains(r#"value="off" checked"#),
        "--no-mcp starts the knob off, since nothing would read the port"
    );
    assert!(!html.contains("<script"), "the knobs cost no javascript");
    assert!(
        html.contains(r#"<legend class="knob__k u-sr-only">where</legend>"#),
        "the where group lost its accessible name: {html}"
    );
    for legend in ["Scene errors in the terminal", "Deep-link flags"] {
        assert!(
            html.contains(&format!(r#"<legend class="knob__k">{legend}</legend>"#)),
            "{legend} has no accessible group name"
        );
    }
    assert!(
        !STYLE.contains("opacity: 0") && !STYLE.contains(":checked ~"),
        "no hidden radios and no reveal rules are left"
    );
}

/// The QR is 17 KB of data url: it belongs to the phone card and is
/// emitted only when the phone card is the one on screen.
#[test]
fn the_qr_rides_the_phone_card_only() {
    let mut phone = target(WHERE_PHONE, "phone", "decentraland://open", Carry::Nothing);
    phone.qr = r#"<span class="qr"><img src="data:image/svg+xml,QRQR"></span>"#.into();
    let targets = vec![
        target(
            WHERE_DESKTOP,
            "this machine",
            "decentraland://a",
            Carry::Loopback,
        ),
        phone,
    ];
    let desktop = join_control(&targets, 0, true, true, &Knobs::default(), &[], "");
    assert!(!desktop.contains("QRQR"), "no QR on a card that has none");
    let phone = join_control(&targets, 1, true, true, &Knobs::default(), &[], "");
    assert_eq!(phone.matches("QRQR").count(), 1, "the QR is emitted once");
}

/// The href of the one launch button, as a browser would read it.
fn launch_href(html: &str) -> String {
    const OPEN: &str = r#"<a class="jn__cta" id="launch" href=""#;
    let at = html.find(OPEN).expect("no launch button") + OPEN.len();
    html[at..][..html[at..].find('"').unwrap()].replace("&amp;", "&")
}

/// The knobs reach the link, come back set, and cannot repoint it: a core
/// key keeps the value this server chose, and there is no longer any way
/// to put a flag of one's own choosing into this page's launch button.
#[test]
fn page_knobs_reach_the_link_but_cannot_repoint_it() {
    let st = landing_state(vec![Project {
        root: std::path::PathBuf::from("/tmp/scene"),
        scene_json: json!({
            "display": { "title": "Gather" },
            "spawnPoints": [{ "name": "entrance", "position": { "x": 8, "y": 0, "z": 8 } }]
        }),
    }]);
    let knobs = knobs(
        Some("opt=multi-instance&spawn=entrance&args=--gatekeeper-url%3Dhttps%3A%2F%2Fevil.example&realm=http%3A%2F%2Fevil.example"),
        &["entrance".to_string()],
    );
    assert_eq!(
        knobs.tokens(Carry::Loopback),
        [
            "--multi-instance=true",
            "--landscape-terrain-enabled=false",
            "--spawnpoint=entrance"
        ],
        "only the knobs the page draws become tokens"
    );
    let html = render(
        &st,
        "http://127.0.0.1:8000",
        "",
        "http://127.0.0.1:8000",
        None,
        &knobs,
    );
    let link = launch_href(&html);
    assert!(link.contains("multi-instance=true"), "{link}");
    assert!(link.contains("spawnpoint=entrance"), "{link}");
    assert!(
        link.contains("realm=http%3A%2F%2F127.0.0.1%3A8000"),
        "the realm stays this server's: {link}"
    );
    assert!(
        !html.contains("evil.example") && !html.contains("gatekeeper"),
        "a query key this page does not own reaches neither the link nor the page"
    );
    assert!(
        !html.contains(r#"name="args""#),
        "the free-text field is gone, so there is nothing to reflect"
    );
    assert!(
        html.contains(r#"value="multi-instance" checked"#),
        "the checkbox comes back checked"
    );
    assert!(
        html.contains(r#"<option value="entrance" selected"#),
        "the chosen spawn point stays chosen"
    );
}

/// The mcp knob is the whole difference between its two links — the port
/// comes from the Explorer's own default, so neither end has to be told.
#[test]
fn the_mcp_knob_adds_exactly_the_mcp_pair() {
    let st = landing_state(vec![Project {
        root: std::path::PathBuf::from("/tmp/scene"),
        scene_json: json!({ "display": { "title": "Gather" } }),
    }]);
    let link = |query: &str| {
        launch_href(&render(
            &st,
            "http://127.0.0.1:8000",
            "",
            "http://127.0.0.1:8000",
            None,
            &knobs(Some(query), &[]),
        ))
    };
    let (off, on) = (link("mcp=off"), link("mcp=on"));
    assert_eq!(
        on.replace(
            &format!(
                "&mcp=true&mcp-port={}",
                joinblock::DEFAULT_EXPLORER_MCP_PORT
            ),
            ""
        ),
        off
    );
    assert!(!off.contains("mcp"), "{off}");
    assert_eq!(link(""), on, "--mcp is the default the page starts from");
}

/// The query is the page's whole state, so it is also its whole untrusted
/// input. Every key it owns is an allowlist — a checkbox the page does not
/// draw, a spawn point this scene does not define, a target that is not on
/// offer — and every key it does not own is ignored.
#[test]
fn the_knobs_take_only_what_the_page_offers() {
    let names = ["entrance".to_string()];
    let tokens = |q: Option<&str>| knobs(q, &names).tokens(Carry::Loopback);
    // Terrain is inverted (the client draws it unless =false), so the fresh
    // page — which starts it checked — emits nothing for it, while any
    // APPLIED state without the box means "off".
    let terrain_off = ["--landscape-terrain-enabled=false".to_string()];
    assert_eq!(
        tokens(None),
        ["--multi-instance=true", "--skip-auth-screen=true"],
        "a fresh page carries exactly the default-on flags; its terrain default is the client's own"
    );
    assert_eq!(tokens(Some("other=1")), terrain_off);
    assert_eq!(
        tokens(Some("opt=landscape-terrain-enabled")),
        Vec::<String>::new(),
        "the checked box is the client's own default, so it adds no token"
    );
    assert_eq!(
        tokens(Some("opt=launch-cdp-monitor-on-start")),
        terrain_off,
        "a checkbox the page never shows is not a checkbox"
    );
    assert_eq!(
        tokens(Some("spawn=nowhere")),
        terrain_off,
        "a spawn point this scene does not define"
    );
    assert_eq!(
        tokens(Some("spawn=entrance")),
        ["--landscape-terrain-enabled=false", "--spawnpoint=entrance"],
        "one it does define"
    );
    for smuggled in [
        "args=--gatekeeper-url%3Dhttps%3A%2F%2Fevil",
        "gatekeeper-url=https%3A%2F%2Fevil",
        "--gatekeeper-url=https%3A%2F%2Fevil",
        "opt=gatekeeper-url",
    ] {
        assert_eq!(
            tokens(Some(smuggled)),
            terrain_off,
            "{smuggled} smuggles nothing beyond the terrain default"
        );
    }
    assert_eq!(knobs(Some("where=lan"), &names).where_key, "lan");
    assert_eq!(
        knobs(Some("where=%2F%2Fevil"), &names).where_key,
        "",
        "a target that is not on offer selects nothing"
    );
    assert_eq!(knobs(Some("mcp=on"), &names).mcp, Some(true));
    assert_eq!(
        knobs(Some("mcp=maybe"), &names).mcp,
        None,
        "the mcp knob has two states and nothing else"
    );
}

/// `Uri.IsLoopback` is what the client asks before it keeps the
/// loopback-only tier of its deep-link allowlist, so this has to ask the
/// same question of the realm this page is about to build a link for.
#[test]
fn realm_carry_calls_only_loopback_loopback() {
    for loopback in [
        "http://127.0.0.1:8000",
        "http://127.0.0.1:8000/prefix",
        "http://localhost:8000",
        "http://[::1]:8000",
    ] {
        assert_eq!(realm_carry(loopback), Carry::Loopback, "{loopback}");
    }
    for routable in [
        "http://192.168.1.9:8000",
        "http://10.0.0.4:8000",
        "https://preview.example.org",
        "http://127.0.0.1.evil.example:8000",
    ] {
        assert_eq!(realm_carry(routable), Carry::Routable, "{routable}");
    }
}

/// The LAN target's realm is not loopback, so the client drops the whole
/// loopback-only tier. The page must not send those params into a link
/// that cannot use them, and must not claim they do anything.
#[test]
fn the_lan_target_drops_the_knobs_its_client_would_throw_away() {
    let st = landing_state(vec![Project {
        root: std::path::PathBuf::from("/tmp/scene"),
        scene_json: json!({
            "display": { "title": "Gather" },
            "spawnPoints": [{ "name": "entrance", "position": { "x": 8, "y": 0, "z": 8 } }]
        }),
    }]);
    let query = "opt=multi-instance&opt=hub&opt=force-open-backpack&spawn=entrance&mcp=on";
    let page = |where_key: &str| {
        render(
            &st,
            "http://127.0.0.1:8000",
            "",
            "http://127.0.0.1:8000",
            Some("http://192.168.1.9:8000"),
            &knobs(
                Some(&format!("{query}&where={where_key}")),
                &["entrance".to_string()],
            ),
        )
    };
    let desktop = page(WHERE_DESKTOP);
    let desktop_link = launch_href(&desktop);
    for kept in ["multi-instance=true", "hub=true", "mcp=true"] {
        assert!(desktop_link.contains(kept), "loopback keeps {kept}");
    }

    let lan = page(WHERE_LAN);
    let link = launch_href(&lan);
    assert!(link.contains("192.168.1.9"), "the lan card is selected");
    for dropped in ["multi-instance", "hub=true", "mcp=true", "mcp-port"] {
        assert!(
            !link.contains(dropped),
            "the client drops {dropped} for a lan realm, so the link must not carry it: {link}"
        );
    }
    for kept in ["force-open-backpack=true", "spawnpoint=entrance"] {
        assert!(link.contains(kept), "any realm keeps {kept}: {link}");
    }
    assert!(
        lan.contains(r#"<div class="flag flag--off"><span class="flag__l"><span class="chk__k">multi-instance</span><b class="knob__on">on</b></span>"#),
        "a dead flag shows its value as text, not a disabled switch: {lan}"
    );
    assert!(
        lan.contains(r#"type="hidden" name="opt" value="multi-instance""#),
        "the dead flag's value still survives an Apply round-trip: {lan}"
    );
    assert!(
        !lan.contains(r#"type="checkbox" checked disabled"#)
            && !lan.contains(r#"type="radio" name="mcp" value="off" disabled"#),
        "no disabled controls remain: {lan}"
    );
    assert!(
        lan.contains(r#"<input type="hidden" name="opt" value="multi-instance">"#),
        "a disabled knob keeps its value for the target that can use it"
    );
    let form_at = desktop.find("<form").expect("the knob form");
    let form = &desktop[form_at..desktop[form_at..].find("</form>").unwrap() + form_at];
    assert!(
        !form.contains(" disabled>")
            && !desktop.contains(r#"class="chk chk--off""#)
            && !desktop.contains(r#"class="knob__l knob__l--off""#),
        "no knob is greyed out on the loopback target: {form}"
    );
}

/// The web and phone links carry no deep-link params at all, so every knob
/// is inert for them — including the spawn point, which is a param too.
#[test]
fn the_targets_that_read_no_params_grey_out_every_knob() {
    let st = landing_state(vec![Project {
        root: std::path::PathBuf::from("/tmp/scene"),
        scene_json: json!({
            "display": { "title": "Gather" },
            "spawnPoints": [{ "name": "entrance", "position": { "x": 8, "y": 0, "z": 8 } }]
        }),
    }]);
    for where_key in [WHERE_WEB, WHERE_PHONE] {
        let html = render(
            &st,
            "http://127.0.0.1:8000",
            "",
            "http://127.0.0.1:8000",
            None,
            &knobs(
                Some(&format!("opt=hub&spawn=entrance&where={where_key}")),
                &["entrance".to_string()],
            ),
        );
        let link = launch_href(&html);
        assert!(
            !link.contains("hub") && !link.contains("spawnpoint"),
            "{link}"
        );
        assert!(
            html.contains(r#"<span class="chk__k">hub</span><b class="knob__on">on</b>"#),
            "{where_key}: a fixed flag reads as its value, no control: {html}"
        );
        assert!(
            !html.contains(r#"<select class="knob__sel""#)
                && html.contains("this link carries only the realm and a position"),
            "{where_key}: the spawn choice reads as text with the why: {html}"
        );
        assert!(
            html.contains("this target's link never carries the MCP flag"),
            "{where_key}: the MCP state says why it is fixed: {html}"
        );
        assert!(
            html.contains(r#"type="hidden" name="opt" value="hub""#)
                && html.contains(r#"type="hidden" name="spawn" value="entrance""#),
            "{where_key}: fixed values still ride an Apply round-trip: {html}"
        );
    }
}

/// The label a screen reader announces is the label on the screen: an
/// aria-label that says something else fails WCAG 2.5.3, and there is no
/// aria-label on this page that has to.
#[test]
fn every_control_is_named_by_the_text_beside_it() {
    let st = landing_state(vec![Project {
        root: std::path::PathBuf::from("/tmp/scene"),
        scene_json: json!({
            "display": { "title": "Gather" },
            "spawnPoints": [{ "name": "entrance", "position": { "x": 8, "y": 0, "z": 8 } }]
        }),
    }]);
    let html = render(
        &st,
        "http://127.0.0.1:8000",
        "",
        "http://127.0.0.1:8000",
        None,
        &Knobs::default(),
    );
    assert_eq!(
        html.matches("<form").count(),
        2,
        "the join form and the bar's connect, stylesheet included"
    );
    assert!(
        !html.contains("aria-label=\"spawn"),
        "the <select> is named by its visible <label for>"
    );
    assert!(html.contains(r#"<label class="knob__k" for="spawn">Spawn point</label>"#));
    assert!(html.contains(r#"id="spawn""#));
    assert_eq!(
        html.matches(r#"<fieldset class="knob"#).count(),
        3,
        "the tabs, mcp and the flag group each have a <legend>: {html}"
    );
    assert_eq!(html.matches("<legend ").count(), 3);
    assert!(html.contains(r#"<legend class="knob__k u-sr-only">where</legend>"#));
}

/// A memo that never recomputes is a bug and a memo that always
/// recomputes is not a memo.
#[test]
fn the_ttl_memo_computes_once_inside_its_window() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let cell = Mutex::new(None);
    let calls = AtomicUsize::new(0);
    let hit = || {
        memoised(&cell, Duration::from_secs(60), || {
            calls.fetch_add(1, Ordering::SeqCst);
            7u32
        })
    };
    assert_eq!((hit(), hit(), hit()), (7, 7, 7));
    assert_eq!(calls.load(Ordering::SeqCst), 1, "one getifaddrs, not three");
    let expired = Mutex::new(None);
    let stale = || {
        memoised(&expired, Duration::ZERO, || {
            calls.fetch_add(1, Ordering::SeqCst);
            7u32
        })
    };
    assert_eq!((stale(), stale()), (7, 7));
    assert_eq!(
        calls.load(Ordering::SeqCst),
        3,
        "an expired entry is recomputed"
    );
}

/// The QR memo is keyed on the link, which is the only thing the encoding
/// depends on: a new link has to produce a new QR.
#[test]
fn the_keyed_memo_recomputes_when_the_key_changes() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let cell = Mutex::new(None);
    let calls = AtomicUsize::new(0);
    let hit = |key: &str| {
        memoised_by(&cell, key, || {
            calls.fetch_add(1, Ordering::SeqCst);
            key.to_uppercase()
        })
    };
    assert_eq!(hit("a"), "A");
    assert_eq!(hit("a"), "A");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(hit("b"), "B", "a different link is a different QR");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let link = "decentraland://open?preview=http://127.0.0.1:8000&position=0,0";
    assert_eq!(qr_data_url(link), joinblock::qr_svg_data_url(link));
}

/// `--ink-6` is the colour of the copy that says what each knob does and
/// `--ink-7` the colour of the running prose, so both have to clear AA —
/// and, now that section heads and helper lines sit outside the cards,
/// against `--page` as well as `--panel`, in both schemes.
#[test]
fn the_knob_copy_clears_aa_in_both_schemes() {
    fn contrast(fg: (f64, f64, f64), bg: (f64, f64, f64)) -> f64 {
        let lum = |c: (f64, f64, f64)| {
            let ch = |v: f64| {
                let v = v / 255.0;
                match v <= 0.03928 {
                    true => v / 12.92,
                    false => ((v + 0.055) / 1.055).powf(2.4),
                }
            };
            0.2126 * ch(c.0) + 0.7152 * ch(c.1) + 0.0722 * ch(c.2)
        };
        let (a, b) = (lum(fg), lum(bg));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }
    let alpha = |ink: &str| {
        let at = STYLE.find(ink).unwrap_or_else(|| panic!("no {ink}"));
        let open = STYLE[at..].find('(').unwrap() + at + 1;
        let decl = &STYLE[open..][..STYLE[open..].find(')').unwrap()];
        decl.rsplit(',').next().unwrap().parse::<f64>().unwrap()
    };
    let over = |ink: (f64, f64, f64), a: f64, bg: (f64, f64, f64)| {
        let mix = |i: f64, b: f64| i * a + b * (1.0 - a);
        contrast((mix(ink.0, bg.0), mix(ink.1, bg.1), mix(ink.2, bg.2)), bg)
    };
    let light = over(
        (22.0, 21.0, 24.0),
        alpha("--ink-6: rgba(22"),
        (255.0, 255.0, 255.0),
    );
    assert!(light >= 4.5, "light --ink-6 is {light:.2}:1, under AA");
    let dark = over(
        (255.0, 255.0, 255.0),
        alpha("--ink-6: rgba(255"),
        (22.0, 21.0, 24.0),
    );
    assert!(dark >= 7.0, "dark --ink-6 regressed to {dark:.2}:1");
    let page_light = (243.0, 242.0, 245.0);
    let page_dark = (14.0, 13.0, 16.0);
    for (name, ink, decl, bg) in [
        (
            "light --ink-7 on --panel",
            (22.0, 21.0, 24.0),
            "--ink-7: rgba(22",
            (255.0, 255.0, 255.0),
        ),
        (
            "light --ink-7 on --page",
            (22.0, 21.0, 24.0),
            "--ink-7: rgba(22",
            page_light,
        ),
        (
            "light --ink-6 on --page",
            (22.0, 21.0, 24.0),
            "--ink-6: rgba(22",
            page_light,
        ),
        (
            "dark --ink-7 on --page",
            (255.0, 255.0, 255.0),
            "--ink-7: rgba(255",
            page_dark,
        ),
        (
            "dark --ink-6 on --page",
            (255.0, 255.0, 255.0),
            "--ink-6: rgba(255",
            page_dark,
        ),
    ] {
        let ratio = over(ink, alpha(decl), bg);
        assert!(ratio >= 4.5, "{name} is {ratio:.2}:1, under AA");
    }
    let hex = |name: &str| {
        let at = STYLE.find(name).unwrap_or_else(|| panic!("no {name}"));
        let h = &STYLE[at + name.len()..][..6];
        (
            u8::from_str_radix(&h[0..2], 16).unwrap() as f64,
            u8::from_str_radix(&h[2..4], 16).unwrap() as f64,
            u8::from_str_radix(&h[4..6], 16).unwrap() as f64,
        )
    };
    let label = contrast((255.0, 255.0, 255.0), hex("--brand-cta: #"));
    assert!(
        label >= 4.5,
        "white on --brand-cta is {label:.2}:1, under AA"
    );
}

/// The machine-checkable form of "clean style, no extra labels".
///
/// The sheet gets exactly two shouts, both brand idioms: the button that
/// launches (the builder's SIGN IN / BUILD SCENES, tracked at .04em) and
/// `.knob__k` — the join card's micro-captions ("SPAWN POINT",
/// "DEEP-LINK FLAGS"), the Creator Hub datumtile caption brought back by
/// the join-card mockup. Every other label stays quiet: a shouted caption
/// stacked over every value it names is the failure mode this pins out.
#[test]
fn the_only_shouts_are_the_launch_button_and_the_knob_captions() {
    let rule = |sel: &str| {
        let at = STYLE
            .find(&format!("\n{sel} {{"))
            .unwrap_or_else(|| panic!("no rule for {sel}"));
        let open = STYLE[at..].find('{').unwrap() + at;
        &STYLE[open..][..STYLE[open..].find('}').unwrap()]
    };
    // A caption's traits now come from grouped selector lists, so every rule
    // whose selector list names the caption is checked, not one lone rule.
    for caption in [".datum__cap", ".kv .k", ".chk__k", ".bar__mark"] {
        let mut at = 0;
        let mut seen = 0;
        while let Some(pos) = STYLE[at..].find(caption) {
            let i = at + pos;
            at = i + caption.len();
            let after = &STYLE[at..];
            let selector_position = (after.starts_with(',') || after.starts_with(" {"))
                && (i == 0 || STYLE[..i].ends_with('\n') || STYLE[..i].ends_with(' '));
            if !selector_position {
                continue;
            }
            seen += 1;
            let open = STYLE[i..].find('{').unwrap() + i;
            let body = &STYLE[open..][..STYLE[open..].find('}').unwrap()];
            assert!(
                !body.contains("text-transform"),
                "{caption} shouts over the thing it names: {body}"
            );
            assert!(
                !body.contains("letter-spacing: ."),
                "{caption} is tracked like a caption that shouts: {body}"
            );
        }
        assert!(seen > 0, "no rule mentions {caption}");
    }
    assert_eq!(
        STYLE.matches("text-transform: uppercase").count(),
        2,
        "exactly two rules in the sheet shout"
    );
    assert!(
        rule(".jn__cta").contains("text-transform: uppercase")
            && rule(".jn__cta").contains("letter-spacing: .04em"),
        "the primary button, at the builder's own tracking"
    );
    assert!(
        rule(".knob__k").contains("text-transform: uppercase")
            && rule(".knob__k").contains("letter-spacing: .06em"),
        "and the knob captions, at datumtile tracking"
    );
    assert_eq!(
        STYLE.matches("letter-spacing: .").count(),
        2,
        "positive tracking rides those two rules and nothing else"
    );
}

#[test]
fn esc_neutralizes_html() {
    assert_eq!(esc(r#"<b a="1">&x"#), "&lt;b a=&quot;1&quot;&gt;&amp;x");
}

/// The feedback that reshaped the card: the deep link is text on the card,
/// not a disclosure to open — now in the footer beside the launch button.
/// The copy button ships hidden and the script that makes it work is what
/// reveals it.
#[test]
fn the_deep_link_is_shown_not_toggled() {
    let card = join_control(
        &[target(
            WHERE_DESKTOP,
            "this machine",
            "decentraland://a",
            Carry::Loopback,
        )],
        0,
        true,
        true,
        &Knobs::default(),
        &[],
        "",
    );
    assert!(
        !card.contains("<details"),
        "no disclosure left on the card: {card}"
    );
    assert!(!card.contains("Copy the deep link"));
    assert!(card.contains(r#"<span class="jn__url" id="deep-link">decentraland://a</span>"#));
    assert!(card.contains(r#"<button class="deep__copy" id="copy-link" type="button" hidden>"#));
    assert!(
        card.contains(r#"<div class="jn2__foot"><a class="jn__cta" id="launch""#),
        "the footer leads with Launch, the link line rides beside it: {card}"
    );
    assert!(
        card.find(r#"id="launch""#).unwrap() < card.find(r#"id="deep-link""#).unwrap(),
        "Launch before the link: {card}"
    );
}

/// The scene card is scene.json's editor, progressively: the server ships
/// every editor target with its id but INERT — no contenteditable, every
/// editor button disabled, the add affordances behind `body.editing` — and
/// the script is what switches them on. Without it the page degrades to
/// the read-only page it used to be, promising no click it cannot honour.
#[tokio::test]
async fn the_scene_card_ships_its_editors_inert_for_the_script_to_enable() {
    let st = landing_state(vec![Project {
        root: std::path::PathBuf::from("/tmp/scene"),
        scene_json: json!({
            "display": { "title": "Gather" },
            "spawnPoints": [{ "name": "s", "position": { "x": 1, "y": 0, "z": 1 } }]
        }),
    }]);
    let html = scene_html(&st).await;
    assert!(html.contains(r#"<h1 class="scene__title" id="edit-title">"#));
    assert!(
        !html.contains("contenteditable="),
        "editability is the script's to grant, or typing saves nowhere"
    );
    assert!(
        html.contains(r#"id="edit-desc""#)
            && html.contains(r#"data-hint="Click to add a description""#),
        "an empty description still renders, as the place to add one"
    );
    assert!(html.contains(r#"<div class="tags" id="edit-tags">"#));
    assert!(html.contains(r#"<span class="tag tag--add">+ Add tags</span>"#));
    assert!(html.contains(
        r#"<input id="cover-input" type="file" accept="image/png,image/jpeg,image/webp" hidden disabled>"#
    ));
    assert!(
        html.contains(r#"<label class="cover-edit""#),
        "the whole cover is the file input's click target"
    );
    assert!(html.contains(r#"<div id="lay-sed"></div>"#));
    assert!(html.contains(r#"id="lay-sadd" aria-pressed="false" disabled"#));
    assert!(html.contains(r#"data-spawn="0" disabled"#));
    assert!(
        html.contains(r#"data-laytab="info" role="tab" aria-selected="true" disabled"#)
            && html.contains(r#"data-laytab="parcels" role="tab" aria-selected="false" disabled"#),
        "the layout tabs ship inert, Info selected: {html}"
    );
    assert!(html.contains(r#"id="lay-base-pick" aria-pressed="false" disabled"#));
    assert!(
        html.contains(r#"data-perm="USE_WEBSOCKET" disabled"#),
        "the permission switches ship inert: {html}"
    );
    for gate in [
        "body.editing .lay__cell--add",
        "body.editing .lay__hint",
        "body.editing .tag--add",
        "body.editing .scene__desc:empty::before",
    ] {
        assert!(STYLE.contains(gate), "{gate} is not gated on the script");
    }
}

/// The `#edit-data` blob is page state rendered as JSON, so `<` must never
/// survive into it: a scene string containing `</script>` would otherwise
/// close the tag and hand the rest of the document to the scene.
#[test]
fn the_edit_data_blob_cannot_be_closed_by_a_scene_string() {
    let st = landing_state(vec![Project {
        root: std::path::PathBuf::from("/tmp/scene"),
        scene_json: json!({
            "display": { "title": "Gather" },
            "tags": ["a</script><script>b"]
        }),
    }]);
    let html = render(
        &st,
        "http://127.0.0.1:8000",
        "",
        "http://127.0.0.1:8000",
        None,
        &Knobs::default(),
    );
    const OPEN: &str = r#"<script type="application/json" id="edit-data">"#;
    let at = html.find(OPEN).expect("the blob is on the page") + OPEN.len();
    let blob = &html[at..][..html[at..].find("</script>").unwrap()];
    assert!(
        !blob.contains('<'),
        "every < is escaped inside the blob: {blob}"
    );
    assert!(
        blob.contains(r"\u003c/script"),
        "the hostile tag rides, escaped: {blob}"
    );
}

#[test]
fn coord_collapses_equal_ranges_and_keeps_spans() {
    assert_eq!(coord(Some(&json!([16, 16]))), "16");
    assert_eq!(coord(Some(&json!([0, 4]))), "0\u{2013}4");
    assert_eq!(coord(Some(&json!(2.5))), "2.5");
    assert_eq!(coord(None), "0");
}

#[test]
fn layout_grid_accents_the_base_and_rings_the_editable_scene_with_add_cells() {
    let spawns = vec![json!({
        "name": "porch",
        "position": { "x": [16, 16], "y": 0, "z": [16, 16] }
    })];
    let grid = layout_grid(&[(0, 0), (1, 0), (0, 1), (1, 1)], (0, 0), &spawns);
    assert_eq!(grid.matches("lay__cell--in").count(), 3);
    assert_eq!(
        grid.matches("lay__cell--base").count(),
        1,
        "only the base is held"
    );
    assert_eq!(
        grid.matches("lay__cell--add").count(),
        12,
        "one ring of add targets around the 2×2"
    );
    assert_eq!(grid.matches("data-cell=").count(), 16);
    assert_eq!(grid.matches(r#"class="lay__area""#).count(), 1);
    assert!(grid.contains("left:calc(var(--lay-step)*32/16)"), "{grid}");
    assert!(grid.contains("top:calc(var(--lay-step)*32/16)"), "{grid}");
    assert!(grid.contains(">porch</span>"));
    assert!(
        grid.contains(r#"style="--lay-cols:4""#),
        "the ruler and the rows agree on the column count: {grid}"
    );
    assert_eq!(grid.matches("data-x=").count(), 4);
    assert_eq!(grid.matches("data-y=").count(), 4);
}

/// A scene wider than the ring bound gets a plain grid — the add ring is
/// a rendering economy, not a size rule; the layout itself stays editable
/// by removal and through scene.json.
#[test]
fn an_oversized_scene_draws_its_grid_without_add_cells() {
    let strip: Vec<(i64, i64)> = (0..GHOST_RING_SPAN).map(|x| (x, 0)).collect();
    let grid = layout_grid(&strip, (0, 0), &[]);
    assert_eq!(grid.matches("lay__cell--add").count(), 0);
    assert_eq!(grid.matches("data-cell=").count(), strip.len());
    assert!(
        grid.contains(r#"<span data-x="7" data-n="7"></span>"#),
        "a wide grid thins its ruler to every fifth label: {grid}"
    );
    assert!(grid.contains(r#"<span data-x="5" data-n="5">5</span>"#));
}

#[tokio::test]
async fn no_content_url_on_the_page_decodes_back_to_the_scene_directory() {
    let tmp = std::env::temp_dir().join(format!(
        "dcl-one-sdk-landing-leak-{}-{:x}",
        std::process::id(),
        rand::random::<u64>()
    ));
    let root = tmp.join("somebody-scenes").join("gather");
    std::fs::create_dir_all(root.join("assets")).unwrap();
    std::fs::write(root.join("scene.json"), "{}").unwrap();
    std::fs::write(root.join("assets/thumb.png"), b"png").unwrap();
    let st = landing_state(vec![Project {
        root: root.clone(),
        scene_json: json!({
            "display": { "title": "Gather", "navmapThumbnail": "assets/thumb.png" }
        }),
    }]);

    let html = scene_html(&st).await;
    let hashes: Vec<&str> = html
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'))
        .filter(|w| w.starts_with("b64-"))
        .collect();
    assert!(
        !hashes.is_empty(),
        "the thumbnail url is missing, so this test would pass without proving anything"
    );
    for hash in hashes {
        use base64::Engine;
        let payload = hash.strip_prefix("b64-").unwrap();
        let payload = payload.split('.').next().unwrap();
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(payload)
            .expect("a preview hash is base64url, and anyone can decode it");
        let decoded = String::from_utf8_lossy(&bytes).to_string();
        assert!(
            !decoded.contains("somebody-scenes") && !decoded.contains(tmp.to_str().unwrap()),
            "{hash} base64-decodes to {decoded}, handing every visitor the scene's path on disk"
        );
    }
    assert!(!html.contains("somebody-scenes"));
    let _ = std::fs::remove_dir_all(&tmp);
}

/// The permissions tab is the whole schema list as switch rows — a
/// required one on, the rest off, each with the line saying what it
/// grants — plus any key the scene carries that the list does not, so
/// toggling a neighbour cannot silently drop it.
#[test]
fn permission_rows_draw_every_known_toggle_with_its_state() {
    let rows = permission_rows(&json!({ "requiredPermissions": ["USE_FETCH", "CUSTOM_X"] }));
    assert_eq!(
        rows.matches(r#"<label class="lay__perm">"#).count(),
        PERMISSIONS.len() + 1
    );
    assert!(rows
        .contains(r#"<input class="sw" type="checkbox" data-perm="USE_FETCH" checked disabled>"#));
    assert!(
        rows.contains(r#"<input class="sw" type="checkbox" data-perm="USE_WEB3_API" disabled>"#)
    );
    assert!(
        rows.contains(r#"<span class="lay__perm-d">Call external HTTP endpoints</span>"#),
        "every known permission says what granting it allows"
    );
    let custom = rows
        .find(r#"data-perm="CUSTOM_X" checked"#)
        .expect("CUSTOM_X");
    assert!(
        !rows[custom..].contains("lay__perm-d"),
        "an unknown key keeps its raw name and claims nothing"
    );

    let none = permission_rows(&json!({}));
    assert_eq!(
        none.matches(r#"<label class="lay__perm">"#).count(),
        PERMISSIONS.len()
    );
    assert_eq!(none.matches(" checked").count(), 0);
}

/// The layout card in one piece: Info leads the sub-navigation holding the
/// scene hero, each editing tab carries its count, and the rail panes ship
/// with only the Info one visible — the server's no-script default.
#[test]
fn the_layout_card_leads_with_info_and_counts_its_tabs() {
    let scene = json!({
        "worldConfiguration": { "name": "gather.dcl.eth" },
        "requiredPermissions": ["USE_FETCH", "USE_WEBSOCKET"],
    });
    let spawns = vec![json!({ "name": "a", "position": { "x": 1, "y": 0, "z": 1 } })];
    let card = scene_layout_card(
        &scene,
        &[(0, 0), (1, 0)],
        (0, 0),
        &spawns,
        "<article class=\"scene\">HERO</article>",
    );
    assert!(
        card.contains(r#"class="jn lay lay--info" id="scene-layout""#),
        "the server's no-script default is the Info tab: {card}"
    );
    assert!(card
        .contains(r#"data-laytab="info" role="tab" aria-selected="true" disabled>Info</button>"#));
    assert!(card.contains(
        r#"data-laytab="parcels" role="tab" aria-selected="false" disabled>Shape</button>"#
    ));
    assert!(card.contains(
        r#"data-laytab="spawns" role="tab" aria-selected="false" disabled>Spawn points</button>"#
    ));
    assert!(card.contains(
        r#"data-laytab="perms" role="tab" aria-selected="false" disabled>Permissions</button>"#
    ));
    assert!(
        !card.contains("sec__count"),
        "the scene tabs carry no counts in the harmonized design: {card}"
    );
    assert!(
        card.contains(r#"<section class="lay__pane" data-pane="info"><article class="scene">HERO</article></section>"#),
        "the scene hero rides the Info pane: {card}"
    );
    assert!(card.contains(r#"<section class="lay__pane" data-pane="parcels" hidden><span class="knob__k">Scene bounds</span>"#));
    assert!(card.contains(r#"<section class="lay__pane" data-pane="spawns" hidden>"#));
    assert!(card.contains(r#"<section class="lay__pane" data-pane="perms" hidden>"#));
    assert!(card.contains("32 × 16 m"), "{card}");
    assert!(card.contains(r#"<code>0,0 → 1,0</code>"#));
    assert!(card.contains("X 1 · Z 1 · Y 0"), "{card}");
    assert!(!card.contains("<form"), "the card rides no form");
    assert!(
        !card.contains("jn2__head"),
        "the tab strip sits flush at the card top, no head above it"
    );
}

/// A fresh page starts with the local-iteration flags on — a second client
/// beside the running one, straight past the auth screen — and a submitted
/// form still names its own set, so unchecking sticks.
#[test]
fn the_default_knobs_launch_a_second_client_straight_in() {
    let fresh = knobs(None, &[]);
    assert_eq!(
        fresh.opts,
        ["multi-instance", "skip-auth-screen", TERRAIN],
        "the fresh page also keeps the terrain the client draws by default"
    );
    assert_eq!(
        fresh.tokens(Carry::Loopback),
        ["--multi-instance=true", "--skip-auth-screen=true"],
        "the fresh page's launch link carries both; checked terrain is the client's own default"
    );
    assert!(
        knobs(Some("where=desktop"), &[]).opts.is_empty(),
        "a submitted form names its own set"
    );
}
