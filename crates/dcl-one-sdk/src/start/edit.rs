//! `POST /scene-json` and `POST /scene-thumbnail` — the landing page's
//! editors, registered outside the CORS layer behind the deploy gates. No
//! page token on purpose: `/` is CORS-readable (it doubles as the reload
//! websocket), so a token in its HTML protects nothing — the Origin gate is
//! the one that holds. The body is an allowlist: an unnamed field is
//! refused, not ignored.

use super::{forwarded_prefix, AppState};
use crate::scene::b64_content_hash;
use axum::extract::{ConnectInfo, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Map, Value};
use std::net::SocketAddr;
use std::path::{Component, Path};
use std::sync::{Arc, Mutex, PoisonError};

const MAX_TITLE: usize = 150;
const MAX_DESCRIPTION: usize = 5000;
const MAX_TAGS: usize = 50;
const MAX_TAG: usize = 64;
const MAX_SPAWNS: usize = 32;
const MAX_SPAWN_NAME: usize = 64;
const MAX_SPAWN_COORD: f64 = 10_000.0;
const MAX_THUMBNAIL_BYTES: usize = 2 * 1024 * 1024;

/// One writer at a time across both routes: each is a read-modify-write of
/// the same file, and two racing would resurrect each other's overwritten
/// fields.
static WRITE: Mutex<()> = Mutex::new(());

/// Every answer here is a status and one line of text.
fn reply(status: StatusCode, why: impl std::fmt::Display) -> Response {
    (status, format!("{why}\n")).into_response()
}

fn refuse(why: &str) -> Response {
    reply(StatusCode::FORBIDDEN, why)
}

/// The shared write gates, with no remote escape: scene.json is the
/// developer's file, so edits stay on the hosting machine even when
/// --allow-remote-deploy opened publishing up.
fn refused(peer: SocketAddr, headers: &HeaderMap) -> Option<Response> {
    if super::remote_peer(false, peer, headers) {
        return Some(refuse(
            "scene editing runs only on the machine hosting this preview",
        ));
    }
    super::cross_origin_refusal(headers).map(refuse)
}

/// Every field the page has an editor for, and nothing else. All optional:
/// a request names only what it changes.
#[derive(serde::Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SceneEdit {
    title: Option<String>,
    description: Option<String>,
    tags: Option<Vec<String>>,
    parcels: Option<Vec<String>>,
    base: Option<String>,
    required_permissions: Option<Vec<String>>,
    spawn_points: Option<Vec<SpawnEdit>>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SpawnEdit {
    name: String,
    #[serde(default)]
    default: bool,
    position: Xyz,
    camera_target: Option<Xyz>,
}

#[derive(serde::Deserialize)]
struct Xyz {
    x: Coord,
    y: Coord,
    z: Coord,
}

/// The schema's two coordinate shapes: a number, or a `[min, max]` range the
/// client rolls a position from. The page's editor writes numbers, but a
/// spawn it passes through untouched keeps its ranges.
#[derive(serde::Deserialize)]
#[serde(untagged)]
enum Coord {
    Num(f64),
    Range([f64; 2]),
}

pub(super) async fn scene_json(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Result<Json<SceneEdit>, axum::extract::rejection::JsonRejection>,
) -> Response {
    if let Some(refusal) = refused(peer, &headers) {
        return refusal;
    }
    let edit = match body {
        Ok(Json(edit)) => edit,
        Err(e) => return reply(StatusCode::UNPROCESSABLE_ENTITY, e),
    };
    let Some(project) = st.first_project() else {
        return reply(StatusCode::NOT_FOUND, "no scene loaded");
    };
    let outcome = tokio::task::spawn_blocking(move || {
        edit_scene_json(&st, &project.root, |scene| apply(scene, &edit))
    })
    .await;
    match outcome {
        Ok(Ok(scene)) => Json(scene).into_response(),
        Ok(Err((status, why))) => reply(status, why),
        Err(_) => reply(StatusCode::INTERNAL_SERVER_ERROR, "the edit did not finish"),
    }
}

