use catalyrst_crypto::sign::{create_simple_auth_chain, Wallet};
use catalyrst_crypto::signed_fetch::handshake::{
    extract_from_object, optional_signer, require_signer, validate_signature_either_payload,
    verify_handshake, AuthChainError,
};
use catalyrst_crypto::signed_fetch::{
    build_legacy_payload, build_payload_v6, AUTH_CHAIN_HEADER_PREFIX, AUTH_METADATA_HEADER,
    AUTH_TIMESTAMP_HEADER,
};
use http::{HeaderMap, HeaderName, HeaderValue};

const TEST_KEY: &str = "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d";
const FIVE_MINUTES: i64 = 5 * 60;
const METHOD: &str = "get";
const PATH: &str = "/api/quests/abc";
const METADATA: &str = r#"{"intent":"dcl:explorer:comms-handshake","signer":"dcl:explorer","isGuest":false,"realmName":"LocalPreview","sceneId":"bafkreiAbC123"}"#;

#[derive(Clone, Copy, Debug)]
enum Shape {
    Legacy,
    V6,
}

const SHAPES: [Shape; 2] = [Shape::Legacy, Shape::V6];

fn wallet() -> Wallet {
    Wallet::from_hex(TEST_KEY).unwrap()
}

fn expected_signer() -> String {
    wallet().address().to_lowercase()
}

fn payload(shape: Shape, method: &str, path: &str, ts: &str, metadata: &str) -> String {
    match shape {
        Shape::Legacy => build_legacy_payload(method, path, ts, metadata),
        Shape::V6 => build_payload_v6(method, path, ts, metadata),
    }
}

fn frame_for(
    shape: Shape,
    signed_path: &str,
    signed_metadata: &str,
    delivered_metadata: &str,
    ts_ms: i64,
) -> serde_json::Map<String, serde_json::Value> {
    let ts = ts_ms.to_string();
    let signed = payload(shape, METHOD, signed_path, &ts, signed_metadata);
    let chain = create_simple_auth_chain(&wallet(), &signed).unwrap();

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
    obj
}

fn frame_json(
    shape: Shape,
    signed_path: &str,
    signed_metadata: &str,
    delivered_metadata: &str,
    ts_ms: i64,
) -> String {
    serde_json::Value::Object(frame_for(
        shape,
        signed_path,
        signed_metadata,
        delivered_metadata,
        ts_ms,
    ))
    .to_string()
}

