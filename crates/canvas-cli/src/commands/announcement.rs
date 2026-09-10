//! `canvas announcement` (class C, §12.6).

use std::io::{self, Write};
use std::process::ExitCode;

use canvas_core::resolve::canvas_url;
use canvas_core::store::lookup_dataset;
use canvas_core::sync::{AnnouncementDetailDataset, RefreshOutcome, refresh_announcement};
use jiff::tz::TimeZone;

use super::announcements::{AnnouncementRow, load_announcement};
use super::course_load::{RefreshFail, cached_outcome, outcome_freshness};
use super::emit::{base_envelope, emit, emit_error, session_error};
use super::{Globals, assignment_read as read};
use crate::output::{
    AnnouncementDetailJson, AnnouncementResult, SCHEMA_ANNOUNCEMENT, now_timestamp,
};
use crate::session::{Session, ttl_announcements};

/// Run `canvas announcement <course> <id>` or `canvas announcement <url>`.
pub async fn run(globals: &Globals, target: String, id: Option<String>) -> ExitCode {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(globals.json, e, globals.profile.clone()),
    };

    let mut freshness = Vec::new();
    let (course_id, announcement_id) = match id {
        Some(id) => {
            let Some(announcement_id) = parse_id(&id) else {
                return refuse_bare_id(globals, &session);
            };
            let (resolved, rows, _) =
                match super::course::resolve_with_refresh(globals, &session, &target).await {
                    Ok(v) => v,
                    Err(code) => return code,
                };
            freshness.extend(rows);
            (resolved.id, announcement_id)
        }
        None => match parse_announcement_url(&target, &session.identity.origin) {
            Ok(Some(pair)) => pair,
            // A bare id, a name, or a URL of another shape: §5 refuses it.
            Ok(None) => return refuse_bare_id(globals, &session),
            Err(()) => {
                return emit_error(
                    globals.json,
                    "resolution",
                    "url origin does not match the active identity",
                    6,
                    session.profile.clone(),
                    Some(session.identity_ref()),
                );
            }
        },
    };

    let outcome = match ensure_announcement(globals, &session, course_id, announcement_id).await {
        Ok(o) => o,
        Err(e) => return super::course::refresh_fail(globals, &session, e),
    };
    freshness.push(outcome_freshness(&outcome));

    let row = match session
        .open
        .store
        .call(move |conns| load_announcement(conns, announcement_id))
        .await
    {
        Ok(Some(row)) => row,
        Ok(None) => {
            return emit_error(
                globals.json,
                "resolution",
                &format!("announcement {announcement_id} not found"),
                6,
                session.profile.clone(),
                Some(session.identity_ref()),
            );
        }
        Err(e) => {
            return emit_error(
                globals.json,
                "local",
                &e.to_string(),
                13,
                session.profile.clone(),
                Some(session.identity_ref()),
            );
        }
    };

    let zone = read::zone(&session);
    let announcement = detail_json(&row, &zone).await;
    let result = AnnouncementResult { announcement };
    let mut envelope = base_envelope(SCHEMA_ANNOUNCEMENT, &session, result);
    envelope.freshness = freshness;
    envelope.requests = session.requests();
    if outcome.freshness.stale {
        envelope
            .warnings
            .push("served stale announcement cache".into());
    }
    if let Some(error) = outcome.error.clone() {
        envelope.warnings.push(error);
    }

    emit(globals.json, &envelope, || {
        print_human(&envelope.result.announcement)
    })
}

/// Build the detail payload, converting the message to Markdown.
async fn detail_json(row: &AnnouncementRow, zone: &TimeZone) -> AnnouncementDetailJson {
    let message_markdown = match row.message.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(html) => canvas_core::markdown::html_to_markdown(html)
            .await
            .ok()
            .filter(|s| !s.is_empty()),
        None => None,
    };
    AnnouncementDetailJson {
        item: row.to_json(zone),
        message_markdown,
    }
}

fn parse_id(raw: &str) -> Option<i64> {
    raw.parse::<i64>().ok().filter(|id| *id > 0)
}

/// `/courses/:cid/discussion_topics/:id` on the identity origin.
fn parse_announcement_url(input: &str, origin: &str) -> Result<Option<(i64, i64)>, ()> {
    let url = canvas_url(input, origin).map_err(|_| ())?;
    let Some(url) = url else {
        return Ok(None);
    };
    let parts: Vec<_> = url.path_segments().into_iter().flatten().collect();
    Ok(parts
        .windows(4)
        .find(|p| p[0] == "courses" && p[2] == "discussion_topics")
        .and_then(|p| Some((p[1].parse().ok()?, p[3].parse().ok()?))))
}

fn refuse_bare_id(globals: &Globals, session: &Session) -> ExitCode {
    emit_error(
        globals.json,
        "resolution",
        "bare announcement IDs are not accepted; use <course> <id> or a URL",
        6,
        session.profile.clone(),
        Some(session.identity_ref()),
    )
}

async fn ensure_announcement(
    globals: &Globals,
    session: &Session,
    course_id: i64,
    id: i64,
) -> Result<RefreshOutcome, RefreshFail> {
    let now = now_timestamp();
    let ttl = ttl_announcements();
    let dataset = AnnouncementDetailDataset::new(course_id, id, ttl);
    let lookup = session
        .open
        .store
        .call(move |conns| lookup_dataset(conns, &dataset, now, None))
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
    refresh_announcement(
        client,
        &session.open.store,
        course_id,
        id,
        ttl,
        now,
        globals.fresh,
        false,
    )
    .await
    .map_err(RefreshFail::Sync)
}

fn print_human(detail: &AnnouncementDetailJson) -> io::Result<()> {
    let item = &detail.item;
    let mut out = io::stdout();
    let heading = match &item.course_code {
        Some(code) => format!("[{code}] {}", item.title),
        None => item.title.clone(),
    };
    writeln!(out, "{heading}")?;
    if let Some(posted) = item.posted_at_local.as_deref() {
        writeln!(out, "posted {posted}")?;
    }
    if let Some(author) = item.author.as_deref() {
        writeln!(out, "by {author}")?;
    }
    writeln!(out, "{}", if item.read { "read" } else { "unread" })?;
    if let Some(url) = item.html_url.as_deref() {
        writeln!(out, "{url}")?;
    }
    if let Some(message) = detail.message_markdown.as_deref() {
        writeln!(out)?;
        writeln!(out, "{message}")?;
    }
    Ok(())
}
