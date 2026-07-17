//! Human-facing landing page served at `/` for browser requests.
//!
//! Explorers never load this: they resolve the realm through `/about` and the
//! live-reload clients upgrade `/` to a websocket. Only a person pasting the
//! preview URL into a browser sees HTML, so the page previews the scene the
//! way Decentraland surfaces it — a What's On style card, the parcel layout,
//! spawn points and permissions — plus the join links.

use super::{forwarded_host, forwarded_prefix, forwarded_proto, AppState};
use crate::joinblock::{self, desktop_deep_link, mobile_deep_link, scene_title, web_join_url};
use crate::netinfo;
use crate::scene::b64_hash;
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
        // A phone cannot reach the host's loopback; prefer the LAN address.
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

/// "16" for 16 or [16,16]; "0–4" for [0,4].
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
                .filter_map(|s| {
                    let (x, y) = s.split_once(',')?;
                    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
                })
                .collect()
        })
        .unwrap_or_default();
    let base = scene_json
        .get("scene")
        .and_then(|s| s.get("base"))
        .and_then(|b| b.as_str())
        .and_then(|s| {
            let (x, y) = s.split_once(',')?;
            Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
        })
        .unwrap_or_else(|| parcels.first().copied().unwrap_or((0, 0)));
    (parcels, base)
}

/// Mini-map of the parcel layout (north up), base parcel accented, spawn
/// points dotted at their in-scene position.
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
        // north (higher y) renders at the top; the coordinate labels make the
        // orientation and the base parcel self-describing
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
        // spawn positions are meters relative to the base parcel's SW corner
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

