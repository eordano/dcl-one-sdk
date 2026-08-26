//! Gates over the parsed `x-identity-metadata` object.
//!
//! The 6.x signed-fetch payload joins the metadata bytes verbatim, so what a
//! handler reads is exactly what was signed and nothing may be normalized on
//! the way in. These gates therefore *refuse* a non-canonical spelling instead
//! of folding it before comparing: folding bases the decision on a value the
//! handler never sees, and comparing without folding lets
//! `Decentraland-Kernel-Scene` read as "not a scene" and walk through.
//!
//! Ordering is a call-site contract, not something these functions can enforce:
//! run them BEFORE signature verification so a rejection is a 400 that costs no
//! catalyst round-trip for an EIP-1654 chain.

use serde_json::{Map, Value};
use thiserror::Error;

use crate::signed_fetch::AuthChainError;

const SIGNER_KEY: &str = "signer";
const DETAIL_MAX_CHARS: usize = 64;

#[derive(Debug, Error)]
pub enum SignerGateError {
    #[error("signer gate requires at least one value")]
    NoValues,
    #[error("signer gate expects non-empty canonical (trimmed, lowercase) values, got: {0}")]
    NotCanonical(String),
}

pub(crate) fn truncate_detail(value: &str) -> String {
    if value.chars().count() > DETAIL_MAX_CHARS {
        let head: String = value.chars().take(DETAIL_MAX_CHARS).collect();
        format!("{head}...")
    } else {
        value.to_string()
    }
}

/// Upstream folds with `String.prototype.trim`, whose WhiteSpace set includes
/// ZWNBSP (U+FEFF); Rust's `str::trim` follows the Unicode `White_Space`
/// property, which does not. Without U+FEFF here a `"\u{FEFF}<signer>"` reads
/// as canonical, misses the equality check and walks through a gate upstream
/// fails closed on.
fn js_trim(value: &str) -> &str {
    value.trim_matches(|c: char| c.is_whitespace() || c == '\u{FEFF}')
}

fn is_canonical(value: &str) -> bool {
    value == js_trim(value).to_lowercase().as_str()
}

/// The "this endpoint is not for those signers" gate.
///
/// A request carrying no `signer` passes: it is not claiming to be one. A
/// `signer` that is present but not canonical is refused rather than compared,
/// so a re-spelled value cannot read as absent and slip through.
#[derive(Debug, Clone)]
pub struct SignerGate {
    rejected: Vec<String>,
}

impl SignerGate {
    /// Reads `signer` as an own key of the metadata object, which
    /// `serde_json::Map` gives structurally - there is no prototype chain for a
    /// value the client never sent to arrive through.
    pub fn permits(&self, metadata: &Value) -> bool {
        let Some(object) = metadata.as_object() else {
            return true;
        };
        let Some(declared) = object.get(SIGNER_KEY) else {
            return true;
        };
        let Some(signer) = declared.as_str() else {
            return false;
        };
        is_canonical(signer) && !self.rejected.iter().any(|value| value == signer)
    }
}

/// Build once at startup and reuse: a non-canonical argument could never match
/// a value that passed the canonical check, so it must be a startup failure
/// rather than a gate that silently never fires.
pub fn reject_if_signer(signers: &[&str]) -> Result<SignerGate, SignerGateError> {
    if signers.is_empty() {
        return Err(SignerGateError::NoValues);
    }
    for signer in signers {
        if signer.is_empty() || !is_canonical(signer) {
            return Err(SignerGateError::NotCanonical(truncate_detail(signer)));
        }
    }
    Ok(SignerGate {
        rejected: signers.iter().map(|signer| (*signer).to_string()).collect(),
    })
}

/// Checked before the first signature attempt, not on the legacy branch, so a
/// misconfigured rollout fails on the first request rather than on the first
/// one that happens to need the fallback.
pub fn assert_canonical_metadata_keys(canonical_keys: &[&str]) -> Result<(), AuthChainError> {
    for declared_path in canonical_keys {
        if declared_path.split('.').any(|segment| segment.is_empty()) {
            return Err(AuthChainError::MalformedChain {
                detail: format!(
                    "canonical metadata key has an empty path segment: \"{}\"",
                    truncate_detail(declared_path)
                ),
            });
        }
    }
    Ok(())
}

fn objects_to_inspect<'a>(value: &'a Value, out: &mut Vec<&'a Map<String, Value>>) {
    match value {
        Value::Array(items) => {
            for item in items {
                objects_to_inspect(item, out);
            }
        }
        Value::Object(object) => out.push(object),
        _ => {}
    }
}

