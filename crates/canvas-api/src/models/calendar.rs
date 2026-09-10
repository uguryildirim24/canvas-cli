//! Calendar event model.

use jiff::Timestamp;
use jiff::civil::Date;
use reqwest::Url;
use serde::Deserialize;

use crate::serde_util::{
    deserialize_id, deserialize_opt_date, deserialize_opt_timestamp, deserialize_opt_url,
};

/// Calendar event.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct CalendarEvent {
    #[serde(deserialize_with = "deserialize_id")]
    pub id: i64,
    pub title: Option<String>,
    pub description: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_timestamp")]
    pub start_at: Option<Timestamp>,
    #[serde(default, deserialize_with = "deserialize_opt_timestamp")]
    pub end_at: Option<Timestamp>,
    pub location_name: Option<String>,
    pub location_address: Option<String>,
    pub context_code: Option<String>,
    pub workflow_state: Option<String>,
    pub hidden: Option<bool>,
    pub all_day: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_opt_date")]
    pub all_day_date: Option<Date>,
    #[serde(default, deserialize_with = "deserialize_opt_url")]
    pub html_url: Option<Url>,
    pub url: Option<String>,
}
