//! New Quizzes metadata models (§12.7, M10-b).
//!
//! The New Quizzes REST API (`/api/quiz/v1/...`) builds and describes
//! quizzes; it has no session, answer, or completion endpoint. Taking one
//! happens inside the LTI tool session, which no token reaches. These models
//! therefore cover only what an agent may read: the listing, the detail, and
//! the taking rules.

use jiff::Timestamp;
use serde::Deserialize;

use crate::serde_util::{deserialize_id, deserialize_opt_id, deserialize_opt_timestamp};

/// The taking rules of a New Quiz, as the student may see them.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct NewQuizSettings {
    pub one_at_a_time_type: Option<String>,
    pub allow_backtracking: Option<bool>,
    pub shuffle_answers: Option<bool>,
    pub shuffle_questions: Option<bool>,
    pub require_student_access_code: Option<bool>,
    pub has_time_limit: Option<bool>,
    pub session_time_limit_in_seconds: Option<u64>,
    pub multiple_attempts: Option<NewQuizMultipleAttempts>,
}

/// How many attempts a New Quiz allows.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct NewQuizMultipleAttempts {
    pub multiple_attempts_enabled: Option<bool>,
    pub attempt_limit: Option<bool>,
    pub max_attempts: Option<i64>,
    pub score_to_keep: Option<String>,
}

/// One New Quiz (`GET /api/quiz/v1/courses/:cid/quizzes[/aid]`).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct NewQuiz {
    #[serde(deserialize_with = "deserialize_id")]
    pub id: i64,
    pub title: Option<String>,
    pub instructions: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_id")]
    pub assignment_group_id: Option<i64>,
    #[serde(default, deserialize_with = "deserialize_opt_id")]
    pub assignment_id: Option<i64>,
    pub points_possible: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_opt_timestamp")]
    pub due_at: Option<Timestamp>,
    #[serde(default, deserialize_with = "deserialize_opt_timestamp")]
    pub lock_at: Option<Timestamp>,
    #[serde(default, deserialize_with = "deserialize_opt_timestamp")]
    pub unlock_at: Option<Timestamp>,
    pub published: Option<bool>,
    pub grading_type: Option<String>,
    pub quiz_settings: Option<NewQuizSettings>,
    pub html_url: Option<String>,
}

impl NewQuiz {
    /// The time limit in seconds, when the quiz is timed.
    #[must_use]
    pub fn time_limit_seconds(&self) -> Option<u64> {
        let settings = self.quiz_settings.as_ref()?;
        if settings.has_time_limit.unwrap_or(false) {
            settings.session_time_limit_in_seconds.filter(|s| *s > 0)
        } else {
            None
        }
    }
}
