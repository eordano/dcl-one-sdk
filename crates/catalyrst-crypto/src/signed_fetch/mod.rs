//! Shared signed-fetch validation; replay tolerance is always an explicit
//! call-site parameter. There is no nonce and no replay cache on this path: a
//! window is the *only* bound on how long a captured request stays usable, so
//! every row below is a replay-tolerance budget, not a clock-skew allowance.
//!
//! `validate_signature` (and `handshake::validate_signature`) compare with
//! `(now - signed_at).abs()`, so a window here is symmetric: +/- the stated
//! amount. Services that re-implement freshness locally are called out below
//! where they diverge from that.
//!
//! All entry points here are `async`: they route through
//! `verify::verify_auth_chain_async` with `default_eip1654_validator()`, so
//! smart-contract-wallet (EIP-1654) auth chains verify against the configured
//! `RPC_ENDPOINT_ETH` exactly like plain ECDSA chains. With no RPC configured
//! the validator is `None` and EIP-1654 links fail closed as
//! `EipNotImplemented`. Inject a validator explicitly via the
//! `validate_signature_with` twins.
//!
//! ## Services routed through this module
//!
//! | service | entry point | window |
//! |---|---|---|
//! | camera-reel, credits, events, market, notifications | require_signer / try_extract_signer | +/- 5 min |
//! | places | require_signer (no optional variant) | +/- 5 min |
//! | builder | require_signer | +/- 30 min |
//! | builder | try_extract_signer | +/- 5 min |
//! | comms | require_signer / try_extract_signer | +/- 5 min |
//! | comms | crate-local verify_signed_fetch (scene admin/bans/voice/adapter) | +/- 1 min |
//! | governance | verify_signed_fetch (SIGNED_FETCH_MAX_AGE_SECS) | +/- 5 min |
//! | social-service rest | require_signer / try_extract_signer | +/- 5 min |
//! | social-service rpc | handshake::require_signer / verify_handshake | +/- 5 min |
//! | quests | handshake::require_signer / optional_signer / verify_handshake | +/- 5 min (compile-time constant, not env-overridable) |
//! | server, worlds | require_verified (verify_signed_fetch_meta) | +/- 5 min |
//! | signatures (rentals) | verify_signed_fetch via `auth::require_signer` | AUTH_EXPIRATION_SECONDS, default +/- 5 min |
//!
//! The `handshake::*_v6` twins take the same windows and add the 6.x payload
//! plus the metadata gates; a WS/RPC path migrating off the legacy payload
//! moves to those, not to a new window.
//!
//! ## Services that recover an address WITHOUT this module
//!
//! These re-implement extraction and/or freshness. Listed here because the
//! window is the same kind of budget and diverging silently is the hazard.
//!
//! | service | entry point | window |
//! |---|---|---|
//! | scene-state | `auth::verify_auth_frame` (own extraction; no SIGNER-at-0 structural check) | +/- 5 min |
//! | world-storage | `auth_chain::verify_request` -> local `check_freshness` | +/- 1 min (local copy of the symmetric `.abs()` bound) |
//! | pulse | `handshake::verify_handshake_bytes` (own header-bag parse) | +/- 1 min (MAX_TIMESTAMP_SKEW_MS) |
//! | archipelago | `auth::ChallengeStore::redeem_and_verify` | server-issued single-use challenge; 2 min TTL, 5 min max age |
//! | explorer-api auth-api | `auth_api::validation::validate_auth_chain` | NONE - chain is verified against its own final authority, no timestamp is bound |
//!
//! Deployment entities (server `write_deployer`, worlds `handlers::deploy`)
//! bind the signature to the entity CID rather than to method+path+timestamp,
//! so they have no window at all; freshness there comes only from the
//! ECDSA_EPHEMERAL link's own expiration.

pub mod handshake;

use std::sync::{Arc, OnceLock};

use http::HeaderMap;
use thiserror::Error;

use crate::eip1654::Eip1654Validator;
use crate::metadata_gate::{
    assert_canonical_metadata_keys, assert_legacy_metadata_keys, truncate_detail, SignerGate,
};
use crate::signer::Signer;
use crate::verify::verify_auth_chain_async;
use crate::AuthError;
use crate::{RpcEip1654Validator, ValidationCache};
use catalyrst_types::{AuthLink as CryptoAuthLink, AuthLinkType, EthAddress};

