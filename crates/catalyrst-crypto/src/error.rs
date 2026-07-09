use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("Malformed auth chain: {0}")]
    MalformedChain(String),

    #[error("Missing signature on {link_type} link at index {index}")]
    MissingSignature { link_type: String, index: usize },

    #[error("Signature recovery failed: {0}")]
    RecoveryFailed(String),

    #[error("Invalid signer address at link {index}. Expected: {expected}. Actual: {actual}")]
    SignerMismatch {
        index: usize,
        expected: String,
        actual: String,
    },

    #[error("Ephemeral key expired. Expiration: {expiration_ms}. Checked against: {now_ms}")]
    EphemeralExpired { expiration_ms: i64, now_ms: i64 },

    #[error("Invalid ephemeral payload: {0}")]
    InvalidEphemeralPayload(String),

    #[error("Invalid final authority. Expected: {expected}. Got: {actual}")]
    FinalAuthorityMismatch { expected: String, actual: String },

    #[error("EIP-1654 smart-contract wallet verification is not implemented (requires RPC)")]
    Eip1654NotImplemented,

    #[error("EIP-1654 validation failed: {0}")]
    Eip1654ValidationFailed(String),

    #[error("EIP-1654 contract at {contract} rejected the signature")]
    Eip1654Rejected { contract: String },
}
