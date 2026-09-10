//! Planner items.

use jiff::Timestamp;
use serde::Deserialize;
use serde_json::Value;

use crate::serde_util::{deserialize_opt_id, deserialize_opt_timestamp};

/// Nested plannable payload (assignment, event, note, …).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Plannable {
    #[serde(default, deserialize_with = "deserialize_opt_id")]
    pub id: Option<i64>,
    pub title: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_id")]
    pub course_id: Option<i64>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub points_possible: crate::serde_util::Supplied<f64>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub due_at: crate::serde_util::Supplied<Timestamp>,
    #[serde(default, deserialize_with = "deserialize_opt_id")]
    pub assignment_id: Option<i64>,
    #[serde(default, deserialize_with = "deserialize_opt_id")]
    pub parent_assignment_id: Option<i64>,
    #[serde(default, deserialize_with = "deserialize_opt_timestamp")]
    pub todo_date: Option<Timestamp>,
    #[serde(default, deserialize_with = "deserialize_opt_timestamp")]
    pub start_at: Option<Timestamp>,
    #[serde(default, deserialize_with = "deserialize_opt_timestamp")]
    pub end_at: Option<Timestamp>,
    pub all_day: Option<bool>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_opt_date")]
    pub all_day_date: Option<jiff::civil::Date>,
    /// Preserve payload fields for unrecognized planner kinds.
    #[serde(flatten)]
    pub extra: std::collections::BTreeMap<String, Value>,
}

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
    #[serde(
        default,
        deserialize_with = "crate::serde_util::deserialize_supplied_url"
    )]
    pub html_url: crate::serde_util::Supplied<reqwest::Url>,
    pub new_activity: Option<bool>,
}
