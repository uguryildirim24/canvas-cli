//! Dataset trait, field-group ingest, and `FakeEntity` for tests.

use std::collections::HashSet;

use jiff::Timestamp;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Deserializer};

use super::db::DbError;
use super::ops::{self, EpochAbort, FetchLogRow};

/// Field families used for TTL reporting and `FakeEntity` groups.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldGroup {
    Core,
    Detail,
    Status,
}

impl FieldGroup {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Detail => "detail",
            Self::Status => "status",
        }
    }
}

/// Three-state field supply from a source payload.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Supplied<T> {
    #[default]
    Absent,
    Null,
    Value(T),
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Supplied<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Option::<T>::deserialize(deserializer).map(|value| value.map_or(Self::Null, Self::Value))
    }
}

impl<T> Supplied<T> {
    #[must_use]
    pub fn is_supplied(&self) -> bool {
        !matches!(self, Self::Absent)
    }
}

/// One field a source wants to write (present in the payload, possibly null).
#[derive(Debug, Clone)]
pub struct FieldWrite {
    pub name: &'static str,
    pub group: FieldGroup,
    /// `None` means explicit JSON null; `Some` is the value to store.
    pub value: Option<String>,
}

/// One page (or object) handed to [`Dataset::ingest`].
#[derive(Debug, Clone)]
pub struct IngestPage {
    pub fetched_at: Timestamp,
    pub entities: Vec<EntityIngest>,
}

/// Entity row upsert input for a dataset page.
#[derive(Debug, Clone)]
pub struct EntityIngest {
    pub entity_key: String,
    pub fields: Vec<FieldWrite>,
}

/// Cache coverage lookup outcome.
#[derive(Debug, Clone)]
pub enum LookupResult {
    Hit(FetchLogRow),
    Stale(FetchLogRow),
    Miss,
}

/// Options for a dataset ingest commit.
#[derive(Debug, Clone)]
pub struct IngestOpts<'a> {
    pub epoch_seen: i64,
    pub complete: bool,
    pub stale: bool,
    /// Sanitized summary only; never a raw response body or credential-bearing error.
    pub error: Option<&'a str>,
    pub window: Option<(Timestamp, Timestamp)>,
    pub contexts: Option<&'a str>,
}

/// Dataset contract: scope, TTL, ingest with field-group write rules.
pub trait Dataset {
    /// Dataset name stored in `membership` / `fetch_log`.
    fn name(&self) -> &str;
    /// Scope key for this fetch.
    fn scope_key(&self) -> &str;
    /// Fully qualified durable epoch scope (§10), distinct from membership scope.
    fn epoch_scope(&self) -> String {
        format!("{}:{}", self.name(), self.scope_key())
    }
    /// Effective epoch for this dataset and any values derived during its refresh.
    fn current_epoch(&self, state: &Connection) -> Result<i64, DbError> {
        ops::read_scope_epoch(state, &self.epoch_scope())
    }
    /// Publish dependent coverage in the same cache transaction, after the epoch guard.
    fn finish_refresh(
        &self,
        _tx: &Transaction<'_>,
        _state: &Connection,
        _pages: &[IngestPage],
    ) -> Result<(), IngestError> {
        Ok(())
    }
    /// TTL for the hit predicate.
    fn ttl(&self) -> jiff::Span;
    /// Entity kind written by this dataset.
    fn entity_kind(&self) -> &str;

    /// Ingest downloaded pages into the cache under one write transaction.
    ///
    /// Records `epoch_seen` from before the refresh; aborts if the state epoch
    /// for this scope advanced since then.
    fn ingest(
        &self,
        pages: &[IngestPage],
        opts: &IngestOpts<'_>,
        conns: &mut super::db::StoreConns,
    ) -> Result<(), IngestError> {
        // Only folders/files 403/404 denials are successful coverage with an error.
        let recorded_denial = matches!(self.name(), "files" | "folders")
            && matches!(opts.error, Some("unavailable:403" | "unavailable:404"));
        if !opts.complete || opts.stale || (opts.error.is_some() && !recorded_denial) {
            return mark_refresh_failed(self, opts.error.unwrap_or("refresh incomplete"), conns);
        }
        let result = commit_refresh(self, pages, opts, conns);
        if result.is_err() && !matches!(result, Err(IngestError::Epoch(_))) {
            mark_refresh_failed(self, "refresh ingestion failed", conns)?;
        }
        result
    }

