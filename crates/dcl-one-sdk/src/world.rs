use crate::deploy::{encode_segment, load_signer, now_ms};
use crate::ux::{self, TrySteps, UserError};
use anyhow::{Context, Result};
use catalyrst_crypto::Wallet;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct SettingsUpdate {
    pub title: Option<String>,
    pub description: Option<String>,
    pub content_rating: Option<String>,
    pub spawn_coordinates: Option<String>,
    pub skybox_time: Option<String>,
    pub single_player: Option<bool>,
    pub show_in_places: Option<bool>,
    pub categories: Vec<String>,
    pub thumbnail: Option<PathBuf>,
}

impl SettingsUpdate {
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.description.is_none()
            && self.content_rating.is_none()
            && self.spawn_coordinates.is_none()
            && self.skybox_time.is_none()
            && self.single_player.is_none()
            && self.show_in_places.is_none()
            && self.categories.is_empty()
            && self.thumbnail.is_none()
    }
}

pub fn resolve_target(target_content: Option<&str>) -> Result<String> {
    if let Some(t) = target_content {
        return Ok(t.trim().trim_end_matches('/').to_string());
    }
    if let Ok(t) = std::env::var("DCL_ONE_SDK_DEFAULT_TARGET") {
        let base = crate::deploy::sanitize_catalyst_url(&t);
        ux::note(format!(
            "using DCL_ONE_SDK_DEFAULT_TARGET as the worlds server: {base}"
        ));
        return Ok(base);
    }
    Err(UserError::new(
        "no worlds server given",
        TrySteps::one("pass --target-content <worlds-content-server-url>")
            .and("the public worlds server is https://worlds-content-server.decentraland.org")
            .and("or set DCL_ONE_SDK_DEFAULT_TARGET=<url>"),
    )
    .into())
}

fn require_signer(sign_key: Option<&Path>) -> Result<Wallet> {
    match load_signer(sign_key)? {
        Some(signer) => Ok(signer),
        None => Err(UserError::new(
            "no wallet available to sign this world request",
            TrySteps::one("set DCL_PRIVATE_KEY=<hex> (the world owner or a permitted deployer)")
                .and("or pass --sign-key <path-to-key-file>"),
        )
        .into()),
    }
}

fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .context("building the http client")
}

pub fn signed_headers(signer: &Wallet, method: &str, path: &str) -> Result<Vec<(String, String)>> {
    let timestamp = now_ms().to_string();
    let metadata = "{}";
    let payload = format!("{method}:{path}:{timestamp}:{metadata}").to_lowercase();
    let chain = catalyrst_crypto::create_simple_auth_chain(signer, &payload)
        .context("EIP-191 sign of the signed-fetch payload")?;
    let mut headers = vec![
        ("x-identity-timestamp".to_string(), timestamp),
        ("x-identity-metadata".to_string(), metadata.to_string()),
    ];
    for (i, link) in chain.as_array().into_iter().flatten().enumerate() {
        headers.push((format!("x-identity-auth-chain-{i}"), link.to_string()));
    }
    Ok(headers)
}

fn refused(action: &str, world: &str, status: u16, body: &str) -> anyhow::Error {
    let steps = if status == 401 || status == 403 {
        TrySteps::one(format!(
            "check the signing wallet owns {world} (or holds the needed permission)"
        ))
        .and("world permissions list <name> shows the owner and allow-lists")
    } else {
        TrySteps::one("read the server message above")
            .and("re-run with --verbose for the full response")
    };
    let mut u = UserError::new(
        format!("the worlds server refused to {action} (HTTP {status})"),
        steps,
    );
    let body = body.trim();
    if !body.is_empty() {
        u = u.why(body.to_string());
    }
    u.into()
}

fn unreachable(url: &str, e: reqwest::Error) -> anyhow::Error {
    UserError::new(
        "could not reach the worlds server",
        TrySteps::one("check the server is running and the URL is right")
            .and("pass --target-content <worlds-content-server-url>"),
    )
    .why(format!("request failed: {url}"))
    .caused_by(e)
    .into()
}

pub async fn settings_get(name: &str, target_content: Option<&str>) -> Result<()> {
    let base = resolve_target(target_content)?;
    let url = format!("{base}/world/{}/settings", encode_segment(name));
    let resp = match client()?.get(&url).send().await {
        Ok(resp) => resp,
        Err(e) => return Err(unreachable(&url, e)),
    };
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(refused("read the settings", name, status.as_u16(), &body));
    }
    let mut steps = ux::Steps::new(1);
    match serde_json::from_str::<Value>(&body) {
        Ok(v) => println!("{}", serde_json::to_string_pretty(&v)?),
        Err(_) => println!("{body}"),
    }
    steps.done(format!("Settings fetched for {name}"));
    Ok(())
}

