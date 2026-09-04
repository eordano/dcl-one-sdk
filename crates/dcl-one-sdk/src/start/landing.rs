//! The landing page at `/`: server-rendered, one GET form, plus [`SCRIPT`]
//! which turns the page into scene.json's editor. The server ships every
//! editor control inert and the script enables them, so no-JS degrades to a
//! read-only page. All mutations are fetch POSTs to the `edit.rs` routes
//! (outside the CORS layer, loopback + same-origin gated); nothing here POSTs
//! as a form.

#[path = "join_card.rs"]
mod join_card;
#[path = "layout_card.rs"]
mod layout_card;

use super::chrome::{document, esc, html, Nav};
use super::deploy_page::{known_account, nav_badge, token};
use super::{forwarded_host, forwarded_prefix, forwarded_proto, AppState};
use crate::joinblock::{self, desktop_deep_link, mobile_deep_link, scene_title, web_join_url};
use crate::netinfo;
use crate::scene::b64_content_hash;
use crate::scene::Project;
use axum::http::{header, HeaderMap};
use axum::response::Response;
pub(in crate::start) use join_card::DEFAULT_ON;
use join_card::{
    host_label, join_control, knobs, realm_carry, Carry, Knobs, Target, WHERE_DESKTOP, WHERE_LAN,
    WHERE_PHONE, WHERE_WEB,
};
use layout_card::{grid_bounds, scene_layout_card};
pub(in crate::start) use layout_card::{parse_parcels, PERMISSIONS};
use serde_json::Value;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The buffer holds hundreds of requests; a dozen is what someone opening the
/// drawer actually reads.
const RECENT_REQUESTS_SHOWN: usize = 12;

fn spawn_points(scene_json: &Value) -> &[Value] {
    match scene_json.get("spawnPoints").and_then(Value::as_array) {
        Some(spawns) => spawns,
        None => &[],
    }
}

/// The only values `spawnpoint` may take: the client matches on the name.
fn spawn_names(scene_json: &Value) -> Vec<String> {
    spawn_points(scene_json)
        .iter()
        .filter_map(|s| s.get("name").and_then(Value::as_str))
        .map(str::to_string)
        .collect()
}

/// Reuse the cached value while `fresh` holds for its key, else recompute and
/// store it under `key()`.
fn memo<K, T: Clone>(
    cell: &Mutex<Option<(K, T)>>,
    fresh: impl FnOnce(&K) -> bool,
    key: impl FnOnce() -> K,
    compute: impl FnOnce() -> T,
) -> T {
    let Ok(mut slot) = cell.lock() else {
        return compute();
    };
    if let Some((k, value)) = slot.as_ref() {
        if fresh(k) {
            return value.clone();
        }
    }
    let value = compute();
    *slot = Some((key(), value.clone()));
    value
}

/// A value recomputed at most once per `ttl`.
fn memoised<T: Clone>(
    cell: &Mutex<Option<(Instant, T)>>,
    ttl: Duration,
    compute: impl FnOnce() -> T,
) -> T {
    memo(cell, |at| at.elapsed() < ttl, Instant::now, compute)
}

/// A value recomputed only when its one string input changes.
fn memoised_by<T: Clone>(
    cell: &Mutex<Option<(String, T)>>,
    key: &str,
    compute: impl FnOnce() -> T,
) -> T {
    memo(cell, |cached| cached == key, || key.to_string(), compute)
}

/// `getifaddrs` is a syscall per render; a laptop's interfaces change on the
/// scale of minutes.
fn share_ip() -> Option<std::net::Ipv4Addr> {
    static IFACES: Mutex<Option<(Instant, Option<std::net::Ipv4Addr>)>> = Mutex::new(None);
    memoised(&IFACES, Duration::from_secs(10), || {
        netinfo::share_ip(&netinfo::enumerate())
    })
}

/// Encoding the QR is the most expensive thing on this page by a wide margin,
/// and its input only moves when the Host header or the base parcel does.
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
    html(render(
        st,
        &realm,
        &prefix,
        &mobile_realm,
        lan_realm.as_deref(),
        &knobs(query, &names),
    ))
}

