use crate::hash::{hash_bytes, hash_bytes_v1};

pub fn is_canonical_cid(s: &str) -> bool {
    if s.is_empty() || s.len() > 100 {
        return false;
    }

    let cidv0 = s.len() == 46
        && s.starts_with("Qm")
        && s[2..].chars().all(|c| {
            matches!(c,
                '1'..='9' | 'A'..='H' | 'J'..='N' | 'P'..='Z' | 'a'..='k' | 'm'..='z')
        });

    let cidv1 = s.starts_with("ba")
        && s.len() >= 58
        && s[2..].chars().all(|c| matches!(c, 'a'..='z' | '2'..='7'));
    cidv0 || cidv1
}

pub fn verify_hash(data: &[u8], expected: &str) -> bool {
    if !is_canonical_cid(expected) {
        return false;
    }
    if expected.starts_with("Qm") {
        hash_bytes(data) == expected
    } else if expected.starts_with("ba") {
        hash_bytes_v1(data) == expected
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_cidv0_match() {
        assert!(verify_hash(
            b"hello world",
            "QmaozNR7DZHQK1ZcU9p7QdrshMvXqWK6gpu5rmrkPdT3L4"
        ));
    }

    #[test]
    fn verify_cidv0_mismatch() {
        assert!(!verify_hash(
            b"hello world",
            "QmaozNR7DZHQK1ZcU9p7QdrshMvXqWK6gpu5rmrkPdAAAA"
        ));
    }

    #[test]
    fn verify_cidv1_match() {
        assert!(verify_hash(
            b"hello world",
            "bafkreifzjut3te2nhyekklss27nh3k72ysco7y32koao5eei66wof36n5e"
        ));
    }

    #[test]
    fn verify_cidv1_mismatch() {
        assert!(!verify_hash(
            b"hello world",
            "bafkreifzjut3te2nhyekklss27nh3k72ysco7y32koao5eei66wof36AAA"
        ));
    }

    #[test]
    fn verify_unknown_prefix() {
        assert!(!verify_hash(b"hello", "z2foobar"));
    }

    #[test]
    fn verify_rejects_short_qm_prefix() {
        assert!(!verify_hash(b"hello", "Qmshort"));
    }

    #[test]
    fn verify_rejects_short_ba_prefix() {
        assert!(!verify_hash(b"hello", "bashort"));
    }

    #[test]
    fn verify_rejects_path_chars() {
        assert!(!verify_hash(b"hello", "Qm/../etc/passwd"));
        assert!(!verify_hash(b"hello", "ba/../etc/passwd"));
    }

    #[test]
    fn verify_rejects_nul_byte() {
        assert!(!verify_hash(b"hello", "Qm\0evil"));
    }
}
