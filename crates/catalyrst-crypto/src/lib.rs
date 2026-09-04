pub mod auth_chain;
pub mod eip1654;
pub mod eip712;
pub mod error;
pub mod metadata_gate;
pub mod recover;
pub mod rpc_validator;
pub mod sign;
pub mod signed_fetch;
pub mod signer;
pub mod validation_cache;
pub mod verify;

pub use auth_chain::{AuthChain, AuthLink, AuthLinkType};
pub use eip1654::{verify_eip1654, Eip1654Validator};
pub use eip712::{
    domain_separator, domain_separator_salted, hash_array_of_structs, hash_dynamic, struct_hash,
    typed_data_digest, word_address, word_u256, word_u64,
};
pub use error::AuthError;
pub use metadata_gate::{
    field_is_canonical, reject_if_signer, require_canonical_field, require_signer, FieldGateError,
    RequiredFieldGate, SignerGate, SignerGateError,
};
pub use rpc_validator::RpcEip1654Validator;
pub use sign::{create_simple_auth_chain, SignError, Wallet};
pub use signed_fetch::{
    build_legacy_payload, build_payload, build_payload_v6, check_symmetric_skew,
    default_eip1654_validator, AttemptOrder, SignedFetchPolicy,
};
pub use signer::Signer;
pub use validation_cache::ValidationCache;
