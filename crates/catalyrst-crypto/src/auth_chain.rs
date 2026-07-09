pub use catalyrst_types::{AuthChain, AuthLink, AuthLinkType, MAX_AUTH_CHAIN_LINKS};

pub const MAX_AUTH_LINK_FIELD_LEN: usize = 100_000;

pub fn is_valid_auth_chain(chain: &AuthChain) -> bool {
    if chain.is_empty() {
        return false;
    }
    if chain.len() > MAX_AUTH_CHAIN_LINKS {
        return false;
    }
    for (i, link) in chain.iter().enumerate() {
        if i == 0 && link.link_type != AuthLinkType::SIGNER {
            return false;
        }
        if link.link_type == AuthLinkType::SIGNER && i != 0 {
            return false;
        }
        if link.payload.len() > MAX_AUTH_LINK_FIELD_LEN {
            return false;
        }
        if link
            .signature
            .as_ref()
            .is_some_and(|s| s.len() > MAX_AUTH_LINK_FIELD_LEN)
        {
            return false;
        }
    }
    true
}

fn is_valid_eth_address(addr: &str) -> bool {
    addr.len() == 42 && addr.starts_with("0x") && addr[2..].chars().all(|c| c.is_ascii_hexdigit())
}

pub fn parse_ephemeral_payload(payload: &str) -> Result<(String, String, i64), crate::AuthError> {
    let message = payload.replace('\r', "");
    let parts: Vec<&str> = message.split('\n').collect();

    let ephemeral_prefix = "Ephemeral address: ";
    let expiration_prefix = "Expiration: ";

    if parts.len() < 3
        || !parts[1].starts_with(ephemeral_prefix)
        || !parts[2].starts_with(expiration_prefix)
    {
        return Err(crate::AuthError::InvalidEphemeralPayload(
            "Expected 3 lines with 'Ephemeral address: ' on line 2 and 'Expiration: ' on line 3"
                .into(),
        ));
    }

    let ephemeral_address = parts[1][ephemeral_prefix.len()..].to_string();

    if !is_valid_eth_address(&ephemeral_address) {
        return Err(crate::AuthError::InvalidEphemeralPayload(
            "invalid ephemeral address format".into(),
        ));
    }

    let expiration_str = &parts[2][expiration_prefix.len()..];

    let expiration = parse_expiration(expiration_str)?;

    Ok((message, ephemeral_address, expiration))
}

