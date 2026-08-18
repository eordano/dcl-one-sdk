//! Human-facing landing page served at `/` for browser requests.
//!
//! No JavaScript and no form anywhere, by test: every affordance is an `<a>`,
//! a `<details>` or a `:hover`, and the stylesheet is inlined.
//!
//! Hence deploy prints a command rather than offering a button: a publish
//! route would be an unauthenticated loopback POST, and any page open in
//! another tab can aim a cross-origin form at loopback — a stray click
//! elsewhere would sign and publish the developer's scene.

use super::{forwarded_host, forwarded_prefix, forwarded_proto, AppState};
use crate::joinblock::{self, desktop_deep_link, mobile_deep_link, scene_title, web_join_url};
use crate::netinfo;
use crate::scene::b64_content_hash;
use axum::http::{header, HeaderMap};
use axum::response::{IntoResponse, Response};
use serde_json::Value;

pub(super) fn page(st: &AppState, headers: &HeaderMap) -> Response {
    let host = forwarded_host(headers).unwrap_or_else(|| {
        headers
            .get(header::HOST)
            .and_then(|h| h.to_str().ok())
            .unwrap_or("127.0.0.1")
            .to_string()
    });
    let proto = forwarded_proto(headers);
    let prefix = forwarded_prefix(headers);
    let realm = format!("{proto}://{host}{prefix}");
    let lan_realm =
        netinfo::share_ip(&netinfo::enumerate()).map(|ip| format!("http://{ip}:{}", st.port));
    let mobile_realm = match &lan_realm {
        Some(lan) if host.starts_with("127.") || host.starts_with("localhost") => lan.clone(),
        _ => realm.clone(),
    };
    let html = render(st, &realm, &prefix, &mobile_realm, lan_realm.as_deref());
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8".to_string()),
            (header::CACHE_CONTROL, "no-cache".to_string()),
        ],
        html,
    )
        .into_response()
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn coord(v: Option<&Value>) -> String {
    match v {
        Some(Value::Array(range)) => {
            let f = |i: usize| range.get(i).and_then(|x| x.as_f64()).unwrap_or(0.0);
            let (a, b) = (f(0), f(1.min(range.len().saturating_sub(1))));
            if a == b {
                trim_num(a)
            } else {
                format!("{}\u{2013}{}", trim_num(a), trim_num(b))
            }
        }
        Some(v) => trim_num(v.as_f64().unwrap_or(0.0)),
        None => "0".to_string(),
    }
}

fn coord_mid(v: Option<&Value>) -> f64 {
    match v {
        Some(Value::Array(range)) => {
            let f = |i: usize| range.get(i).and_then(|x| x.as_f64()).unwrap_or(0.0);
            (f(0) + f(1.min(range.len().saturating_sub(1)))) / 2.0
        }
        Some(v) => v.as_f64().unwrap_or(0.0),
        None => 0.0,
    }
}

fn trim_num(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

fn permission_label(key: &str) -> &str {
    match key {
        "USE_WEBSOCKET" => "websockets",
        "USE_FETCH" => "http fetch",
        "USE_WEB3_API" => "web3 wallet",
        "OPEN_EXTERNAL_LINK" => "open links",
        "ALLOW_TO_MOVE_PLAYER_INSIDE_SCENE" => "move player",
        "ALLOW_TO_TRIGGER_AVATAR_EMOTE" => "trigger emotes",
        "ALLOW_MEDIA_HOSTNAMES" => "external media",
        other => other,
    }
}

fn parse_parcels(scene_json: &Value) -> (Vec<(i64, i64)>, (i64, i64)) {
    let parcels: Vec<(i64, i64)> = scene_json
        .get("scene")
        .and_then(|s| s.get("parcels"))
        .and_then(|p| p.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .filter_map(catalyrst_types::pointer::parse_pointer)
                .collect()
        })
        .unwrap_or_default();
    let base = scene_json
        .get("scene")
        .and_then(|s| s.get("base"))
        .and_then(|b| b.as_str())
        .and_then(catalyrst_types::pointer::parse_pointer)
        .unwrap_or_else(|| parcels.first().copied().unwrap_or((0, 0)));
    (parcels, base)
}

fn parcels_svg(parcels: &[(i64, i64)], base: (i64, i64), spawns: &[Value]) -> String {
    if parcels.is_empty() {
        return String::new();
    }
    const CELL: i64 = 40;
    const GAP: i64 = 4;
    let min_x = parcels.iter().map(|p| p.0).min().unwrap();
    let max_x = parcels.iter().map(|p| p.0).max().unwrap();
    let min_y = parcels.iter().map(|p| p.1).min().unwrap();
    let max_y = parcels.iter().map(|p| p.1).max().unwrap();
    let w = (max_x - min_x + 1) * (CELL + GAP) - GAP;
    let h = (max_y - min_y + 1) * (CELL + GAP) - GAP;
    let mut svg = format!(
        r#"<svg width="{w}" height="{h}" viewBox="0 0 {w} {h}" xmlns="http://www.w3.org/2000/svg" role="img" aria-label="parcel layout">"#
    );
    for (x, y) in parcels {
        let px = (x - min_x) * (CELL + GAP);
        let py = (max_y - y) * (CELL + GAP);
        let is_base = (*x, *y) == base;
        let fill = if is_base { "#ff2d55" } else { "#2a2b37" };
        let label_fill = if is_base { "#fff" } else { "#8f8ba3" };
        svg.push_str(&format!(
            r#"<rect x="{px}" y="{py}" width="{CELL}" height="{CELL}" rx="6" fill="{fill}"/>"#
        ));
        svg.push_str(&format!(
            r##"<text x="{tx}" y="{ty}" text-anchor="middle" dominant-baseline="central" font-family="ui-monospace, Menlo, monospace" font-size="11" fill="{label_fill}">{x},{y}</text>"##,
            tx = px + CELL / 2,
            ty = py + CELL / 2,
        ));
    }
    for spawn in spawns {
        let pos = spawn.get("position");
        let sx = coord_mid(pos.and_then(|p| p.get("x")));
        let sz = coord_mid(pos.and_then(|p| p.get("z")));
        let gx = base.0 as f64 + sx / 16.0 - min_x as f64;
        let gy = base.1 as f64 + sz / 16.0 - min_y as f64;
        let cx = gx * (CELL + GAP) as f64;
        let cy = h as f64 - gy * (CELL + GAP) as f64;
        svg.push_str(&format!(
            r##"<circle cx="{cx:.1}" cy="{cy:.1}" r="4" fill="#fff" stroke="#0d0e12" stroke-width="1.5"><title>spawn</title></circle>"##
        ));
    }
    svg.push_str("</svg>");
    svg
}

