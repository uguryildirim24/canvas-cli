//! Quiz taking: the live session and its questions (§12.7, M10-a).
//!
//! A Classic Quizzes session is started with one `POST` and answered with two
//! more: the answers, then the completion. Canvas hands the session a
//! `validation_token` that authorizes both, so the token is stored, never
//! printed, and read back by the operation that sends the answers.
//!
//! `quiz questions` is the read half: it joins the live session or starts
//! one, with the person's confirmation at the terminal, and prints the
//! censored questions. Starting is consequential — it begins an attempt and,
//! on a timed quiz, the clock — and it is idempotent: a session already in
//! progress is reused, never restarted. The state row is the record of that;
//! the answers and the completion are the journaled operation (`operations`).

use canvas_api::models::{
    QuizSubmission, QuizSubmissionQuestion, QuizSubmissionQuestionsDoc, QuizSubmissionsDoc,
};
use jiff::Timestamp;
use rusqlite::{OptionalExtension, params};

use crate::operations::OperationError;
use crate::store::{DbError, Store};

/// The live session one quiz holds, as this machine knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// Canvas quiz submission id.
    pub quiz_submission_id: i64,
    /// The attempt this session runs.
    pub attempt: i64,
    /// The token that authorizes answering and completing.
    pub validation_token: String,
    /// When Canvas started the session.
    pub started_at: Option<String>,
    /// When the attempt is overdue, on a timed quiz.
    pub end_at: Option<String>,
}

/// Read the stored session for one quiz, when one was recorded.
#[must_use]
pub fn load_session(store: &Store, identity_key: &str, quiz_id: i64) -> Option<Session> {
    let identity_key = identity_key.to_owned();
    store
        .call_blocking(move |conns| {
            conns
                .state
                .query_row(
                    "SELECT quiz_submission_id, attempt, validation_token, started_at, end_at
                     FROM quiz_session WHERE identity_key = ?1 AND quiz_id = ?2",
                    params![identity_key, quiz_id],
                    |row| {
                        Ok(Session {
                            quiz_submission_id: row.get(0)?,
                            attempt: row.get(1)?,
                            validation_token: row.get(2)?,
                            started_at: row.get(3)?,
                            end_at: row.get(4)?,
                        })
                    },
                )
                .optional()
                .map_err(DbError::from)
        })
        .ok()
        .flatten()
}

/// Record the session for one quiz, replacing any earlier one.
pub fn save_session(
    store: &Store,
    identity_key: &str,
    course_id: i64,
    quiz_id: i64,
    session: &Session,
) -> Result<(), OperationError> {
    let identity_key = identity_key.to_owned();
    let session = session.clone();
    let now = Timestamp::now().to_string();
    store
        .call_blocking(move |conns| {
            conns
                .state
                .execute(
                    "INSERT INTO quiz_session
                     (identity_key, quiz_id, course_id, quiz_submission_id, attempt,
                      validation_token, started_at, end_at, workflow_state, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'untaken', ?9)
                 ON CONFLICT(identity_key, quiz_id) DO UPDATE SET
                     course_id = excluded.course_id,
                     quiz_submission_id = excluded.quiz_submission_id,
                     attempt = excluded.attempt,
                     validation_token = excluded.validation_token,
                     started_at = excluded.started_at,
                     end_at = excluded.end_at,
                     workflow_state = excluded.workflow_state,
                     updated_at = excluded.updated_at",
                    params![
                        identity_key,
                        quiz_id,
                        course_id,
                        session.quiz_submission_id,
                        session.attempt,
                        session.validation_token,
                        session.started_at,
                        session.end_at,
                        now
                    ],
                )
                .map(|_| ())
                .map_err(DbError::from)
        })
        .map_err(OperationError::Store)?;
    Ok(())
}

/// Forget the stored session, after Canvas closed it.
pub fn clear_session(
    store: &Store,
    identity_key: &str,
    quiz_id: i64,
) -> Result<(), OperationError> {
    let identity_key = identity_key.to_owned();
    store
        .call_blocking(move |conns| {
            conns
                .state
                .execute(
                    "DELETE FROM quiz_session WHERE identity_key = ?1 AND quiz_id = ?2",
                    params![identity_key, quiz_id],
                )
                .map(|_| ())
                .map_err(DbError::from)
        })
        .map_err(OperationError::Store)?;
    Ok(())
}

/// `GET /courses/:cid/quizzes/:qid/submission` — the current user's session.
pub async fn own_submission(
    client: &canvas_api::Client,
    course_id: i64,
    quiz_id: i64,
) -> Result<Option<QuizSubmission>, OperationError> {
    let path = format!("/api/v1/courses/{course_id}/quizzes/{quiz_id}/submission");
    let doc: QuizSubmissionsDoc = client.get(&path).await?;
    Ok(doc.quiz_submissions.into_iter().next())
}

