//! The scene-layout card: the parcel grid, spawn areas, permissions, and
//! the tabs that hold them.

use super::super::chrome::esc;
use serde_json::Value;

/// A coordinate's `[a, b]` as given; a plain number is `(n, n)`.
fn coord_pair(v: Option<&Value>) -> (f64, f64) {
    match v {
        Some(Value::Array(range)) => {
            let f = |i: usize| range.get(i).and_then(Value::as_f64).unwrap_or(0.0);
            (f(0), f(1.min(range.len().saturating_sub(1))))
        }
        _ => {
            let n = v.and_then(Value::as_f64).unwrap_or(0.0);
            (n, n)
        }
    }
}

pub(super) fn coord(v: Option<&Value>) -> String {
    let (a, b) = coord_pair(v);
    if a == b {
        trim_num(a)
    } else {
        format!("{}\u{2013}{}", trim_num(a), trim_num(b))
    }
}

/// A coordinate as the `[min, max]` span it covers.
pub(super) fn coord_range(v: Option<&Value>) -> (f64, f64) {
    let (a, b) = coord_pair(v);
    (a.min(b), a.max(b))
}

pub(super) fn trim_num(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

/// Every `requiredPermissions` key the schema defines, with its label and
/// what granting it allows. The edit endpoint accepts no key outside this
/// list that the scene does not already carry.
pub(in crate::start) const PERMISSIONS: [(&str, &str, &str); 7] = [
    (
        "USE_WEBSOCKET",
        "Websockets",
        "Open socket connections to a server",
    ),
    ("USE_FETCH", "HTTP fetch", "Call external HTTP endpoints"),
    (
        "USE_WEB3_API",
        "Web3 wallet",
        "Ask the player to sign transactions",
    ),
    (
        "OPEN_EXTERNAL_LINK",
        "Open links",
        "Open a URL in the browser",
    ),
    (
        "ALLOW_TO_MOVE_PLAYER_INSIDE_SCENE",
        "Move player",
        "Move the avatar inside the scene",
    ),
    (
        "ALLOW_TO_TRIGGER_AVATAR_EMOTE",
        "Trigger emotes",
        "Play an emote on the avatar",
    ),
    (
        "ALLOW_MEDIA_HOSTNAMES",
        "External media",
        "Stream video and audio from outside",
    ),
];

pub(in crate::start) fn parse_parcels(scene_json: &Value) -> (Vec<(i64, i64)>, (i64, i64)) {
    let parcels: Vec<(i64, i64)> = scene_json
        .get("scene")
        .and_then(|s| s.get("parcels"))
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .filter_map(catalyrst_auth_chain::pointer::parse_pointer)
                .collect()
        })
        .unwrap_or_default();
    let base = scene_json
        .get("scene")
        .and_then(|s| s.get("base"))
        .and_then(Value::as_str)
        .and_then(catalyrst_auth_chain::pointer::parse_pointer)
        .unwrap_or_else(|| parcels.first().copied().unwrap_or((0, 0)));
    (parcels, base)
}

/// Past this many cells a side the grid draws no add ring: a rendering bound
/// only (the edit endpoint takes any connected layout), since the grid is
/// drawn whole and a huge scene's add cells are page weight for drags nobody
/// makes at that size.
pub(super) const GHOST_RING_SPAN: i64 = 40;

/// `(min_x, min_y, max_x, max_y)` of the parcels, `(0, 0, 0, 0)` for none.
fn bbox(parcels: &[(i64, i64)]) -> (i64, i64, i64, i64) {
    let xs = || parcels.iter().map(|p| p.0);
    let ys = || parcels.iter().map(|p| p.1);
    (
        xs().min().unwrap_or(0),
        ys().min().unwrap_or(0),
        xs().max().unwrap_or(0),
        ys().max().unwrap_or(0),
    )
}

/// The grid the layout card draws: the bounding box plus, under
/// [`GHOST_RING_SPAN`], one ring of add targets. The same bounds ride the
/// `#edit-data` blob so the script and the markup agree on where it starts.
pub(super) fn grid_bounds(parcels: &[(i64, i64)]) -> (i64, i64, i64, i64, bool) {
    let (min_x, min_y, max_x, max_y) = bbox(parcels);
    let ring = (1 + max_x - min_x).max(1 + max_y - min_y) < GHOST_RING_SPAN;
    let r = i64::from(ring);
    (min_x - r, min_y - r, max_x + r, max_y + r, ring)
}

