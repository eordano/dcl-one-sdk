use anyhow::{anyhow, Context, Result};
use std::env;

pub fn required(key: &str) -> Result<String> {
    env::var(key).map_err(|_| anyhow!("missing required env var: {}", key))
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
                 environment variables — run `{} --help` for the full list",
                service_name, service_name
            );
            std::process::exit(2);
        }
    }
}

fn print_help(service_name: &str, env_docs: &[(&str, &str)]) {
    println!("{} — env-configured service", service_name);
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
    use super::{env_bool, get_int, get_port, get_u64, required};

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
