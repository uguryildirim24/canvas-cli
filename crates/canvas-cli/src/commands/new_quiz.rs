//! `canvas new-quizzes` and `canvas new-quiz` (class D, M10-b).
//!
//! New Quiz metadata reads: what is due, how many attempts, how long, and
//! the instructions. Taking one happens inside the LTI tool session, which
//! no token reaches, so these commands read and never write. They fetch on
//! demand and keep no cache: the LTI engine owns the state, and the CLI
//! never stores what it cannot reconcile (§19 item 53).

use std::io::{self, Write};
use std::process::ExitCode;

use canvas_api::models::NewQuiz;
use comfy_table::Row;

use super::Globals;
use super::course::resolve_with_refresh;
use super::emit::{base_envelope, emit_error, require_client, session_error, sync_error};
use super::handled::Handled;
use crate::output::{
    NewQuizDetailJson, NewQuizResult, NewQuizSummaryJson, NewQuizzesResult, SCHEMA_NEW_QUIZ,
    SCHEMA_NEW_QUIZZES, apply_two_space_padding, new_table,
};

/// Run `canvas new-quizzes` for the CLI: one envelope, one exit code.
pub async fn run_list(globals: &Globals, course: String) -> ExitCode {
    handle_list(globals, course).await.emit(globals.json)
}

/// Run `canvas new-quizzes <course>`.
pub async fn handle_list(globals: &Globals, course: String) -> Handled {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(e, globals.profile.clone()),
    };
    if globals.offline {
        return emit_error(
            "usage",
            "this command cannot run with --offline",
            2,
            session.profile.clone(),
            Some(session.identity_ref()),
        );
    }
    let (resolved, freshness, _) = match resolve_with_refresh(globals, &session, &course).await {
        Ok(v) => v,
        Err(code) => return code,
    };
    let client = match require_client(globals, &session) {
        Ok(c) => c,
        Err(code) => return code,
    };
    if let Err(e) = session.validate_network_token().await {
        return sync_error(&session, &e);
    }

    let quizzes: Vec<NewQuiz> = match client
        .get(&format!("/api/quiz/v1/courses/{}/quizzes", resolved.id))
        .await
    {
        Ok(quizzes) => quizzes,
        Err(e) => return super::operation::map_operation_error(&session, e.into()),
    };

    let result = NewQuizzesResult {
        course_id: resolved.id.to_string(),
        quizzes: quizzes.iter().map(summarize).collect(),
    };
    let mut envelope = base_envelope(SCHEMA_NEW_QUIZZES, &session, result);
    envelope.freshness = freshness;
    envelope.requests = session.requests();

    Handled::new(envelope, move |envelope| {
        print_new_quiz_table(&envelope.result.quizzes)
    })
}

/// Run `canvas new-quiz` for the CLI: one envelope, one exit code.
pub async fn run_show(globals: &Globals, course: String, quiz: String) -> ExitCode {
    handle_show(globals, course, quiz).await.emit(globals.json)
}

/// Run `canvas new-quiz <course> <assignment-id>`.
pub async fn handle_show(globals: &Globals, course: String, quiz: String) -> Handled {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(e, globals.profile.clone()),
    };
    if globals.offline {
        return emit_error(
            "usage",
            "this command cannot run with --offline",
            2,
            session.profile.clone(),
            Some(session.identity_ref()),
        );
    }
    let (resolved, freshness, _) = match resolve_with_refresh(globals, &session, &course).await {
        Ok(v) => v,
        Err(code) => return code,
    };
    // A New Quiz is addressed by its assignment id: that is the id Canvas
    // prints beside it, and the detail route takes it.
    let assignment_id: i64 = match quiz.trim().parse() {
        Ok(id) => id,
        Err(_) => {
            return emit_error(
                "resolution",
                "a New Quiz is addressed by its assignment id",
                6,
                session.profile.clone(),
                Some(session.identity_ref()),
            );
        }
    };
    let client = match require_client(globals, &session) {
        Ok(c) => c,
        Err(code) => return code,
    };
    if let Err(e) = session.validate_network_token().await {
        return sync_error(&session, &e);
    }

    let item: NewQuiz = match client
        .get(&format!(
            "/api/quiz/v1/courses/{}/quizzes/{assignment_id}",
            resolved.id
        ))
        .await
    {
        Ok(item) => item,
        Err(canvas_api::Error::NotFound) => {
            return emit_error(
                "resolution",
                &format!("no New Quiz {assignment_id} in course {}", resolved.id),
                6,
                session.profile.clone(),
                Some(session.identity_ref()),
            );
        }
        Err(e) => return super::operation::map_operation_error(&session, e.into()),
    };

    let rich = canvas_core::markdown::rich_text_opt(item.instructions.as_deref())
        .await
        .unwrap_or_default();
    let refs = rich.refs.resolve(&session.identity.origin);
    let detail = NewQuizDetailJson {
        id: item.id.to_string(),
        course_id: resolved.id.to_string(),
        assignment_id: item.assignment_id.map(|id| id.to_string()),
        title: item.title.clone(),
        points_possible: item.points_possible,
        time_limit_seconds: item.time_limit_seconds(),
        max_attempts: item
            .quiz_settings
            .as_ref()
            .and_then(|s| s.multiple_attempts.as_ref())
            .and_then(|a| a.max_attempts)
            .map(|n| n.to_string()),
        score_to_keep: item
            .quiz_settings
            .as_ref()
            .and_then(|s| s.multiple_attempts.as_ref())
            .and_then(|a| a.score_to_keep.clone()),
        one_at_a_time: item
            .quiz_settings
            .as_ref()
            .and_then(|s| s.one_at_a_time_type.clone()),
        shuffle_answers: item.quiz_settings.as_ref().and_then(|s| s.shuffle_answers),
        access_code_required: item
            .quiz_settings
            .as_ref()
            .and_then(|s| s.require_student_access_code),
        published: item.published,
        due_at: item.due_at.map(|t| t.to_string()),
        unlock_at: item.unlock_at.map(|t| t.to_string()),
        lock_at: item.lock_at.map(|t| t.to_string()),
        html_url: item.html_url.clone(),
        instructions_markdown: rich.markdown.clone(),
        truncated: rich.refs.truncated,
        embedded: super::pages::embedded_json(&refs),
        files: super::pages::files_json(&refs),
        external_links: super::pages::external_json(&refs),
    };

    let mut envelope = base_envelope(SCHEMA_NEW_QUIZ, &session, NewQuizResult { quiz: detail });
    envelope.freshness = freshness;
    envelope.requests = session.requests();
    super::pages::truncation_partial(
        &mut envelope,
        rich.refs.truncated,
        &format!("new-quiz:{assignment_id}"),
    );

    Handled::new(envelope, move |envelope| {
        print_new_quiz(&envelope.result.quiz)
    })
}