/// The layout map as a DIV grid — ruler, row numbers, one cell per parcel of
/// the bounding box plus the add ring, and the spawn areas positioned over it
/// in world metres.
///
/// The cell size rides the `--lay-pitch` variable: the stylesheet ships a
/// default so the map renders without JavaScript, and the script refits it
/// to the pane. Ruler labels carry their value in `data-n` so the script can
/// thin them to every fifth when the cells get small.
pub(super) fn layout_grid(parcels: &[(i64, i64)], base: (i64, i64), spawns: &[Value]) -> String {
    let members: std::collections::HashSet<(i64, i64)> = parcels.iter().copied().collect();
    let (x0, y0, x1, y1, ring) = grid_bounds(parcels);
    let (min_x, min_y, max_x, max_y) = bbox(parcels);
    let cols = x1 - x0 + 1;
    let rows = y1 - y0 + 1;
    let show_all = cols <= 24 && rows <= 24;
    let tick = |axis: &str, v: i64, lo: i64, hi: i64| {
        let n = if (lo..=hi).contains(&v) {
            v.to_string()
        } else {
            String::new()
        };
        let shown = if !n.is_empty() && (show_all || v % 5 == 0) {
            n.as_str()
        } else {
            ""
        };
        format!(r#"<span data-{axis}="{v}" data-n="{n}">{shown}</span>"#)
    };
    let ruler: String = (x0..=x1).map(|x| tick("x", x, min_x, max_x)).collect();
    let nums: String = (y0..=y1)
        .rev()
        .map(|y| tick("y", y, min_y, max_y))
        .collect();
    let cell = |x: i64, y: i64| {
        let (kind, title) = if (x, y) == base {
            ("base", format!("Base parcel {x},{y}"))
        } else if members.contains(&(x, y)) {
            ("in", format!("{x},{y}"))
        } else if ring {
            ("add", format!("Add {x},{y}"))
        } else {
            return r#"<div class="lay__cell"></div>"#.to_string();
        };
        format!(
            r#"<div class="lay__cell lay__cell--{kind}" data-cell="{x},{y}" title="{title}"></div>"#
        )
    };
    let cells: String = (y0..=y1)
        .rev()
        .flat_map(|y| (x0..=x1).map(move |x| (x, y)))
        .map(|(x, y)| cell(x, y))
        .collect();
    let mut areas = String::new();
    for (i, spawn) in spawns.iter().enumerate() {
        let pos = spawn.get("position");
        let (sx0, sx1) = coord_range(pos.and_then(|p| p.get("x")));
        let (sz0, sz1) = coord_range(pos.and_then(|p| p.get("z")));
        let name = spawn.get("name").and_then(Value::as_str).unwrap_or("spawn");
        let left = (base.0 - x0) as f64 * 16.0 + sx0;
        let top = (y1 + 1 - base.1) as f64 * 16.0 - sz1;
        areas.push_str(&format!(
            r#"<div class="lay__area" data-area="{i}" style="left:calc(var(--lay-step)*{l}/16);top:calc(var(--lay-step)*{t}/16);width:max(5px,calc(var(--lay-step)*{w}/16));height:max(5px,calc(var(--lay-step)*{h}/16))"><span class="lay__area-name">{n}</span></div>"#,
            l = trim_num(left),
            t = trim_num(top),
            w = trim_num(sx1 - sx0),
            h = trim_num(sz1 - sz0),
            n = esc(name),
        ));
    }
    format!(
        r#"<div class="lay__map" style="--lay-cols:{cols}"><div class="lay__ruler">{ruler}</div><div class="lay__mid"><div class="lay__nums">{nums}</div><div class="lay__grid" id="lay-grid" role="img" aria-label="parcel layout">{cells}{areas}</div></div></div>"#
    )
}

/// Each spawn area as a row that opens its editor, plus the dashed button
/// that arms drawing a new one on the grid.
pub(super) fn spawn_rows(spawns: &[Value]) -> String {
    let mut out: String = spawns
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let name = s.get("name").and_then(Value::as_str).unwrap_or("spawn");
            let def = s.get("default").and_then(Value::as_bool) == Some(true);
            let pos = s.get("position");
            let p = |k| coord(pos.and_then(|p: &Value| p.get(k)));
            format!(
                r#"<button type="button" class="lay__srow" data-spawn="{i}" disabled><span class="lay__star{d}">★</span><span class="lay__srow-t"><span class="lay__srow-n">{name}</span><code class="lay__srow-c">X {x} · Z {z} · Y {y}</code></span></button>"#,
                d = if def { " lay__star--def" } else { "" },
                name = esc(name),
                x = p("x"),
                z = p("z"),
                y = p("y"),
            )
        })
        .collect();
    out.push_str(
        r#"<button type="button" class="lay__sadd" id="lay-sadd" aria-pressed="false" disabled>+ Add spawn area</button>"#,
    );
    out
}

