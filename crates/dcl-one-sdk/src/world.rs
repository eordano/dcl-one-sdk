use crate::deploy::{
    caused, encode_segment, load_signer, now_ms, read_server_message, refusal, send_text,
    with_headers,
};
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
    /// Every text field as `(name, value)`; the thumbnail stays a file
    /// upload, not a text pair.
    fn pairs(&self) -> Vec<(&'static str, String)> {
        let text = [
            ("title", &self.title),
            ("description", &self.description),
            ("content_rating", &self.content_rating),
            ("spawn_coordinates", &self.spawn_coordinates),
            ("skybox_time", &self.skybox_time),
        ];
        let flags = [
            ("single_player", self.single_player),
            ("show_in_places", self.show_in_places),
        ];
        text.iter()
            .filter_map(|(k, v)| v.as_ref().map(|v| (*k, v.clone())))
            .chain(
                flags
                    .iter()
                    .filter_map(|(k, v)| v.map(|v| (*k, v.to_string()))),
            )
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.pairs().is_empty() && self.categories.is_empty() && self.thumbnail.is_none()
    }

    /// `field=value` for the fields this update touches, shown on the
    /// signing page.
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

    /// Rebuilt per attempt: a browser signer may retry with another wallet,
    /// and `reqwest::multipart::Form` is single-use.
    fn to_form(&self) -> Result<reqwest::multipart::Form> {
        let mut form = reqwest::multipart::Form::new();
        for (k, v) in self.pairs() {
            form = form.text(k, v);
        }
        for c in &self.categories {
            form = form.text("categories", c.clone());
        }
        if let Some(thumb) = &self.thumbnail {
            let bytes = std::fs::read(thumb).map_err(caused(
                format!("could not read the thumbnail {}", thumb.display()),
                TrySteps::one("check the --thumbnail path"),
            ))?;
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

/// A signed world-management request: the action owns its HTTP method, path
/// and body, so a local key and a browser wallet differ only in who produced
/// the `x-identity-*` headers.
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
            WorldAction::Permission { revoke: true, .. } => "delete",
            _ => "put",
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
                revoke: true,
            } => format!("revoke {permission} from {address}"),
            WorldAction::Permission {
                permission,
                address,
                revoke: false,
            } => format!("grant {permission} to {address}"),
        }
    }

    pub fn success(&self, name: &str) -> String {
        match self {
            WorldAction::SettingsSet(_) => format!("Settings updated for {name}"),
            WorldAction::Permission {
                permission,
                address,
                revoke: true,
            } => format!("Revoked {permission} from {address} on {name}"),
            WorldAction::Permission {
                permission,
                address,
                revoke: false,
            } => format!("Granted {permission} to {address} on {name}"),
        }
    }

    pub async fn send(
        &self,
        base: &str,
        name: &str,
        headers: Vec<(String, String)>,
    ) -> Result<(u16, String)> {
        let url = format!("{base}{}", self.path(name));
        let method = match self.method() {
            "delete" => reqwest::Method::DELETE,
            _ => reqwest::Method::PUT,
        };
        let mut req = client()?.request(method, &url);
        if let WorldAction::SettingsSet(update) = self {
            req = req.multipart(update.to_form()?);
        }
        send_text(with_headers(req, headers))
            .await
            .map_err(|e| unreachable(&url, e))
    }

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

pub struct BrowserOptions {
    pub port: Option<u16>,
    pub no_browser: bool,
    pub ci: bool,
}

