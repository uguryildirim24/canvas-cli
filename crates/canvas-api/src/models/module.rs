//! Modules and module items.

use serde::Deserialize;
use serde_json::Value;

use crate::serde_util::{deserialize_id, deserialize_opt_id};

/// Content details on a module item.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct ModuleItemContentDetails {
    pub locked_for_user: Option<bool>,
    pub lock_explanation: Option<String>,
    pub lock_info: Option<Value>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub points_possible: crate::serde_util::Supplied<f64>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub due_at: crate::serde_util::Supplied<jiff::Timestamp>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub unlock_at: crate::serde_util::Supplied<jiff::Timestamp>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub lock_at: crate::serde_util::Supplied<jiff::Timestamp>,
}

/// One module item.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct ModuleItem {
    #[serde(deserialize_with = "deserialize_id")]
    pub id: i64,
    #[serde(default, deserialize_with = "deserialize_opt_id")]
    pub module_id: Option<i64>,
    pub title: Option<String>,
    pub position: Option<i64>,
    pub indent: Option<i64>,
    #[serde(rename = "type")]
    pub item_type: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_id")]
    pub content_id: Option<i64>,
    #[serde(
        default,
        deserialize_with = "crate::serde_util::deserialize_supplied_url"
    )]
    pub html_url: crate::serde_util::Supplied<reqwest::Url>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_opt_url")]
    pub url: Option<reqwest::Url>,
    pub page_url: Option<String>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_opt_url")]
    pub external_url: Option<reqwest::Url>,
    pub new_tab: Option<bool>,
    pub completion_requirement: Option<Value>,
    pub content_details: Option<ModuleItemContentDetails>,
    pub published: Option<bool>,
}

/// Course module.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Module {
    #[serde(deserialize_with = "deserialize_id")]
    pub id: i64,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub name: crate::serde_util::Supplied<String>,
    pub position: Option<i64>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub unlock_at: crate::serde_util::Supplied<jiff::Timestamp>,
    pub require_sequential_progress: Option<bool>,
    pub publish_final_grade: Option<bool>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_opt_ids")]
    pub prerequisite_module_ids: Option<Vec<i64>>,
    pub state: Option<String>,
    #[serde(
        default,
        deserialize_with = "crate::serde_util::deserialize_opt_timestamp"
    )]
    pub completed_at: Option<jiff::Timestamp>,
    pub items_count: Option<u64>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_opt_url")]
    pub items_url: Option<reqwest::Url>,
    pub items: Option<Vec<ModuleItem>>,
    pub published: Option<bool>,
}