    /// Upsert one entity using the per-field write rule.
    fn upsert_entity(
        &self,
        tx: &Transaction<'_>,
        entity: &EntityIngest,
        fetched_at: Timestamp,
    ) -> Result<(), IngestError>;
}

fn mark_refresh_failed<D: Dataset + ?Sized>(
    dataset: &D,
    error: &str,
    conns: &mut super::db::StoreConns,
) -> Result<(), IngestError> {
    let tx = conns
        .cache
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    tx.execute(
        "UPDATE fetch_log SET stale = 1,
             error = CASE WHEN dataset IN ('files', 'folders')
                               AND error IN ('unavailable:403', 'unavailable:404')
                          THEN error ELSE ?1 END
         WHERE dataset = ?2 AND scope = ?3",
        params![error, dataset.name(), dataset.scope_key()],
    )?;
    tx.commit()?;
    Ok(())
}

fn commit_refresh<D: Dataset + ?Sized>(
    dataset: &D,
    pages: &[IngestPage],
    opts: &IngestOpts<'_>,
    conns: &mut super::db::StoreConns,
) -> Result<(), IngestError> {
    let super::db::StoreConns { cache, state, .. } = conns;
    let tx = cache.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    for page in pages {
        for entity in &page.entities {
            dataset.upsert_entity(&tx, entity, page.fetched_at)?;
        }
    }

    // Replace membership for the exact scope.
    tx.execute(
        "DELETE FROM membership WHERE dataset = ?1 AND scope = ?2",
        params![dataset.name(), dataset.scope_key()],
    )?;
    let mut pos = 0i64;
    let mut seen = HashSet::new();
    for page in pages {
        for entity in &page.entities {
            if !seen.insert(&entity.entity_key) {
                continue;
            }
            tx.execute(
                "INSERT INTO membership (dataset, scope, entity_kind, entity_id, position)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    dataset.name(),
                    dataset.scope_key(),
                    dataset.entity_kind(),
                    entity.entity_key,
                    pos
                ],
            )?;
            pos += 1;
        }
    }

    let fetched_at = pages.last().map_or_else(
        || Timestamp::now().to_string(),
        |p| p.fetched_at.to_string(),
    );
    let (window_start, window_end) = match opts.window {
        Some((s, e)) => (Some(s.to_string()), Some(e.to_string())),
        None => (None, None),
    };
    tx.execute(
        "INSERT INTO fetch_log (
                dataset, scope, fetched_at, complete, count, stale, error,
                epoch_seen, contexts, window_start, window_end
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(dataset, scope) DO UPDATE SET
                fetched_at = excluded.fetched_at,
                complete = excluded.complete,
                count = excluded.count,
                stale = excluded.stale,
                error = excluded.error,
                epoch_seen = excluded.epoch_seen,
                contexts = excluded.contexts,
                window_start = excluded.window_start,
                window_end = excluded.window_end",
        params![
            dataset.name(),
            dataset.scope_key(),
            fetched_at,
            i64::from(opts.complete),
            pos,
            i64::from(opts.stale),
            opts.error,
            opts.epoch_seen,
            opts.contexts,
            window_start,
            window_end,
        ],
    )?;

    // Serialize the final epoch check with journal transitions until cache commit.
    // Always acquire cache before state; no network work runs under either lock.
    let state_tx = state.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let scope = dataset.epoch_scope();
    let current = dataset.current_epoch(&state_tx)?;
    if current > opts.epoch_seen {
        return Err(IngestError::Epoch(EpochAbort {
            scope,
            epoch_seen: opts.epoch_seen,
            current,
        }));
    }
    dataset.finish_refresh(&tx, &state_tx, pages)?;
    tx.commit()?;
    state_tx.commit()?;
    Ok(())
}

