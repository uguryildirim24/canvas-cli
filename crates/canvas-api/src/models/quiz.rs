//! Classic Quizzes models (§12.7, M10-a).
//!
//! Only the fields this project reads. Unknown fields are ignored, as every
//! other model does. The Quiz Submissions API is verified against the Canvas
//! source: `Quizzes::QuizSubmissionsApiController` and
//! `Quizzes::QuizSubmissionQuestionsController`.

use jiff::Timestamp;
use serde::Deserialize;
use serde_json::Value;

use crate::serde_util::{deserialize_id, deserialize_opt_timestamp};

/// One Classic Quiz (Quizzes API).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Quiz {
    #[serde(deserialize_with = "deserialize_id")]
    pub id: i64,
    pub title: Option<String>,
    pub description: Option<String>,
    /// `practice_quiz`, `assignment`, `graded_survey`, or `survey`.
    pub quiz_type: Option<String>,
    pub assignment_group_id: Option<i64>,
    pub assignment_id: Option<i64>,
    /// Minutes, when the quiz is timed.
    pub time_limit: Option<u64>,
    pub allowed_attempts: Option<i64>,
    pub question_count: Option<u64>,
    pub points_possible: Option<f64>,
    pub cant_go_back: Option<bool>,
    pub one_question_at_a_time: Option<bool>,
    pub shuffle_answers: Option<bool>,
    pub show_correct_answers: Option<bool>,
    pub hide_results: Option<Value>,
    pub scoring_policy: Option<String>,
    pub published: Option<bool>,
    pub unlocked_for_user: Option<bool>,
    pub locked_for_user: Option<bool>,
    pub lock_explanation: Option<String>,
    pub require_lockdown_browser: Option<bool>,
    /// Present only when the quiz is IP-filtered.
    pub ip_filter: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_timestamp")]
    pub due_at: Option<Timestamp>,
    #[serde(default, deserialize_with = "deserialize_opt_timestamp")]
    pub unlock_at: Option<Timestamp>,
    #[serde(default, deserialize_with = "deserialize_opt_timestamp")]
    pub lock_at: Option<Timestamp>,
    pub html_url: Option<String>,
}

impl Quiz {
    /// The quiz is timed when `time_limit` is present and positive.
    #[must_use]
    pub fn is_timed(&self) -> bool {
        self.time_limit.is_some_and(|minutes| minutes > 0)
    }

    /// The quiz can be taken again when Canvas reports attempts left, or when
    /// attempts are unlimited (`allowed_attempts` negative or absent).
    #[must_use]
    pub fn has_attempts_left(&self) -> bool {
        !matches!(self.allowed_attempts, Some(0))
    }
}

/// One quiz submission (Quiz Submissions API).
///
/// `validation_token` is the local key for answering and completing: Canvas
/// hands it out when the session starts and it never prints elsewhere. The
/// struct therefore derives no `Serialize`; the token is stored, not echoed.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct QuizSubmission {
    #[serde(deserialize_with = "deserialize_id")]
    pub id: i64,
    #[serde(deserialize_with = "deserialize_id")]
    pub quiz_id: i64,
    #[serde(deserialize_with = "deserialize_id")]
    pub user_id: i64,
    #[serde(deserialize_with = "deserialize_id")]
    pub submission_id: i64,
    #[serde(default, deserialize_with = "deserialize_opt_timestamp")]
    pub started_at: Option<Timestamp>,
    #[serde(default, deserialize_with = "deserialize_opt_timestamp")]
    pub finished_at: Option<Timestamp>,
    #[serde(default, deserialize_with = "deserialize_opt_timestamp")]
    pub end_at: Option<Timestamp>,
    pub attempt: Option<i64>,
    pub extra_attempts: Option<i64>,
    pub extra_time: Option<i64>,
    pub manually_unlocked: Option<bool>,
    pub time_spent: Option<i64>,
    pub score: Option<f64>,
    pub score_before_regrade: Option<f64>,
    pub kept_score: Option<f64>,
    pub fudge_points: Option<f64>,
    pub has_seen_results: Option<bool>,
    pub validation_token: Option<String>,
    /// `untaken`, `pending_review`, `complete`, `settings_only`, or `preview`.
    pub workflow_state: Option<String>,
    pub overdue_and_needs_submission: Option<bool>,
    pub attempts_left: Option<i64>,
    pub quiz_points_possible: Option<f64>,
    pub html_url: Option<String>,
    pub result_url: Option<String>,
}

impl QuizSubmission {
    /// An answerable, in-progress session.
    #[must_use]
    pub fn is_live(&self) -> bool {
        self.workflow_state.as_deref() == Some("untaken")
    }

    /// A session Canvas graded and closed.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.workflow_state.as_deref() == Some("complete")
    }
}

/// `{"quiz_submissions": [...]}`.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct QuizSubmissionsDoc {
    pub quiz_submissions: Vec<QuizSubmission>,
}

/// One question of a quiz-taking session, censored for a student.
///
/// The primary collection of
/// `GET /quiz_submissions/:id/questions?include[]=quiz_question`: the
/// serializer merges the question data into every record, so one struct holds
/// both halves. `answer` is the recorded answer, `answers` the choices.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct QuizSubmissionQuestion {
    #[serde(deserialize_with = "deserialize_id")]
    pub id: i64,
    pub flagged: Option<bool>,
    /// The answer this session holds for the question, or `null`.
    pub answer: Option<Value>,
    /// The possible answers, when the student may see them.
    pub answers: Option<Value>,
    pub position: Option<i64>,
    pub question_name: Option<String>,
    /// `multiple_choice_question`, `essay_question`, and the rest of the
    /// Question Answer Formats appendix.
    pub question_type: Option<String>,
    pub question_text: Option<Value>,
    pub points_possible: Option<f64>,
}

/// `{"quiz_submission_questions": [...]}`.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct QuizSubmissionQuestionsDoc {
    pub quiz_submission_questions: Vec<QuizSubmissionQuestion>,
}

/// One answer to post: the question id and the answer value in the format the
/// Question Answer Formats appendix defines for its type.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct QuizAnswer {
    /// The `QuizQuestion` id.
    pub id: i64,
    /// The answer value: an id, a text, a decimal, an array, or a map.
    pub answer: Value,
}
