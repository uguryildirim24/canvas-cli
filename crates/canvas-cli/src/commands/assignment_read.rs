//! Shared cache-or-network orchestration and payloads for assignment reads.
use super::{
    Globals,
    emit::{call_resolve, resolve_error, sync_error},
};
use crate::session::{Session, ttl_assignments, ttl_missing, ttl_planner};
use canvas_core::resolve::{CommandClass, ResolveError, resolve_assignment};
use canvas_core::store::{Dataset, LookupResult, WindowQuery, lookup_dataset};
use canvas_core::sync::{self, RefreshOutcome, SyncError};
use canvas_core::todo::TodoItem;
use jiff::{Timestamp, tz::TimeZone};
use serde_json::{Value, json};
use std::process::ExitCode;

pub async fn cached<D: Dataset + Clone + Send + 'static>(
    session: &Session,
    dataset: D,
    window: Option<sync::PlannerWindow>,
    globals: &Globals,
) -> Result<Option<RefreshOutcome>, SyncError> {
    let now = crate::output::now_timestamp();
    let lookup = session
        .open
        .store
        .call(move |conns| {
            lookup_dataset(
                conns,
                &dataset,
                now,
                window.as_ref().map(|w| WindowQuery {
                    start: w.start_timestamp(),
                    end: w.end_timestamp(),
                    contexts: "",
                }),
            )
        })
        .await?;
    let hit = matches!(&lookup, LookupResult::Hit(_));
    if globals.offline || (hit && !globals.fresh) {
        let row = match lookup {
            LookupResult::Hit(row) | LookupResult::Stale(row) if row.complete => row,
            _ => return Err(SyncError::OfflineMiss),
        };
        return Ok(Some(RefreshOutcome {
            freshness: sync::FreshnessInfo {
                dataset: row.dataset,
                scope: row.scope,
                source: sync::FreshnessSource::Cache,
                fetched_at: row.fetched_at,
                complete: row.complete,
                count: row.count,
                stale: globals.offline || !hit,
            },
            requests: 0,
            error: row.error,
        }));
    }
    session.validate_network_token().await?;
    Ok(None)
}

pub async fn assignments(
    session: &Session,
    globals: &Globals,
    course_id: i64,
) -> Result<RefreshOutcome, SyncError> {
    if let Some(out) = cached(
        session,
        sync::AssignmentsDataset::new(course_id, ttl_assignments()),
        None,
        globals,
    )
    .await?
    {
        return Ok(out);
    }
    sync::refresh_assignments(
        session
            .client
            .as_ref()
            .ok_or(canvas_api::Error::Unauthorized)?,
        &session.open.store,
        course_id,
        ttl_assignments(),
        crate::output::now_timestamp(),
        globals.fresh,
        false,
    )
    .await
}
pub async fn detail(
    session: &Session,
    globals: &Globals,
    course_id: i64,
    id: i64,
) -> Result<RefreshOutcome, SyncError> {
    let observed = session
        .open
        .store
        .call(move |conns| {
            Ok(canvas_core::todo::assignment_observations(conns)?
                .get(&id)
                .copied())
        })
        .await?;
    let now = crate::output::now_timestamp();
    let field_fresh = observed.is_some_and(|at| {
        at <= now
            && at
                .checked_add(ttl_assignments())
                .is_ok_and(|expiry| now <= expiry)
    });
    let mut options = globals.clone();
    options.fresh |= !field_fresh && !globals.offline;
    if let Some(mut out) = cached(
        session,
        sync::AssignmentDetailDataset::new(course_id, id, ttl_assignments()),
        None,
        &options,
    )
    .await?
    {
        out.freshness.stale |= !field_fresh;
        return Ok(out);
    }
    let mut out = sync::refresh_assignment(
        session
            .client
            .as_ref()
            .ok_or(canvas_api::Error::Unauthorized)?,
        &session.open.store,
        course_id,
        id,
        ttl_assignments(),
        now,
        options.fresh,
        false,
    )
    .await?;
    let supplied = session
        .open
        .store
        .call(move |conns| {
            Ok(canvas_core::todo::assignment_observations(conns)?
                .get(&id)
                .copied())
        })
        .await?;
    out.freshness.stale |= supplied.is_none_or(|at| {
        at > now
            || !at
                .checked_add(ttl_assignments())
                .is_ok_and(|expiry| now <= expiry)
    });
    Ok(out)
}
pub async fn missing(session: &Session, globals: &Globals) -> Result<RefreshOutcome, SyncError> {
    if let Some(out) = cached(
        session,
        sync::MissingDataset::new(ttl_missing()),
        None,
        globals,
    )
    .await?
    {
        return Ok(out);
    }
    sync::refresh_missing(
        session
            .client
            .as_ref()
            .ok_or(canvas_api::Error::Unauthorized)?,
        &session.open.store,
        ttl_missing(),
        crate::output::now_timestamp(),
        globals.fresh,
        false,
    )
    .await
}
pub async fn planner(
    session: &Session,
    globals: &Globals,
    window: sync::PlannerWindow,
) -> Result<RefreshOutcome, SyncError> {
    if let Some(out) = cached(
        session,
        sync::PlannerDataset::new(window.clone(), ttl_planner()),
        Some(window.clone()),
        globals,
    )
    .await?
    {
        return Ok(out);
    }
    sync::refresh_planner(
        session
            .client
            .as_ref()
            .ok_or(canvas_api::Error::Unauthorized)?,
        &session.open.store,
        window,
        ttl_planner(),
        crate::output::now_timestamp(),
        globals.fresh,
        false,
    )
    .await
}
pub async fn submission(
    session: &Session,
    globals: &Globals,
    course_id: i64,
    id: i64,
) -> Result<RefreshOutcome, SyncError> {
    if let Some(out) = cached(
        session,
        sync::SubmissionDataset::new(course_id, id, ttl_assignments()),
        None,
        globals,
    )
    .await?
    {
        return Ok(out);
    }
    sync::refresh_submission(
        session
            .client
            .as_ref()
            .ok_or(canvas_api::Error::Unauthorized)?,
        &session.open.store,
        course_id,
        id,
        ttl_assignments(),
        crate::output::now_timestamp(),
        globals.fresh,
        false,
    )
    .await
}
pub async fn resolve(
    session: &Session,
    globals: &Globals,
    course_id: i64,
    input: &str,
    freshness: &mut Vec<RefreshOutcome>,
) -> Result<i64, ExitCode> {
    let mut fetched = false;
    loop {
        let origin = session.identity.origin.clone();
        let input = input.to_owned();
        match call_resolve(session, move |conns| {
            resolve_assignment(conns, course_id, &input, &origin, CommandClass::C)
        })
        .await
        {
            Ok(Ok(a)) => return Ok(a.id),
            Ok(Err(ResolveError::IncompleteDataset { .. })) if !fetched => {
                freshness.push(
                    assignments(session, globals, course_id)
                        .await
                        .map_err(|e| sync_error(globals, session, &e))?,
                );
                fetched = true;
            }
            Ok(Err(e)) => return Err(resolve_error(globals, session, &e)),
            Err(e) => return Err(sync_error(globals, session, &e.into())),
        }
    }
}

