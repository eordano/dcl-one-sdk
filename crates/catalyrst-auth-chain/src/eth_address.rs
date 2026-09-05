pub type EthAddress = String;

pub fn is_eth_address(value: &str) -> bool {
    value.len() == 42
        && value.starts_with("0x")
        && value[2..].bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_eth_address_accepts_lowercase_and_mixed_case() {
        assert!(is_eth_address("0x0000000000000000000000000000000000000000"));
        assert!(is_eth_address("0xabcdefABCDEF0123456789abcdefABCDEF012345"));
    }

    #[test]
    fn is_eth_address_rejects_bad_inputs() {
        assert!(!is_eth_address("0x0"));
        assert!(!is_eth_address(
            "00000000000000000000000000000000000000000000"
        ));
        assert!(!is_eth_address(
            "0xZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ"
        ));
        assert!(!is_eth_address("0x000000000000000000000000000000000000000"));
        assert!(!is_eth_address(""));
    }
}
