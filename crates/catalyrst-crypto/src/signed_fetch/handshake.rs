use serde_json::Value;
use thiserror::Error;

use super::{
    build_legacy_payload, build_payload, build_payload_v6, default_eip1654_validator,
    parse_metadata, signed_fetch_path, AuthChain, AuthLink, AUTH_CHAIN_HEADER_PREFIX,
    AUTH_METADATA_HEADER, AUTH_TIMESTAMP_HEADER, MAX_AUTH_CHAIN_LINKS,
};
use crate::eip1654::Eip1654Validator;
use crate::metadata_gate::{
    assert_canonical_metadata_keys, assert_legacy_metadata_keys, truncate_detail, SignerGate,
};
use crate::signer::Signer;
use crate::verify::verify_auth_chain_async;
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

    let crypto_chain = super::to_crypto_chain(chain);
    verify_auth_chain_async(&crypto_chain, payload, Some(now * 1000), validator)
        .await
        .map_err(map_auth_error)?;
    Ok(Signer::from_verified_chain(&chain.signer))
}

/// Fail closed: a non-numeric timestamp must be rejected, not silently skip
/// the replay/expiration window. This module's error has no
/// `InvalidTimestamp` variant, so it maps to the closest 400-class one.
fn signed_at_secs(timestamp: &str) -> Result<i64, AuthChainError> {
    timestamp
        .parse::<i64>()
        .map(|ms| ms / 1000)
        .map_err(|_| AuthChainError::MalformedChain {
            index: 0,
            detail: format!("invalid timestamp: {timestamp}"),
        })
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

/// The metadata gates are shared with the HTTP path, so they report through its
/// `AuthChainError`. Only their 400-class `MalformedChain` can reach here, and
/// the detail is what tells one refusal from another during a rollout.
fn map_metadata_error(err: super::AuthChainError) -> AuthChainError {
    AuthChainError::MalformedChain {
        index: 0,
        detail: match err {
            super::AuthChainError::MalformedChain { detail } => detail,
            other => other.to_string(),
        },
    }
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
        AuthError::Eip1654NotImplemented => AuthChainError::EipNotImplemented,
        AuthError::Eip1654ValidationFailed(_) | AuthError::Eip1654Rejected { .. } => {
            AuthChainError::InvalidSignature(err.to_string())
        }
    }
}

pub async fn verify_handshake(
    frame_json: &str,
    method: &str,
    path: &str,
    expiration_secs: i64,
    now_secs: i64,
) -> Result<Signer, AuthChainError> {
    let obj = frame_object(frame_json)?;
    let chain = extract_from_object(&obj)?;
    let timestamp = obj_str(&obj, AUTH_TIMESTAMP_HEADER)
        .ok_or(AuthChainError::MissingHeader(AUTH_TIMESTAMP_HEADER))?;
    let metadata = obj_str(&obj, AUTH_METADATA_HEADER).unwrap_or("{}");
    let payload = build_payload(method, path, timestamp, metadata);
    validate_signature(&chain, &payload, timestamp, expiration_secs, now_secs).await
}

fn header_object(headers: &http::HeaderMap) -> serde_json::Map<String, Value> {
    let mut value = serde_json::Map::new();
    for (name, val) in headers.iter() {
        if let Ok(s) = val.to_str() {
            value.insert(name.as_str().to_string(), Value::String(s.to_string()));
        }
    }
    value
}

pub async fn require_signer(
    headers: &http::HeaderMap,
    method: &str,
    path: &str,
    tolerance_secs: i64,
) -> Result<Signer, AuthChainError> {
    let path = signed_fetch_path(headers, path);
    let path = path.as_ref();
    let value = header_object(headers);
    let chain = extract_from_object(&value)?;
    let timestamp = obj_str(&value, AUTH_TIMESTAMP_HEADER)
        .ok_or(AuthChainError::MissingHeader(AUTH_TIMESTAMP_HEADER))?
        .to_string();
    let metadata = obj_str(&value, AUTH_METADATA_HEADER)
        .unwrap_or("{}")
        .to_string();
    let payload = build_payload(method, path, &timestamp, &metadata);
    let now = chrono::Utc::now().timestamp();
    validate_signature(&chain, &payload, &timestamp, tolerance_secs, now).await
}

pub async fn optional_signer(
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
    require_signer(headers, method, path, tolerance_secs)
        .await
        .ok()
}

