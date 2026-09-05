use serde::{Deserialize, Serialize};

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
}
