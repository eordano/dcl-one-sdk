use chrono::{Duration, Utc};
use ethers_core::utils::keccak256;
use ethers_signers::{LocalWallet, Signer};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let method = args.get(1).map(String::as_str).unwrap_or("get");
    let path = args.get(2).map(String::as_str).unwrap_or("/cart");
    let metadata = args.get(3).map(String::as_str).unwrap_or("{}");

    let root_hex = std::env::var("LANDILER_ROOT_KEY")
        .expect("set LANDILER_ROOT_KEY=<64 hex chars> (the stable root private key)");
    let root_bytes = hex::decode(root_hex.trim().trim_start_matches("0x"))
        .expect("LANDILER_ROOT_KEY must be hex");
    let root = LocalWallet::from_bytes(&root_bytes).expect("invalid root key");

    let eph_bytes = keccak256(&root_bytes);
    let ephemeral = LocalWallet::from_bytes(&eph_bytes).expect("invalid ephemeral key");

    let root_address = format!("{:#x}", root.address());
    let ephemeral_address = format!("{:#x}", ephemeral.address());

    let expiration = (Utc::now() + Duration::days(7)).format("%Y-%m-%dT%H:%M:%S%.3fZ");
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

    let link0 = serde_json::json!({"type":"SIGNER","payload":root_address,"signature":""});
    let link1 = serde_json::json!({
        "type":"ECDSA_EPHEMERAL","payload":ephemeral_payload,"signature":format!("0x{ephemeral_sig}")});
    let link2 = serde_json::json!({
        "type":"ECDSA_SIGNED_ENTITY","payload":signed_fetch_payload,"signature":format!("0x{entity_sig}")});

    println!("ADDR={root_address}");
    println!("TS={timestamp}");
    println!("PAYLOAD={signed_fetch_payload}");
    println!("-H 'x-identity-auth-chain-0: {}'", link0);
    println!("-H 'x-identity-auth-chain-1: {}'", link1);
    println!("-H 'x-identity-auth-chain-2: {}'", link2);
    println!("-H 'x-identity-timestamp: {timestamp}'");
    println!("-H 'x-identity-metadata: {metadata}'");
}
