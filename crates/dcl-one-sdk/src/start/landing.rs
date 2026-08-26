//! Human-facing landing page served at `/` for browser requests.
//!
//! The page is server-rendered and its one form is still a GET back to
//! itself; the stylesheet is inlined. What it now ships beyond that is its
//! own script ([`SCRIPT`]), which turns the scene card into scene.json's
//! editor — click the title, description, tags or thumbnail to change them,
//! toggle parcels, permissions and spawn points in place — and auto-submits
//! the knob form so a choice applies on click.
//!
//! The enhancement is progressive: the server ships every editor control
//! inert (no contenteditable, disabled buttons, add-affordances gated behind
//! the `editing` class the script puts on the body), so without JavaScript
//! the page degrades to the read-only page it used to be — the Apply button
//! still submits the GET form, and nothing promises a click it cannot honour.
//!
//! Every mutation the script makes is a fetch POST to `/scene-json` or
//! `/scene-thumbnail`, and those routes are registered beside `/deploy`
//! outside the permissive CORS layer, behind the loopback + same-origin gates
//! described in `edit.rs`. Nothing on this page POSTs as a form, so a stray
//! cross-origin click still cannot make this server do anything — and
//! `/deploy` keeps printing a command rather than growing a button here.

use super::{forwarded_host, forwarded_prefix, forwarded_proto, AppState};
use crate::joinblock::{self, desktop_deep_link, mobile_deep_link, scene_title, web_join_url};
use crate::netinfo;
use crate::scene::b64_content_hash;
use axum::http::{header, HeaderMap};
use axum::response::{IntoResponse, Response};
use serde_json::Value;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The boolean deep-link params this page offers as checkboxes, with what each
/// one does to the launch.
///
/// Every key here is on the client's own allowlist (`DeepLinkAllowlist.cs`),
/// which has two tiers: the first four are kept only when the realm is
/// loopback — `ApplicationParametersParser` decides that with `Uri.IsLoopback`
/// and drops the whole tier otherwise — and `force-open-backpack` is kept for
/// any realm. A key in neither tier is dropped for every realm, which is why
/// the rest of the client's ~90 flags are not offered: a knob for one would
/// promise a change the client silently discards.
///
/// The tier matters here because not every target this page offers is a
/// loopback realm: see [`Carry`], which greys out the knobs the selected
/// target's client would throw away.
const TOGGLES: [(&str, &str); 5] = [
    (
        "multi-instance",
        "A second client beside one already running",
    ),
    ("skip-auth-screen", "Straight in on the cached identity"),
    (
        "landscape-terrain-enabled",
        "Draw the terrain around the scene",
    ),
    ("hub", "Mark the session as launched from the Creator Hub"),
    ("force-open-backpack", "Land with the backpack open"),
];

/// How much of the deep link a target's client will actually keep, which is
/// decided by whether its realm is loopback ([`TOGGLES`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Carry {
    /// A loopback realm: the client keeps every knob this page offers.
    Loopback,
    /// A routable realm — the LAN address another device has to dial. The
    /// client drops the loopback-only tier of its allowlist.
    Routable,
    /// A link with no deep-link params at all: the web build reads none of
    /// them, and the mobile link is a realm and a position.
    Nothing,
}

impl Carry {
    fn keeps_toggle(self, key: &str) -> bool {
        match self {
            Carry::Loopback => true,
            Carry::Routable => key == "force-open-backpack",
            Carry::Nothing => false,
        }
    }
    fn keeps_mcp(self) -> bool {
        self == Carry::Loopback
    }
    /// `spawnpoint` is not in the loopback-only tier, so it survives anywhere
    /// the client reads deep-link params at all.
    fn keeps_spawn(self) -> bool {
        self != Carry::Nothing
    }
    /// What to tell someone whose knob is greyed out.
    fn note(self) -> &'static str {
        match self {
            Carry::Loopback => "",
            Carry::Routable => "This target's realm is a LAN address, not loopback, so the client drops these before the session starts",
            Carry::Nothing => "This target reads no deep-link params, so the client never sees these",
        }
    }
}

/// The `where` knob's choices, as the values that ride in the query string.
/// Stable names rather than indices, so a link keeps pointing at the target it
/// named even when the LAN row is missing.
/// How many of the buffered requests the drawer draws. The buffer holds
/// hundreds so the tail covers a whole scene load; a dozen is what someone
/// opening the drawer actually reads, and the rest was a thousand pixels of
/// dev-server log under a page whose subject is a launch button.
const RECENT_REQUESTS_SHOWN: usize = 12;

const WHERE_DESKTOP: &str = "desktop";
const WHERE_LAN: &str = "lan";
const WHERE_WEB: &str = "web";
const WHERE_PHONE: &str = "phone";
const WHERE_KEYS: [&str; 4] = [WHERE_DESKTOP, WHERE_LAN, WHERE_WEB, WHERE_PHONE];

/// The page's own state, carried in the query string because the knobs have no
/// JavaScript to hold it: the one form GETs back here and the server rebuilds
/// the launch link with the choices folded in.
#[derive(Default)]
struct Knobs {
    /// Checked `opt=` boxes, in [`TOGGLES`] order.
    opts: Vec<String>,
    /// A `spawnPoints[].name` from this scene, or empty for the default one.
    spawn: String,
    /// A [`WHERE_KEYS`] entry, or empty for the first target on offer.
    where_key: String,
    /// The mcp radio, or `None` before anyone has touched it — the server's
    /// own `--mcp` decides the default.
    mcp: Option<bool>,
}

impl Knobs {
    /// The `--key=value` tokens these knobs add, in the form
    /// [`joinblock::parse_passthrough_params`] already understands — so the
    /// same rules apply as to `--` params on the command line: a core key
    /// (realm, position, …) or one a flag already set is dropped rather than
    /// allowed to repoint the link.
    ///
    /// A knob the target's client would throw away is left out of the link
    /// rather than sent and silently dropped.
    fn tokens(&self, carry: Carry) -> Vec<String> {
        let mut out: Vec<String> = self
            .opts
            .iter()
            .filter(|o| carry.keeps_toggle(o))
            .map(|o| format!("--{o}=true"))
            .collect();
        if !self.spawn.is_empty() && carry.keeps_spawn() {
            out.push(format!("--spawnpoint={}", self.spawn));
        }
        out
    }
}

/// Every knob on the page comes back through the query string, and nothing
/// else does: a key this page does not own is ignored, and so is a value the
/// page never offered. The page can therefore only ever build a link out of
/// what it drew — there is no free-text field to smuggle a flag through.
fn knobs(query: Option<&str>, spawn_names: &[String]) -> Knobs {
    let Some(query) = query else {
        return Knobs::default();
    };
    let mut knobs = Knobs::default();
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        match key.as_ref() {
            "opt" if TOGGLES.iter().any(|(k, _)| *k == value) => {
                knobs.opts.push(value.into_owned())
            }
            "spawn" if spawn_names.iter().any(|n| *n == value) => knobs.spawn = value.into_owned(),
            "where" if WHERE_KEYS.contains(&value.as_ref()) => knobs.where_key = value.into_owned(),
            "mcp" if value == "on" || value == "off" => knobs.mcp = Some(value == "on"),
            _ => {}
        }
    }
    knobs
}

/// The names this scene gives its spawn points, which is the only thing
/// `spawnpoint` may be set to — the client matches on the name, and a name the
/// scene does not define would land the player at the default anyway.
fn spawn_names(scene_json: &Value) -> Vec<String> {
    scene_json
        .get("spawnPoints")
        .and_then(|s| s.as_array())
        .map(|spawns| {
            spawns
                .iter()
                .filter_map(|s| s.get("name").and_then(|n| n.as_str()))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// A value worth recomputing occasionally but not on every render, kept behind
/// a coarse TTL: the caller pays for a stale answer only for `ttl` after the
/// world changes, and pays nothing the rest of the time.
fn memoised<T: Clone>(
    cell: &Mutex<Option<(Instant, T)>>,
    ttl: Duration,
    compute: impl FnOnce() -> T,
) -> T {
    let Ok(mut slot) = cell.lock() else {
        return compute();
    };
    if let Some((at, value)) = slot.as_ref() {
        if at.elapsed() < ttl {
            return value.clone();
        }
    }
    let value = compute();
    *slot = Some((Instant::now(), value.clone()));
    value
}

/// The same, for a value whose only input is one string: hold the last answer
/// and the key that produced it, so a repeat render is a string compare.
fn memoised_by<T: Clone>(
    cell: &Mutex<Option<(String, T)>>,
    key: &str,
    compute: impl FnOnce() -> T,
) -> T {
    let Ok(mut slot) = cell.lock() else {
        return compute();
    };
    if let Some((cached, value)) = slot.as_ref() {
        if cached == key {
            return value.clone();
        }
    }
    let value = compute();
    *slot = Some((key.to_string(), value.clone()));
    value
}

/// `getifaddrs` is a syscall per render and the interfaces of a laptop change
/// on the scale of minutes, not requests.
fn share_ip() -> Option<std::net::Ipv4Addr> {
    static IFACES: Mutex<Option<(Instant, Option<std::net::Ipv4Addr>)>> = Mutex::new(None);
    memoised(&IFACES, Duration::from_secs(10), || {
        netinfo::share_ip(&netinfo::enumerate())
    })
}

/// Encoding the QR is the most expensive thing on this page by a wide margin,
/// and its input is one link that only moves when the Host header or the
/// scene's base parcel does.
fn qr_data_url(link: &str) -> Option<String> {
    static QR: Mutex<Option<(String, Option<String>)>> = Mutex::new(None);
    memoised_by(&QR, link, || joinblock::qr_svg_data_url(link))
}

pub(super) fn page(st: &AppState, headers: &HeaderMap, query: Option<&str>) -> Response {
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
    let lan_realm = share_ip().map(|ip| format!("http://{ip}:{}", st.port));
    let mobile_realm = match &lan_realm {
        Some(lan) if host.starts_with("127.") || host.starts_with("localhost") => lan.clone(),
        _ => realm.clone(),
    };
    let names = st
        .first_project()
        .map(|p| spawn_names(&p.scene_json))
        .unwrap_or_default();
    let html = render(
        st,
        &realm,
        &prefix,
        &mobile_realm,
        lan_realm.as_deref(),
        &knobs(query, &names),
    );
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8".to_string()),
            (header::CACHE_CONTROL, "no-cache".to_string()),
        ],
        html,
    )
        .into_response()
}

/// One label/value row, the shape every panel on every page uses.
pub(super) fn kv(k: &str, v: String) -> String {
    format!(
        r#"<div class="kv"><span class="k">{}</span>{v}</div>"#,
        esc(k)
    )
}

/// A rendered page, with the headers that keep a preview page from being
/// cached while the scene under it changes.
pub(super) fn html(body: String) -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8".to_string()),
            (header::CACHE_CONTROL, "no-cache".to_string()),
        ],
        body,
    )
        .into_response()
}

