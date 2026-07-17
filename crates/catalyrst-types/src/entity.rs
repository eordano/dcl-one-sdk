use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

pub type EntityId = String;

pub type Pointer = String;

pub type ContentFileHash = String;

pub type EthAddress = String;

pub type DeploymentId = i32;

pub type Timestamp = i64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntityType {
    Scene,
    Profile,
    Wearable,
    Store,
    Emote,
    Outfits,
}

impl EntityType {
    pub const ALL: &'static [&'static str] =
        &["scene", "profile", "wearable", "store", "emote", "outfits"];

    pub fn parse(s: &str) -> Option<EntityType> {
        let raw = s.trim().to_lowercase();
        if let Some(t) = Self::match_exact(&raw) {
            return Some(t);
        }
        if raw.ends_with('s') {
            let stripped = &raw[..raw.len() - 1];
            return Self::match_exact(stripped);
        }
        None
    }

    fn match_exact(s: &str) -> Option<EntityType> {
        match s {
            "scene" => Some(EntityType::Scene),
            "profile" => Some(EntityType::Profile),
            "wearable" => Some(EntityType::Wearable),
            "store" => Some(EntityType::Store),
            "emote" => Some(EntityType::Emote),
            "outfits" => Some(EntityType::Outfits),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            EntityType::Scene => "scene",
            EntityType::Profile => "profile",
            EntityType::Wearable => "wearable",
            EntityType::Store => "store",
            EntityType::Emote => "emote",
            EntityType::Outfits => "outfits",
        }
    }
}

impl std::fmt::Display for EntityType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EntityVersion {
    #[serde(rename = "v2")]
    V2,
    #[serde(rename = "v3")]
    V3,
    #[serde(rename = "v4")]
    V4,
}

impl EntityVersion {
    pub fn as_str(&self) -> &'static str {
        match self {
            EntityVersion::V2 => "v2",
            EntityVersion::V3 => "v3",
            EntityVersion::V4 => "v4",
        }
    }
}

impl std::fmt::Display for EntityVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentMapping {
    pub file: String,
    pub hash: ContentFileHash,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entity {
    pub version: String,

    pub id: EntityId,

    #[serde(rename = "type")]
    pub entity_type: EntityType,

    pub pointers: Vec<Pointer>,

    pub timestamp: Timestamp,

    pub content: Vec<ContentMapping>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DeploymentField {
    Content,
    Pointers,
    Metadata,
    AuditInfo,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pagination {
    pub offset: i64,
    pub limit: i64,
    pub page_size: i64,
    pub page_num: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatusProbeResult {
    pub name: String,
    pub data: serde_json::Map<String, serde_json::Value>,
}

pub const PROFILE_DURATION_MS: i64 = 365 * 24 * 60 * 60 * 1000;

pub fn timestamp_ms_to_naive(ms: Timestamp) -> Option<NaiveDateTime> {
    chrono::DateTime::from_timestamp_millis(ms).map(|dt| dt.naive_utc())
}

pub fn naive_to_timestamp_ms(dt: NaiveDateTime) -> Timestamp {
    dt.and_utc().timestamp_millis()
}

pub fn is_eth_address(value: &str) -> bool {
    value.len() == 42
        && value.starts_with("0x")
        && value[2..].bytes().all(|b| b.is_ascii_hexdigit())
}

pub fn parse_eth_address(value: &str) -> Option<EthAddress> {
    if is_eth_address(value) {
        Some(value.to_lowercase())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_entity_type_strips_trailing_s() {
        assert_eq!(EntityType::parse("profiles"), Some(EntityType::Profile));
        assert_eq!(EntityType::parse("SCENES"), Some(EntityType::Scene));
        assert_eq!(EntityType::parse("Wearable"), Some(EntityType::Wearable));
    }

    #[test]
    fn parse_entity_type_outfits_matches_directly() {
        assert_eq!(EntityType::parse("outfits"), Some(EntityType::Outfits));
        assert_eq!(EntityType::parse("OUTFITS"), Some(EntityType::Outfits));
    }

    #[test]
    fn parse_entity_type_unknown_returns_none() {
        assert_eq!(EntityType::parse("bogus"), None);
        assert_eq!(EntityType::parse(""), None);
    }

    #[test]
    fn entity_type_roundtrip() {
        let json = serde_json::to_string(&EntityType::Scene).unwrap();
        assert_eq!(json, "\"scene\"");
        let back: EntityType = serde_json::from_str(&json).unwrap();
        assert_eq!(back, EntityType::Scene);
    }

    #[test]
    fn entity_version_roundtrip() {
        let json = serde_json::to_string(&EntityVersion::V3).unwrap();
        assert_eq!(json, "\"v3\"");
        let back: EntityVersion = serde_json::from_str(&json).unwrap();
        assert_eq!(back, EntityVersion::V3);
    }

    #[test]
    fn timestamp_conversion_roundtrip() {
        let ms: Timestamp = 1_700_000_000_000;
        let dt = timestamp_ms_to_naive(ms).unwrap();
        let back = naive_to_timestamp_ms(dt);
        assert_eq!(back, ms);
    }

    #[test]
    fn is_eth_address_accepts_lowercase_and_mixed_case() {
        assert!(is_eth_address("0x0000000000000000000000000000000000000000"));
        assert!(is_eth_address("0xabcdefABCDEF0123456789abcdefABCDEF012345"));
    }

    #[test]
    fn is_eth_address_rejects_bad_inputs() {
        assert!(!is_eth_address("0x0"));
        assert!(!is_eth_address(
            "00000000000000000000000000000000000000000000"
        ));
        assert!(!is_eth_address(
            "0xZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ"
        ));
        assert!(!is_eth_address("0x000000000000000000000000000000000000000"));
        assert!(!is_eth_address(""));
    }

    #[test]
    fn parse_eth_address_lowercases() {
        let parsed = parse_eth_address("0xABCDEF0123456789ABCDEF0123456789ABCDEF01").unwrap();
        assert_eq!(parsed, "0xabcdef0123456789abcdef0123456789abcdef01");
    }

    #[test]
    fn parse_eth_address_returns_none_on_invalid() {
        assert!(parse_eth_address("not-an-address").is_none());
    }
}
