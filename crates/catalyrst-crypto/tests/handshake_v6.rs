use catalyrst_crypto::metadata_gate::{reject_if_signer, SignerGate};
use catalyrst_crypto::sign::{create_simple_auth_chain, Wallet};
use catalyrst_crypto::signed_fetch::handshake::{
    optional_signer_v6, require_signer_v6, verify_handshake_meta_v6, verify_handshake_v6,
    AuthChainError,
};
use catalyrst_crypto::signed_fetch::{
    build_legacy_payload, build_payload_v6, AUTH_CHAIN_HEADER_PREFIX, AUTH_METADATA_HEADER,
    AUTH_TIMESTAMP_HEADER,
};
use http::{HeaderMap, HeaderName, HeaderValue};

const TEST_KEY: &str = "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d";
const FIVE_MINUTES: i64 = 5 * 60;
const METHOD: &str = "get";
const PATH: &str = "/";
const SCENE_SIGNER: &str = "decentraland-kernel-scene";
const SCENE_KEYS: &[&str] = &["signer", "intent", "sceneId", "realm.serverName"];
const METADATA: &str = r#"{"intent":"dcl:explorer:comms-handshake","signer":"dcl:explorer","sceneId":"bafkreiAbC123","realm":{"serverName":"LocalPreview"}}"#;

#[derive(Clone, Copy)]
enum Shape {
    Legacy,
    V6,
}

fn frame(shape: Shape, signed_metadata: &str, delivered_metadata: &str, ts_ms: i64) -> String {
    let wallet = Wallet::from_hex(TEST_KEY).unwrap();
    let ts = ts_ms.to_string();
    let payload = match shape {
        Shape::Legacy => build_legacy_payload(METHOD, PATH, &ts, signed_metadata),
        Shape::V6 => build_payload_v6(METHOD, PATH, &ts, signed_metadata),
    };
    let chain = create_simple_auth_chain(&wallet, &payload).unwrap();

    let mut obj = serde_json::Map::new();
    obj.insert(AUTH_TIMESTAMP_HEADER.to_string(), ts.into());
    obj.insert(
        AUTH_METADATA_HEADER.to_string(),
        delivered_metadata.to_string().into(),
    );
    for (i, link) in chain.as_array().into_iter().flatten().enumerate() {
        obj.insert(
            format!("{AUTH_CHAIN_HEADER_PREFIX}{i}"),
            link.to_string().into(),
        );
    }
    serde_json::Value::Object(obj).to_string()
}

