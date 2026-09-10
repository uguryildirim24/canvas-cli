//! `folders` dataset (`course:<id>`).

use canvas_api::models::Folder;
use jiff::{Span, Timestamp};
use rusqlite::{Transaction, params};
use serde_json::{Map, Value};

use crate::store::{
    Dataset, DbError, EntityIngest, FieldGroup, FieldWrite, IngestError, IngestPage,
    apply_field_writes,
};

use super::fields::{push_api_str, push_opt_bool, push_opt_i64, push_opt_str, push_opt_ts};

/// Folders listing for one course.
#[derive(Debug, Clone)]
pub struct FoldersDataset {
    pub course_id: i64,
    pub ttl: Span,
    scope_key: String,
}

impl FoldersDataset {
    #[must_use]
    pub fn new(course_id: i64, ttl: Span) -> Self {
        Self {
            course_id,
            ttl,
            scope_key: format!("course:{course_id}"),
        }
    }

    /// Uses [`super::files::default_ttl_files`] (1 hour).
    #[must_use]
    pub fn with_default_ttl(course_id: i64) -> Self {
        Self::new(course_id, Span::new().hours(1))
    }
}

impl Dataset for FoldersDataset {
    fn name(&self) -> &'static str {
        "folders"
    }

    fn scope_key(&self) -> &str {
        &self.scope_key
    }

    fn ttl(&self) -> Span {
        self.ttl
    }

    fn entity_kind(&self) -> &'static str {
        "folder"
    }

    fn upsert_entity(
        &self,
        tx: &Transaction<'_>,
        entity: &EntityIngest,
        fetched_at: Timestamp,
    ) -> Result<(), IngestError> {
        upsert_folder(tx, self.course_id, entity, fetched_at)
    }
}

/// Path for course folders listing.
#[must_use]
pub fn folders_path(course_id: i64) -> String {
    format!("/api/v1/courses/{course_id}/folders")
}

/// Convert folder items into an ingest page.
#[must_use]
pub fn folders_to_ingest_page(
    folders: &[Folder],
    course_id: i64,
    fetched_at: Timestamp,
) -> IngestPage {
    IngestPage {
        fetched_at,
        entities: folders
            .iter()
            .map(|f| folder_to_entity(f, course_id))
            .collect(),
    }
}

/// Convert one folder into an entity ingest row.
#[must_use]
pub fn folder_to_entity(folder: &Folder, course_id: i64) -> EntityIngest {
    let mut fields = Vec::new();
    push_api_str(&mut fields, "name", FieldGroup::Core, &folder.name);
    push_opt_str(
        &mut fields,
        "full_name",
        FieldGroup::Core,
        folder.full_name.as_deref(),
    );
    push_opt_i64(
        &mut fields,
        "parent_folder_id",
        FieldGroup::Core,
        folder.parent_folder_id,
    );
    fields.push(FieldWrite {
        name: "course_id",
        group: FieldGroup::Core,
        value: Some(course_id.to_string()),
    });
    push_opt_bool(&mut fields, "hidden", FieldGroup::Status, folder.hidden);
    push_opt_bool(&mut fields, "locked", FieldGroup::Status, folder.locked);
    push_opt_bool(
        &mut fields,
        "locked_for_user",
        FieldGroup::Status,
        folder.locked_for_user,
    );
    push_opt_i64(
        &mut fields,
        "context_id",
        FieldGroup::Detail,
        folder.context_id,
    );
    push_opt_str(
        &mut fields,
        "context_type",
        FieldGroup::Detail,
        folder.context_type.as_deref(),
    );
    if let Some(n) = folder.files_count {
        fields.push(FieldWrite {
            name: "files_count",
            group: FieldGroup::Detail,
            value: Some(n.to_string()),
        });
    }
    if let Some(n) = folder.folders_count {
        fields.push(FieldWrite {
            name: "folders_count",
            group: FieldGroup::Detail,
            value: Some(n.to_string()),
        });
    }
    push_opt_i64(&mut fields, "position", FieldGroup::Detail, folder.position);
    push_opt_ts(
        &mut fields,
        "updated_at",
        FieldGroup::Detail,
        folder.updated_at,
    );
    push_opt_str(
        &mut fields,
        "files_url",
        FieldGroup::Detail,
        folder.files_url.as_ref().map(reqwest::Url::as_str),
    );
    push_opt_str(
        &mut fields,
        "folders_url",
        FieldGroup::Detail,
        folder.folders_url.as_ref().map(reqwest::Url::as_str),
    );
    EntityIngest {
        entity_key: folder.id.to_string(),
        fields,
    }
}

struct FolderSplit {
    course_id: i64,
    column_fields: Vec<FieldWrite>,
    extra: Map<String, Value>,
}

fn upsert_folder(
    tx: &Transaction<'_>,
    dataset_course_id: i64,
    entity: &EntityIngest,
    fetched_at: Timestamp,
) -> Result<(), IngestError> {
    let id = parse_id("folder", &entity.entity_key)?;
    let split = split_folder_fields(dataset_course_id, &entity.fields)?;
    validate_column_fields(
        &split.column_fields,
        &["name", "full_name", "parent_folder_id", "course_id"],
    )?;
    tx.execute(
        "INSERT INTO folders (id, course_id) VALUES (?1, ?2)
         ON CONFLICT(id) DO NOTHING",
        params![id, split.course_id],
    )?;
    let applied = apply_field_writes(tx, "folder", &entity.entity_key, fetched_at, &entity.fields)?;
    if applied.fields.contains(&"course_id") {
        tx.execute(
            "UPDATE folders SET course_id = ?1 WHERE id = ?2",
            params![split.course_id, id],
        )?;
    }
    apply_folder_columns(tx, id, &split.column_fields, &applied.fields)?;
    let extra = split
        .extra
        .into_iter()
        .filter(|(name, _)| applied.fields.contains(&name.as_str()))
        .collect();
    merge_extra(tx, "folders", id, extra)?;
    touch_observed(
        tx,
        "folders",
        id,
        fetched_at,
        applied.core,
        applied.detail,
        applied.status,
    )?;
    Ok(())
}