/// Ingest failures including epoch abort.
#[derive(Debug, thiserror::Error)]
pub enum IngestError {
    #[error(transparent)]
    Db(#[from] DbError),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Epoch(#[from] EpochAbort),
}

/// Result of applying field writes: which fields and groups won.
#[derive(Debug, Default)]
pub struct AppliedWrites {
    pub fields: Vec<&'static str>,
    pub core: bool,
    pub detail: bool,
    pub status: bool,
}

/// Apply supplied fields to `field_obs` and return which fields were written.
pub fn apply_field_writes(
    tx: &Transaction<'_>,
    entity_kind: &str,
    entity_key: &str,
    fetched_at: Timestamp,
    fields: &[FieldWrite],
) -> Result<AppliedWrites, IngestError> {
    let mut out = AppliedWrites::default();
    let fetched = fetched_at.to_string();
    for field in fields {
        let prev: Option<String> = tx
            .query_row(
                "SELECT observed_at FROM field_obs
                 WHERE entity_kind = ?1 AND entity_key = ?2 AND field = ?3",
                params![entity_kind, entity_key, field.name],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(ref observed) = prev {
            // Newer fetched_at wins; equal does not overwrite.
            if fetched_at <= parse_observed(observed)? {
                continue;
            }
        }
        tx.execute(
            "INSERT INTO field_obs (entity_kind, entity_key, field, observed_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(entity_kind, entity_key, field) DO UPDATE SET
                observed_at = excluded.observed_at",
            params![entity_kind, entity_key, field.name, fetched],
        )?;
        out.fields.push(field.name);
        match field.group {
            FieldGroup::Core => out.core = true,
            FieldGroup::Detail => out.detail = true,
            FieldGroup::Status => out.status = true,
        }
    }
    Ok(out)
}

fn parse_observed(value: &str) -> Result<Timestamp, DbError> {
    value.parse().map_err(|e| {
        DbError::Sqlite(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(e),
        ))
    })
}

fn validate_fields(fields: &[FieldWrite], allowed: &[&str]) -> Result<(), IngestError> {
    let mut seen = HashSet::new();
    for field in fields {
        if !allowed.contains(&field.name) || !seen.insert(field.name) {
            return Err(DbError::Message("unsupported or duplicate entity field".into()).into());
        }
    }
    Ok(())
}

/// Read current `observed_at` for a field.
pub fn field_observed_at(
    conn: &Connection,
    entity_kind: &str,
    entity_key: &str,
    field: &str,
) -> Result<Option<String>, DbError> {
    Ok(conn
        .query_row(
            "SELECT observed_at FROM field_obs
             WHERE entity_kind = ?1 AND entity_key = ?2 AND field = ?3",
            params![entity_kind, entity_key, field],
            |r| r.get(0),
        )
        .optional()?)
}

/// Fake entity dataset used by store tests.
#[derive(Debug, Clone)]
pub struct FakeEntity {
    pub scope: String,
    pub ttl: jiff::Span,
}

impl FakeEntity {
    #[must_use]
    pub fn new(scope: impl Into<String>) -> Self {
        Self {
            scope: scope.into(),
            ttl: jiff::Span::new().minutes(10),
        }
    }
}

impl Dataset for FakeEntity {
    fn name(&self) -> &'static str {
        "fake"
    }

    fn scope_key(&self) -> &str {
        &self.scope
    }

    fn ttl(&self) -> jiff::Span {
        self.ttl
    }

