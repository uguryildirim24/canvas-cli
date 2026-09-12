//! `canvas quizzes` and `canvas quiz` (class C, M10-a).
//!
//! The listing and the detail route answer the same Classic Quiz shape, so one
//! table holds both and `quiz` refreshes the row the listing wrote by quiz id.
//! A quiz is addressed by id or by title, exactly as an assignment is.

use std::io::{self, Write};
use std::process::ExitCode;

use canvas_core::store::{DbError, StoreConns, lookup_dataset};
use canvas_core::sync::{QuizDetailDataset, QuizzesDataset, RefreshOutcome, refresh_quiz};
use comfy_table::Row;
use rusqlite::{OptionalExtension, params};
use serde_json::Value;

use super::Globals;
use super::course::{refresh_fail, resolve_with_refresh};
use super::course_load::{RefreshFail, cached_outcome, outcome_freshness};
use super::emit::{base_envelope, call_resolve, emit_error, resolve_error, session_error};
use super::handled::Handled;
use super::pages::{json_bool, json_f64, json_string, json_u64};
use crate::output::{
    QuizDetailJson, QuizResult, QuizSummaryJson, QuizzesResult, SCHEMA_QUIZ, SCHEMA_QUIZZES,
    apply_two_space_padding, new_table, now_timestamp,
};
use crate::session::{Session, ttl_quizzes};

/// Run `canvas quizzes` for the CLI: one envelope, one exit code.
pub async fn run_list(globals: &Globals, course: String) -> ExitCode {
    handle_list(globals, course).await.emit(globals.json)
}

/// Run `canvas quizzes <course>`.
pub async fn handle_list(globals: &Globals, course: String) -> Handled {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(e, globals.profile.clone()),
    };
    let (resolved, mut freshness, _) = match resolve_with_refresh(globals, &session, &course).await
    {
        Ok(v) => v,
        Err(code) => return code,
    };

    let outcome = match ensure_quizzes(globals, &session, resolved.id).await {
        Ok(o) => o,
        Err(e) => return refresh_fail(&session, e),
    };
    freshness.push(outcome_freshness(&outcome));

    let rows = match session
        .open
        .store
        .call(move |conns| load_quizzes(conns, resolved.id))
        .await
    {
        Ok(rows) => rows,
        Err(e) => return local_error(&session, &e),
    };

    let result = QuizzesResult {
        course_id: resolved.id.to_string(),
        quizzes: rows,
    };
    let mut envelope = base_envelope(SCHEMA_QUIZZES, &session, result);
    envelope.freshness = freshness;
    envelope.requests = session.requests();
    if outcome.freshness.stale {
        envelope.warnings.push("served stale quizzes cache".into());
    }

    Handled::new(envelope, move |envelope| {
        print_quiz_table(&envelope.result.quizzes)
    })
}

/// Run `canvas quiz` for the CLI: one envelope, one exit code.
pub async fn run_show(globals: &Globals, course: String, quiz: String) -> ExitCode {
    handle_show(globals, course, quiz).await.emit(globals.json)
}

