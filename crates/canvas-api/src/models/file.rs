//! File model.

use jiff::Timestamp;
use reqwest::Url;
use serde::Deserialize;

use crate::serde_util::{
    deserialize_id, deserialize_opt_id, deserialize_opt_timestamp, deserialize_opt_url,
};

/// Canvas file metadata.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct File {
    #[serde(deserialize_with = "deserialize_id")]
    pub id: i64,
    pub display_name: Option<String>,
    pub filename: Option<String>,
    #[serde(alias = "content-type")]
    pub content_type: Option<String>,
    pub size: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_opt_id")]
    pub folder_id: Option<i64>,
    pub hidden: Option<bool>,
    pub locked: Option<bool>,
    pub locked_for_user: Option<bool>,
    pub lock_explanation: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_timestamp")]
    pub updated_at: Option<Timestamp>,
    #[serde(default, deserialize_with = "deserialize_opt_timestamp")]
    pub modified_at: Option<Timestamp>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub unlock_at: crate::serde_util::Supplied<Timestamp>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub lock_at: crate::serde_util::Supplied<Timestamp>,
    #[serde(default, deserialize_with = "deserialize_opt_url")]
    pub url: Option<Url>,
    #[serde(default, deserialize_with = "deserialize_opt_url")]
    pub thumbnail_url: Option<Url>,
}
