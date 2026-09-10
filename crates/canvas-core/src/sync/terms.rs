//! `terms` dataset (usually written as a side effect of `courses`).

use jiff::{Span, Timestamp};
use rusqlite::{Transaction, params};

use crate::store::{
    Dataset, DbError, EntityIngest, FieldGroup, FieldWrite, IngestError, apply_field_writes,
};

use super::fields::{push_opt_str, push_opt_ts};

/// Default TTL mirrors `ttl_courses` (terms arrive with courses).
#[must_use]
pub fn default_ttl_terms() -> Span {
    Span::new().hours(6)
}

/// Terms membership dataset (`scope` typically `"all"`).
#[derive(Debug, Clone)]
pub struct TermsDataset {
    pub scope: String,
    pub ttl: Span,
}

impl TermsDataset {
    #[must_use]
    pub fn new(scope: impl Into<String>, ttl: Span) -> Self {
        Self {
            scope: scope.into(),
            ttl,
        }
    }
}

impl Dataset for TermsDataset {
    fn name(&self) -> &'static str {
        "terms"
    }

    fn scope_key(&self) -> &str {
        &self.scope
    }

    fn ttl(&self) -> Span {
        self.ttl
    }

    fn entity_kind(&self) -> &'static str {
        "term"
    }

    fn upsert_entity(
        &self,
        tx: &Transaction<'_>,
        entity: &EntityIngest,
        fetched_at: Timestamp,
    ) -> Result<(), IngestError> {
        upsert_term(tx, entity, fetched_at)
    }
}

/// Build an entity ingest row from an embedded Canvas term.
#[must_use]
pub fn term_to_entity(term: &canvas_api::models::Term) -> Option<EntityIngest> {
    let id = term.id?;
    let mut fields = Vec::new();
    push_opt_str(&mut fields, "name", FieldGroup::Core, term.name.as_deref());
    push_opt_ts(&mut fields, "start_at", FieldGroup::Core, term.start_at);
    push_opt_ts(&mut fields, "end_at", FieldGroup::Core, term.end_at);
    Some(EntityIngest {
        entity_key: id.to_string(),
        fields,
    })
}

/// Upsert one term row (shared by `TermsDataset` and courses side effects).
pub fn upsert_term(
    tx: &Transaction<'_>,
    entity: &EntityIngest,
    fetched_at: Timestamp,
) -> Result<(), IngestError> {
    let id: i64 = entity.entity_key.parse().map_err(|e| {
        DbError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(
            std::io::Error::new(std::io::ErrorKind::InvalidData, format!("bad term id: {e}")),
        )))
    })?;
    if entity.entity_key != id.to_string() {
        return Err(DbError::Message("entity key must be normalized".into()).into());
    }
    validate_term_fields(&entity.fields)?;
    tx.execute(
        "INSERT INTO terms (id) VALUES (?1) ON CONFLICT(id) DO NOTHING",
        params![id],
    )?;
    let applied = apply_field_writes(tx, "term", &entity.entity_key, fetched_at, &entity.fields)?;
    for field in &entity.fields {
        if !applied.fields.contains(&field.name) {
            continue;
        }
        match field.name {
            "name" | "start_at" | "end_at" => {
                tx.execute(
                    &format!("UPDATE terms SET {} = ?1 WHERE id = ?2", field.name),
                    params![field.value, id],
                )?;
            }
            _ => {}
        }
    }
    let ts = fetched_at.to_string();
    if applied.core {
        tx.execute(
            "UPDATE terms SET observed_at_core = ?1 WHERE id = ?2",
            params![ts, id],
        )?;
    }
    if applied.detail {
        tx.execute(
            "UPDATE terms SET observed_at_detail = ?1 WHERE id = ?2",
            params![ts, id],
        )?;
    }
    if applied.status {
        tx.execute(
            "UPDATE terms SET observed_at_status = ?1 WHERE id = ?2",
            params![ts, id],
        )?;
    }
    Ok(())
}

fn validate_term_fields(fields: &[FieldWrite]) -> Result<(), IngestError> {
    let allowed = ["name", "start_at", "end_at"];
    let mut seen = std::collections::HashSet::new();
    for field in fields {
        if !allowed.contains(&field.name) || !seen.insert(field.name) {
            return Err(DbError::Message("unsupported or duplicate term field".into()).into());
        }
    }
    Ok(())
}
