//! The preview as a signing proxy: when storage points at a service instead
//! of the local database, every `/values`, `/players` and `/env` request the
//! host, the page or the CLI makes is re-issued to that service with the
//! ADR-44 signed-fetch headers the service authorizes on. What signs is the
//! same identity a publish would use: `DCL_PRIVATE_KEY`, or the session key
//! the header's Connect-with-DCL flow delegated.

use crate::deploy::DeployIdentity;
use anyhow::{Context, Result};
use catalyrst_crypto::Wallet;
use serde_json::Value;
use std::time::Duration;

/// Names the writer on every forwarded request, as the host does locally.
pub const SOURCE_HEADER: &str = "x-dcl-one-storage-source";
pub const CONFIRM_HEADER: &str = "x-confirm-delete-all";

/// The scene the service scopes the values to: upstream's
/// `buildStorageMetadata`, from scene.json's `worldConfiguration.name` and
/// `scene.base`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneMetadata {
    pub world: Option<String>,
    pub parcel: String,
}

impl SceneMetadata {
    pub fn from_scene_json(scene_json: &Value) -> Self {
        let world = scene_json
            .pointer("/worldConfiguration/name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let parcel = scene_json
            .pointer("/scene/base")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("0,0")
            .to_string();
        SceneMetadata { world, parcel }
    }

    /// The `x-identity-metadata` JSON, keys in upstream's order.
    pub fn to_json(&self) -> String {
        let mut meta = serde_json::Map::new();
        if let Some(world) = &self.world {
            meta.insert("realm".into(), serde_json::json!({ "serverName": world }));
            meta.insert("realmName".into(), Value::String(world.clone()));
        }
        meta.insert("parcel".into(), Value::String(self.parcel.clone()));
        Value::Object(meta).to_string()
    }

    pub fn describe(&self) -> String {
        match &self.world {
            Some(world) => format!("world {world}"),
            None => format!("parcel {}", self.parcel),
        }
    }
}

/// Who signs the forwarded requests.
#[derive(Clone)]
pub enum Signer {
    /// A private key, hex: `DCL_PRIVATE_KEY` or the CLI's `--sign-key`.
    Key(String),
    /// A wallet-delegated session key (the header's Connect with DCL).
    Delegated(DeployIdentity),
}

/// Names the signer without ever printing a key.
impl std::fmt::Debug for Signer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Signer({})", self.describe())
    }
}

impl Signer {
    /// `DCL_PRIVATE_KEY`, when set and a valid key. Quiet: this is read per
    /// request, not once per command.
    pub fn from_env() -> Option<Signer> {
        let key = std::env::var("DCL_PRIVATE_KEY").ok()?;
        Wallet::from_hex(&key).ok()?;
        Some(Signer::Key(key))
    }

    pub fn address(&self) -> String {
        match self {
            Signer::Key(hex) => Wallet::from_hex(hex)
                .map(|w| w.address())
                .unwrap_or_default(),
            Signer::Delegated(id) => id.signer.clone(),
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Signer::Key(_) => format!("{} (DCL_PRIVATE_KEY)", self.address()),
            Signer::Delegated(id) => {
                let left = (id.expiration_ms - crate::deploy::now_ms()).max(0) / 60_000;
                format!("{} (Connect with DCL session, {left} min left)", id.signer)
            }
        }
    }

    /// The `x-identity-*` headers for one request: the ADR-44 payload
    /// `method:pathname:timestamp:metadata`, lowercased, signed by the key
    /// (a simple chain) or by the session key under its delegation. The
    /// metadata header keeps its original case; the verifier lowercases.
    pub fn headers(
        &self,
        method: &str,
        pathname: &str,
        metadata: &str,
    ) -> Result<Vec<(String, String)>> {
        let timestamp = crate::deploy::now_ms();
        let payload = format!("{method}:{pathname}:{timestamp}:{metadata}").to_lowercase();
        let chain = match self {
            Signer::Key(hex) => {
                let wallet = Wallet::from_hex(hex).context("the signing key is not valid")?;
                catalyrst_crypto::create_simple_auth_chain(&wallet, &payload)
                    .context("signing the storage request")?
            }
            Signer::Delegated(id) => {
                let session =
                    Wallet::from_hex(&id.ephemeral_key).context("the session key is not valid")?;
                let signature = session
                    .sign_message(payload.as_bytes())
                    .context("signing the storage request with the session key")?;
                crate::deploy::ephemeral_auth_chain(
                    &id.signer,
                    &id.delegation_payload,
                    &id.delegation_signature,
                    &payload,
                    &signature,
                )
            }
        };
        let mut headers = vec![
            ("x-identity-timestamp".to_string(), timestamp.to_string()),
            ("x-identity-metadata".to_string(), metadata.to_string()),
        ];
        for (i, link) in chain.as_array().into_iter().flatten().enumerate() {
            headers.push((format!("x-identity-auth-chain-{i}"), link.to_string()));
        }
        Ok(headers)
    }
}