fn parse_id(kind: &str, entity_key: &str) -> Result<i64, IngestError> {
    let id: i64 = entity_key.parse().map_err(|e| {
        DbError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("bad {kind} id: {e}"),
            ),
        )))
    })?;
    if entity_key != id.to_string() {
        return Err(DbError::Message("entity key must be normalized".into()).into());
    }
    Ok(id)
}

fn split_folder_fields(
    dataset_course_id: i64,
    fields: &[FieldWrite],
) -> Result<FolderSplit, IngestError> {
    let mut column_fields = Vec::new();
    let mut extra = Map::new();
    let mut course_id = dataset_course_id;
    for field in fields {
        match field.name {
            "name" | "full_name" | "parent_folder_id" => column_fields.push(field.clone()),
            "course_id" => {
                if let Some(ref v) = field.value {
                    course_id = v.parse().map_err(|e| {
                        DbError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
                    })?;
                }
                column_fields.push(field.clone());
            }
            "hidden" | "locked" | "locked_for_user" | "context_id" | "context_type"
            | "files_count" | "folders_count" | "position" | "updated_at" | "files_url"
            | "folders_url" => {
                extra.insert(
                    field.name.to_owned(),
                    match &field.value {
                        Some(v) => Value::String(v.clone()),
                        None => Value::Null,
                    },
                );
            }
            other => {
                return Err(DbError::Message(format!("unsupported folder field: {other}")).into());
            }
        }
    }
    Ok(FolderSplit {
        course_id,
        column_fields,
        extra,
    })
}

fn apply_folder_columns(
    tx: &Transaction<'_>,
    id: i64,
    tracked: &[FieldWrite],
    applied: &[&str],
) -> Result<(), IngestError> {
    for field in tracked {
        if !applied.contains(&field.name) {
            continue;
        }
        match field.name {
            "name" | "full_name" => {
                tx.execute(
                    &format!("UPDATE folders SET {} = ?1 WHERE id = ?2", field.name),
                    params![field.value, id],
                )?;
            }
            "parent_folder_id" => {
                let v: Option<i64> = field
                    .value
                    .as_ref()
                    .map(|s| s.parse())
                    .transpose()
                    .map_err(|e: std::num::ParseIntError| {
                        DbError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
                    })?;
                tx.execute(
                    "UPDATE folders SET parent_folder_id = ?1 WHERE id = ?2",
                    params![v, id],
                )?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_column_fields(fields: &[FieldWrite], allowed: &[&str]) -> Result<(), IngestError> {
    let mut seen = std::collections::HashSet::new();
    for field in fields {
        if !allowed.contains(&field.name) || !seen.insert(field.name) {
            return Err(DbError::Message("unsupported or duplicate field".into()).into());
        }
    }
    Ok(())
}

pub(super) fn merge_extra(
    tx: &Transaction<'_>,
    table: &str,
    id: i64,
    extra: Map<String, Value>,
) -> Result<(), IngestError> {
    if extra.is_empty() {
        return Ok(());
    }
    let raw: String = tx.query_row(
        &format!("SELECT data_json FROM {table} WHERE id = ?1"),
        params![id],
        |r| r.get(0),
    )?;
    let mut data: Value = serde_json::from_str(&raw).map_err(|e| {
        DbError::Sqlite(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(e),
        ))
    })?;
    let obj = data
        .as_object_mut()
        .ok_or_else(|| DbError::Message(format!("{table} data_json must be an object")))?;
    for (k, v) in extra {
        obj.insert(k, v);
    }
    tx.execute(
        &format!("UPDATE {table} SET data_json = ?1 WHERE id = ?2"),
        params![data.to_string(), id],
    )?;
    Ok(())
}

pub(super) fn touch_observed(
    tx: &Transaction<'_>,
    table: &str,
    id: i64,
    fetched_at: Timestamp,
    core: bool,
    detail: bool,
    status: bool,
) -> Result<(), IngestError> {
    let ts = fetched_at.to_string();
    if core {
        tx.execute(
            &format!("UPDATE {table} SET observed_at_core = ?1 WHERE id = ?2"),
            params![ts, id],
        )?;
    }
    if detail {
        tx.execute(
            &format!("UPDATE {table} SET observed_at_detail = ?1 WHERE id = ?2"),
            params![ts, id],
        )?;
    }
    if status {
        tx.execute(
            &format!("UPDATE {table} SET observed_at_status = ?1 WHERE id = ?2"),
            params![ts, id],
        )?;
    }
    Ok(())
}

pub(super) fn parse_entity_id(kind: &str, entity_key: &str) -> Result<i64, IngestError> {
    parse_id(kind, entity_key)
}

pub(super) fn validate_fields(fields: &[FieldWrite], allowed: &[&str]) -> Result<(), IngestError> {
    validate_column_fields(fields, allowed)
}
