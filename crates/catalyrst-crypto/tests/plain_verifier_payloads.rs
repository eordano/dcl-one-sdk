use catalyrst_crypto::sign::{create_simple_auth_chain, Wallet};
use catalyrst_crypto::signed_fetch::{
    build_legacy_payload, build_payload_v6, extract_auth_chain, try_extract_signer,
    validate_signature_either_payload, verify_signed_fetch, verify_signed_fetch_meta,
    AuthChainError, AUTH_CHAIN_HEADER_PREFIX, AUTH_METADATA_HEADER, AUTH_TIMESTAMP_HEADER,
};
use http::{HeaderMap, HeaderName, HeaderValue};

const TEST_KEY: &str = "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d";
const FIVE_MINUTES: i64 = 5 * 60;
const METHOD: &str = "post";
const PATH: &str = "/scene-admin";
const METADATA: &str = r#"{"intent":"dcl:explorer:comms-handshake","signer":"dcl:explorer","isGuest":false,"realmName":"LocalPreview","serverName":"LocalPreview","sceneId":"bafkreiAbC123"}"#;

#[derive(Clone, Copy, Debug)]
enum Shape {
    Legacy,
    V6,
}

const SHAPES: [Shape; 2] = [Shape::Legacy, Shape::V6];

fn wallet() -> Wallet {
    Wallet::from_hex(TEST_KEY).unwrap()
}

fn payload(shape: Shape, method: &str, path: &str, ts: &str, metadata: &str) -> String {
    match shape {
        Shape::Legacy => build_legacy_payload(method, path, ts, metadata),
        Shape::V6 => build_payload_v6(method, path, ts, metadata),
    }
}

fn headers_for(
    shape: Shape,
    signed_path: &str,
    signed_metadata: &str,
    delivered_metadata: &str,
    ts_ms: i64,
) -> HeaderMap {
    let ts = ts_ms.to_string();
    let signed = payload(shape, METHOD, signed_path, &ts, signed_metadata);
    let chain = create_simple_auth_chain(&wallet(), &signed).unwrap();

    let mut headers = HeaderMap::new();
    headers.insert(AUTH_TIMESTAMP_HEADER, HeaderValue::from_str(&ts).unwrap());
    headers.insert(
        AUTH_METADATA_HEADER,
        HeaderValue::from_str(delivered_metadata).unwrap(),
    );
    for (i, link) in chain.as_array().into_iter().flatten().enumerate() {
        headers.insert(
            HeaderName::from_bytes(format!("{AUTH_CHAIN_HEADER_PREFIX}{i}").as_bytes()).unwrap(),
            HeaderValue::from_str(&link.to_string()).unwrap(),
        );
    }
    headers
}

fn fresh(shape: Shape) -> HeaderMap {
    headers_for(
        shape,
        PATH,
        METADATA,
        METADATA,
        chrono::Utc::now().timestamp_millis(),
    )
}

fn expected_signer() -> String {
    wallet().address().to_lowercase()
}

#[test]
fn the_fixture_metadata_makes_the_two_shapes_differ() {
    assert_ne!(
        build_legacy_payload(METHOD, PATH, "1", METADATA),
        build_payload_v6(METHOD, PATH, "1", METADATA)
    );
}

#[tokio::test]
async fn a_6x_signed_request_with_mixed_case_metadata_verifies_through_every_plain_verifier() {
    let headers = fresh(Shape::V6);

    let signer = verify_signed_fetch(&headers, METHOD, PATH, FIVE_MINUTES)
        .await
        .unwrap();
    assert_eq!(signer, expected_signer());

    let signer = try_extract_signer(&headers, METHOD, PATH, FIVE_MINUTES)
        .await
        .expect("6.x-signed request must extract a signer");
    assert_eq!(signer, expected_signer());

    let (signer, metadata) = verify_signed_fetch_meta(&headers, METHOD, PATH, FIVE_MINUTES)
        .await
        .unwrap();
    assert_eq!(signer, expected_signer());
    assert_eq!(metadata["sceneId"], serde_json::json!("bafkreiAbC123"));
    assert_eq!(metadata["serverName"], serde_json::json!("LocalPreview"));
    assert_eq!(metadata["isGuest"], serde_json::json!(false));
}

