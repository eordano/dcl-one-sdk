use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::deployment::MAX_AUTH_CHAIN_LINKS;

pub const MAX_DEPLOY_FILES: usize = 1000;
pub const MAX_DEPLOY_FILE_BYTES: usize = 50 * 1024 * 1024;

pub fn extract_auth_chain_from_fields(
    fields: &BTreeMap<String, String>,
) -> Result<Option<Value>, String> {
    if let Some(chain_str) = fields.get("authChain") {
        let chain: Value =
            serde_json::from_str(chain_str).map_err(|_| "Invalid auth chain".to_string())?;
        let arr = chain
            .as_array()
            .ok_or_else(|| "Invalid auth chain".to_string())?;
        if arr.len() > MAX_AUTH_CHAIN_LINKS {
            return Err(too_long_message());
        }
        return Ok(Some(chain));
    }

    let mut biggest_index: i64 = -1;
    for key in fields.keys() {
        if let Some(rest) = key.strip_prefix("authChain[") {
            if let Some(idx_str) = rest.split(']').next() {
                if let Ok(idx) = idx_str.parse::<i64>() {
                    if idx > biggest_index {
                        biggest_index = idx;
                    }
                }
            }
        }
    }

    if biggest_index == -1 {
        return Ok(None);
    }
    if biggest_index >= MAX_AUTH_CHAIN_LINKS as i64 {
        return Err(too_long_message());
    }

    let mut chain = Vec::new();
    for i in 0..=biggest_index {
        let payload = indexed_field(fields, i, "payload")?;
        let signature = indexed_field(fields, i, "signature")?;
        let link_type = indexed_field(fields, i, "type")?;
        chain.push(json!({
            "type": link_type,
            "payload": payload,
            "signature": signature,
        }));
    }
    Ok(Some(Value::Array(chain)))
}

fn indexed_field<'a>(
    fields: &'a BTreeMap<String, String>,
    i: i64,
    part: &str,
) -> Result<&'a String, String> {
    fields
        .get(&format!("authChain[{i}][{part}]"))
        .ok_or_else(|| format!("Missing auth chain element at index {i}"))
}

fn too_long_message() -> String {
    format!("Auth chain is too long; the maximum allowed is {MAX_AUTH_CHAIN_LINKS} elements")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link_fields(count: usize) -> BTreeMap<String, String> {
        let mut f = BTreeMap::new();
        for i in 0..count {
            f.insert(format!("authChain[{i}][type]"), "SIGNER".to_string());
            f.insert(format!("authChain[{i}][payload]"), format!("0xp{i}"));
            f.insert(format!("authChain[{i}][signature]"), format!("0xs{i}"));
        }
        f
    }

    #[test]
    fn parses_json_field() {
        let mut f = BTreeMap::new();
        f.insert(
            "authChain".to_string(),
            r#"[{"type":"SIGNER","payload":"0xabc","signature":""}]"#.to_string(),
        );
        let chain = extract_auth_chain_from_fields(&f).unwrap().unwrap();
        assert_eq!(chain.as_array().unwrap().len(), 1);
    }

    #[test]
    fn parses_indexed_fields() {
        let chain = extract_auth_chain_from_fields(&link_fields(2))
            .unwrap()
            .unwrap();
        let arr = chain.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[1]["payload"], "0xp1");
    }

    #[test]
    fn absent_chain_is_none() {
        assert_eq!(extract_auth_chain_from_fields(&BTreeMap::new()), Ok(None));
    }

    #[test]
    fn rejects_invalid_json() {
        let mut f = BTreeMap::new();
        f.insert("authChain".to_string(), "not-json".to_string());
        assert_eq!(
            extract_auth_chain_from_fields(&f),
            Err("Invalid auth chain".to_string())
        );
        f.insert("authChain".to_string(), r#"{"a":1}"#.to_string());
        assert_eq!(
            extract_auth_chain_from_fields(&f),
            Err("Invalid auth chain".to_string())
        );
    }

    #[test]
    fn rejects_over_length_chains_on_both_paths() {
        let err =
            extract_auth_chain_from_fields(&link_fields(MAX_AUTH_CHAIN_LINKS + 1)).unwrap_err();
        assert!(err.starts_with("Auth chain is too long"), "{err}");

        let mut f = BTreeMap::new();
        let long: Vec<Value> = (0..=MAX_AUTH_CHAIN_LINKS)
            .map(|_| json!({"type":"SIGNER","payload":"p","signature":"s"}))
            .collect();
        f.insert(
            "authChain".to_string(),
            serde_json::to_string(&long).unwrap(),
        );
        let err = extract_auth_chain_from_fields(&f).unwrap_err();
        assert!(err.starts_with("Auth chain is too long"), "{err}");
    }

    #[test]
    fn rejects_missing_indexed_element() {
        let mut f = link_fields(2);
        f.remove("authChain[0][signature]");
        assert_eq!(
            extract_auth_chain_from_fields(&f),
            Err("Missing auth chain element at index 0".to_string())
        );
    }
}