/// The `verify_signed_fetch_meta_with_legacy_fallback` contract over a
/// header bag encoded as one JSON object, which is what the WS/RPC
/// handshake frame is.
///
/// Stage order matches upstream `verify()`: chain, timestamp, expiration,
/// metadata parse, `metadata_gate`, signature. `canonical_metadata_keys`
/// doubles as the legacy switch: an empty slice is 6.x-only, and there is
/// no way to accept the legacy payload without naming the fields that make
/// doing so safe. The fallback is reached only on `InvalidSignature` -
/// every other failure is deterministic in the payload shape.
async fn verify_object_v6(
    obj: &serde_json::Map<String, Value>,
    method: &str,
    path: &str,
    expiration_secs: i64,
    now_secs: i64,
    canonical_metadata_keys: &[&str],
    metadata_gate: Option<&SignerGate>,
) -> Result<(Signer, Value), AuthChainError> {
    assert_canonical_metadata_keys(canonical_metadata_keys).map_err(map_metadata_error)?;
    let chain = extract_from_object(obj)?;
    let timestamp = obj_str(obj, AUTH_TIMESTAMP_HEADER)
        .ok_or(AuthChainError::MissingHeader(AUTH_TIMESTAMP_HEADER))?;
    assert_within_window(signed_at_secs(timestamp)?, now_secs, expiration_secs)?;

    let metadata_raw = obj_str(obj, AUTH_METADATA_HEADER).unwrap_or("{}");
    let metadata = parse_metadata(metadata_raw).map_err(map_metadata_error)?;

    if let Some(gate) = metadata_gate {
        if !gate.permits(&metadata) {
            return Err(AuthChainError::MalformedChain {
                index: 0,
                detail: format!(
                    "invalid metadata content: \"{}\"",
                    truncate_detail(metadata_raw)
                ),
            });
        }
    }

    let payload = build_payload_v6(method, path, timestamp, metadata_raw);
    let signer =
        match validate_signature(&chain, &payload, timestamp, expiration_secs, now_secs).await {
            Ok(signer) => signer,
            Err(AuthChainError::InvalidSignature(_)) if !canonical_metadata_keys.is_empty() => {
                assert_legacy_metadata_keys(&metadata, canonical_metadata_keys)
                    .map_err(map_metadata_error)?;
                let legacy = build_legacy_payload(method, path, timestamp, metadata_raw);
                validate_signature(&chain, &legacy, timestamp, expiration_secs, now_secs).await?
            }
            Err(err) => return Err(err),
        };

    Ok((signer, metadata))
}

fn frame_object(frame_json: &str) -> Result<serde_json::Map<String, Value>, AuthChainError> {
    let value: Value =
        serde_json::from_str(frame_json).map_err(|e| AuthChainError::MalformedChain {
            index: 0,
            detail: format!("frame not JSON: {e}"),
        })?;
    match value {
        Value::Object(obj) => Ok(obj),
        _ => Err(AuthChainError::EnvelopeNotObject),
    }
}

pub async fn verify_handshake_meta_v6(
    frame_json: &str,
    method: &str,
    path: &str,
    expiration_secs: i64,
    now_secs: i64,
    canonical_metadata_keys: &[&str],
    metadata_gate: Option<&SignerGate>,
) -> Result<(Signer, Value), AuthChainError> {
    let obj = frame_object(frame_json)?;
    verify_object_v6(
        &obj,
        method,
        path,
        expiration_secs,
        now_secs,
        canonical_metadata_keys,
        metadata_gate,
    )
    .await
}

pub async fn verify_handshake_v6(
    frame_json: &str,
    method: &str,
    path: &str,
    expiration_secs: i64,
    now_secs: i64,
    canonical_metadata_keys: &[&str],
    metadata_gate: Option<&SignerGate>,
) -> Result<Signer, AuthChainError> {
    verify_handshake_meta_v6(
        frame_json,
        method,
        path,
        expiration_secs,
        now_secs,
        canonical_metadata_keys,
        metadata_gate,
    )
    .await
    .map(|(signer, _)| signer)
}

pub async fn require_signer_meta_v6(
    headers: &http::HeaderMap,
    method: &str,
    path: &str,
    tolerance_secs: i64,
    canonical_metadata_keys: &[&str],
    metadata_gate: Option<&SignerGate>,
) -> Result<(Signer, Value), AuthChainError> {
    let path = signed_fetch_path(headers, path);
    let path = path.as_ref();
    verify_object_v6(
        &header_object(headers),
        method,
        path,
        tolerance_secs,
        chrono::Utc::now().timestamp(),
        canonical_metadata_keys,
        metadata_gate,
    )
    .await
}

pub async fn require_signer_v6(
    headers: &http::HeaderMap,
    method: &str,
    path: &str,
    tolerance_secs: i64,
    canonical_metadata_keys: &[&str],
    metadata_gate: Option<&SignerGate>,
) -> Result<Signer, AuthChainError> {
    require_signer_meta_v6(
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

pub async fn optional_signer_v6(
    headers: &http::HeaderMap,
    method: &str,
    path: &str,
    tolerance_secs: i64,
    canonical_metadata_keys: &[&str],
    metadata_gate: Option<&SignerGate>,
) -> Option<Signer> {
    if !headers.keys().any(|k| {
        k.as_str()
            .eq_ignore_ascii_case(&format!("{}0", AUTH_CHAIN_HEADER_PREFIX))
    }) {
        return None;
    }
    require_signer_v6(
        headers,
        method,
        path,
        tolerance_secs,
        canonical_metadata_keys,
        metadata_gate,
    )
    .await
    .ok()
}
