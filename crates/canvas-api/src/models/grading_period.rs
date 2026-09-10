//! Grading periods and wrapped collections.

use jiff::Timestamp;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::serde_util::{deserialize_id, deserialize_opt_timestamp};

/// One grading period.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct GradingPeriod {
    #[serde(deserialize_with = "deserialize_id")]
    pub id: i64,
    pub title: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_timestamp")]
    pub start_date: Option<Timestamp>,
    #[serde(default, deserialize_with = "deserialize_opt_timestamp")]
    pub end_date: Option<Timestamp>,
    #[serde(default, deserialize_with = "deserialize_opt_timestamp")]
    pub close_date: Option<Timestamp>,
    pub weight: Option<f64>,
    pub is_closed: Option<bool>,
}

/// Wrapped Canvas collection (`{ "grading_periods": [...], "meta": … }`).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(bound(deserialize = "T: DeserializeOwned"))]
pub struct WrappedCollection<T> {
    /// Collection items (JSON name `grading_periods` or generic `items`).
    #[serde(alias = "grading_periods")]
    pub items: Vec<T>,
    /// Optional meta object.
    #[serde(default)]
    pub meta: Option<Value>,
}

impl<T> WrappedCollection<T> {
    /// Extract the inner item vector.
    #[must_use]
    pub fn into_items(self) -> Vec<T> {
        self.items
    }
}
