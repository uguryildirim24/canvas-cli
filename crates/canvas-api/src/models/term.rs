//! Enrollment term.

use jiff::Timestamp;
use serde::Deserialize;

use crate::serde_util::{deserialize_opt_id, deserialize_opt_timestamp};

/// Academic term attached to a course.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Term {
    #[serde(default, deserialize_with = "deserialize_opt_id")]
    pub id: Option<i64>,
    pub name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_timestamp")]
    pub start_at: Option<Timestamp>,
    #[serde(default, deserialize_with = "deserialize_opt_timestamp")]
    pub end_at: Option<Timestamp>,
}
