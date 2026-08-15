#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HexDecodeError {
    #[error("odd-length hex")]
    OddLength,
    #[error("invalid hex char: {c}")]
    InvalidChar { c: char, index: usize },
}

pub fn decode_hex_0x(s: &str) -> Result<Vec<u8>, HexDecodeError> {
    hex::decode(s.strip_prefix("0x").unwrap_or(s)).map_err(|e| match e {
        hex::FromHexError::InvalidHexCharacter { c, index } => {
            HexDecodeError::InvalidChar { c, index }
        }
        hex::FromHexError::OddLength | hex::FromHexError::InvalidStringLength => {
            HexDecodeError::OddLength
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_with_and_without_prefix() {
        assert_eq!(
            decode_hex_0x("0xdeadbeef"),
            Ok(vec![0xde, 0xad, 0xbe, 0xef])
        );
        assert_eq!(decode_hex_0x("deadbeef"), Ok(vec![0xde, 0xad, 0xbe, 0xef]));
        assert_eq!(decode_hex_0x("0x"), Ok(vec![]));
        assert_eq!(decode_hex_0x(""), Ok(vec![]));
    }

    #[test]
    fn rejects_odd_length() {
        assert_eq!(decode_hex_0x("0xabc"), Err(HexDecodeError::OddLength));
    }

    #[test]
    fn rejects_invalid_chars_without_panicking() {
        assert!(matches!(
            decode_hex_0x("0xzz"),
            Err(HexDecodeError::InvalidChar { c: 'z', .. })
        ));
        assert!(matches!(
            decode_hex_0x("0x\u{00e9}\u{00e9}"),
            Err(HexDecodeError::InvalidChar { .. })
        ));
        assert!(matches!(
            decode_hex_0x("0x0c53c51c\u{30c6}\u{30b9}"),
            Err(HexDecodeError::InvalidChar { .. })
        ));
    }

    #[test]
    fn error_messages_are_stable() {
        assert_eq!(
            decode_hex_0x("abc").unwrap_err().to_string(),
            "odd-length hex"
        );
        assert_eq!(
            decode_hex_0x("zz").unwrap_err().to_string(),
            "invalid hex char: z"
        );
    }
}