pub fn zone(session: &Session) -> TimeZone {
    session.time_zone()
}
pub fn local(at: Option<Timestamp>, zone: &TimeZone) -> Option<String> {
    at.map(|t| {
        let local = t.to_zoned(zone.clone());
        format!("{}{}", local.datetime(), local.strftime("%:z"))
    })
}
pub fn status(item: &TodoItem, zone: &TimeZone) -> Value {
    let s = &item.status;
    json!({"submitted":s.submitted,"graded":s.graded,"score":s.score,"grade":s.grade,"late":s.late,"missing":s.missing,"excused":s.excused,"workflow_state":s.workflow_state,"submitted_at":s.submitted_at.map(|t|t.to_string()),"submitted_at_local":local(s.submitted_at,zone),"attempt":s.attempt,"posted_at":s.posted_at.map(|t|t.to_string()),"pending":s.pending})
}
pub fn availability(item: &TodoItem, zone: &TimeZone) -> Value {
    let a = &item.availability;
    json!({"locked":item.status.locked,"lock_explanation":a.lock_explanation,"submittable":a.submittable,"external":a.external,"unlock_at":a.unlock_at.map(|t|t.to_string()),"unlock_at_local":local(a.unlock_at,zone),"lock_at":a.lock_at.map(|t|t.to_string()),"lock_at_local":local(a.lock_at,zone)})
}
pub fn assignment_json(item: &TodoItem, zone: &TimeZone) -> Value {
    let d = &item.details;
    json!({"id":item.id.to_string(),"course_id":item.course_id.map(|id|id.to_string()),"name":item.title,"due_at":item.due_at.map(|t|t.to_string()),"due_at_local":local(item.due_at,zone),"points_possible":item.points_possible,"submission_types":d.submission_types.clone().unwrap_or_default(),"allowed_extensions":d.allowed_extensions,"allowed_attempts":d.allowed_attempts,"group_assignment":d.group_assignment,"availability":availability(item,zone),"status":status(item,zone),"html_url":item.html_url})
}
pub fn label(item: &TodoItem) -> String {
    if item.status.pending {
        return "pending · unknown".into();
    }
    let mut labels = Vec::new();
    if item.status.missing {
        labels.push("missing".into());
    }
    if item.dismissed {
        labels.push("dismissed".into());
    }
    if item.status.late == Some(true) {
        labels.push("late".into());
    }
    if item.status.graded == Some(true) {
        labels.push(format!(
            "graded {}/{}",
            item.status
                .score
                .map_or_else(|| "?".into(), |v| v.to_string()),
            item.points_possible
                .map_or_else(|| "?".into(), |v| v.to_string())
        ));
    } else if item.status.submitted == Some(true) {
        labels.push("submitted".into());
    }
    if item.status.locked == Some(true) {
        labels.push("locked".into());
    } else if item
        .availability
        .lock_at
        .is_some_and(|t| t < crate::output::now_timestamp())
    {
        labels.push("closed".into());
    }
    if item.availability.external == Some(true) {
        labels.push("external".into());
    }
    if labels.is_empty() {
        labels.push("unknown".into());
    }
    labels.join(" · ")
}
pub fn human_table(items: &[TodoItem], zone: &TimeZone, color: bool) -> String {
    let mut table = crate::output::new_table();
    table.set_header(["ID", "Course", "Name", "Due", "Status"]);
    for item in items {
        table.add_row([
            item.id.to_string(),
            item.course_code.clone().unwrap_or_default(),
            item.title.clone(),
            item.due_at.map_or_else(
                || "—".into(),
                |t| crate::output::format_local_datetime(t, zone),
            ),
            crate::output::paint(
                &label(item),
                crate::output::apply_status_style(style(item)),
                color,
            ),
        ]);
    }
    crate::output::apply_two_space_padding(&mut table);
    table.to_string()
}

