use serde::Serialize;

#[derive(Debug, Clone, Copy)]
pub struct PageInput {
    pub limit: i64,
    pub offset: i64,
}

pub fn clamp_limit(requested: Option<i64>, default: i64, max: i64) -> i64 {
    requested.unwrap_or(default).clamp(1, max)
}

pub fn limit_or_max(requested: Option<i64>, max: i64) -> i64 {
    match requested {
        Some(n) if n > 0 && n <= max => n,
        _ => max,
    }
}

pub fn get_pagination_params(pairs: &[(String, String)], max_limit: i64) -> PageInput {
    let mut limit_raw: Option<&str> = None;
    let mut offset_raw: Option<&str> = None;
    let mut page_raw: Option<&str> = None;

    for (k, v) in pairs {
        match k.as_str() {
            "limit" if limit_raw.is_none() => limit_raw = Some(v),
            "offset" if offset_raw.is_none() => offset_raw = Some(v),
            "page" if page_raw.is_none() => page_raw = Some(v),
            _ => {}
        }
    }

    let limit = limit_or_max(limit_raw.and_then(|s| s.parse().ok()), max_limit);

    let offset = match offset_raw.and_then(|s| s.parse::<i64>().ok()) {
        Some(n) if n >= 0 => n,
        _ => match page_raw.and_then(|s| s.parse::<i64>().ok()) {
            Some(p) if p >= 0 => p.saturating_mul(limit),
            _ => 0,
        },
    };

    PageInput { limit, offset }
}

#[derive(Debug, Serialize)]
pub struct PaginatedResponse<T> {
    pub results: Vec<T>,
    pub total: i64,
    pub page: i64,
    pub pages: i64,
    pub limit: i64,
}

impl<T> PaginatedResponse<T> {
    pub fn new(results: Vec<T>, total: i64, limit: i64, offset: i64) -> Self {
        let page = if limit > 0 { offset / limit } else { 0 };
        let pages = if limit > 0 {
            (total + limit - 1) / limit
        } else {
            0
        };
        Self {
            results,
            total,
            page,
            pages,
            limit,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_limit_applies_default_and_bounds() {
        assert_eq!(clamp_limit(None, 500, 5000), 500);
        assert_eq!(clamp_limit(Some(0), 500, 5000), 1);
        assert_eq!(clamp_limit(Some(-9), 500, 5000), 1);
        assert_eq!(clamp_limit(Some(42), 500, 5000), 42);
        assert_eq!(clamp_limit(Some(1_000_000), 500, 5000), 5000);
    }

    #[test]
    fn pagination_params_honor_limit_offset_page() {
        let pairs = |s: &[(&str, &str)]| -> Vec<(String, String)> {
            s.iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        let p = get_pagination_params(&pairs(&[("limit", "24"), ("offset", "48")]), 100);
        assert_eq!((p.limit, p.offset), (24, 48));
        let p = get_pagination_params(&pairs(&[("limit", "24"), ("page", "2")]), 100);
        assert_eq!((p.limit, p.offset), (24, 48));
        let p = get_pagination_params(&pairs(&[("offset", "-5")]), 100);
        assert_eq!((p.limit, p.offset), (100, 0));
        let p = get_pagination_params(&pairs(&[]), 100);
        assert_eq!((p.limit, p.offset), (100, 0));
    }

    #[test]
    fn limit_or_max_falls_back_to_max() {
        assert_eq!(limit_or_max(None, 100), 100);
        assert_eq!(limit_or_max(Some(0), 100), 100);
        assert_eq!(limit_or_max(Some(-5), 100), 100);
        assert_eq!(limit_or_max(Some(60), 100), 60);
        assert_eq!(limit_or_max(Some(500), 100), 100);
    }
}