fn thumbnail(project: Option<&Project>, machine: &str, prefix: &str) -> Option<String> {
    let project = project?;
    let rel = project
        .scene_json
        .get("display")
        .and_then(|d| d.get("navmapThumbnail"))
        .and_then(Value::as_str)?;
    let abs = project.root.join(rel);
    if !abs.is_file() {
        return None;
    }
    let hash = b64_content_hash(&abs.display().to_string(), machine);
    Some(format!("{prefix}/content/contents/{hash}"))
}

/// Inlined like the stylesheet; it writes through `/scene-json` and
/// `/scene-thumbnail` and reads its state from the `#edit-data` blob.
const SCRIPT: &str = concat!(
    include_str!("page_common.js"),
    include_str!("landing_edit.js")
);

/// The scene hero: cover, title, position line, description and tags — every
/// piece an editor target.
fn scene_card(
    project: Option<&Project>,
    machine: &str,
    prefix: &str,
    scene_json: &Value,
    title: &str,
    position: (i64, i64),
    parcel_count: usize,
) -> String {
    let cover = match thumbnail(project, machine, prefix) {
        Some(src) => format!(r#"<img class="cover" src="{}" alt="">"#, esc(&src)),
        None => {
            let initial = title.chars().next().unwrap_or('D').to_uppercase();
            format!(r#"<div class="cover placeholder"><span>{initial}</span></div>"#)
        }
    };
    let description = scene_json
        .get("display")
        .and_then(|d| d.get("description"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let tags: String = scene_json
        .get("tags")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(|t| format!(r#"<span class="tag">{}</span>"#, esc(t)))
                .collect()
        })
        .unwrap_or_default();
    format!(
        r#"<article class="scene">
    <label class="cover-edit" title="Change the thumbnail">{cover}<input id="cover-input" type="file" accept="image/png,image/jpeg,image/webp" hidden disabled></label>
    <div class="scene__body">
      <h1 class="scene__title" id="edit-title">{title_esc}</h1>
      <div class="pos">At {x},{y} · {parcel_count} parcel{plural}</div>
      <p class="scene__desc" id="edit-desc" data-hint="Click to add a description">{desc}</p>
      <div class="tags" id="edit-tags">{tags}{add}</div>
    </div>
  </article>"#,
        title_esc = esc(title),
        x = position.0,
        y = position.1,
        plural = if parcel_count == 1 { "" } else { "s" },
        desc = esc(description),
        add = if tags.is_empty() {
            r#"<span class="tag tag--add">+ Add tags</span>"#
        } else {
            ""
        },
    )
}

/// The four launch targets, each with the deep link its client will keep.
fn launch_targets(
    st: &AppState,
    knobs: &Knobs,
    realm: &str,
    lan_realm: Option<&str>,
    mobile_realm: &str,
    position: (i64, i64),
    mcp_on: bool,
) -> Vec<Target> {
    let ab = match st.local_ab {
        true => None,
        false => st.optimized_assets_url.get().map(String::as_str),
    };
    let extra = |carry: Carry| {
        let mut params = st.explorer_params.clone();
        params.extend(knobs.tokens(carry));
        let mcp = mcp_on && carry.keeps_mcp();
        joinblock::deep_link_extra(st.local_ab, mcp, mcp.then_some(st.mcp_port), &params)
    };
    let mobile = mobile_deep_link(mobile_realm, position);
    let qr_img = match knobs.where_key == WHERE_PHONE {
        true => qr_data_url(&mobile)
            .map(|qr| {
                format!(r#"<span class="qr"><img src="{qr}" alt="" width="96" height="96"></span>"#)
            })
            .unwrap_or_default(),
        false => String::new(),
    };

    let this_machine = realm_carry(realm);
    let mut targets = vec![Target::new(
        WHERE_DESKTOP,
        "This machine",
        "Opens the installed desktop explorer on this machine, in this realm",
        desktop_deep_link(realm, position, ab, &extra(this_machine)),
        this_machine,
    )];
    if let Some(lan) = lan_realm {
        let lan_host = lan
            .trim_start_matches("http://")
            .rsplit_once(':')
            .map(|(host, _)| host)
            .unwrap_or(lan);
        let lan_assets = ab.map(|u| joinblock::swap_url_host(u, lan_host));
        let carry = realm_carry(lan);
        targets.push(Target::new(
            WHERE_LAN,
            "Another device",
            "Opens the desktop explorer on another device on this wi-fi",
            desktop_deep_link(lan, position, lan_assets.as_deref(), &extra(carry)),
            carry,
        ));
    }
    targets.push(Target::new(
        WHERE_WEB,
        "Web explorer",
        "Opens the web explorer in this browser — no install",
        web_join_url(&joinblock::web_explorer_base(), realm, position),
        Carry::Nothing,
    ));
    let mut phone = Target::new(
        WHERE_PHONE,
        "Phone",
        "Scan with the phone camera to open this preview there",
        mobile,
        Carry::Nothing,
    );
    phone.qr = qr_img;
    targets.push(phone);
    targets
}

/// The request drawer's count and rows. Only the dozen drawn rows are
/// escaped; the buffer holds up to 200 attacker-influenced lines and the rest
/// are just counted.
fn requests_drawer(st: &AppState) -> (usize, String) {
    let Ok(buffer) = st.recent_requests.lock() else {
        return (0, String::new());
    };
    let lines: Vec<_> = buffer
        .iter()
        .filter(|(line, ..)| !line.ends_with("/favicon.ico"))
        .collect();
    let rows = lines
        .iter()
        .rev()
        .take(RECENT_REQUESTS_SHOWN)
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
    (lines.len(), rows)
}

/// The other scenes this realm serves, folded into a drawer.
fn more_scenes_chips(others: &[Project]) -> String {
    if others.is_empty() {
        return String::new();
    }
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
        "<details class=\"drawer\"><summary>Also in this realm \u{b7} {}</summary><div class=\"drawer__body\"><div class=\"chips\">{rest}</div></div></details>",
        others.len()
    )
}

fn route_links(st: &AppState, prefix: &str, has_lan: bool) -> String {
    let prefix_esc = esc(prefix);
    let route = |path: &str| format!(r#"<a href="{prefix_esc}{path}"><code>{path}</code></a>"#);
    let mut routes = vec![
        route("/about"),
        route("/scene.json"),
        route("/scenes"),
        route("/preview-wearables"),
    ];
    if has_lan {
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
    routes.join(" ")
}

/// The one blob the script reads its state from. `<` is escaped so a scene
/// string can never close this tag and open one of its own.
fn edit_data_blob(prefix: &str, scene: &SceneData) -> String {
    let (gx0, gy0, gx1, gy1, _) = grid_bounds(&scene.grid);
    let list = |key: &str| {
        scene
            .json
            .get(key)
            .cloned()
            .unwrap_or_else(|| serde_json::json!([]))
    };
    serde_json::json!({
        "prefix": prefix,
        "tags": list("tags"),
        "parcels": scene.grid.iter().map(|(x, y)| format!("{x},{y}")).collect::<Vec<_>>(),
        "base": format!("{},{}", scene.base.0, scene.base.1),
        "grid": { "x0": gx0, "y0": gy0, "x1": gx1, "y1": gy1, "gap": 3 },
        "permissions": list("requiredPermissions"),
        "spawnPoints": scene.spawns,
    })
    .to_string()
    .replace('<', "\\u003c")
}

/// What both pages read out of the first project's scene.json.
struct SceneData<'a> {
    json: &'a Value,
    title: String,
    parcels: Vec<(i64, i64)>,
    base: (i64, i64),
    spawns: &'a [Value],
    /// A scene with no parcel list still draws a one-cell grid around its base.
    grid: Vec<(i64, i64)>,
}

fn scene_data(projects: &[Project]) -> SceneData<'_> {
    static EMPTY: Value = Value::Null;
    let json = projects.first().map_or(&EMPTY, |p| &p.scene_json);
    let (parcels, base) = parse_parcels(json);
    let grid = if parcels.is_empty() {
        vec![base]
    } else {
        parcels.clone()
    };
    SceneData {
        json,
        title: scene_title(json),
        parcels,
        base,
        spawns: spawn_points(json),
        grid,
    }
}

fn dash(sections: &str, edit_data: &str) -> String {
    format!(
        r##"<main class="dash">
{sections}
</main>
<script type="application/json" id="edit-data">{edit_data}</script>
<script>{SCRIPT}</script>
"##
    )
}

fn shell(
    st: &AppState,
    scene: &SceneData,
    prefix: &str,
    skip: (&str, &str),
    active: &str,
    host: &str,
    body: &str,
) -> String {
    let nav = Nav {
        active,
        badge: nav_badge(st),
        host,
        account: known_account(st),
        token: token(st),
    };
    document(&scene.title, prefix, "", skip.0, skip.1, Some(&nav), body)
}

fn render(
    st: &AppState,
    realm: &str,
    prefix: &str,
    mobile_realm: &str,
    lan_realm: Option<&str>,
    knobs: &Knobs,
) -> String {
    let projects = st.projects();
    let scene = scene_data(&projects);
    let mcp_on = knobs.mcp.unwrap_or(st.mcp);
    let targets = launch_targets(
        st,
        knobs,
        realm,
        lan_realm,
        mobile_realm,
        scene.base,
        mcp_on,
    );
    let selected = targets
        .iter()
        .position(|t| t.key == knobs.where_key)
        .unwrap_or(0);
    let (request_count, request_rows) = requests_drawer(st);

    // The landing page never carries the wallet panel: the CLI's printed URL
    // is /deploy, a page publish signs on /deploy — `/` is the preview, and
    // a wallet prompt on it would be a surprise wherever the visitor came
    // from.
    let sections = format!(
        r##"  <section id="join" class="sec">
    {join_control}
  </section>

  <section id="requests" class="sec">
    <details class="drawer"><summary>Recent requests · {request_count}</summary>
      <div class="drawer__body"><div class="reqs">{request_rows}</div></div></details>
    <div class="routes">{route_links}</div>
  </section>"##,
        join_control = join_control(
            &targets,
            selected,
            st.mcp,
            mcp_on,
            knobs,
            scene.spawns,
            prefix
        ),
        route_links = route_links(st, prefix, lan_realm.is_some()),
    );
    shell(
        st,
        &scene,
        prefix,
        ("#launch", "Skip to the launch button"),
        "preview",
        host_label(realm),
        &dash(&sections, &edit_data_blob(prefix, &scene)),
    )
}

/// `/scene` — the layout card with an Info tab holding the scene hero, so
/// every fact about the scene edits in one card under one sub-navigation.
pub(super) fn scene_page(st: &AppState, headers: &HeaderMap) -> Response {
    let prefix = forwarded_prefix(headers);
    let projects = st.projects();
    let scene = scene_data(&projects);
    let info = scene_card(
        projects.first(),
        &st.machine,
        &prefix,
        scene.json,
        &scene.title,
        scene.base,
        scene.parcels.len().max(1),
    );
    let sections = format!(
        r##"  <section id="scene" class="sec">
    {layout_card}
    {more_scenes}
  </section>"##,
        layout_card = scene_layout_card(scene.json, &scene.grid, scene.base, scene.spawns, &info),
        more_scenes = more_scenes_chips(projects.get(1..).unwrap_or_default()),
    );
    html(shell(
        st,
        &scene,
        &prefix,
        ("#scene", "Skip to the scene"),
        "scene",
        &format!("127.0.0.1:{}", st.port),
        &dash(&sections, &edit_data_blob(&prefix, &scene)),
    ))
}

#[cfg(test)]
#[path = "landing_tests.rs"]
mod tests;