fn style(item: &TodoItem) -> crate::output::StatusKind {
    use crate::output::StatusKind;
    let now = crate::output::now_timestamp();
    if item.status.pending {
        StatusKind::Pending
    } else if item.status.missing {
        StatusKind::Missing
    } else if item.status.submitted == Some(true) {
        StatusKind::Submitted
    } else if item.status.locked == Some(true) {
        StatusKind::Locked
    } else if item.availability.lock_at.is_some_and(|t| t < now) {
        StatusKind::Closed
    } else if item.due_at.is_some_and(|t| t < now) {
        StatusKind::Overdue
    } else if item
        .due_at
        .is_some_and(|t| t.as_second() - now.as_second() < 86400)
    {
        StatusKind::DueSoon
    } else {
        StatusKind::Unknown
    }
}
pub fn use_color(globals: &Globals) -> bool {
    use std::io::IsTerminal;
    crate::output::resolve_color(globals.json, globals.color, std::io::stdout().is_terminal())
}
pub fn human_todo(items: &[TodoItem], zone: &TimeZone, color: bool) -> String {
    let mut out = String::new();
    let mut start = 0;
    while start < items.len() {
        let day = items[start]
            .scheduled_at
            .or(items[start].due_at)
            .map(|t| t.to_zoned(zone.clone()).date());
        let end = items[start..]
            .iter()
            .position(|item| {
                item.scheduled_at
                    .or(item.due_at)
                    .map(|t| t.to_zoned(zone.clone()).date())
                    != day
            })
            .map_or(items.len(), |n| start + n);
        out.push_str(&day.map_or_else(|| "Undated".into(), |d| d.to_string()));
        out.push('\n');
        out.push_str(&human_table(&items[start..end], zone, color));
        out.push('\n');
        start = end;
    }
    out
}

/// Refresh an aged eligibility value only from the endpoint that supplies it.
pub async fn refresh_stale_eligibility(
    session: &Session,
    globals: &Globals,
    course_ids: Vec<i64>,
    planner_scope: Option<String>,
) -> Result<Vec<RefreshOutcome>, SyncError> {
    if globals.offline {
        return Ok(Vec::new());
    }
    let now = crate::output::now_timestamp();
    let ids = session
        .open
        .store
        .call(move |conns| {
            let mut eligible=std::collections::BTreeSet::new();
            for cid in &course_ids {
                let mut stmt=conns.cache.prepare("SELECT CAST(entity_id AS INTEGER) FROM membership WHERE dataset='assignments' AND scope=?1 AND entity_kind='assignment'")?;
                eligible.extend(stmt.query_map([format!("course:{cid}")],|r|r.get::<_,i64>(0))?.collect::<Result<Vec<_>,_>>()?);
            }
            if let Some(scope)=planner_scope {
                eligible.extend(canvas_core::todo::load_missing_rows(conns)?.into_iter().map(|r|r.id));
                eligible.extend(canvas_core::todo::load_planner_rows(conns,&scope)?.into_iter().filter_map(|r|r.assignment.map(|a|a.id)));
            }
            let observations = canvas_core::todo::assignment_observations(conns)?;
            let mut stmt = conns
                .cache
                .prepare("SELECT id,course_id FROM assignments WHERE can_submit IS NOT NULL")?;
            let rows = stmt
                .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows
                .into_iter()
                .filter(|(id, _)| {
                    eligible.contains(id)
                        && observations.get(id).is_none_or(|at| {
                            *at > now
                                || !at
                                    .checked_add(ttl_assignments())
                                    .is_ok_and(|expiry| now <= expiry)
                        })
                })
                .collect::<Vec<_>>())
        })
        .await?;
    let mut out = Vec::new();
    for (id, cid) in ids {
        out.push(detail(session, globals, cid, id).await?);
    }
    Ok(out)
}
