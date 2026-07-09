use ethers_core::types::{RecoveryMessage, Signature, H160, H256};

use crate::AuthError;

pub fn recover_address(message: &[u8], signature: &str) -> Result<String, AuthError> {
    let sig_bytes = parse_signature_hex(signature)?;
    reject_high_s(&sig_bytes)?;
    let sig = parse_ethers_signature(&sig_bytes)?;

    let recovered: H160 = sig
        .recover(RecoveryMessage::Data(message.to_vec()))
        .map_err(|e| AuthError::RecoveryFailed(format!("ecrecover failed: {}", e)))?;

    Ok(format!("{:#x}", recovered))
}

pub fn recover_address_from_digest(
    digest: &[u8; 32],
    signature: &str,
) -> Result<String, AuthError> {
    let sig_bytes = parse_signature_hex(signature)?;
    reject_high_s(&sig_bytes)?;
    let sig = parse_ethers_signature(&sig_bytes)?;

    let recovered: H160 = sig
        .recover(RecoveryMessage::Hash(H256::from(*digest)))
        .map_err(|e| AuthError::RecoveryFailed(format!("ecrecover failed: {}", e)))?;

    Ok(format!("{:#x}", recovered))
}

const SECP256K1_N: [u8; 32] = [
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xfe,
    0xba, 0xae, 0xdc, 0xe6, 0xaf, 0x48, 0xa0, 0x3b, 0xbf, 0xd2, 0x5e, 0x8c, 0xd0, 0x36, 0x41, 0x41,
];

const SECP256K1_HALF_N: [u8; 32] = [
    0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    0x5d, 0x57, 0x6e, 0x73, 0x57, 0xa4, 0x50, 0x1d, 0xdf, 0xe9, 0x2f, 0x46, 0x68, 0x1b, 0x20, 0xa0,
];

fn reject_high_s(bytes: &[u8; 65]) -> Result<(), AuthError> {
    let s = &bytes[32..64];
    if s.iter().all(|&b| b == 0) {
        return Err(AuthError::RecoveryFailed("signature s is zero".into()));
    }
    if cmp_be(s, &SECP256K1_N) != std::cmp::Ordering::Less {
        return Err(AuthError::RecoveryFailed(
            "signature s >= group order n".into(),
        ));
    }
    if cmp_be(s, &SECP256K1_HALF_N) == std::cmp::Ordering::Greater {
        return Err(AuthError::RecoveryFailed(
            "non-canonical high-s signature rejected (malleability)".into(),
        ));
    }
    Ok(())
}

fn cmp_be(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    a.iter().cmp(b.iter())
}

fn parse_signature_hex(hex: &str) -> Result<[u8; 65], AuthError> {
    let hex = hex.strip_prefix("0x").unwrap_or(hex);

    if hex.len() != 130 {
        return Err(AuthError::RecoveryFailed(format!(
            "Signature hex must be 130 characters (65 bytes), got {}",
            hex.len()
        )));
    }

    let bytes = hex::decode(hex)
        .map_err(|e| AuthError::RecoveryFailed(format!("Invalid hex in signature: {}", e)))?;

    let mut arr = [0u8; 65];
    arr.copy_from_slice(&bytes);
    Ok(arr)
}

fn parse_ethers_signature(bytes: &[u8; 65]) -> Result<Signature, AuthError> {
    let mut v = bytes[64];
    if v >= 27 {
        v -= 27;
    }

    let mut sig_bytes = [0u8; 65];
    sig_bytes[..64].copy_from_slice(&bytes[..64]);
    sig_bytes[64] = v;

    Signature::try_from(sig_bytes.as_slice())
        .map_err(|e| AuthError::RecoveryFailed(format!("Invalid signature bytes: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_signature_hex_strips_0x() {
        let hex_str = format!("0x{}", "00".repeat(65));
        let result = parse_signature_hex(&hex_str);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), [0u8; 65]);
    }

    #[test]
    fn test_parse_signature_hex_wrong_length() {
        assert!(parse_signature_hex("0xdeadbeef").is_err());
    }

    #[tokio::test]
    async fn test_high_s_malleated_signature_rejected() {
        use ethers_signers::{LocalWallet, Signer};

        let wallet: LocalWallet =
            "ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
                .parse()
                .unwrap();
        let addr = format!("{:#x}", wallet.address());
        let msg = b"get:/foo:1700000000000:{}";
        let sig = wallet.sign_message(&msg[..]).await.unwrap();

        let sig_hex = format!("0x{}", sig);
        let rec = recover_address(msg, &sig_hex).expect("low-s sig must verify");
        assert_eq!(rec, addr, "canonical sig recovers signer");

        let mut raw = parse_signature_hex(&sig_hex).unwrap();
        let s = ethers_core::types::U256::from_big_endian(&raw[32..64]);
        let n = ethers_core::types::U256::from_big_endian(&SECP256K1_N);
        let s2 = n - s;
        let mut s2_be = [0u8; 32];
        s2.to_big_endian(&mut s2_be);
        raw[32..64].copy_from_slice(&s2_be);
        raw[64] = if raw[64] == 27 {
            28
        } else if raw[64] == 28 {
            27
        } else {
            raw[64] ^ 1
        };
        let mall_hex = format!("0x{}", hex_encode(&raw));

        let res = recover_address(msg, &mall_hex);
        assert!(
            res.is_err(),
            "malleated high-s twin must be rejected, got {:?}",
            res
        );
    }

    fn hex_encode(b: &[u8]) -> String {
        let mut s = String::with_capacity(b.len() * 2);
        for x in b {
            s.push_str(&format!("{:02x}", x));
        }
        s
    }

    #[tokio::test]
    async fn test_recover_from_raw_digest() {
        use ethers_core::utils::keccak256;
        use ethers_signers::{LocalWallet, Signer};

        let wallet: LocalWallet =
            "ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
                .parse()
                .unwrap();
        let addr = format!("{:#x}", wallet.address());

        let digest = keccak256(b"some 32-byte typed-data digest input");
        let sig = wallet.sign_hash(H256::from(digest)).unwrap();
        let sig_hex = format!("0x{}", sig);

        let rec = recover_address_from_digest(&digest, &sig_hex).expect("digest sig must verify");
        assert_eq!(rec, addr);

        let prefixed = recover_address(&digest, &sig_hex).unwrap();
        assert_ne!(prefixed, addr);
    }

    #[test]
    fn test_v_normalization() {
        let mut bytes = [0u8; 65];
        bytes[64] = 27;
        let sig = parse_ethers_signature(&bytes);
        assert!(sig.is_err() || sig.unwrap().v == 0);
    }
}