/// Refuses legacy-signed metadata whose keys are not in the spelling the
/// service declared.
///
/// The legacy payload folds the metadata, so `{"Signer":...}` and
/// `{"signer":...}` share one valid signature while a service comparing
/// `metadata["signer"]` reads the first as absent. Requiring the declared
/// spelling removes that ambiguity rather than resolving it, so nothing is
/// rewritten.
///
/// Only keys are checked. Values belong to [`SignerGate`], which runs on both
/// payload shapes; requiring canonical values here would refuse legitimate
/// traffic, since fields such as `sceneId` carry case-sensitive CIDs.
pub fn assert_legacy_metadata_keys(
    metadata: &Value,
    canonical_keys: &[&str],
) -> Result<(), AuthChainError> {
    for declared_path in canonical_keys {
        let mut containers: Vec<&Value> = vec![metadata];

        for segment in declared_path.split('.') {
            let folded = segment.to_lowercase();
            let mut objects: Vec<&Map<String, Value>> = Vec::new();
            for container in &containers {
                objects_to_inspect(container, &mut objects);
            }

            let mut next: Vec<&Value> = Vec::new();
            for object in objects {
                let delivered: Vec<&String> = object
                    .keys()
                    .filter(|key| key.to_lowercase() == folded)
                    .collect();

                if delivered.is_empty() {
                    continue;
                }
                // Two spellings fold to one field, so which value the service
                // reads depends on key order rather than on anything the
                // signature pinned. Refused even when one of them is canonical.
                if delivered.len() > 1 {
                    return Err(AuthChainError::MalformedChain {
                        detail: format!(
                            "invalid chain metadata: \"{}\" delivered under {} spellings",
                            truncate_detail(segment),
                            delivered.len()
                        ),
                    });
                }
                if delivered[0].as_str() != segment {
                    return Err(AuthChainError::MalformedChain {
                        detail: format!(
                            "invalid chain metadata: expected \"{}\", got \"{}\"",
                            truncate_detail(segment),
                            truncate_detail(delivered[0])
                        ),
                    });
                }
                if let Some(value) = object.get(segment) {
                    next.push(value);
                }
            }

            if next.is_empty() {
                break;
            }
            containers = next;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SCENE_SIGNER: &str = "decentraland-kernel-scene";
    const SCENE_KEYS: &[&str] = &["signer", "intent", "sceneId", "realm.serverName"];

    fn scene_gate() -> SignerGate {
        reject_if_signer(&[SCENE_SIGNER]).unwrap()
    }

    fn detail(err: AuthChainError) -> String {
        match err {
            AuthChainError::MalformedChain { detail } => detail,
            other => panic!("expected MalformedChain, got {other:?}"),
        }
    }

    #[test]
    fn gate_passes_a_request_declaring_no_signer() {
        assert!(scene_gate().permits(&json!({ "intent": "dcl:explorer:comms-handshake" })));
        assert!(scene_gate().permits(&json!({})));
        assert!(scene_gate().permits(&Value::Null));
    }

    #[test]
    fn gate_refuses_the_banned_signer() {
        assert!(!scene_gate().permits(&json!({ "signer": SCENE_SIGNER })));
    }

    #[test]
    fn gate_refuses_a_recased_or_padded_signer_instead_of_folding_it() {
        assert!(!scene_gate().permits(&json!({ "signer": "Decentraland-Kernel-Scene" })));
        assert!(!scene_gate().permits(&json!({ "signer": "DECENTRALAND-KERNEL-SCENE" })));
        assert!(!scene_gate().permits(&json!({ "signer": " decentraland-kernel-scene " })));
    }

    #[test]
    fn gate_refuses_a_signer_wrapped_in_a_byte_order_mark() {
        let bom = format!("\u{FEFF}{SCENE_SIGNER}");
        assert!(!scene_gate().permits(&json!({ "signer": bom })));
        assert!(!scene_gate().permits(&json!({ "signer": format!("{SCENE_SIGNER}\u{FEFF}") })));
        assert!(!scene_gate().permits(&json!({ "signer": "\u{FEFF}dcl:explorer" })));
    }

    #[test]
    fn gate_refuses_a_signer_that_is_not_a_string() {
        assert!(!scene_gate().permits(&json!({ "signer": 42 })));
        assert!(!scene_gate().permits(&json!({ "signer": null })));
        assert!(!scene_gate().permits(&json!({ "signer": { "value": SCENE_SIGNER } })));
    }

    #[test]
    fn gate_passes_an_unrelated_canonical_signer() {
        assert!(scene_gate().permits(&json!({ "signer": "dcl:explorer" })));
        assert!(scene_gate().permits(&json!({ "signer": "" })));
    }

    #[test]
    fn gate_construction_refuses_a_non_canonical_or_empty_declaration() {
        assert!(matches!(
            reject_if_signer(&[]),
            Err(SignerGateError::NoValues)
        ));
        assert!(matches!(
            reject_if_signer(&["Decentraland-Kernel-Scene"]),
            Err(SignerGateError::NotCanonical(_))
        ));
        assert!(matches!(
            reject_if_signer(&[" decentraland-kernel-scene"]),
            Err(SignerGateError::NotCanonical(_))
        ));
        assert!(matches!(
            reject_if_signer(&[""]),
            Err(SignerGateError::NotCanonical(_))
        ));
    }

    #[test]
    fn gate_construction_refuses_a_declaration_carrying_a_byte_order_mark() {
        let bom = format!("\u{FEFF}{SCENE_SIGNER}");
        assert!(matches!(
            reject_if_signer(&[bom.as_str()]),
            Err(SignerGateError::NotCanonical(_))
        ));
        let trailing = format!("{SCENE_SIGNER}\u{FEFF}");
        assert!(matches!(
            reject_if_signer(&[trailing.as_str()]),
            Err(SignerGateError::NotCanonical(_))
        ));
    }

    #[test]
    fn canonical_keys_option_refuses_empty_path_segments() {
        assert!(assert_canonical_metadata_keys(SCENE_KEYS).is_ok());
        assert!(assert_canonical_metadata_keys(&[]).is_ok());
        assert!(assert_canonical_metadata_keys(&["realm..serverName"]).is_err());
        assert!(assert_canonical_metadata_keys(&[".signer"]).is_err());
        assert!(assert_canonical_metadata_keys(&["signer."]).is_err());
        assert!(assert_canonical_metadata_keys(&[""]).is_err());
    }

    #[test]
    fn legacy_keys_accept_the_declared_spelling() {
        let metadata = json!({
            "signer": "dcl:explorer",
            "intent": "dcl:explorer:comms-handshake",
            "sceneId": "bafkreiAbC123",
            "realm": { "serverName": "LocalPreview" }
        });
        assert!(assert_legacy_metadata_keys(&metadata, SCENE_KEYS).is_ok());
    }

    #[test]
    fn legacy_keys_refuse_a_recased_top_level_key() {
        let metadata = json!({ "sceneId": "bafkreiAbC123", "SIGNER": "dcl:explorer" });
        let err = detail(assert_legacy_metadata_keys(&metadata, SCENE_KEYS).unwrap_err());
        assert!(err.contains("expected \"signer\""), "{err}");
    }

    #[test]
    fn legacy_keys_refuse_a_recased_nested_key() {
        let metadata = json!({ "realm": { "servername": "LocalPreview" } });
        let err = detail(assert_legacy_metadata_keys(&metadata, SCENE_KEYS).unwrap_err());
        assert!(err.contains("expected \"serverName\""), "{err}");
    }

    #[test]
    fn legacy_keys_refuse_two_spellings_of_one_field_as_ambiguous() {
        let metadata: Value =
            serde_json::from_str(r#"{"signer":"dcl:explorer","Signer":"other"}"#).unwrap();
        let err = detail(assert_legacy_metadata_keys(&metadata, SCENE_KEYS).unwrap_err());
        assert!(err.contains("2 spellings"), "{err}");

        let nested: Value =
            serde_json::from_str(r#"{"realm":{"serverName":"a","servername":"b"}}"#).unwrap();
        let err = detail(assert_legacy_metadata_keys(&nested, SCENE_KEYS).unwrap_err());
        assert!(err.contains("2 spellings"), "{err}");
    }

    #[test]
    fn legacy_keys_walk_every_element_of_an_array() {
        let keys = &["items.sceneId"];
        let canonical = json!({ "items": [{ "sceneId": "a" }, { "sceneId": "b" }] });
        assert!(assert_legacy_metadata_keys(&canonical, keys).is_ok());

        let second_recased = json!({ "items": [{ "sceneId": "a" }, { "SceneId": "b" }] });
        assert!(assert_legacy_metadata_keys(&second_recased, keys).is_err());

        let nested_arrays = json!({ "items": [[{ "sceneid": "a" }]] });
        assert!(assert_legacy_metadata_keys(&nested_arrays, keys).is_err());
    }

    #[test]
    fn legacy_keys_ignore_paths_that_reach_nothing() {
        let keys = &["items.sceneId"];
        assert!(assert_legacy_metadata_keys(&json!({ "items": ["a", "b"] }), keys).is_ok());
        assert!(assert_legacy_metadata_keys(&json!({ "items": 7 }), keys).is_ok());
        assert!(assert_legacy_metadata_keys(&json!({}), keys).is_ok());
        assert!(assert_legacy_metadata_keys(&Value::Null, keys).is_ok());
    }

    #[test]
    fn legacy_keys_leave_undeclared_fields_alone() {
        let metadata = json!({ "signer": "dcl:explorer", "isguest": true });
        assert!(assert_legacy_metadata_keys(&metadata, SCENE_KEYS).is_ok());
    }

    #[test]
    fn legacy_keys_guard_a_container_only_at_the_depth_declared() {
        let metadata = json!({ "Realm": { "serverName": "LocalPreview" } });
        assert!(assert_legacy_metadata_keys(&metadata, &["realm.serverName"]).is_err());
        assert!(assert_legacy_metadata_keys(&metadata, &["Realm"]).is_ok());
    }
}