/// The read-modify-write every scene.json writer shares (the landing
/// editors here, and /target's destination pointer). Disk is the source of
/// truth — not the in-memory copy — so a hand edit made since the last watch
/// batch is carried forward rather than overwritten.
pub(super) fn edit_scene_json(
    st: &AppState,
    root: &Path,
    change: impl FnOnce(&mut Value) -> Result<(), String>,
) -> Result<Value, (StatusCode, String)> {
    let _guard = WRITE.lock().unwrap_or_else(PoisonError::into_inner);
    let path = root.join("scene.json");
    let bytes = std::fs::read(&path).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("reading scene.json: {e}"),
        )
    })?;
    let mut scene: Value = serde_json::from_slice(&bytes).map_err(|e| {
        (
            StatusCode::CONFLICT,
            format!(
                "scene.json on disk is not valid JSON (line {}, column {}) — fix it there first",
                e.line(),
                e.column()
            ),
        )
    })?;
    if !scene.is_object() {
        return Err((
            StatusCode::CONFLICT,
            "scene.json on disk is not a JSON object".to_string(),
        ));
    }
    change(&mut scene).map_err(|why| (StatusCode::UNPROCESSABLE_ENTITY, why))?;
    let text = format!(
        "{}\n",
        serde_json::to_string_pretty(&scene).unwrap_or_default()
    );
    let tmp = root.join("scene.json.tmp");
    std::fs::write(&tmp, &text)
        .and_then(|()| std::fs::rename(&tmp, &path))
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("writing scene.json: {e}"),
            )
        })?;
    st.set_scene_json(root, scene.clone());
    Ok(scene)
}

/// One line of plain text: trimmed, capped, no control characters.
fn plain_text<'a>(s: &'a str, max: usize, what: &str) -> Result<&'a str, String> {
    let s = s.trim();
    if s.chars().count() > max || s.chars().any(char::is_control) {
        return Err(format!("{what} caps at {max} plain characters"));
    }
    Ok(s)
}

fn apply(scene: &mut Value, edit: &SceneEdit) -> Result<(), String> {
    if let Some(title) = &edit.title {
        let title = plain_text(title, MAX_TITLE, "the title")?;
        if title.is_empty() {
            return Err("the title cannot be empty".into());
        }
        set_display(scene, "title", Some(json!(title)));
    }
    if let Some(description) = &edit.description {
        let description = description.trim();
        if description.chars().count() > MAX_DESCRIPTION {
            return Err(format!(
                "the description caps at {MAX_DESCRIPTION} characters"
            ));
        }
        if description
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        {
            return Err("the description is text, not control characters".into());
        }
        set_display(
            scene,
            "description",
            (!description.is_empty()).then(|| json!(description)),
        );
    }
    if let Some(tags) = &edit.tags {
        let mut clean: Vec<String> = Vec::new();
        for tag in tags {
            let tag = plain_text(tag, MAX_TAG, "a tag")?;
            if tag.is_empty() || clean.iter().any(|t| t == tag) {
                continue;
            }
            clean.push(tag.to_string());
        }
        if clean.len() > MAX_TAGS {
            return Err(format!("at most {MAX_TAGS} tags"));
        }
        set_key(scene, "tags", (!clean.is_empty()).then(|| json!(clean)));
    }
    if edit.parcels.is_some() || edit.base.is_some() {
        apply_parcels(scene, edit)?;
    }
    if let Some(permissions) = &edit.required_permissions {
        apply_permissions(scene, permissions)?;
    }
    if let Some(spawns) = &edit.spawn_points {
        set_key(scene, "spawnPoints", spawn_values(spawns)?);
    }
    Ok(())
}

