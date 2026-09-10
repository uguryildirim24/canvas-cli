//! Course model.

use reqwest::Url;
use serde::Deserialize;

use super::term::Term;
use crate::serde_util::deserialize_id;

/// Enrollment summary embedded on a course (`include[]=total_scores`).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct CourseEnrollment {
    #[serde(rename = "type")]
    pub enrollment_type: Option<String>,
    pub enrollment_state: Option<String>,
    pub role: Option<String>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub computed_current_score: crate::serde_util::Supplied<f64>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub computed_final_score: crate::serde_util::Supplied<f64>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub computed_current_grade: crate::serde_util::Supplied<String>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub computed_final_grade: crate::serde_util::Supplied<String>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub current_period_computed_current_score: crate::serde_util::Supplied<f64>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub current_period_computed_final_score: crate::serde_util::Supplied<f64>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub current_period_computed_current_grade: crate::serde_util::Supplied<String>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub current_period_computed_final_grade: crate::serde_util::Supplied<String>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_opt_id")]
    pub current_grading_period_id: Option<i64>,
    pub current_grading_period_title: Option<String>,
}

/// Canvas course.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Course {
    #[serde(deserialize_with = "deserialize_id")]
    pub id: i64,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub name: crate::serde_util::Supplied<String>,
    pub course_code: Option<String>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub workflow_state: crate::serde_util::Supplied<String>,
    pub enrollment_state: Option<String>,
    pub term: Option<Term>,
    pub enrollments: Option<Vec<CourseEnrollment>>,
    pub is_favorite: Option<bool>,
    pub syllabus_body: Option<String>,
    /// Canvas sets this when the course is not yet available by date.
    #[serde(alias = "access_restricted_by_date")]
    pub restricted: Option<bool>,
    #[serde(
        default,
        deserialize_with = "crate::serde_util::deserialize_supplied_url"
    )]
    pub html_url: crate::serde_util::Supplied<Url>,
    pub time_zone: Option<String>,
}