    fn entity_kind(&self) -> &'static str {
        "fake_entity"
    }

    fn upsert_entity(
        &self,
        tx: &Transaction<'_>,
        entity: &EntityIngest,
        fetched_at: Timestamp,
    ) -> Result<(), IngestError> {
        let id: i64 = entity.entity_key.parse().map_err(|e| {
            DbError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(
                std::io::Error::new(std::io::ErrorKind::InvalidData, format!("bad id: {e}")),
            )))
        })?;

        if entity.entity_key != id.to_string() {
            return Err(DbError::Message("entity key must be normalized".into()).into());
        }
        validate_fields(
            &entity.fields,
            &["name", "due_at", "description", "score", "can_submit"],
        )?;
        // Ensure a row exists.
        tx.execute(
            "INSERT INTO fake_entities (id) VALUES (?1)
             ON CONFLICT(id) DO NOTHING",
            params![id],
        )?;

        let applied = apply_field_writes(
            tx,
            self.entity_kind(),
            &entity.entity_key,
            fetched_at,
            &entity.fields,
        )?;

        for field in &entity.fields {
            if !applied.fields.contains(&field.name) {
                continue;
            }
            match field.name {
                "name" => {
                    tx.execute(
                        "UPDATE fake_entities SET name = ?1 WHERE id = ?2",
                        params![field.value, id],
                    )?;
                }
                "due_at" => {
                    tx.execute(
                        "UPDATE fake_entities SET due_at = ?1 WHERE id = ?2",
                        params![field.value, id],
                    )?;
                }
                "description" => {
                    tx.execute(
                        "UPDATE fake_entities SET description = ?1 WHERE id = ?2",
                        params![field.value, id],
                    )?;
                }
                "score" => {
                    let score: Option<f64> = field
                        .value
                        .as_ref()
                        .map(|s| s.parse())
                        .transpose()
                        .map_err(|e: std::num::ParseFloatError| {
                            DbError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
                        })?;
                    tx.execute(
                        "UPDATE fake_entities SET score = ?1 WHERE id = ?2",
                        params![score, id],
                    )?;
                }
                "can_submit" => {
                    let v: Option<i64> = field
                        .value
                        .as_ref()
                        .map(|s| i64::from(s == "true" || s == "1"));
                    tx.execute(
                        "UPDATE fake_entities SET can_submit = ?1 WHERE id = ?2",
                        params![v, id],
                    )?;
                }
                _ => {}
            }
        }

        let ts = fetched_at.to_string();
        if applied.core {
            tx.execute(
                "UPDATE fake_entities SET observed_at_core = ?1 WHERE id = ?2",
                params![ts, id],
            )?;
        }
        if applied.detail {
            tx.execute(
                "UPDATE fake_entities SET observed_at_detail = ?1 WHERE id = ?2",
                params![ts, id],
            )?;
        }
        if applied.status {
            tx.execute(
                "UPDATE fake_entities SET observed_at_status = ?1 WHERE id = ?2",
                params![ts, id],
            )?;
        }
        Ok(())
    }
}

/// Page helper for `FakeEntity` tests.
#[derive(Debug, Clone)]
pub struct FakeEntityPage {
    pub fetched_at: Timestamp,
    pub rows: Vec<FakeEntityRow>,
}

/// One `FakeEntity` row with three-state fields.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct FakeEntityRow {
    pub id: i64,
    pub name: Supplied<String>,
    pub due_at: Supplied<String>,
    pub description: Supplied<String>,
    pub score: Supplied<f64>,
    pub can_submit: Supplied<bool>,
}

impl FakeEntityPage {
    #[must_use]
    pub fn to_ingest(&self) -> IngestPage {
        IngestPage {
            fetched_at: self.fetched_at,
            entities: self.rows.iter().map(FakeEntityRow::to_entity).collect(),
        }
    }
}

impl FakeEntityRow {
    #[must_use]
    pub fn to_entity(&self) -> EntityIngest {
        let mut fields = Vec::new();
        push_str(&mut fields, "name", FieldGroup::Core, &self.name);
        push_str(&mut fields, "due_at", FieldGroup::Core, &self.due_at);
        push_str(
            &mut fields,
            "description",
            FieldGroup::Detail,
            &self.description,
        );
        match &self.score {
            Supplied::Absent => {}
            Supplied::Null => fields.push(FieldWrite {
                name: "score",
                group: FieldGroup::Status,
                value: None,
            }),
            Supplied::Value(v) => fields.push(FieldWrite {
                name: "score",
                group: FieldGroup::Status,
                value: Some(v.to_string()),
            }),
        }
        match &self.can_submit {
            Supplied::Absent => {}
            Supplied::Null => fields.push(FieldWrite {
                name: "can_submit",
                group: FieldGroup::Detail,
                value: None,
            }),
            Supplied::Value(v) => fields.push(FieldWrite {
                name: "can_submit",
                group: FieldGroup::Detail,
                value: Some(v.to_string()),
            }),
        }
        EntityIngest {
            entity_key: self.id.to_string(),
            fields,
        }
    }
}