pub use catalyrst_types::MAX_AUTH_CHAIN_LINKS;

pub const AUTH_CHAIN_HEADER_PREFIX: &str = "x-identity-auth-chain-";
pub const AUTH_TIMESTAMP_HEADER: &str = "x-identity-timestamp";
pub const AUTH_METADATA_HEADER: &str = "x-identity-metadata";

#[derive(Debug, Clone)]
pub struct AuthLink {
    pub kind: AuthLinkType,
    pub payload: String,
    pub signature: String,
}

#[derive(Debug, Clone)]
pub struct AuthChain {
    pub links: Vec<AuthLink>,
    pub signer: EthAddress,
}

#[derive(Debug, Error)]
pub enum AuthChainError {
    #[error("Invalid Auth Chain")]
    MalformedChain { detail: String },
    #[error("Invalid Auth Chain")]
    InsufficientLinks,
    #[error("Missing timestamp")]
    MissingTimestamp,
    #[error("Expired signature")]
    Expired {
        signed_at: i64,
        now: i64,
        window_secs: i64,
    },
    #[error("Invalid signature")]
    InvalidSignature(String),
    #[error("Access denied, invalid signer")]
    ForbiddenSigner,
    #[error("EIP-1654 not implemented")]
    EipNotImplemented,

    // Variants below are produced by service-side validators built on this
    // chain type (market's address check, world-storage's async EIP-1654
    // validator and scene-signer policy), not by the extraction/validation in
    // this module. They live here so services share one AuthChainError instead
    // of redefining the enum plus a From<> mapping each.
    #[error("Forbidden: address mismatch")]
    AddressMismatch { expected: String, recovered: String },
    #[error("Invalid timestamp")]
    InvalidTimestamp(String),
    #[error("Error connecting to catalyst")]
    CatalystUnavailable(String),
    #[error("Requests from scenes are not allowed")]
    SceneSignerRejected,
}

impl AuthChainError {
    pub fn is_bad_request(&self) -> bool {
        matches!(
            self,
            AuthChainError::MalformedChain { .. }
                | AuthChainError::InsufficientLinks
                | AuthChainError::MissingTimestamp
                | AuthChainError::InvalidTimestamp(_)
        )
    }

    /// The 400 body text, detail included.
    ///
    /// `Display` stays the upstream-facing constant that several services pin
    /// byte for byte, so the reason a request was refused - which canonical key
    /// was misconfigured, which spelling arrived, which signer the gate turned
    /// down - is only reachable through here. A rollout across the two payload
    /// shapes is undiagnosable without it: every one of those refusals renders
    /// as the same "Invalid Auth Chain".
    pub fn http_message(&self) -> String {
        match self {
            AuthChainError::MalformedChain { detail } if !detail.is_empty() => {
                format!("{self}: {detail}")
            }
            other => other.to_string(),
        }
    }
}

impl From<AuthChainError> for catalyrst_types::ApiError {
    fn from(e: AuthChainError) -> Self {
        if e.is_bad_request() {
            catalyrst_types::ApiError::http(400, e.http_message())
        } else {
            catalyrst_types::ApiError::http(401, format!("Unauthenticated: {e}"))
        }
    }
}

/// `None` when RPC_ENDPOINT_ETH is unset: EIP-1654 links then fail closed as
/// unverifiable rather than validated against a production RPC.
pub fn default_eip1654_validator() -> Option<&'static Arc<dyn Eip1654Validator>> {
    static V: OnceLock<Option<Arc<dyn Eip1654Validator>>> = OnceLock::new();
    V.get_or_init(|| {
        let rpc_url = std::env::var("RPC_ENDPOINT_ETH")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())?;
        let rpc = RpcEip1654Validator::new(rpc_url);
        Some(Arc::new(ValidationCache::new(Arc::new(rpc))) as Arc<dyn Eip1654Validator>)
    })
    .as_ref()
}

/// The pre-6.0.0 signed-fetch payload: the whole joined string folded, metadata
/// included. Every in-tree signer (bevy-explorer, ui3, dcl-one-sdk, the
/// scene-state js runtime, the deploy signer) still mints this shape, so it
/// stays what `build_payload` returns; deployed wasm clients cannot be updated
/// atomically with a server.
pub fn build_legacy_payload(method: &str, path: &str, timestamp: &str, metadata: &str) -> String {
    format!("{}:{}:{}:{}", method, path, timestamp, metadata).to_lowercase()
}

