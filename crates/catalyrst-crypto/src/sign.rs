use ethers_core::k256::ecdsa::SigningKey;
use ethers_core::types::{Signature, H160, U256};
use ethers_core::utils::{hash_message, secret_key_to_address};
use serde_json::{json, Value};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SignError {
    #[error("invalid private key: {0}")]
    InvalidPrivateKey(String),

    #[error("signing failed: {0}")]
    SigningFailed(String),
}

pub struct Wallet {
    key: SigningKey,
    address: H160,
}

impl Wallet {
    pub fn from_hex(raw: &str) -> Result<Self, SignError> {
        let hexs = raw.trim().trim_start_matches("0x");
        let bytes =
            hex::decode(hexs).map_err(|e| SignError::InvalidPrivateKey(format!("not hex: {e}")))?;
        if bytes.len() != 32 {
            return Err(SignError::InvalidPrivateKey(format!(
                "expected 32 bytes (64 hex chars), got {}",
                bytes.len()
            )));
        }
        let key = SigningKey::from_slice(&bytes)
            .map_err(|e| SignError::InvalidPrivateKey(e.to_string()))?;
        let address = secret_key_to_address(&key);
        Ok(Wallet { key, address })
    }

    pub fn address(&self) -> String {
        format!("{:#x}", self.address)
    }

    pub fn sign_message(&self, message: &[u8]) -> Result<String, SignError> {
        let hash = hash_message(message);
        let (sig, recovery_id) = self
            .key
            .sign_prehash_recoverable(hash.as_ref())
            .map_err(|e| SignError::SigningFailed(e.to_string()))?;
        let signature = Signature {
            r: U256::from_big_endian(&sig.r().to_bytes()),
            s: U256::from_big_endian(&sig.s().to_bytes()),
            v: u8::from(recovery_id) as u64 + 27,
        };
        Ok(format!("0x{signature}"))
    }
}

pub fn create_simple_auth_chain(wallet: &Wallet, payload: &str) -> Result<Value, SignError> {
    let signature = wallet.sign_message(payload.as_bytes())?;
    Ok(json!([
        { "type": "SIGNER", "payload": wallet.address(), "signature": "" },
        { "type": "ECDSA_SIGNED_ENTITY", "payload": payload, "signature": signature },
    ]))
}

pub fn verify_signed_message(
    chain: &crate::AuthChain,
    message: &str,
    owner_address: &str,
    now_ms: Option<i64>,
) -> Result<(), crate::AuthError> {
    crate::verify::verify_auth_chain(chain, message, now_ms)?;
    let owner = chain
        .first()
        .filter(|link| link.link_type == crate::AuthLinkType::SIGNER)
        .map(|link| link.payload.as_str())
        .ok_or_else(|| {
            crate::AuthError::MalformedChain("auth chain must start with a SIGNER link".into())
        })?;
    if owner.to_lowercase() != owner_address.to_lowercase() {
        return Err(crate::AuthError::SignerMismatch {
            index: 0,
            expected: owner_address.to_lowercase(),
            actual: owner.to_lowercase(),
        });
    }
    if chain.len() < 2 {
        return Err(crate::AuthError::MalformedChain(
            "auth chain carries no signed link over the message".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recover::recover_address;

    const KEY: &str = "0x4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f362318";

    #[test]
    fn from_hex_accepts_prefixed_and_bare_and_rejects_garbage() {
        let w = Wallet::from_hex(KEY).unwrap();
        let bare = Wallet::from_hex(KEY.trim_start_matches("0x")).unwrap();
        assert_eq!(w.address(), bare.address());
        assert_eq!(w.address(), w.address().to_lowercase());
        assert!(w.address().starts_with("0x") && w.address().len() == 42);
        assert!(Wallet::from_hex("0x1234").is_err());
        assert!(Wallet::from_hex("zz").is_err());
        assert!(Wallet::from_hex("").is_err());
    }

    #[test]
    fn sign_message_matches_the_ethers_localwallet_vector() {
        let w = Wallet::from_hex(KEY).unwrap();
        let sig = w.sign_message(b"Some data").unwrap();
        assert_eq!(
            sig,
            "0xb91467e570a6466aa9e9876cbcd013baba02900b8979d43fe208a4a4f339f5fd6007e74cd82e037b800186422fc2da167c747ef045e5d18a5f5d4300f8e1a0291c"
        );
    }

    #[test]
    fn signatures_recover_to_the_wallet_address() {
        let w = Wallet::from_hex(KEY).unwrap();
        for message in [&b"hello"[..], b"", b"delete:/entities/x:0:{}"] {
            let sig = w.sign_message(message).unwrap();
            assert_eq!(sig.len(), 132);
            assert_eq!(recover_address(message, &sig).unwrap(), w.address());
        }
    }

    #[test]
    fn simple_auth_chain_has_the_two_link_deploy_shape() {
        let w = Wallet::from_hex(KEY).unwrap();
        let chain = create_simple_auth_chain(&w, "bafkreiabc").unwrap();
        let links = chain.as_array().unwrap();
        assert_eq!(links.len(), 2);
        assert_eq!(links[0]["type"], "SIGNER");
        assert_eq!(links[0]["payload"], json!(w.address()));
        assert_eq!(links[0]["signature"], "");
        assert_eq!(links[1]["type"], "ECDSA_SIGNED_ENTITY");
        assert_eq!(links[1]["payload"], "bafkreiabc");
        let sig = links[1]["signature"].as_str().unwrap();
        assert_eq!(recover_address(b"bafkreiabc", sig).unwrap(), w.address());
    }

    fn signed_chain(wallet: &Wallet, message: &str) -> crate::AuthChain {
        serde_json::from_value(create_simple_auth_chain(wallet, message).unwrap()).unwrap()
    }

    #[test]
    fn verify_signed_message_accepts_the_chains_own_owner() {
        let w = Wallet::from_hex(KEY).unwrap();
        let challenge = "dcl-abc123";
        let chain = signed_chain(&w, challenge);
        assert!(verify_signed_message(&chain, challenge, &w.address(), Some(0)).is_ok());
    }

    #[test]
    fn verify_signed_message_rejects_a_chain_owned_by_another_wallet() {
        let owner = Wallet::from_hex(KEY).unwrap();
        let challenge = "dcl-abc123";
        let chain = signed_chain(&owner, challenge);
        let claimed = "0x0000000000000000000000000000000000000001";
        let err = verify_signed_message(&chain, challenge, claimed, Some(0)).unwrap_err();
        assert!(matches!(
            err,
            crate::AuthError::SignerMismatch { index: 0, .. }
        ));
    }

    #[test]
    fn verify_signed_message_rejects_a_lone_signer_echo_of_the_challenge() {
        let challenge = "dcl-abc123";
        let claimed = "0x000000000000000000000000000000000000dead";
        let echo = vec![crate::AuthLink {
            link_type: crate::AuthLinkType::SIGNER,
            payload: challenge.to_string(),
            signature: None,
        }];
        let err = verify_signed_message(&echo, challenge, claimed, Some(0)).unwrap_err();
        assert!(matches!(
            err,
            crate::AuthError::SignerMismatch { index: 0, .. }
        ));
    }

    #[test]
    fn verify_signed_message_rejects_a_lone_signer_even_when_owner_matches() {
        let claimed = "0x000000000000000000000000000000000000dead";
        let lone = vec![crate::AuthLink {
            link_type: crate::AuthLinkType::SIGNER,
            payload: claimed.to_string(),
            signature: None,
        }];
        assert!(verify_signed_message(&lone, claimed, claimed, Some(0)).is_err());
    }
}
