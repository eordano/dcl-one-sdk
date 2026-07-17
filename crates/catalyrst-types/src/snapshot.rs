use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::{AuthChain, EntityId, Pointer, Timestamp};

fn de_timestamp<'de, D>(d: D) -> Result<Timestamp, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let n = serde_json::Number::deserialize(d)?;
    n.as_i64()
        .or_else(|| n.as_f64().map(|f| f as i64))
        .ok_or_else(|| serde::de::Error::custom("invalid timestamp number"))
}

fn de_timestamp_opt<'de, D>(d: D) -> Result<Option<Timestamp>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<serde_json::Number>::deserialize(d)?;
    match opt {
        None => Ok(None),
        Some(n) => n
            .as_i64()
            .or_else(|| n.as_f64().map(|f| f as i64))
            .map(Some)
            .ok_or_else(|| serde::de::Error::custom("invalid timestamp number")),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncDeployment {
    pub entity_id: EntityId,
    pub entity_type: String,
    pub pointers: Vec<Pointer>,
    pub auth_chain: AuthChain,
    #[serde(deserialize_with = "de_timestamp")]
    pub entity_timestamp: Timestamp,
    #[serde(
        default,
        deserialize_with = "de_timestamp_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub local_timestamp: Option<Timestamp>,
}

const MAX_DECOMPRESSED_SNAPSHOT_BYTES: u64 = 4 * 1024 * 1024 * 1024;

pub fn decompress_snapshot(data: &[u8]) -> std::borrow::Cow<'_, [u8]> {
    if data.len() >= 2 && data[0] == 0x1f && data[1] == 0x8b {
        use std::io::Read;

        let mut decoder =
            flate2::read::GzDecoder::new(data).take(MAX_DECOMPRESSED_SNAPSHOT_BYTES + 1);
        let mut buf = Vec::new();
        match decoder.read_to_end(&mut buf) {
            Ok(_) => {
                if buf.len() as u64 > MAX_DECOMPRESSED_SNAPSHOT_BYTES {
                    warn!(
                        bytes = buf.len(),
                        cap = MAX_DECOMPRESSED_SNAPSHOT_BYTES,
                        "Snapshot decompressed to > cap, refusing"
                    );
                    return std::borrow::Cow::Owned(Vec::new());
                }
                return std::borrow::Cow::Owned(buf);
            }
            Err(e) => {
                warn!(error = %e, "Failed to decompress snapshot");
            }
        }
    }
    std::borrow::Cow::Borrowed(data)
}

pub fn parse_snapshot_entities(data: &[u8]) -> Vec<SyncDeployment> {
    let text_bytes = decompress_snapshot(data);
    let text = String::from_utf8_lossy(&text_bytes);
    let mut deployments = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('{') && trimmed.ends_with('}') {
            match serde_json::from_str::<SyncDeployment>(trimmed) {
                Ok(deployment) => deployments.push(deployment),
                Err(e) => {
                    warn!(
                        error = %e,
                        line_preview = &trimmed[..trimmed.len().min(100)],
                        "Invalid deployment in snapshot file, skipping"
                    );
                }
            }
        }
    }

    deployments
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_snapshot_entities_basic() {
        let data = b"### Decentraland json snapshot\n\
{\"entityId\":\"abc\",\"entityType\":\"scene\",\"pointers\":[\"0,0\"],\"authChain\":[{\"type\":\"SIGNER\",\"payload\":\"0xabc\"}],\"entityTimestamp\":1000}\n\
{\"entityId\":\"def\",\"entityType\":\"profile\",\"pointers\":[\"0xdef\"],\"authChain\":[{\"type\":\"SIGNER\",\"payload\":\"0xdef\"}],\"entityTimestamp\":2000}\n";

        let deployments = parse_snapshot_entities(data);
        assert_eq!(deployments.len(), 2);
        assert_eq!(deployments[0].entity_id, "abc");
        assert_eq!(deployments[1].entity_id, "def");
    }

    #[test]
    fn test_parse_snapshot_entities_skips_invalid() {
        let data = b"### header\n\
{\"entityId\":\"abc\",\"entityType\":\"scene\",\"pointers\":[],\"authChain\":[],\"entityTimestamp\":1000}\n\
not valid json\n\
{\"entityId\":\"def\",\"entityType\":\"profile\",\"pointers\":[],\"authChain\":[],\"entityTimestamp\":2000}\n";

        let deployments = parse_snapshot_entities(data);
        assert_eq!(deployments.len(), 2);
    }

    #[test]
    fn test_parse_snapshot_empty() {
        let deployments = parse_snapshot_entities(b"### Decentraland json snapshot\n");
        assert!(deployments.is_empty());
    }
}
