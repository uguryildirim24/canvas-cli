//! Assignment groups.

use serde::Deserialize;

use crate::serde_util::{deserialize_id, deserialize_opt_id};

/// Drop rules for an assignment group.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct AssignmentGroupRules {
    pub drop_lowest: Option<u32>,
    pub drop_highest: Option<u32>,
    pub never_drop: Option<Vec<i64>>,
}

/// Assignment group with optional embedded assignments.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct AssignmentGroup {
    #[serde(deserialize_with = "deserialize_id")]
    pub id: i64,
    pub name: Option<String>,
    pub position: Option<i64>,
    pub group_weight: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_opt_id")]
    pub course_id: Option<i64>,
    pub rules: Option<AssignmentGroupRules>,
    pub assignments: Option<Vec<serde_json::Value>>,
}
