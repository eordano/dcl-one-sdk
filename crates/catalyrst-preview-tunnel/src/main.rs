use anyhow::Result;
use catalyrst_preview_tunnel::{router, AppState, Config};
use std::net::SocketAddr;
use std::sync::Arc;

const ENV_DOCS: &[(&str, &str)] = &[
    ("HTTP_SERVER_HOST", "bind address (default 127.0.0.1)"),
    ("HTTP_SERVER_PORT", "listen port (default 5167)"),
    (
        "PUBLIC_BASE_URL",
        "public URL prefix advertised in allocated tunnel URLs (default http://HOST:PORT)",
    ),
    (
        "TUNNEL_TOKENS",
        "comma-separated bearer tokens required to open a trunk (default empty = no auth)",
    ),
    (
        "TUNNEL_ALLOW_IDS",
        "comma-separated tunnel ids clients may claim (default empty = random ids)",
    ),
    (
        "TUNNEL_GRACE_SECS",
        "seconds a disconnected tunnel id stays reserved for reconnect (default 120)",
    ),
    (
        "TUNNEL_PING_SECS",
        "trunk keepalive ping interval in seconds (default 20, min 1)",
    ),
    (
        "TUNNEL_OPEN_TIMEOUT_SECS",
        "seconds to wait for the agent to answer an open request (default 15, min 1)",
    ),
    (
        "TUNNEL_BODY_MAX_BYTES",
        "max proxied request body size in bytes (default 67108864)",
    ),
    (
        "RUST_LOG",
        "tracing filter (default catalyrst_preview_tunnel=info)",
    ),
];

#[tokio::main]
async fn main() -> Result<()> {
    catalyrst_envcfg::handle_standard_args("catalyrst-preview-tunnel", ENV_DOCS);

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "catalyrst_preview_tunnel=info".into()),
        )
        .with_target(false)
        .init();

    let cfg = Config::from_env()?;
    let addr: SocketAddr = format!("{}:{}", cfg.http_host, cfg.http_port).parse()?;
    let public_base = cfg.public_base();
    let state = Arc::new(AppState::new(cfg));
    let app = router(state);

    tracing::info!(%addr, %public_base, "catalyrst-preview-tunnel listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
