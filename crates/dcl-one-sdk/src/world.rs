use crate::deploy::{encode_segment, load_signer, now_ms};
use crate::ux::{self, TrySteps, UserError};
use anyhow::{Context, Result};
use catalyrst_crypto::Wallet;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Default)]
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
    /// Every text field as `(name, value)` — the one field walk `is_empty`,
    /// `changed_fields` and `to_form` all derive from (the thumbnail stays
    /// special: a file upload, not a text pair).
    fn pairs(&self) -> Vec<(&'static str, String)> {
        let mut out = Vec::new();
        let mut push = |k: &'static str, v: String| out.push((k, v));
        if let Some(v) = &self.title {
            push("title", v.clone());
        }
        if let Some(v) = &self.description {
            push("description", v.clone());
        }
        if let Some(v) = &self.content_rating {
            push("content_rating", v.clone());
        }
        if let Some(v) = &self.spawn_coordinates {
            push("spawn_coordinates", v.clone());
        }
        if let Some(v) = &self.skybox_time {
            push("skybox_time", v.clone());
        }
        if let Some(v) = self.single_player {
            push("single_player", v.to_string());
        }
        if let Some(v) = self.show_in_places {
            push("show_in_places", v.to_string());
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.pairs().is_empty() && self.categories.is_empty() && self.thumbnail.is_none()
    }

    /// `field=value` pairs for the fields this update actually touches — shown
    /// on the signing page so the wallet holder sees what they are approving.
    pub fn changed_fields(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .pairs()
            .into_iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        if !self.categories.is_empty() {
            out.push(format!("categories={}", self.categories.join(",")));
        }
        if let Some(v) = &self.thumbnail {
            out.push(format!("thumbnail={}", v.display()));
        }
        out
    }

    /// The multipart body. Rebuilt per attempt — a browser signer may retry
    /// with a different wallet, and `reqwest::multipart::Form` is single-use.
    fn to_form(&self) -> Result<reqwest::multipart::Form> {
        let mut form = reqwest::multipart::Form::new();
        for (k, v) in self.pairs() {
            form = form.text(k, v);
        }
        for c in &self.categories {
            form = form.text("categories", c.clone());
        }
        if let Some(thumb) = &self.thumbnail {
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
        Ok(form)
    }
}

/// A signed world-management request.
///
/// Both signing paths go through this: the action owns its HTTP method, path
/// and body, so the only difference between a local key and a browser wallet
/// is who produced the `x-identity-*` headers.
pub enum WorldAction {
    SettingsSet(SettingsUpdate),
    Permission {
        permission: String,
        address: String,
        revoke: bool,
    },
}

impl WorldAction {
    pub fn validate(&self) -> Result<()> {
        match self {
            WorldAction::SettingsSet(update) => {
                if update.is_empty() {
                    return Err(UserError::new(
                        "nothing to update \u{2014} no settings flags given",
                        TrySteps::one(
                            "pass at least one of --title --description --content-rating --spawn-coordinates --skybox-time --single-player --show-in-places --category --thumbnail",
                        ),
                    )
                    .into());
                }
                Ok(())
            }
            WorldAction::Permission {
                permission,
                address,
                ..
            } => {
                check_permission_name(permission)?;
                check_address(address)
            }
        }
    }

    pub fn method(&self) -> &'static str {
        match self {
            WorldAction::SettingsSet(_) => "put",
            WorldAction::Permission { revoke, .. } => {
                if *revoke {
                    "delete"
                } else {
                    "put"
                }
            }
        }
    }

    pub fn path(&self, name: &str) -> String {
        match self {
            WorldAction::SettingsSet(_) => format!("/world/{}/settings", encode_segment(name)),
            WorldAction::Permission {
                permission,
                address,
                ..
            } => format!(
                "/world/{}/permissions/{}/{}",
                encode_segment(name),
                encode_segment(permission),
                encode_segment(&address.to_lowercase())
            ),
        }
    }

    /// What signing this authorizes, in one line.
    pub fn summary(&self) -> String {
        match self {
            WorldAction::SettingsSet(update) => {
                format!(
                    "update the settings ({})",
                    update.changed_fields().join(", ")
                )
            }
            WorldAction::Permission {
                permission,
                address,
                revoke,
            } => {
                if *revoke {
                    format!("revoke {permission} from {address}")
                } else {
                    format!("grant {permission} to {address}")
                }
            }
        }
    }

    pub fn success(&self, name: &str) -> String {
        match self {
            WorldAction::SettingsSet(_) => format!("Settings updated for {name}"),
            WorldAction::Permission {
                permission,
                address,
                revoke,
            } => {
                if *revoke {
                    format!("Revoked {permission} from {address} on {name}")
                } else {
                    format!("Granted {permission} to {address} on {name}")
                }
            }
        }
    }

    /// Send the request with headers someone else has already signed.
    pub async fn send(
        &self,
        base: &str,
        name: &str,
        headers: Vec<(String, String)>,
    ) -> Result<(u16, String)> {
        let url = format!("{base}{}", self.path(name));
        let mut req = match self {
            WorldAction::SettingsSet(update) => client()?.put(&url).multipart(update.to_form()?),
            WorldAction::Permission { revoke, .. } => {
                let method = if *revoke {
                    reqwest::Method::DELETE
                } else {
                    reqwest::Method::PUT
                };
                client()?.request(method, &url)
            }
        };
        for (k, v) in headers {
            req = req.header(k, v);
        }
        let resp = match req.send().await {
            Ok(resp) => resp,
            Err(e) => return Err(unreachable(&url, e)),
        };
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        Ok((status, body))
    }

    /// Echo whatever the server returned that is worth seeing.
    pub fn print_body(&self, body: &str) {
        if let WorldAction::SettingsSet(_) = self {
            if let Ok(v) = serde_json::from_str::<Value>(body) {
                if let Some(settings) = v.get("settings") {
                    if let Ok(pretty) = serde_json::to_string_pretty(settings) {
                        println!("{pretty}");
                    }
                }
            }
        }
    }
}

