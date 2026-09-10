//! `canvas assignment` (class C).
use super::emit::{base_envelope, emit, emit_error, session_error, sync_error};
use super::{Globals, assignment_read as read};
use crate::output::{SCHEMA_ASSIGNMENT, now_timestamp};
use crate::session::ttl_assignments;
use serde_json::{Value, json};
use std::io::{self, Write};
use std::process::ExitCode;

#[allow(clippy::too_many_lines)]
pub async fn run(globals: &Globals, target: String, assignment: Option<String>) -> ExitCode {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(globals.json, e, globals.profile.clone()),
    };
    if assignment.is_none() && !target.contains("://") {
        return emit_error(
            globals.json,
            "usage",
            "assignment requires a course and assignment or a URL",
            2,
            session.profile.clone(),
            Some(session.identity_ref()),
        );
    }
    let (course, mut freshness, _) =
        match super::course::resolve_with_refresh(globals, &session, &target).await {
            Ok(v) => v,
            Err(e) => return e,
        };
    let mut outcomes = Vec::new();
    let input = assignment.as_deref().unwrap_or(&target);
    let id = match read::resolve(&session, globals, course.id, input, &mut outcomes).await {
        Ok(id) => id,
        Err(e) => return e,
    };
    match read::detail(&session, globals, course.id, id).await {
        Ok(o) => outcomes.push(o),
        Err(e) => return sync_error(globals, &session, &e),
    }
    let now = now_timestamp();
    let item = match session
        .open
        .store
        .call(move |conns| {
            canvas_core::todo::load_assignment_item(conns, id, now, ttl_assignments())
        })
        .await
    {
        Ok(Some(i)) => i,
        Ok(None) => return sync_error(globals, &session, &canvas_api::Error::Decode.into()),
        Err(e) => return sync_error(globals, &session, &e.into()),
    };
    let mut rubric_assessed = false;
    let mut rubric_assessment = Vec::<Value>::new();
    let mut comments_count = None;
    if item.status.graded == Some(true) {
        match read::submission(&session, globals, course.id, id).await {
            Ok(o) => {
                outcomes.push(o);
                let data=session.open.store.call(move|conns| {
                    let raw: String=conns.cache.query_row("SELECT s.data_json FROM membership m JOIN submissions s ON s.id=CAST(m.entity_id AS INTEGER) WHERE m.dataset='submission' AND m.scope=?1",[format!("assignment:{id}")],|r|r.get(0))?;
                    serde_json::from_str::<Value>(&raw).map_err(|_|canvas_core::store::DbError::Message("invalid submission cache".into()))
                }).await;
                match data {
                    Ok(v) => {
                        rubric_assessed = v["rubric_assessment_json"].is_array();
                        rubric_assessment = v["rubric_assessment_json"]
                            .as_array()
                            .cloned()
                            .unwrap_or_default();
                        comments_count = v["submission_comments_json"].as_array().map(Vec::len);
                    }
                    Err(e) => return sync_error(globals, &session, &e.into()),
                }
            }
            Err(e) => return sync_error(globals, &session, &e),
        }
    }
    let description = match item.details.description.as_deref() {
        Some(html) => match canvas_core::markdown::html_to_markdown(html).await {
            Ok(md) => Some(md),
            Err(_) => return sync_error(globals, &session, &canvas_api::Error::Decode.into()),
        },
        None => None,
    };
    let zone = read::zone(&session);
    let mut value = read::assignment_json(&item, &zone);
    let obj = value.as_object_mut().expect("assignment object");
    obj.extend(json!({"description_markdown":description,"can_submit":item.details.can_submit,"extra_attempts":item.details.extra_attempts,"rubric":item.details.rubric,"rubric_assessed":rubric_assessed,"rubric_assessment":rubric_assessment,"comments_count":comments_count,"external_tool_name":item.details.external_tool_name}).as_object().unwrap().clone());
    let mut envelope = base_envelope(SCHEMA_ASSIGNMENT, &session, json!({"assignment":value}));
    freshness.extend(outcomes.iter().map(super::course_load::outcome_freshness));
    envelope.freshness = freshness;
    envelope
        .warnings
        .extend(outcomes.into_iter().filter_map(|o| o.error));
    emit(globals.json, &envelope, || {
        writeln!(
            io::stdout(),
            "{}",
            read::human_table(std::slice::from_ref(&item), &zone, read::use_color(globals))
        )?;
        if let Some(md) = &description {
            writeln!(io::stdout(), "\n{md}")?;
        }
        writeln!(
            io::stdout(),
            "Submission types: {}\nAllowed extensions: {}\nAttempts: {}, extra: {}\ncan_submit: {}",
            item.details
                .submission_types
                .clone()
                .unwrap_or_default()
                .join(", "),
            item.details.allowed_extensions.join(", "),
            item.details
                .allowed_attempts
                .map_or_else(|| "unknown".into(), |v| v.to_string()),
            item.details
                .extra_attempts
                .map_or_else(|| "unknown".into(), |v| v.to_string()),
            item.details
                .can_submit
                .map_or_else(|| "unknown".into(), |v| v.to_string())
        )?;
        writeln!(
            io::stdout(),
            "Unlock: {}\nLock: {}",
            read::local(item.availability.unlock_at, &zone).unwrap_or_else(|| "unknown".into()),
            read::local(item.availability.lock_at, &zone).unwrap_or_else(|| "unknown".into())
        )?;
        for criterion in &item.details.rubric {
            writeln!(
                io::stdout(),
                "Rubric: {} ({})",
                criterion["description"].as_str().unwrap_or(""),
                criterion["points"]
            )?;
        }
        for assessment in &rubric_assessment {
            writeln!(
                io::stdout(),
                "Feedback: {} {} {}",
                assessment["criterion_id"].as_str().unwrap_or(""),
                assessment["points"],
                assessment["comments"].as_str().unwrap_or("")
            )?;
        }
        if item.availability.external == Some(true) {
            writeln!(
                io::stdout(),
                "External tool: {}\ncanvas open assignment {} {}",
                item.details
                    .external_tool_name
                    .as_deref()
                    .unwrap_or("unknown"),
                course.id,
                id
            )?;
        }
        if let Some(url) = &item.html_url {
            writeln!(io::stdout(), "{url}")?;
        }
        Ok(())
    })
}
