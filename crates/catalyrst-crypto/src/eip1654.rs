use async_trait::async_trait;

use crate::error::AuthError;

#[async_trait]
pub trait Eip1654Validator: Send + Sync {
    async fn validate_signature(
        &self,
        contract_address: &str,
        hash: &[u8],
        signature: &[u8],
    ) -> Result<bool, AuthError>;
}

pub async fn verify_eip1654(
    validator: &dyn Eip1654Validator,
    contract_address: &str,
    message: &[u8],
    signature: &[u8],
) -> Result<bool, AuthError> {
    let raw_hash = alloy_primitives::keccak256(message);
    if validator
        .validate_signature(contract_address, raw_hash.as_slice(), signature)
        .await?
    {
        return Ok(true);
    }

    let prefixed_hash = alloy_primitives::eip191_hash_message(message);
    validator
        .validate_signature(contract_address, prefixed_hash.as_slice(), signature)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct HashMatchValidator {
        expected_hash: [u8; 32],
        calls: AtomicUsize,
    }

    #[async_trait]
    impl Eip1654Validator for HashMatchValidator {
        async fn validate_signature(
            &self,
            _contract_address: &str,
            hash: &[u8],
            _signature: &[u8],
        ) -> Result<bool, AuthError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(hash == self.expected_hash.as_slice())
        }
    }

    #[tokio::test]
    async fn accepts_raw_keccak_hash_without_trying_prefixed() {
        let message = b"hello world";
        let raw = alloy_primitives::keccak256(message).0;
        let v = HashMatchValidator {
            expected_hash: raw,
            calls: AtomicUsize::new(0),
        };
        let ok = verify_eip1654(&v, "0xcontract", message, &[0u8; 65])
            .await
            .unwrap();
        assert!(ok);
        assert_eq!(v.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn falls_back_to_eip191_prefixed_hash() {
        let message = b"hello world";
        let prefixed: [u8; 32] = alloy_primitives::eip191_hash_message(message).0;
        let v = HashMatchValidator {
            expected_hash: prefixed,
            calls: AtomicUsize::new(0),
        };
        let ok = verify_eip1654(&v, "0xcontract", message, &[0u8; 65])
            .await
            .unwrap();
        assert!(ok);
        assert_eq!(v.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn rejects_when_neither_hash_matches() {
        let v = HashMatchValidator {
            expected_hash: [0xEE; 32],
            calls: AtomicUsize::new(0),
        };
        let ok = verify_eip1654(&v, "0xcontract", b"msg", &[0u8; 65])
            .await
            .unwrap();
        assert!(!ok);
        assert_eq!(v.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn propagates_rpc_error() {
        struct Failing;
        #[async_trait]
        impl Eip1654Validator for Failing {
            async fn validate_signature(
                &self,
                _c: &str,
                _h: &[u8],
                _s: &[u8],
            ) -> Result<bool, AuthError> {
                Err(AuthError::Eip1654ValidationFailed("boom".into()))
            }
        }
        let err = verify_eip1654(&Failing, "0xcontract", b"msg", &[0u8; 65])
            .await
            .unwrap_err();
        assert!(matches!(err, AuthError::Eip1654ValidationFailed(_)));
    }
}
