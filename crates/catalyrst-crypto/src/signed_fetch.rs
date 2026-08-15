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

use http::HeaderMap;
use thiserror::Error;

use crate::signer::Signer;
use crate::verify::verify_auth_chain;
use crate::AuthError;
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

impl From<AuthChainError> for catalyrst_types::ApiError {
    fn from(e: AuthChainError) -> Self {
        catalyrst_types::ApiError::http(401, format!("Unauthenticated: {e}"))
    }
}

pub fn build_payload(method: &str, path: &str, timestamp: &str, metadata: &str) -> String {
    format!("{}:{}:{}:{}", method, path, timestamp, metadata).to_lowercase()
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

pub fn validate_signature(
    chain: &AuthChain,
    payload: &str,
    timestamp: &str,
    expiration_secs: i64,
    now: i64,
) -> Result<Signer, AuthChainError> {
    // Fail closed: a non-numeric timestamp must be rejected, not silently
    // skip the replay/expiration window below.
    let signed_at_ms = timestamp
        .parse::<i64>()
        .map_err(|_| AuthChainError::InvalidTimestamp(timestamp.to_string()))?;
    let signed_at = signed_at_ms / 1000;
    if (now - signed_at).abs() > expiration_secs {
        return Err(AuthChainError::Expired {
            signed_at,
            now,
            window_secs: expiration_secs,
        });
    }

    let crypto_chain = to_crypto_chain(chain);
    verify_auth_chain(&crypto_chain, payload, Some(now * 1000)).map_err(map_auth_error)?;
    Ok(Signer::from_verified_chain(&chain.signer))
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
        AuthError::Eip1654NotImplemented
        | AuthError::Eip1654ValidationFailed(_)
        | AuthError::Eip1654Rejected { .. } => AuthChainError::EipNotImplemented,
    }
}

pub fn try_extract(headers: &HeaderMap) -> Option<AuthChain> {
    extract_auth_chain(headers).ok()
}

pub fn try_extract_signer(
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
    validate_signature(&chain, &payload, &ts, tolerance_secs, now).ok()
}

pub fn verify_signed_fetch(
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
    validate_signature(&chain, &payload, &ts, tolerance_secs, now)
}

pub fn verify_signed_fetch_meta(
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
    let signer = validate_signature(&chain, &payload, &ts, tolerance_secs, now)?;

    let metadata: serde_json::Value =
        serde_json::from_str(&metadata_raw).unwrap_or(serde_json::Value::Null);
    Ok((signer, metadata))
}

pub mod handshake {
    use serde_json::Value;
    use thiserror::Error;

    use super::{
        build_payload, signed_fetch_path, AuthChain, AuthLink, AUTH_CHAIN_HEADER_PREFIX,
        AUTH_METADATA_HEADER, AUTH_TIMESTAMP_HEADER, MAX_AUTH_CHAIN_LINKS,
    };
    use crate::signer::Signer;
    use crate::verify::verify_auth_chain;
    use crate::AuthError;
    use catalyrst_types::{AuthLink as CryptoAuthLink, AuthLinkType};

    #[derive(Debug, Error)]
    pub enum AuthChainError {
        #[error("invalid auth-chain envelope: not a JSON object")]
        EnvelopeNotObject,
        #[error("invalid auth-chain link {index}: {detail}")]
        MalformedChain { index: usize, detail: String },
        #[error("auth-chain shorter than 2 links")]
        InsufficientLinks,
        #[error("missing {0}")]
        MissingHeader(&'static str),
        #[error("signature older than {window_secs}s window: signed_at={signed_at} now={now}")]
        Expired {
            signed_at: i64,
            now: i64,
            window_secs: i64,
        },
        #[error("signature did not verify: {0}")]
        InvalidSignature(String),
        #[error("EIP-1654 chains not implemented")]
        EipNotImplemented,
    }

