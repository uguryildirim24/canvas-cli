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
    pub points_possible: Option<f64>,
    pub due_at: Option<String>,
    pub unlock_at: Option<String>,
    pub lock_at: Option<String>,
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
    pub html_url: Option<String>,
    pub url: Option<String>,
    pub page_url: Option<String>,
    pub external_url: Option<String>,
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
    pub name: Option<String>,
    pub position: Option<i64>,
    pub unlock_at: Option<String>,
    pub require_sequential_progress: Option<bool>,
    pub publish_final_grade: Option<bool>,
    pub prerequisite_module_ids: Option<Vec<i64>>,
    pub state: Option<String>,
    pub completed_at: Option<String>,
    pub items_count: Option<u64>,
    pub items_url: Option<String>,
    pub items: Option<Vec<ModuleItem>>,
    pub published: Option<bool>,
}
