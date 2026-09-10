//! Assignment and missing-submission shapes.

use jiff::Timestamp;
use reqwest::Url;
use serde::Deserialize;
use serde_json::Value;

use super::submission::Submission;
use crate::serde_util::{
    Supplied, deserialize_id, deserialize_opt_id, deserialize_opt_timestamp, deserialize_opt_url,
    deserialize_supplied,
};

/// External tool attributes on an assignment.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct ExternalToolTagAttributes {
    pub url: Option<String>,
    pub new_tab: Option<bool>,
    pub resource_link_id: Option<String>,
    pub external_data: Option<String>,
    pub content_id: Option<i64>,
    pub content_type: Option<String>,
}

/// Canvas assignment.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Assignment {
    #[serde(deserialize_with = "deserialize_id")]
    pub id: i64,
    pub name: Option<String>,
    pub description: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_timestamp")]
    pub due_at: Option<Timestamp>,
    #[serde(default, deserialize_with = "deserialize_opt_timestamp")]
    pub unlock_at: Option<Timestamp>,
    #[serde(default, deserialize_with = "deserialize_opt_timestamp")]
    pub lock_at: Option<Timestamp>,
    pub points_possible: Option<f64>,
    pub grading_type: Option<String>,
    pub submission_types: Option<Vec<String>>,
    pub allowed_extensions: Option<Vec<String>>,
    pub allowed_attempts: Option<i64>,
    #[serde(default, deserialize_with = "deserialize_opt_id")]
    pub group_category_id: Option<i64>,
    #[serde(default, deserialize_with = "deserialize_opt_id")]
    pub course_id: Option<i64>,
    #[serde(default, deserialize_with = "deserialize_opt_id")]
    pub assignment_group_id: Option<i64>,
    pub position: Option<i64>,
    pub published: Option<bool>,
    pub locked_for_user: Option<bool>,
    pub lock_explanation: Option<String>,
    pub omit_from_final_grade: Option<bool>,
    pub anonymous_submissions: Option<bool>,
    pub muted: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_opt_url")]
    pub html_url: Option<Url>,
    #[serde(default, deserialize_with = "deserialize_opt_url")]
    pub submissions_download_url: Option<Url>,
    pub external_tool_tag_attributes: Option<ExternalToolTagAttributes>,
    pub submission: Option<Submission>,
    /// Present only when the endpoint includes `can_submit`.
    #[serde(default, deserialize_with = "deserialize_supplied")]
    pub can_submit: Supplied<bool>,
    pub rubric: Option<Value>,
    pub has_submitted_submissions: Option<bool>,
    pub workflow_state: Option<String>,
}

/// Missing-submissions list entries are assignment-shaped.
pub type MissingSubmission = Assignment;
