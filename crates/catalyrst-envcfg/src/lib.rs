use anyhow::{anyhow, Context, Result};
use std::env;

pub fn required(key: &str) -> Result<String> {
    env::var(key).map_err(|_| anyhow!("missing required env var: {}", key))
}

fn trimmed(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Endpoints have no built-in default on purpose: every value this stack once
/// defaulted to was a live production Decentraland host, so an unset variable
/// silently sent real traffic upstream. Fail closed instead.
pub fn required_endpoint(key: &str) -> Result<String> {
    trimmed(key).ok_or_else(|| {
        anyhow!(
            "missing required endpoint env var: {key} \u{2014} set it explicitly. There is \
             deliberately no default: every historical default for this variable was a \
             production Decentraland endpoint, and falling back to one would send live \
             traffic off this deployment."
        )
    })
}

/// For endpoints a service in this stack implements itself; the fallback is that
/// service's loopback port on this host, never an upstream host.
pub fn local_endpoint(key: &str, port: u16) -> String {
    trimmed(key).unwrap_or_else(|| format!("http://127.0.0.1:{port}"))
}

/// Endpoints whose feature is switched off when no upstream is configured.
/// Unset and empty both mean "disabled", never "use production".
pub fn optional_endpoint(key: &str) -> Option<String> {
    trimmed(key)
}

pub fn get_port(key: &str, default: u16) -> Result<u16> {
    match env::var(key) {
        Ok(s) => s.parse::<u16>().with_context(|| format!("invalid {}", key)),
        Err(_) => Ok(default),
    }
}

pub fn get_int(key: &str, default: i64) -> Result<i64> {
    match env::var(key) {
        Ok(s) => s.parse::<i64>().with_context(|| format!("invalid {}", key)),
        Err(_) => Ok(default),
    }
}

pub fn get_u64(key: &str, default: u64) -> Result<u64> {
    match env::var(key) {
        Ok(s) => s.parse::<u64>().with_context(|| format!("invalid {}", key)),
        Err(_) => Ok(default),
    }
}

pub fn env_bool(key: &str, default: bool) -> bool {
    let raw = match env::var(key) {
        Ok(v) => v,
        Err(_) => return default,
    };
    match raw.trim().to_ascii_lowercase().as_str() {
        "" => default,
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        other => {
            tracing::warn!(
                key,
                value = other,
                default,
                "unrecognized boolean env value; keeping default \
                 (use 1/true/yes/on or 0/false/no/off)"
            );
            default
        }
    }
}

/// Standard service-bin tracing bootstrap: `RUST_LOG` via `EnvFilter` with a
/// per-service default filter, target names off.
pub fn init_tracing(default_filter: &str) {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| default_filter.into()),
        )
        .with_target(false)
        .init();
}