    pub fn obj_str<'a>(obj: &'a serde_json::Map<String, Value>, key: &str) -> Option<&'a str> {
        obj.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .and_then(|(_, v)| v.as_str())
    }

    pub fn extract_from_object(
        obj: &serde_json::Map<String, Value>,
    ) -> Result<AuthChain, AuthChainError> {
        let mut links = Vec::new();

        for i in 0..MAX_AUTH_CHAIN_LINKS {
            let name = format!("{}{}", AUTH_CHAIN_HEADER_PREFIX, i);
            let Some(raw) = obj_str(obj, &name) else {
                break;
            };
            let link: CryptoAuthLink = serde_json::from_str(raw).map_err(|e| {
                let mut detail = e.to_string();
                if detail.len() > 64 {
                    detail.truncate(64);
                }
                AuthChainError::MalformedChain { index: i, detail }
            })?;
            match link.link_type {
                AuthLinkType::SIGNER => {
                    if i != 0 {
                        return Err(AuthChainError::MalformedChain {
                            index: i,
                            detail: "SIGNER link at non-zero index".into(),
                        });
                    }
                }
                _ => {
                    if i == 0 {
                        return Err(AuthChainError::MalformedChain {
                            index: 0,
                            detail: "first link must be SIGNER".into(),
                        });
                    }
                    if link.signature.as_deref().unwrap_or("").is_empty() {
                        return Err(AuthChainError::MalformedChain {
                            index: i,
                            detail: "missing signature".into(),
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
        if obj_str(obj, &overflow).is_some() {
            return Err(AuthChainError::MalformedChain {
                index: MAX_AUTH_CHAIN_LINKS,
                detail: format!("exceeds max length {MAX_AUTH_CHAIN_LINKS}"),
            });
        }
        if links.len() < 2 {
            return Err(AuthChainError::InsufficientLinks);
        }
        let signer = links[0].payload.to_lowercase();
        Ok(AuthChain { links, signer })
    }

    pub fn validate_signature(
        chain: &AuthChain,
        payload: &str,
        timestamp: &str,
        expiration_secs: i64,
        now: i64,
    ) -> Result<Signer, AuthChainError> {
        // Fail closed: a non-numeric timestamp must be rejected, not silently
        // skip the replay/expiration window below. (This module's AuthChainError
        // has no InvalidTimestamp variant, so map to the closest: MalformedChain.)
        let signed_at_ms =
            timestamp
                .parse::<i64>()
                .map_err(|_| AuthChainError::MalformedChain {
                    index: 0,
                    detail: format!("invalid timestamp: {timestamp}"),
                })?;
        let signed_at = signed_at_ms / 1000;
        if (now - signed_at).abs() > expiration_secs {
            return Err(AuthChainError::Expired {
                signed_at,
                now,
                window_secs: expiration_secs,
            });
        }

        let crypto_chain = super::to_crypto_chain(chain);
        verify_auth_chain(&crypto_chain, payload, Some(now * 1000)).map_err(map_auth_error)?;
        Ok(Signer::from_verified_chain(&chain.signer))
    }

    fn map_auth_error(err: AuthError) -> AuthChainError {
        match err {
            AuthError::MalformedChain(d) => AuthChainError::MalformedChain {
                index: 0,
                detail: d,
            },
            AuthError::MissingSignature { .. } => AuthChainError::MalformedChain {
                index: 0,
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
            AuthError::InvalidEphemeralPayload(d) => AuthChainError::MalformedChain {
                index: 0,
                detail: d,
            },
            AuthError::Eip1654NotImplemented
            | AuthError::Eip1654ValidationFailed(_)
            | AuthError::Eip1654Rejected { .. } => AuthChainError::EipNotImplemented,
        }
    }

    pub fn verify_handshake(
        frame_json: &str,
        method: &str,
        path: &str,
        expiration_secs: i64,
        now_secs: i64,
    ) -> Result<Signer, AuthChainError> {
        let value: Value =
            serde_json::from_str(frame_json).map_err(|e| AuthChainError::MalformedChain {
                index: 0,
                detail: format!("frame not JSON: {e}"),
            })?;
        let obj = value.as_object().ok_or(AuthChainError::EnvelopeNotObject)?;

        let chain = extract_from_object(obj)?;
        let timestamp = obj_str(obj, AUTH_TIMESTAMP_HEADER)
            .ok_or(AuthChainError::MissingHeader(AUTH_TIMESTAMP_HEADER))?;
        let metadata = obj_str(obj, AUTH_METADATA_HEADER).unwrap_or("{}");
        let payload = build_payload(method, path, timestamp, metadata);
        validate_signature(&chain, &payload, timestamp, expiration_secs, now_secs)
    }

    pub fn require_signer(
        headers: &http::HeaderMap,
        method: &str,
        path: &str,
        tolerance_secs: i64,
    ) -> Result<Signer, AuthChainError> {
        let path = signed_fetch_path(headers, path);
        let path = path.as_ref();
        let mut value = serde_json::Map::new();
        for (name, val) in headers.iter() {
            if let Ok(s) = val.to_str() {
                value.insert(name.as_str().to_string(), Value::String(s.to_string()));
            }
        }
        let chain = extract_from_object(&value)?;
        let timestamp = obj_str(&value, AUTH_TIMESTAMP_HEADER)
            .ok_or(AuthChainError::MissingHeader(AUTH_TIMESTAMP_HEADER))?
            .to_string();
        let metadata = obj_str(&value, AUTH_METADATA_HEADER)
            .unwrap_or("{}")
            .to_string();
        let payload = build_payload(method, path, &timestamp, &metadata);
        let now = chrono::Utc::now().timestamp();
        validate_signature(&chain, &payload, &timestamp, tolerance_secs, now)
    }

    pub fn optional_signer(
        headers: &http::HeaderMap,
        method: &str,
        path: &str,
        tolerance_secs: i64,
    ) -> Option<Signer> {
        if !headers.keys().any(|k| {
            k.as_str()
                .eq_ignore_ascii_case(&format!("{}0", AUTH_CHAIN_HEADER_PREFIX))
        }) {
            return None;
        }
        require_signer(headers, method, path, tolerance_secs).ok()
    }
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

    #[test]
    fn verify_signed_fetch_recovers_signer() {
        let wallet = Wallet::from_hex(TEST_KEY).unwrap();
        let now_ms = chrono::Utc::now().timestamp_millis();
        let headers = signed_headers(&wallet, "get", "/api/thing", now_ms);
        let signer = verify_signed_fetch(&headers, "get", "/api/thing", FIVE_MINUTES).unwrap();
        assert_eq!(signer, wallet.address().to_lowercase());
    }

    #[test]
    fn verify_signed_fetch_rejects_outside_tolerance() {
        let wallet = Wallet::from_hex(TEST_KEY).unwrap();
        let old_ms = chrono::Utc::now().timestamp_millis() - (FIVE_MINUTES + 60) * 1000;
        let headers = signed_headers(&wallet, "get", "/api/thing", old_ms);
        let err = verify_signed_fetch(&headers, "get", "/api/thing", FIVE_MINUTES).unwrap_err();
        assert!(matches!(err, AuthChainError::Expired { .. }));
        let wider = verify_signed_fetch(&headers, "get", "/api/thing", 30 * 60);
        assert!(wider.is_ok());
    }

    #[test]
    fn verify_signed_fetch_rejects_wrong_path() {
        let wallet = Wallet::from_hex(TEST_KEY).unwrap();
        let now_ms = chrono::Utc::now().timestamp_millis();
        let headers = signed_headers(&wallet, "get", "/api/thing", now_ms);
        let err = verify_signed_fetch(&headers, "get", "/api/other", FIVE_MINUTES).unwrap_err();
        assert!(matches!(err, AuthChainError::InvalidSignature(_)));
    }

    #[test]
    fn verify_signed_fetch_missing_chain_is_insufficient() {
        let headers = HeaderMap::new();
        let err = verify_signed_fetch(&headers, "get", "/api/thing", FIVE_MINUTES).unwrap_err();
        assert!(matches!(err, AuthChainError::InsufficientLinks));
    }

    #[test]
    fn verify_signed_fetch_meta_parses_metadata() {
        let wallet = Wallet::from_hex(TEST_KEY).unwrap();
        let now_ms = chrono::Utc::now().timestamp_millis();
        let headers = signed_headers(&wallet, "post", "/api/thing", now_ms);
        let (signer, metadata) =
            verify_signed_fetch_meta(&headers, "post", "/api/thing", FIVE_MINUTES).unwrap();
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

    #[test]
    fn verify_signed_fetch_rejects_forged_original_path_replay() {
        let wallet = Wallet::from_hex(TEST_KEY).unwrap();
        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut headers = signed_headers(&wallet, "get", "/v1/friends", now_ms);
        headers.insert("x-original-path", HeaderValue::from_static("/v1/friends"));
        let err = verify_signed_fetch(&headers, "get", "/v1/communities/abc/bans", FIVE_MINUTES)
            .unwrap_err();
        assert!(matches!(err, AuthChainError::InvalidSignature(_)));
    }

    #[test]
    fn verify_signed_fetch_accepts_proxy_prefixed_original_path() {
        let wallet = Wallet::from_hex(TEST_KEY).unwrap();
        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut headers = signed_headers(&wallet, "get", "/market/v1/lists", now_ms);
        headers.insert(
            "x-original-path",
            HeaderValue::from_static("/market/v1/lists"),
        );
        let signer = verify_signed_fetch(&headers, "get", "/v1/lists", FIVE_MINUTES).unwrap();
        assert_eq!(signer, wallet.address().to_lowercase());
    }
}