fn apply_parcels(scene: &mut Value, edit: &SceneEdit) -> Result<(), String> {
    let current = super::landing::parse_parcels(scene);
    let parcels = match &edit.parcels {
        Some(given) => {
            let mut parsed: Vec<(i64, i64)> = Vec::new();
            for p in given {
                let Some(xy) = catalyrst_auth_chain::pointer::parse_pointer(p) else {
                    return Err(format!("\"{}\" is not an x,y parcel", p.escape_default()));
                };
                if !parsed.contains(&xy) {
                    parsed.push(xy);
                }
            }
            parsed
        }
        None => current.0.clone(),
    };
    if parcels.is_empty() {
        return Err("a scene keeps at least one parcel".into());
    }
    if !connected(&parcels) {
        return Err("the parcels must form one edge-connected shape".into());
    }
    let base = match &edit.base {
        Some(b) => catalyrst_auth_chain::pointer::parse_pointer(b)
            .ok_or_else(|| format!("\"{}\" is not an x,y base parcel", b.escape_default()))?,
        None => current.1,
    };
    if !parcels.contains(&base) {
        return Err("the base parcel anchors the scene and must stay in the layout".into());
    }
    let mut sorted = parcels;
    sorted.sort_unstable();
    let obj = ensure_object(scene, "scene");
    obj.insert(
        "parcels".into(),
        json!(sorted
            .iter()
            .map(|(x, y)| format!("{x},{y}"))
            .collect::<Vec<_>>()),
    );
    obj.insert("base".into(), json!(format!("{},{}", base.0, base.1)));
    Ok(())
}

/// Deploy refuses a scattered layout, so the editor must too. It is the only
/// shape rule: parcel count and extent are the deployer's business.
fn connected(parcels: &[(i64, i64)]) -> bool {
    let set: std::collections::HashSet<(i64, i64)> = parcels.iter().copied().collect();
    let mut seen = std::collections::HashSet::new();
    let mut stack = vec![parcels[0]];
    while let Some((x, y)) = stack.pop() {
        if !seen.insert((x, y)) {
            continue;
        }
        for next in [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)] {
            if set.contains(&next) {
                stack.push(next);
            }
        }
    }
    seen.len() == set.len()
}

/// A permission is kept only if the page offered it or the scene already
/// required it, so an exotic key survives a toggle of its neighbours without
/// this endpoint becoming a way to write arbitrary strings.
fn apply_permissions(scene: &mut Value, permissions: &[String]) -> Result<(), String> {
    let existing: Vec<String> = scene
        .get("requiredPermissions")
        .and_then(|p| p.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let mut clean: Vec<&String> = Vec::new();
    for key in permissions {
        let offered = super::landing::PERMISSIONS.iter().any(|(k, ..)| k == key)
            || existing.iter().any(|e| e == key);
        if !offered {
            return Err(format!(
                "\"{}\" is not a permission this page offers",
                key.escape_default()
            ));
        }
        if !clean.contains(&key) {
            clean.push(key);
        }
    }
    set_key(
        scene,
        "requiredPermissions",
        (!clean.is_empty()).then(|| json!(clean)),
    );
    Ok(())
}

fn spawn_values(spawns: &[SpawnEdit]) -> Result<Option<Value>, String> {
    if spawns.len() > MAX_SPAWNS {
        return Err(format!("at most {MAX_SPAWNS} spawn points"));
    }
    let mut out = Vec::new();
    for spawn in spawns {
        let name = plain_text(&spawn.name, MAX_SPAWN_NAME, "a spawn point name")?;
        if name.is_empty() {
            return Err("a spawn point needs a name".into());
        }
        if out
            .iter()
            .any(|s: &Value| s.get("name").and_then(|n| n.as_str()) == Some(name))
        {
            return Err(format!(
                "two spawn points named \"{}\" — the deep link picks them by name",
                name.escape_default()
            ));
        }
        let mut value = json!({ "name": name, "position": xyz_value(&spawn.position)? });
        if spawn.default {
            value["default"] = json!(true);
        }
        if let Some(target) = &spawn.camera_target {
            value["cameraTarget"] = xyz_value(target)?;
        }
        out.push(value);
    }
    Ok((!out.is_empty()).then(|| json!(out)))
}

fn xyz_value(xyz: &Xyz) -> Result<Value, String> {
    let num = |v: f64| -> Result<Value, String> {
        if !v.is_finite() || v.abs() > MAX_SPAWN_COORD {
            return Err(format!(
                "a spawn coordinate stays within ±{MAX_SPAWN_COORD}"
            ));
        }
        Ok(if v.fract() == 0.0 {
            json!(v as i64)
        } else {
            json!(v)
        })
    };
    let coord = |c: &Coord| -> Result<Value, String> {
        match c {
            Coord::Num(v) => num(*v),
            Coord::Range([a, b]) => Ok(json!([num(*a)?, num(*b)?])),
        }
    };
    Ok(json!({ "x": coord(&xyz.x)?, "y": coord(&xyz.y)?, "z": coord(&xyz.z)? }))
}

fn ensure_object<'a>(scene: &'a mut Value, key: &str) -> &'a mut Map<String, Value> {
    let obj = scene.as_object_mut().expect("checked in edit_scene_json");
    if !obj.get(key).is_some_and(Value::is_object) {
        obj.insert(key.to_string(), json!({}));
    }
    obj.get_mut(key)
        .and_then(Value::as_object_mut)
        .expect("just inserted")
}