/// How the browser signing page should be presented when no local key exists.
pub struct BrowserOptions {
    pub port: Option<u16>,
    pub no_browser: bool,
    pub ci: bool,
}

/// Run a signed world request, signing headlessly when a key is available and
/// falling back to a browser wallet on a printed URL otherwise.
pub async fn run_action(
    name: &str,
    action: WorldAction,
    target_content: Option<&str>,
    sign_key: Option<&Path>,
    browser: BrowserOptions,
) -> Result<()> {
    action.validate()?;
    let base = resolve_target(target_content)?;
    let Some(signer) = load_signer(sign_key)? else {
        let message = crate::world_linker::run(
            crate::world_linker::WorldSignRequest {
                base,
                name: name.to_string(),
                action,
            },
            crate::linker::LinkerOptions {
                port: browser.port,
                open_browser: !browser.no_browser && !browser.ci,
                timeout: crate::linker::linker_timeout(),
                host: None,
            },
        )
        .await?;
        let mut steps = ux::Steps::new(1);
        steps.done(message);
        return Ok(());
    };
    let path = action.path(name);
    let headers = signed_headers(&signer, action.method(), &path)?;
    let (status, body) = action.send(&base, name, headers).await?;
    if !(200..300).contains(&status) {
        return Err(refused(&action.summary(), name, status, &body));
    }
    let mut steps = ux::Steps::new(1);
    action.print_body(&body);
    steps.done(action.success(name));
    Ok(())
}

pub fn resolve_target(target_content: Option<&str>) -> Result<String> {
    if let Some(t) = target_content {
        return Ok(t.trim().trim_end_matches('/').to_string());
    }
    if let Some(t) = crate::deploy::env_default_target() {
        let base = crate::deploy::sanitize_catalyst_url(&t);
        ux::note(format!(
            "using DCL_ONE_SDK_DEFAULT_TARGET as the worlds server: {base}"
        ));
        return Ok(base);
    }
    ux::note(format!(
        "using the public worlds server {}",
        crate::deploy::WORLDS_CONTENT_SERVER
    ));
    Ok(crate::deploy::WORLDS_CONTENT_SERVER.to_string())
}

fn client() -> Result<reqwest::Client> {
    crate::deploy::client(Duration::from_secs(30), Duration::from_secs(30))
}

/// The ADR signed-fetch payload: `method:path:timestamp:metadata`, lowercased.
/// This is the exact string a wallet signs, whether that wallet is a local key
/// or a browser extension.
pub fn signed_fetch_payload(method: &str, path: &str, timestamp: i64) -> String {
    format!("{method}:{path}:{timestamp}:{{}}").to_lowercase()
}

/// Build the `x-identity-*` headers for an already-signed payload. The
/// timestamp and metadata are read back out of the payload rather than
/// regenerated, so the headers always describe exactly the bytes that were
/// signed — a browser signature can arrive seconds after it was minted.
pub(crate) fn headers_from_chain(payload: &str, chain: &Value) -> Vec<(String, String)> {
    let parts: Vec<&str> = payload.split(':').collect();
    let timestamp = parts.get(2).copied().unwrap_or_default().to_string();
    let metadata = parts.get(3).copied().unwrap_or("{}").to_string();
    let mut headers = vec![
        ("x-identity-timestamp".to_string(), timestamp),
        ("x-identity-metadata".to_string(), metadata),
    ];
    for (i, link) in chain.as_array().into_iter().flatten().enumerate() {
        headers.push((format!("x-identity-auth-chain-{i}"), link.to_string()));
    }
    headers
}