#[tokio::test]
async fn a_legacy_signed_request_with_mixed_case_metadata_still_verifies_through_every_plain_verifier(
) {
    let headers = fresh(Shape::Legacy);

    let signer = verify_signed_fetch(&headers, METHOD, PATH, FIVE_MINUTES)
        .await
        .unwrap();
    assert_eq!(signer, expected_signer());

    let signer = try_extract_signer(&headers, METHOD, PATH, FIVE_MINUTES)
        .await
        .expect("legacy-signed request must still extract a signer");
    assert_eq!(signer, expected_signer());

    let (signer, metadata) = verify_signed_fetch_meta(&headers, METHOD, PATH, FIVE_MINUTES)
        .await
        .unwrap();
    assert_eq!(signer, expected_signer());
    assert_eq!(metadata["sceneId"], serde_json::json!("bafkreiAbC123"));
}

#[tokio::test]
async fn either_shape_verifies_with_a_proxy_prefixed_original_path() {
    let now = chrono::Utc::now().timestamp_millis();
    for shape in SHAPES {
        let mut headers = headers_for(shape, "/comms/scene-admin", METADATA, METADATA, now);
        headers.insert(
            "x-original-path",
            HeaderValue::from_static("/comms/scene-admin?x=1"),
        );
        let signer = verify_signed_fetch(&headers, METHOD, PATH, FIVE_MINUTES)
            .await
            .unwrap_or_else(|err| panic!("{shape:?}: {err:?}"));
        assert_eq!(signer, expected_signer());
    }
}

#[tokio::test]
async fn a_bad_signature_still_fails_as_invalid_signature_under_either_shape() {
    let now = chrono::Utc::now().timestamp_millis();
    for shape in SHAPES {
        let wrong_path = headers_for(shape, "/scene-bans", METADATA, METADATA, now);
        let err = verify_signed_fetch(&wrong_path, METHOD, PATH, FIVE_MINUTES)
            .await
            .unwrap_err();
        assert!(
            matches!(err, AuthChainError::InvalidSignature(_)),
            "{shape:?}: {err:?}"
        );
        assert!(try_extract_signer(&wrong_path, METHOD, PATH, FIVE_MINUTES)
            .await
            .is_none());
        let err = verify_signed_fetch_meta(&wrong_path, METHOD, PATH, FIVE_MINUTES)
            .await
            .unwrap_err();
        assert!(
            matches!(err, AuthChainError::InvalidSignature(_)),
            "{shape:?}: {err:?}"
        );

        let tampered = METADATA.replace("bafkreiAbC123", "bafkreiXyZ999");
        assert_ne!(tampered, METADATA);
        let tampered_metadata = headers_for(shape, PATH, METADATA, &tampered, now);
        let err = verify_signed_fetch(&tampered_metadata, METHOD, PATH, FIVE_MINUTES)
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
    let stale = chrono::Utc::now().timestamp_millis() - (FIVE_MINUTES + 60) * 1000;
    for shape in SHAPES {
        let headers = headers_for(shape, PATH, METADATA, METADATA, stale);
        let err = verify_signed_fetch(&headers, METHOD, PATH, FIVE_MINUTES)
            .await
            .unwrap_err();
        assert!(
            matches!(err, AuthChainError::Expired { .. }),
            "{shape:?}: {err:?}"
        );

        let mut headers = fresh(shape);
        headers.insert(AUTH_TIMESTAMP_HEADER, HeaderValue::from_static("soon"));
        let err = verify_signed_fetch(&headers, METHOD, PATH, FIVE_MINUTES)
            .await
            .unwrap_err();
        assert!(
            matches!(err, AuthChainError::InvalidTimestamp(_)),
            "{shape:?}: {err:?}"
        );
    }
}

#[tokio::test]
async fn validate_signature_either_payload_accepts_both_shapes_over_one_chain_type() {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let now = now_ms / 1000;
    for shape in SHAPES {
        let headers = headers_for(shape, PATH, METADATA, METADATA, now_ms);
        let chain = extract_auth_chain(&headers).unwrap();
        let signer = validate_signature_either_payload(
            &chain,
            METHOD,
            PATH,
            &now_ms.to_string(),
            METADATA,
            FIVE_MINUTES,
            now,
        )
        .await
        .unwrap_or_else(|err| panic!("{shape:?}: {err:?}"));
        assert_eq!(signer, expected_signer());

        let err = validate_signature_either_payload(
            &chain,
            "get",
            PATH,
            &now_ms.to_string(),
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
