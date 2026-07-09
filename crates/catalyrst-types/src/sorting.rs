use serde::{Deserialize, Serialize};

use crate::entity::Timestamp;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SortingField {
    #[serde(rename = "local_timestamp")]
    LocalTimestamp,
    #[serde(rename = "entity_timestamp")]
    EntityTimestamp,
}

impl SortingField {
    pub fn as_sql_column(&self) -> &'static str {
        match self {
            SortingField::LocalTimestamp => "local_timestamp",
            SortingField::EntityTimestamp => "entity_timestamp",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SortingOrder {
    #[serde(rename = "ASC")]
    Ascending,
    #[serde(rename = "DESC")]
    Descending,
}

impl SortingOrder {
    pub fn as_sql(&self) -> &'static str {
        match self {
            SortingOrder::Ascending => "ASC",
            SortingOrder::Descending => "DESC",
        }
    }
}

pub type DeploymentSortingField = SortingField;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityComparable {
    pub timestamp: Timestamp,
    pub entity_id: String,
}

pub trait IntoEntityComparable {
    fn to_comparable(&self) -> EntityComparable;
}

impl IntoEntityComparable for EntityComparable {
    fn to_comparable(&self) -> EntityComparable {
        self.clone()
    }
}

impl IntoEntityComparable for crate::entity::Entity {
    fn to_comparable(&self) -> EntityComparable {
        EntityComparable {
            timestamp: self.timestamp,
            entity_id: self.id.clone(),
        }
    }
}

impl IntoEntityComparable for crate::deployment::Deployment {
    fn to_comparable(&self) -> EntityComparable {
        EntityComparable {
            timestamp: self.entity_timestamp,
            entity_id: self.entity_id.clone(),
        }
    }
}

pub fn happened_before(a: &impl IntoEntityComparable, b: &impl IntoEntityComparable) -> bool {
    let a = a.to_comparable();
    let b = b.to_comparable();
    a.timestamp < b.timestamp
        || (a.timestamp == b.timestamp && a.entity_id.to_lowercase() < b.entity_id.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorting_field_roundtrip() {
        let json = serde_json::to_string(&SortingField::LocalTimestamp).unwrap();
        assert_eq!(json, "\"local_timestamp\"");
        let back: SortingField = serde_json::from_str(&json).unwrap();
        assert_eq!(back, SortingField::LocalTimestamp);
    }

    #[test]
    fn sorting_order_roundtrip() {
        let json = serde_json::to_string(&SortingOrder::Descending).unwrap();
        assert_eq!(json, "\"DESC\"");
        let back: SortingOrder = serde_json::from_str(&json).unwrap();
        assert_eq!(back, SortingOrder::Descending);
    }

    #[test]
    fn happened_before_by_timestamp() {
        let a = EntityComparable {
            timestamp: 100,
            entity_id: "bbb".into(),
        };
        let b = EntityComparable {
            timestamp: 200,
            entity_id: "aaa".into(),
        };
        assert!(happened_before(&a, &b));
        assert!(!happened_before(&b, &a));
    }

    #[test]
    fn happened_before_tiebreak_by_entity_id() {
        let a = EntityComparable {
            timestamp: 100,
            entity_id: "aaa".into(),
        };
        let b = EntityComparable {
            timestamp: 100,
            entity_id: "bbb".into(),
        };
        assert!(happened_before(&a, &b));
        assert!(!happened_before(&b, &a));
    }

    #[test]
    fn happened_before_case_insensitive() {
        let a = EntityComparable {
            timestamp: 100,
            entity_id: "AAA".into(),
        };
        let b = EntityComparable {
            timestamp: 100,
            entity_id: "bbb".into(),
        };
        assert!(happened_before(&a, &b));
    }

    #[test]
    fn happened_before_equal_is_false() {
        let a = EntityComparable {
            timestamp: 100,
            entity_id: "aaa".into(),
        };
        let b = EntityComparable {
            timestamp: 100,
            entity_id: "aaa".into(),
        };
        assert!(!happened_before(&a, &b));
    }
}