fn push_str(
    fields: &mut Vec<FieldWrite>,
    name: &'static str,
    group: FieldGroup,
    supplied: &Supplied<String>,
) {
    match supplied {
        Supplied::Absent => {}
        Supplied::Null => fields.push(FieldWrite {
            name,
            group,
            value: None,
        }),
        Supplied::Value(v) => fields.push(FieldWrite {
            name,
            group,
            value: Some(v.clone()),
        }),
    }
}

/// Upsert `enrollment_grades` with composite key `enrollment_id|period`.
pub fn upsert_enrollment_grades(
    tx: &Transaction<'_>,
    enrollment_id: i64,
    period: &str,
    fetched_at: Timestamp,
    fields: &[FieldWrite],
) -> Result<(), IngestError> {
    validate_fields(
        fields,
        &[
            "course_id",
            "current_score",
            "final_score",
            "current_grade",
            "final_grade",
        ],
    )?;
    let entity_key = format!("{enrollment_id}|{period}");
    tx.execute(
        "INSERT INTO enrollment_grades (enrollment_id, period) VALUES (?1, ?2)
         ON CONFLICT(enrollment_id, period) DO NOTHING",
        params![enrollment_id, period],
    )?;
    let applied = apply_field_writes(tx, "enrollment_grades", &entity_key, fetched_at, fields)?;
    for field in fields {
        if !applied.fields.contains(&field.name) {
            continue;
        }
        match field.name {
            "current_score" => {
                let v: Option<f64> = field
                    .value
                    .as_ref()
                    .map(|s| s.parse())
                    .transpose()
                    .map_err(|e: std::num::ParseFloatError| {
                        DbError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
                    })?;
                tx.execute(
                    "UPDATE enrollment_grades SET current_score = ?1
                     WHERE enrollment_id = ?2 AND period = ?3",
                    params![v, enrollment_id, period],
                )?;
            }
            "final_score" => {
                let v: Option<f64> = field
                    .value
                    .as_ref()
                    .map(|s| s.parse())
                    .transpose()
                    .map_err(|e: std::num::ParseFloatError| {
                        DbError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
                    })?;
                tx.execute(
                    "UPDATE enrollment_grades SET final_score = ?1
                     WHERE enrollment_id = ?2 AND period = ?3",
                    params![v, enrollment_id, period],
                )?;
            }
            "current_grade" | "final_grade" | "course_id" => {
                // Identifiers above are allowlisted, values remain bound parameters.
                tx.execute(&format!("UPDATE enrollment_grades SET {} = ?1 WHERE enrollment_id = ?2 AND period = ?3", field.name),
                    params![field.value, enrollment_id, period])?;
            }
            _ => unreachable!("fields were validated"),
        }
    }
    let ts = fetched_at.to_string();
    if applied.core {
        tx.execute(
            "UPDATE enrollment_grades SET observed_at_core = ?1
             WHERE enrollment_id = ?2 AND period = ?3",
            params![ts, enrollment_id, period],
        )?;
    }
    if applied.detail {
        tx.execute(
            "UPDATE enrollment_grades SET observed_at_detail = ?1
             WHERE enrollment_id = ?2 AND period = ?3",
            params![ts, enrollment_id, period],
        )?;
    }
    if applied.status {
        tx.execute(
            "UPDATE enrollment_grades SET observed_at_status = ?1
             WHERE enrollment_id = ?2 AND period = ?3",
            params![ts, enrollment_id, period],
        )?;
    }
    Ok(())
}