fn headers_for(
    shape: Shape,
    signed_path: &str,
    signed_metadata: &str,
    delivered_metadata: &str,
    ts_ms: i64,
) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for (name, value) in frame_for(
        shape,
        signed_path,
        signed_metadata,
        delivered_metadata,
        ts_ms,
    ) {
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

#[test]
fn the_fixture_metadata_makes_the_two_shapes_differ() {
    assert_ne!(
        build_legacy_payload(METHOD, PATH, "1", METADATA),
        build_payload_v6(METHOD, PATH, "1", METADATA)
    );
}

#[tokio::test]
async fn either_shape_verifies_through_the_frame_and_header_verifiers() {
    for shape in SHAPES {
        let ts_ms = now_ms();
        let now = ts_ms / 1000;

        let frame = frame_json(shape, PATH, METADATA, METADATA, ts_ms);
        let signer = verify_handshake(&frame, METHOD, PATH, FIVE_MINUTES, now)
            .await
            .unwrap_or_else(|err| panic!("{shape:?}: {err:?}"));
        assert_eq!(signer, expected_signer());

        let headers = headers_for(shape, PATH, METADATA, METADATA, ts_ms);
        let signer = require_signer(&headers, METHOD, PATH, FIVE_MINUTES)
            .await
            .unwrap_or_else(|err| panic!("{shape:?}: {err:?}"));
        assert_eq!(signer, expected_signer());

        let signer = optional_signer(&headers, METHOD, PATH, FIVE_MINUTES)
            .await
            .unwrap_or_else(|| panic!("{shape:?}: optional_signer must recover a signer"));
        assert_eq!(signer, expected_signer());
    }
}

#[tokio::test]
async fn either_shape_verifies_with_a_proxy_prefixed_original_path() {
    for shape in SHAPES {
        let mut headers = headers_for(
            shape,
            "/quests/api/quests/abc",
            METADATA,
            METADATA,
            now_ms(),
        );
        headers.insert(
            "x-original-path",
            HeaderValue::from_static("/quests/api/quests/abc?x=1"),
        );
        let signer = require_signer(&headers, METHOD, PATH, FIVE_MINUTES)
            .await
            .unwrap_or_else(|err| panic!("{shape:?}: {err:?}"));
        assert_eq!(signer, expected_signer());
    }
}

#[tokio::test]
async fn a_bad_signature_still_fails_as_invalid_signature_under_either_shape() {
    for shape in SHAPES {
        let ts_ms = now_ms();
        let now = ts_ms / 1000;

        let wrong_path = frame_json(shape, "/api/quests/other", METADATA, METADATA, ts_ms);
        let err = verify_handshake(&wrong_path, METHOD, PATH, FIVE_MINUTES, now)
            .await
            .unwrap_err();
        assert!(
            matches!(err, AuthChainError::InvalidSignature(_)),
            "{shape:?}: {err:?}"
        );

        let wrong_path = headers_for(shape, "/api/quests/other", METADATA, METADATA, ts_ms);
        let err = require_signer(&wrong_path, METHOD, PATH, FIVE_MINUTES)
            .await
            .unwrap_err();
        assert!(
            matches!(err, AuthChainError::InvalidSignature(_)),
            "{shape:?}: {err:?}"
        );
        assert!(optional_signer(&wrong_path, METHOD, PATH, FIVE_MINUTES)
            .await
            .is_none());

        let tampered = METADATA.replace("bafkreiAbC123", "bafkreiXyZ999");
        assert_ne!(tampered, METADATA);
        let tampered_metadata = frame_json(shape, PATH, METADATA, &tampered, ts_ms);
        let err = verify_handshake(&tampered_metadata, METHOD, PATH, FIVE_MINUTES, now)
            .await
            .unwrap_err();
        assert!(
            matches!(err, AuthChainError::InvalidSignature(_)),
            "{shape:?}: {err:?}"
        );
    }
}

#[tokio::test]
async fn a_deterministic_failure_is_not_retried_against_the_other_shape() {
    for shape in SHAPES {
        let ts_ms = now_ms();
        let stale_now = ts_ms / 1000 + FIVE_MINUTES + 60;
        let frame = frame_json(shape, PATH, METADATA, METADATA, ts_ms);
        let err = verify_handshake(&frame, METHOD, PATH, FIVE_MINUTES, stale_now)
            .await
            .unwrap_err();
        assert!(
            matches!(err, AuthChainError::Expired { .. }),
            "{shape:?}: {err:?}"
        );

        let mut obj = frame_for(shape, PATH, METADATA, METADATA, ts_ms);
        obj.insert(AUTH_TIMESTAMP_HEADER.to_string(), "soon".into());
        let frame = serde_json::Value::Object(obj).to_string();
        let err = verify_handshake(&frame, METHOD, PATH, FIVE_MINUTES, ts_ms / 1000)
            .await
            .unwrap_err();
        assert!(
            matches!(err, AuthChainError::MalformedChain { .. }),
            "{shape:?}: {err:?}"
        );

        let mut obj = frame_for(shape, PATH, METADATA, METADATA, ts_ms);
        obj.remove(AUTH_TIMESTAMP_HEADER);
        let frame = serde_json::Value::Object(obj).to_string();
        let err = verify_handshake(&frame, METHOD, PATH, FIVE_MINUTES, ts_ms / 1000)
            .await
            .unwrap_err();
        assert!(
            matches!(err, AuthChainError::MissingHeader(_)),
            "{shape:?}: {err:?}"
        );
    }
}

#[tokio::test]
async fn validate_signature_either_payload_accepts_both_shapes_over_the_handshake_chain() {
    for shape in SHAPES {
        let ts_ms = now_ms();
        let now = ts_ms / 1000;
        let obj = frame_for(shape, PATH, METADATA, METADATA, ts_ms);
        let chain = extract_from_object(&obj).unwrap();
        let signer = validate_signature_either_payload(
            &chain,
            METHOD,
            PATH,
            &ts_ms.to_string(),
            METADATA,
            FIVE_MINUTES,
            now,
        )
        .await
        .unwrap_or_else(|err| panic!("{shape:?}: {err:?}"));
        assert_eq!(signer, expected_signer());

        let err = validate_signature_either_payload(
            &chain,
            "post",
            PATH,
            &ts_ms.to_string(),
            METADATA,
            FIVE_MINUTES,
            now,
        )
        .await
        .unwrap_err();
        assert!(
            matches!(err, AuthChainError::InvalidSignature(_)),
            "{shape:?}: {err:?}"
        );
    }
}
