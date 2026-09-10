//! Calendar event model.

use jiff::Timestamp;
use jiff::civil::Date;
use reqwest::Url;
use serde::Deserialize;

use crate::serde_util::{deserialize_id, deserialize_opt_date, deserialize_opt_timestamp};

/// Calendar event.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct CalendarEvent {
    #[serde(deserialize_with = "deserialize_id")]
    pub id: i64,
    pub title: Option<String>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub description: crate::serde_util::Supplied<String>,
    #[serde(default, deserialize_with = "deserialize_opt_timestamp")]
    pub start_at: Option<Timestamp>,
    #[serde(default, deserialize_with = "deserialize_opt_timestamp")]
    pub end_at: Option<Timestamp>,
    pub location_name: Option<String>,
    pub location_address: Option<String>,
    pub context_code: Option<String>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub workflow_state: crate::serde_util::Supplied<String>,
    pub hidden: Option<bool>,
    pub all_day: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_opt_date")]
    pub all_day_date: Option<Date>,
    #[serde(
        default,
        deserialize_with = "crate::serde_util::deserialize_supplied_url"
    )]
    pub html_url: crate::serde_util::Supplied<Url>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_opt_url")]
    pub url: Option<reqwest::Url>,
}