fn set_or_remove(obj: &mut Map<String, Value>, key: &str, value: Option<Value>) {
    match value {
        Some(v) => {
            obj.insert(key.to_string(), v);
        }
        None => {
            obj.remove(key);
        }
    }
}

fn set_display(scene: &mut Value, key: &str, value: Option<Value>) {
    set_or_remove(ensure_object(scene, "display"), key, value);
}

fn set_key(scene: &mut Value, key: &str, value: Option<Value>) {
    let obj = scene.as_object_mut().expect("checked in edit_scene_json");
    set_or_remove(obj, key, value);
}

/// (content type, file extensions it may keep, magic prefix)
const THUMBNAIL_TYPES: [(&str, &[&str], &[u8]); 3] = [
    ("image/png", &["png"], b"\x89PNG"),
    ("image/jpeg", &["jpg", "jpeg"], b"\xff\xd8\xff"),
    ("image/webp", &["webp"], b"RIFF"),
];

pub(super) async fn thumbnail(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    if let Some(refusal) = refused(peer, &headers) {
        return refusal;
    }
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let Some((_, exts, magic)) = THUMBNAIL_TYPES.iter().find(|(t, _, _)| *t == content_type) else {
        return reply(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "the thumbnail must be a png, jpeg or webp",
        );
    };
    if body.len() > MAX_THUMBNAIL_BYTES {
        return reply(StatusCode::PAYLOAD_TOO_LARGE, "a thumbnail caps at 2 MB");
    }
    if !body.starts_with(magic)
        || (content_type == "image/webp" && body.get(8..12) != Some(b"WEBP"))
    {
        return reply(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            format!("the bytes are not {content_type}"),
        );
    }
    let Some(project) = st.first_project() else {
        return reply(StatusCode::NOT_FOUND, "no scene loaded");
    };
    let rel = thumbnail_rel(&project.scene_json, exts);
    let abs = project.root.join(&rel);
    if let Some(parent) = abs.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return reply(
                StatusCode::INTERNAL_SERVER_ERROR,
                "the thumbnail folder could not be created",
            );
        }
    }
    if std::fs::write(&abs, &body).is_err() {
        return reply(
            StatusCode::INTERNAL_SERVER_ERROR,
            "the thumbnail could not be written",
        );
    }
    let repoint = edit_scene_json(&st, &project.root, |scene| {
        set_display(scene, "navmapThumbnail", Some(json!(rel)));
        Ok(())
    });
    if let Err((status, why)) = repoint {
        return reply(status, why);
    }
    let prefix = forwarded_prefix(&headers);
    let hash = b64_content_hash(&abs.display().to_string(), &st.machine);
    Json(json!({
        "navmapThumbnail": rel,
        "url": format!("{prefix}/content/contents/{hash}"),
    }))
    .into_response()
}

/// Overwrite the thumbnail scene.json already names when its extension
/// matches the upload; otherwise a fresh `scene-thumbnail.{ext}` at the root.
/// A named path that steps outside the project is never written to.
fn thumbnail_rel(scene_json: &Value, exts: &[&str]) -> String {
    let current = scene_json
        .get("display")
        .and_then(|d| d.get("navmapThumbnail"))
        .and_then(|t| t.as_str());
    if let Some(rel) = current {
        let ext = rel
            .rsplit('.')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if safe_rel(rel) && exts.contains(&ext.as_str()) {
            return rel.to_string();
        }
    }
    format!("scene-thumbnail.{}", exts[0])
}

