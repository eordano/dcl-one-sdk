use std::time::Duration;

use axum::http::{Method, StatusCode};
use axum::Router;
use tower_http::cors::{Any, CorsLayer};
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

pub fn finish_app<S>(routes: Router<S>, state: S, cors: Option<CorsLayer>) -> Router
where
    S: Clone + Send + Sync + 'static,
{
    let routes = match cors {
        Some(cors) => routes.layer(cors),
        None => routes,
    };
    routes.layer(TraceLayer::new_for_http()).with_state(state)
}

pub fn cors_any(methods: &[Method]) -> CorsLayer {
    CorsLayer::new()
        .allow_origin(Any)
        .allow_headers(Any)
        .allow_methods(methods.to_vec())
}

/// Wildcard `Access-Control-Allow-Methods`, for services that answered `*` before the
/// scaffold existed and whose preflight bytes have to stay put.
pub fn cors_any_methods() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(Any)
        .allow_headers(Any)
        .allow_methods(Any)
}

pub fn request_timeout_layer(secs: u64) -> TimeoutLayer {
    TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, Duration::from_secs(secs))
}

#[cfg(test)]
mod tests {
    use super::{cors_any, finish_app, request_timeout_layer};

    use axum::body::Body;
    use axum::extract::State;
    use axum::http::{Method, Request, StatusCode};
    use axum::routing::get;
    use axum::Router;
    use tower::ServiceExt;

    #[derive(Clone)]
    struct SmokeState {
        value: &'static str,
    }

    #[tokio::test]
    async fn cors_any_allows_wildcard_origin_and_configured_methods() {
        let cors = cors_any(&[Method::GET, Method::POST]);
        let app: Router<()> = Router::new()
            .route("/ping", get(|| async { "pong" }))
            .layer(cors);

        let request = Request::builder()
            .method(Method::OPTIONS)
            .uri("/ping")
            .header("origin", "https://example.com")
            .header("access-control-request-method", "GET")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("access-control-allow-origin")
                .unwrap(),
            "*"
        );
        let allow_methods = response
            .headers()
            .get("access-control-allow-methods")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(allow_methods.contains("GET"));
        assert!(allow_methods.contains("POST"));
    }

    #[tokio::test(start_paused = true)]
    async fn request_timeout_layer_returns_configured_status_after_deadline() {
        let app: Router<()> = Router::new()
            .route(
                "/slow",
                get(|| async {
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                    "too slow"
                }),
            )
            .layer(request_timeout_layer(1));

        let request = Request::builder().uri("/slow").body(Body::empty()).unwrap();
        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
    }

    #[tokio::test]
    async fn finish_app_applies_cors_trace_and_state() {
        let state = SmokeState { value: "hi" };
        let routes: Router<SmokeState> = Router::new().route(
            "/echo",
            get(|State(state): State<SmokeState>| async move { state.value }),
        );
        let cors = cors_any(&[Method::GET]);
        let app = finish_app(routes, state, Some(cors));

        let request = Request::builder()
            .uri("/echo")
            .header("origin", "https://example.com")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("access-control-allow-origin")
                .unwrap(),
            "*"
        );
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(&body[..], b"hi");
    }

    #[tokio::test]
    async fn finish_app_without_cors_omits_cors_headers() {
        let state = SmokeState { value: "hi" };
        let routes: Router<SmokeState> = Router::new().route("/ping", get(|| async { "pong" }));
        let app = finish_app(routes, state, None);

        let request = Request::builder().uri("/ping").body(Body::empty()).unwrap();
        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(response
            .headers()
            .get("access-control-allow-origin")
            .is_none());
    }
}
