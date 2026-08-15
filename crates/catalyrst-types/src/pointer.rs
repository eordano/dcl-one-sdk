pub fn parse_pointer(s: &str) -> Option<(i64, i64)> {
    let (x, y) = s.split_once(',')?;
    Some((parse_axis(x)?, parse_axis(y)?))
}

fn parse_axis(part: &str) -> Option<i64> {
    let t = part.trim();
    let digits = t.strip_prefix('-').unwrap_or(t);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    t.parse::<i64>().ok()
}

pub fn canonicalize_pointer(s: &str) -> String {
    match parse_pointer(s) {
        Some((x, y)) => format!("{x},{y}"),
        None => s.to_string(),
    }
}

pub fn is_canonical_pointer(s: &str) -> bool {
    match s.split_once(',') {
        Some((x, y)) => is_canonical_axis(x) && is_canonical_axis(y),
        None => false,
    }
}

fn is_canonical_axis(value: &str) -> bool {
    let digits = value.strip_prefix('-').unwrap_or(value);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    if digits.len() > 1 && digits.starts_with('0') {
        return false;
    }
    !(value.starts_with('-') && digits == "0")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_pointer_accepts_signed_trimmed_coords() {
        assert_eq!(parse_pointer("52,-52"), Some((52, -52)));
        assert_eq!(parse_pointer(" -10 , 20 "), Some((-10, 20)));
        assert_eq!(parse_pointer("01,002"), Some((1, 2)));
        assert_eq!(parse_pointer("-0,-0"), Some((0, 0)));
    }

    #[test]
    fn parse_pointer_rejects_garbage() {
        for bad in ["52", "a,b", "1,2,3", "", "1e2,3", "+1,2", "12.5,3", "-,2"] {
            assert_eq!(parse_pointer(bad), None, "{bad} should not parse");
        }
    }

    #[test]
    fn canonicalize_pointer_numerically_normalizes() {
        assert_eq!(canonicalize_pointer("00,00"), "0,0");
        assert_eq!(canonicalize_pointer("-0,-0"), "0,0");
        assert_eq!(canonicalize_pointer(" 01 , 002 "), "1,2");
        assert_eq!(canonicalize_pointer("-05,10"), "-5,10");
        assert_eq!(canonicalize_pointer("not-a-parcel"), "not-a-parcel");
        assert_eq!(canonicalize_pointer("1,2,3"), "1,2,3");
    }

    #[test]
    fn is_canonical_pointer_accepts_only_normalized_forms() {
        for good in ["0,0", "1,5", "-1,5", "-150,150"] {
            assert!(is_canonical_pointer(good), "{good} should be canonical");
        }
        for bad in ["01,2", "-0,0", "0, 1", "1.5,2", "0", "a,b", "1,,2", ""] {
            assert!(!is_canonical_pointer(bad), "{bad} should not be canonical");
        }
    }
}