pub async fn settings_set(
    name: &str,
    target_content: Option<&str>,
    sign_key: Option<&Path>,
    update: SettingsUpdate,
) -> Result<()> {
    if update.is_empty() {
        return Err(UserError::new(
            "nothing to update \u{2014} no settings flags given",
            TrySteps::one(
                "pass at least one of --title --description --content-rating --spawn-coordinates --skybox-time --single-player --show-in-places --category --thumbnail",
            ),
        )
        .into());
    }
    let base = resolve_target(target_content)?;
    let signer = require_signer(sign_key)?;
    let path = format!("/world/{}/settings", encode_segment(name));
    let url = format!("{base}{path}");

    let mut form = reqwest::multipart::Form::new();
    if let Some(v) = &update.title {
        form = form.text("title", v.clone());
    }
    if let Some(v) = &update.description {
        form = form.text("description", v.clone());
    }
    if let Some(v) = &update.content_rating {
        form = form.text("content_rating", v.clone());
    }
    if let Some(v) = &update.spawn_coordinates {
        form = form.text("spawn_coordinates", v.clone());
    }
    if let Some(v) = &update.skybox_time {
        form = form.text("skybox_time", v.clone());
    }
    if let Some(v) = update.single_player {
        form = form.text("single_player", v.to_string());
    }
    if let Some(v) = update.show_in_places {
        form = form.text("show_in_places", v.to_string());
    }
    for c in &update.categories {
        form = form.text("categories", c.clone());
    }
    if let Some(thumb) = &update.thumbnail {
        let bytes = std::fs::read(thumb).map_err(|e| {
            anyhow::Error::from(
                UserError::new(
                    format!("could not read the thumbnail {}", thumb.display()),
                    TrySteps::one("check the --thumbnail path"),
                )
                .caused_by(e),
            )
        })?;
        let file_name = thumb
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| "thumbnail.png".to_string());
        form = form.part(
            "thumbnail",
            reqwest::multipart::Part::bytes(bytes).file_name(file_name),
        );
    }

    let mut req = client()?.put(&url).multipart(form);
    for (k, v) in signed_headers(&signer, "put", &path)? {
        req = req.header(k, v);
    }
    let resp = match req.send().await {
        Ok(resp) => resp,
        Err(e) => return Err(unreachable(&url, e)),
    };
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(refused("update the settings", name, status.as_u16(), &body));
    }
    let mut steps = ux::Steps::new(1);
    if let Ok(v) = serde_json::from_str::<Value>(&body) {
        if let Some(settings) = v.get("settings") {
            println!("{}", serde_json::to_string_pretty(settings)?);
        }
    }
    steps.done(format!("Settings updated for {name}"));
    Ok(())
}

pub async fn permissions_list(name: &str, target_content: Option<&str>) -> Result<()> {
    let base = resolve_target(target_content)?;
    let url = format!("{base}/world/{}/permissions", encode_segment(name));
    let resp = match client()?.get(&url).send().await {
        Ok(resp) => resp,
        Err(e) => return Err(unreachable(&url, e)),
    };
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(refused(
            "list the permissions",
            name,
            status.as_u16(),
            &body,
        ));
    }
    let v: Value = serde_json::from_str(&body).context("parsing the permissions response")?;
    let mut steps = ux::Steps::new(1);
    println!("{}", render_permissions(name, &v));
    steps.done(format!("Permissions fetched for {name}"));
    Ok(())
}

pub fn render_permissions(name: &str, v: &Value) -> String {
    let mut out = String::new();
    out.push_str(&format!("world: {name}\n"));
    let owner = v
        .get("owner")
        .and_then(|o| o.as_str())
        .unwrap_or("(unknown)");
    out.push_str(&format!("owner: {owner}\n"));
    let perms = v.get("permissions").cloned().unwrap_or_default();
    for kind in ["deployment", "streaming"] {
        let wallets: Vec<String> = perms
            .get(kind)
            .and_then(|p| p.get("wallets"))
            .and_then(|w| w.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let ty = perms
            .get(kind)
            .and_then(|p| p.get("type"))
            .and_then(|t| t.as_str())
            .unwrap_or("allow-list");
        if wallets.is_empty() {
            out.push_str(&format!("{kind}: {ty} (no extra wallets)\n"));
        } else {
            out.push_str(&format!("{kind}: {ty}\n"));
            for w in wallets {
                out.push_str(&format!("  - {w}\n"));
            }
        }
    }
    let access = perms
        .get("access")
        .and_then(|a| a.get("type"))
        .and_then(|t| t.as_str())
        .unwrap_or("unrestricted");
    out.push_str(&format!("access: {access}"));
    out
}

const GRANTABLE: [&str; 3] = ["deployment", "streaming", "access"];

fn check_permission_name(permission: &str) -> Result<()> {
    if GRANTABLE.contains(&permission) {
        return Ok(());
    }
    Err(UserError::new(
        format!("\"{permission}\" is not a grantable permission"),
        TrySteps::one(format!("use one of: {}", GRANTABLE.join(", "))),
    )
    .into())
}

fn check_address(address: &str) -> Result<()> {
    let hexpart = address.strip_prefix("0x").unwrap_or("");
    if hexpart.len() == 40 && hexpart.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(());
    }
    Err(UserError::new(
        format!("\"{address}\" is not an ethereum address"),
        TrySteps::one("expect 0x + 40 hex chars"),
    )
    .into())
}

