//! Course enrollment grade summaries.

use serde::Deserialize;

use crate::serde_util::{deserialize_id, deserialize_opt_id};

/// Enrollment grades object.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct EnrollmentGrades {
    pub html_url: Option<String>,
    pub current_score: Option<f64>,
    pub current_grade: Option<String>,
    pub final_score: Option<f64>,
    pub final_grade: Option<String>,
    pub unposted_current_score: Option<f64>,
    pub unposted_current_grade: Option<String>,
    pub unposted_final_score: Option<f64>,
    pub unposted_final_grade: Option<String>,
}

/// User enrollment on a course.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Enrollment {
    #[serde(deserialize_with = "deserialize_id")]
    pub id: i64,
    #[serde(default, deserialize_with = "deserialize_opt_id")]
    pub course_id: Option<i64>,
    #[serde(default, deserialize_with = "deserialize_opt_id")]
    pub user_id: Option<i64>,
    #[serde(rename = "type")]
    pub enrollment_type: Option<String>,
    pub role: Option<String>,
    pub enrollment_state: Option<String>,
    pub grades: Option<EnrollmentGrades>,
    pub computed_current_score: Option<f64>,
    pub computed_final_score: Option<f64>,
    pub computed_current_grade: Option<String>,
    pub computed_final_grade: Option<String>,
    pub current_period_computed_current_score: Option<f64>,
    pub current_period_computed_final_score: Option<f64>,
    pub current_period_computed_current_grade: Option<String>,
    pub current_period_computed_final_grade: Option<String>,
}
