//! Course enrollment grade summaries.

use serde::Deserialize;

use crate::serde_util::{deserialize_id, deserialize_opt_id};

/// Enrollment grades object.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct EnrollmentGrades {
    #[serde(
        default,
        deserialize_with = "crate::serde_util::deserialize_supplied_url"
    )]
    pub html_url: crate::serde_util::Supplied<reqwest::Url>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub current_score: crate::serde_util::Supplied<f64>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub current_grade: crate::serde_util::Supplied<String>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub final_score: crate::serde_util::Supplied<f64>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub final_grade: crate::serde_util::Supplied<String>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub unposted_current_score: crate::serde_util::Supplied<f64>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub unposted_current_grade: crate::serde_util::Supplied<String>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub unposted_final_score: crate::serde_util::Supplied<f64>,
    #[serde(default, deserialize_with = "crate::serde_util::deserialize_supplied")]
    pub unposted_final_grade: crate::serde_util::Supplied<String>,
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
}