/// Run `canvas quiz <course> <id|title|URL>`.
pub async fn handle_show(globals: &Globals, course: String, quiz: String) -> Handled {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(e, globals.profile.clone()),
    };
    let (resolved, mut freshness, _) = match resolve_with_refresh(globals, &session, &course).await
    {
        Ok(v) => v,
        Err(code) => return code,
    };

    // A title resolves against the listing, so the listing refreshes first.
    let outcome = match ensure_quizzes(globals, &session, resolved.id).await {
        Ok(o) => o,
        Err(e) => return refresh_fail(&session, e),
    };
    freshness.push(outcome_freshness(&outcome));

    let quiz_id = match quiz_id_of(&session, resolved.id, &quiz).await {
        Ok(id) => id,
        Err(code) => return code,
    };

    let detail_outcome = match ensure_quiz(globals, &session, resolved.id, quiz_id).await {
        Ok(o) => o,
        Err(e) => return refresh_fail(&session, e),
    };
    freshness.push(outcome_freshness(&detail_outcome));

    let row = match session
        .open
        .store
        .call(move |conns| load_quiz(conns, resolved.id, quiz_id))
        .await
    {
        Ok(Some(row)) => row,
        Ok(None) => {
            return emit_error(
                "resolution",
                &format!("quiz {quiz} not found in course {}", resolved.id),
                6,
                session.profile.clone(),
                Some(session.identity_ref()),
            );
        }
        Err(e) => return local_error(&session, &e),
    };

    let rich = canvas_core::markdown::rich_text_opt(row.description.as_deref())
        .await
        .unwrap_or_default();
    let refs = rich.refs.resolve(&session.identity.origin);
    let detail = QuizDetailJson {
        id: row.id.to_string(),
        course_id: resolved.id.to_string(),
        title: row.title.clone(),
        quiz_type: json_string(&row.data, "quiz_type"),
        time_limit: json_u64(&row.data, "time_limit"),
        allowed_attempts: json_i64(&row.data, "allowed_attempts"),
        question_count: json_u64(&row.data, "question_count"),
        points_possible: json_f64(&row.data, "points_possible"),
        cant_go_back: json_bool(&row.data, "cant_go_back"),
        one_question_at_a_time: json_bool(&row.data, "one_question_at_a_time"),
        require_lockdown_browser: json_bool(&row.data, "require_lockdown_browser"),
        ip_filtered: Some(
            !json_string(&row.data, "ip_filter")
                .unwrap_or_default()
                .is_empty(),
        ),
        published: json_bool(&row.data, "published"),
        unlocked_for_user: json_bool(&row.data, "unlocked_for_user"),
        locked_for_user: json_bool(&row.data, "locked_for_user"),
        lock_explanation: json_string(&row.data, "lock_explanation"),
        assignment_id: json_string(&row.data, "assignment_id"),
        html_url: json_string(&row.data, "html_url"),
        due_at: row.due_at.clone(),
        unlock_at: row.unlock_at.clone(),
        lock_at: row.lock_at.clone(),
        description_markdown: rich.markdown.clone(),
        truncated: rich.refs.truncated,
        embedded: super::pages::embedded_json(&refs),
        files: super::pages::files_json(&refs),
        external_links: super::pages::external_json(&refs),
    };

    let mut envelope = base_envelope(SCHEMA_QUIZ, &session, QuizResult { quiz: detail });
    envelope.freshness = freshness;
    envelope.requests = session.requests();
    if detail_outcome.freshness.stale || outcome.freshness.stale {
        envelope.warnings.push("served stale quiz cache".into());
    }
    super::pages::truncation_partial(
        &mut envelope,
        rich.refs.truncated,
        &format!("quiz:{quiz_id}"),
    );

    Handled::new(envelope, move |envelope| print_quiz(&envelope.result.quiz))
}

/// Resolve a quiz operand against the cached listing: an id, a URL, or a title.
///
/// The caller refreshed the listing first, so a title resolves the way an
/// assignment name does. Failures are exit 6 with candidates, exactly as an
/// assignment name resolves.
pub(super) async fn quiz_id_of(
    session: &Session,
    course_id: i64,
    operand: &str,
) -> Result<i64, Handled> {
    let origin = session.identity.origin.clone();
    let operand = operand.to_owned();
    match call_resolve(session, move |conns| {
        canvas_core::resolve::resolve_quiz(
            conns,
            course_id,
            &operand,
            &origin,
            canvas_core::resolve::CommandClass::C,
        )
    })
    .await
    {
        Ok(Ok(resolved)) => Ok(resolved.id),
        Ok(Err(e)) => Err(resolve_error(session, &e)),
        Err(e) => Err(local_error(session, &e)),
    }
}