pub(super) fn esc(s: &str) -> String {
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

/// Every `requiredPermissions` key the schema defines, with the label the
/// page shows for it. The panel draws all of them as toggles, and the edit
/// endpoint accepts no key outside this list that the scene does not already
/// carry.
pub(super) const PERMISSIONS: [(&str, &str); 7] = [
    ("USE_WEBSOCKET", "Websockets"),
    ("USE_FETCH", "HTTP fetch"),
    ("USE_WEB3_API", "Web3 wallet"),
    ("OPEN_EXTERNAL_LINK", "Open links"),
    ("ALLOW_TO_MOVE_PLAYER_INSIDE_SCENE", "Move player"),
    ("ALLOW_TO_TRIGGER_AVATAR_EMOTE", "Trigger emotes"),
    ("ALLOW_MEDIA_HOSTNAMES", "External media"),
];

pub(super) fn parse_parcels(scene_json: &Value) -> (Vec<(i64, i64)>, (i64, i64)) {
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

/// Past this many cells a side, the map draws the parcels without the add
/// ring. A rendering bound only — the edit endpoint takes any connected
/// layout — because the grid is drawn whole, gaps included, and a huge
/// scene's ghost cells are page weight for clicks nobody makes at that size.
const GHOST_RING_SPAN: i64 = 40;

/// The layout map, and since the page edits the layout, also its control
/// surface: every parcel is a removable cell and every empty cell of the
/// bounding box plus one ring is an add target, tagged `data-parcel` /
/// `data-add` for the script. A scene wider than [`GHOST_RING_SPAN`] draws
/// its parcels alone.
fn parcels_svg(parcels: &[(i64, i64)], base: (i64, i64), spawns: &[Value]) -> String {
    if parcels.is_empty() {
        return String::new();
    }
    const CELL: i64 = 40;
    const GAP: i64 = 4;
    let ring = i64::from(
        (1 + parcels.iter().map(|p| p.0).max().unwrap()
            - parcels.iter().map(|p| p.0).min().unwrap())
        .max(
            1 + parcels.iter().map(|p| p.1).max().unwrap()
                - parcels.iter().map(|p| p.1).min().unwrap(),
        ) < GHOST_RING_SPAN,
    );
    let min_x = parcels.iter().map(|p| p.0).min().unwrap() - ring;
    let max_x = parcels.iter().map(|p| p.0).max().unwrap() + ring;
    let min_y = parcels.iter().map(|p| p.1).min().unwrap() - ring;
    let max_y = parcels.iter().map(|p| p.1).max().unwrap() + ring;
    let w = (max_x - min_x + 1) * (CELL + GAP) - GAP;
    let h = (max_y - min_y + 1) * (CELL + GAP) - GAP;
    let mut svg = format!(
        r#"<svg width="{w}" height="{h}" viewBox="0 0 {w} {h}" xmlns="http://www.w3.org/2000/svg" role="img" aria-label="parcel layout">"#
    );
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let px = (x - min_x) * (CELL + GAP);
            let py = (max_y - y) * (CELL + GAP);
            if !parcels.contains(&(x, y)) {
                if ring == 1 {
                    svg.push_str(&format!(
                        r#"<rect class="map__ghost" x="{px}" y="{py}" width="{CELL}" height="{CELL}" rx="6" data-add="{x},{y}"><title>Add {x},{y}</title></rect>"#
                    ));
                }
                continue;
            }
            let is_base = (x, y) == base;
            let fill = if is_base { "#ff2d55" } else { "#2a2b37" };
            let label_fill = if is_base { "#fff" } else { "#8f8ba3" };
            let verb = if is_base { "Base parcel" } else { "Remove" };
            svg.push_str(&format!(
                r#"<rect class="map__p" x="{px}" y="{py}" width="{CELL}" height="{CELL}" rx="6" fill="{fill}" data-parcel="{x},{y}"{db}><title>{verb} {x},{y}</title></rect>"#,
                db = if is_base { r#" data-base="1""# } else { "" },
            ));
            svg.push_str(&format!(
                r##"<text x="{tx}" y="{ty}" text-anchor="middle" dominant-baseline="central" font-family="ui-monospace, Menlo, monospace" font-size="11" fill="{label_fill}">{x},{y}</text>"##,
                tx = px + CELL / 2,
                ty = py + CELL / 2,
            ));
        }
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

/// Each spawn point as a button that opens its editor, and one more that adds
/// a spawn point.
fn spawn_chips(scene_json: &Value) -> String {
    let spawns = scene_json
        .get("spawnPoints")
        .and_then(|s| s.as_array())
        .map(Vec::as_slice)
        .unwrap_or_default();
    let mut out: String = spawns
        .iter()
        .enumerate()
        .map(|(i, s)| {
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
            format!(
                r#"<button type="button" class="chip spawn" data-spawn="{i}" disabled>{chip}</button>"#
            )
        })
        .collect();
    if spawns.is_empty() {
        out.push_str(r#"<span class="chip dim">Default spawn</span>"#);
    }
    out.push_str(
        r#"<button type="button" class="chip chip--add" id="spawn-add" disabled>+ Add spawn point</button>"#,
    );
    out
}

/// Every known permission as a toggle, on or off, plus any key the scene
/// carries that the schema list does not — shown so toggling a neighbour
/// cannot silently drop it.
fn permission_chips(scene_json: &Value) -> String {
    let required: Vec<&str> = scene_json
        .get("requiredPermissions")
        .and_then(|p| p.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    let chip = |key: &str, label: &str, on: bool| {
        format!(
            r#"<button type="button" class="chip perm{off}" data-perm="{key}" aria-pressed="{on}" disabled>{label}</button>"#,
            off = if on { "" } else { " perm--off" },
            key = esc(key),
            label = esc(label),
        )
    };
    let mut out: String = PERMISSIONS
        .iter()
        .map(|(key, label)| chip(key, label, required.contains(key)))
        .collect();
    for key in required {
        if !PERMISSIONS.iter().any(|(k, _)| *k == key) {
            out.push_str(&chip(key, key, true));
        }
    }
    out
}

const WORLDS_CONTENT_SERVER: &str = "https://worlds-content-server.decentraland.org";

/// Must resolve the same way `deploy::net::resolve_target_from` does, so the
/// command shown is the command that runs.
pub(super) fn deploy_target(scene_json: &Value, default_target: Option<&str>) -> (String, String) {
    let world = scene_json
        .get("worldConfiguration")
        .and_then(|w| w.get("name"))
        .and_then(|n| n.as_str());
    if let Some(target) = default_target.map(str::trim).filter(|t| !t.is_empty()) {
        return (target.to_string(), String::new());
    }
    if let Some(world) = world {
        return (
            format!("World {world} on {WORLDS_CONTENT_SERVER}"),
            format!(" --target-content {WORLDS_CONTENT_SERVER}"),
        );
    }
    (
        "A healthy catalyst from the public Genesis City rotation".to_string(),
        String::new(),
    )
}

fn thumbnail(st: &AppState, prefix: &str) -> Option<String> {
    let project = st.first_project()?;
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
  --page: #0e0d10;
  --panel: #161518;
  --line: rgba(255,255,255,.08);
  /* The one control edge: deployworldview.css `.deploy-world-wizard__btn`. */
  --line-ctl: rgba(255,255,255,.18);
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
  /* White on --brand is 3.65:1. Every filled button with a white label takes
     --brand-cta (5.31:1) instead; --brand stays a border, tint and ring. */
  --brand-cta: #d80029;
  --brand-cta-hover: #b00021;
  --brand-cta-active: #8f001b;
  --brand-hover: #ff4d70;
  --brand-ink: #ff6b87;
  --purple: #982de2;
  --on-brand: #fff;
  --success: #34ce77;
  --online: #57df41;
  --warning: #fe9c2a;
  --error: #fb3b3b;
  --glass: rgba(13,12,15,.92);
  --shadow-bar: 0 2px 14px rgba(0,0,0,.35);
  /* the mini-map's literal hexes are counted by a unit test, so recolour it
     here: a `fill` declaration beats the SVG presentation attribute */
  --map-cell: var(--fill-3);
  /* The selected parcel carries a white label, so it is a filled button by
     another name and takes --brand-cta (5.31:1) rather than --brand (3.65:1). */
  --map-base: var(--brand-cta);
  --map-label: var(--ink-45);
  --map-dot: #fff;
  --map-dot-stroke: var(--page);
  /* No @font-face and no font file to serve, so naming Inter only made the
     page claim a typeface it never loaded. This is what actually draws. */
  --font-sans: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif;
  --font-mono: ui-monospace, "SFMono-Regular", Menlo, Consolas, monospace;
  --r-control: 10px;
  --r-card: 14px;
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
  --s-10: 64px;
  --fs-13: 13px;
  --fs-14: 14px;
  --fs-15: 15px;
  --fs-16: 16px;
  --fs-18: 18px;
  --fs-20: 20px;
  --fs-24: 24px;
  --fs-28: 28px;
  --fs-34: 34px;
  /* One leading per text role; no bare numeric line-height below this block. */
  --lh-flat: 1.1;
  --lh-tight: 1.15;
  --lh-snug: 1.3;
  --lh-ui: 1.45;
  --lh-body: 1.6;
  --lh-code: 1.55;
  --measure: 68ch;
  --measure-narrow: 44ch;
  /* 400/600/700 only: 500 is Roboto's Medium and not SF's, and 800 smears on a
     synthesised fallback at control sizes. */
  --fw-regular: 400;
  --fw-semibold: 600;
  --fw-bold: 700;
  --focus-ring-color: var(--brand);
  --focus-ring-width: 2px;
  --focus-ring-offset: 2px;
  --focus-ring-color-invert: #fff;
  --dur-fast: 140ms;
  --ease-out: cubic-bezier(.16,1,.3,1);
}

@media (prefers-color-scheme: light) {
  :root {
    --page: #f3f2f5;
    --panel: #ffffff;
    --line: rgba(22,21,24,.14);
    --line-ctl: rgba(22,21,24,.18);
    --fill-1: rgba(22,21,24,.04);
    --fill-2: rgba(22,21,24,.06);
    --fill-3: rgba(22,21,24,.08);
    --fill-4: rgba(22,21,24,.1);
    --fill-5: rgba(22,21,24,.12);
    --text: rgba(22,21,24,.9);
    --ink-85: rgba(22,21,24,.85);
    /* Body prose now sits on --page as often as on --panel. At .6 this token
       was 4.25:1 over the page; .68 is 5.66:1 even under the corner radials. */
    --ink-7: rgba(22,21,24,.68);
    /* .knob__note and .chk__w — the copy that says what each knob does — are
       this token over --panel. At .55 it was 4.03:1, under AA; .62 is 5.08:1.
       Dark mode keeps its own .6, which is already 7.09:1. */
    --ink-6: rgba(22,21,24,.62);
    --ink-45: rgba(22,21,24,.6);
    --brand-ink: #d80029;
    --glass: rgba(255,255,255,.92);
    --shadow-bar: 0 1px 3px rgba(22,20,26,.08);
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
  }
}

*, *::before, *::after { box-sizing: border-box; }
* { margin: 0; padding: 0; }
/* The script toggles `hidden` on elements whose classes set a display, and an
   author display beats the UA's [hidden] rule without this. */
[hidden] { display: none !important; }
html { -webkit-text-size-adjust: 100%; }
body {
  min-height: 100vh;
  background: var(--page);
  color: var(--text);
  font-family: var(--font-sans);
  font-size: var(--fs-14);
  line-height: var(--lh-body);
  -webkit-font-smoothing: antialiased;
}
/* The Creator Hub's own field: two corner radials over a flat page, no
   animation — frames/creatorhubchrome.css `.ch`. It rides its own fixed layer
   rather than `background-attachment: fixed` on the body, so scrolling
   composites one plane instead of repainting the viewport. */
body::before {
  content: ""; position: fixed; inset: 0; z-index: -1; pointer-events: none;
  background:
    radial-gradient(120% 60% at 100% 0%, color-mix(in srgb, var(--brand) 8%, transparent), transparent 60%),
    radial-gradient(90% 50% at 0% 0%, color-mix(in srgb, var(--purple) 7%, transparent), transparent 55%);
}
@media (prefers-color-scheme: light) {
  body::before {
    background:
      radial-gradient(120% 60% at 100% 0%, color-mix(in srgb, var(--brand) 5%, transparent), transparent 60%),
      radial-gradient(90% 50% at 0% 0%, color-mix(in srgb, var(--purple) 4%, transparent), transparent 55%);
  }
}
a { color: var(--brand-ink); text-decoration: none; }
a:hover { color: var(--brand-hover); text-decoration: underline; text-underline-offset: 3px; }
code, pre { font-family: var(--font-mono); font-size: var(--fs-13); overflow-wrap: anywhere; }
img, svg { display: block; max-width: 100%; }
::selection { background: rgba(255,45,85,.3); color: var(--text); }
:focus-visible {
  outline: var(--focus-ring-width) solid var(--focus-ring-color);
  outline-offset: var(--focus-ring-offset);
  border-radius: 4px;
}
* { scrollbar-width: thin; scrollbar-color: var(--fill-5) transparent; }
*::-webkit-scrollbar { width: 8px; height: 8px; }
*::-webkit-scrollbar-track { background: transparent; }
*::-webkit-scrollbar-thumb { background: var(--fill-5); border-radius: var(--r-pill); }

.skip {
  position: absolute; left: -9999px; top: 0; z-index: 200;
  padding: var(--s-2) var(--s-3); background: var(--panel); color: var(--text);
  border: 1px solid var(--line); border-radius: var(--r-control);
  font-size: var(--fs-13); font-weight: var(--fw-semibold);
}
.skip:focus { left: var(--s-2); top: var(--s-2); }

/* The bar's gutter tracks the page gutter, so the wordmark sits on the same
   vertical as the page title under it at every width. */
.bar {
  position: sticky; top: 0; z-index: 60;
  display: flex; align-items: center; gap: var(--s-4); flex-wrap: wrap;
  min-height: 60px;
  padding: var(--s-3) max(var(--s-6), calc((100% - 1120px) / 2 + var(--s-6)));
  background: var(--glass);
  -webkit-backdrop-filter: blur(8px); backdrop-filter: blur(8px);
  border-bottom: 1px solid var(--line); box-shadow: var(--shadow-bar);
}
.bar__mark {
  display: flex; align-items: center; gap: var(--s-2);
  font-size: var(--fs-14); font-weight: var(--fw-bold); letter-spacing: 0;
  color: var(--text); white-space: nowrap;
}
.bar__dot {
  width: 8px; height: 8px; flex: none; border-radius: var(--r-pill);
  background: var(--online);
}
.dash {
  width: 100%; max-width: 1120px; margin: 0 auto;
  padding: 80px var(--s-6) 96px;
  display: flex; flex-direction: column; gap: var(--s-10);
}

/* Panels are flat: one hairline, one radius, no wash and no shadow. */
.panel {
  padding: var(--s-6); background: var(--panel);
  border: 1px solid var(--line); border-radius: var(--r-card);
  display: flex; flex-direction: column; gap: var(--s-4);
}
.sec { display: flex; flex-direction: column; gap: var(--s-5); }
/* 60px is .bar's min-height: sticky, so the skip-link target lands under it */
.sec[id] { scroll-margin-top: calc(60px + var(--s-3)); }
.sec__head { display: flex; flex-direction: column; gap: var(--s-1-5); }
h2 {
  font-size: var(--fs-20); font-weight: var(--fw-semibold);
  line-height: var(--lh-snug); letter-spacing: -.01em; color: var(--text);
}
h3 {
  font-size: var(--fs-15); font-weight: var(--fw-semibold);
  line-height: var(--lh-snug); letter-spacing: 0; color: var(--text);
}
.sec__count {
  font-size: var(--fs-13); font-weight: var(--fw-semibold); letter-spacing: 0;
  color: var(--ink-6); font-variant-numeric: tabular-nums;
}
.note, .knob__note, .chk__w {
  max-width: var(--measure); font-size: var(--fs-13);
  line-height: var(--lh-body); color: var(--ink-6);
}
.row { display: flex; flex-direction: column; gap: var(--s-3); }
/* The map is as wide as the scene is, which is rarely the width of the panel —
   the spawn points take the slack beside it rather than under it. */
.row--map {
  display: grid; grid-template-columns: auto minmax(12rem, 1fr);
  gap: var(--s-3) var(--s-6); align-items: start;
}
.row--map > h2 { grid-column: 1; }
.row--map > .map { grid-column: 1; }
/* Under editing the map hint widens the auto column until the spawn column
   sits at its 12rem floor, so the two-column split only earns its keep on the
   widest screens. */
@media (max-width: 860px) { .row--map { grid-template-columns: minmax(0, 1fr); } }

/* The title block sits on the page field, not in a card — only the cover
   image keeps an edge. */
.scene {
  display: grid; grid-template-columns: 260px minmax(0,1fr);
  gap: var(--s-3) var(--s-7); align-items: start;
}
.cover {
  width: 100%; aspect-ratio: 16 / 10; object-fit: cover;
  border: 1px solid var(--line); border-radius: var(--r-card);
  background: var(--fill-2);
}
.cover.placeholder {
  display: flex; align-items: center; justify-content: center; border: 0;
  background: linear-gradient(160deg, #3a1660 0%, #25103f 55%, #1a0c2e 100%);
}
.cover.placeholder span {
  font-size: var(--fs-34); font-weight: var(--fw-bold); line-height: var(--lh-flat);
  letter-spacing: -.02em; color: rgba(255,255,255,.85);
}
.scene__body { min-width: 0; display: flex; flex-direction: column; gap: var(--s-2); }
/* Decentraland puts the name of the thing at the top, big, with nothing over
   it. `.page__title` is the same block for a page that is not a scene card. */
.scene__title, .page__title {
  font-size: var(--fs-34); font-weight: var(--fw-bold);
  line-height: var(--lh-tight); letter-spacing: -.02em;
  color: var(--text); overflow-wrap: anywhere;
}
.page__sub {
  max-width: var(--measure); font-size: var(--fs-15);
  line-height: var(--lh-body); color: var(--ink-7);
}
.pos {
  font-size: var(--fs-14); font-weight: var(--fw-regular); letter-spacing: 0;
  color: var(--ink-7); font-variant-numeric: tabular-nums;
}
.pos code { font-size: var(--fs-13); color: var(--ink-85); }
/* No clamp: the description is shown whole, and edited where it is shown. */
.scene p {
  max-width: var(--measure); font-size: var(--fs-15);
  line-height: var(--lh-body); color: var(--ink-7); white-space: pre-wrap;
}
body.editing .scene__desc:empty::before { content: attr(data-hint); color: var(--ink-45); }
.tags { display: flex; flex-wrap: wrap; gap: var(--s-2); margin-top: var(--s-1); }

/* A number is the thing; its caption sits over it, quieter and smaller —
   creatorhub/components/datumtile.css, stepped down one. */
.datum { display: flex; flex-direction: column; gap: var(--s-1-5); min-width: 0; }
.datum__cap {
  font-size: var(--fs-13); font-weight: var(--fw-semibold); letter-spacing: 0;
  color: var(--ink-6);
}
.datum__v { display: flex; align-items: baseline; flex-wrap: wrap; gap: var(--s-2); }
.datum__num {
  font-size: var(--fs-24); font-weight: var(--fw-bold);
  line-height: var(--lh-flat); letter-spacing: -.02em; color: var(--text);
  font-variant-numeric: tabular-nums;
}
.datum__unit {
  font-size: var(--fs-14); font-weight: var(--fw-semibold); color: var(--ink-6);
}

/* One card is the whole join surface: header, target tabs, knobs, footer. */
.jn {
  display: flex; flex-direction: column; min-width: 0;
  background: var(--panel); border: 1px solid var(--line);
  border-radius: var(--r-card); color: var(--text); overflow: hidden;
}
.jn > .side { display: flex; flex-direction: column; min-width: 0; }
.jn2__head {
  display: flex; align-items: flex-start; justify-content: space-between;
  gap: var(--s-4); flex-wrap: wrap; padding: var(--s-5) var(--s-6);
}
.jn2__title { display: flex; flex-direction: column; gap: var(--s-1-5); min-width: 0; }
.jn2__head h2 { font-size: var(--fs-24); letter-spacing: -.01em; }
.jn__hint {
  max-width: var(--measure); font-size: var(--fs-15);
  line-height: var(--lh-body); color: var(--ink-7);
}
.jn2__host {
  display: inline-flex; align-items: center; gap: var(--s-2); flex: none;
  padding: var(--s-2) var(--s-4); border-radius: var(--r-pill);
  background: var(--fill-2); border: 1px solid var(--line);
  font-size: var(--fs-14); font-weight: var(--fw-semibold); color: var(--text);
}
/* The tabs are the where-radio drawn as a bar: the input is appearance-none
   and stretched over its whole label, so the tab itself is the focusable,
   clickable control and the focus ring lands on it. */
.knob--tabs { display: block; }
.jn2__tabs {
  display: grid; grid-auto-flow: column; grid-auto-columns: 1fr;
  border-top: 1px solid var(--line); border-bottom: 1px solid var(--line);
}
.jn2__tab {
  position: relative; display: flex; align-items: center; justify-content: center;
  min-width: 0; padding: var(--s-4) var(--s-3);
  font-size: var(--fs-15); font-weight: var(--fw-semibold); color: var(--ink-6);
  border-left: 1px solid var(--line); cursor: pointer;
  transition: background var(--dur-fast) var(--ease-out),
    color var(--dur-fast) var(--ease-out);
}
.jn2__tab:first-child { border-left: 0; }
.jn2__tab:hover { background: var(--fill-1); color: var(--text); }
.jn2__tab-r {
  appearance: none; -webkit-appearance: none; position: absolute; inset: 0;
  margin: 0; border: 0; background: transparent; cursor: pointer;
}
.jn2__tab:has(.jn2__tab-r:checked) {
  color: var(--text);
  background: color-mix(in srgb, var(--brand) 10%, transparent);
  box-shadow: inset 0 -2px 0 var(--brand);
}
.jn2__noterow { padding: var(--s-4) var(--s-6) 0; }
.jn2__body { display: grid; grid-template-columns: minmax(0, 2fr) minmax(0, 3fr); }
.jn2__col {
  display: flex; flex-direction: column; gap: var(--s-5); min-width: 0;
  padding: var(--s-5) var(--s-6) var(--s-6);
}
.jn2__col + .jn2__col { border-left: 1px solid var(--line); }
/* The footer pairs the button with the link it fires. The link keeps to one
   line; select-all still grabs the whole url the ellipsis hides. */
.jn2__foot {
  display: flex; align-items: center; gap: var(--s-4); flex-wrap: wrap;
  padding: var(--s-5) var(--s-6);
  border-top: 1px solid var(--line); background: var(--fill-1);
}
.jn2__link {
  flex: 1 1 240px; min-width: 0; display: flex; align-items: center; gap: var(--s-3);
  padding: var(--s-2-5) var(--s-3);
  background: var(--fill-2); border: 1px solid var(--line);
  border-radius: var(--r-control);
}
.jn2__link .jn__url {
  flex: 1 1 auto; min-width: 0; padding: 0; border: 0; background: transparent;
  white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
}
/* The box is the click target for select-all, which is what earns it. */
.jn__url, .cmd {
  display: block; padding: var(--s-2-5) var(--s-3);
  background: var(--fill-1); border: 1px solid var(--line);
  border-radius: var(--r-control);
  font-family: var(--font-mono); font-size: var(--fs-13);
  line-height: var(--lh-code); color: var(--ink-85); overflow-wrap: anywhere;
  -webkit-user-select: all; user-select: all;
}
.deep__copy {
  flex: none; padding: 2px var(--s-2-5); border: 1px solid var(--line-ctl);
  border-radius: var(--r-pill); background: var(--fill-2); color: var(--ink-7);
  font: inherit; font-size: var(--fs-13); font-weight: var(--fw-semibold);
  line-height: var(--lh-ui); cursor: pointer;
  transition: background var(--dur-fast) var(--ease-out);
}
.deep__copy:hover { background: var(--fill-4); color: var(--text); }

/* /deploy has no button to fill — its whole output is one line to paste — so
   the line itself takes the selected-chip recipe from deployworldview.css
   `.deploy-world-wizard__name`: a brand edge over a tint, never a solid fill. */
.cmd--hero {
  padding: var(--s-4); border-color: var(--brand);
  background: rgba(255,45,85,.14); font-size: var(--fs-16); color: var(--text);
}

/* PRIMARY: one brand fill per page, on the button that launches. */
.jn__cta {
  display: inline-flex; align-items: center; justify-content: center;
  /* The card is a flex column on most targets and a grid on the QR one, so
     the button has to refuse to stretch in both axes' terms. */
  gap: var(--s-2); align-self: flex-start; justify-self: start;
  padding: 13px 26px; border: 1px solid transparent;
  border-radius: var(--r-control); background: var(--brand-cta); color: #fff;
  font: inherit; font-size: var(--fs-13); font-weight: var(--fw-bold);
  /* The one place Decentraland shouts, and it shouts here only: the builder's
     SIGN IN / BUILD SCENES button — uppercase, tracked, small radius. */
  text-transform: uppercase; letter-spacing: .04em;
  line-height: var(--lh-ui); white-space: nowrap;
  cursor: pointer;
  box-shadow: 0 0 24px color-mix(in srgb, var(--brand) 35%, transparent);
  transition: background var(--dur-fast) var(--ease-out),
    border-color var(--dur-fast) var(--ease-out);
}
.jn__cta:hover { background: var(--brand-cta-hover); color: #fff; text-decoration: none; }
.jn__cta:active { background: var(--brand-cta-active); }
/* A --brand ring is invisible on a --brand fill: this one goes white. */
.jn__cta:focus-visible { outline-color: var(--focus-ring-color-invert); }

/* SECONDARY: same box, no fill. */
.knob__go, .bar__cta {
  display: inline-flex; align-items: center; justify-content: center;
  gap: var(--s-2);
  padding: 11px 24px; border: 1px solid var(--line-ctl);
  border-radius: var(--r-control); background: var(--fill-3); color: var(--text);
  font: inherit; font-size: var(--fs-14); font-weight: var(--fw-semibold);
  letter-spacing: 0; line-height: var(--lh-ui); white-space: nowrap;
  cursor: pointer;
  transition: background var(--dur-fast) var(--ease-out),
    border-color var(--dur-fast) var(--ease-out);
}
.knob__go:hover, .bar__cta:hover {
  background: var(--fill-5); border-color: var(--ink-45);
  color: var(--text); text-decoration: none;
}
.knob__go:active, .bar__cta:active { background: var(--fill-4); }
.bar__cta { margin-left: auto; font-size: var(--fs-13); padding: var(--s-2) var(--s-4); }
/* The apply button is the form's submit, not one more chip in a column. */
.knob__go { align-self: flex-start; }

/* The 17 KB QR rides the footer of the phone card only. */
.qr { display: block; line-height: 0; flex: none; }
.qr img {
  width: 96px; height: 96px; padding: var(--s-1-5); background: #fff;
  border-radius: var(--r-card);
}
/* Every group is a fieldset so its legend names it to a screen reader; the
   browser default fieldset chrome is what has to be undone. */
.knob {
  display: flex; flex-direction: column; gap: var(--s-3); min-width: 0;
  border: 0; padding: 0; margin: 0;
}
/* The deep-link flags as switch rows, hairline-separated like the kv lists. */
.flags { display: flex; flex-direction: column; }
.flag { display: flex; flex-direction: column; gap: var(--s-1); padding: var(--s-3) 0; }
.flag + .flag { border-top: 1px solid var(--line); }
.flag__l { display: flex; align-items: center; gap: var(--s-3); cursor: pointer; }
.flag:hover .chk__k { color: var(--text); }
.chk__k {
  font-family: var(--font-mono); font-size: var(--fs-13);
  font-weight: var(--fw-regular); letter-spacing: 0; color: var(--ink-85);
  line-height: var(--lh-ui); overflow-wrap: anywhere;
}
.chk__w { margin-left: calc(40px + var(--s-3)); }
/* The switch IS the checkbox — appearance-none, so it stays a real, focusable
   input and the state rides the :checked it always rode. */
.sw {
  appearance: none; -webkit-appearance: none; margin: 0; flex: none;
  width: 40px; height: 22px; border-radius: var(--r-pill);
  background: var(--fill-5); position: relative; cursor: pointer;
  transition: background var(--dur-fast) var(--ease-out);
}
.sw::before {
  content: ""; position: absolute; top: 3px; left: 3px; width: 16px; height: 16px;
  border-radius: var(--r-pill); background: #fff;
  box-shadow: 0 1px 2px rgba(0,0,0,.35);
  transition: transform var(--dur-fast) var(--ease-out);
}
.sw:checked { background: var(--brand); }
.sw:checked::before { transform: translateX(18px); }
.sw:disabled { background: var(--fill-2); cursor: not-allowed; }
.flag--off .flag__l { cursor: not-allowed; }
.flag--off .chk__k, .flag--off:hover .chk__k { color: var(--ink-45); }
/* The platform chevron and popup are the accessible ones; keep the native
   appearance and only re-skin the box around them. */
.knob__sel {
  height: 40px; padding: 0 var(--s-3); background: var(--fill-2);
  border: 1px solid var(--line); border-radius: var(--r-control);
  color: var(--text); font: inherit; font-size: var(--fs-14);
  font-weight: var(--fw-semibold); cursor: pointer;
  transition: border-color var(--dur-fast) var(--ease-out);
}
.knob__sel:hover { border-color: var(--ink-45); }
.knob__sel:focus-visible { border-color: color-mix(in srgb, var(--brand) 55%, transparent); }
/* Micro-captions over their control, the Creator Hub datumtile idiom: small,
   tracked, shouted. The join redesign brought this back deliberately. */
.knob__k {
  font-size: var(--fs-13); font-weight: var(--fw-semibold);
  text-transform: uppercase; letter-spacing: .06em;
  line-height: var(--lh-ui); color: var(--ink-6);
}
.knob__on { color: var(--brand-ink); margin-left: var(--s-2); font-weight: var(--fw-bold); }
/* The mcp radio as a two-segment switch — the same appearance-none trick as
   the tabs, so each segment is its own focusable input. */
.seg-group {
  display: flex; align-self: flex-start; padding: 3px;
  border: 1px solid var(--line-ctl); border-radius: var(--r-pill);
  background: var(--fill-1);
}
.seg {
  position: relative; display: flex; align-items: center; justify-content: center;
  min-width: 92px; padding: var(--s-2) var(--s-5); border-radius: var(--r-pill);
  font-size: var(--fs-14); font-weight: var(--fw-semibold); color: var(--ink-6);
  cursor: pointer;
  transition: background var(--dur-fast) var(--ease-out),
    color var(--dur-fast) var(--ease-out);
}
.seg__r {
  appearance: none; -webkit-appearance: none; position: absolute; inset: 0;
  margin: 0; border: 0; background: transparent; cursor: pointer;
  border-radius: var(--r-pill);
}
.seg:hover { color: var(--text); }
/* A filled segment carries a white label, so it takes --brand-cta (5.31:1). */
.seg:has(.seg__r:checked) { background: var(--brand-cta); color: var(--on-brand); }
.seg--off, .seg--off .seg__r { cursor: not-allowed; }
.seg--off { color: var(--ink-45); }
.seg--off:has(.seg__r:checked) { background: var(--fill-3); color: var(--ink-45); }
.knob__sel:disabled {
  background: var(--fill-1); border-style: dashed; color: var(--ink-45);
  cursor: not-allowed;
}
.knob__note { max-width: var(--measure-narrow); }

.map {
  display: flex; align-items: center; gap: var(--s-4); flex-wrap: wrap;
  overflow-x: auto;
}
.map svg { flex: none; }
.map svg rect { fill: var(--map-cell); }
.map svg rect[fill="#ff2d55"] { fill: var(--map-base); }
.map svg text { fill: var(--map-label); font-family: var(--font-mono); pointer-events: none; }
.map svg text[fill="#fff"] { fill: var(--on-brand); }
.map svg circle { fill: var(--map-dot); stroke: var(--map-dot-stroke); pointer-events: none; }
/* The map is the parcel editor: cells add, parcels remove, the base holds —
   but only under body.editing; without the script it is the plain map. */
body.editing .map svg rect.map__p:not([data-base]) { cursor: pointer; }
body.editing .map svg rect.map__p:not([data-base]):hover { fill: var(--fill-5); }
.map svg rect.map__ghost { display: none; }
body.editing .map svg rect.map__ghost {
  display: inline;
  fill: transparent; stroke: var(--line-ctl); stroke-dasharray: 4 3;
  cursor: pointer;
}
body.editing .map svg rect.map__ghost:hover { fill: var(--fill-2); stroke: var(--ink-45); }

.chips { display: flex; flex-wrap: wrap; gap: var(--s-2); }
/* A chip that is not a control has no hover: one metric for .tag and .chip. */
.chip, .tag {
  display: inline-flex; align-items: center; padding: 5px var(--s-3);
  border: 1px solid var(--line); border-radius: var(--r-pill);
  background: var(--fill-2); color: var(--ink-7);
  font-size: var(--fs-13); font-weight: var(--fw-semibold);
  line-height: var(--lh-ui);
}
.chip.dim { background: var(--fill-1); color: var(--ink-45); }
.chip.perm {
  color: var(--warning);
  border-color: color-mix(in srgb, var(--warning) 40%, transparent);
  background: color-mix(in srgb, var(--warning) 14%, transparent);
}

/* Rows are separated by one hairline and nothing else; the panel around them
   is already the box. */
.kvs { display: flex; flex-direction: column; }
.kv {
  display: grid; grid-template-columns: minmax(120px,190px) minmax(0,1fr);
  gap: var(--s-2) var(--s-4); align-items: baseline;
  padding: var(--s-3) 0; border-top: 1px solid var(--line);
  font-size: var(--fs-14); line-height: var(--lh-ui);
}
.kv:first-child { border-top: 0; padding-top: 0; }
.kv .k {
  font-size: var(--fs-13); font-weight: var(--fw-semibold); letter-spacing: 0;
  color: var(--ink-6); overflow-wrap: anywhere;
}
/* A path is not a label: it keeps its own case and its own font. */
.kv .k--file {
  font-family: var(--font-mono); font-weight: var(--fw-regular);
  color: var(--ink-7); overflow-wrap: anywhere;
}
.kv code {
  font-family: var(--font-mono); font-size: var(--fs-13); color: var(--ink-85);
  font-variant-numeric: tabular-nums; overflow-wrap: anywhere;
  -webkit-user-select: all; user-select: all;
}
.kv a code { color: var(--brand-ink); text-decoration: underline; text-underline-offset: 2px; }
.kv a:hover code { color: var(--brand-hover); }

.drawer { background: var(--panel); border: 1px solid var(--line); border-radius: var(--r-card); }
.drawer > summary {
  display: flex; align-items: center; gap: var(--s-2-5); list-style: none;
  cursor: pointer; padding: var(--s-4) var(--s-5); border-radius: var(--r-card);
  font-size: var(--fs-14); font-weight: var(--fw-semibold); letter-spacing: 0;
  color: var(--text);
  transition: background var(--dur-fast) var(--ease-out);
}
.drawer > summary::-webkit-details-marker { display: none; }
.drawer > summary:hover { background: var(--fill-1); }
.drawer > summary::before {
  content: ""; width: 7px; height: 7px; flex: none;
  border-right: 1.5px solid var(--ink-45); border-bottom: 1.5px solid var(--ink-45);
  transform: rotate(-45deg); transition: transform var(--dur-fast) var(--ease-out);
}
.drawer[open] > summary::before { transform: rotate(45deg); }
.drawer[open] > summary {
  border-radius: var(--r-card) var(--r-card) 0 0;
  border-bottom: 1px solid var(--line);
}
.drawer > summary .sec__count { margin-left: auto; }
.drawer__body { display: flex; flex-direction: column; gap: var(--s-3); padding: var(--s-5); }

.reqs {
  display: flex; flex-direction: column;
  font-family: var(--font-mono); font-size: var(--fs-13); line-height: var(--lh-ui);
  color: var(--ink-6); font-variant-numeric: tabular-nums;
}
.reqs > div {
  padding: var(--s-2) 0; border-top: 1px solid var(--line);
  white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
}
.reqs > div:first-child { border-top: 0; padding-top: 0; }
.reqs > div:hover { color: var(--ink-85); }
.reqs:empty { display: none; }
.routes {
  display: flex; flex-wrap: wrap; gap: var(--s-2) var(--s-4);
  font-size: var(--fs-14); font-weight: var(--fw-semibold);
}
.routes code { font-size: var(--fs-13); color: var(--brand-ink); }
.routes a:hover code { color: var(--brand-hover); }
.panel--warn { border-color: color-mix(in srgb, var(--warning) 45%, transparent); }
.panel--warn > h2 { color: var(--warning); }
.reqs .st { font-weight: var(--fw-bold); color: var(--ink-45); }
.reqs .st--ok { color: var(--success); }
.reqs .st--warn { color: var(--warning); }
.reqs .st--err { color: var(--error); }

/* The scene card's editors: quiet until pointed at, saved where they are.
   Everything below that only makes sense with the script running — cursors,
   add affordances, the ghost cells — waits for the `editing` class it puts on
   the body, so without JavaScript the page degrades to the read-only page it
   used to be rather than promising clicks that do nothing. */
.cover-edit { display: block; }
body.editing .cover-edit { cursor: pointer; }
body.editing .cover-edit:hover .cover { border-color: var(--ink-45); }
.cover-edit:focus-within {
  outline: var(--focus-ring-width) solid var(--focus-ring-color);
  outline-offset: var(--focus-ring-offset); border-radius: var(--r-card);
}
[contenteditable] { border-radius: 6px; }
[contenteditable]:hover { background: var(--fill-1); }
body.editing #edit-tags { cursor: pointer; }
.tag--add, .chip--add { border-style: dashed; background: var(--fill-1); color: var(--ink-6); }
.tag--add, .chip--add, .map__hint { display: none; }
body.editing .tag--add, body.editing .chip--add { display: inline-flex; }
body.editing .map__hint { display: block; }
.tags-input {
  height: 36px; min-width: min(320px, 100%); padding: 0 var(--s-2-5);
  background: var(--panel); border: 1px solid var(--line-ctl);
  border-radius: var(--r-control); color: var(--text);
  font: inherit; font-size: var(--fs-13);
}
button.chip {
  font: inherit;
  transition: background var(--dur-fast) var(--ease-out),
    border-color var(--dur-fast) var(--ease-out), color var(--dur-fast) var(--ease-out);
}
button.chip:not(:disabled) { cursor: pointer; }
button.chip:not(:disabled):hover { background: var(--fill-4); color: var(--text); }
/* A permission that is off is a control at rest, not a warning. */
.chip.perm--off { color: var(--ink-6); border-color: var(--line); background: var(--fill-1); }
.chip.perm--off:hover { color: var(--text); }
.spawn-editor {
  display: flex; flex-wrap: wrap; gap: var(--s-3); align-items: flex-end;
  padding: var(--s-4); background: var(--fill-1);
  border: 1px solid var(--line); border-radius: var(--r-control);
}
.spawn-editor label {
  display: flex; flex-direction: column; gap: var(--s-1); min-width: 0;
  font-size: var(--fs-13); font-weight: var(--fw-semibold); color: var(--ink-6);
}
.spawn-editor label.spawn-editor__chk { flex-direction: row; align-items: center; gap: var(--s-2); }
.spawn-editor input[type="text"], .spawn-editor input[type="number"] {
  height: 36px; padding: 0 var(--s-2-5); background: var(--panel);
  border: 1px solid var(--line-ctl); border-radius: var(--r-control);
  color: var(--text); font: inherit; font-size: var(--fs-14); font-weight: var(--fw-regular);
}
/* The UA's 20-character default refuses to shrink, and the editor's column can
   be narrower than that; sized by the label, it follows the space there is. */
.spawn-editor input[type="text"] { width: 100%; min-width: 0; }
.spawn-editor input[type="number"] { width: 5rem; }
.spawn-editor input[type="checkbox"] { width: 16px; height: 16px; accent-color: var(--brand); }
.spawn-editor__btns { display: flex; flex-wrap: wrap; gap: var(--s-2); flex-basis: 100%; }
#spawn-editor:empty { display: none; }
.toast {
  position: fixed; left: 50%; bottom: var(--s-5); transform: translateX(-50%);
  z-index: 100; max-width: min(80vw, 60ch); padding: var(--s-2-5) var(--s-4);
  background: var(--glass); color: var(--text); border: 1px solid var(--line-ctl);
  border-radius: var(--r-control); box-shadow: var(--shadow-bar);
  font-size: var(--fs-13);
}
.toast--err { border-color: color-mix(in srgb, var(--error) 45%, transparent); color: var(--error); }

.u-sr-only {
  position: absolute; width: 1px; height: 1px; overflow: hidden;
  clip: rect(0 0 0 0); clip-path: inset(50%); white-space: nowrap;
}

@media (max-width: 860px) {
  .dash { gap: var(--s-8); padding: var(--s-7) var(--s-5) var(--s-10); }
  .bar { padding: var(--s-3) var(--s-5); }
  .panel { padding: var(--s-5); }
  .jn2__head, .jn2__col, .jn2__foot, .jn2__noterow {
    padding-left: var(--s-5); padding-right: var(--s-5);
  }
  .jn2__body { grid-template-columns: minmax(0,1fr); }
  .jn2__col + .jn2__col { border-left: 0; border-top: 1px solid var(--line); }
  .scene { grid-template-columns: 200px minmax(0,1fr); }
}

@media (max-width: 600px) {
  body { font-size: var(--fs-13); }
  .dash { gap: var(--s-7); padding: var(--s-6) var(--s-4) var(--s-8); }
  .bar { gap: var(--s-3); padding: var(--s-3) var(--s-4); }
  .panel { padding: var(--s-4); }
  .jn2__head, .jn2__col, .jn2__foot, .jn2__noterow {
    padding-left: var(--s-4); padding-right: var(--s-4);
  }
  /* Four tabs cannot share 320px: two rows of two. */
  .jn2__tabs { grid-auto-flow: initial; grid-template-columns: repeat(2, minmax(0,1fr)); }
  .jn2__tab { border-left: 0; border-top: 1px solid var(--line); }
  .jn2__tab:nth-child(-n+2) { border-top: 0; }
  .jn2__tab:nth-child(even) { border-left: 1px solid var(--line); }
  .drawer > summary, .drawer__body { padding: var(--s-4); }
  .scene { grid-template-columns: minmax(0,1fr); gap: var(--s-4); }
  .cover { max-width: 180px; }
  .scene__title, .page__title { font-size: 26px; }
  .kv { grid-template-columns: minmax(0,1fr); gap: var(--s-1); }
  .map { gap: var(--s-3); }
}

@media (pointer: coarse) {
  .jn__cta, .knob__go, .bar__cta, .drawer > summary { min-height: 40px; }
  .jn2__tab, .seg, .knob__sel { min-height: 40px; }
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

/// The page's behaviour, inlined like the stylesheet. Everything it writes
/// goes through `/scene-json` and `/scene-thumbnail`; its state rides in the
/// `#edit-data` JSON blob the page renders beside it.
const SCRIPT: &str = include_str!("landing_edit.js");

/// One choice of the "where" knob: the link it launches and how much of the
/// rest of the panel that link can carry.
struct Target {
    /// The value this choice rides under in the query string ([`WHERE_KEYS`]).
    key: &'static str,
    label: &'static str,
    hint: String,
    /// Built with the knobs this target can carry already folded in.
    url: String,
    qr: String,
    carry: Carry,
    /// The desktop app on this machine is the one anybody reaching this page
    /// most likely wants, so it is the card that gets the accent.
    primary: bool,
}

/// Which tier of the client's deep-link allowlist a realm qualifies for.
/// `ApplicationParametersParser` asks `Uri.IsLoopback`, so this asks the same
/// question of the host — a LAN address is not loopback however local it feels.
fn realm_carry(realm: &str) -> Carry {
    let host = realm
        .split_once("://")
        .map_or(realm, |(_, rest)| rest)
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("");
    let host = match host.strip_prefix('[') {
        Some(rest) => rest.split(']').next().unwrap_or(""),
        None => host.rsplit_once(':').map_or(host, |(h, _)| h),
    };
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    match loopback {
        true => Carry::Loopback,
        false => Carry::Routable,
    }
}

/// A knob the client would throw away is disabled, but the choice is not
/// forgotten: it rides along hidden so switching back to a target that can use
/// it finds it still set.
fn kept(name: &str, value: &str) -> String {
    format!(
        r#"<input type="hidden" name="{}" value="{}">"#,
        esc(name),
        esc(value)
    )
}

/// The whole join surface as one card, and the card as one GET form: a
/// header naming the page and the host, the targets as a tab bar, the knobs
/// in two columns under it, and a footer holding the launch button beside the
/// deep link it fires. Every control rides in the form, so `apply` (or the
/// script's auto-apply) carries all of them back in the query string; knobs
/// the selected target's client would discard are drawn disabled with a note
/// saying so, rather than drawn live and quietly ignored.
///
/// The tab and segment radios are `appearance: none` and stretched across
/// their whole label — still real, focusable inputs (the focus ring lands on
/// the input, which IS the tab), just without the dot the design has no room
/// for. The 17 KB QR only appears when the phone target is on screen.
#[allow(clippy::too_many_arguments)]
fn join_control(
    targets: &[Target],
    selected: usize,
    mcp_server: bool,
    mcp_on: bool,
    knobs: &Knobs,
    spawns: &[Value],
    prefix: &str,
    host: &str,
) -> String {
    let target = &targets[selected];
    let carry = target.carry;
    let mut card_class = String::from("jn");
    if target.primary {
        card_class.push_str(" jn--primary");
    }
    if !target.qr.is_empty() {
        card_class.push_str(" jn--qr");
    }

    let tabs: String = targets
        .iter()
        .enumerate()
        .map(|(i, t)| {
            format!(
                r#"<label class="jn2__tab"><input class="jn2__tab-r" type="radio" name="where" value="{v}"{c}>{label}</label>"#,
                v = esc(t.key),
                label = esc(t.label),
                c = if i == selected { " checked" } else { "" },
            )
        })
        .collect();

    let mut mcp_opts: String = [("off", false), ("on", true)]
        .iter()
        .map(|(name, on)| {
            let enabled = carry.keeps_mcp();
            format!(
                r#"<label class="seg{off}"><input class="seg__r" type="radio" name="mcp" value="{name}"{c}{d}>{label}</label>"#,
                off = if enabled { "" } else { " seg--off" },
                label = if *on { "On" } else { "Off" },
                c = if *on == mcp_on { " checked" } else { "" },
                d = if enabled { "" } else { " disabled" },
            )
        })
        .collect();
    if !carry.keeps_mcp() {
        mcp_opts.push_str(&kept("mcp", if mcp_on { "on" } else { "off" }));
    }
    let where_note = match carry.note() {
        "" => String::new(),
        note => format!(
            r#"<div class="jn2__noterow"><span class="knob__note">{}</span></div>"#,
            esc(note)
        ),
    };
    let mcp_note = match (carry.keeps_mcp(), mcp_server) {
        (false, _) => "",
        (true, true) => "The client opens its MCP port and this preview reads the running scene's errors out of it",
        (true, false) => "This preview was started with --no-mcp, so nothing here reads the port even when the link opens it",
    };
    let mcp_note = match mcp_note {
        "" => String::new(),
        note => format!(r#"<span class="knob__note">{}</span>"#, esc(note)),
    };

    let flags: String = TOGGLES
        .iter()
        .map(|(key, what)| {
            let on = knobs.opts.iter().any(|o| o == key);
            let live = carry.keeps_toggle(key);
            let mut row = format!(
                r#"<div class="flag{off}"><label class="flag__l"><input class="sw" type="checkbox" name="opt" value="{k}"{c}{d}><span class="chk__k">{k}</span></label><span class="chk__w">{w}</span>"#,
                off = if live { "" } else { " flag--off" },
                k = esc(key),
                w = esc(what),
                c = if on { " checked" } else { "" },
                d = if live { "" } else { " disabled" },
            );
            if on && !live {
                row.push_str(&kept("opt", key));
            }
            row.push_str("</div>");
            row
        })
        .collect();
    let flags_on = TOGGLES
        .iter()
        .filter(|(k, _)| carry.keeps_toggle(k) && knobs.opts.iter().any(|o| o == *k))
        .count();
    let flag_count = match flags_on {
        0 => String::new(),
        n => format!(r#"<b class="knob__on">{n} on</b>"#),
    };

    format!(
        r#"<div class="{card_class}"><form class="side" method="get" action="{action}"><div class="jn2__head"><div class="jn2__title"><h2>Join this preview</h2><span class="jn__hint">{hint}</span></div><span class="jn2__host"><span class="bar__dot"></span>{host}</span></div><fieldset class="knob knob--tabs"><legend class="knob__k u-sr-only">where</legend><div class="jn2__tabs">{tabs}</div></fieldset>{where_note}<div class="jn2__body"><div class="jn2__col">{spawn_knob}<fieldset class="knob"><legend class="knob__k">Scene errors in the terminal</legend><div class="seg-group">{mcp_opts}</div>{mcp_note}</fieldset><button class="knob__go" type="submit">Apply</button></div><div class="jn2__col"><fieldset class="knob"><legend class="knob__k">Deep-link flags{flag_count}</legend><div class="flags">{flags}</div></fieldset></div></div><div class="jn2__foot"><a class="jn__cta" id="launch" href="{u}">Launch</a><span class="jn2__link"><span class="jn__url" id="deep-link">{u}</span><button class="deep__copy" id="copy-link" type="button" hidden>Copy</button></span>{qr}</div></form></div>"#,
        action = esc(&format!("{prefix}/")),
        hint = esc(&target.hint),
        host = esc(host),
        u = esc(&target.url),
        qr = target.qr,
        spawn_knob = match spawn_select(knobs, spawns, carry) {
            knob if knob.is_empty() => String::new(),
            knob => format!(r#"<div class="knob">{knob}</div>"#),
        },
    )
}

/// What the header pill names: the host the visitor reached this page on,
/// scheme and prefix stripped.
fn host_label(realm: &str) -> &str {
    let rest = realm.split_once("://").map_or(realm, |(_, r)| r);
    rest.split('/').next().unwrap_or(rest)
}

/// `spawnpoint` picks which of the scene's own named spawn points the player
/// lands on; a scene that names none has nothing to choose, so the knob is
/// left out rather than shown empty.
fn spawn_select(knobs: &Knobs, spawns: &[Value], carry: Carry) -> String {
    let options: String = spawns
        .iter()
        .filter_map(|s| s.get("name").and_then(|n| n.as_str()))
        .map(|name| {
            format!(
                r#"<option value="{n}"{sel}>{n}</option>"#,
                n = esc(name),
                sel = if knobs.spawn == name { " selected" } else { "" },
            )
        })
        .collect();
    if options.is_empty() {
        return String::new();
    }
    let live = carry.keeps_spawn();
    format!(
        r#"<label class="knob__k" for="spawn">Spawn point</label><select class="knob__sel" id="spawn" name="spawn"{d}><option value=""{def}>The scene's default</option>{options}</select>{keep}"#,
        d = if live { "" } else { " disabled" },
        def = if knobs.spawn.is_empty() {
            " selected"
        } else {
            ""
        },
        keep = match (live, knobs.spawn.is_empty()) {
            (false, false) => kept("spawn", &knobs.spawn),
            _ => String::new(),
        },
    )
}

fn render(
    st: &AppState,
    realm: &str,
    prefix: &str,
    mobile_realm: &str,
    lan_realm: Option<&str>,
    knobs: &Knobs,
) -> String {
    let scene_json = st.first_project().map(|p| p.scene_json).unwrap_or_default();
    let title = scene_title(&scene_json);
    let description = scene_json
        .get("display")
        .and_then(|d| d.get("description"))
        .and_then(|d| d.as_str())
        .unwrap_or("");
    // Rendered even when empty: each is a click-to-edit surface, and an empty
    // one is where the description or the first tag gets added.
    let description_block = format!(
        r#"<p class="scene__desc" id="edit-desc" data-hint="Click to add a description">{}</p>"#,
        esc(description)
    );
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
    let tags_block = format!(
        r#"<div class="tags" id="edit-tags">{tags}{add}</div>"#,
        add = if tags.is_empty() {
            r#"<span class="tag tag--add">+ Add tags</span>"#
        } else {
            ""
        },
    );
    let position = st.base();
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
    let mcp_on = knobs.mcp.unwrap_or(st.mcp);
    let extra = |carry: Carry| {
        let mut params = st.explorer_params.clone();
        params.extend(knobs.tokens(carry));
        let mcp = mcp_on && carry.keeps_mcp();
        joinblock::deep_link_extra(st.local_ab, mcp, mcp.then_some(st.mcp_port), &params)
    };
    let mobile = mobile_deep_link(mobile_realm, position);

    let hero_media = match thumbnail(st, prefix) {
        Some(src) => format!(r#"<img class="cover" src="{}" alt="">"#, esc(&src)),
        None => {
            let initial = title.chars().next().unwrap_or('D').to_uppercase();
            format!(r#"<div class="cover placeholder"><span>{initial}</span></div>"#)
        }
    };
    // The label makes the whole cover the file input's click target, so
    // changing the thumbnail is clicking the image.
    let hero_media = format!(
        r#"<label class="cover-edit" title="Change the thumbnail">{hero_media}<input id="cover-input" type="file" accept="image/png,image/jpeg,image/webp" hidden disabled></label>"#
    );
    let qr_img = qr_data_url(&mobile)
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
                r#"<span class="pos">World <code>{}</code> · scene-local coordinates</span>"#,
                esc(n)
            )
        })
        .unwrap_or_default();

    let web_explorer = joinblock::web_explorer_base();
    let this_machine = realm_carry(realm);
    let mut targets = vec![Target {
        key: WHERE_DESKTOP,
        label: "This machine",
        hint: "Opens the installed desktop explorer on this machine, in this realm".to_string(),
        url: desktop_deep_link(realm, position, ab, &extra(this_machine)),
        qr: String::new(),
        carry: this_machine,
        primary: true,
    }];
    if let Some(lan) = lan_realm {
        let lan_host = lan
            .trim_start_matches("http://")
            .rsplit_once(':')
            .map(|(host, _)| host)
            .unwrap_or(lan);
        let lan_assets = ab.map(|u| joinblock::swap_url_host(u, lan_host));
        let carry = realm_carry(lan);
        targets.push(Target {
            key: WHERE_LAN,
            label: "Another device",
            hint: "Opens the desktop explorer on another device on this wi-fi".to_string(),
            url: desktop_deep_link(lan, position, lan_assets.as_deref(), &extra(carry)),
            qr: String::new(),
            carry,
            primary: false,
        });
    }
    targets.push(Target {
        key: WHERE_WEB,
        label: "Web explorer",
        hint: "Opens the web explorer in this browser — no install".to_string(),
        url: web_join_url(&web_explorer, realm, position),
        qr: String::new(),
        carry: Carry::Nothing,
        primary: false,
    });
    targets.push(Target {
        key: WHERE_PHONE,
        label: "Phone",
        hint: "Scan with the phone camera to open this preview there".to_string(),
        url: mobile.clone(),
        qr: qr_img,
        carry: Carry::Nothing,
        primary: false,
    });
    let selected = targets
        .iter()
        .position(|t| t.key == knobs.where_key)
        .unwrap_or(0);
    let join_control = join_control(
        &targets,
        selected,
        st.mcp,
        mcp_on,
        knobs,
        &spawns,
        prefix,
        host_label(realm),
    );

    let recent: Vec<(String, u16, Instant)> = match st.recent_requests.lock() {
        Ok(recent) => recent.iter().rev().cloned().collect(),
        Err(_) => Vec::new(),
    };
    let requests: Vec<String> = recent
        .iter()
        .filter(|(line, _, _)| !line.ends_with("/favicon.ico"))
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
        .collect();
    let request_count = requests.len();
    let request_rows: String = requests.into_iter().take(RECENT_REQUESTS_SHOWN).collect();

    let projects = st.projects();
    let others = projects.get(1..).unwrap_or_default();
    let more_scenes = if others.is_empty() {
        String::new()
    } else {
        let rest: String = others
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
            r#"<details class="drawer"><summary>Also in this realm<span class="sec__count">{}</span></summary><div class="drawer__body"><div class="chips">{rest}</div></div></details>"#,
            others.len()
        )
    };
    let map_hint = match map_svg.contains("map__ghost") {
        true => {
            r#"<span class="note map__hint">Click a parcel to remove it, a dashed cell to add one; the base parcel stays.</span>"#
        }
        false => "",
    };
    let map_row = match map_svg.is_empty() && world_note.is_empty() {
        true => String::new(),
        false => {
            format!(r#"<h2>Parcels</h2><div class="map">{map_svg}{world_note}{map_hint}</div>"#)
        }
    };
    let parcels_panel = format!(
        r#"<div class="panel"><div class="row row--map">{map_row}<div class="row"><h2>Spawn points</h2><div class="chips">{spawn_chips}</div><div id="spawn-editor"></div></div></div><div class="row"><h2>Permissions</h2><div class="chips">{permission_chips}</div></div></div>"#,
        spawn_chips = spawn_chips(&scene_json),
        permission_chips = permission_chips(&scene_json),
    );

    let prefix_esc = esc(prefix);
    let route = |path: &str| format!(r#"<a href="{prefix_esc}{path}"><code>{path}</code></a>"#);
    let mut routes = vec![
        route("/about"),
        route("/scene.json"),
        route("/scenes"),
        route("/preview-wearables"),
    ];
    if lan_realm.is_some() {
        routes.push(route("/mobile-preview"));
    }
    if st
        .data_layer
        .as_ref()
        .is_some_and(|dl| dl.public_dir.is_some())
    {
        routes.push(route("/inspector/"));
    }
    routes.push(route("/deploy"));
    let route_links = routes.join(" ");

    // The one blob the script reads its state from. `<` is escaped so a scene
    // string can never close this tag and open one of its own.
    let edit_data = serde_json::json!({
        "prefix": prefix,
        "tags": scene_json.get("tags").cloned().unwrap_or_else(|| Value::Array(vec![])),
        "parcels": parcels
            .iter()
            .map(|(x, y)| format!("{x},{y}"))
            .collect::<Vec<_>>(),
        "base": format!("{},{}", base.0, base.1),
        "permissions": scene_json
            .get("requiredPermissions")
            .cloned()
            .unwrap_or_else(|| Value::Array(vec![])),
        "spawnPoints": spawns,
    })
    .to_string()
    .replace('<', "\\u003c");

    let body = format!(
        r##"<main class="dash">
  <article class="scene">
    {hero_media}
    <div class="scene__body">
      <h1 class="scene__title" id="edit-title">{title_esc}</h1>
      <div class="pos">At {x},{y} · {parcel_count} parcel{parcel_plural}</div>
      {description_block}
      {tags_block}
    </div>
  </article>

  <section id="join" class="sec">
    {join_control}
  </section>

  <section class="sec">
    {parcels_panel}
    {more_scenes}
  </section>

  <section id="requests" class="sec">
    <details class="drawer"><summary>Recent requests<span
      class="sec__count">{request_count}</span></summary>
      <div class="drawer__body"><div class="reqs">{request_rows}</div></div></details>
    <div class="routes">{route_links}</div>
  </section>
</main>
<script type="application/json" id="edit-data">{edit_data}</script>
<script>{SCRIPT}</script>
"##,
        x = position.0,
        y = position.1,
        parcel_count = parcels.len().max(1),
        parcel_plural = if parcels.len() == 1 { "" } else { "s" },
        title_esc = esc(&title),
    );
    document(
        &title,
        prefix,
        "",
        "#launch",
        "Skip to the launch button",
        ("/deploy", "Deploy"),
        &body,
    )
}

/// The chrome every page on this server shares: one stylesheet, one header, one
/// skip link. Split out of [`render`] when `/deploy` became its own page, so
/// the two cannot drift into looking like different servers.
pub(super) fn document(
    title: &str,
    prefix: &str,
    extra_css: &str,
    skip_to: &str,
    skip_label: &str,
    bar_cta: (&str, &str),
    body: &str,
) -> String {
    let home = esc(prefix);
    format!(
        r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{t} — dcl-one-sdk preview</title>
<style>{STYLE}{extra_css}</style>
</head>
<body>
<a class="skip" href="{skip_to}">{skip_label}</a>
<header class="bar">
  <div class="bar__mark"><span class="bar__dot"></span>DCL One SDK</div>
  <a class="bar__cta" href="{home}{cta_href}">{cta_label}</a>
</header>
{body}
</body>
</html>
"##,
        t = esc(title),
        cta_href = esc(bar_cta.0),
        cta_label = esc(bar_cta.1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Project;
    use serde_json::json;
    use std::collections::{HashMap, VecDeque};
    use std::sync::Mutex;
    use tokio::sync::broadcast;

    fn landing_state(projects: Vec<Project>) -> AppState {
        let (reload_tx, _) = broadcast::channel(4);
        AppState {
            projects: std::sync::RwLock::new(projects),
            machine: "test-machine".to_string(),
            reload_tx,
            offline_comms: true,
            port: 8000,
            data_layer: None,
            entity_cache: Mutex::new(HashMap::new()),
            optimized_assets_url: std::sync::OnceLock::new(),
            local_ab: false,
            deep_link_extra: String::new(),
            mcp: true,
            mcp_port: crate::joinblock::DEFAULT_EXPLORER_MCP_PORT,
            allow_remote_deploy: false,
            deploy_dry_run: true,
            explorer_params: Vec::new(),
            recent_requests: Mutex::new(VecDeque::new()),
        }
    }

    fn target(key: &'static str, label: &'static str, url: &str, carry: Carry) -> Target {
        Target {
            key,
            label,
            hint: "h".into(),
            url: url.into(),
            qr: String::new(),
            carry,
            primary: key == WHERE_DESKTOP,
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
        let html = join_control(&targets, 0, true, true, &Knobs::default(), &[], "", "127.0.0.1:8000");
        assert_eq!(
            html.matches(r#"<div class="jn "#).count()
                + html.matches(r#"<div class="jn">"#).count(),
            1,
            "one card, the selected target: {html}"
        );
        assert!(html.contains("decentraland://a") && !html.contains("decentraland://b"));
        assert_eq!(
            join_control(&targets, 1, true, true, &Knobs::default(), &[], "", "127.0.0.1:8000")
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
            join_control(&targets, 0, false, false, &Knobs::default(), &[], "", "127.0.0.1:8000")
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
    fn the_qr_rides_the_phone_card_and_the_accent_stays_on_the_desktop_one() {
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
        let desktop = join_control(&targets, 0, true, true, &Knobs::default(), &[], "", "127.0.0.1:8000");
        assert!(desktop.contains(r#"<div class="jn jn--primary">"#));
        assert!(!desktop.contains("QRQR"), "no QR on a card that has none");
        let phone = join_control(&targets, 1, true, true, &Knobs::default(), &[], "", "127.0.0.1:8000");
        assert!(
            phone.contains(r#"<div class="jn jn--qr">"#),
            "the phone card needs the grid the QR is placed in: {phone}"
        );
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
            ["--multi-instance=true", "--spawnpoint=entrance"],
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
        assert!(tokens(None).is_empty());
        assert!(tokens(Some("other=1")).is_empty());
        assert!(
            tokens(Some("opt=launch-cdp-monitor-on-start")).is_empty(),
            "a checkbox the page never shows is not a checkbox"
        );
        assert!(
            tokens(Some("spawn=nowhere")).is_empty(),
            "a spawn point this scene does not define"
        );
        assert_eq!(
            tokens(Some("spawn=entrance")),
            ["--spawnpoint=entrance"],
            "one it does define"
        );
        for smuggled in [
            "args=--gatekeeper-url%3Dhttps%3A%2F%2Fevil",
            "gatekeeper-url=https%3A%2F%2Fevil",
            "--gatekeeper-url=https%3A%2F%2Fevil",
            "opt=gatekeeper-url",
        ] {
            assert!(tokens(Some(smuggled)).is_empty(), "{smuggled}");
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
            lan.contains(r#"value="multi-instance" checked disabled"#)
                && lan.contains(r#"value="off" disabled"#),
            "the knobs that do nothing here are drawn disabled: {lan}"
        );
        assert!(
            lan.contains("not loopback"),
            "and say why, rather than claiming the client reads them"
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
                html.contains(r#"value="hub" checked disabled"#)
                    && html
                        .contains(r#"<select class="knob__sel" id="spawn" name="spawn" disabled>"#),
                "{where_key} reads no deep-link params, so its knobs are greyed: {html}"
            );
            assert!(html.contains("reads no deep-link params"), "{where_key}");
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
            1,
            "the whole page, stylesheet included, is one form"
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
        assert!(
            STYLE.contains("--map-base: var(--brand-cta)")
                && STYLE.contains(r##".map svg rect[fill="#ff2d55"] { fill: var(--map-base); }"##),
            "the selected parcel took --brand back, which cannot carry a white label"
        );
        let label = contrast((255.0, 255.0, 255.0), hex("--brand-cta: #"));
        assert!(label >= 4.5, "the parcel label is {label:.2}:1, under AA");
    }

    #[test]
    fn esc_neutralizes_html() {
        assert_eq!(esc(r#"<b a="1">&x"#), "&lt;b a=&quot;1&quot;&gt;&amp;x");
    }

    /// The feedback that reshaped the card: the deep link is text on the card,
    /// not a disclosure to open. The copy button ships hidden and the script
    /// that makes it work is what reveals it.
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
            "127.0.0.1:8000",
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
            "the footer pairs the button with the link it fires: {card}"
        );
    }

    /// The scene card is scene.json's editor, progressively: the server ships
    /// every editor target with its id but INERT — no contenteditable, every
    /// editor button disabled, the add affordances behind `body.editing` — and
    /// the script is what switches them on. Without it the page degrades to
    /// the read-only page it used to be, promising no click it cannot honour.
    #[test]
    fn the_scene_card_ships_its_editors_inert_for_the_script_to_enable() {
        let st = landing_state(vec![Project {
            root: std::path::PathBuf::from("/tmp/scene"),
            scene_json: json!({
                "display": { "title": "Gather" },
                "spawnPoints": [{ "name": "s", "position": { "x": 1, "y": 0, "z": 1 } }]
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
        assert!(html.contains(r#"<div id="spawn-editor"></div>"#));
        assert!(html.contains(r#"id="spawn-add" disabled"#));
        assert!(html.contains(r#"data-spawn="0" disabled"#));
        for gate in [
            "body.editing .map svg rect.map__ghost",
            "body.editing .map__hint",
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
    fn parcels_svg_accents_the_base_and_rings_the_editable_scene_with_add_cells() {
        let spawns = vec![json!({ "position": { "x": [16, 16], "y": 0, "z": [16, 16] } })];
        let svg = parcels_svg(&[(0, 0), (1, 0), (0, 1), (1, 1)], (0, 0), &spawns);
        assert_eq!(svg.matches("data-parcel=").count(), 4);
        assert_eq!(
            svg.matches("data-add=").count(),
            12,
            "one ring of add targets around the 2×2"
        );
        assert_eq!(svg.matches("<rect").count(), 16);
        assert_eq!(svg.matches("#ff2d55").count(), 1);
        assert_eq!(
            svg.matches(r#"data-base="1""#).count(),
            1,
            "only the base is held"
        );
        assert_eq!(svg.matches("<circle").count(), 1);
    }

    /// A scene wider than the ring bound gets a plain map — the ghost grid is
    /// a rendering economy, not a size rule; the layout itself stays editable
    /// by removal and through scene.json.
    #[test]
    fn an_oversized_scene_draws_its_map_without_add_cells() {
        let strip: Vec<(i64, i64)> = (0..GHOST_RING_SPAN).map(|x| (x, 0)).collect();
        let svg = parcels_svg(&strip, (0, 0), &[]);
        assert_eq!(svg.matches("data-add=").count(), 0);
        assert_eq!(svg.matches("data-parcel=").count(), strip.len());
    }

    #[test]
    fn no_content_url_on_the_page_decodes_back_to_the_scene_directory() {
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

        let html = render(
            &st,
            "http://127.0.0.1:8000",
            "",
            "http://127.0.0.1:8000",
            None,
            &Knobs::default(),
        );
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

    #[test]
    fn deploy_target_lets_the_default_target_outrank_the_worlds_server() {
        let world = json!({ "worldConfiguration": { "name": "my.dcl.eth" } });
        let (dest, flags) = deploy_target(&world, None);
        assert!(dest.contains("my.dcl.eth"));
        assert_eq!(flags, format!(" --target-content {WORLDS_CONTENT_SERVER}"));
        assert_eq!(
            deploy_target(&world, Some("https://example.org")),
            ("https://example.org".to_string(), String::new())
        );
        assert_eq!(deploy_target(&world, Some("  ")).1, flags);
        assert!(deploy_target(&json!({}), None).0.contains("Genesis City"));
        assert_eq!(
            deploy_target(&json!({}), Some(" https://example.org ")),
            ("https://example.org".to_string(), String::new())
        );
    }

    /// The permissions panel is the whole schema list as toggles — a required
    /// one pressed, the rest at rest — plus any key the scene carries that the
    /// list does not, so toggling a neighbour cannot silently drop it.
    #[test]
    fn permission_chips_draw_every_known_toggle_with_its_state() {
        let chips = permission_chips(&json!({ "requiredPermissions": ["USE_FETCH", "CUSTOM_X"] }));
        assert_eq!(chips.matches("<button").count(), PERMISSIONS.len() + 1);
        assert!(chips.contains(
            r#"<button type="button" class="chip perm" data-perm="USE_FETCH" aria-pressed="true" disabled>HTTP fetch</button>"#
        ));
        assert!(chips.contains(
            r#"<button type="button" class="chip perm perm--off" data-perm="USE_WEB3_API" aria-pressed="false" disabled>Web3 wallet</button>"#
        ));
        assert!(chips.contains(r#"data-perm="CUSTOM_X" aria-pressed="true""#));

        let none = permission_chips(&json!({}));
        assert_eq!(none.matches("<button").count(), PERMISSIONS.len());
        assert_eq!(none.matches(r#"aria-pressed="true""#).count(), 0);
    }
}
