//! `files` dataset (`course:<id>`).

use canvas_api::models::File;
use jiff::{Span, Timestamp};
use rusqlite::{Transaction, params};
use serde_json::{Map, Value};

use crate::store::{
    Dataset, DbError, EntityIngest, FieldGroup, FieldWrite, IngestError, IngestPage,
    apply_field_writes,
};

use super::fields::{push_api_to_string, push_opt_bool, push_opt_i64, push_opt_str, push_opt_ts};
use super::folders::{merge_extra, parse_entity_id, touch_observed, validate_fields};

/// Default TTL: `ttl_files` = 1 hour.
#[must_use]
pub fn default_ttl_files() -> Span {
    Span::new().hours(1)
}

/// Files listing for one course.
#[derive(Debug, Clone)]
pub struct FilesDataset {
    pub course_id: i64,
    pub ttl: Span,
    scope_key: String,
}

impl FilesDataset {
    #[must_use]
    pub fn new(course_id: i64, ttl: Span) -> Self {
        Self {
            course_id,
            ttl,
            scope_key: format!("course:{course_id}"),
        }
    }

    #[must_use]
    pub fn with_default_ttl(course_id: i64) -> Self {
        Self::new(course_id, default_ttl_files())
    }
}

impl Dataset for FilesDataset {
    fn name(&self) -> &'static str {
        "files"
    }

    fn scope_key(&self) -> &str {
        &self.scope_key
    }

    fn ttl(&self) -> Span {
        self.ttl
    }

    fn entity_kind(&self) -> &'static str {
        "file"
    }

    fn upsert_entity(
        &self,
        tx: &Transaction<'_>,
        entity: &EntityIngest,
        fetched_at: Timestamp,
    ) -> Result<(), IngestError> {
        upsert_file(tx, self.course_id, entity, fetched_at)
    }
}

/// Path for course files listing.
#[must_use]
pub fn files_path(course_id: i64) -> String {
    format!("/api/v1/courses/{course_id}/files")
}

/// Convert file items into an ingest page.
#[must_use]
pub fn files_to_ingest_page(files: &[File], course_id: i64, fetched_at: Timestamp) -> IngestPage {
    IngestPage {
        fetched_at,
        entities: files.iter().map(|f| file_to_entity(f, course_id)).collect(),
    }
}

/// Convert one file into an entity ingest row.
///
/// `hidden` and `locked` / `locked_for_user` are separate fields and are never merged.
#[must_use]
pub fn file_to_entity(file: &File, course_id: i64) -> EntityIngest {
    let mut fields = Vec::new();
    fields.push(FieldWrite {
        name: "course_id",
        group: FieldGroup::Core,
        value: Some(course_id.to_string()),
    });
    push_opt_i64(&mut fields, "folder_id", FieldGroup::Core, file.folder_id);
    push_opt_str(
        &mut fields,
        "display_name",
        FieldGroup::Core,
        file.display_name.as_deref(),
    );
    push_opt_str(
        &mut fields,
        "filename",
        FieldGroup::Core,
        file.filename.as_deref(),
    );
    if let Some(size) = file.size {
        fields.push(FieldWrite {
            name: "size",
            group: FieldGroup::Core,
            value: Some(size.to_string()),
        });
    }
    push_opt_str(
        &mut fields,
        "content_type",
        FieldGroup::Core,
        file.content_type.as_deref(),
    );
    // Keep listing visibility and lock state independent.
    push_opt_bool(&mut fields, "hidden", FieldGroup::Status, file.hidden);
    push_opt_bool(&mut fields, "locked", FieldGroup::Status, file.locked);
    push_opt_bool(
        &mut fields,
        "locked_for_user",
        FieldGroup::Status,
        file.locked_for_user,
    );
    push_opt_str(
        &mut fields,
        "lock_explanation",
        FieldGroup::Status,
        file.lock_explanation.as_deref(),
    );
    push_opt_ts(
        &mut fields,
        "updated_at",
        FieldGroup::Detail,
        file.updated_at,
    );
    push_opt_str(
        &mut fields,
        "url",
        FieldGroup::Detail,
        file.url.as_ref().map(reqwest::Url::as_str),
    );
    push_opt_str(
        &mut fields,
        "thumbnail_url",
        FieldGroup::Detail,
        file.thumbnail_url.as_ref().map(reqwest::Url::as_str),
    );
    push_api_to_string(
        &mut fields,
        "unlock_at",
        FieldGroup::Status,
        &file.unlock_at,
    );
    push_api_to_string(&mut fields, "lock_at", FieldGroup::Status, &file.lock_at);
    EntityIngest {
        entity_key: file.id.to_string(),
        fields,
    }
}