/// Refresh the `quizzes` listing for one course.
pub(super) async fn ensure_quizzes(
    globals: &Globals,
    session: &Session,
    course_id: i64,
) -> Result<RefreshOutcome, RefreshFail> {
    let now = now_timestamp();
    let ttl = ttl_quizzes();
    let ds = QuizzesDataset::new(course_id, ttl);
    let lookup = session
        .open
        .store
        .call(move |conns| lookup_dataset(conns, &ds, now, None))
        .await
        .map_err(RefreshFail::Db)?;
    if let Some(outcome) = cached_outcome(lookup, globals.fresh, globals.offline)? {
        return Ok(outcome);
    }
    session
        .validate_network_token()
        .await
        .map_err(RefreshFail::Sync)?;
    let client = session.client.as_ref().ok_or(RefreshFail::NeedAuth)?;
    canvas_core::sync::refresh_quizzes(
        client,
        &session.open.store,
        course_id,
        ttl,
        now,
        globals.fresh,
        false,
    )
    .await
    .map_err(RefreshFail::Sync)
}

/// Refresh one quiz by id.
async fn ensure_quiz(
    globals: &Globals,
    session: &Session,
    course_id: i64,
    quiz_id: i64,
) -> Result<RefreshOutcome, RefreshFail> {
    let now = now_timestamp();
    let ttl = ttl_quizzes();
    let ds = QuizDetailDataset::new(course_id, quiz_id, ttl);
    let lookup = session
        .open
        .store
        .call(move |conns| lookup_dataset(conns, &ds, now, None))
        .await
        .map_err(RefreshFail::Db)?;
    if let Some(outcome) = cached_outcome(lookup, globals.fresh, globals.offline)? {
        return Ok(outcome);
    }
    session
        .validate_network_token()
        .await
        .map_err(RefreshFail::Sync)?;
    let client = session.client.as_ref().ok_or(RefreshFail::NeedAuth)?;
    refresh_quiz(
        client,
        &session.open.store,
        course_id,
        quiz_id,
        ttl,
        now,
        globals.fresh,
        false,
    )
    .await
    .map_err(RefreshFail::Sync)
}

/// One cached quiz row.
struct QuizRow {
    id: i64,
    title: Option<String>,
    description: Option<String>,
    due_at: Option<String>,
    unlock_at: Option<String>,
    lock_at: Option<String>,
    data: Value,
}

const QUIZ_SELECT: &str =
    "SELECT id, title, description, due_at, unlock_at, lock_at, data_json FROM quizzes";

fn read_quiz(r: &rusqlite::Row<'_>) -> Result<QuizRow, rusqlite::Error> {
    let raw: String = r.get(6)?;
    Ok(QuizRow {
        id: r.get(0)?,
        title: r.get(1)?,
        description: r.get(2)?,
        due_at: r.get(3)?,
        unlock_at: r.get(4)?,
        lock_at: r.get(5)?,
        data: serde_json::from_str(&raw).unwrap_or(Value::Null),
    })
}

fn load_quizzes(conns: &StoreConns, course_id: i64) -> Result<Vec<QuizSummaryJson>, DbError> {
    let scope = format!("course:{course_id}");
    let sql = format!(
        "{QUIZ_SELECT} INNER JOIN membership m ON CAST(m.entity_id AS INTEGER) = quizzes.id
         WHERE m.dataset = 'quizzes' AND m.scope = ?1 AND m.entity_kind = 'quiz'
         ORDER BY m.position, quizzes.id"
    );
    let mut stmt = conns.cache.prepare(&sql)?;
    let rows = stmt
        .query_map([scope], read_quiz)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows
        .into_iter()
        .map(|row| QuizSummaryJson {
            id: row.id.to_string(),
            title: row.title,
            quiz_type: json_string(&row.data, "quiz_type"),
            due_at: row.due_at,
            question_count: json_u64(&row.data, "question_count"),
            points_possible: json_f64(&row.data, "points_possible"),
            time_limit: json_u64(&row.data, "time_limit"),
            allowed_attempts: json_i64(&row.data, "allowed_attempts"),
            published: json_bool(&row.data, "published"),
            locked_for_user: json_bool(&row.data, "locked_for_user"),
        })
        .collect())
}

