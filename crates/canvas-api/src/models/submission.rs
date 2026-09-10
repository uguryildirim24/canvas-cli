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
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub attempt: crate::serde_util::Supplied<i64>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub submitted_at: crate::serde_util::Supplied<Timestamp>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub workflow_state: crate::serde_util::Supplied<String>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub score: crate::serde_util::Supplied<f64>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub grade: crate::serde_util::Supplied<String>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub late: crate::serde_util::Supplied<bool>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub missing: crate::serde_util::Supplied<bool>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub excused: crate::serde_util::Supplied<bool>,
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
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub attempt: crate::serde_util::Supplied<i64>,
    pub extra_attempts: Option<i64>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub submitted_at: crate::serde_util::Supplied<Timestamp>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub graded_at: crate::serde_util::Supplied<Timestamp>,
    #[serde(default, deserialize_with = "deserialize_opt_timestamp")]
    pub posted_at: Option<Timestamp>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub workflow_state: crate::serde_util::Supplied<String>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub score: crate::serde_util::Supplied<f64>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub grade: crate::serde_util::Supplied<String>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub late: crate::serde_util::Supplied<bool>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub missing: crate::serde_util::Supplied<bool>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub excused: crate::serde_util::Supplied<bool>,
    pub submission_type: Option<String>,
    pub body: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_url")]
    pub url: Option<Url>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_opt_url")]
    pub preview_url: Option<reqwest::Url>,
    pub attachments: Option<Vec<SubmissionAttachment>>,
    pub submission_comments: Option<Vec<SubmissionComment>>,
    pub submission_history: Option<Vec<SubmissionHistoryEntry>>,
    pub rubric_assessment: Option<RubricAssessment>,
}
