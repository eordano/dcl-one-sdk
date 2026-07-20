use serde::{Deserialize, Serialize};

use crate::entity::{
    DeploymentField, DeploymentId, EntityId, EntityType, EntityVersion, EthAddress, Pointer,
    Timestamp,
};
use crate::sorting::{SortingField, SortingOrder};

pub const MAX_AUTH_CHAIN_LINKS: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AuthLinkType {
    SIGNER,
    #[serde(rename = "ECDSA_EPHEMERAL")]
    EcdsaEphemeral,
    #[serde(rename = "ECDSA_SIGNED_ENTITY")]
    EcdsaSignedEntity,
    #[serde(rename = "ECDSA_EIP_1654_EPHEMERAL")]
    EcdsaEip1654Ephemeral,
    #[serde(rename = "ECDSA_EIP_1654_SIGNED_ENTITY")]
    EcdsaEip1654SignedEntity,
}

impl std::fmt::Display for AuthLinkType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SIGNER => write!(f, "SIGNER"),
            Self::EcdsaEphemeral => write!(f, "ECDSA_EPHEMERAL"),
            Self::EcdsaSignedEntity => write!(f, "ECDSA_SIGNED_ENTITY"),
            Self::EcdsaEip1654Ephemeral => write!(f, "ECDSA_EIP_1654_EPHEMERAL"),
            Self::EcdsaEip1654SignedEntity => write!(f, "ECDSA_EIP_1654_SIGNED_ENTITY"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthLink {
    #[serde(rename = "type")]
    pub link_type: AuthLinkType,

    pub payload: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

pub type AuthChain = Vec<AuthLink>;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentFilters {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<Timestamp>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<Timestamp>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deployed_by: Option<Vec<EthAddress>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_types: Option<Vec<EntityType>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_ids: Option<Vec<EntityId>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pointers: Option<Vec<Pointer>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub only_currently_pointed: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentSorting {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<SortingField>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<SortingOrder>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentRequestOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filters: Option<DeploymentFilters>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_by: Option<DeploymentSorting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentOptions {
    #[serde(flatten)]
    pub request: DeploymentRequestOptions,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<Vec<DeploymentField>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_denylisted: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PointerChangesOptions {
    #[serde(flatten)]
    pub request: DeploymentRequestOptions,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_auth_chain: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentBase {
    pub entity_version: EntityVersion,
    pub entity_type: EntityType,
    pub entity_id: EntityId,
    pub entity_timestamp: Timestamp,
    pub deployed_by: EthAddress,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentContent {
    pub key: String,
    pub hash: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditInfo {
    pub version: EntityVersion,
    pub auth_chain: AuthChain,
    pub local_timestamp: Timestamp,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overwritten_by: Option<EntityId>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_denylisted: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub denylisted_content: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalDeploymentAuditInfo {
    pub auth_chain: AuthChain,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Deployment {
    pub entity_version: EntityVersion,
    pub entity_type: EntityType,
    pub entity_id: EntityId,
    pub entity_timestamp: Timestamp,
    pub deployed_by: EthAddress,

    pub pointers: Vec<Pointer>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<Vec<DeploymentContent>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,

    pub audit_info: AuditInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PartialDeploymentHistory<T> {
    pub deployments: Vec<T>,
    pub filters: DeploymentFilters,
    pub pagination: HistoryPagination,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPagination {
    pub offset: i64,
    pub limit: i64,
    pub more_data: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvalidResult {
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DeploymentResult {
    Success(DeploymentId),
    Invalid(InvalidResult),
}

impl DeploymentResult {
    pub fn is_success(&self) -> bool {
        matches!(self, DeploymentResult::Success(_))
    }

    pub fn is_invalid(&self) -> bool {
        matches!(self, DeploymentResult::Invalid(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DeploymentContext {
    Local,
    Synced,
    SyncedLegacyEntity,
    FixAttempt,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoricalDeployment {
    pub deployment_id: DeploymentId,
    pub entity_id: EntityId,
    pub entity_type: String,
    pub pointers: Vec<Pointer>,
    pub auth_chain: AuthChain,
    pub entity_timestamp: Timestamp,
    pub local_timestamp: Timestamp,
    pub metadata: Option<serde_json::Value>,
    pub deployer_address: EthAddress,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overwritten_by: Option<EntityId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoricalDeploymentsRow {
    pub id: DeploymentId,
    pub deployer_address: EthAddress,
    pub version: String,
    pub entity_type: String,
    pub entity_id: EntityId,
    pub entity_metadata: Option<serde_json::Value>,
    pub entity_timestamp: Timestamp,
    pub entity_pointers: Vec<Pointer>,
    pub local_timestamp: Timestamp,
    pub auth_chain: AuthChain,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleter_deployment: Option<DeploymentId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overwritten_by: Option<EntityId>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_link_type_roundtrip() {
        let json = serde_json::to_string(&AuthLinkType::EcdsaEphemeral).unwrap();
        assert_eq!(json, "\"ECDSA_EPHEMERAL\"");
        let back: AuthLinkType = serde_json::from_str(&json).unwrap();
        assert_eq!(back, AuthLinkType::EcdsaEphemeral);
    }

    #[test]
    fn auth_link_type_display() {
        assert_eq!(AuthLinkType::SIGNER.to_string(), "SIGNER");
        assert_eq!(AuthLinkType::EcdsaEphemeral.to_string(), "ECDSA_EPHEMERAL");
        assert_eq!(
            AuthLinkType::EcdsaSignedEntity.to_string(),
            "ECDSA_SIGNED_ENTITY"
        );
        assert_eq!(
            AuthLinkType::EcdsaEip1654Ephemeral.to_string(),
            "ECDSA_EIP_1654_EPHEMERAL"
        );
        assert_eq!(
            AuthLinkType::EcdsaEip1654SignedEntity.to_string(),
            "ECDSA_EIP_1654_SIGNED_ENTITY"
        );
    }

    #[test]
    fn auth_link_signer_serialization() {
        let link = AuthLink {
            link_type: AuthLinkType::SIGNER,
            payload: "0xabc123".to_string(),
            signature: None,
        };
        let json = serde_json::to_value(&link).unwrap();
        assert_eq!(json["type"], "SIGNER");
        assert_eq!(json["payload"], "0xabc123");
        assert!(json.get("signature").is_none());
    }

    #[test]
    fn deployment_context_roundtrip() {
        let json = serde_json::to_string(&DeploymentContext::SyncedLegacyEntity).unwrap();
        assert_eq!(json, "\"SYNCED_LEGACY_ENTITY\"");
        let back: DeploymentContext = serde_json::from_str(&json).unwrap();
        assert_eq!(back, DeploymentContext::SyncedLegacyEntity);
    }

    #[test]
    fn deployment_filters_default_is_empty() {
        let f = DeploymentFilters::default();
        let json = serde_json::to_string(&f).unwrap();
        assert_eq!(json, "{}");
    }

    #[test]
    fn deployment_result_helpers() {
        let ok = DeploymentResult::Success(42);
        assert!(ok.is_success());
        assert!(!ok.is_invalid());

        let err = DeploymentResult::Invalid(InvalidResult {
            errors: vec!["bad pointer".into()],
        });
        assert!(err.is_invalid());
        assert!(!err.is_success());
    }
}