/// Sign headlessly when a key is available, else with a browser wallet on a
/// printed URL.
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
        ux::Steps::new(1).done(message);
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
    if let Some(t) = crate::deploy::configured_target_server() {
        let base = crate::deploy::sanitize_catalyst_url(&t);
        ux::note(format!(
            "using DCL_ONE_SDK_TARGET_SERVER as the worlds server: {base}"
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

/// The ADR signed-fetch payload `method:path:timestamp:metadata`, lowercased:
/// the exact string a wallet signs, local key or browser extension alike.
pub fn signed_fetch_payload(method: &str, path: &str, timestamp: i64) -> String {
    format!("{method}:{path}:{timestamp}:{{}}").to_lowercase()
}

/// The `x-identity-*` headers for an already-signed payload. Timestamp and
/// metadata are read back out of the payload, never regenerated, so the
/// headers describe exactly the bytes that were signed.
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

/// Same headers, from a browser wallet's `personal_sign` over `payload`.
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
        read_server_message()
    };
    refusal(
        UserError::new(
            format!("the worlds server refused to {action} (HTTP {status})"),
            steps,
        ),
        body,
    )
}

fn unreachable(url: &str, e: reqwest::Error) -> anyhow::Error {
    UserError::new(
        "could not reach the worlds server",
        TrySteps::one("check the server is running and the URL is right")
            .and("pass --target-server <worlds-content-server-url>"),
    )
    .why(format!("request failed: {url}"))
    .caused_by(e)
    .into()
}

/// GET `/world/<name>/<suffix>` and return the body, or the refusal.
async fn get_world(
    name: &str,
    target_content: Option<&str>,
    suffix: &str,
    action: &str,
) -> Result<String> {
    let base = resolve_target(target_content)?;
    let url = format!("{base}/world/{}/{suffix}", encode_segment(name));
    let (status, body) = send_text(client()?.get(&url))
        .await
        .map_err(|e| unreachable(&url, e))?;
    if !(200..300).contains(&status) {
        return Err(refused(action, name, status, &body));
    }
    Ok(body)
}

pub async fn settings_get(name: &str, target_content: Option<&str>) -> Result<()> {
    let body = get_world(name, target_content, "settings", "read the settings").await?;
    let mut steps = ux::Steps::new(1);
    match serde_json::from_str::<Value>(&body) {
        Ok(v) => println!("{}", serde_json::to_string_pretty(&v)?),
        Err(_) => println!("{body}"),
    }
    steps.done(format!("Settings fetched for {name}"));
    Ok(())
}

pub async fn permissions_list(name: &str, target_content: Option<&str>) -> Result<()> {
    let body = get_world(name, target_content, "permissions", "list the permissions").await?;
    let v: Value = serde_json::from_str(&body).context("parsing the permissions response")?;
    let mut steps = ux::Steps::new(1);
    println!("{}", render_permissions(name, &v));
    steps.done(format!("Permissions fetched for {name}"));
    Ok(())
}

pub fn render_permissions(name: &str, v: &Value) -> String {
    let owner = v
        .get("owner")
        .and_then(Value::as_str)
        .unwrap_or("(unknown)");
    let mut lines = vec![format!("world: {name}"), format!("owner: {owner}")];
    let perms = v.get("permissions").cloned().unwrap_or_default();
    for kind in ["deployment", "streaming"] {
        let p = perms.get(kind);
        let wallets: Vec<&str> = p
            .and_then(|p| p.get("wallets"))
            .and_then(Value::as_array)
            .map(|arr| arr.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let ty = p
            .and_then(|p| p.get("type"))
            .and_then(Value::as_str)
            .unwrap_or("allow-list");
        if wallets.is_empty() {
            lines.push(format!("{kind}: {ty} (no extra wallets)"));
        } else {
            lines.push(format!("{kind}: {ty}"));
            lines.extend(wallets.iter().map(|w| format!("  - {w}")));
        }
    }
    let access = perms
        .get("access")
        .and_then(|a| a.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("unrestricted");
    lines.push(format!("access: {access}"));
    lines.join("\n")
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
    if catalyrst_auth_chain::is_eth_address(address) {
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
    use axum::http::{HeaderMap, HeaderName, HeaderValue};
    use serde_json::json;

    const FIVE_MINUTES: i64 = 5 * 60;

    fn header_map(headers: Vec<(String, String)>) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (k, v) in headers {
            map.insert(
                HeaderName::from_bytes(k.as_bytes()).unwrap(),
                HeaderValue::from_str(&v).unwrap(),
            );
        }
        map
    }

    fn permission(permission: &str, revoke: bool) -> WorldAction {
        WorldAction::Permission {
            permission: permission.to_string(),
            address: "0xAAAA111111111111111111111111111111111111".to_string(),
            revoke,
        }
    }

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
        use catalyrst_crypto::signed_fetch::verify_signed_fetch;

        let signer = crate::random_test_wallet();
        let method = "put";
        let path =
            "/world/Test.dcl.eth/permissions/deployment/0x1111111111111111111111111111111111111111";

        let payload = signed_fetch_payload(method, path, now_ms());
        let signature = signer.sign_message(payload.as_bytes()).unwrap();
        let map = header_map(browser_headers(&signer.address(), &payload, &signature));
        let recovered = verify_signed_fetch(&map, method, path, FIVE_MINUTES)
            .await
            .expect("browser-signed headers must pass the shared validator");
        assert_eq!(recovered, signer.address().to_lowercase());
    }

    #[test]
    fn actions_describe_their_own_http_shape() {
        let grant = permission("deployment", false);
        assert_eq!(grant.method(), "put");
        assert_eq!(
            grant.path("My-World.dcl.eth"),
            "/world/My-World.dcl.eth/permissions/deployment/0xaaaa111111111111111111111111111111111111"
        );
        assert!(grant.validate().is_ok());

        let revoke = permission("deployment", true);
        assert_eq!(revoke.method(), "delete");
        assert!(revoke.summary().starts_with("revoke deployment from"));

        assert!(permission("root", false).validate().is_err());

        let empty = WorldAction::SettingsSet(SettingsUpdate::default());
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
        assert!(SettingsUpdate::default().is_empty());
        assert_eq!(
            resolve_target(Some("http://127.0.0.1:5142/")).unwrap(),
            "http://127.0.0.1:5142"
        );
    }

    #[tokio::test]
    async fn signed_headers_are_accepted_by_the_shared_validator() {
        use catalyrst_crypto::signed_fetch::{verify_signed_fetch, verify_signed_fetch_meta};

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
            let headers = header_map(signed_headers(&signer, method, path).unwrap());
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
