//! Submission model.

use jiff::Timestamp;
use reqwest::Url;
use serde::Deserialize;
use serde_json::Value;

use crate::serde_util::{
    deserialize_id, deserialize_opt_id, deserialize_opt_timestamp, deserialize_opt_url,
};

/// File attachment on a submission.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct SubmissionAttachment {
    #[serde(deserialize_with = "deserialize_id")]
    pub id: i64,
    pub display_name: Option<String>,
    pub filename: Option<String>,
    pub size: Option<u64>,
    #[serde(alias = "content-type")]
    pub content_type: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_url")]
    pub url: Option<Url>,
}

/// Comment on a submission.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct SubmissionComment {
    #[serde(default, deserialize_with = "deserialize_opt_id")]
    pub id: Option<i64>,
    pub comment: Option<String>,
    pub author_name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_timestamp")]
    pub created_at: Option<Timestamp>,
}

/// One history entry (same shape as a submission row).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct SubmissionHistoryEntry {
    #[serde(default, deserialize_with = "deserialize_opt_id")]
    pub id: Option<i64>,
    pub attempt: Option<i64>,
    #[serde(default, deserialize_with = "deserialize_opt_timestamp")]
    pub submitted_at: Option<Timestamp>,
    pub workflow_state: Option<String>,
    pub score: Option<f64>,
    pub grade: Option<String>,
    pub late: Option<bool>,
    pub missing: Option<bool>,
    pub excused: Option<bool>,
    pub submission_type: Option<String>,
    pub body: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_url")]
    pub url: Option<Url>,
    pub attachments: Option<Vec<SubmissionAttachment>>,
}

/// Rubric assessment map keyed by criterion id.
pub type RubricAssessment = Value;

/// Canvas submission.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Submission {
    #[serde(default, deserialize_with = "deserialize_opt_id")]
    pub id: Option<i64>,
    #[serde(default, deserialize_with = "deserialize_opt_id")]
    pub assignment_id: Option<i64>,
    #[serde(default, deserialize_with = "deserialize_opt_id")]
    pub user_id: Option<i64>,
    pub attempt: Option<i64>,
    pub extra_attempts: Option<i64>,
    #[serde(default, deserialize_with = "deserialize_opt_timestamp")]
    pub submitted_at: Option<Timestamp>,
    #[serde(default, deserialize_with = "deserialize_opt_timestamp")]
    pub graded_at: Option<Timestamp>,
    #[serde(default, deserialize_with = "deserialize_opt_timestamp")]
    pub posted_at: Option<Timestamp>,
    pub workflow_state: Option<String>,
    pub score: Option<f64>,
    pub grade: Option<String>,
    pub late: Option<bool>,
    pub missing: Option<bool>,
    pub excused: Option<bool>,
    pub submission_type: Option<String>,
    pub body: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_url")]
    pub url: Option<Url>,
    pub preview_url: Option<String>,
    pub attachments: Option<Vec<SubmissionAttachment>>,
    pub submission_comments: Option<Vec<SubmissionComment>>,
    pub submission_history: Option<Vec<SubmissionHistoryEntry>>,
    pub rubric_assessment: Option<RubricAssessment>,
}
