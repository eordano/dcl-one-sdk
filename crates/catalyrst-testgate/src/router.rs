use std::net::SocketAddr;

use tokio::task::JoinHandle;

pub async fn spawn_router(router: axum::Router) -> (SocketAddr, JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral fixture listener");
    let addr = listener.local_addr().expect("fixture listener local addr");
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    (addr, handle)
}

pub fn base_url(addr: SocketAddr) -> String {
    format!("http://{addr}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::get;
    use axum::Router;

    #[tokio::test]
    async fn spawn_router_serves_on_its_own_ephemeral_port() {
        let router = Router::new().route("/ping", get(|| async { "pong" }));
        let (addr, _handle) = spawn_router(router).await;

        let resp = reqwest::get(format!("{}/ping", base_url(addr)))
            .await
            .expect("request the spawned router");
        assert_eq!(resp.status(), 200);
        assert_eq!(resp.text().await.unwrap(), "pong");
    }

    #[tokio::test]
    async fn two_spawned_routers_never_share_a_port() {
        let a = Router::new().route("/", get(|| async { "a" }));
        let b = Router::new().route("/", get(|| async { "b" }));
        let (addr_a, _ha) = spawn_router(a).await;
        let (addr_b, _hb) = spawn_router(b).await;
        assert_ne!(addr_a.port(), addr_b.port());
    }
}
