//! `canvas quiz questions` (class D, M10-a).
//!
//! Join the session in progress, or start one first. Starting begins an
//! attempt and, on a timed quiz, the clock — so it is confirmed at the
//! terminal, or recorded with `--yes`. Reading a live session is a plain read
//! and asks nothing.

use std::io::{self, Write};
use std::process::ExitCode;

use canvas_core::markdown::rich_text_opt;
use serde_json::Value;

use super::Globals;
use super::course::{refresh_fail, resolve_with_refresh};
use super::course_load::outcome_freshness;
use super::emit::{base_envelope, emit_error, require_client, session_error, sync_error};
use super::handled::Handled;
use super::quizzes::{ensure_quizzes, quiz_id_of};
use super::submit::confirm_tty;
use crate::output::{
    QuizAnswerOptionJson, QuizQuestionJson, QuizQuestionsResult, SCHEMA_QUIZ_QUESTIONS,
};

/// Operands and flags of one `canvas quiz questions`.
#[derive(Debug, Clone, Default)]
pub struct QuizQuestionsArgs {
    pub course: String,
    pub quiz: String,
    pub access_code: Option<String>,
    pub yes: bool,
}

/// Run `canvas quiz questions` for the CLI: one envelope, one exit code.
pub async fn run_questions(globals: &Globals, args: QuizQuestionsArgs) -> ExitCode {
    handle_questions(globals, args).await.emit(globals.json)
}

/// Join or start the session, then print its censored questions.
pub async fn handle_questions(globals: &Globals, args: QuizQuestionsArgs) -> Handled {
    if globals.offline {
        return emit_error(
            "usage",
            "this command cannot run with --offline",
            2,
            globals.profile.clone(),
            None,
        );
    }
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(e, globals.profile.clone()),
    };
    let (resolved, mut freshness, _) =
        match resolve_with_refresh(globals, &session, &args.course).await {
            Ok(v) => v,
            Err(code) => return code,
        };

    let outcome = match ensure_quizzes(globals, &session, resolved.id).await {
        Ok(o) => o,
        Err(e) => return refresh_fail(&session, e),
    };
    freshness.push(outcome_freshness(&outcome));

    let quiz_id = match quiz_id_of(&session, resolved.id, &args.quiz).await {
        Ok(id) => id,
        Err(code) => return code,
    };

    let client = match require_client(globals, &session) {
        Ok(c) => c,
        Err(code) => return code,
    };
    if let Err(e) = session.validate_network_token().await {
        return sync_error(&session, &e);
    }

    // A live session is a read. Starting one is the person's decision: the
    // terminal is asked, or `--yes` records it.
    let live = match canvas_core::quiz::own_submission(client, resolved.id, quiz_id).await {
        Ok(found) => found.is_some_and(|s| s.is_live()),
        Err(e) => {
            return super::operation::map_operation_error(&session, e);
        }
    };
    if !live {
        let proceed = if args.yes {
            true
        } else {
            match confirm_tty("Start this quiz? This begins an attempt and may start the clock.")
                .await
            {
                Ok(true) => true,
                Ok(false) => {
                    return emit_error(
                        "cancelled",
                        "quiz not started",
                        11,
                        session.profile.clone(),
                        Some(session.identity_ref()),
                    );
                }
                Err(message) => {
                    return emit_error(
                        "usage",
                        &message,
                        2,
                        session.profile.clone(),
                        Some(session.identity_ref()),
                    );
                }
            }
        };
        if !proceed {
            return emit_error(
                "cancelled",
                "quiz not started",
                11,
                session.profile.clone(),
                Some(session.identity_ref()),
            );
        }
    }

    let (live_session, started) = match canvas_core::quiz::live_session(
        client,
        &session.open.store,
        session.identity.key.as_str(),
        resolved.id,
        quiz_id,
        args.access_code.as_deref(),
    )
    .await
    {
        Ok(v) => v,
        Err(e) => return super::operation::map_operation_error(&session, e),
    };

    let questions =
        match canvas_core::quiz::questions(client, live_session.quiz_submission_id).await {
            Ok(questions) => questions,
            Err(e) => return super::operation::map_operation_error(&session, e),
        };

    let mut rendered = Vec::with_capacity(questions.len());
    for question in &questions {
        let text = question
            .question_text
            .as_ref()
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let rich = match text {
            Some(html) => rich_text_opt(Some(html)).await.unwrap_or_default(),
            None => canvas_core::markdown::RichText::default(),
        };
        let mut options = Vec::new();
        if let Some(list) = question.answers.as_ref().and_then(|v| v.as_array()) {
            for option in list {
                options.push(QuizAnswerOptionJson {
                    id: option.get("id").and_then(|v| {
                        v.as_i64()
                            .map(|n| n.to_string())
                            .or_else(|| v.as_str().map(str::to_owned))
                    }),
                    text: option
                        .get("text")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .or_else(|| option.get("html").and_then(Value::as_str).map(strip_tags)),
                });
            }
        }
        rendered.push(QuizQuestionJson {
            id: question.id.to_string(),
            position: question.position,
            name: question.question_name.clone(),
            question_type: question.question_type.clone(),
            points_possible: question.points_possible,
            text_markdown: rich.markdown.clone(),
            answer: question.answer.clone(),
            answers: options,
        });
    }

    let result = QuizQuestionsResult {
        course_id: resolved.id.to_string(),
        quiz_id: quiz_id.to_string(),
        attempt: Some(live_session.attempt),
        started_at: live_session.started_at.clone(),
        end_at: live_session.end_at.clone(),
        started,
        questions: rendered,
    };
    let mut envelope = base_envelope(SCHEMA_QUIZ_QUESTIONS, &session, result);
    envelope.freshness = freshness;
    envelope.requests = session.requests();
    if outcome.freshness.stale {
        envelope.warnings.push("served stale quizzes cache".into());
    }

    Handled::new(envelope, move |envelope| print_questions(&envelope.result))
}

/// Strip tags the way an agent needs: the option text is plain already, and
/// only a missing one falls back to the HTML.
fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut inside = false;
    for ch in html.chars() {
        match ch {
            '<' => inside = true,
            '>' => inside = false,
            _ => {
                if !inside {
                    out.push(ch);
                }
            }
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn print_questions(result: &QuizQuestionsResult) -> io::Result<()> {
    let mut out = io::stdout();
    writeln!(
        out,
        "attempt {} of quiz {}",
        result.attempt.unwrap_or(0),
        result.quiz_id
    )?;
    if let Some(end) = result.end_at.as_deref() {
        writeln!(out, "attempt ends {end}")?;
    }
    for question in &result.questions {
        writeln!(out)?;
        writeln!(
            out,
            "Q{}  {}  {} pts  {}",
            question.id,
            question.question_type.as_deref().unwrap_or("unknown type"),
            question.points_possible.unwrap_or(0.0),
            question.name.as_deref().unwrap_or("")
        )?;
        if let Some(text) = question.text_markdown.as_deref() {
            writeln!(out, "{text}")?;
        }
        for option in &question.answers {
            writeln!(
                out,
                "  [{}] {}",
                option.id.as_deref().unwrap_or("?"),
                option.text.as_deref().unwrap_or("")
            )?;
        }
        if question.answer.is_some() {
            writeln!(out, "  (already answered)")?;
        }
    }
    Ok(())
}