fn spawn_chips(scene_json: &Value) -> String {
    let Some(spawns) = scene_json.get("spawnPoints").and_then(|s| s.as_array()) else {
        return r#"<span class="chip">default spawn</span>"#.to_string();
    };
    if spawns.is_empty() {
        return r#"<span class="chip">default spawn</span>"#.to_string();
    }
    spawns
        .iter()
        .map(|s| {
            let name = s.get("name").and_then(|n| n.as_str()).unwrap_or("spawn");
            let star = if s.get("default").and_then(|d| d.as_bool()) == Some(true) {
                "\u{2605} "
            } else {
                ""
            };
            let pos = s.get("position");
            let p = |k| coord(pos.and_then(|p: &Value| p.get(k)));
            let mut chip = format!("{star}{} ({}, {}, {})", esc(name), p("x"), p("y"), p("z"));
            if let Some(t) = s.get("cameraTarget") {
                let t = |k| coord(t.get(k));
                chip.push_str(&format!(
                    " \u{2192} looks at ({}, {}, {})",
                    t("x"),
                    t("y"),
                    t("z")
                ));
            }
            format!(r#"<span class="chip">{chip}</span>"#)
        })
        .collect()
}

fn permission_chips(scene_json: &Value) -> String {
    let perms: Vec<&str> = scene_json
        .get("requiredPermissions")
        .and_then(|p| p.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    if perms.is_empty() {
        return r#"<span class="chip dim">none required</span>"#.to_string();
    }
    perms
        .iter()
        .map(|p| {
            format!(
                r#"<span class="chip perm">{}</span>"#,
                esc(permission_label(p))
            )
        })
        .collect()
}

const WORLDS_CONTENT_SERVER: &str = "https://worlds-content-server.decentraland.org";

/// Single-quoted for `/bin/sh`: on macOS every Creator Hub scene root lives
/// under "Application Support", so the pasted command must survive spaces.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Must resolve the same way `deploy::net::resolve_target_from` does, so the
/// command shown is the command that runs.
fn deploy_target(scene_json: &Value, default_target: Option<&str>) -> (String, String) {
    let world = scene_json
        .get("worldConfiguration")
        .and_then(|w| w.get("name"))
        .and_then(|n| n.as_str());
    if let Some(world) = world {
        return (
            format!("world {world} on {WORLDS_CONTENT_SERVER}"),
            format!(" --target-content {WORLDS_CONTENT_SERVER}"),
        );
    }
    match default_target.map(str::trim) {
        Some(target) if !target.is_empty() => (target.to_string(), String::new()),
        _ => (
            "a healthy catalyst from the public Genesis City rotation".to_string(),
            String::new(),
        ),
    }
}

fn thumbnail(st: &AppState, prefix: &str) -> Option<String> {
    let project = st.projects.first()?;
    let rel = project
        .scene_json
        .get("display")
        .and_then(|d| d.get("navmapThumbnail"))
        .and_then(|t| t.as_str())?;
    let abs = project.root.join(rel);
    if !abs.is_file() {
        return None;
    }
    let hash = b64_content_hash(&abs.display().to_string(), &st.machine);
    Some(format!("{prefix}/content/contents/{hash}"))
}

/// Kept out of the `format!` template because every CSS brace would otherwise
/// have to be doubled.
const STYLE: &str = r##"
:root {
  color-scheme: dark light;
  --page: #0e0d11;
  --panel: #161518;
  --panel-2: #242129;
  --wash: #201e25;
  --line: rgba(255,255,255,.08);
  --line-strong: rgba(255,255,255,.1);
  --line-soft: rgba(255,255,255,.06);
  --fill-1: rgba(255,255,255,.04);
  --fill-2: rgba(255,255,255,.06);
  --fill-3: rgba(255,255,255,.08);
  --fill-4: rgba(255,255,255,.1);
  --fill-5: rgba(255,255,255,.12);
  --text: #fcfcfc;
  --ink-85: rgba(255,255,255,.85);
  --ink-7: rgba(255,255,255,.7);
  --ink-6: rgba(255,255,255,.6);
  --ink-45: rgba(255,255,255,.5);
  --brand: #ff2d55;
  --brand-cta: #d80029;
  --brand-hover: #ff4d70;
  --brand-ink: #ff6b87;
  --on-brand: #fff;
  --success: #34ce77;
  --online: #57df41;
  --warning: #fe9c2a;
  --error: #fb3b3b;
  --info: #2196f3;
  --offline: #a09ba8;
  --glass: rgba(13,12,15,.92);
  --shadow-bar: 0 2px 14px rgba(0,0,0,.35);
  --shadow-panel: 0 8px 24px rgba(0,0,0,.35);
  /* the mini-map's literal hexes are counted by a unit test, so recolour it
     here: a `fill` declaration beats the SVG presentation attribute */
  --map-cell: var(--fill-3);
  --map-label: var(--ink-45);
  --map-dot: #fff;
  --map-dot-stroke: var(--page);
  --font-sans: ui-sans-serif, system-ui, -apple-system, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
  --font-mono: ui-monospace, "SFMono-Regular", Menlo, Consolas, monospace;
  --r-control: 10px;
  --r-card: 14px;
  --r-panel: 18px;
  --r-pill: 999px;
  --s-1: 4px;
  --s-1-5: 6px;
  --s-2: 8px;
  --s-2-5: 10px;
  --s-3: 12px;
  --s-4: 16px;
  --s-5: 24px;
  --s-6: 32px;
  --s-7: 40px;
  --s-8: 48px;
  --dur-fast: 140ms;
  --ease-out: cubic-bezier(.16,1,.3,1);
}

@media (prefers-color-scheme: light) {
  :root {
    --page: #f3f2f5;
    --panel: #ffffff;
    --panel-2: #f3f2f5;
    --wash: #f8f7f9;
    --line: rgba(22,21,24,.14);
    --line-strong: rgba(22,21,24,.18);
    --line-soft: rgba(22,21,24,.08);
    --fill-1: rgba(22,21,24,.04);
    --fill-2: rgba(22,21,24,.06);
    --fill-3: rgba(22,21,24,.08);
    --fill-4: rgba(22,21,24,.1);
    --fill-5: rgba(22,21,24,.12);
    --text: rgba(22,21,24,.9);
    --ink-85: rgba(22,21,24,.85);
    --ink-7: rgba(22,21,24,.6);
    --ink-6: rgba(22,21,24,.55);
    --ink-45: rgba(22,21,24,.6);
    --brand-ink: #d80029;
    --glass: rgba(255,255,255,.92);
    --shadow-bar: 0 1px 3px rgba(22,20,26,.08);
    --shadow-panel: 0 1px 3px rgba(22,20,26,.06);
    --map-cell: rgba(22,21,24,.1);
    --map-label: rgba(22,21,24,.6);
    --map-dot: #161518;
    --map-dot-stroke: #fff;
    /* The dark-page hues fail on white — #34ce77 over its own 14% tint is
       1.8:1. Darkened until each clears 4.5:1 against --panel. */
    --success: #0f7a3d;
    --online: #2b7a15;
    --warning: #8a4b00;
    --error: #c11414;
    --info: #0b5fa5;
  }
}

*, *::before, *::after { box-sizing: border-box; }
* { margin: 0; padding: 0; }
html { -webkit-text-size-adjust: 100%; }
body {
  min-height: 100vh;
  background: var(--page);
  color: var(--text);
  font-family: var(--font-sans);
  font-size: 14px;
  line-height: 1.6;
  -webkit-font-smoothing: antialiased;
}
a { color: var(--brand-ink); text-decoration: none; }
a:hover { color: var(--brand-hover); }
code, pre, .mono { font-family: var(--font-mono); font-size: 12px; overflow-wrap: anywhere; }
img, svg { display: block; max-width: 100%; }
::selection { background: rgba(255,45,85,.3); color: var(--text); }
:focus-visible { outline: 2px solid var(--brand); outline-offset: 2px; border-radius: 4px; }
* { scrollbar-width: thin; scrollbar-color: var(--fill-5) transparent; }
*::-webkit-scrollbar { width: 8px; height: 8px; }
*::-webkit-scrollbar-track { background: transparent; }
*::-webkit-scrollbar-thumb { background: var(--fill-5); border-radius: 4px; }

.skip {
  position: absolute; left: -9999px; top: 0; z-index: 200;
  padding: 6px 12px; background: var(--panel); color: var(--text);
  border: 1px solid var(--line); border-radius: var(--r-control);
  font-size: 13px; font-weight: 700;
}
.skip:focus { left: var(--s-2); top: var(--s-2); }

.bar {
  position: sticky; top: 0; z-index: 60;
  display: flex; align-items: center; gap: var(--s-3); flex-wrap: wrap;
  min-height: 60px; padding: var(--s-2-5) var(--s-5);
  background: var(--glass);
  -webkit-backdrop-filter: blur(8px); backdrop-filter: blur(8px);
  border-bottom: 1px solid var(--line); box-shadow: var(--shadow-bar);
}
.bar__mark {
  display: flex; align-items: center; gap: var(--s-2);
  font-size: 12px; font-weight: 700; letter-spacing: .12em;
  text-transform: uppercase; color: var(--text); white-space: nowrap;
}
.bar__dot {
  width: 8px; height: 8px; flex: none; border-radius: var(--r-pill);
  background: var(--online); box-shadow: 0 0 10px rgba(87,223,65,.6);
}
.bar__scene {
  min-width: 0; max-width: 34ch; padding-left: var(--s-3);
  border-left: 1px solid var(--line);
  font-size: 13px; font-weight: 600; color: var(--ink-7);
  white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
}
.dash {
  width: 100%; max-width: 1180px; margin: 0 auto;
  padding: var(--s-5) var(--s-5) var(--s-7);
  display: flex; flex-direction: column; gap: var(--s-6);
}

.panel {
  padding: var(--s-4); background: var(--panel);
  border: 1px solid var(--line); border-radius: var(--r-card);
}
.sec { display: flex; flex-direction: column; gap: var(--s-3); }
/* 60px is .bar's min-height: sticky, so the skip-link target lands under it */
.sec[id] { scroll-margin-top: calc(60px + var(--s-3)); }
.sec__head { display: flex; align-items: baseline; gap: var(--s-2-5); flex-wrap: wrap; }
h2, h3, .panel > h3 {
  font-size: 12px; font-weight: 700; line-height: 1.4; letter-spacing: .07em;
  text-transform: uppercase; color: var(--ink-45);
}
.sec__count {
  font-size: 11px; font-weight: 700; letter-spacing: .05em;
  color: var(--ink-45); font-variant-numeric: tabular-nums;
}
.sec__sub, .note {
  max-width: 72ch; font-size: 12px; line-height: 1.6; color: var(--ink-45);
}
.row { display: flex; flex-direction: column; gap: var(--s-2); }
.row h3 { margin-bottom: 2px; }

.scene {
  display: grid; grid-template-columns: 132px minmax(0,1fr); gap: var(--s-4);
  padding: var(--s-4); background: var(--panel);
  border: 1px solid var(--line); border-radius: var(--r-card);
}
.cover {
  width: 100%; aspect-ratio: 16 / 10; object-fit: cover;
  border: 1px solid var(--line); border-radius: var(--r-control);
  background: var(--fill-2);
}
.cover.placeholder {
  display: flex; align-items: center; justify-content: center; border: 0;
  background: linear-gradient(160deg, #3a1660 0%, #25103f 55%, #1a0c2e 100%);
}
.cover.placeholder span {
  font-size: 34px; font-weight: 700; line-height: 1;
  letter-spacing: -.02em; color: rgba(255,255,255,.85);
}
.scene__body { min-width: 0; display: flex; flex-direction: column; gap: var(--s-1-5); }
.eyebrow {
  font-size: 11px; font-weight: 700; letter-spacing: .14em;
  text-transform: uppercase; color: var(--brand-ink);
}
.scene__title {
  font-size: 24px; font-weight: 700; line-height: 1.15;
  letter-spacing: -.015em; color: var(--text); overflow-wrap: anywhere;
}
.pos {
  font-size: 12px; font-weight: 600; letter-spacing: .04em;
  text-transform: uppercase; color: var(--ink-45);
  font-variant-numeric: tabular-nums;
}
.pos code { font-size: 11px; letter-spacing: 0; text-transform: none; color: var(--ink-7); }
.scene p {
  max-width: 68ch; font-size: 13px; line-height: 1.65; color: var(--ink-6);
  display: -webkit-box; -webkit-line-clamp: 3; -webkit-box-orient: vertical;
  overflow: hidden;
}
.tags { display: flex; flex-wrap: wrap; gap: var(--s-1-5); margin-top: 2px; }
.tag {
  display: inline-flex; align-items: center; padding: 2px 9px;
  border: 1px solid var(--line); border-radius: var(--r-pill);
  background: var(--fill-2); color: var(--ink-7);
  font-size: 11px; font-weight: 600; line-height: 1.5; white-space: nowrap;
}

.join {
  display: grid; grid-template-columns: repeat(auto-fit, minmax(260px,1fr));
  gap: var(--s-3);
}
.jn {
  display: flex; flex-direction: column; gap: var(--s-1-5); min-width: 0;
  padding: var(--s-3) var(--s-4) var(--s-4);
  background: var(--panel); border: 1px solid var(--line);
  border-radius: var(--r-card); color: var(--text);
  transition: background var(--dur-fast) var(--ease-out),
    border-color var(--dur-fast) var(--ease-out),
    box-shadow var(--dur-fast) var(--ease-out);
}
.jn:hover {
  background: var(--panel-2); border-color: var(--ink-45);
  color: var(--text); text-decoration: none;
}
.jn__label {
  display: flex; align-items: center; gap: var(--s-2);
  font-size: 11px; font-weight: 700; letter-spacing: .07em;
  text-transform: uppercase; color: var(--ink-45);
}
.jn__hint { font-size: 12px; line-height: 1.5; color: var(--ink-6); }
.jn__url {
  display: block; margin-top: auto; padding: var(--s-1-5) var(--s-2);
  background: var(--fill-1); border: 1px solid var(--line-soft);
  border-radius: var(--r-control);
  font-family: var(--font-mono); font-size: 11px; line-height: 1.5;
  color: var(--ink-7); overflow-wrap: anywhere;
  -webkit-user-select: all; user-select: all;
}
.jn:hover .jn__url { color: var(--ink-85); }
.jn--primary {
  border-color: color-mix(in srgb, var(--brand) 45%, transparent);
  background: linear-gradient(180deg,
      color-mix(in srgb, var(--brand) 10%, transparent) 0%, transparent 60%),
    var(--panel);
  box-shadow: 0 0 0 1px rgba(255,45,85,.12);
}
.jn--primary .jn__label { color: var(--brand-ink); }
.jn--primary:hover {
  border-color: var(--brand);
  box-shadow: 0 0 0 1px rgba(255,45,85,.4), 0 4px 20px rgba(255,45,85,.25);
}
.jn__cta {
  display: inline-flex; align-items: center; justify-content: center;
  gap: var(--s-2); align-self: flex-start; height: 34px; padding: 0 18px;
  border-radius: var(--r-pill); background: var(--brand-cta); color: #fff;
  font-size: 12px; font-weight: 700; letter-spacing: .04em;
  text-transform: uppercase; white-space: nowrap;
}
.jn:hover .jn__cta, .jn__cta:hover { background: var(--brand-hover); color: var(--on-brand); }
.jn__cta--ghost { background: transparent; border: 1px solid var(--brand); color: var(--brand); }
.jn:hover .jn__cta--ghost, .jn__cta--ghost:hover {
  background: color-mix(in srgb, var(--brand) 12%, transparent); color: var(--brand);
}
.jn--qr {
  display: grid; grid-template-columns: minmax(0,1fr) auto;
  grid-template-rows: repeat(4, auto); column-gap: var(--s-4); align-items: start;
}
.jn--qr .jn__label, .jn--qr .jn__hint, .jn--qr .jn__cta, .jn--qr .jn__url { grid-column: 1; }
.qr { grid-column: 2; grid-row: 1 / span 4; align-self: center; display: block; line-height: 0; }
.qr img {
  width: 88px; height: 88px; padding: 5px; background: #fff;
  border: 1px solid var(--line); border-radius: var(--r-control);
}

.grid {
  display: grid; grid-template-columns: repeat(auto-fit, minmax(280px,1fr));
  gap: var(--s-4); align-items: start;
}
.grid > * { min-width: 0; }
.span-2 { grid-column: span 2; }

.map {
  display: flex; align-items: center; gap: var(--s-4); flex-wrap: wrap;
  padding: var(--s-2) 0; overflow-x: auto;
}
.map svg { flex: none; }
.map svg rect { fill: var(--map-cell); }
.map svg rect[fill="#ff2d55"] { fill: var(--brand); }
.map svg text { fill: var(--map-label); font-family: var(--font-mono); }
.map svg text[fill="#fff"] { fill: var(--on-brand); }
.map svg circle { fill: var(--map-dot); stroke: var(--map-dot-stroke); }
.map .pos { font-weight: 500; letter-spacing: 0; text-transform: none; color: var(--ink-6); }

.chips { display: flex; flex-wrap: wrap; gap: var(--s-1-5); }
.chip {
  display: inline-flex; align-items: center; padding: 3px var(--s-2-5);
  border: 1px solid var(--line); border-radius: var(--r-pill);
  background: var(--fill-2); color: var(--ink-7);
  font-size: 12px; font-weight: 600; line-height: 1.5;
}
.chip.dim { background: var(--fill-1); color: var(--ink-45); }
.chip.perm {
  color: var(--warning);
  border-color: color-mix(in srgb, var(--warning) 40%, transparent);
  background: color-mix(in srgb, var(--warning) 14%, transparent);
  font-weight: 700;
}
.chip.mono { font-family: var(--font-mono); font-size: 11px; font-weight: 400; }

.kvs {
  display: flex; flex-direction: column; border: 1px solid var(--line);
  border-radius: var(--r-control); overflow: hidden;
}
.kv {
  display: grid; grid-template-columns: minmax(120px,190px) minmax(0,1fr);
  gap: var(--s-2) var(--s-4); align-items: baseline;
  padding: 9px var(--s-3); border-top: 1px solid var(--line-soft);
  font-size: 13px;
}
.kv:first-child { border-top: 0; }
.kv:nth-child(even) { background: var(--fill-1); }
.kv .k {
  font-size: 11px; font-weight: 700; letter-spacing: .05em;
  text-transform: uppercase; color: var(--ink-45); overflow-wrap: anywhere;
}
.kv code {
  font-family: var(--font-mono); font-size: 12px; color: var(--ink-85);
  font-variant-numeric: tabular-nums; overflow-wrap: anywhere;
  -webkit-user-select: all; user-select: all;
}
.kv a code { color: var(--brand-ink); text-decoration: underline; text-underline-offset: 2px; }
.kv a:hover code { color: var(--brand-hover); }

.drawer { background: var(--panel); border: 1px solid var(--line); border-radius: var(--r-card); }
.drawer > summary {
  display: flex; align-items: center; gap: var(--s-2); list-style: none;
  cursor: pointer; padding: var(--s-3) var(--s-4); border-radius: var(--r-card);
  font-size: 12px; font-weight: 700; letter-spacing: .07em;
  text-transform: uppercase; color: var(--ink-45);
  transition: color var(--dur-fast) var(--ease-out),
    background var(--dur-fast) var(--ease-out);
}
.drawer > summary::-webkit-details-marker { display: none; }
.drawer > summary:hover { color: var(--ink-7); background: var(--fill-1); }
.drawer > summary::before {
  content: ""; width: 7px; height: 7px; flex: none; margin-left: 2px;
  border-right: 1.5px solid currentColor; border-bottom: 1.5px solid currentColor;
  transform: rotate(-45deg); transition: transform var(--dur-fast) var(--ease-out);
}
.drawer[open] > summary::before { transform: rotate(45deg); }
.drawer[open] > summary {
  border-radius: var(--r-card) var(--r-card) 0 0;
  border-bottom: 1px solid var(--line);
}
.drawer > summary .sec__count { margin-left: auto; }
.drawer__body { display: flex; flex-direction: column; gap: var(--s-3); padding: var(--s-4); }

.reqs {
  display: flex; flex-direction: column; border: 1px solid var(--line);
  border-radius: var(--r-control); overflow: hidden;
  font-family: var(--font-mono); font-size: 12px; line-height: 1.5;
  color: var(--ink-6); font-variant-numeric: tabular-nums;
}
.reqs > div {
  padding: 5px var(--s-3); border-top: 1px solid var(--line-soft);
  white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
}
.reqs > div:first-child { border-top: 0; }
.reqs > div:nth-child(even) { background: var(--fill-1); }
.reqs > div:hover { background: var(--fill-2); color: var(--ink-85); }
.reqs:empty { display: none; }
.reqs .st { font-weight: 700; color: var(--ink-45); }
.reqs .st--ok { color: var(--success); }
.reqs .st--warn { color: var(--warning); }
.reqs .st--err { color: var(--error); }

.cmd {
  display: block; padding: var(--s-2) var(--s-2-5);
  background: var(--fill-1); border: 1px solid var(--line-soft);
  border-radius: var(--r-control);
  font-family: var(--font-mono); font-size: 12px; line-height: 1.6;
  color: var(--ink-85); overflow-wrap: anywhere;
  -webkit-user-select: all; user-select: all;
}

.u-sr-only {
  position: absolute; width: 1px; height: 1px; overflow: hidden;
  clip: rect(0 0 0 0); clip-path: inset(50%); white-space: nowrap;
}

@media (max-width: 1024px) {
  .span-2 { grid-column: auto; }
}

@media (max-width: 860px) {
  .dash { gap: var(--s-5); padding: var(--s-4) var(--s-4) var(--s-6); }
  .bar { padding: var(--s-2-5) var(--s-4); }
  .bar__scene { max-width: 22ch; }
  .join, .grid { grid-template-columns: minmax(0,1fr); }
}

@media (max-width: 600px) {
  body { font-size: 13px; }
  .dash { gap: var(--s-4); padding: var(--s-3) var(--s-3) var(--s-6); }
  .bar { gap: var(--s-2); padding: var(--s-2) var(--s-3); }
  .bar__scene { padding-left: var(--s-2); }
  .scene { grid-template-columns: minmax(0,1fr); gap: var(--s-3); padding: var(--s-3); }
  .cover { max-width: 180px; }
  .scene__title { font-size: 20px; }
  .kv { grid-template-columns: minmax(0,1fr); gap: 2px; padding: var(--s-2) var(--s-3); }
  .jn--qr { grid-template-columns: minmax(0,1fr); }
  .qr { grid-column: 1; grid-row: auto; justify-self: start; margin-top: var(--s-2); }
  .map { gap: var(--s-3); }
}

@media (pointer: coarse) {
  .jn, .jn__cta, .drawer > summary { min-height: 40px; }
}

@media (prefers-reduced-motion: reduce) {
  *, *::before, *::after {
    animation-duration: .001ms !important;
    animation-iteration-count: 1 !important;
    transition-duration: .001ms !important;
    scroll-behavior: auto !important;
  }
}

@media print {
  .bar {
    position: static; box-shadow: none;
    -webkit-backdrop-filter: none; backdrop-filter: none;
  }
  .drawer[open] > summary { border-bottom: 1px solid var(--line); }
  .scene, .jn, .panel { break-inside: avoid; }
}
"##;

fn render(
    st: &AppState,
    realm: &str,
    prefix: &str,
    mobile_realm: &str,
    lan_realm: Option<&str>,
) -> String {
    let scene_json = st
        .projects
        .first()
        .map(|p| p.scene_json.clone())
        .unwrap_or_default();
    let title = scene_title(&scene_json);
    let description = scene_json
        .get("display")
        .and_then(|d| d.get("description"))
        .and_then(|d| d.as_str())
        .unwrap_or("");
    let description_block = match description.is_empty() {
        true => String::new(),
        false => format!("<p>{}</p>", esc(description)),
    };
    let tags: String = scene_json
        .get("tags")
        .and_then(|t| t.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(|t| format!(r#"<span class="tag">{}</span>"#, esc(t)))
                .collect()
        })
        .unwrap_or_default();
    let tags_block = match tags.is_empty() {
        true => String::new(),
        false => format!(r#"<div class="tags">{tags}</div>"#),
    };
    let position = st.base;
    let (parcels, base) = parse_parcels(&scene_json);
    let spawns: Vec<Value> = scene_json
        .get("spawnPoints")
        .and_then(|s| s.as_array())
        .cloned()
        .unwrap_or_default();
    let map_svg = parcels_svg(&parcels, base, &spawns);

    let ab = match st.local_ab {
        true => None,
        false => st.optimized_assets_url.get().map(String::as_str),
    };
    let desktop = desktop_deep_link(realm, position, ab, &st.deep_link_extra);
    let desktop2 = desktop_deep_link(
        realm,
        position,
        ab,
        &format!("&multi-instance=true{}", st.deep_link_extra),
    );
    let mobile = mobile_deep_link(mobile_realm, position);

    let hero_media = match thumbnail(st, prefix) {
        Some(src) => format!(r#"<img class="cover" src="{}" alt="">"#, esc(&src)),
        None => {
            let initial = title.chars().next().unwrap_or('D').to_uppercase();
            format!(r#"<div class="cover placeholder"><span>{initial}</span></div>"#)
        }
    };
    let qr_img = joinblock::qr_svg_data_url(&mobile)
        .map(|qr| {
            format!(r#"<span class="qr"><img src="{qr}" alt="" width="96" height="96"></span>"#)
        })
        .unwrap_or_default();

    let world_note = scene_json
        .get("worldConfiguration")
        .and_then(|w| w.get("name"))
        .and_then(|n| n.as_str())
        .map(|n| {
            format!(
                r#"<span class="pos">world <code>{}</code> · scene-local coordinates</span>"#,
                esc(n)
            )
        })
        .unwrap_or_default();

    let kv = |k: &str, v: String| {
        format!(
            r#"<div class="kv"><span class="k">{}</span>{v}</div>"#,
            esc(k)
        )
    };
    let join_card = |class: &str,
                     label: &str,
                     hint: &str,
                     cta_class: &str,
                     cta: &str,
                     url: &str| {
        format!(
            r#"<article class="{class}"><span class="jn__label">{label}</span><span class="jn__hint">{hint}</span><a class="{cta_class}" href="{u}">{cta}</a><span class="jn__url">{u}</span></article>"#,
            u = esc(url)
        )
    };
    let web_explorer = joinblock::web_explorer_base();
    let web_card = join_card(
        "jn",
        "web explorer",
        "no install — runs in this browser",
        "jn__cta jn__cta--ghost",
        "open",
        &web_join_url(&web_explorer, realm, position),
    );
    let network_card = lan_realm
        .map(|lan| {
            let lan_host = lan
                .trim_start_matches("http://")
                .rsplit_once(':')
                .map(|(host, _)| host)
                .unwrap_or(lan);
            let lan_assets = ab.map(|u| joinblock::swap_url_host(u, lan_host));
            join_card(
                "jn",
                "network · another device",
                "same wi-fi, different machine",
                "jn__cta jn__cta--ghost",
                "launch",
                &desktop_deep_link(lan, position, lan_assets.as_deref(), &st.deep_link_extra),
            )
        })
        .unwrap_or_default();
    let qr_card = format!(
        r#"<article class="jn jn--qr"><span class="jn__label">qr / mobile app</span><span class="jn__hint">scan with the phone camera</span><a class="jn__cta jn__cta--ghost" href="{m}" title="open in the Decentraland mobile app">open</a><span class="jn__url">{m}</span>{qr_img}</article>"#,
        m = esc(&mobile)
    );
    let join_cards = format!(
        "{}{}{network_card}{web_card}{qr_card}",
        join_card(
            "jn jn--primary",
            "desktop app",
            "the installed explorer, this realm",
            "jn__cta",
            "launch",
            &desktop,
        ),
        join_card(
            "jn",
            "desktop · 2nd instance",
            "a second client beside the first",
            "jn__cta jn__cta--ghost",
            "launch",
            &desktop2,
        ),
    );

    let default_target = std::env::var("DCL_ONE_SDK_DEFAULT_TARGET").ok();
    let (deploy_dest, deploy_flags) = deploy_target(&scene_json, default_target.as_deref());
    let deploy_dir = st
        .projects
        .first()
        .map(|p| sh_quote(&p.root.display().to_string()))
        .unwrap_or_else(|| ".".to_string());
    let deploy_cmd = format!("dcl-one-sdk deploy --dir {deploy_dir}{deploy_flags}");
    let deploy_rows: String = [
        kv("scene", format!("<code>{}</code>", esc(&title))),
        kv(
            "publishes to",
            format!("<code>{}</code>", esc(&deploy_dest)),
        ),
    ]
    .into_iter()
    .collect();

    let requests: Vec<String> = st
        .recent_requests
        .lock()
        .map(|recent| {
            recent
                .iter()
                .rev()
                .take(10)
                .map(|(line, status, at)| {
                    let secs = at.elapsed().as_secs();
                    let ago = if secs < 60 {
                        format!("{secs}s ago")
                    } else {
                        format!("{}m ago", secs / 60)
                    };
                    let tone = match *status {
                        s if s >= 500 => "st--err",
                        s if s >= 400 => "st--warn",
                        _ => "st--ok",
                    };
                    format!(
                        r#"<div><b class="st {tone}">{status}</b> {} · {ago}</div>"#,
                        esc(line)
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let request_count = requests.len();
    let request_rows: String = requests.concat();

    let more_scenes = if st.projects.len() > 1 {
        let rest: String = st.projects[1..]
            .iter()
            .map(|p| {
                let (parcels, _) = parse_parcels(&p.scene_json);
                format!(
                    r#"<span class="chip">{} · {} parcels</span>"#,
                    esc(&scene_title(&p.scene_json)),
                    parcels.len()
                )
            })
            .collect();
        format!(
            r#"<details class="drawer"><summary>also in this realm<span class="sec__count">{}</span></summary><div class="drawer__body"><div class="chips">{rest}</div></div></details>"#,
            st.projects.len() - 1
        )
    } else {
        String::new()
    };
    let map_row = match map_svg.is_empty() && world_note.is_empty() {
        true => String::new(),
        false => format!(r#"<h3>parcels</h3><div class="map">{map_svg}{world_note}</div>"#),
    };
    let parcels_panel = format!(
        r#"<div class="panel span-2"><div class="row">{map_row}<h3>spawn points</h3><div class="chips">{spawn_chips}</div></div></div>"#,
        spawn_chips = spawn_chips(&scene_json),
    );

    let prefix_esc = esc(prefix);
    let page_row = |label: &str, path: &str, note: &str| {
        kv(
            label,
            format!(
                r#"<a href="{prefix_esc}{path}"><code>{path}</code></a> <span class="note">{note}</span>"#
            ),
        )
    };
    let mut page_rows = vec![
        page_row(
            "realm handshake",
            "/about",
            "what the explorer reads to enter",
        ),
        page_row("scene manifest", "/scene.json", "this scene's scene.json"),
        page_row("scene list", "/scenes", "explorer compatibility stub"),
        page_row(
            "local wearables",
            "/preview-wearables",
            "wearables found in this project",
        ),
    ];
    if lan_realm.is_some() {
        page_rows.push(page_row(
            "mobile deep link",
            "/mobile-preview",
            "the link behind the qr, as json",
        ));
    }
    if st
        .data_layer
        .as_ref()
        .is_some_and(|dl| dl.public_dir.is_some())
    {
        page_rows.push(page_row(
            "visual editor",
            "/inspector/",
            "the @dcl/inspector ui on this scene",
        ));
    }
    let page_rows: String = page_rows.concat();

    format!(
        r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title_esc} — dcl-one-sdk preview</title>
<style>{STYLE}</style>
</head>
<body>
<a class="skip" href="#join">skip to join links</a>
<h1 class="u-sr-only">{title_esc} &mdash; dcl-one-sdk preview</h1>
<header class="bar">
  <div class="bar__mark"><span class="bar__dot"></span>dcl one sdk · preview</div>
  <div class="bar__scene">{title_esc}</div>
</header>
<main class="dash">
  <article class="scene">
    {hero_media}
    <div class="scene__body">
      <div class="eyebrow">preview scene</div>
      <h2 class="scene__title">{title_esc}</h2>
      <div class="pos">at {x},{y} · {parcel_count} parcel{parcel_plural}</div>
      {description_block}
      {tags_block}
    </div>
  </article>

  <section id="join" class="sec">
    <div class="sec__head"><h2>join this preview</h2>
      <span class="sec__sub">the button launches · the url below it selects whole on a click,
        ready to copy</span></div>
    <div class="join">{join_cards}</div>
  </section>

  <section class="grid">
    {parcels_panel}
    <div class="panel"><h3>permissions</h3><div class="chips">{permission_chips}</div></div>
    {more_scenes}
  </section>

  <details class="drawer"><summary>recent requests<span class="sec__count">{request_count}</span></summary>
    <div class="drawer__body"><div class="reqs">{request_rows}</div></div></details>

  <section id="pages" class="sec">
    <div class="sec__head"><h2>pages on this server</h2>
      <span class="sec__sub">everything else this preview serves on this port</span></div>
    <div class="panel"><div class="kvs">{page_rows}</div></div>
  </section>

  <section id="deploy" class="sec">
    <div class="sec__head"><h2>deploy</h2>
      <span class="sec__sub">publish this scene from the terminal</span></div>
    <div class="panel"><div class="row">
      <div class="kvs">{deploy_rows}</div>
      <code class="cmd">{deploy_cmd_esc}</code>
      <span class="note">signing opens your wallet, so this runs where you are — the preview
        server has no publish route of its own. The command starts a signing page of its own on
        a throwaway port and shuts it down once the wallet answers, so it is not listed above.
        Add <code>--dry-run</code> to pack and hash the entity without touching the network.</span>
    </div></div>
  </section>
</main>
</body>
</html>
"##,
        title_esc = esc(&title),
        deploy_cmd_esc = esc(&deploy_cmd),
        x = position.0,
        y = position.1,
        parcel_count = parcels.len().max(1),
        parcel_plural = if parcels.len() == 1 { "" } else { "s" },
        permission_chips = permission_chips(&scene_json),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn esc_neutralizes_html() {
        assert_eq!(esc(r#"<b a="1">&x"#), "&lt;b a=&quot;1&quot;&gt;&amp;x");
    }

    #[test]
    fn coord_collapses_equal_ranges_and_keeps_spans() {
        assert_eq!(coord(Some(&json!([16, 16]))), "16");
        assert_eq!(coord(Some(&json!([0, 4]))), "0\u{2013}4");
        assert_eq!(coord(Some(&json!(2.5))), "2.5");
        assert_eq!(coord(None), "0");
    }

    #[test]
    fn parcels_svg_accents_the_base_and_dots_spawns() {
        let spawns = vec![json!({ "position": { "x": [16, 16], "y": 0, "z": [16, 16] } })];
        let svg = parcels_svg(&[(0, 0), (1, 0), (0, 1), (1, 1)], (0, 0), &spawns);
        assert_eq!(svg.matches("<rect").count(), 4);
        assert_eq!(svg.matches("#ff2d55").count(), 1);
        assert_eq!(svg.matches("<circle").count(), 1);
    }

    #[test]
    fn sh_quote_survives_spaces_and_apostrophes() {
        assert_eq!(
            sh_quote("/Application Support/x"),
            "'/Application Support/x'"
        );
        assert_eq!(sh_quote("/it's/here"), r"'/it'\''s/here'");
    }

    #[test]
    fn deploy_target_names_the_worlds_server_for_a_world_scene() {
        let world = json!({ "worldConfiguration": { "name": "my.dcl.eth" } });
        let (dest, flags) = deploy_target(&world, None);
        assert!(dest.contains("my.dcl.eth"));
        assert_eq!(flags, format!(" --target-content {WORLDS_CONTENT_SERVER}"));
        assert_eq!(deploy_target(&world, Some("https://example.org")).1, flags);
        assert!(deploy_target(&json!({}), None).0.contains("Genesis City"));
        assert_eq!(
            deploy_target(&json!({}), Some(" https://example.org ")),
            ("https://example.org".to_string(), String::new())
        );
    }

    #[test]
    fn permission_chips_label_known_keys() {
        let chips = permission_chips(&json!({ "requiredPermissions": ["USE_FETCH", "CUSTOM_X"] }));
        assert!(chips.contains("http fetch"));
        assert!(chips.contains("CUSTOM_X"));
        assert!(permission_chips(&json!({})).contains("none required"));
    }
}
