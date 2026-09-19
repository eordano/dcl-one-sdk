use super::AppState;
use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use std::{path::PathBuf, sync::Arc};

pub(super) fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/api/project/ui-designer/{asset}", get(asset))
}

fn runtime_dir(st: &AppState) -> Option<PathBuf> {
    let public = st.data_layer.as_ref()?.public_dir.as_ref()?;
    let dir = public.join("sdk-ui-designer");
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).ok()?).ok()?;
    (manifest["oxc"] == "0.60.0"
        && manifest["miniRpc"] == "1.0.7"
        && manifest["uiDesigner"] == true
        && dir.join("runtime.js").is_file()
        && dir.join("oxc_parser_wasm_bg.wasm").is_file()
        && (public.join("bundle.js").is_file() || public.join("bundle.js.gz").is_file()))
    .then_some(dir)
}

pub(super) fn available(st: &AppState) -> bool {
    runtime_dir(st).is_some()
}

async fn asset(State(st): State<Arc<AppState>>, Path(asset): Path<String>) -> Response {
    let mime = match asset.as_str() {
        "runtime.js" => "application/javascript",
        "oxc_parser_wasm_bg.wasm" => "application/wasm",
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    let Some(dir) = runtime_dir(&st) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match tokio::fs::read(dir.join(asset)).await {
        Ok(bytes) => ([(header::CONTENT_TYPE, mime)], bytes).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::start::testkit::{scene, state, Tmp};

    #[tokio::test]
    async fn designer_requires_exact_runtime_and_real_bundle() {
        let temp = Tmp::new("ui-designer");
        let mut st = state(vec![scene(&temp.0.join("scene"), "Scene", &["0,0"], "")]);
        let public = temp.0.join("public");
        let runtime = public.join("sdk-ui-designer");
        std::fs::create_dir_all(&runtime).unwrap();
        let (_, port_rx) = tokio::sync::watch::channel(1000);
        st.data_layer = Some(crate::data_layer::DataLayerState {
            port_rx,
            public_dir: Some(public.clone()),
        });
        assert!(!available(&st));
        for path in [
            runtime.join("runtime.js"),
            runtime.join("oxc_parser_wasm_bg.wasm"),
            public.join("bundle.js"),
        ] {
            std::fs::write(path, b"fixture").unwrap();
        }
        std::fs::write(
            runtime.join("manifest.json"),
            r#"{"oxc":"0.61.0","miniRpc":"1.0.7","uiDesigner":true}"#,
        )
        .unwrap();
        assert!(!available(&st));
        std::fs::write(
            runtime.join("manifest.json"),
            r#"{"oxc":"0.60.0","miniRpc":"1.0.7","uiDesigner":true}"#,
        )
        .unwrap();
        assert!(available(&st));
        let st = Arc::new(st);
        assert_eq!(
            asset(State(st.clone()), Path("runtime.js".into()))
                .await
                .status(),
            StatusCode::OK
        );
        assert_eq!(
            asset(State(st), Path("../manifest.json".into()))
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
    }
}