async fn permissions_change(
    name: &str,
    permission: &str,
    address: &str,
    target_content: Option<&str>,
    sign_key: Option<&Path>,
    revoke: bool,
) -> Result<()> {
    check_permission_name(permission)?;
    check_address(address)?;
    let base = resolve_target(target_content)?;
    let signer = require_signer(sign_key)?;
    let path = format!(
        "/world/{}/permissions/{}/{}",
        encode_segment(name),
        encode_segment(permission),
        encode_segment(&address.to_lowercase())
    );
    let url = format!("{base}{path}");
    let (method, verb) = if revoke {
        (reqwest::Method::DELETE, "delete")
    } else {
        (reqwest::Method::PUT, "put")
    };
    let mut req = client()?.request(method, &url);
    for (k, v) in signed_headers(&signer, verb, &path)? {
        req = req.header(k, v);
    }
    let resp = match req.send().await {
        Ok(resp) => resp,
        Err(e) => return Err(unreachable(&url, e)),
    };
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        let action = if revoke {
            format!("revoke {permission} from {address}")
        } else {
            format!("grant {permission} to {address}")
        };
        return Err(refused(&action, name, status.as_u16(), &body));
    }
    let mut steps = ux::Steps::new(1);
    if revoke {
        steps.done(format!("Revoked {permission} from {address} on {name}"));
    } else {
        steps.done(format!("Granted {permission} to {address} on {name}"));
    }
    Ok(())
}

pub async fn permissions_grant(
    name: &str,
    permission: &str,
    address: &str,
    target_content: Option<&str>,
    sign_key: Option<&Path>,
) -> Result<()> {
    permissions_change(name, permission, address, target_content, sign_key, false).await
}

pub async fn permissions_revoke(
    name: &str,
    permission: &str,
    address: &str,
    target_content: Option<&str>,
    sign_key: Option<&Path>,
) -> Result<()> {
    permissions_change(name, permission, address, target_content, sign_key, true).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn signed_headers_carry_a_verifiable_lowercased_payload() {
        let signer = crate::random_test_wallet();
        let headers = signed_headers(&signer, "put", "/world/Test.dcl.eth/settings").unwrap();
        let ts = &headers[0];
        assert_eq!(ts.0, "x-identity-timestamp");
        assert!(ts.1.chars().all(|c| c.is_ascii_digit()));
        assert_eq!(
            headers[1],
            ("x-identity-metadata".to_string(), "{}".to_string())
        );
        let link0: Value = serde_json::from_str(&headers[2].1).unwrap();
        assert_eq!(link0["type"], json!("SIGNER"));
        assert_eq!(link0["payload"], json!(signer.address()));
        let link1: Value = serde_json::from_str(&headers[3].1).unwrap();
        assert_eq!(link1["type"], json!("ECDSA_SIGNED_ENTITY"));
        let payload = link1["payload"].as_str().unwrap();
        assert_eq!(
            payload,
            format!("put:/world/test.dcl.eth/settings:{}:{{}}", ts.1)
        );
        assert_eq!(payload, payload.to_lowercase());
        assert!(link1["signature"].as_str().unwrap().starts_with("0x"));
    }

    #[test]
    fn permission_and_address_validation() {
        assert!(check_permission_name("deployment").is_ok());
        assert!(check_permission_name("streaming").is_ok());
        assert!(check_permission_name("access").is_ok());
        assert!(check_permission_name("root").is_err());
        assert!(check_address("0x85199e57d98bdc780c729f96f26dc9343e4a9b14").is_ok());
        assert!(check_address("85199e57d98bdc780c729f96f26dc9343e4a9b14").is_err());
        assert!(check_address("0x123").is_err());
    }

    #[test]
    fn permissions_render_is_stable() {
        let v = json!({
            "owner": "0xabc",
            "permissions": {
                "deployment": { "type": "allow-list", "wallets": ["0x1", "0x2"] },
                "streaming": { "type": "allow-list", "wallets": [] },
                "access": { "type": "unrestricted" }
            }
        });
        let out = render_permissions("w.dcl.eth", &v);
        assert_eq!(
            out,
            "world: w.dcl.eth\nowner: 0xabc\ndeployment: allow-list\n  - 0x1\n  - 0x2\nstreaming: allow-list (no extra wallets)\naccess: unrestricted"
        );
    }

    #[test]
    fn empty_update_is_rejected_and_target_required() {
        let update = SettingsUpdate {
            title: None,
            description: None,
            content_rating: None,
            spawn_coordinates: None,
            skybox_time: None,
            single_player: None,
            show_in_places: None,
            categories: Vec::new(),
            thumbnail: None,
        };
        assert!(update.is_empty());
        assert_eq!(
            resolve_target(Some("http://127.0.0.1:5142/")).unwrap(),
            "http://127.0.0.1:5142"
        );
    }
}
