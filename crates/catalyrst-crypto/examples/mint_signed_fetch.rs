use alloy::signers::{local::PrivateKeySigner, Signer};
use chrono::{Duration, Utc};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let method = args.get(1).map(String::as_str).unwrap_or("get");
    let path = args.get(2).map(String::as_str).unwrap_or("/notifications");
    let metadata = args.get(3).map(String::as_str).unwrap_or("{}");

    let root = PrivateKeySigner::random();
    let ephemeral = PrivateKeySigner::random();
    let root_address = format!("{:#x}", root.address());
    let ephemeral_address = format!("{:#x}", ephemeral.address());

    let expiration = (Utc::now() + Duration::days(1)).format("%Y-%m-%dT%H:%M:%S%.3fZ");
    let ephemeral_payload = format!(
        "Decentraland Login\nEphemeral address: {ephemeral_address}\nExpiration: {expiration}"
    );
    let ephemeral_sig = root
        .sign_message(ephemeral_payload.as_bytes())
        .await
        .unwrap();

    let timestamp = Utc::now().timestamp_millis().to_string();
    let signed_fetch_payload = format!("{method}:{path}:{timestamp}:{metadata}").to_lowercase();
    let entity_sig = ephemeral
        .sign_message(signed_fetch_payload.as_bytes())
        .await
        .unwrap();

    let chain = [
        serde_json::json!({"type": "SIGNER", "payload": root_address, "signature": ""}),
        serde_json::json!({
            "type": "ECDSA_EPHEMERAL",
            "payload": ephemeral_payload,
            "signature": ephemeral_sig.to_string(),
        }),
        serde_json::json!({
            "type": "ECDSA_SIGNED_ENTITY",
            "payload": signed_fetch_payload,
            "signature": entity_sig.to_string(),
        }),
    ];

    println!("PAYLOAD={signed_fetch_payload}");
    println!("SIGNER={root_address}");
    for (i, link) in chain.iter().enumerate() {
        println!(
            "HDR{i}={}",
            serde_json::to_string(&link.to_string()).unwrap()
        );
    }
    println!("TS={timestamp}");
    println!("META={metadata}");
}