pub fn signed_headers(signer: &Wallet, method: &str, path: &str) -> Result<Vec<(String, String)>> {
    let payload = signed_fetch_payload(method, path, now_ms());
    let chain = catalyrst_crypto::create_simple_auth_chain(signer, &payload)
        .context("EIP-191 sign of the signed-fetch payload")?;
    Ok(headers_from_chain(&payload, &chain))
}

/// Same headers, but from a signature produced by a browser wallet's
/// `personal_sign` over `payload`.
pub fn browser_headers(address: &str, payload: &str, signature: &str) -> Vec<(String, String)> {
    let chain = crate::deploy::simple_auth_chain(address, payload, signature);
    headers_from_chain(payload, &chain)
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
    if catalyrst_types::is_eth_address(address) {
        return Ok(());
    }
    Err(UserError::new(
        format!("\"{address}\" is not an ethereum address"),
        TrySteps::one("expect 0x + 40 hex chars"),
    )
    .into())
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

    #[tokio::test]
    async fn browser_headers_verify_exactly_like_key_signed_ones() {
        use axum::http::{HeaderMap, HeaderName, HeaderValue};
        use catalyrst_crypto::signed_fetch::verify_signed_fetch;

        const FIVE_MINUTES: i64 = 5 * 60;
        let signer = crate::random_test_wallet();
        let method = "put";
        let path =
            "/world/Test.dcl.eth/permissions/deployment/0x1111111111111111111111111111111111111111";

        let payload = signed_fetch_payload(method, path, now_ms());
        let signature = signer.sign_message(payload.as_bytes()).unwrap();
        let headers = browser_headers(&signer.address(), &payload, &signature);

        let mut map = HeaderMap::new();
        for (k, v) in headers {
            map.insert(
                HeaderName::from_bytes(k.as_bytes()).unwrap(),
                HeaderValue::from_str(&v).unwrap(),
            );
        }
        let recovered = verify_signed_fetch(&map, method, path, FIVE_MINUTES)
            .await
            .expect("browser-signed headers must pass the shared validator");
        assert_eq!(recovered, signer.address().to_lowercase());
    }

    #[test]
    fn actions_describe_their_own_http_shape() {
        let grant = WorldAction::Permission {
            permission: "deployment".to_string(),
            address: "0xAAAA111111111111111111111111111111111111".to_string(),
            revoke: false,
        };
        assert_eq!(grant.method(), "put");
        assert_eq!(
            grant.path("My-World.dcl.eth"),
            "/world/My-World.dcl.eth/permissions/deployment/0xaaaa111111111111111111111111111111111111"
        );
        assert!(grant.validate().is_ok());

        let revoke = WorldAction::Permission {
            permission: "deployment".to_string(),
            address: "0xAAAA111111111111111111111111111111111111".to_string(),
            revoke: true,
        };
        assert_eq!(revoke.method(), "delete");
        assert!(revoke.summary().starts_with("revoke deployment from"));

        let bad = WorldAction::Permission {
            permission: "root".to_string(),
            address: "0xAAAA111111111111111111111111111111111111".to_string(),
            revoke: false,
        };
        assert!(bad.validate().is_err());

        let empty = WorldAction::SettingsSet(SettingsUpdate {
            title: None,
            description: None,
            content_rating: None,
            spawn_coordinates: None,
            skybox_time: None,
            single_player: None,
            show_in_places: None,
            categories: Vec::new(),
            thumbnail: None,
        });
        assert!(empty.validate().is_err());
        assert_eq!(empty.method(), "put");
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

    #[tokio::test]
    async fn signed_headers_are_accepted_by_the_shared_validator() {
        use axum::http::{HeaderMap, HeaderName, HeaderValue};
        use catalyrst_crypto::signed_fetch::{verify_signed_fetch, verify_signed_fetch_meta};

        const FIVE_MINUTES: i64 = 5 * 60;
        let signer = crate::random_test_wallet();
        let expected = signer.address().to_lowercase();

        for (method, path) in [
            ("put", "/world/My-World.dcl.eth/settings"),
            (
                "put",
                "/world/My-World.dcl.eth/permissions/deployment/0x1111111111111111111111111111111111111111",
            ),
            ("delete", "/scenes/52,-52"),
        ] {
            let mut headers = HeaderMap::new();
            for (k, v) in signed_headers(&signer, method, path).unwrap() {
                headers.insert(
                    HeaderName::from_bytes(k.as_bytes()).unwrap(),
                    HeaderValue::from_str(&v).unwrap(),
                );
            }
            let recovered = verify_signed_fetch(&headers, method, path, FIVE_MINUTES)
                .await
                .unwrap_or_else(|e| panic!("{method} {path} rejected: {e}"));
            assert_eq!(recovered, expected);
            let (meta_signer, metadata) =
                verify_signed_fetch_meta(&headers, method, path, FIVE_MINUTES)
                    .await
                    .unwrap();
            assert_eq!(meta_signer, expected);
            assert_eq!(metadata, json!({}));
        }
    }
}
