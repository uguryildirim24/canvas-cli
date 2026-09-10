//! Planner items.

use jiff::Timestamp;
use serde::Deserialize;
use serde_json::Value;

use crate::serde_util::{deserialize_opt_id, deserialize_opt_timestamp};

/// Nested plannable payload (assignment, event, note, …).
pub type Plannable = Value;

/// Planner override flags.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct PlannerOverride {
    #[serde(default, deserialize_with = "deserialize_opt_id")]
    pub id: Option<i64>,
    pub plannable_type: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_id")]
    pub plannable_id: Option<i64>,
    pub marked_complete: Option<bool>,
    pub dismissed: Option<bool>,
}

/// One planner item.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct PlannerItem {
    #[serde(default, deserialize_with = "deserialize_opt_id")]
    pub course_id: Option<i64>,
    pub context_type: Option<String>,
    pub context_name: Option<String>,
    pub plannable_type: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_id")]
    pub plannable_id: Option<i64>,
    pub plannable: Option<Plannable>,
    #[serde(default, deserialize_with = "deserialize_opt_timestamp")]
    pub plannable_date: Option<Timestamp>,
    pub planner_override: Option<PlannerOverride>,
    pub submissions: Option<Value>,
    pub html_url: Option<String>,
    pub new_activity: Option<bool>,
}
