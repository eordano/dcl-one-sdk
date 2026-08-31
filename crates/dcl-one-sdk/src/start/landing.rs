//! The landing page at `/`: server-rendered, one GET form, plus [`SCRIPT`]
//! which turns the page into scene.json's editor. Progressive: the server
//! ships every editor control inert and the script enables them, so no-JS
//! degrades to a read-only page. All mutations are fetch POSTs to the
//! `edit.rs` routes (outside the CORS layer, loopback + same-origin gated);
//! nothing here POSTs as a form.

#[path = "join_card.rs"]
mod join_card;
#[path = "layout_card.rs"]
mod layout_card;

use super::chrome::{document, esc, html};
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

/// The `where` knob's choices, as the values that ride in the query string.
/// Stable names rather than indices, so a link keeps pointing at the target it
/// named even when the LAN row is missing.
/// How many of the buffered requests the drawer draws. The buffer holds
/// hundreds so the tail covers a whole scene load; a dozen is what someone
/// opening the drawer actually reads, and the rest was a thousand pixels of
/// dev-server log under a page whose subject is a launch button.
const RECENT_REQUESTS_SHOWN: usize = 12;

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
        .and_then(|t| t.as_str())?;
    let abs = project.root.join(rel);
    if !abs.is_file() {
        return None;
    }
    let hash = b64_content_hash(&abs.display().to_string(), machine);
    Some(format!("{prefix}/content/contents/{hash}"))
}

/// The page's behaviour, inlined like the stylesheet. Everything it writes
/// goes through `/scene-json` and `/scene-thumbnail`; its state rides in the
/// `#edit-data` JSON blob the page renders beside it.
const SCRIPT: &str = concat!(
    include_str!("page_common.js"),
    include_str!("landing_edit.js")
);

/// The scene hero: cover (the thumbnail editor's click target), title,
/// position line, description and tags — every piece an editor target.
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
        .and_then(|d| d.as_str())
        .unwrap_or("");
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
    // 17 KB of data url, encoded only when the phone tab shows it.
    let qr_img = match knobs.where_key == WHERE_PHONE {
        true => qr_data_url(&mobile)
            .map(|qr| {
                format!(r#"<span class="qr"><img src="{qr}" alt="" width="96" height="96"></span>"#)
            })
            .unwrap_or_default(),
        false => String::new(),
    };

    let this_machine = realm_carry(realm);
    let mut targets = vec![Target {
        key: WHERE_DESKTOP,
        label: "This machine",
        hint: "Opens the installed desktop explorer on this machine, in this realm".to_string(),
        url: desktop_deep_link(realm, position, ab, &extra(this_machine)),
        qr: String::new(),
        carry: this_machine,
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
        });
    }
    targets.push(Target {
        key: WHERE_WEB,
        label: "Web explorer",
        hint: "Opens the web explorer in this browser — no install".to_string(),
        url: web_join_url(&joinblock::web_explorer_base(), realm, position),
        qr: String::new(),
        carry: Carry::Nothing,
    });
    targets.push(Target {
        key: WHERE_PHONE,
        label: "Phone",
        hint: "Scan with the phone camera to open this preview there".to_string(),
        url: mobile,
        qr: qr_img,
        carry: Carry::Nothing,
    });
    targets
}

