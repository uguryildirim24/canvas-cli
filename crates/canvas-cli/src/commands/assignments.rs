//! `canvas assignments` (class C).
use super::emit::{base_envelope, emit, session_error, sync_error};
use super::{Globals, assignment_read as read};
use crate::cli::AssignmentBucket as CliBucket;
use crate::output::{SCHEMA_ASSIGNMENTS, now_timestamp};
use crate::session::ttl_assignments;
use canvas_core::todo::{AssignmentBucket, in_bucket};
use serde_json::json;
use std::io::{self, Write};
use std::process::ExitCode;

pub async fn run(
    globals: &Globals,
    course: String,
    bucket: Option<CliBucket>,
    search: Option<String>,
) -> ExitCode {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(globals.json, e, globals.profile.clone()),
    };
    let (course, mut freshness, _) =
        match super::course::resolve_with_refresh(globals, &session, &course).await {
            Ok(v) => v,
            Err(e) => return e,
        };
    let outcome = match read::assignments(&session, globals, course.id).await {
        Ok(o) => o,
        Err(e) => return sync_error(globals, &session, &e),
    };
    freshness.push(super::course_load::outcome_freshness(&outcome));
    let eligibility =
        match read::refresh_stale_eligibility(&session, globals, vec![course.id], None).await {
            Ok(o) => o,
            Err(e) => return sync_error(globals, &session, &e),
        };
    freshness.extend(
        eligibility
            .iter()
            .map(super::course_load::outcome_freshness),
    );
    let id = course.id;
    let now = now_timestamp();
    let bucket = bucket.map_or(AssignmentBucket::Open, map_bucket);
    let items=session.open.store.call(move|conns|{
        let mut stmt=conns.cache.prepare("SELECT entity_id FROM membership WHERE dataset='assignments' AND scope=?1 AND entity_kind='assignment'")?;
        let ids=stmt.query_map([format!("course:{id}")],|r|r.get::<_,String>(0))?.collect::<Result<Vec<_>,_>>()?;
        let mut items=Vec::new(); let needle=search.map(|s|s.to_lowercase());
        for id in ids {
            let id=id.parse().map_err(|_|canvas_core::store::DbError::Message("invalid assignment membership".into()))?;
            if let Some(item)=canvas_core::todo::load_assignment_item(conns,id,now,ttl_assignments())? {
                if in_bucket(&item,bucket,now) && needle.as_ref().is_none_or(|s|item.title.to_lowercase().contains(s)) {items.push(item);}
            }
        }
        items.sort_by_key(|i|(i.due_at.is_none(),i.due_at,i.id));
        Ok(items)
    }).await;
    let items = match items {
        Ok(i) => i,
        Err(e) => return sync_error(globals, &session, &e.into()),
    };
    let zone = read::zone(&session);
    let result = json!({"course_id":id.to_string(),"bucket":bucket.as_str(),"assignments":items.iter().map(|i|read::assignment_json(i,&zone)).collect::<Vec<_>>()});
    let mut envelope = base_envelope(SCHEMA_ASSIGNMENTS, &session, result);
    envelope.freshness = freshness;
    envelope.warnings.extend(outcome.error);
    envelope
        .warnings
        .extend(eligibility.into_iter().filter_map(|o| o.error));
    emit(globals.json, &envelope, || {
        writeln!(
            io::stdout(),
            "{}",
            read::human_table(&items, &zone, read::use_color(globals))
        )
    })
}

fn map_bucket(b: CliBucket) -> AssignmentBucket {
    match b {
        CliBucket::Open => AssignmentBucket::Open,
        CliBucket::Upcoming => AssignmentBucket::Upcoming,
        CliBucket::Overdue => AssignmentBucket::Overdue,
        CliBucket::Past => AssignmentBucket::Past,
        CliBucket::Undated => AssignmentBucket::Undated,
        CliBucket::Unsubmitted => AssignmentBucket::Unsubmitted,
        CliBucket::Ungraded => AssignmentBucket::Ungraded,
        CliBucket::Future => AssignmentBucket::Future,
        CliBucket::All => AssignmentBucket::All,
    }
}