fn safe_rel(rel: &str) -> bool {
    !rel.is_empty()
        && !rel.contains('\\')
        && Path::new(rel)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_title_is_trimmed_and_a_blank_one_refused() {
        let mut scene = json!({});
        let ok = apply(
            &mut scene,
            &SceneEdit {
                title: Some("  Gathering Stage  ".into()),
                ..Default::default()
            },
        );
        assert!(ok.is_ok());
        assert_eq!(scene["display"]["title"], json!("Gathering Stage"));
        for bad in ["", "   ", "a\nb"] {
            assert!(
                apply(
                    &mut scene,
                    &SceneEdit {
                        title: Some(bad.into()),
                        ..Default::default()
                    }
                )
                .is_err(),
                "{bad:?}"
            );
        }
        assert_eq!(scene["display"]["title"], json!("Gathering Stage"));
    }

    #[test]
    fn an_emptied_description_and_tags_leave_no_key_behind() {
        let mut scene = json!({
            "display": { "title": "T", "description": "old" },
            "tags": ["a"]
        });
        apply(
            &mut scene,
            &SceneEdit {
                description: Some("  ".into()),
                tags: Some(vec!["  ".into(), String::new()]),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(scene["display"].get("description").is_none());
        assert!(scene.get("tags").is_none());
        assert_eq!(
            scene["display"]["title"],
            json!("T"),
            "untouched fields stay"
        );
    }

    #[test]
    fn tags_dedupe_and_keep_their_order() {
        let mut scene = json!({});
        apply(
            &mut scene,
            &SceneEdit {
                tags: Some(vec!["events".into(), " theatre ".into(), "events".into()]),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(scene["tags"], json!(["events", "theatre"]));
    }

    fn parcels(parcels: &[&str]) -> SceneEdit {
        SceneEdit {
            parcels: Some(parcels.iter().map(|p| p.to_string()).collect()),
            ..Default::default()
        }
    }

    #[test]
    fn parcels_must_stay_connected_and_keep_the_base() {
        let mut scene = json!({ "scene": { "parcels": ["0,0"], "base": "0,0" } });
        apply(&mut scene, &parcels(&["0,0", "0,1"])).unwrap();
        assert_eq!(scene["scene"]["parcels"], json!(["0,0", "0,1"]));
        assert_eq!(scene["scene"]["base"], json!("0,0"));

        assert!(apply(&mut scene, &parcels(&["0,0", "2,2"]))
            .unwrap_err()
            .contains("edge-connected"));

        assert!(
            apply(&mut scene, &parcels(&["0,0", "1,1"])).is_err(),
            "diagonal adjacency is not adjacency"
        );

        assert!(apply(&mut scene, &parcels(&["0,1", "0,2"]))
            .unwrap_err()
            .contains("base parcel"));

        let rebase = SceneEdit {
            base: Some("0,1".into()),
            ..parcels(&["0,1", "0,0"])
        };
        apply(&mut scene, &rebase).unwrap();
        assert_eq!(scene["scene"]["base"], json!("0,1"));

        assert!(apply(&mut scene, &parcels(&["a,b"])).is_err());
        assert!(
            apply(&mut scene, &parcels(&[])).is_err(),
            "a scene keeps a parcel"
        );
    }

    /// There is no parcel-count or span cap here: a 500-parcel strip is a
    /// legal layout, and whether a target accepts it is the deployer's
    /// business. Connectivity is the one shape rule.
    #[test]
    fn a_big_connected_layout_is_not_second_guessed() {
        let strip: Vec<String> = (0..500).map(|x| format!("{x},0")).collect();
        let mut scene = json!({ "scene": { "parcels": ["0,0"], "base": "0,0" } });
        apply(
            &mut scene,
            &SceneEdit {
                parcels: Some(strip),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(scene["scene"]["parcels"].as_array().unwrap().len(), 500);
    }

    #[test]
    fn permissions_allow_the_offered_and_the_already_present_only() {
        let perms = |keys: &[&str]| SceneEdit {
            required_permissions: Some(keys.iter().map(|k| k.to_string()).collect()),
            ..Default::default()
        };
        let mut scene = json!({ "requiredPermissions": ["CUSTOM_X"] });
        apply(&mut scene, &perms(&["USE_FETCH", "CUSTOM_X"])).unwrap();
        assert_eq!(
            scene["requiredPermissions"],
            json!(["USE_FETCH", "CUSTOM_X"])
        );

        assert!(
            apply(&mut scene, &perms(&["CUSTOM_Y"])).is_err(),
            "a key never offered"
        );

        apply(&mut scene, &perms(&[])).unwrap();
        assert!(scene.get("requiredPermissions").is_none());
    }

    fn xyz(x: f64, y: f64, z: f64) -> Xyz {
        Xyz {
            x: Coord::Num(x),
            y: Coord::Num(y),
            z: Coord::Num(z),
        }
    }

    #[test]
    fn spawn_points_write_the_client_shape_and_refuse_twins() {
        let spawn = |name: &str| SpawnEdit {
            name: name.into(),
            default: name == "main",
            position: xyz(8.0, 0.0, 8.5),
            camera_target: Some(xyz(16.0, 1.0, 16.0)),
        };
        let spawns = |spawns: Vec<SpawnEdit>| SceneEdit {
            spawn_points: Some(spawns),
            ..Default::default()
        };
        let mut scene = json!({});
        apply(&mut scene, &spawns(vec![spawn("main"), spawn("side")])).unwrap();
        let written = scene["spawnPoints"].as_array().unwrap();
        assert_eq!(written.len(), 2);
        assert_eq!(written[0]["default"], json!(true));
        assert!(written[1].get("default").is_none(), "false is absence");
        assert_eq!(
            written[0]["position"],
            json!({ "x": 8, "y": 0, "z": 8.5 }),
            "whole coordinates write as integers"
        );
        assert_eq!(written[0]["cameraTarget"]["x"], json!(16));

        let twins = spawns(vec![spawn("main"), spawn("main")]);
        assert!(apply(&mut scene, &twins).unwrap_err().contains("main"));
        let far = SpawnEdit {
            position: xyz(1e6, 0.0, 0.0),
            camera_target: None,
            ..spawn("far")
        };
        assert!(apply(&mut scene, &spawns(vec![far])).is_err());

        let ranged = SpawnEdit {
            position: Xyz {
                x: Coord::Range([0.0, 3.0]),
                y: Coord::Num(0.0),
                z: Coord::Range([1.5, 2.0]),
            },
            camera_target: None,
            ..spawn("area")
        };
        apply(&mut scene, &spawns(vec![ranged])).unwrap();
        assert_eq!(
            scene["spawnPoints"][0]["position"],
            json!({ "x": [0, 3], "y": 0, "z": [1.5, 2] }),
            "a range survives the round trip"
        );

        apply(&mut scene, &spawns(vec![])).unwrap();
        assert!(
            scene.get("spawnPoints").is_none(),
            "empty means the default spawn"
        );
    }

    #[test]
    fn the_thumbnail_path_keeps_a_matching_name_and_never_escapes() {
        let png = ["png"].as_slice();
        let named = json!({ "display": { "navmapThumbnail": "images/thumb.png" } });
        assert_eq!(thumbnail_rel(&named, png), "images/thumb.png");
        let other_ext = json!({ "display": { "navmapThumbnail": "images/thumb.jpg" } });
        assert_eq!(thumbnail_rel(&other_ext, png), "scene-thumbnail.png");
        for escape in ["../thumb.png", "/etc/thumb.png", "a/../../thumb.png"] {
            let scene = json!({ "display": { "navmapThumbnail": escape } });
            assert_eq!(
                thumbnail_rel(&scene, png),
                "scene-thumbnail.png",
                "{escape}"
            );
        }
        assert_eq!(thumbnail_rel(&json!({}), png), "scene-thumbnail.png");
    }
}