/// The request drawer's count and rows. Only the dozen drawn rows are cloned
/// and escaped; the buffer holds up to 200 attacker-influenced lines and the
/// rest are just counted.
fn requests_drawer(st: &AppState) -> (usize, String) {
    let (count, recent): (usize, Vec<(String, u16, Instant)>) = match st.recent_requests.lock() {
        Ok(buffer) => {
            let shown: Vec<_> = buffer
                .iter()
                .rev()
                .filter(|(line, _, _)| !line.ends_with("/favicon.ico"))
                .take(RECENT_REQUESTS_SHOWN)
                .cloned()
                .collect();
            let count = buffer
                .iter()
                .filter(|(line, _, _)| !line.ends_with("/favicon.ico"))
                .count();
            (count, shown)
        }
        Err(_) => (0, Vec::new()),
    };
    let rows = recent
        .iter()
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
    (count, rows)
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
fn edit_data_blob(
    prefix: &str,
    scene_json: &Value,
    grid_parcels: &[(i64, i64)],
    base: (i64, i64),
    spawns: &[Value],
) -> String {
    let (gx0, gy0, gx1, gy1, _) = grid_bounds(grid_parcels);
    serde_json::json!({
        "prefix": prefix,
        "tags": scene_json.get("tags").cloned().unwrap_or_else(|| Value::Array(vec![])),
        "parcels": grid_parcels
            .iter()
            .map(|(x, y)| format!("{x},{y}"))
            .collect::<Vec<_>>(),
        "base": format!("{},{}", base.0, base.1),
        "grid": { "x0": gx0, "y0": gy0, "x1": gx1, "y1": gy1, "gap": 3 },
        "permissions": scene_json
            .get("requiredPermissions")
            .cloned()
            .unwrap_or_else(|| Value::Array(vec![])),
        "spawnPoints": spawns,
    })
    .to_string()
    .replace('<', "\\u003c")
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
    let empty = Value::Null;
    let scene_json: &Value = projects.first().map(|p| &p.scene_json).unwrap_or(&empty);
    let title = scene_title(scene_json);
    let (parcels, base) = parse_parcels(scene_json);
    let spawns: Vec<Value> = scene_json
        .get("spawnPoints")
        .and_then(|s| s.as_array())
        .cloned()
        .unwrap_or_default();
    // A scene with no parcel list still draws a one-cell grid around its base.
    let grid_parcels = if parcels.is_empty() {
        vec![base]
    } else {
        parcels.clone()
    };
    let mcp_on = knobs.mcp.unwrap_or(st.mcp);

    let targets = launch_targets(st, knobs, realm, lan_realm, mobile_realm, base, mcp_on);
    let selected = targets
        .iter()
        .position(|t| t.key == knobs.where_key)
        .unwrap_or(0);
    let (request_count, request_rows) = requests_drawer(st);

    // The landing page never carries the wallet panel: the CLI's printed URL
    // is /deploy, a page publish signs on /deploy — `/` is the preview, and
    // a wallet prompt on it would be a surprise wherever the visitor came
    // from.
    let body = format!(
        r##"<main class="dash">
  <section id="join" class="sec">
    {join_control}
  </section>

  <section id="requests" class="sec">
    <details class="drawer"><summary>Recent requests · {request_count}</summary>
      <div class="drawer__body"><div class="reqs">{request_rows}</div></div></details>
    <div class="routes">{route_links}</div>
  </section>
</main>
<script type="application/json" id="edit-data">{edit_data}</script>
<script>{SCRIPT}</script>
"##,
        join_control = join_control(&targets, selected, st.mcp, mcp_on, knobs, &spawns, prefix),
        route_links = route_links(st, prefix, lan_realm.is_some()),
        edit_data = edit_data_blob(prefix, scene_json, &grid_parcels, base, &spawns),
    );
    document(
        &title,
        prefix,
        "",
        "#launch",
        "Skip to the launch button",
        Some(&super::chrome::Nav {
            active: "preview",
            badge: super::deploy_page::nav_badge(st),
            host: host_label(realm),
            account: super::deploy_page::known_account(st),
            token: super::deploy_page::token(st),
        }),
        &body,
    )
}

/// `/scene` — the scene section: the layout card grown an Info tab that holds
/// what used to be the landing hero (cover, title, description, tags), so
/// every fact about the scene edits in one card under one sub-navigation.
pub(super) fn scene_page(st: &AppState, headers: &HeaderMap) -> Response {
    let prefix = forwarded_prefix(headers);
    let projects = st.projects();
    let empty = Value::Null;
    let scene_json: &Value = projects.first().map(|p| &p.scene_json).unwrap_or(&empty);
    let title = scene_title(scene_json);
    let (parcels, base) = parse_parcels(scene_json);
    let spawns: Vec<Value> = scene_json
        .get("spawnPoints")
        .and_then(|s| s.as_array())
        .cloned()
        .unwrap_or_default();
    let grid_parcels = if parcels.is_empty() {
        vec![base]
    } else {
        parcels.clone()
    };
    let info = scene_card(
        projects.first(),
        &st.machine,
        &prefix,
        scene_json,
        &title,
        base,
        parcels.len().max(1),
    );
    let body = format!(
        r##"<main class="dash">
  <section id="scene" class="sec">
    {layout_card}
    {more_scenes}
  </section>
</main>
<script type="application/json" id="edit-data">{edit_data}</script>
<script>{SCRIPT}</script>
"##,
        layout_card = scene_layout_card(scene_json, &grid_parcels, base, &spawns, &info),
        more_scenes = more_scenes_chips(projects.get(1..).unwrap_or_default()),
        edit_data = edit_data_blob(&prefix, scene_json, &grid_parcels, base, &spawns),
    );
    html(document(
        &title,
        &prefix,
        "",
        "#scene",
        "Skip to the scene",
        Some(&super::chrome::Nav {
            active: "scene",
            badge: super::deploy_page::nav_badge(st),
            host: &format!("127.0.0.1:{}", st.port),
            account: super::deploy_page::known_account(st),
            token: super::deploy_page::token(st),
        }),
        &body,
    ))
}

#[cfg(test)]
#[path = "landing_tests.rs"]
mod tests;
