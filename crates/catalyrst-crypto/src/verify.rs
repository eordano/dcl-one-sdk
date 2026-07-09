use tracing::debug;

use crate::auth_chain::{
    is_valid_auth_chain, parse_ephemeral_payload, AuthLinkType, MAX_AUTH_CHAIN_LINKS,
};
use crate::eip1654::{verify_eip1654, Eip1654Validator};
use crate::error::AuthError;
use crate::recover::recover_address;
use crate::AuthChain;

pub fn verify_auth_chain(
    chain: &AuthChain,
    expected_address: &str,
    now_ms: Option<i64>,
) -> Result<(), AuthError> {
    verify_chain_inner(chain, expected_address, now_ms)
}

pub fn verify_auth_chain_with_validator(
    chain: &AuthChain,
    expected_address: &str,
    now_ms: Option<i64>,
    _eip1654_validator: Option<&dyn Eip1654Validator>,
) -> Result<(), AuthError> {
    verify_chain_inner(chain, expected_address, now_ms)
}

pub async fn verify_auth_chain_async(
    chain: &AuthChain,
    expected_address: &str,
    now_ms: Option<i64>,
    eip1654_validator: Option<&dyn Eip1654Validator>,
) -> Result<(), AuthError> {
    if chain.len() > MAX_AUTH_CHAIN_LINKS {
        return Err(AuthError::MalformedChain(format!(
            "auth chain too long: {} links (max {})",
            chain.len(),
            MAX_AUTH_CHAIN_LINKS
        )));
    }
    if !is_valid_auth_chain(chain) {
        return Err(AuthError::MalformedChain("invalid chain structure".into()));
    }

    let now_ms = now_ms.unwrap_or_else(|| chrono::Utc::now().timestamp_millis());
    let mut current_authority = String::new();

    for (index, link) in chain.iter().enumerate() {
        match link.link_type {
            AuthLinkType::SIGNER => {
                current_authority = link.payload.clone();
                debug!(address = %current_authority, "SIGNER link: set initial authority");
            }

            AuthLinkType::EcdsaSignedEntity => {
                let signature = require_signature(link, index)?;
                let recovered = recover_address(link.payload.as_bytes(), &signature)?;

                check_address_match(&current_authority, &recovered, index)?;

                debug!(
                    payload = %link.payload,
                    signer = %recovered,
                    "ECDSA_SIGNED_ENTITY: verified"
                );
                current_authority = link.payload.clone();
            }

            AuthLinkType::EcdsaEphemeral => {
                let signature = require_signature(link, index)?;
                let (message, ephemeral_address, expiration_ms) =
                    parse_ephemeral_payload(&link.payload)?;

                if expiration_ms <= now_ms {
                    return Err(AuthError::EphemeralExpired {
                        expiration_ms,
                        now_ms,
                    });
                }

                let recovered = recover_address(message.as_bytes(), &signature)?;

                check_address_match(&current_authority, &recovered, index)?;

                debug!(
                    ephemeral = %ephemeral_address,
                    signer = %recovered,
                    expiration_ms,
                    "ECDSA_EPHEMERAL: verified, advancing authority to ephemeral address"
                );
                current_authority = ephemeral_address;
            }

            AuthLinkType::EcdsaEip1654Ephemeral => {
                let validator = eip1654_validator.ok_or(AuthError::Eip1654NotImplemented)?;
                let signature = require_signature(link, index)?;
                let (message, ephemeral_address, expiration_ms) =
                    parse_ephemeral_payload(&link.payload)?;

                if expiration_ms <= now_ms {
                    return Err(AuthError::EphemeralExpired {
                        expiration_ms,
                        now_ms,
                    });
                }

                let sig_bytes = decode_hex_signature(&signature)?;

                let valid = verify_eip1654(
                    validator,
                    &current_authority,
                    message.as_bytes(),
                    &sig_bytes,
                )
                .await?;

                if !valid {
                    return Err(AuthError::Eip1654Rejected {
                        contract: current_authority.clone(),
                    });
                }

                debug!(
                    ephemeral = %ephemeral_address,
                    contract = %current_authority,
                    expiration_ms,
                    "ECDSA_EIP_1654_EPHEMERAL: contract validated, advancing authority"
                );
                current_authority = ephemeral_address;
            }

            AuthLinkType::EcdsaEip1654SignedEntity => {
                let validator = eip1654_validator.ok_or(AuthError::Eip1654NotImplemented)?;
                let signature = require_signature(link, index)?;

                let sig_bytes = decode_hex_signature(&signature)?;

                let valid = verify_eip1654(
                    validator,
                    &current_authority,
                    link.payload.as_bytes(),
                    &sig_bytes,
                )
                .await?;

                if !valid {
                    return Err(AuthError::Eip1654Rejected {
                        contract: current_authority.clone(),
                    });
                }

                debug!(
                    payload = %link.payload,
                    contract = %current_authority,
                    "ECDSA_EIP_1654_SIGNED_ENTITY: contract validated"
                );
                current_authority = link.payload.clone();
            }
        }
    }

    if current_authority != expected_address {
        return Err(AuthError::FinalAuthorityMismatch {
            expected: expected_address.to_string(),
            actual: current_authority,
        });
    }

    Ok(())
}

