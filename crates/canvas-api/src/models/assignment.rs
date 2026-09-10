//! Assignment and missing-submission shapes.

use jiff::Timestamp;
use reqwest::Url;
use serde::Deserialize;
use serde_json::Value;

use super::submission::Submission;
use crate::serde_util::{
    Supplied, deserialize_id, deserialize_opt_id, deserialize_opt_url, deserialize_supplied,
};

/// External tool attributes on an assignment.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct ExternalToolTagAttributes {
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_opt_url")]
    pub url: Option<reqwest::Url>,
    pub new_tab: Option<bool>,
    pub resource_link_id: Option<String>,
    pub external_data: Option<String>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_opt_id")]
    pub content_id: Option<i64>,
    pub content_type: Option<String>,
}

/// Canvas assignment.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Assignment {
    #[serde(deserialize_with = "deserialize_id")]
    pub id: i64,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub name: crate::serde_util::Supplied<String>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub description: crate::serde_util::Supplied<String>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub due_at: crate::serde_util::Supplied<Timestamp>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub unlock_at: crate::serde_util::Supplied<Timestamp>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub lock_at: crate::serde_util::Supplied<Timestamp>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub points_possible: crate::serde_util::Supplied<f64>,
    pub grading_type: Option<String>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub submission_types: crate::serde_util::Supplied<Vec<String>>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub allowed_extensions: crate::serde_util::Supplied<Vec<String>>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub allowed_attempts: crate::serde_util::Supplied<i64>,
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
    #[serde(
        default,
        deserialize_with = "crate::serde_util::deserialize_supplied_url"
    )]
    pub html_url: crate::serde_util::Supplied<Url>,
    #[serde(default, deserialize_with = "deserialize_opt_url")]
    pub submissions_download_url: Option<Url>,
    pub external_tool_tag_attributes: Option<ExternalToolTagAttributes>,
    pub submission: Option<Submission>,
    pub course: Option<super::Course>,
    pub planner_override: Option<super::PlannerOverride>,
    /// Present only when the endpoint includes `can_submit`.
    #[serde(default, deserialize_with = "deserialize_supplied")]
    pub can_submit: Supplied<bool>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub rubric: crate::serde_util::Supplied<Value>,
    pub has_submitted_submissions: Option<bool>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub workflow_state: crate::serde_util::Supplied<String>,
}

/// Missing-submissions list entries are assignment-shaped.
pub type MissingSubmission = Assignment;
