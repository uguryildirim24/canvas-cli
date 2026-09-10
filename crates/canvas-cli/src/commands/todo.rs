//! `canvas todo` (class C).
use super::emit::{base_envelope, emit, emit_error, session_error, sync_error};
use super::{Globals, assignment_read as read};
use crate::output::{SCHEMA_TODO, now_timestamp};
use crate::session::{ttl_assignments, ttl_courses};
use canvas_core::sync::{self, CoursesScope, PlannerWindow};
use canvas_core::todo::{self, TodoFilters, TodoItem, TodoWindow};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};
use std::process::ExitCode;

#[allow(clippy::too_many_lines)]
pub async fn run(
    globals: &Globals,
    days: Option<u32>,
    all: bool,
    missing: bool,
    course: Option<String>,
) -> ExitCode {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(globals.json, e, globals.profile.clone()),
    };
    let days = days.unwrap_or(14);
    if days == 0
        || now_timestamp()
            .to_zoned(jiff::tz::TimeZone::UTC)
            .date()
            .checked_add(jiff::Span::new().days(i64::from(days)))
            .is_err()
    {
        return emit_error(
            globals.json,
            "usage",
            "days must be positive and fit the supported date range",
            2,
            session.profile.clone(),
            Some(session.identity_ref()),
        );
    }
    let now = now_timestamp();
    let zone = read::zone(&session);
    let today = now.to_zoned(zone.clone()).date();
    let window = PlannerWindow::todo_default(today, days);
    let todo_window = TodoWindow {
        start: window.start,
        end: window.end,
        days,
    };
    let courses = async {
        if let Some(out) = read::cached(
            &session,
            sync::CoursesDataset::new(CoursesScope::Active, ttl_courses()),
            None,
            globals,
        )
        .await?
        {
            return Ok(out);
        }
        sync::refresh_courses(
            session
                .client
                .as_ref()
                .ok_or(canvas_api::Error::Unauthorized)?,
            &session.open.store,
            CoursesScope::Active,
            ttl_courses(),
            now,
            globals.fresh,
            false,
        )
        .await
    }
    .await;
    let mut outcomes = Vec::new();
    match courses {
        Ok(o) => outcomes.push(o),
        Err(e) => return sync_error(globals, &session, &e),
    }
    let planner_scope = match read::planner(&session, globals, window.clone()).await {
        Ok(o) => {
            let scope = o.freshness.scope.clone();
            outcomes.push(o);
            scope
        }
        Err(e) => return sync_error(globals, &session, &e),
    };
    match read::missing(&session, globals).await {
        Ok(o) => outcomes.push(o),
        Err(e) => return sync_error(globals, &session, &e),
    }
    let mut resolver_freshness = Vec::new();
    let course_id = if let Some(course) = course {
        match super::course::resolve_with_refresh(globals, &session, &course).await {
            Ok((c, f, _)) => {
                resolver_freshness = f;
                Some(c.id)
            }
            Err(e) => return e,
        }
    } else {
        None
    };
    let active=match session.open.store.call(|conns|{
        let mut stmt=conns.cache.prepare("SELECT CAST(entity_id AS INTEGER) FROM membership WHERE dataset='courses' AND scope='active' AND entity_kind='course'")?;
        Ok(stmt.query_map([],|r|r.get::<_,i64>(0))?.collect::<Result<Vec<_>,_>>()?)
    }).await{Ok(ids)=>ids,Err(e)=>return sync_error(globals,&session,&e.into())};
    if all {
        for id in &active {
            match read::assignments(&session, globals, *id).await {
                Ok(o) => outcomes.push(o),
                Err(e) => return sync_error(globals, &session, &e),
            }
        }
    }
    match read::refresh_stale_eligibility(
        &session,
        globals,
        if all { active.clone() } else { vec![] },
        Some(planner_scope.clone()),
    )
    .await
    {
        Ok(o) => outcomes.extend(o),
        Err(e) => return sync_error(globals, &session, &e),
    }
    let view=session.open.store.call({let window=window.clone();let todo_window=todo_window.clone();let zone=zone.clone();move|conns|{
        let mut planner=todo::load_planner_rows(conns,&planner_scope)?;
        planner.retain(|row|{
            let data:Value=serde_json::from_str(&row.data_json).unwrap_or(Value::Null);
            let at=data.get("plannable_date").or_else(||data.get("due_at")).and_then(Value::as_str).and_then(|s|s.parse::<jiff::Timestamp>().ok());
            at.is_none_or(|t|t>=window.start_timestamp() && t<window.end_timestamp())
        });
        let mut rows=todo::load_missing_rows(conns)?;
        let mut ids:BTreeSet<i64>=rows.iter().map(|r|r.id).collect();
        if all {
            for cid in active {
                let mut stmt=conns.cache.prepare("SELECT CAST(entity_id AS INTEGER) FROM membership WHERE dataset='assignments' AND scope=?1 AND entity_kind='assignment'")?;
                let members=stmt.query_map([format!("course:{cid}")],|r|r.get::<_,i64>(0))?.collect::<Result<Vec<_>,_>>()?;
                for id in members {if ids.insert(id){if let Some(row)=todo::load_assignment_row(conns,id)?{rows.push(row);}}}
            }
        }
        let mut codes=BTreeMap::new();let mut pending=BTreeMap::new();
        for row in &planner {
            if let Some(cid)=row.course_id{if let Some(code)=todo::course_code(conns,cid)?{codes.insert(cid,code);}}
            if let Some(a)=&row.assignment{ids.insert(a.id);}
            let data:Value=serde_json::from_str(&row.data_json).unwrap_or(Value::Null);
            for name in ["assignment_id","parent_assignment_id"] {
                if let Some(id)=data.get(name).and_then(|v|v.as_i64().or_else(||v.as_str().and_then(|s|s.parse().ok()))){ids.insert(id);}
            }
        }
        for row in &rows{if let Some(cid)=row.course_id{if let Some(code)=todo::course_code(conns,cid)?{codes.insert(cid,code);}}}
        for id in ids{pending.insert(id,todo::assignment_pending(conns,id)?);}
        Ok(todo::build_todo_in_zone(&planner,&rows,&codes,&pending,now,today,&todo_window,&TodoFilters{all,missing_only:missing,course_id,..TodoFilters::default()},ttl_assignments(),&todo::assignment_observations(conns)?,&zone))
    }}).await;
    let (items, counts) = match view {
        Ok(v) => v,
        Err(e) => return sync_error(globals, &session, &e.into()),
    };
    let result = json!({"window":{"start":todo_window.start.to_string(),"end":todo_window.end.to_string(),"days":days},"items":items.iter().map(|i|item_json(i,&zone)).collect::<Vec<_>>(),"counts":{"missing":counts.missing,"due_today":counts.due_today,"due_week":counts.due_week,"hidden":counts.hidden}});
    let mut envelope = base_envelope(SCHEMA_TODO, &session, result);
    envelope.freshness = outcomes
        .iter()
        .map(super::course_load::outcome_freshness)
        .collect();
    for row in resolver_freshness {
        if !envelope
            .freshness
            .iter()
            .any(|f| f.dataset == row.dataset && f.scope == row.scope)
        {
            envelope.freshness.push(row);
        }
    }
    envelope
        .warnings
        .extend(outcomes.into_iter().filter_map(|o| o.error));
    emit(globals.json, &envelope, || {
        writeln!(
            io::stdout(),
            "missing={} due_today={} due_week={} hidden={}",
            counts.missing,
            counts.due_today,
            counts.due_week,
            counts.hidden
        )?;
        writeln!(
            io::stdout(),
            "{}",
            read::human_todo(&items, &zone, read::use_color(globals))
        )
    })
}
fn item_json(item: &TodoItem, zone: &jiff::tz::TimeZone) -> Value {
    json!({"key":item.key,"kind":item.kind.as_str(),"raw_type":item.raw_type,"id":item.id.to_string(),"assignment_id":item.assignment_id.map(|id|id.to_string()),"parent_assignment_id":item.parent_assignment_id.map(|id|id.to_string()),"course_id":item.course_id.map(|id|id.to_string()),"course_code":item.course_code,"title":item.title,"due_at":item.due_at.map(|t|t.to_string()),"due_at_local":read::local(item.due_at,zone),"scheduled_at":item.scheduled_at.map(|t|t.to_string()),"scheduled_at_local":read::local(item.scheduled_at,zone),"points_possible":item.points_possible,"status":read::status(item,zone),"availability":read::availability(item,zone),"marked_complete":item.marked_complete,"dismissed":item.dismissed,"html_url":item.html_url})
}
