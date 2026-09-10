//! Merged planner + missing todo view (§12.1).

#![allow(clippy::doc_markdown)]

mod buckets;
mod merge;

#[cfg(test)]
mod tests;

pub use buckets::{AssignmentBucket, in_bucket};
pub use merge::{
    TodoAvailability, TodoCounts, TodoFilters, TodoItem, TodoKind, TodoStatus, TodoWindow,
    build_todo, map_plannable_kind,
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
                id: r.get(0)?,
                plannable_id: r.get(1)?,
                plannable_type: r.get(2)?,
                course_id: r.get(3)?,
                title: r.get(4)?,
                data_json: r.get::<_, String>(5).unwrap_or_else(|_| "{}".into()),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Load missing membership assignment ids.
pub fn load_missing_rows(conns: &StoreConns) -> Result<Vec<merge::MissingSourceRow>, DbError> {
    let mut stmt = conns.cache.prepare(
        "SELECT a.id, a.course_id, a.name, a.due_at, a.points_possible, a.html_url,
                a.submitted, a.graded, a.score, a.late, a.missing, a.excused,
                a.can_submit, a.unlock_at, a.lock_at, a.data_json
         FROM membership m
         INNER JOIN assignments a ON a.id = CAST(m.entity_id AS INTEGER)
         WHERE m.dataset = 'missing' AND m.scope = 'all' AND m.entity_kind = 'assignment'
         ORDER BY m.position ASC",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(merge::MissingSourceRow {
                id: r.get(0)?,
                course_id: r.get(1)?,
                name: r.get(2)?,
                due_at: r.get(3)?,
                points_possible: r.get(4)?,
                html_url: r.get(5)?,
                submitted: r.get(6)?,
                graded: r.get(7)?,
                score: r.get(8)?,
                late: r.get(9)?,
                missing: r.get(10)?,
                excused: r.get(11)?,
                can_submit: r.get(12)?,
                unlock_at: r.get(13)?,
                lock_at: r.get(14)?,
                data_json: r.get::<_, String>(15).unwrap_or_else(|_| "{}".into()),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
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
