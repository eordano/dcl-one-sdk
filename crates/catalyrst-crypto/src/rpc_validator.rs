use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const DEFAULT_RPC_TIMEOUT: Duration = Duration::from_secs(30);

use crate::eip1654::Eip1654Validator;
use crate::error::AuthError;

const IS_VALID_SIGNATURE_SELECTOR: [u8; 4] = [0x16, 0x26, 0xba, 0x7e];

const EIP1271_MAGIC_VALUE: [u8; 4] = [0x16, 0x26, 0xba, 0x7e];

pub fn encode_is_valid_signature(hash: &[u8], signature: &[u8]) -> Vec<u8> {
    assert!(hash.len() == 32, "hash must be 32 bytes");

    let sig_len = signature.len();
    let padded_sig_len = sig_len.div_ceil(32) * 32;

    let mut buf = Vec::with_capacity(4 + 32 + 32 + 32 + padded_sig_len);

    buf.extend_from_slice(&IS_VALID_SIGNATURE_SELECTOR);

    buf.extend_from_slice(hash);

    let mut offset = [0u8; 32];
    offset[31] = 0x40;
    buf.extend_from_slice(&offset);

    let mut len_word = [0u8; 32];
    let len_bytes = (sig_len as u64).to_be_bytes();
    len_word[24..32].copy_from_slice(&len_bytes);
    buf.extend_from_slice(&len_word);

    buf.extend_from_slice(signature);
    buf.resize(buf.len() + padded_sig_len - sig_len, 0);

    buf
}

pub fn is_magic_return(hex_response: &str) -> bool {
    let hex = hex_response.strip_prefix("0x").unwrap_or(hex_response);
    if hex.len() < 8 {
        return false;
    }
    let Ok(first4) = hex::decode(&hex[..8]) else {
        return false;
    };
    first4.as_slice() == EIP1271_MAGIC_VALUE
}

#[derive(Serialize)]
struct JsonRpcRequest<'a> {
    jsonrpc: &'a str,
    method: &'a str,
    params: Value,
    id: u64,
}

#[derive(Deserialize)]
struct JsonRpcResponse {
    result: Option<Value>,
    error: Option<JsonRpcError>,
}

#[derive(Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

pub struct RpcEip1654Validator {
    rpc_url: String,
    client: Client,
}

impl RpcEip1654Validator {
    pub fn new(rpc_url: impl Into<String>) -> Self {
        Self::with_timeout(rpc_url, DEFAULT_RPC_TIMEOUT)
    }

    pub fn with_timeout(rpc_url: impl Into<String>, timeout: Duration) -> Self {
        let client = Client::builder()
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::limited(2))
            .build()
            .expect("reqwest client with timeout should build");
        Self {
            rpc_url: rpc_url.into(),
            client,
        }
    }

    pub fn with_client(rpc_url: impl Into<String>, client: Client) -> Self {
        Self {
            rpc_url: rpc_url.into(),
            client,
        }
    }

    async fn eth_call(&self, to: &str, data: &[u8]) -> Result<String, AuthError> {
        let call_data = format!("0x{}", hex::encode(data));

        let req = JsonRpcRequest {
            jsonrpc: "2.0",
            method: "eth_call",
            params: serde_json::json!([
                {
                    "to": to,
                    "data": call_data,
                },
                "latest"
            ]),
            id: 1,
        };

        let resp = self
            .client
            .post(&self.rpc_url)
            .json(&req)
            .send()
            .await
            .map_err(|e| AuthError::Eip1654ValidationFailed(format!("RPC request failed: {e}")))?;

        if !resp.status().is_success() {
            return Err(AuthError::Eip1654ValidationFailed(format!(
                "RPC returned HTTP {}",
                resp.status()
            )));
        }

        let body: JsonRpcResponse = resp.json().await.map_err(|e| {
            AuthError::Eip1654ValidationFailed(format!("Failed to parse RPC response: {e}"))
        })?;

        if let Some(err) = body.error {
            return Err(AuthError::Eip1654ValidationFailed(format!(
                "RPC error {}: {}",
                err.code, err.message
            )));
        }

        match body.result {
            Some(Value::String(s)) => Ok(s),
            Some(other) => Err(AuthError::Eip1654ValidationFailed(format!(
                "Unexpected RPC result type: {other}"
            ))),
            None => Err(AuthError::Eip1654ValidationFailed(
                "RPC returned null result with no error".into(),
            )),
        }
    }
}

#[async_trait]
impl Eip1654Validator for RpcEip1654Validator {
    async fn validate_signature(
        &self,
        contract_address: &str,
        hash: &[u8],
        signature: &[u8],
    ) -> Result<bool, AuthError> {
        if hash.len() != 32 {
            return Err(AuthError::Eip1654ValidationFailed(format!(
                "hash must be 32 bytes, got {}",
                hash.len()
            )));
        }

        let calldata = encode_is_valid_signature(hash, signature);
        let result_hex = self.eth_call(contract_address, &calldata).await?;

        Ok(is_magic_return(&result_hex))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_roundtrip() {
        let hash = [0xABu8; 32];
        let sig = vec![0x01, 0x02, 0x03];

        let encoded = encode_is_valid_signature(&hash, &sig);

        assert_eq!(encoded.len(), 4 + 32 + 32 + 32 + 32);

        assert_eq!(&encoded[0..4], &IS_VALID_SIGNATURE_SELECTOR);

        assert_eq!(&encoded[4..36], &hash);

        assert_eq!(encoded[35 + 32], 0x40);

        assert_eq!(encoded[4 + 32 + 32 + 31], 3);

        assert_eq!(&encoded[4 + 32 + 32 + 32..4 + 32 + 32 + 32 + 3], &[1, 2, 3]);

        assert!(encoded[4 + 32 + 32 + 32 + 3..].iter().all(|&b| b == 0));
    }

    #[test]
    fn encode_exact_32_byte_sig() {
        let hash = [0x00u8; 32];
        let sig = [0xFFu8; 32];

        let encoded = encode_is_valid_signature(&hash, &sig);

        assert_eq!(encoded.len(), 132);
    }

    #[test]
    fn encode_65_byte_sig() {
        let hash = [0x11u8; 32];
        let sig = [0xAAu8; 65];

        let encoded = encode_is_valid_signature(&hash, &sig);

        assert_eq!(encoded.len(), 4 + 32 + 32 + 32 + 96);

        assert_eq!(encoded[4 + 32 + 32 + 31], 65);
    }

    #[test]
    fn magic_value_detected() {
        let hex = "0x1626ba7e00000000000000000000000000000000000000000000000000000000";
        assert!(is_magic_return(hex));

        let hex = "1626ba7e00000000000000000000000000000000000000000000000000000000";
        assert!(is_magic_return(hex));
    }

    #[test]
    fn wrong_magic_rejected() {
        let hex = "0xffffffff00000000000000000000000000000000000000000000000000000000";
        assert!(!is_magic_return(hex));

        assert!(!is_magic_return("0x1626ba"));

        assert!(!is_magic_return("0x"));
    }

    #[test]
    fn zero_return_rejected() {
        let hex = "0x0000000000000000000000000000000000000000000000000000000000000000";
        assert!(!is_magic_return(hex));
    }
}
