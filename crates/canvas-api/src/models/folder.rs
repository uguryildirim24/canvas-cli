//! Folder model.

use jiff::Timestamp;
use reqwest::Url;
use serde::Deserialize;

use crate::serde_util::{
    deserialize_id, deserialize_opt_id, deserialize_opt_timestamp, deserialize_opt_url,
};

/// Course folder.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Folder {
    #[serde(deserialize_with = "deserialize_id")]
    pub id: i64,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub name: crate::serde_util::Supplied<String>,
    pub full_name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_id")]
    pub parent_folder_id: Option<i64>,
    #[serde(default, deserialize_with = "deserialize_opt_id")]
    pub context_id: Option<i64>,
    pub context_type: Option<String>,
    pub files_count: Option<u64>,
    pub folders_count: Option<u64>,
    pub position: Option<i64>,
    pub hidden: Option<bool>,
    pub locked: Option<bool>,
    pub locked_for_user: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_opt_timestamp")]
    pub updated_at: Option<Timestamp>,
    #[serde(default, deserialize_with = "deserialize_opt_url")]
    pub files_url: Option<Url>,
    #[serde(default, deserialize_with = "deserialize_opt_url")]
    pub folders_url: Option<Url>,
}