/// Standard service-bin tail: parse `host:port`, log `<name> listening`,
/// bind, and serve `app` until shutdown. Bins that need extra serve behavior
/// (connect-info, graceful shutdown, TLS) keep their own tail.
pub async fn run_service(
    name: &str,
    host: impl std::fmt::Display,
    port: impl std::fmt::Display,
    app: axum::Router,
) -> anyhow::Result<()> {
    let addr: std::net::SocketAddr = format!("{}:{}", host, port).parse()?;
    tracing::info!(%addr, "{} listening", name);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

pub fn handle_standard_args(service_name: &str, env_docs: &[(&str, &str)]) {
    handle_standard_args_with_version(service_name, env!("CARGO_PKG_VERSION"), env_docs)
}

pub fn handle_standard_args_with_version(
    service_name: &str,
    version: &str,
    env_docs: &[(&str, &str)],
) {
    let Some(first) = env::args().nth(1) else {
        return;
    };
    match first.as_str() {
        "--help" | "-h" => {
            print_help(service_name, env_docs);
            std::process::exit(0);
        }
        "--version" | "-V" => {
            println!("{} {}", service_name, version);
            std::process::exit(0);
        }
        other => {
            eprintln!("{}: unexpected argument {:?}", service_name, other);
            eprintln!(
                "{} takes no arguments besides --help/--version; all configuration is via \
                 environment variables \u{2014} run `{} --help` for the full list",
                service_name, service_name
            );
            std::process::exit(2);
        }
    }
}

fn print_help(service_name: &str, env_docs: &[(&str, &str)]) {
    println!("{} \u{2014} env-configured service", service_name);
    println!();
    println!("usage: {} [--help | --version]", service_name);
    println!();
    println!("environment variables:");
    let width = env_docs.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
    for (key, doc) in env_docs {
        println!("  {:<width$}  {}", key, doc, width = width);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        env_bool, get_int, get_port, get_u64, local_endpoint, optional_endpoint, required,
        required_endpoint,
    };

    #[test]
    fn endpoints_never_default_to_production() {
        let err = required_endpoint("ENVCFG_TEST_ENDPOINT_MISSING").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("ENVCFG_TEST_ENDPOINT_MISSING"), "{msg}");
        assert!(msg.contains("no default"), "{msg}");

        std::env::set_var("ENVCFG_TEST_ENDPOINT_BLANK", "   ");
        assert!(required_endpoint("ENVCFG_TEST_ENDPOINT_BLANK").is_err());

        std::env::set_var("ENVCFG_TEST_ENDPOINT_SET", "  http://127.0.0.1:5142 ");
        assert_eq!(
            required_endpoint("ENVCFG_TEST_ENDPOINT_SET").unwrap(),
            "http://127.0.0.1:5142"
        );
    }

    #[test]
    fn local_endpoint_falls_back_to_loopback() {
        assert_eq!(
            local_endpoint("ENVCFG_TEST_LOCAL_UNSET", 5142),
            "http://127.0.0.1:5142"
        );
        std::env::set_var("ENVCFG_TEST_LOCAL_BLANK", "");
        assert_eq!(
            local_endpoint("ENVCFG_TEST_LOCAL_BLANK", 5134),
            "http://127.0.0.1:5134"
        );
        std::env::set_var("ENVCFG_TEST_LOCAL_SET", "http://10.0.0.1:9");
        assert_eq!(
            local_endpoint("ENVCFG_TEST_LOCAL_SET", 5134),
            "http://10.0.0.1:9"
        );
    }

    #[test]
    fn optional_endpoint_treats_blank_as_disabled() {
        assert_eq!(optional_endpoint("ENVCFG_TEST_OPT_UNSET"), None);
        std::env::set_var("ENVCFG_TEST_OPT_BLANK", "  ");
        assert_eq!(optional_endpoint("ENVCFG_TEST_OPT_BLANK"), None);
        std::env::set_var("ENVCFG_TEST_OPT_SET", "http://127.0.0.1:5139");
        assert_eq!(
            optional_endpoint("ENVCFG_TEST_OPT_SET").as_deref(),
            Some("http://127.0.0.1:5139")
        );
    }

    #[test]
    fn required_error_shape() {
        let err = required("ENVCFG_TEST_REQUIRED_MISSING").unwrap_err();
        assert_eq!(
            err.to_string(),
            "missing required env var: ENVCFG_TEST_REQUIRED_MISSING"
        );
        std::env::set_var("ENVCFG_TEST_REQUIRED_SET", "v");
        assert_eq!(required("ENVCFG_TEST_REQUIRED_SET").unwrap(), "v");
    }

    #[test]
    fn port_parsing() {
        assert_eq!(get_port("ENVCFG_TEST_PORT_UNSET", 5133).unwrap(), 5133);
        std::env::set_var("ENVCFG_TEST_PORT_SET", "8080");
        assert_eq!(get_port("ENVCFG_TEST_PORT_SET", 1).unwrap(), 8080);
        std::env::set_var("ENVCFG_TEST_PORT_BAD", "nope");
        assert!(get_port("ENVCFG_TEST_PORT_BAD", 1).is_err());
    }

    #[test]
    fn int_parsing() {
        assert_eq!(get_int("ENVCFG_TEST_INT_UNSET", -7).unwrap(), -7);
        std::env::set_var("ENVCFG_TEST_INT_SET", "42");
        assert_eq!(get_int("ENVCFG_TEST_INT_SET", 0).unwrap(), 42);
        assert_eq!(get_u64("ENVCFG_TEST_U64_UNSET", 9).unwrap(), 9);
        std::env::set_var("ENVCFG_TEST_U64_BAD", "-1");
        assert!(get_u64("ENVCFG_TEST_U64_BAD", 0).is_err());
    }

    #[test]
    fn bool_grammar() {
        std::env::set_var("ENVCFG_TEST_BOOL_ON", "YeS");
        std::env::set_var("ENVCFG_TEST_BOOL_OFF", " Off ");
        std::env::set_var("ENVCFG_TEST_BOOL_WEIRD", "banana");
        std::env::set_var("ENVCFG_TEST_BOOL_EMPTY", "");
        assert!(env_bool("ENVCFG_TEST_BOOL_ON", false));
        assert!(!env_bool("ENVCFG_TEST_BOOL_OFF", true));
        assert!(!env_bool("ENVCFG_TEST_BOOL_WEIRD", false));
        assert!(env_bool("ENVCFG_TEST_BOOL_WEIRD", true));
        assert!(!env_bool("ENVCFG_TEST_BOOL_EMPTY", false));
        assert!(env_bool("ENVCFG_TEST_BOOL_EMPTY", true));
        assert!(env_bool("ENVCFG_TEST_BOOL_UNSET_XYZ", true));
        assert!(!env_bool("ENVCFG_TEST_BOOL_UNSET_XYZ", false));
    }
}