fn verify_chain_inner(
    chain: &AuthChain,
    expected_address: &str,
    now_ms: Option<i64>,
) -> Result<(), AuthError> {
    if chain.len() > MAX_AUTH_CHAIN_LINKS {
        return Err(AuthError::MalformedChain(format!(
            "auth chain too long: {} links (max {})",
            chain.len(),
            MAX_AUTH_CHAIN_LINKS
        )));
    }
    if !is_valid_auth_chain(chain) {
        return Err(AuthError::MalformedChain("invalid chain structure".into()));
    }

    let now_ms = now_ms.unwrap_or_else(|| chrono::Utc::now().timestamp_millis());
    let mut current_authority = String::new();

    for (index, link) in chain.iter().enumerate() {
        match link.link_type {
            AuthLinkType::SIGNER => {
                current_authority = link.payload.clone();
                debug!(address = %current_authority, "SIGNER link: set initial authority");
            }

            AuthLinkType::EcdsaSignedEntity => {
                let signature = require_signature(link, index)?;
                let recovered = recover_address(link.payload.as_bytes(), &signature)?;

                check_address_match(&current_authority, &recovered, index)?;

                debug!(
                    payload = %link.payload,
                    signer = %recovered,
                    "ECDSA_SIGNED_ENTITY: verified"
                );
                current_authority = link.payload.clone();
            }

            AuthLinkType::EcdsaEphemeral => {
                let signature = require_signature(link, index)?;
                let (message, ephemeral_address, expiration_ms) =
                    parse_ephemeral_payload(&link.payload)?;

                if expiration_ms <= now_ms {
                    return Err(AuthError::EphemeralExpired {
                        expiration_ms,
                        now_ms,
                    });
                }

                let recovered = recover_address(message.as_bytes(), &signature)?;

                check_address_match(&current_authority, &recovered, index)?;

                debug!(
                    ephemeral = %ephemeral_address,
                    signer = %recovered,
                    expiration_ms,
                    "ECDSA_EPHEMERAL: verified, advancing authority to ephemeral address"
                );
                current_authority = ephemeral_address;
            }

            AuthLinkType::EcdsaEip1654Ephemeral | AuthLinkType::EcdsaEip1654SignedEntity => {
                return Err(AuthError::Eip1654NotImplemented);
            }
        }
    }

    if current_authority != expected_address {
        return Err(AuthError::FinalAuthorityMismatch {
            expected: expected_address.to_string(),
            actual: current_authority,
        });
    }

    Ok(())
}

fn decode_hex_signature(hex_str: &str) -> Result<Vec<u8>, AuthError> {
    let hex = hex_str.strip_prefix("0x").unwrap_or(hex_str);
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    if !hex.len().is_multiple_of(2) {
        return Err(AuthError::RecoveryFailed("Odd-length signature hex".into()));
    }
    for i in (0..hex.len()).step_by(2) {
        let byte = u8::from_str_radix(&hex[i..i + 2], 16)
            .map_err(|e| AuthError::RecoveryFailed(format!("Hex decode: {}", e)))?;
        bytes.push(byte);
    }
    Ok(bytes)
}

fn require_signature(link: &crate::AuthLink, index: usize) -> Result<String, AuthError> {
    match &link.signature {
        Some(sig) if !sig.is_empty() => Ok(sig.clone()),
        _ => Err(AuthError::MissingSignature {
            link_type: link.link_type.to_string(),
            index,
        }),
    }
}