/// The scene.json fields that configure how the scene runs, shown verbatim
/// (dotted key paths, raw values) rather than paraphrased.
fn setup_rows(scene_json: &Value) -> String {
    let mut rows: Vec<(String, String)> = Vec::new();
    let mut push = |key: &str, v: Option<&Value>| {
        if let Some(v) = v {
            let shown = match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            rows.push((key.to_string(), shown));
        }
    };
    push("main", scene_json.get("main"));
    push("runtimeVersion", scene_json.get("runtimeVersion"));
    push("ecs7", scene_json.get("ecs7"));
    push(
        "worldConfiguration.name",
        scene_json
            .get("worldConfiguration")
            .and_then(|w| w.get("name")),
    );
    if let Some(toggles) = scene_json.get("featureToggles").and_then(|t| t.as_object()) {
        for (k, v) in toggles {
            let key = format!("featureToggles.{k}");
            rows.push((
                key,
                match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                },
            ));
        }
    }
    rows.iter()
        .map(|(k, v)| {
            format!(
                r#"<div class="kv"><span class="k">{}</span><code>{}</code></div>"#,
                esc(k),
                esc(v)
            )
        })
        .collect()
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
    let hash = b64_hash(&abs.display().to_string(), &st.machine);
    Some(format!("{prefix}/content/contents/{hash}"))
}

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
    let position = st.base;
    let (parcels, base) = parse_parcels(&scene_json);
    let spawns: Vec<Value> = scene_json
        .get("spawnPoints")
        .and_then(|s| s.as_array())
        .cloned()
        .unwrap_or_default();
    let map_svg = parcels_svg(&parcels, base, &spawns);

    let ab = st.optimized_assets_url.get().map(String::as_str);
    let desktop = desktop_deep_link(realm, position, ab, "");
    let desktop2 = desktop_deep_link(realm, position, ab, "&multi-instance=true");
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
            format!(
                r#"<a class="qr" href="{m}" title="open in the Decentraland mobile app"><img src="{qr}" alt="" width="96" height="96"></a>"#,
                m = esc(&mobile)
            )
        })
        .unwrap_or_default();

    let setup_rows = setup_rows(&scene_json);
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
    let url_link = |url: &str| format!(r#"<a href="{u}"><code>{u}</code></a>"#, u = esc(url));
    // decentraland.org/play is sunset — a web row only makes sense when a
    // custom web explorer is configured via DCL_ONE_SDK_WEB_EXPLORER.
    let web_explorer = joinblock::web_explorer_base();
    let web_row = if web_explorer != joinblock::DEFAULT_WEB_EXPLORER {
        let web = web_join_url(&web_explorer, realm, position);
        kv("web explorer", url_link(&web))
    } else {
        String::new()
    };
    // abgen binds all interfaces; the network link advertises it under the
    // LAN address the joining device can reach, not this machine's loopback.
    let network_row = lan_realm
        .map(|lan| {
            let lan_host = lan
                .trim_start_matches("http://")
                .rsplit_once(':')
                .map(|(host, _)| host)
                .unwrap_or(lan);
            let lan_assets = ab.map(|u| joinblock::swap_url_host(u, lan_host));
            kv(
                "network · another device",
                url_link(&desktop_deep_link(lan, position, lan_assets.as_deref(), "")),
            )
        })
        .unwrap_or_default();
    let link_rows: String = [
        ("desktop app", &desktop),
        ("desktop · 2nd instance", &desktop2),
    ]
    .into_iter()
    .map(|(label, url)| kv(label, url_link(url)))
    .chain(std::iter::once(network_row))
    .chain(std::iter::once(web_row))
    .chain(std::iter::once(kv(
        "qr / mobile app",
        format!("{}{qr_img}", url_link(&mobile)),
    )))
    .collect();

    let ws_proto = if realm.starts_with("https") {
        "wss"
    } else {
        "ws"
    };
    let comms_adapter = if st.offline_comms {
        "offline:offline".to_string()
    } else {
        let host_part = realm.split("://").nth(1).unwrap_or(realm);
        format!("ws-room:{ws_proto}://{host_part}/mini-comms/room-1")
    };
    let abgen = match st.optimized_assets_url.get() {
        Some(url) => url_link(url),
        None => r#"<code>not running</code>"#.to_string(),
    };
    let server_rows: String = [
        kv("realm", format!("<code>{}</code>", esc(realm))),
        kv("comms", format!("<code>{}</code>", esc(&comms_adapter))),
        kv("asset bundles (abgen)", abgen),
    ]
    .into_iter()
    .collect();

    let request_rows: String = st
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
                    format!("<div>{status} {} · {ago}</div>", esc(line))
                })
                .collect()
        })
        .unwrap_or_default();
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
            r#"<div class="row"><h3>also in this realm</h3><div class="chips">{rest}</div></div>"#
        )
    } else {
        String::new()
    };

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title_esc} — dcl-one-sdk preview</title>
<style>
  :root {{ color-scheme: dark; }}
  * {{ box-sizing: border-box; }}
  body {{ margin: 0; padding: 2rem 1.25rem 3rem; background: #0d0e12; color: #e8e6f0;
         font: 15px/1.5 ui-sans-serif, system-ui, -apple-system, "Segoe UI", sans-serif;
         display: flex; justify-content: center; }}
  main {{ max-width: 52rem; width: 100%; }}
  a {{ color: #ff2d55; text-decoration: none; }}
  .hero {{ display: flex; gap: 1.5rem; flex-wrap: wrap; align-items: flex-start; }}
  .card {{ width: 21rem; max-width: 100%; background: #16171d; border-radius: 16px;
           overflow: hidden; box-shadow: 0 8px 30px rgba(0,0,0,.45); }}
  .cover {{ display: block; width: 100%; aspect-ratio: 16/10; object-fit: cover; }}
  .cover.placeholder {{ display: flex; align-items: center; justify-content: center;
        background: linear-gradient(135deg, #ff2d55 0%, #3c1f8f 100%); }}
  .cover.placeholder span {{ font-size: 4rem; font-weight: 800; color: rgba(255,255,255,.85); }}
  .card .body {{ padding: .9rem 1rem 1rem; }}
  .card h1 {{ font-size: 1.1rem; margin: 0 0 .3rem; line-height: 1.3; }}
  .card .pos {{ color: #8f8ba3; font-size: .85rem; }}
  .card p {{ margin: .5rem 0 0; color: #b5b1c8; font-size: .85rem;
             display: -webkit-box; -webkit-line-clamp: 4; -webkit-box-orient: vertical; overflow: hidden; }}
  .tag {{ display: inline-block; background: #2a2b37; color: #c9c5dd; border-radius: 999px;
          padding: .1rem .6rem; font-size: .75rem; margin: .5rem .3rem 0 0; }}
  .facts {{ flex: 1; min-width: 16rem; display: flex; flex-direction: column; gap: .9rem; }}
  .row h3 {{ margin: 0 0 .35rem; font-size: .75rem; text-transform: uppercase;
             letter-spacing: .08em; color: #8f8ba3; font-weight: 600; }}
  .chips {{ display: flex; flex-wrap: wrap; gap: .35rem; }}
  .chip {{ background: #1e1f28; border: 1px solid #2c2d39; border-radius: 8px;
           padding: .25rem .6rem; font-size: .82rem; color: #d8d5e6; }}
  .chip.dim {{ color: #8f8ba3; }}
  .chip.perm {{ border-color: #55361f; background: #251c14; color: #f0b27a; }}
  .chip.mono {{ font-family: ui-monospace, Menlo, monospace; font-size: .78rem; }}
  .kvs {{ display: flex; flex-direction: column; gap: .2rem; }}
  .kv {{ display: flex; gap: .6rem; align-items: baseline; font-size: .84rem; flex-wrap: wrap; }}
  .kv .k {{ color: #8f8ba3; min-width: 11.5rem; }}
  .kv code {{ font: .8rem ui-monospace, Menlo, monospace; color: #d8d5e6; overflow-wrap: anywhere; }}
  .kv a code {{ color: #ff8fa6; }}
  .row.wide {{ margin-top: 1.75rem; }}
  .reqs {{ margin-top: .5rem; font: .76rem ui-monospace, Menlo, monospace; color: #8f8ba3;
           display: flex; flex-direction: column; gap: .15rem; }}
  .map {{ display: flex; align-items: center; gap: .75rem; }}
  .map .pos {{ color: #8f8ba3; font-size: .85rem; }}
  .qr img {{ display: block; background: #fff; border-radius: 8px; padding: 4px; }}
  .kv:has(.qr) {{ align-items: center; }}
</style>
</head>
<body>
<main>
  <div class="hero">
    <div class="card">
      {hero_media}
      <div class="body">
        <h1>{title_esc}</h1>
        <div class="pos">at {x},{y} · {parcel_count} parcel{parcel_plural}</div>
        <p>{description_esc}</p>
        <div>{tags}</div>
      </div>
    </div>
    <div class="facts">
      <div class="row"><h3>parcels</h3><div class="map">{map_svg}{world_note}</div></div>
      <div class="row"><h3>spawn</h3><div class="chips">{spawn_chips}</div></div>
      <div class="row"><h3>permissions</h3><div class="chips">{permission_chips}</div></div>
      <div class="row"><h3>setup</h3><div class="kvs">{setup_rows}</div></div>
      {more_scenes}
    </div>
  </div>

  <div class="row wide"><h3>links</h3><div class="kvs">{link_rows}</div></div>
  <div class="row wide"><h3>server</h3><div class="kvs">{server_rows}</div>
    <div class="reqs">{request_rows}</div></div>
</main>
</body>
</html>
"#,
        title_esc = esc(&title),
        x = position.0,
        y = position.1,
        parcel_count = parcels.len().max(1),
        parcel_plural = if parcels.len() == 1 { "" } else { "s" },
        description_esc = esc(description),
        spawn_chips = spawn_chips(&scene_json),
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
    fn permission_chips_label_known_keys() {
        let chips = permission_chips(&json!({ "requiredPermissions": ["USE_FETCH", "CUSTOM_X"] }));
        assert!(chips.contains("http fetch"));
        assert!(chips.contains("CUSTOM_X"));
        assert!(permission_chips(&json!({})).contains("none required"));
    }
}
