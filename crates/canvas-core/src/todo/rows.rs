//! Canonical assignment values shared by todo, list and detail reads.
use super::merge::{MissingSourceRow, TodoItem, TodoStatus, missing_to_item};
use crate::store::{DbError, StoreConns};
use jiff::{Span, Timestamp};
use rusqlite::OptionalExtension;
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub fn load_assignment_row(
    conns: &StoreConns,
    id: i64,
) -> Result<Option<MissingSourceRow>, DbError> {
    Ok(conns.cache.query_row(
        "SELECT id,course_id,name,due_at,points_possible,html_url,submitted,graded,score,late,missing,excused,can_submit,unlock_at,lock_at,data_json,submission_types,allowed_extensions,allowed_attempts,description,rubric_json,workflow_state,attempt FROM assignments WHERE id=?1", [id], |r| {
            let raw: String = r.get(15)?;
            let mut data: Value = serde_json::from_str(&raw).map_err(|e| rusqlite::Error::FromSqlConversionFailure(15,rusqlite::types::Type::Text,Box::new(e)))?;
            for (name, col) in [("submission_types",16),("allowed_extensions",17),("rubric",20)] {
                let raw: Option<String> = r.get(col)?;
                data[name] = raw.and_then(|v| serde_json::from_str(&v).ok()).unwrap_or(Value::Null);
            }
            data["allowed_attempts"] = json!(r.get::<_, Option<i64>>(18)?);
            data["description"] = json!(r.get::<_, Option<String>>(19)?);
            data["workflow_state"] = json!(r.get::<_, Option<String>>(21)?);
            data["attempt"] = json!(r.get::<_, Option<i64>>(22)?);
            Ok(MissingSourceRow {
                id:r.get(0)?, course_id:r.get(1)?, name:r.get(2)?, due_at:r.get(3)?, points_possible:r.get(4)?, html_url:r.get(5)?, submitted:r.get(6)?, graded:r.get(7)?, score:r.get(8)?, late:r.get(9)?, missing:r.get(10)?, excused:r.get(11)?, can_submit:r.get(12)?, unlock_at:r.get(13)?, lock_at:r.get(14)?, data_json:data.to_string(),
            })
        }).optional()?)
}

pub fn assignment_observations(conns: &StoreConns) -> Result<BTreeMap<i64, Timestamp>, DbError> {
    let mut stmt = conns.cache.prepare("SELECT entity_key,observed_at FROM field_obs WHERE entity_kind='assignment' AND field='can_submit'")?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(id, at)| {
            Ok((
                id.parse()
                    .map_err(|_| DbError::Message("invalid assignment key".into()))?,
                at.parse()
                    .map_err(|_| DbError::Message("invalid observation time".into()))?,
            ))
        })
        .collect()
}

pub fn load_assignment_item(
    conns: &StoreConns,
    id: i64,
    now: Timestamp,
    ttl: Span,
) -> Result<Option<TodoItem>, DbError> {
    let Some(row) = load_assignment_row(conns, id)? else {
        return Ok(None);
    };
    let pending = super::assignment_pending(conns, id)?;
    let mut codes = BTreeMap::new();
    if let Some(cid) = row.course_id
        && let Some(code) = super::course_code(conns, cid)?
    {
        codes.insert(cid, code);
    }
    let mut item = missing_to_item(
        &row,
        &codes,
        &BTreeMap::from([(id, pending)]),
        now,
        ttl,
        &assignment_observations(conns)?,
    );
    if pending {
        item.status = TodoStatus {
            pending: true,
            missing: item.status.missing,
            ..TodoStatus::default()
        };
    }
    Ok(Some(item))
}