fn headers(shape: Shape, signed_metadata: &str, delivered_metadata: &str, ts_ms: i64) -> HeaderMap {
    let raw = frame(shape, signed_metadata, delivered_metadata, ts_ms);
    let obj: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let mut headers = HeaderMap::new();
    for (name, value) in obj.as_object().unwrap() {
        headers.insert(
            HeaderName::from_bytes(name.as_bytes()).unwrap(),
            HeaderValue::from_str(value.as_str().unwrap()).unwrap(),
        );
    }
    headers
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn detail(err: AuthChainError) -> String {
    match err {
        AuthChainError::MalformedChain { detail, .. } => detail,
        other => panic!("expected MalformedChain, got {other:?}"),
    }
}

fn address() -> String {
    Wallet::from_hex(TEST_KEY).unwrap().address().to_lowercase()
}

#[tokio::test]
async fn a_v6_signed_frame_verifies_without_consulting_the_legacy_path() {
    let f = frame(Shape::V6, METADATA, METADATA, now_ms());
    let now = chrono::Utc::now().timestamp();

    let (signer, metadata) =
        verify_handshake_meta_v6(&f, METHOD, PATH, FIVE_MINUTES, now, SCENE_KEYS, None)
            .await
            .unwrap();
    assert_eq!(signer, address());
    assert_eq!(metadata["sceneId"], serde_json::json!("bafkreiAbC123"));

    let strict = verify_handshake_v6(&f, METHOD, PATH, FIVE_MINUTES, now, &[], None)
        .await
        .unwrap();
    assert_eq!(strict, address());
}

#[tokio::test]
async fn a_legacy_signed_frame_needs_declared_canonical_keys() {
    let f = frame(Shape::Legacy, METADATA, METADATA, now_ms());
    let now = chrono::Utc::now().timestamp();

    let err = verify_handshake_v6(&f, METHOD, PATH, FIVE_MINUTES, now, &[], None)
        .await
        .unwrap_err();
    assert!(
        matches!(err, AuthChainError::InvalidSignature(_)),
        "{err:?}"
    );

    let signer = verify_handshake_v6(&f, METHOD, PATH, FIVE_MINUTES, now, SCENE_KEYS, None)
        .await
        .unwrap();
    assert_eq!(signer, address());
}

#[tokio::test]
async fn a_recased_declared_key_is_refused_on_the_legacy_frame() {
    let delivered = METADATA.replace("\"sceneId\"", "\"sceneid\"");
    assert_ne!(delivered, METADATA);
    let f = frame(Shape::Legacy, METADATA, &delivered, now_ms());
    let now = chrono::Utc::now().timestamp();

    let err = verify_handshake_v6(&f, METHOD, PATH, FIVE_MINUTES, now, SCENE_KEYS, None)
        .await
        .unwrap_err();
    assert!(detail(err).contains("invalid chain metadata"));
}

#[tokio::test]
async fn every_error_class_but_a_signature_failure_propagates_immediately() {
    let now = chrono::Utc::now().timestamp();
    let stale = now_ms() - (FIVE_MINUTES + 60) * 1000;
    let delivered = METADATA.replace("\"sceneId\"", "\"sceneid\"");

    let f = frame(Shape::Legacy, METADATA, &delivered, stale);
    let err = verify_handshake_v6(&f, METHOD, PATH, FIVE_MINUTES, now, SCENE_KEYS, None)
        .await
        .unwrap_err();
    assert!(matches!(err, AuthChainError::Expired { .. }), "{err:?}");

    let err = verify_handshake_v6(
        "not json",
        METHOD,
        PATH,
        FIVE_MINUTES,
        now,
        SCENE_KEYS,
        None,
    )
    .await
    .unwrap_err();
    assert!(detail(err).contains("frame not JSON"));

    let err = verify_handshake_v6("[]", METHOD, PATH, FIVE_MINUTES, now, SCENE_KEYS, None)
        .await
        .unwrap_err();
    assert!(matches!(err, AuthChainError::EnvelopeNotObject), "{err:?}");

    let f = frame(Shape::Legacy, "not json", "not json", now_ms());
    let err = verify_handshake_v6(&f, METHOD, PATH, FIVE_MINUTES, now, SCENE_KEYS, None)
        .await
        .unwrap_err();
    assert!(detail(err).contains("invalid chain metadata"));

    let f = frame(Shape::V6, METADATA, METADATA, now_ms());
    let err = verify_handshake_v6(
        &f,
        METHOD,
        PATH,
        FIVE_MINUTES,
        now,
        &["realm..serverName"],
        None,
    )
    .await
    .unwrap_err();
    assert!(detail(err).contains("empty path segment"));
}

#[tokio::test]
async fn the_signer_gate_answers_before_signature_verification() {
    let gate: SignerGate = reject_if_signer(&[SCENE_SIGNER]).unwrap();
    let now = chrono::Utc::now().timestamp();
    let raw = "{\"signer\":\"Decentraland-Kernel-Scene\"}";
    let f = frame(
        Shape::Legacy,
        "{\"signer\":\"someone-else\"}",
        raw,
        now_ms(),
    );

    let err = verify_handshake_v6(&f, METHOD, PATH, FIVE_MINUTES, now, SCENE_KEYS, Some(&gate))
        .await
        .unwrap_err();
    assert!(
        detail(err).contains("invalid metadata content"),
        "signature failures must not pre-empt the gate"
    );

    let bom = format!("{{\"signer\":\"\\uFEFF{SCENE_SIGNER}\"}}");
    let f = frame(Shape::Legacy, &bom, &bom, now_ms());
    let err = verify_handshake_v6(&f, METHOD, PATH, FIVE_MINUTES, now, SCENE_KEYS, Some(&gate))
        .await
        .unwrap_err();
    assert!(detail(err).contains("invalid metadata content"));

    let clean = "{\"signer\":\"dcl:explorer\"}";
    let f = frame(Shape::V6, clean, clean, now_ms());
    assert!(
        verify_handshake_v6(&f, METHOD, PATH, FIVE_MINUTES, now, &[], Some(&gate))
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn the_header_bag_twins_take_the_same_contract() {
    let gate = reject_if_signer(&[SCENE_SIGNER]).unwrap();

    let bag = headers(Shape::Legacy, METADATA, METADATA, now_ms());
    let signer = require_signer_v6(&bag, METHOD, PATH, FIVE_MINUTES, SCENE_KEYS, Some(&gate))
        .await
        .unwrap();
    assert_eq!(signer, address());
    assert_eq!(
        optional_signer_v6(&bag, METHOD, PATH, FIVE_MINUTES, SCENE_KEYS, Some(&gate)).await,
        Some(signer)
    );

    let err = require_signer_v6(&bag, METHOD, PATH, FIVE_MINUTES, &[], Some(&gate))
        .await
        .unwrap_err();
    assert!(
        matches!(err, AuthChainError::InvalidSignature(_)),
        "{err:?}"
    );

    assert_eq!(
        optional_signer_v6(
            &HeaderMap::new(),
            METHOD,
            PATH,
            FIVE_MINUTES,
            SCENE_KEYS,
            Some(&gate)
        )
        .await,
        None
    );
}
