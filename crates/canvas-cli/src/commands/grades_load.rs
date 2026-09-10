//! Period-aware grade reads over `enrollment_grades`, `course_totals`,
//! `grading_periods` and `assignment_groups` (SPEC §12.4).

use std::collections::HashMap;

use canvas_core::store::{DbError, StoreConns};
use canvas_core::sync::PeriodKey;
use jiff::Timestamp;
use rusqlite::OptionalExtension;
use serde_json::Value;

use crate::output::{
    AssignmentGroupJson, GradingPeriodJson, GroupAssignmentJson, GroupRulesJson, GroupSubtotalJson,
    SubmissionStatusJson,
};

/// The grading period a run was asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeriodSelection {
    /// Whole course (`--period all`).
    All,
    /// The course's current period (`--period current`).
    Current,
    /// One explicit period id (`--period ID`).
    Id(i64),
}

impl PeriodSelection {
    /// Appendix D `period.mode` and the top-level `period_mode`.
    #[must_use]
    pub fn mode(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Current => "current",
            Self::Id(_) => "id",
        }
    }

    /// `course_totals.mode` for the default modes; an explicit id has no row.
    #[must_use]
    pub fn totals_mode(self) -> Option<&'static str> {
        match self {
            Self::All => Some("all"),
            Self::Current => Some("current"),
            Self::Id(_) => None,
        }
    }

    /// Scope qualifier for `enrollment_grades` and `assignment_groups`.
    #[must_use]
    pub fn period_key(self) -> PeriodKey {
        match self {
            Self::Id(id) => PeriodKey::Id(id),
            Self::All | Self::Current => PeriodKey::None,
        }
    }
}

/// Parse `--period current|all|ID`. `Err` carries the offending input.
pub fn parse_period(raw: &str) -> Result<PeriodSelection, String> {
    match raw {
        "all" => Ok(PeriodSelection::All),
        "current" => Ok(PeriodSelection::Current),
        other => other
            .parse::<i64>()
            .map(PeriodSelection::Id)
            .map_err(|_| other.to_owned()),
    }
}

/// Canvas-reported totals for one course under one period selection.
#[derive(Debug, Clone, Default)]
pub struct PeriodTotals {
    pub current_score: Option<f64>,
    pub current_grade: Option<String>,
    pub final_score: Option<f64>,
    pub final_grade: Option<String>,
    pub period_id: Option<String>,
    pub period_title: Option<String>,
    /// `false` when Canvas has no row at all for this course and period.
    pub covered: bool,
}