/// What the service answered, relayed as-is.
#[derive(Debug, Clone)]
pub struct Reply {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn json(&self) -> Option<Value> {
        serde_json::from_slice(&self.body).ok()
    }

    /// The service's `message`, or its status line.
    pub fn message(&self) -> String {
        self.json()
            .and_then(|v| v.get("message").and_then(Value::as_str).map(str::to_string))
            .unwrap_or_else(|| format!("HTTP {}", self.status))
    }
}

/// One request to forward: the route path (`/values/high-score`), its query,
/// and the raw JSON body a PUT carries.
pub struct Outgoing<'a> {
    pub method: &'a str,
    pub path: &'a str,
    pub query: Option<&'a str>,
    pub body: Option<Vec<u8>>,
    pub confirm_all: bool,
    pub source: &'a str,
}

pub fn client() -> Result<reqwest::Client> {
    crate::deploy::client(Duration::from_secs(10), Duration::from_secs(30))
}

/// Re-issues the request against `base` (a service URL, its path prefix
/// kept), signed when a signer is at hand. An unsigned request still goes
/// out: the service's refusal is the clearest account of what is missing.
pub async fn forward(
    client: &reqwest::Client,
    base: &str,
    signer: Option<&Signer>,
    metadata: &SceneMetadata,
    out: Outgoing<'_>,
) -> Result<Reply> {
    let base = base.trim_end_matches('/');
    let url = match out.query {
        Some(q) if !q.is_empty() => format!("{base}{}?{q}", out.path),
        _ => format!("{base}{}", out.path),
    };
    let pathname = url::Url::parse(&url)
        .map(|u| u.path().to_string())
        .with_context(|| format!("{url} is not a URL"))?;
    let method = reqwest::Method::from_bytes(out.method.as_bytes())
        .with_context(|| format!("{} is not an HTTP method", out.method))?;
    let mut req = client
        .request(method, &url)
        .header(SOURCE_HEADER, out.source);
    if let Some(signer) = signer {
        for (name, value) in signer.headers(out.method, &pathname, &metadata.to_json())? {
            req = req.header(name, value);
        }
    }
    if out.confirm_all {
        req = req.header(CONFIRM_HEADER, "true");
    }
    if let Some(body) = out.body {
        req = req.header(reqwest::header::CONTENT_TYPE, "application/json");
        req = req.header(reqwest::header::CONTENT_LENGTH, body.len());
        req = req.body(body);
    }
    let resp = req
        .send()
        .await
        .with_context(|| format!("could not reach the storage service at {base}"))?;
    let status = resp.status().as_u16();
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/json")
        .to_string();
    let body = resp
        .bytes()
        .await
        .context("reading the storage service's answer")?
        .to_vec();
    Ok(Reply {
        status,
        content_type,
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, HeaderName, HeaderValue};
    use catalyrst_crypto::signed_fetch::{verify_signed_fetch, verify_signed_fetch_meta};
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

    const KEY: &str = "0x0123456789012345678901234567890123456789012345678901234567890123";

    #[test]
    fn metadata_follows_upstreams_builder() {
        let world = SceneMetadata::from_scene_json(&json!({
            "worldConfiguration": { "name": "My.DCL.eth" },
            "scene": { "base": "1,2" }
        }));
        assert_eq!(
            world.to_json(),
            r#"{"realm":{"serverName":"My.DCL.eth"},"realmName":"My.DCL.eth","parcel":"1,2"}"#
        );
        assert_eq!(world.describe(), "world My.DCL.eth");
        let land = SceneMetadata::from_scene_json(&json!({ "scene": { "base": "-3,4" } }));
        assert_eq!(land.to_json(), r#"{"parcel":"-3,4"}"#);
        assert_eq!(land.describe(), "parcel -3,4");
        assert_eq!(SceneMetadata::from_scene_json(&json!({})).parcel, "0,0");
    }

    #[tokio::test]
    async fn a_key_signer_emits_a_verifiable_simple_chain_with_original_case_metadata() {
        let signer = Signer::Key(KEY.to_string());
        let meta = r#"{"realmName":"Foo.dcl.eth","parcel":"0,0"}"#;
        let headers = signer.headers("PUT", "/values/High-Score", meta).unwrap();
        let get = |name: &str| {
            headers
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.clone())
        };
        assert_eq!(get("x-identity-metadata").as_deref(), Some(meta));
        let ts = get("x-identity-timestamp").unwrap();
        let link0: Value = serde_json::from_str(&get("x-identity-auth-chain-0").unwrap()).unwrap();
        let link1: Value = serde_json::from_str(&get("x-identity-auth-chain-1").unwrap()).unwrap();
        assert_eq!(link0["type"], "SIGNER");
        assert_eq!(link0["payload"], signer.address());
        assert_eq!(link1["type"], "ECDSA_SIGNED_ENTITY");
        assert_eq!(
            link1["payload"],
            format!("put:/values/high-score:{ts}:{}", meta.to_lowercase())
        );
        assert!(headers.iter().all(|(n, _)| n != "x-identity-auth-chain-2"));
        let (recovered, metadata) = verify_signed_fetch_meta(
            &header_map(headers),
            "put",
            "/values/High-Score",
            FIVE_MINUTES,
        )
        .await
        .expect("the shared validator accepts the chain");
        assert_eq!(recovered, signer.address().to_lowercase());
        assert_eq!(
            metadata,
            json!({ "realmName": "Foo.dcl.eth", "parcel": "0,0" })
        );
    }

    #[tokio::test]
    async fn a_delegated_signer_chains_the_session_key_under_the_wallets_delegation() {
        let wallet = Wallet::from_hex(KEY).unwrap();
        let session_key = "0x1111111111111111111111111111111111111111111111111111111111111111";
        let session = Wallet::from_hex(session_key).unwrap();
        let expiration = "2999-01-01T00:00:00.000Z";
        let delegation = format!(
            "Decentraland Login\nEphemeral address: {}\nExpiration: {expiration}",
            session.address()
        );
        let identity = DeployIdentity {
            signer: wallet.address(),
            ephemeral_key: session_key.to_string(),
            delegation_payload: delegation.clone(),
            delegation_signature: wallet.sign_message(delegation.as_bytes()).unwrap(),
            expiration_ms: i64::MAX / 2,
        };
        let signer = Signer::Delegated(identity);
        let headers = signer.headers("GET", "/env/API_KEY", "{}").unwrap();
        let links: Vec<Value> = (0..3)
            .map(|i| {
                serde_json::from_str(
                    &headers
                        .iter()
                        .find(|(n, _)| *n == format!("x-identity-auth-chain-{i}"))
                        .unwrap()
                        .1,
                )
                .unwrap()
            })
            .collect();
        assert_eq!(links[0]["type"], "SIGNER");
        assert_eq!(links[1]["type"], "ECDSA_EPHEMERAL");
        assert_eq!(links[1]["payload"], delegation);
        assert_eq!(links[2]["type"], "ECDSA_SIGNED_ENTITY");
        let payload = links[2]["payload"].as_str().unwrap().to_string();
        assert!(payload.starts_with("get:/env/api_key:"));
        let recovered =
            verify_signed_fetch(&header_map(headers), "get", "/env/API_KEY", FIVE_MINUTES)
                .await
                .expect("the shared validator walks the delegation to the wallet");
        assert_eq!(recovered, wallet.address().to_lowercase());
        assert!(signer.describe().contains("Connect with DCL session"));
    }

    #[tokio::test]
    async fn env_signer_needs_a_valid_key() {
        let _guard = crate::deploy::ENV_LOCK.lock().await;
        std::env::set_var("DCL_PRIVATE_KEY", "nope");
        assert!(Signer::from_env().is_none());
        std::env::set_var("DCL_PRIVATE_KEY", KEY);
        let signer = Signer::from_env().unwrap();
        assert_eq!(signer.address(), Wallet::from_hex(KEY).unwrap().address());
        assert!(signer.describe().ends_with("(DCL_PRIVATE_KEY)"));
        std::env::remove_var("DCL_PRIVATE_KEY");
        assert!(Signer::from_env().is_none());
    }

    #[test]
    fn a_reply_reads_the_services_message_or_falls_back_to_the_status() {
        let reply = Reply {
            status: 403,
            content_type: "application/json".into(),
            body: br#"{"message":"not the owner"}"#.to_vec(),
        };
        assert_eq!(reply.message(), "not the owner");
        let plain = Reply {
            status: 502,
            content_type: "text/plain".into(),
            body: b"bad gateway".to_vec(),
        };
        assert_eq!(plain.message(), "HTTP 502");
    }
}
