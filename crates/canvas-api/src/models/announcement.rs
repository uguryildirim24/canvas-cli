//! Announcement (discussion topic) model.

use jiff::Timestamp;
use reqwest::Url;
use serde::Deserialize;
use serde_json::Value;

use crate::serde_util::{deserialize_id, deserialize_opt_timestamp};

/// Discussion topic used as an announcement.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Announcement {
    #[serde(deserialize_with = "deserialize_id")]
    pub id: i64,
    pub title: Option<String>,
    pub message: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_timestamp")]
    pub posted_at: Option<Timestamp>,
    #[serde(default, deserialize_with = "deserialize_opt_timestamp")]
    pub delayed_post_at: Option<Timestamp>,
    pub user_name: Option<String>,
    pub author: Option<Value>,
    pub read_state: Option<String>,
    pub unread_count: Option<u64>,
    pub published: Option<bool>,
    pub locked: Option<bool>,
    pub pinned: Option<bool>,
    pub is_announcement: Option<bool>,
    pub context_code: Option<String>,
    #[serde(
        default,
        deserialize_with = "crate::serde_util::deserialize_supplied_url"
    )]
    pub html_url: crate::serde_util::Supplied<Url>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_opt_url")]
    pub url: Option<reqwest::Url>,
}
