use catalyrst_crypto::metadata_gate::{reject_if_signer, SignerGate};
use catalyrst_crypto::sign::{create_simple_auth_chain, Wallet};
use catalyrst_crypto::signed_fetch::{
    build_legacy_payload, build_payload_v6, verify_signed_fetch_meta_with_legacy_fallback,
    AuthChainError, AUTH_CHAIN_HEADER_PREFIX, AUTH_METADATA_HEADER, AUTH_TIMESTAMP_HEADER,
};
use catalyrst_types::ApiError;
use http::{HeaderMap, HeaderName, HeaderValue};

const TEST_KEY: &str = "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d";
const FIVE_MINUTES: i64 = 5 * 60;
const METHOD: &str = "post";
const PATH: &str = "/get-scene-adapter";
const SCENE_SIGNER: &str = "decentraland-kernel-scene";
const SCENE_KEYS: &[&str] = &["signer", "intent", "sceneId", "realm.serverName"];
const METADATA: &str = r#"{"intent":"dcl:explorer:comms-handshake","signer":"dcl:explorer","isGuest":true,"realm":{"serverName":"LocalPreview"},"sceneId":"bafkreiAbC123"}"#;

#[derive(Clone, Copy)]
enum Shape {
    Legacy,
    V6,
}

fn headers_for(
    shape: Shape,
    signed_metadata: &str,
    delivered_metadata: &str,
    ts_ms: i64,
) -> HeaderMap {
    let wallet = Wallet::from_hex(TEST_KEY).unwrap();
    let ts = ts_ms.to_string();
    let payload = match shape {
        Shape::Legacy => build_legacy_payload(METHOD, PATH, &ts, signed_metadata),
        Shape::V6 => build_payload_v6(METHOD, PATH, &ts, signed_metadata),
    };
    let chain = create_simple_auth_chain(&wallet, &payload).unwrap();

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

fn legacy_headers(delivered_metadata: &str) -> HeaderMap {
    headers_for(
        Shape::Legacy,
        METADATA,
        delivered_metadata,
        chrono::Utc::now().timestamp_millis(),
    )
}

async fn verify(
    headers: &HeaderMap,
    canonical_keys: &[&str],
    gate: Option<&SignerGate>,
) -> Result<serde_json::Value, AuthChainError> {
    let wallet = Wallet::from_hex(TEST_KEY).unwrap();
    let (signer, metadata) = verify_signed_fetch_meta_with_legacy_fallback(
        headers,
        METHOD,
        PATH,
        FIVE_MINUTES,
        canonical_keys,
        gate,
    )
    .await?;
    assert_eq!(signer, wallet.address().to_lowercase());
    Ok(metadata)
}

fn malformed_detail(err: AuthChainError) -> String {
    match err {
        AuthChainError::MalformedChain { detail } => detail,
        other => panic!("expected MalformedChain, got {other:?}"),
    }
}

#[tokio::test]
async fn current_format_verifies_without_consulting_the_legacy_path() {
    let headers = headers_for(
        Shape::V6,
        METADATA,
        METADATA,
        chrono::Utc::now().timestamp_millis(),
    );
    let metadata = verify(&headers, SCENE_KEYS, None).await.unwrap();
    assert_eq!(
        metadata,
        serde_json::from_str::<serde_json::Value>(METADATA).unwrap()
    );

    let strict = verify(&headers, &[], None).await.unwrap();
    assert_eq!(strict["sceneId"], serde_json::json!("bafkreiAbC123"));
}

#[tokio::test]
async fn legacy_signed_request_is_refused_when_no_canonical_keys_are_declared() {
    let headers = legacy_headers(METADATA);
    let err = verify(&headers, &[], None).await.unwrap_err();
    assert!(
        matches!(err, AuthChainError::InvalidSignature(_)),
        "{err:?}"
    );
}

#[tokio::test]
async fn legacy_signed_request_verifies_when_canonical_keys_are_declared() {
    let headers = legacy_headers(METADATA);
    let metadata = verify(&headers, SCENE_KEYS, None).await.unwrap();
    assert_eq!(metadata["sceneId"], serde_json::json!("bafkreiAbC123"));
    assert_eq!(
        metadata["realm"]["serverName"],
        serde_json::json!("LocalPreview")
    );
}

#[tokio::test]
async fn recased_declared_key_is_refused_on_the_legacy_path() {
    for (from, to) in [
        ("\"sceneId\"", "\"sceneid\""),
        ("\"signer\"", "\"Signer\""),
        ("\"serverName\"", "\"servername\""),
    ] {
        let delivered = METADATA.replace(from, to);
        assert_ne!(delivered, METADATA);
        let headers = legacy_headers(&delivered);

        let err = verify(&headers, SCENE_KEYS, None).await.unwrap_err();
        let detail = malformed_detail(err);
        assert!(detail.contains("invalid chain metadata"), "{detail}");
    }
}

#[tokio::test]
async fn a_declared_key_delivered_under_two_spellings_is_ambiguous() {
    for (from, to) in [
        (
            "\"signer\":\"dcl:explorer\"",
            "\"signer\":\"dcl:explorer\",\"Signer\":\"other\"",
        ),
        (
            "\"serverName\":\"LocalPreview\"",
            "\"serverName\":\"LocalPreview\",\"servername\":\"other\"",
        ),
    ] {
        let delivered = METADATA.replace(from, to);
        assert_ne!(delivered, METADATA);
        let headers = legacy_headers(&delivered);

        let detail = malformed_detail(verify(&headers, SCENE_KEYS, None).await.unwrap_err());
        assert!(detail.contains("spellings"), "{detail}");
    }
}

#[tokio::test]
async fn a_declared_path_is_followed_through_arrays() {
    const ARRAY_KEYS: &[&str] = &["items.sceneId"];
    let signed = r#"{"items":[{"sceneId":"bafkreiAbC"},{"sceneId":"bafkreiDeF"}]}"#;
    let now = chrono::Utc::now().timestamp_millis();

    let canonical = headers_for(Shape::Legacy, signed, signed, now);
    assert!(verify(&canonical, ARRAY_KEYS, None).await.is_ok());

    let second_recased = signed.replace("\"sceneId\":\"bafkreiDeF\"", "\"SceneId\":\"bafkreiDeF\"");
    let headers = headers_for(Shape::Legacy, signed, &second_recased, now);
    let detail = malformed_detail(verify(&headers, ARRAY_KEYS, None).await.unwrap_err());
    assert!(detail.contains("invalid chain metadata"), "{detail}");

    let scalars = r#"{"items":["bafkreiAbC","bafkreiDeF"]}"#;
    let headers = headers_for(Shape::Legacy, scalars, scalars, now);
    assert!(verify(&headers, ARRAY_KEYS, None).await.is_ok());
}

#[tokio::test]
async fn an_undeclared_key_may_be_recased() {
    let delivered = METADATA.replace("\"isGuest\"", "\"isguest\"");
    assert_ne!(delivered, METADATA);
    let headers = legacy_headers(&delivered);
    let metadata = verify(&headers, SCENE_KEYS, None).await.unwrap();
    assert_eq!(metadata["isguest"], serde_json::json!(true));
}

#[tokio::test]
async fn an_expired_chain_never_reaches_the_legacy_retry() {
    let delivered = METADATA.replace("\"sceneId\"", "\"sceneid\"");
    let stale = chrono::Utc::now().timestamp_millis() - (FIVE_MINUTES + 60) * 1000;
    let headers = headers_for(Shape::Legacy, METADATA, &delivered, stale);

    let err = verify(&headers, SCENE_KEYS, None).await.unwrap_err();
    assert!(matches!(err, AuthChainError::Expired { .. }), "{err:?}");
}

#[tokio::test]
async fn a_bad_request_class_failure_never_reaches_the_legacy_retry() {
    let delivered = METADATA.replace("\"sceneId\"", "\"sceneid\"");
    let mut headers = legacy_headers(&delivered);
    headers.insert(
        AUTH_TIMESTAMP_HEADER,
        HeaderValue::from_static("not-a-number"),
    );
    let err = verify(&headers, SCENE_KEYS, None).await.unwrap_err();
    assert!(
        matches!(err, AuthChainError::InvalidTimestamp(_)),
        "{err:?}"
    );

    let mut headers = legacy_headers(&delivered);
    headers.remove(AUTH_TIMESTAMP_HEADER);
    let err = verify(&headers, SCENE_KEYS, None).await.unwrap_err();
    assert!(matches!(err, AuthChainError::MissingTimestamp), "{err:?}");

    let mut headers = legacy_headers(&delivered);
    headers
        .remove(HeaderName::from_bytes(format!("{AUTH_CHAIN_HEADER_PREFIX}1").as_bytes()).unwrap());
    let err = verify(&headers, SCENE_KEYS, None).await.unwrap_err();
    assert!(matches!(err, AuthChainError::InsufficientLinks), "{err:?}");
}

#[tokio::test]
async fn a_misconfigured_key_list_fails_before_anything_is_verified() {
    let headers = headers_for(
        Shape::V6,
        METADATA,
        METADATA,
        chrono::Utc::now().timestamp_millis(),
    );
    let detail = malformed_detail(
        verify(&headers, &["realm..serverName"], None)
            .await
            .unwrap_err(),
    );
    assert!(detail.contains("empty path segment"), "{detail}");
}

#[tokio::test]
async fn unparseable_or_non_object_metadata_is_refused() {
    let now = chrono::Utc::now().timestamp_millis();
    for raw in ["not json", "[{\"signer\":\"dcl:explorer\"}]", "\"signer\""] {
        let headers = headers_for(Shape::Legacy, raw, raw, now);
        let detail = malformed_detail(verify(&headers, SCENE_KEYS, None).await.unwrap_err());
        assert!(detail.contains("invalid chain metadata"), "{detail}");
    }
}

#[tokio::test]
async fn the_signer_gate_refuses_a_recased_scene_signer() {
    let gate = reject_if_signer(&[SCENE_SIGNER]).unwrap();
    let now = chrono::Utc::now().timestamp_millis();

    for signer in [
        SCENE_SIGNER,
        "Decentraland-Kernel-Scene",
        " decentraland-kernel-scene",
    ] {
        let raw = format!("{{\"signer\":\"{signer}\"}}");
        let headers = headers_for(Shape::Legacy, &raw, &raw, now);
        let detail = malformed_detail(verify(&headers, SCENE_KEYS, Some(&gate)).await.unwrap_err());
        assert!(detail.contains("invalid metadata content"), "{detail}");
    }
}

#[tokio::test]
async fn the_signer_gate_runs_before_signature_verification() {
    let gate = reject_if_signer(&[SCENE_SIGNER]).unwrap();
    let raw = "{\"signer\":\"Decentraland-Kernel-Scene\"}";
    let headers = headers_for(
        Shape::Legacy,
        "{\"signer\":\"someone-else\"}",
        raw,
        chrono::Utc::now().timestamp_millis(),
    );

    let err = verify(&headers, SCENE_KEYS, Some(&gate)).await.unwrap_err();
    assert!(err.is_bad_request(), "{err:?}");
    assert!(
        malformed_detail(err).contains("invalid metadata content"),
        "signature failures must not pre-empt the gate"
    );
}

/// Upstream runs expiration before the metadata validator so a replayed request
/// pays for neither the gate nor, on an EIP-1654 chain, the catalyst round-trip.
#[tokio::test]
async fn expiration_answers_before_the_signer_gate() {
    let gate = reject_if_signer(&[SCENE_SIGNER]).unwrap();
    let raw = format!("{{\"signer\":\"{SCENE_SIGNER}\"}}");
    let stale = chrono::Utc::now().timestamp_millis() - (FIVE_MINUTES + 60) * 1000;
    let headers = headers_for(Shape::V6, &raw, &raw, stale);

    let err = verify(&headers, SCENE_KEYS, Some(&gate)).await.unwrap_err();
    assert!(matches!(err, AuthChainError::Expired { .. }), "{err:?}");
    assert!(!err.is_bad_request());
}

#[tokio::test]
async fn every_refusal_stays_distinguishable_at_the_http_boundary() {
    let gate = reject_if_signer(&[SCENE_SIGNER]).unwrap();
    let now = chrono::Utc::now().timestamp_millis();
    let scene = format!("{{\"signer\":\"{SCENE_SIGNER}\"}}");
    let recased = METADATA.replace("\"signer\"", "\"Signer\"");
    let ambiguous = METADATA.replace(
        "\"signer\":\"dcl:explorer\"",
        "\"signer\":\"dcl:explorer\",\"Signer\":\"other\"",
    );

    let cases: Vec<(&str, HeaderMap, &[&str], Option<&SignerGate>)> = vec![
        (
            "empty path segment",
            headers_for(Shape::V6, METADATA, METADATA, now),
            &["realm..serverName"],
            None,
        ),
        (
            "expected \"signer\"",
            legacy_headers(&recased),
            SCENE_KEYS,
            None,
        ),
        ("spellings", legacy_headers(&ambiguous), SCENE_KEYS, None),
        (
            "invalid metadata content",
            headers_for(Shape::Legacy, &scene, &scene, now),
            SCENE_KEYS,
            Some(&gate),
        ),
        (
            "invalid chain metadata: \"not json\"",
            headers_for(Shape::Legacy, "not json", "not json", now),
            SCENE_KEYS,
            None,
        ),
    ];

    let mut rendered = Vec::new();
    for (needle, headers, keys, metadata_gate) in cases {
        let err = verify(&headers, keys, metadata_gate).await.unwrap_err();
        assert!(err.is_bad_request(), "{needle}: {err:?}");
        let ApiError::Http { status, message } = ApiError::from(err) else {
            panic!("{needle}: expected ApiError::Http");
        };
        assert_eq!(status, 400, "{needle}");
        assert!(message.contains(needle), "{needle}: {message}");
        rendered.push(message);
    }

    let built = rendered.len();
    rendered.sort();
    rendered.dedup();
    assert_eq!(rendered.len(), built, "{rendered:?}");
}

#[tokio::test]
async fn the_signer_gate_passes_a_request_declaring_no_signer() {
    let gate = reject_if_signer(&[SCENE_SIGNER]).unwrap();
    let raw = "{\"intent\":\"dcl:explorer:comms-handshake\"}";
    let now = chrono::Utc::now().timestamp_millis();

    let legacy = headers_for(Shape::Legacy, raw, raw, now);
    assert!(verify(&legacy, SCENE_KEYS, Some(&gate)).await.is_ok());

    let current = headers_for(Shape::V6, raw, raw, now);
    assert!(verify(&current, &[], Some(&gate)).await.is_ok());
}