fn summarize(item: &NewQuiz) -> NewQuizSummaryJson {
    NewQuizSummaryJson {
        id: item.id.to_string(),
        assignment_id: item.assignment_id.map(|id| id.to_string()),
        title: item.title.clone(),
        due_at: item.due_at.map(|t| t.to_string()),
        points_possible: item.points_possible,
        time_limit_seconds: item.time_limit_seconds(),
        max_attempts: item
            .quiz_settings
            .as_ref()
            .and_then(|s| s.multiple_attempts.as_ref())
            .and_then(|a| a.max_attempts)
            .map(|n| n.to_string()),
        published: item.published,
    }
}

fn print_new_quiz_table(quizzes: &[NewQuizSummaryJson]) -> io::Result<()> {
    if quizzes.is_empty() {
        return writeln!(io::stdout(), "no New Quizzes");
    }
    let mut table = new_table();
    table.set_header(Row::from(vec![
        "ID",
        "ASSIGNMENT",
        "TITLE",
        "DUE",
        "POINTS",
        "TIME",
        "ATTEMPTS",
    ]));
    for quiz in quizzes {
        table.add_row(Row::from(vec![
            quiz.id.clone(),
            quiz.assignment_id.clone().unwrap_or_default(),
            quiz.title.clone().unwrap_or_default(),
            quiz.due_at.clone().unwrap_or_default(),
            quiz.points_possible
                .map_or_else(String::new, |n| n.to_string()),
            quiz.time_limit_seconds
                .map_or_else(String::new, |s| format!("{}m", s / 60)),
            quiz.max_attempts.clone().unwrap_or_default(),
        ]));
    }
    apply_two_space_padding(&mut table);
    writeln!(io::stdout(), "{table}")
}

fn print_new_quiz(quiz: &NewQuizDetailJson) -> io::Result<()> {
    let mut out = io::stdout();
    writeln!(out, "{}", quiz.title.clone().unwrap_or_default())?;
    writeln!(out, "New Quiz — taken in the browser")?;
    if let Some(url) = quiz.html_url.as_deref() {
        writeln!(out, "{url}")?;
    }
    let mut facts = Vec::new();
    if let Some(points) = quiz.points_possible {
        facts.push(format!("{points} points"));
    }
    if let Some(seconds) = quiz.time_limit_seconds {
        facts.push(format!("{} minutes", seconds / 60));
    }
    if let Some(attempts) = quiz.max_attempts.as_deref() {
        facts.push(format!("{attempts} attempts"));
    }
    if !facts.is_empty() {
        writeln!(out, "{}", facts.join(" · "))?;
    }
    if let Some(due) = quiz.due_at.as_deref() {
        writeln!(out, "due {due}")?;
    }
    if let Some(body) = quiz.instructions_markdown.as_deref() {
        writeln!(out)?;
        writeln!(out, "{body}")?;
    }
    if quiz.truncated {
        writeln!(out, "\n[instructions truncated; not complete]")?;
    }
    super::pages::print_refs(&mut out, &quiz.embedded, &quiz.files, &quiz.external_links)
}