struct FileSplit {
    course_id: i64,
    column_fields: Vec<FieldWrite>,
    extra: Map<String, Value>,
}

fn upsert_file(
    tx: &Transaction<'_>,
    dataset_course_id: i64,
    entity: &EntityIngest,
    fetched_at: Timestamp,
) -> Result<(), IngestError> {
    let id = parse_entity_id("file", &entity.entity_key)?;
    let split = split_file_fields(dataset_course_id, &entity.fields)?;
    validate_fields(
        &split.column_fields,
        &[
            "course_id",
            "folder_id",
            "display_name",
            "filename",
            "size",
            "content_type",
        ],
    )?;
    tx.execute(
        "INSERT INTO files (id, course_id) VALUES (?1, ?2)
         ON CONFLICT(id) DO NOTHING",
        params![id, split.course_id],
    )?;
    let applied = apply_field_writes(tx, "file", &entity.entity_key, fetched_at, &entity.fields)?;
    if applied.fields.contains(&"course_id") {
        tx.execute(
            "UPDATE files SET course_id = ?1 WHERE id = ?2",
            params![split.course_id, id],
        )?;
    }
    apply_file_columns(tx, id, &split.column_fields, &applied.fields)?;
    let extra = split
        .extra
        .into_iter()
        .filter(|(name, _)| applied.fields.contains(&name.as_str()))
        .collect();
    merge_extra(tx, "files", id, extra)?;
    touch_observed(
        tx,
        "files",
        id,
        fetched_at,
        applied.core,
        applied.detail,
        applied.status,
    )?;
    Ok(())
}

fn split_file_fields(
    dataset_course_id: i64,
    fields: &[FieldWrite],
) -> Result<FileSplit, IngestError> {
    let mut column_fields = Vec::new();
    let mut extra = Map::new();
    let mut course_id = dataset_course_id;
    for field in fields {
        match field.name {
            "folder_id" | "display_name" | "filename" | "size" | "content_type" => {
                column_fields.push(field.clone());
            }
            "course_id" => {
                if let Some(ref v) = field.value {
                    course_id = v.parse().map_err(|e| {
                        DbError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
                    })?;
                }
                column_fields.push(field.clone());
            }
            "hidden" | "locked" | "locked_for_user" | "lock_explanation" | "updated_at" | "url"
            | "thumbnail_url" | "unlock_at" | "lock_at" => {
                extra.insert(
                    field.name.to_owned(),
                    match &field.value {
                        Some(v) => Value::String(v.clone()),
                        None => Value::Null,
                    },
                );
            }
            other => {
                return Err(DbError::Message(format!("unsupported file field: {other}")).into());
            }
        }
    }
    Ok(FileSplit {
        course_id,
        column_fields,
        extra,
    })
}

fn apply_file_columns(
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
            "display_name" | "filename" | "content_type" => {
                tx.execute(
                    &format!("UPDATE files SET {} = ?1 WHERE id = ?2", field.name),
                    params![field.value, id],
                )?;
            }
            "folder_id" | "size" => {
                let v: Option<i64> = field
                    .value
                    .as_ref()
                    .map(|s| s.parse())
                    .transpose()
                    .map_err(|e: std::num::ParseIntError| {
                        DbError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
                    })?;
                tx.execute(
                    &format!("UPDATE files SET {} = ?1 WHERE id = ?2", field.name),
                    params![v, id],
                )?;
            }
            _ => {}
        }
    }
    Ok(())
}
