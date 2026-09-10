//! Merged planner + missing todo view (§12.1).

#![allow(clippy::doc_markdown)]

mod buckets;
mod merge;
mod rows;
pub use rows::{assignment_observations, load_assignment_item, load_assignment_row};

#[cfg(test)]
mod tests;

pub use buckets::{AssignmentBucket, in_bucket};
pub use merge::{
    AssignmentDetails, TodoAvailability, TodoCounts, TodoFilters, TodoItem, TodoKind, TodoStatus,
    TodoWindow, build_todo, build_todo_in_zone, map_plannable_kind,
};

use jiff::civil::Date;

use crate::store::{DbError, StoreConns, pending_for_assignment};

/// Load planner membership rows for a window scope.
pub fn load_planner_rows(
    conns: &StoreConns,
    scope: &str,
) -> Result<Vec<merge::PlannerSourceRow>, DbError> {
    let mut stmt = conns.cache.prepare(
        "SELECT p.id, p.plannable_id, p.plannable_type, p.course_id, p.title, p.data_json
         FROM membership m
         INNER JOIN planner_items p ON p.id = m.entity_id
         WHERE m.dataset = 'planner' AND m.scope = ?1 AND m.entity_kind = 'planner_item'
         ORDER BY m.position ASC",
    )?;
    let rows = stmt
        .query_map([scope], |r| {
            Ok(merge::PlannerSourceRow {
                assignment: None,
                id: r.get(0)?,
                plannable_id: r.get(1)?,
                plannable_type: r.get(2)?,
                course_id: r.get(3)?,
                title: r.get(4)?,
                data_json: r.get::<_, String>(5).unwrap_or_else(|_| "{}".into()),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut rows = rows;
    for row in &mut rows {
        let data: serde_json::Value = serde_json::from_str(&row.data_json)
            .map_err(|_| DbError::Message("invalid planner data".into()))?;
        let aid = match row.plannable_type.as_deref() {
            Some("assignment") => row.plannable_id,
            Some("quiz" | "discussion_topic") => data.get("assignment_id").and_then(|v| {
                v.as_i64()
                    .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            }),
            _ => None,
        };
        if let Some(id) = aid {
            row.assignment = load_assignment_row(conns, id)?;
        }
    }
    Ok(rows)
}

/// Load missing membership assignment ids.
pub fn load_missing_rows(conns: &StoreConns) -> Result<Vec<merge::MissingSourceRow>, DbError> {
    let mut stmt = conns.cache.prepare("SELECT entity_id FROM membership WHERE dataset='missing' AND scope='all' AND entity_kind='assignment' ORDER BY position")?;
    let ids = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    ids.into_iter()
        .map(|id| {
            let id = id
                .parse()
                .map_err(|_| DbError::Message("invalid assignment membership".into()))?;
            let mut row = load_assignment_row(conns, id)?
                .ok_or_else(|| DbError::Message("missing assignment entity".into()))?;
            row.missing = Some(1);
            Ok(row)
        })
        .collect()
}

/// Course code lookup for display.
pub fn course_code(conns: &StoreConns, course_id: i64) -> Result<Option<String>, DbError> {
    Ok(conns
        .cache
        .query_row(
            "SELECT course_code FROM courses WHERE id = ?1",
            [course_id],
            |r| r.get(0),
        )
        .optional()?)
}

use rusqlite::OptionalExtension;

/// Read pending flag for an assignment id.
pub fn assignment_pending(conns: &StoreConns, assignment_id: i64) -> Result<bool, DbError> {
    pending_for_assignment(&conns.state, assignment_id)
}

/// Civil "today" in UTC from a timestamp string or wall clock.
#[must_use]
pub fn today_utc(now: jiff::Timestamp) -> Date {
    now.to_zoned(jiff::tz::TimeZone::UTC).date()
}