fn check_address_match(expected: &str, actual: &str, index: usize) -> Result<(), AuthError> {
    if expected.to_lowercase() != actual.to_lowercase() {
        return Err(AuthError::SignerMismatch {
            index,
            expected: expected.to_lowercase(),
            actual: actual.to_lowercase(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth_chain::{AuthLink, AuthLinkType};

    #[test]
    fn test_simple_auth_chain_roundtrip() {
        let expected_address = "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266";

        let chain = vec![AuthLink {
            link_type: AuthLinkType::SIGNER,
            payload: expected_address.to_string(),
            signature: None,
        }];

        let result = verify_auth_chain(&chain, expected_address, Some(0));
        assert!(
            result.is_ok(),
            "Single SIGNER chain should verify: {:?}",
            result
        );
    }

    #[test]
    fn test_empty_chain_rejected() {
        let result = verify_auth_chain(&vec![], "0x123", Some(0));
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AuthError::MalformedChain(_)));
    }

    #[test]
    fn test_missing_signature_rejected() {
        let chain = vec![
            AuthLink {
                link_type: AuthLinkType::SIGNER,
                payload: "0xabc".into(),
                signature: None,
            },
            AuthLink {
                link_type: AuthLinkType::EcdsaSignedEntity,
                payload: "hash".into(),
                signature: None,
            },
        ];
        let result = verify_auth_chain(&chain, "hash", Some(0));
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AuthError::MissingSignature { .. }
        ));
    }

    #[test]
    fn test_eip1654_returns_not_implemented() {
        let chain = vec![
            AuthLink {
                link_type: AuthLinkType::SIGNER,
                payload: "0xabc".into(),
                signature: None,
            },
            AuthLink {
                link_type: AuthLinkType::EcdsaEip1654Ephemeral,
                payload: "Decentraland Login\nEphemeral address: 0x123\nExpiration: 2099-01-01T00:00:00.000Z".into(),
                signature: Some("0xdeadbeef".into()),
            },
        ];
        let result = verify_auth_chain(&chain, "0x123", Some(0));
        assert!(matches!(
            result.unwrap_err(),
            AuthError::Eip1654NotImplemented
        ));
    }

    #[test]
    fn test_final_authority_mismatch() {
        let chain = vec![AuthLink {
            link_type: AuthLinkType::SIGNER,
            payload: "0xabc".into(),
            signature: None,
        }];
        let result = verify_auth_chain(&chain, "0xdifferent", Some(0));
        assert!(matches!(
            result.unwrap_err(),
            AuthError::FinalAuthorityMismatch { .. }
        ));
    }

    #[test]
    fn test_final_authority_case_sensitive() {
        let chain = vec![AuthLink {
            link_type: AuthLinkType::SIGNER,
            payload: "0xABC".into(),
            signature: None,
        }];
        let result = verify_auth_chain(&chain, "0xabc", Some(0));
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AuthError::FinalAuthorityMismatch { .. }
        ));

        let result = verify_auth_chain(&chain, "0xABC", Some(0));
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_real_ecdsa_auth_chain() {
        use ethers_signers::{LocalWallet, Signer};

        let root_key: LocalWallet =
            "ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
                .parse()
                .unwrap();
        let root_address = format!("{:#x}", root_key.address());

        let ephemeral_key: LocalWallet =
            "59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d"
                .parse()
                .unwrap();
        let ephemeral_address = format!("{:#x}", ephemeral_key.address());

        let ephemeral_payload = format!(
            "Decentraland Login\nEphemeral address: {}\nExpiration: 2099-01-01T00:00:00.000Z",
            ephemeral_address
        );

        let ephemeral_sig = root_key
            .sign_message(ephemeral_payload.as_bytes())
            .await
            .unwrap();
        let ephemeral_sig_hex = format!("0x{}", ephemeral_sig);

        let entity_payload = "bafybeigdyrzt5sfp7udm7hu76uh7y26nf3efuylqabf3oclgtqy55fbzdi";

        let entity_sig = ephemeral_key
            .sign_message(entity_payload.as_bytes())
            .await
            .unwrap();
        let entity_sig_hex = format!("0x{}", entity_sig);

        let chain = vec![
            AuthLink {
                link_type: AuthLinkType::SIGNER,
                payload: root_address.clone(),
                signature: None,
            },
            AuthLink {
                link_type: AuthLinkType::EcdsaEphemeral,
                payload: ephemeral_payload,
                signature: Some(ephemeral_sig_hex),
            },
            AuthLink {
                link_type: AuthLinkType::EcdsaSignedEntity,
                payload: entity_payload.to_string(),
                signature: Some(entity_sig_hex),
            },
        ];

        let result = verify_auth_chain(&chain, entity_payload, Some(0));
        assert!(
            result.is_ok(),
            "Real ECDSA auth chain should verify: {:?}",
            result
        );

        let bad_result = verify_auth_chain(&chain, "wrong-payload", Some(0));
        assert!(bad_result.is_err());
        assert!(matches!(
            bad_result.unwrap_err(),
            AuthError::FinalAuthorityMismatch { .. }
        ));
    }
}