/// Read `course_totals` for one course in one of the two default modes.
///
/// A `current`-mode row whose scores predate the stored period transition is
/// not applicable to the period it is now labelled with, so those scores are
/// dropped rather than relabelled (SPEC §12.4).
pub fn course_totals_for_mode(
    conns: &StoreConns,
    course_id: i64,
    mode: &str,
) -> Result<PeriodTotals, DbError> {
    /// `(current_score, current_grade, final_score, final_grade, data_json)`.
    type TotalsRow = (
        Option<f64>,
        Option<String>,
        Option<f64>,
        Option<String>,
        String,
    );
    let row: Option<TotalsRow> = conns
        .cache
        .query_row(
            "SELECT current_score, current_grade, final_score, final_grade, data_json
             FROM course_totals WHERE course_id = ?1 AND mode = ?2",
            rusqlite::params![course_id, mode],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?;
    let Some((current_score, current_grade, final_score, final_grade, data_raw)) = row else {
        return Ok(PeriodTotals::default());
    };
    let data: Value = serde_json::from_str(&data_raw).unwrap_or(Value::Null);
    let mut totals = PeriodTotals {
        current_score,
        current_grade,
        final_score,
        final_grade,
        period_id: json_id(data.get("period_id")),
        period_title: data
            .get("period_title")
            .and_then(Value::as_str)
            .map(str::to_owned),
        covered: true,
    };
    if mode == "current" {
        drop_scores_from_an_earlier_period(conns, course_id, &data, &mut totals)?;
    }
    Ok(totals)
}

/// Scores observed before the period changed belong to the previous period.
fn drop_scores_from_an_earlier_period(
    conns: &StoreConns,
    course_id: i64,
    data: &Value,
    totals: &mut PeriodTotals,
) -> Result<(), DbError> {
    let Some(changed) = data.get("period_changed_at").and_then(Value::as_str) else {
        return Ok(());
    };
    let changed: Timestamp = changed
        .parse()
        .map_err(|_| DbError::Message("invalid period transition timestamp".into()))?;
    let key = format!("{course_id}|current");
    for name in [
        "current_score",
        "final_score",
        "current_grade",
        "final_grade",
    ] {
        let observed: Option<String> = conns
            .cache
            .query_row(
                "SELECT observed_at FROM field_obs
                 WHERE entity_kind = 'course_totals' AND entity_key = ?1 AND field = ?2",
                rusqlite::params![key, name],
                |r| r.get(0),
            )
            .optional()?;
        let belongs = observed
            .as_deref()
            .and_then(|v| v.parse::<Timestamp>().ok())
            .is_some_and(|at| at >= changed);
        if !belongs {
            match name {
                "current_score" => totals.current_score = None,
                "final_score" => totals.final_score = None,
                "current_grade" => totals.current_grade = None,
                _ => totals.final_grade = None,
            }
        }
    }
    Ok(())
}

/// Read `enrollment_grades` for one period, de-duplicated per course.
///
/// A student can hold several enrollments in one course (a section change, or a
/// re-enrolment). Canvas reports the same totals on each, so the course keeps
/// one row: the enrollment that supplied any value at all, then the lowest id
/// for a deterministic result.
pub fn enrollment_grades_by_course(
    conns: &StoreConns,
    period: PeriodKey,
) -> Result<HashMap<i64, PeriodTotals>, DbError> {
    let period_value = period.period_value();
    let mut stmt = conns.cache.prepare(
        "SELECT enrollment_id, course_id, current_score, current_grade, final_score, final_grade
         FROM enrollment_grades WHERE period = ?1 ORDER BY enrollment_id",
    )?;
    let rows = stmt
        .query_map([&period_value], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<i64>>(1)?,
                r.get::<_, Option<f64>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<f64>>(4)?,
                r.get::<_, Option<String>>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut out: HashMap<i64, PeriodTotals> = HashMap::new();
    for (_, course_id, current_score, current_grade, final_score, final_grade) in rows {
        let Some(course_id) = course_id else { continue };
        let candidate = PeriodTotals {
            current_score,
            current_grade,
            final_score,
            final_grade,
            period_id: match period {
                PeriodKey::Id(id) => Some(id.to_string()),
                PeriodKey::None => None,
            },
            period_title: None,
            covered: true,
        };
        match out.get(&course_id) {
            // Keep the first enrollment that carried a value.
            Some(existing) if has_values(existing) || !has_values(&candidate) => {}
            _ => {
                out.insert(course_id, candidate);
            }
        }
    }
    Ok(out)
}

fn has_values(t: &PeriodTotals) -> bool {
    t.current_score.is_some()
        || t.current_grade.is_some()
        || t.final_score.is_some()
        || t.final_grade.is_some()
}

/// Read the grading periods of one course, newest boundary last.
pub fn load_grading_periods(
    conns: &StoreConns,
    course_id: i64,
    now: Timestamp,
) -> Result<Vec<GradingPeriodJson>, DbError> {
    let mut stmt = conns.cache.prepare(
        "SELECT p.id, p.title, p.start_date, p.end_date
         FROM membership m
         JOIN grading_periods p ON p.id = CAST(m.entity_id AS INTEGER)
         WHERE m.dataset = 'grading_periods' AND m.scope = ?1
           AND m.entity_kind = 'grading_period'
         ORDER BY m.position, p.id",
    )?;
    let scope = format!("course:{course_id}");
    let rows = stmt
        .query_map([&scope], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows
        .into_iter()
        .map(|(id, title, start_date, end_date)| {
            let is_current = period_contains(start_date.as_deref(), end_date.as_deref(), now);
            GradingPeriodJson {
                id: id.to_string(),
                title: title.unwrap_or_default(),
                start_date,
                end_date,
                is_current,
            }
        })
        .collect())
}

/// An open-ended boundary does not exclude `now`.
fn period_contains(start: Option<&str>, end: Option<&str>, now: Timestamp) -> bool {
    let after_start = start
        .and_then(|v| v.parse::<Timestamp>().ok())
        .is_none_or(|s| now >= s);
    let before_end = end
        .and_then(|v| v.parse::<Timestamp>().ok())
        .is_none_or(|e| now <= e);
    after_start && before_end
}

/// Read the assignment groups of one course for one period.
pub fn load_assignment_groups(
    conns: &StoreConns,
    course_id: i64,
    period: PeriodKey,
    zone: &jiff::tz::TimeZone,
) -> Result<Vec<AssignmentGroupJson>, DbError> {
    let scope = format!("course:{course_id}:{}", period.scope_key());
    let period_value = period.period_value();
    let mut stmt = conns.cache.prepare(
        "SELECT g.id, g.name, g.position, g.group_weight, g.rules_json, g.data_json
         FROM membership m
         JOIN assignment_groups g ON g.id = CAST(m.entity_id AS INTEGER)
         WHERE m.dataset = 'assignment_groups' AND m.scope = ?1
           AND m.entity_kind = 'assignment_group'
         ORDER BY m.position, g.id",
    )?;
    let rows = stmt
        .query_map([&scope], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<i64>>(2)?,
                r.get::<_, Option<f64>>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, String>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut groups = Vec::new();
    for (id, name, position, weight, rules_raw, data_raw) in rows {
        let data: Value = serde_json::from_str(&data_raw).unwrap_or(Value::Null);
        let entry = data
            .get("assignments_by_period")
            .and_then(|m| m.get(&period_value));
        let assignments = entry
            .and_then(|e| e.get("assignments"))
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .map(|a| group_assignment(conns, a, zone))
                    .collect::<Result<Vec<_>, DbError>>()
            })
            .transpose()?
            .unwrap_or_default();
        groups.push(AssignmentGroupJson {
            id: id.to_string(),
            name: name.unwrap_or_default(),
            position: position.unwrap_or(0),
            weight,
            rules: parse_rules(rules_raw.as_deref()),
            subtotal: subtotal(entry),
            assignments: sort_assignments(assignments),
        });
    }
    groups.sort_by(|a, b| a.position.cmp(&b.position).then_with(|| a.id.cmp(&b.id)));
    Ok(groups)
}

/// Appendix D sort: `due_at` ascending, undated last, then `id`.
fn sort_assignments(
    mut items: Vec<(GroupAssignmentJson, Option<String>)>,
) -> Vec<GroupAssignmentJson> {
    items.sort_by(|a, b| match (&a.1, &b.1) {
        (Some(x), Some(y)) => x.cmp(y).then_with(|| a.0.id.cmp(&b.0.id)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.0.id.cmp(&b.0.id),
    });
    items.into_iter().map(|(a, _)| a).collect()
}

/// Only report a subtotal Canvas actually supplied (SPEC §12.4).
fn subtotal(entry: Option<&Value>) -> Option<GroupSubtotalJson> {
    let sub = entry?.get("subtotal")?;
    if sub.is_null() {
        return None;
    }
    Some(GroupSubtotalJson {
        score: sub.get("score").and_then(Value::as_f64),
        possible: sub.get("possible").and_then(Value::as_f64),
    })
}

fn parse_rules(raw: Option<&str>) -> GroupRulesJson {
    let value: Value = raw
        .and_then(|r| serde_json::from_str(r).ok())
        .unwrap_or(Value::Null);
    GroupRulesJson {
        drop_lowest: value
            .get("drop_lowest")
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok()),
        drop_highest: value
            .get("drop_highest")
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok()),
        never_drop: value
            .get("never_drop")
            .and_then(Value::as_array)
            .map(|ids| {
                ids.iter()
                    .filter_map(|v| {
                        v.as_str()
                            .map(str::to_owned)
                            .or_else(|| v.as_i64().map(|n| n.to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// Build one assignment row, pairing it with its `due_at` for sorting.
fn group_assignment(
    conns: &StoreConns,
    raw: &Value,
    zone: &jiff::tz::TimeZone,
) -> Result<(GroupAssignmentJson, Option<String>), DbError> {
    let id = raw
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let pending = id
        .parse::<i64>()
        .ok()
        .map(|n| canvas_core::store::pending_for_assignment(&conns.state, n))
        .transpose()?
        .unwrap_or(false);
    let status = SubmissionStatusJson {
        // Absent means Canvas said nothing; an explicit null means not submitted.
        submitted: raw.get("submitted_at").map(|v| !v.is_null()),
        graded: raw
            .get("workflow_state")
            .and_then(Value::as_str)
            .map(|s| s == "graded"),
        score: raw.get("score").and_then(Value::as_f64),
        grade: raw.get("grade").and_then(Value::as_str).map(str::to_owned),
        late: raw.get("late").and_then(Value::as_bool),
        missing: raw.get("missing").and_then(Value::as_bool).unwrap_or(false),
        excused: raw.get("excused").and_then(Value::as_bool),
        workflow_state: raw
            .get("workflow_state")
            .and_then(Value::as_str)
            .map(str::to_owned),
        submitted_at: raw
            .get("submitted_at")
            .and_then(Value::as_str)
            .map(str::to_owned),
        submitted_at_local: raw
            .get("submitted_at")
            .and_then(Value::as_str)
            .and_then(|v| v.parse::<Timestamp>().ok())
            .map(|t| {
                let local = t.to_zoned(zone.clone());
                format!("{}{}", local.datetime(), local.strftime("%:z"))
            }),
        attempt: raw.get("attempt").and_then(Value::as_i64),
        posted_at: raw
            .get("posted_at")
            .and_then(Value::as_str)
            .map(str::to_owned),
        pending,
    };
    let due_at = raw.get("due_at").and_then(Value::as_str).map(str::to_owned);
    Ok((
        GroupAssignmentJson {
            id,
            name: raw
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            points_possible: raw.get("points_possible").and_then(Value::as_f64),
            omit_from_final_grade: raw
                .get("omit_from_final_grade")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            status,
        },
        due_at,
    ))
}

fn json_id(value: Option<&Value>) -> Option<String> {
    value.and_then(|v| {
        v.as_str()
            .map(str::to_owned)
            .or_else(|| v.as_i64().map(|n| n.to_string()))
    })
}