/// The crypto-middleware >= 6.0.0 payload: only method and path folded, so the
/// metadata bytes are bound to the signature instead of being re-spellable
/// after signing. Kept beside `build_legacy_payload` on purpose - the two must
/// stay in step, and the difference between them is the whole migration.
pub fn build_payload_v6(method: &str, path: &str, timestamp: &str, metadata: &str) -> String {
    format!(
        "{}:{}:{}:{}",
        method.to_lowercase(),
        path.to_lowercase(),
        timestamp,
        metadata
    )
}

pub fn build_payload(method: &str, path: &str, timestamp: &str, metadata: &str) -> String {
    build_legacy_payload(method, path, timestamp, metadata)
}

pub fn signed_fetch_path<'a>(headers: &HeaderMap, fallback: &'a str) -> std::borrow::Cow<'a, str> {
    match headers.get("x-original-path").and_then(|v| v.to_str().ok()) {
        Some(raw) => {
            let stripped = raw.split('?').next().unwrap_or(raw);
            // x-original-path is only trustworthy as the route path behind a
            // proxy prefix; a value that is not a suffix of the actual route is
            // a forged client header and must not rebind the signature.
            if stripped.ends_with(fallback) {
                std::borrow::Cow::Owned(stripped.to_string())
            } else {
                std::borrow::Cow::Borrowed(fallback)
            }
        }
        None => std::borrow::Cow::Borrowed(fallback),
    }
}

pub fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

pub fn extract_auth_chain(headers: &HeaderMap) -> Result<AuthChain, AuthChainError> {
    let mut links = Vec::new();

    for i in 0..MAX_AUTH_CHAIN_LINKS {
        let name = format!("{}{}", AUTH_CHAIN_HEADER_PREFIX, i);
        let Some(raw) = header_str(headers, &name) else {
            break;
        };

        let link: CryptoAuthLink = serde_json::from_str(raw).map_err(|e| {
            let mut detail = e.to_string();
            if detail.len() > 64 {
                detail.truncate(64);
            }
            AuthChainError::MalformedChain { detail }
        })?;

        match link.link_type {
            AuthLinkType::SIGNER => {
                if i != 0 {
                    return Err(AuthChainError::MalformedChain {
                        detail: format!("SIGNER link at non-zero index {}", i),
                    });
                }
            }
            _ => {
                if i == 0 {
                    return Err(AuthChainError::MalformedChain {
                        detail: "first link must be SIGNER".to_string(),
                    });
                }
                if link.signature.as_deref().unwrap_or("").is_empty() {
                    return Err(AuthChainError::MalformedChain {
                        detail: format!("missing signature on link {}", i),
                    });
                }
            }
        }

        links.push(AuthLink {
            kind: link.link_type,
            payload: link.payload,
            signature: link.signature.unwrap_or_default(),
        });
    }

    let overflow = format!("{}{}", AUTH_CHAIN_HEADER_PREFIX, MAX_AUTH_CHAIN_LINKS);
    if header_str(headers, &overflow).is_some() {
        return Err(AuthChainError::MalformedChain {
            detail: format!("exceeds max length of {}", MAX_AUTH_CHAIN_LINKS),
        });
    }
    if links.len() < 2 {
        return Err(AuthChainError::InsufficientLinks);
    }
    let signer = links[0].payload.to_lowercase();
    Ok(AuthChain { links, signer })
}

pub fn to_crypto_chain(chain: &AuthChain) -> Vec<CryptoAuthLink> {
    chain
        .links
        .iter()
        .map(|link| CryptoAuthLink {
            link_type: link.kind,
            payload: link.payload.clone(),
            signature: if link.signature.is_empty() {
                None
            } else {
                Some(link.signature.clone())
            },
        })
        .collect()
}

pub async fn validate_signature(
    chain: &AuthChain,
    payload: &str,
    timestamp: &str,
    expiration_secs: i64,
    now: i64,
) -> Result<Signer, AuthChainError> {
    validate_signature_with(
        chain,
        payload,
        timestamp,
        expiration_secs,
        now,
        default_eip1654_validator().map(|v| &**v),
    )
    .await
}

