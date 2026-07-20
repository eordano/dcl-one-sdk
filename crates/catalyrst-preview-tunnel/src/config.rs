use anyhow::Result;
use catalyrst_envcfg::get_port;
use std::env;

#[derive(Clone, Debug)]
pub struct Config {
    pub http_host: String,
    pub http_port: u16,
    pub public_base_url: Option<String>,
    pub tokens: Vec<String>,
    pub allow_ids: Vec<String>,
    pub grace_secs: u64,
    pub ping_secs: u64,
    pub open_timeout_secs: u64,
    pub body_max_bytes: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            http_host: "127.0.0.1".into(),
            http_port: 5167,
            public_base_url: None,
            tokens: Vec::new(),
            allow_ids: Vec::new(),
            grace_secs: 120,
            ping_secs: 20,
            open_timeout_secs: 15,
            body_max_bytes: 64 * 1024 * 1024,
        }
    }
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let defaults = Config::default();
        let opt = |k: &str| env::var(k).ok().filter(|s| !s.is_empty());
        let http_port = get_port("HTTP_SERVER_PORT", defaults.http_port)?;
        Ok(Config {
            http_host: opt("HTTP_SERVER_HOST").unwrap_or(defaults.http_host),
            http_port,
            public_base_url: opt("PUBLIC_BASE_URL").map(|s| s.trim_end_matches('/').to_string()),
            tokens: parse_list(opt("TUNNEL_TOKENS")),
            allow_ids: parse_list(opt("TUNNEL_ALLOW_IDS")),
            grace_secs: parse_or(opt("TUNNEL_GRACE_SECS"), defaults.grace_secs),
            ping_secs: parse_or(opt("TUNNEL_PING_SECS"), defaults.ping_secs).max(1),
            open_timeout_secs: parse_or(
                opt("TUNNEL_OPEN_TIMEOUT_SECS"),
                defaults.open_timeout_secs,
            )
            .max(1),
            body_max_bytes: parse_or(opt("TUNNEL_BODY_MAX_BYTES"), defaults.body_max_bytes),
        })
    }

    pub fn public_base(&self) -> String {
        self.public_base_url
            .clone()
            .unwrap_or_else(|| format!("http://{}:{}", self.http_host, self.http_port))
    }
}

fn parse_list(raw: Option<String>) -> Vec<String> {
    raw.map(|s| {
        s.split(',')
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_string)
            .collect()
    })
    .unwrap_or_default()
}

fn parse_or<T: std::str::FromStr>(raw: Option<String>, default: T) -> T {
    raw.and_then(|s| s.parse().ok()).unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_public_base_is_the_bind_address() {
        let cfg = Config::default();
        assert_eq!(cfg.public_base(), "http://127.0.0.1:5167");
        let cfg = Config {
            public_base_url: Some("https://tunnel.example".into()),
            ..Config::default()
        };
        assert_eq!(cfg.public_base(), "https://tunnel.example");
    }

    #[test]
    fn parse_list_splits_and_trims() {
        assert_eq!(
            parse_list(Some("a, b ,,c".into())),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
        assert!(parse_list(None).is_empty());
    }
}