fn parse_expiration(s: &str) -> Result<i64, crate::AuthError> {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Ok(dt.timestamp_millis());
    }
    for fmt in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%dT%H:%M:%S"] {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, fmt) {
            return Ok(naive.and_utc().timestamp_millis());
        }
    }
    Err(crate::AuthError::InvalidEphemeralPayload(format!(
        "Invalid expiration date '{}'",
        s
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_link_type_serde_roundtrip() {
        let json = r#""ECDSA_EPHEMERAL""#;
        let parsed: AuthLinkType = serde_json::from_str(json).unwrap();
        assert_eq!(parsed, AuthLinkType::EcdsaEphemeral);
        assert_eq!(serde_json::to_string(&parsed).unwrap(), json);
    }

    #[test]
    fn test_auth_link_type_signed_entity_serde() {
        let json = r#""ECDSA_SIGNED_ENTITY""#;
        let parsed: AuthLinkType = serde_json::from_str(json).unwrap();
        assert_eq!(parsed, AuthLinkType::EcdsaSignedEntity);
    }

    #[test]
    fn test_is_valid_auth_chain_ok() {
        let chain = vec![
            AuthLink {
                link_type: AuthLinkType::SIGNER,
                payload: "0xabc".into(),
                signature: None,
            },
            AuthLink {
                link_type: AuthLinkType::EcdsaSignedEntity,
                payload: "hash".into(),
                signature: Some("0xsig".into()),
            },
        ];
        assert!(is_valid_auth_chain(&chain));
    }

    #[test]
    fn test_is_valid_auth_chain_empty() {
        assert!(!is_valid_auth_chain(&vec![]));
    }

    #[test]
    fn test_is_valid_auth_chain_no_signer_first() {
        let chain = vec![AuthLink {
            link_type: AuthLinkType::EcdsaSignedEntity,
            payload: "hash".into(),
            signature: Some("0xsig".into()),
        }];
        assert!(!is_valid_auth_chain(&chain));
    }

    #[test]
    fn test_is_valid_auth_chain_duplicate_signer() {
        let chain = vec![
            AuthLink {
                link_type: AuthLinkType::SIGNER,
                payload: "0xabc".into(),
                signature: None,
            },
            AuthLink {
                link_type: AuthLinkType::SIGNER,
                payload: "0xdef".into(),
                signature: None,
            },
        ];
        assert!(!is_valid_auth_chain(&chain));
    }

    #[test]
    fn test_parse_ephemeral_payload() {
        let payload =
            "Decentraland Login\nEphemeral address: 0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266\nExpiration: 2025-01-20T22:57:11.334Z";
        let (msg, addr, exp) = parse_ephemeral_payload(payload).unwrap();
        assert_eq!(msg, payload);
        assert_eq!(addr, "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266");
        assert!(exp > 0);
    }

    #[test]
    fn test_parse_ephemeral_payload_unity_editor_format() {
        let payload = "Decentraland Login\nEphemeral address: 0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266\nExpiration: 2036-06-10T16:07:42";
        let (_, _, exp) = parse_ephemeral_payload(payload).unwrap();
        assert!(exp > 2_000_000_000_000);
    }

    #[test]
    fn test_parse_ephemeral_payload_strips_cr() {
        let payload = "Decentraland Login\r\nEphemeral address: 0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266\r\nExpiration: 2025-01-20T22:57:11.334Z";
        let (msg, addr, _) = parse_ephemeral_payload(payload).unwrap();
        assert_eq!(addr, "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266");
        assert!(!msg.contains('\r'));
    }

    #[test]
    fn test_parse_ephemeral_payload_bad_format() {
        assert!(parse_ephemeral_payload("garbage").is_err());
    }

    #[test]
    fn test_is_valid_eth_address() {
        assert!(is_valid_eth_address(
            "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
        ));
        assert!(is_valid_eth_address(
            "0x0000000000000000000000000000000000000000"
        ));
        assert!(!is_valid_eth_address(
            "f39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
        ));
        assert!(!is_valid_eth_address("0x1234"));
        assert!(!is_valid_eth_address(
            "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb922660"
        ));
        assert!(!is_valid_eth_address(
            "0xZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ"
        ));
    }

    #[test]
    fn test_parse_ephemeral_payload_rejects_malformed_address() {
        let payload =
            "Decentraland Login\nEphemeral address: not-an-address\nExpiration: 2025-01-20T22:57:11.334Z";
        let result = parse_ephemeral_payload(payload);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            format!("{}", err).contains("invalid ephemeral address format"),
            "Expected 'invalid ephemeral address format', got: {}",
            err
        );
    }

    #[test]
    fn test_auth_chain_deserialization() {
        let json = r#"[
            {"type": "SIGNER", "payload": "0xabc"},
            {"type": "ECDSA_EPHEMERAL", "payload": "Decentraland Login\nEphemeral address: 0x123\nExpiration: 2025-01-01T00:00:00.000Z", "signature": "0xdeadbeef"},
            {"type": "ECDSA_SIGNED_ENTITY", "payload": "bafyhash", "signature": "0xcafebabe"}
        ]"#;
        let chain: AuthChain = serde_json::from_str(json).unwrap();
        assert_eq!(chain.len(), 3);
        assert_eq!(chain[0].link_type, AuthLinkType::SIGNER);
        assert_eq!(chain[1].link_type, AuthLinkType::EcdsaEphemeral);
        assert_eq!(chain[2].link_type, AuthLinkType::EcdsaSignedEntity);
    }

    #[test]
    fn test_is_valid_auth_chain_rejects_oversized_payload() {
        let over = AuthLink {
            link_type: AuthLinkType::SIGNER,
            payload: "a".repeat(MAX_AUTH_LINK_FIELD_LEN + 1),
            signature: None,
        };
        assert!(!is_valid_auth_chain(&vec![over]));

        let at_limit = AuthLink {
            link_type: AuthLinkType::SIGNER,
            payload: "a".repeat(MAX_AUTH_LINK_FIELD_LEN),
            signature: None,
        };
        assert!(is_valid_auth_chain(&vec![at_limit]));
    }

    #[test]
    fn test_is_valid_auth_chain_rejects_oversized_signature() {
        let make = |sig_len: usize| {
            vec![
                AuthLink {
                    link_type: AuthLinkType::SIGNER,
                    payload: "0xabc".into(),
                    signature: None,
                },
                AuthLink {
                    link_type: AuthLinkType::EcdsaSignedEntity,
                    payload: "hash".into(),
                    signature: Some("a".repeat(sig_len)),
                },
            ]
        };
        assert!(!is_valid_auth_chain(&make(MAX_AUTH_LINK_FIELD_LEN + 1)));
        assert!(is_valid_auth_chain(&make(MAX_AUTH_LINK_FIELD_LEN)));
    }

    #[test]
    fn test_is_valid_auth_chain_count_bound_still_holds() {
        let well_formed = |n: usize| {
            let mut chain = vec![AuthLink {
                link_type: AuthLinkType::SIGNER,
                payload: "0xabc".into(),
                signature: None,
            }];
            for _ in 1..n {
                chain.push(AuthLink {
                    link_type: AuthLinkType::EcdsaSignedEntity,
                    payload: "hash".into(),
                    signature: Some("0xsig".into()),
                });
            }
            chain
        };
        assert!(is_valid_auth_chain(&well_formed(MAX_AUTH_CHAIN_LINKS)));
        assert!(!is_valid_auth_chain(&well_formed(MAX_AUTH_CHAIN_LINKS + 1)));
    }
}