pub async fn validate_signature_with(
    chain: &AuthChain,
    payload: &str,
    timestamp: &str,
    expiration_secs: i64,
    now: i64,
    validator: Option<&dyn Eip1654Validator>,
) -> Result<Signer, AuthChainError> {
    assert_within_window(signed_at_secs(timestamp)?, now, expiration_secs)?;

    let crypto_chain = to_crypto_chain(chain);
    verify_auth_chain_async(&crypto_chain, payload, Some(now * 1000), validator)
        .await
        .map_err(map_auth_error)?;
    Ok(Signer::from_verified_chain(&chain.signer))
}

/// Fail closed: a non-numeric timestamp must be rejected, not silently skip the
/// replay/expiration window.
fn signed_at_secs(timestamp: &str) -> Result<i64, AuthChainError> {
    timestamp
        .parse::<i64>()
        .map(|ms| ms / 1000)
        .map_err(|_| AuthChainError::InvalidTimestamp(timestamp.to_string()))
}

fn assert_within_window(signed_at: i64, now: i64, window_secs: i64) -> Result<(), AuthChainError> {
    if (now - signed_at).abs() > window_secs {
        return Err(AuthChainError::Expired {
            signed_at,
            now,
            window_secs,
        });
    }
    Ok(())
}

fn map_auth_error(err: AuthError) -> AuthChainError {
    match err {
        AuthError::MalformedChain(d) => AuthChainError::MalformedChain { detail: d },
        AuthError::MissingSignature { .. } => AuthChainError::MalformedChain {
            detail: err.to_string(),
        },
        AuthError::RecoveryFailed(d) => AuthChainError::InvalidSignature(d),
        AuthError::SignerMismatch { .. } | AuthError::FinalAuthorityMismatch { .. } => {
            AuthChainError::InvalidSignature(err.to_string())
        }
        AuthError::EphemeralExpired {
            expiration_ms,
            now_ms,
        } => AuthChainError::Expired {
            signed_at: expiration_ms / 1000,
            now: now_ms / 1000,
            window_secs: 0,
        },
        AuthError::InvalidEphemeralPayload(d) => AuthChainError::MalformedChain { detail: d },
        AuthError::Eip1654NotImplemented => AuthChainError::EipNotImplemented,
        AuthError::Eip1654Rejected { .. } => AuthChainError::InvalidSignature(err.to_string()),
        AuthError::Eip1654ValidationFailed(d) => AuthChainError::CatalystUnavailable(d),
    }
}

pub fn try_extract(headers: &HeaderMap) -> Option<AuthChain> {
    extract_auth_chain(headers).ok()
}

pub async fn try_extract_signer(
    headers: &HeaderMap,
    method: &str,
    path: &str,
    tolerance_secs: i64,
) -> Option<Signer> {
    let path = signed_fetch_path(headers, path);
    let path = path.as_ref();
    let chain = try_extract(headers)?;
    let ts = header_str(headers, AUTH_TIMESTAMP_HEADER)?.to_string();
    let metadata = header_str(headers, AUTH_METADATA_HEADER)
        .unwrap_or("{}")
        .to_string();
    let payload = build_payload(method, path, &ts, &metadata);
    let now = chrono::Utc::now().timestamp();
    validate_signature(&chain, &payload, &ts, tolerance_secs, now)
        .await
        .ok()
}

pub async fn verify_signed_fetch(
    headers: &HeaderMap,
    method: &str,
    path: &str,
    tolerance_secs: i64,
) -> Result<Signer, AuthChainError> {
    let path = signed_fetch_path(headers, path);
    let path = path.as_ref();
    let chain = extract_auth_chain(headers)?;
    let ts = header_str(headers, AUTH_TIMESTAMP_HEADER)
        .ok_or(AuthChainError::MissingTimestamp)?
        .to_string();
    let metadata = header_str(headers, AUTH_METADATA_HEADER)
        .unwrap_or("{}")
        .to_string();
    let payload = build_payload(method, path, &ts, &metadata);
    let now = chrono::Utc::now().timestamp();
    validate_signature(&chain, &payload, &ts, tolerance_secs, now).await
}