/// The quiz the `quiz:<course>:<id>` scope covers.
fn load_quiz(conns: &StoreConns, course_id: i64, quiz_id: i64) -> Result<Option<QuizRow>, DbError> {
    let scope = format!("quiz:{course_id}:{quiz_id}");
    let sql = format!(
        "{QUIZ_SELECT} INNER JOIN membership m ON CAST(m.entity_id AS INTEGER) = quizzes.id
         WHERE m.dataset = 'quiz' AND m.scope = ?1 AND m.entity_kind = 'quiz'
         ORDER BY m.position, quizzes.id LIMIT 1"
    );
    Ok(conns
        .cache
        .query_row(&sql, params![scope], read_quiz)
        .optional()?)
}

fn local_error(session: &Session, err: &DbError) -> Handled {
    emit_error(
        "local",
        &err.to_string(),
        13,
        session.profile.clone(),
        Some(session.identity_ref()),
    )
}

/// Parse an optional i64 from a column or a `data_json` string.
fn json_i64(data: &Value, key: &str) -> Option<i64> {
    match data.get(key)? {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

fn print_quiz_table(quizzes: &[QuizSummaryJson]) -> io::Result<()> {
    if quizzes.is_empty() {
        return writeln!(io::stdout(), "no quizzes");
    }
    let mut table = new_table();
    table.set_header(Row::from(vec![
        "ID",
        "TITLE",
        "TYPE",
        "DUE",
        "QUESTIONS",
        "POINTS",
        "STATE",
    ]));
    for quiz in quizzes {
        table.add_row(Row::from(vec![
            quiz.id.clone(),
            quiz.title.clone().unwrap_or_default(),
            quiz.quiz_type.clone().unwrap_or_default(),
            quiz.due_at.clone().unwrap_or_default(),
            quiz.question_count
                .map_or_else(String::new, |n| n.to_string()),
            quiz.points_possible
                .map_or_else(String::new, |n| n.to_string()),
            match quiz.locked_for_user {
                Some(true) => "locked",
                _ => match quiz.published {
                    Some(false) => "unpublished",
                    _ => "",
                },
            }
            .to_owned(),
        ]));
    }
    apply_two_space_padding(&mut table);
    writeln!(io::stdout(), "{table}")
}

fn print_quiz(quiz: &QuizDetailJson) -> io::Result<()> {
    let mut out = io::stdout();
    writeln!(out, "{}", quiz.title.clone().unwrap_or_default())?;
    if let Some(url) = quiz.html_url.as_deref() {
        writeln!(out, "{url}")?;
    }
    let mut facts = Vec::new();
    if let Some(n) = quiz.question_count {
        facts.push(format!("{n} questions"));
    }
    if let Some(points) = quiz.points_possible {
        facts.push(format!("{points} points"));
    }
    if let Some(minutes) = quiz.time_limit {
        facts.push(format!("{minutes} minutes"));
    }
    if let Some(attempts) = quiz.allowed_attempts {
        facts.push(format!("{attempts} attempts"));
    }
    if !facts.is_empty() {
        writeln!(out, "{}", facts.join(" · "))?;
    }
    if let Some(due) = quiz.due_at.as_deref() {
        writeln!(out, "due {due}")?;
    }
    if let Some(body) = quiz.description_markdown.as_deref() {
        writeln!(out)?;
        writeln!(out, "{body}")?;
    }
    if quiz.truncated {
        writeln!(out, "\n[description truncated; not complete]")?;
    }
    super::pages::print_refs(&mut out, &quiz.embedded, &quiz.files, &quiz.external_links)
}
