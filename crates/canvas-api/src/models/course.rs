//! Course model.

use reqwest::Url;
use serde::Deserialize;

use super::term::Term;
use crate::serde_util::{deserialize_id, deserialize_opt_url};

/// Enrollment summary embedded on a course (`include[]=total_scores`).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct CourseEnrollment {
    #[serde(rename = "type")]
    pub enrollment_type: Option<String>,
    pub enrollment_state: Option<String>,
    pub role: Option<String>,
    pub computed_current_score: Option<f64>,
    pub computed_final_score: Option<f64>,
    pub computed_current_grade: Option<String>,
    pub computed_final_grade: Option<String>,
    pub current_period_computed_current_score: Option<f64>,
    pub current_period_computed_final_score: Option<f64>,
    pub current_period_computed_current_grade: Option<String>,
    pub current_period_computed_final_grade: Option<String>,
    pub current_grading_period_id: Option<i64>,
    pub current_grading_period_title: Option<String>,
}

/// Canvas course.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Course {
    #[serde(deserialize_with = "deserialize_id")]
    pub id: i64,
    pub name: Option<String>,
    pub course_code: Option<String>,
    pub workflow_state: Option<String>,
    pub enrollment_state: Option<String>,
    pub term: Option<Term>,
    pub enrollments: Option<Vec<CourseEnrollment>>,
    pub is_favorite: Option<bool>,
    pub syllabus_body: Option<String>,
    /// Canvas sets this when the course is not yet available by date.
    #[serde(alias = "access_restricted_by_date")]
    pub restricted: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_opt_url")]
    pub html_url: Option<Url>,
    pub time_zone: Option<String>,
}