pub async fn verify_signed_fetch_meta(
    headers: &HeaderMap,
    method: &str,
    path: &str,
    tolerance_secs: i64,
) -> Result<(Signer, serde_json::Value), AuthChainError> {
    let path = signed_fetch_path(headers, path);
    let path = path.as_ref();
    let chain = extract_auth_chain(headers)?;
    let ts = header_str(headers, AUTH_TIMESTAMP_HEADER)
        .ok_or(AuthChainError::MissingTimestamp)?
        .to_string();
    let metadata_raw = header_str(headers, AUTH_METADATA_HEADER)
        .unwrap_or("{}")
        .to_string();
    let payload = build_payload(method, path, &ts, &metadata_raw);
    let now = chrono::Utc::now().timestamp();
    let signer = validate_signature(&chain, &payload, &ts, tolerance_secs, now).await?;

    let metadata: serde_json::Value =
        serde_json::from_str(&metadata_raw).unwrap_or(serde_json::Value::Null);
    Ok((signer, metadata))
}

/// Unparseable or non-object metadata is refused before anything is verified,
/// and an explicit JSON `null` reads as an empty object: the key guard the
/// fallback runs is only meaningful over an object.
fn parse_metadata(raw: &str) -> Result<serde_json::Value, AuthChainError> {
    let parsed: serde_json::Value =
        serde_json::from_str(raw).map_err(|_| AuthChainError::MalformedChain {
            detail: format!("invalid chain metadata: \"{}\"", truncate_detail(raw)),
        })?;
    match parsed {
        serde_json::Value::Null => Ok(serde_json::Value::Object(serde_json::Map::new())),
        serde_json::Value::Object(_) => Ok(parsed),
        _ => Err(AuthChainError::MalformedChain {
            detail: format!("invalid chain metadata: \"{}\"", truncate_detail(raw)),
        }),
    }
}

/// Verifies against the 6.x payload and, only for a caller that named the
/// metadata keys it authorizes on, falls back to the legacy payload.
///
/// `canonical_metadata_keys` doubles as the switch deliberately: there is no
/// way to accept the legacy payload without naming the fields that make doing
/// so safe. An empty slice is new-format-only, which is the posture every
/// caller should keep unless its signers cannot be shipped ahead of it.
///
/// Stage order matches upstream `verify()`: chain extraction, timestamp,
/// expiration, metadata parse, `metadata_gate`, signature. Freshness first so a
/// replayed or stale request answers 401 Expired without running the gate; the
/// gate then answers before either signature check, so a refused request pays
/// no catalyst round-trip for an EIP-1654 chain, and it guards both payload
/// shapes.
///
/// The fallback is reached only on `InvalidSignature`: every other failure is
/// deterministic in the payload shape, so retrying it would change nothing.
pub async fn verify_signed_fetch_meta_with_legacy_fallback(
    headers: &HeaderMap,
    method: &str,
    path: &str,
    tolerance_secs: i64,
    canonical_metadata_keys: &[&str],
    metadata_gate: Option<&SignerGate>,
) -> Result<(Signer, serde_json::Value), AuthChainError> {
    assert_canonical_metadata_keys(canonical_metadata_keys)?;
    let path = signed_fetch_path(headers, path);
    let path = path.as_ref();
    let chain = extract_auth_chain(headers)?;
    let ts = header_str(headers, AUTH_TIMESTAMP_HEADER)
        .ok_or(AuthChainError::MissingTimestamp)?
        .to_string();
    let now = chrono::Utc::now().timestamp();
    assert_within_window(signed_at_secs(&ts)?, now, tolerance_secs)?;

    let metadata_raw = header_str(headers, AUTH_METADATA_HEADER)
        .unwrap_or("{}")
        .to_string();
    let metadata = parse_metadata(&metadata_raw)?;

    if let Some(gate) = metadata_gate {
        if !gate.permits(&metadata) {
            return Err(AuthChainError::MalformedChain {
                detail: format!(
                    "invalid metadata content: \"{}\"",
                    truncate_detail(&metadata_raw)
                ),
            });
        }
    }

    let payload = build_payload_v6(method, path, &ts, &metadata_raw);
    let signer = match validate_signature(&chain, &payload, &ts, tolerance_secs, now).await {
        Ok(signer) => signer,
        Err(AuthChainError::InvalidSignature(_)) if !canonical_metadata_keys.is_empty() => {
            assert_legacy_metadata_keys(&metadata, canonical_metadata_keys)?;
            let legacy = build_legacy_payload(method, path, &ts, &metadata_raw);
            validate_signature(&chain, &legacy, &ts, tolerance_secs, now).await?
        }
        Err(err) => return Err(err),
    };

    Ok((signer, metadata))
}