/// `POST /courses/:cid/quizzes/:qid/submissions` — start a session.
async fn start_submission(
    client: &canvas_api::Client,
    course_id: i64,
    quiz_id: i64,
    access_code: Option<&str>,
) -> Result<QuizSubmission, OperationError> {
    let mut path = format!("/api/v1/courses/{course_id}/quizzes/{quiz_id}/submissions");
    if let Some(code) = access_code {
        use std::fmt::Write as _;
        let _ = write!(path, "?access_code={}", urlencode(code));
    }
    let doc: QuizSubmissionsDoc = client.post(&path, &serde_json::json!({})).await?;
    doc.quiz_submissions
        .into_iter()
        .next()
        .ok_or(OperationError::refused(
            "unknown",
            "Canvas named no quiz submission",
        ))
}

/// Percent-encode a query value, keeping the unreserved set intact.
fn urlencode(raw: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    out
}

/// The live session for one quiz: joined when one is in progress, started
/// when one is not.
///
/// The start is refused exactly the way Canvas refuses it: a locked quiz, a
/// wrong access code, an IP filter, and a `LockDown` requirement all leave with
/// exit 8 and a named reason, because the browser is where those quizzes must
/// be taken.
pub async fn live_session(
    client: &canvas_api::Client,
    store: &Store,
    identity_key: &str,
    course_id: i64,
    quiz_id: i64,
    access_code: Option<&str>,
) -> Result<(Session, bool), OperationError> {
    if let Some(existing) = own_submission(client, course_id, quiz_id).await?
        && existing.is_live()
    {
        let Some(token) = existing.validation_token.clone() else {
            return Err(OperationError::refused(
                "session_expired",
                "the session is in progress but Canvas gave no validation token; take the quiz in the browser",
            ));
        };
        let session = Session {
            quiz_submission_id: existing.id,
            attempt: existing.attempt.unwrap_or(1),
            validation_token: token,
            started_at: existing.started_at.map(|t| t.to_string()),
            end_at: existing.end_at.map(|t| t.to_string()),
        };
        let known = load_session(store, identity_key, quiz_id);
        let started = known.is_none_or(|stored| stored != session);
        save_session(store, identity_key, course_id, quiz_id, &session)?;
        return Ok((session, started));
    }

    // No session in progress: starting one is the person's decision, and the
    // caller confirms it at the terminal before this runs.
    let submission = match start_submission(client, course_id, quiz_id, access_code).await {
        Ok(submission) => submission,
        Err(OperationError::Network(canvas_api::Error::Denied { status })) => {
            return Err(refusal_for_status(
                status,
                course_id,
                quiz_id,
                access_code.is_some(),
            ));
        }
        Err(other) => return Err(other),
    };
    let Some(token) = submission.validation_token.clone() else {
        return Err(OperationError::refused(
            "unknown",
            "Canvas started the session but gave no validation token",
        ));
    };
    let session = Session {
        quiz_submission_id: submission.id,
        attempt: submission.attempt.unwrap_or(1),
        validation_token: token,
        started_at: submission.started_at.map(|t| t.to_string()),
        end_at: submission.end_at.map(|t| t.to_string()),
    };
    save_session(store, identity_key, course_id, quiz_id, &session)?;
    Ok((session, true))
}

/// Name the refusal a start status means.
fn refusal_for_status(status: u16, course_id: i64, quiz_id: i64, had_code: bool) -> OperationError {
    match status {
        400 => OperationError::refused(
            "locked",
            format!("quiz {quiz_id} in course {course_id} is locked"),
        ),
        401 | 403 if had_code => {
            OperationError::refused("access_code", "the access code was not accepted")
        }
        401 | 403 => OperationError::refused(
            "ip_filter_or_lockdown",
            "Canvas refused this client; the quiz needs the browser (IP filter or LockDown)",
        ),
        404 => OperationError::refused(
            "unresolved",
            format!("quiz {quiz_id} in course {course_id} was not found"),
        ),
        409 => OperationError::refused(
            "in_progress",
            "a session is already in progress; run `canvas quiz questions` to join it",
        ),
        other => OperationError::refused(
            "unknown",
            format!("Canvas answered {other} when the session started"),
        ),
    }
}

/// The questions of one session, censored for a student.
pub async fn questions(
    client: &canvas_api::Client,
    quiz_submission_id: i64,
) -> Result<Vec<QuizSubmissionQuestion>, OperationError> {
    let path =
        format!("/api/v1/quiz_submissions/{quiz_submission_id}/questions?include[]=quiz_question");
    let doc: QuizSubmissionQuestionsDoc = client.get(&path).await?;
    Ok(doc.quiz_submission_questions)
}