/// Every known permission as a switch row, plus any key the scene carries
/// that the schema list does not — shown so toggling a neighbour cannot
/// silently drop it.
pub(super) fn permission_rows(scene_json: &Value) -> String {
    let required: Vec<&str> = scene_json
        .get("requiredPermissions")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let row = |key: &str, label: &str, what: &str, on: bool| {
        format!(
            r#"<label class="lay__perm"><input class="sw" type="checkbox" data-perm="{key}"{c} disabled><span class="lay__perm-t"><span class="lay__perm-n">{label}</span>{what}</span></label>"#,
            key = esc(key),
            label = esc(label),
            c = if on { " checked" } else { "" },
            what = if what.is_empty() {
                String::new()
            } else {
                format!(r#"<span class="lay__perm-d">{}</span>"#, esc(what))
            },
        )
    };
    let mut out: String = PERMISSIONS
        .iter()
        .map(|(key, label, what)| row(key, label, what, required.contains(key)))
        .collect();
    for key in required {
        if !PERMISSIONS.iter().any(|(k, ..)| *k == key) {
            out.push_str(&row(key, key, "", true));
        }
    }
    out
}

/// The scene-layout card: tabs, the parcel grid, and a rail that changes per
/// tab. The server renders the Info tab active and every control inert; the
/// script switches tabs, drags, and saves.
pub(super) fn scene_layout_card(
    scene_json: &Value,
    parcels: &[(i64, i64)],
    base: (i64, i64),
    spawns: &[Value],
    info_pane: &str,
) -> String {
    let (min_x, min_y, max_x, max_y) = bbox(parcels);
    let (w, h) = ((max_x - min_x + 1) * 16, (max_y - min_y + 1) * 16);
    let tab = |key: &str, label: &str, selected: bool| {
        format!(
            r#"<button type="button" class="jn2__tab lay__tab" data-laytab="{key}" role="tab" aria-selected="{selected}" disabled>{label}</button>"#
        )
    };
    let tabs = format!(
        "{}{}{}{}",
        tab("info", "Info", true),
        tab("parcels", "Shape", false),
        tab("spawns", "Spawn points", false),
        tab("perms", "Permissions", false),
    );
    let info_pane = format!(r#"<section class="lay__pane" data-pane="info">{info_pane}</section>"#);
    let parcels_pane = format!(
        r#"<section class="lay__pane" data-pane="parcels" hidden><span class="knob__k">Scene bounds</span><div class="lay__kvs"><div class="lay__kv"><span class="k">Parcels</span><code>{n}</code></div><div class="lay__kv"><span class="k">Size</span><code>{w} × {h} m</code></div><div class="lay__kv"><span class="k">Range</span><code>{min_x},{min_y} → {max_x},{max_y}</code></div><div class="lay__kv"><span class="k">Base parcel</span><span class="lay__basev"><code class="lay__base">{bx},{by}</code><button type="button" class="deep__copy" id="lay-base-pick" aria-pressed="false" disabled>Change</button></span></div></div><p class="note">One parcel is 16 × 16 m. Drag across the grid to paint or erase parcels; the grid rescales to whatever the scene becomes.</p></section>"#,
        n = parcels.len(),
        bx = base.0,
        by = base.1,
    );
    let spawns_pane = format!(
        r#"<section class="lay__pane" data-pane="spawns" hidden><span class="knob__k">Spawn areas</span><div class="lay__srows">{rows}</div><div id="lay-sed"></div></section>"#,
        rows = spawn_rows(spawns),
    );
    let perms_pane = format!(
        r#"<section class="lay__pane" data-pane="perms" hidden><span class="knob__k">Scene permissions</span><div class="lay__perms">{rows}</div></section>"#,
        rows = permission_rows(scene_json),
    );
    format!(
        r#"<div class="jn lay lay--info" id="scene-layout"><div class="jn2__tabs" role="tablist">{tabs}</div><div class="lay__body"><div class="lay__left"><div class="lay__legend"><span class="lay__key"><i class="lay__swatch lay__swatch--in"></i>In scene</span><span class="lay__key"><i class="lay__swatch lay__swatch--area"></i>Spawn area</span><span class="lay__size">{n} parcel{s} · {w} × {h} m</span></div>{map}<div class="note lay__hint" id="lay-hint">Click the title, description, tags or cover to edit — changes save to scene.json</div></div><div class="lay__rail">{info_pane}{parcels_pane}{spawns_pane}{perms_pane}</div></div></div>"#,
        n = parcels.len(),
        s = if parcels.len() == 1 { "" } else { "s" },
        map = layout_grid(parcels, base, spawns),
    )
}