pub async fn verify_signed_fetch_with_legacy_fallback(
    headers: &HeaderMap,
    method: &str,
    path: &str,
    tolerance_secs: i64,
    canonical_metadata_keys: &[&str],
    metadata_gate: Option<&SignerGate>,
) -> Result<Signer, AuthChainError> {
    verify_signed_fetch_meta_with_legacy_fallback(
        headers,
        method,
        path,
        tolerance_secs,
        canonical_metadata_keys,
        metadata_gate,
    )
    .await
    .map(|(signer, _)| signer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sign::{create_simple_auth_chain, Wallet};
    use http::HeaderValue;

    const TEST_KEY: &str = "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d";
    const FIVE_MINUTES: i64 = 5 * 60;

    fn signed_headers(wallet: &Wallet, method: &str, path: &str, timestamp_ms: i64) -> HeaderMap {
        let metadata = "{}";
        let payload = build_payload(method, path, &timestamp_ms.to_string(), metadata);
        let chain = create_simple_auth_chain(wallet, &payload).unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTH_TIMESTAMP_HEADER,
            HeaderValue::from_str(&timestamp_ms.to_string()).unwrap(),
        );
        headers.insert(AUTH_METADATA_HEADER, HeaderValue::from_static("{}"));
        for (i, link) in chain.as_array().into_iter().flatten().enumerate() {
            headers.insert(
                http::HeaderName::from_bytes(format!("{AUTH_CHAIN_HEADER_PREFIX}{i}").as_bytes())
                    .unwrap(),
                HeaderValue::from_str(&link.to_string()).unwrap(),
            );
        }
        headers
    }

    #[tokio::test]
    async fn verify_signed_fetch_recovers_signer() {
        let wallet = Wallet::from_hex(TEST_KEY).unwrap();
        let now_ms = chrono::Utc::now().timestamp_millis();
        let headers = signed_headers(&wallet, "get", "/api/thing", now_ms);
        let signer = verify_signed_fetch(&headers, "get", "/api/thing", FIVE_MINUTES)
            .await
            .unwrap();
        assert_eq!(signer, wallet.address().to_lowercase());
    }

    #[tokio::test]
    async fn verify_signed_fetch_rejects_outside_tolerance() {
        let wallet = Wallet::from_hex(TEST_KEY).unwrap();
        let old_ms = chrono::Utc::now().timestamp_millis() - (FIVE_MINUTES + 60) * 1000;
        let headers = signed_headers(&wallet, "get", "/api/thing", old_ms);
        let err = verify_signed_fetch(&headers, "get", "/api/thing", FIVE_MINUTES)
            .await
            .unwrap_err();
        assert!(matches!(err, AuthChainError::Expired { .. }));
        let wider = verify_signed_fetch(&headers, "get", "/api/thing", 30 * 60).await;
        assert!(wider.is_ok());
    }

    #[tokio::test]
    async fn verify_signed_fetch_rejects_wrong_path() {
        let wallet = Wallet::from_hex(TEST_KEY).unwrap();
        let now_ms = chrono::Utc::now().timestamp_millis();
        let headers = signed_headers(&wallet, "get", "/api/thing", now_ms);
        let err = verify_signed_fetch(&headers, "get", "/api/other", FIVE_MINUTES)
            .await
            .unwrap_err();
        assert!(matches!(err, AuthChainError::InvalidSignature(_)));
    }

    #[tokio::test]
    async fn verify_signed_fetch_missing_chain_is_insufficient() {
        let headers = HeaderMap::new();
        let err = verify_signed_fetch(&headers, "get", "/api/thing", FIVE_MINUTES)
            .await
            .unwrap_err();
        assert!(matches!(err, AuthChainError::InsufficientLinks));
    }

    #[tokio::test]
    async fn verify_signed_fetch_meta_parses_metadata() {
        let wallet = Wallet::from_hex(TEST_KEY).unwrap();
        let now_ms = chrono::Utc::now().timestamp_millis();
        let headers = signed_headers(&wallet, "post", "/api/thing", now_ms);
        let (signer, metadata) =
            verify_signed_fetch_meta(&headers, "post", "/api/thing", FIVE_MINUTES)
                .await
                .unwrap();
        assert_eq!(signer, wallet.address().to_lowercase());
        assert_eq!(metadata, serde_json::json!({}));
    }

    #[test]
    fn build_payload_is_lowercased() {
        assert_eq!(
            build_payload("GET", "/ws/MyWorld.dcl.eth", "123", "{}"),
            "get:/ws/myworld.dcl.eth:123:{}"
        );
    }

    #[test]
    fn build_payload_still_mints_the_legacy_shape() {
        let metadata = r#"{"sceneId":"bafkreiAbC"}"#;
        assert_eq!(
            build_payload("GET", "/Api/Thing", "123", metadata),
            build_legacy_payload("GET", "/Api/Thing", "123", metadata)
        );
        assert_eq!(
            build_legacy_payload("GET", "/Api/Thing", "123", metadata),
            r#"get:/api/thing:123:{"sceneid":"bafkreiabc"}"#
        );
    }

    #[test]
    fn build_payload_v6_folds_only_method_and_path() {
        let metadata = r#"{"sceneId":"bafkreiAbC"}"#;
        assert_eq!(
            build_payload_v6("GET", "/Api/Thing", "123", metadata),
            r#"get:/api/thing:123:{"sceneId":"bafkreiAbC"}"#
        );
        assert_ne!(
            build_payload_v6("GET", "/Api/Thing", "123", metadata),
            build_legacy_payload("GET", "/Api/Thing", "123", metadata)
        );
    }

    #[test]
    fn v6_payload_differs_when_only_metadata_casing_changes() {
        let signed = r#"{"signer":"dcl:explorer"}"#;
        let recased = r#"{"Signer":"dcl:explorer"}"#;
        assert_eq!(
            build_legacy_payload("get", "/api", "1", signed),
            build_legacy_payload("get", "/api", "1", recased)
        );
        assert_ne!(
            build_payload_v6("get", "/api", "1", signed),
            build_payload_v6("get", "/api", "1", recased)
        );
    }

    #[test]
    fn signed_fetch_path_trusts_proxy_prefix_and_strips_query() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-original-path",
            HeaderValue::from_static("/market/v1/lists?query=1"),
        );
        assert_eq!(signed_fetch_path(&headers, "/v1/lists"), "/market/v1/lists");
    }

    #[test]
    fn signed_fetch_path_ignores_forged_unrelated_path() {
        let mut headers = HeaderMap::new();
        headers.insert("x-original-path", HeaderValue::from_static("/v1/friends"));
        assert_eq!(
            signed_fetch_path(&headers, "/v1/communities/abc/bans"),
            "/v1/communities/abc/bans"
        );
        assert_eq!(
            signed_fetch_path(&HeaderMap::new(), "/fallback"),
            "/fallback"
        );
    }

    #[tokio::test]
    async fn verify_signed_fetch_rejects_forged_original_path_replay() {
        let wallet = Wallet::from_hex(TEST_KEY).unwrap();
        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut headers = signed_headers(&wallet, "get", "/v1/friends", now_ms);
        headers.insert("x-original-path", HeaderValue::from_static("/v1/friends"));
        let err = verify_signed_fetch(&headers, "get", "/v1/communities/abc/bans", FIVE_MINUTES)
            .await
            .unwrap_err();
        assert!(matches!(err, AuthChainError::InvalidSignature(_)));
    }

    #[tokio::test]
    async fn verify_signed_fetch_accepts_proxy_prefixed_original_path() {
        let wallet = Wallet::from_hex(TEST_KEY).unwrap();
        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut headers = signed_headers(&wallet, "get", "/market/v1/lists", now_ms);
        headers.insert(
            "x-original-path",
            HeaderValue::from_static("/market/v1/lists"),
        );
        let signer = verify_signed_fetch(&headers, "get", "/v1/lists", FIVE_MINUTES)
            .await
            .unwrap();
        assert_eq!(signer, wallet.address().to_lowercase());
    }
}
