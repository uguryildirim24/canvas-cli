//! `modules` dataset (`course:<id>`) with module-item completeness.

use canvas_api::models::{Module, ModuleItem};
use jiff::{Span, Timestamp};
use rusqlite::{Transaction, params};
use serde_json::{Map, Value};

use crate::store::{
    Dataset, DbError, EntityIngest, FieldGroup, FieldWrite, IngestError, IngestPage,
    apply_field_writes,
};

use super::fields::{
    push_api_str, push_api_to_string, push_opt_bool, push_opt_i64, push_opt_str, push_opt_ts,
};
use super::folders::{merge_extra, parse_entity_id, touch_observed, validate_fields};

/// Default TTL: `ttl_modules` = 1 hour.
#[must_use]
pub fn default_ttl_modules() -> Span {
    Span::new().hours(1)
}

/// Modules listing for one course.
#[derive(Debug, Clone)]
pub struct ModulesDataset {
    pub course_id: i64,
    pub ttl: Span,
    scope_key: String,
}

impl ModulesDataset {
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
        Self::new(course_id, default_ttl_modules())
    }
}

impl Dataset for ModulesDataset {
    fn name(&self) -> &'static str {
        "modules"
    }

    fn scope_key(&self) -> &str {
        &self.scope_key
    }

    fn ttl(&self) -> Span {
        self.ttl
    }

    fn entity_kind(&self) -> &'static str {
        "module"
    }

    fn upsert_entity(
        &self,
        tx: &Transaction<'_>,
        entity: &EntityIngest,
        fetched_at: Timestamp,
    ) -> Result<(), IngestError> {
        upsert_module(tx, self.course_id, entity, fetched_at)
    }
}

/// Path for modules with inline items and content details.
#[must_use]
pub fn modules_path(course_id: i64) -> String {
    format!("/api/v1/courses/{course_id}/modules?include[]=items&include[]=content_details")
}

/// Path for one module's items with content details.
#[must_use]
pub fn module_items_path(course_id: i64, module_id: i64) -> String {
    format!("/api/v1/courses/{course_id}/modules/{module_id}/items?include[]=content_details")
}

/// True when inline `items` is absent/`None` or shorter than `items_count`.
#[must_use]
pub fn needs_items_fetch(module: &Module) -> bool {
    match &module.items {
        None => true,
        Some(items) => {
            let expected = usize::try_from(module.items_count.unwrap_or(0)).unwrap_or(usize::MAX);
            items.len() < expected
        }
    }
}

/// Count of per-module item list requests implied by [`needs_items_fetch`].
#[must_use]
pub fn count_item_fetch_requests(modules: &[Module]) -> usize {
    modules.iter().filter(|m| needs_items_fetch(m)).count()
}

/// Convert modules (with resolved items) into an ingest page.
#[must_use]
pub fn modules_to_ingest_page(
    modules: &[(Module, bool, Vec<ModuleItem>)],
    course_id: i64,
    fetched_at: Timestamp,
) -> IngestPage {
    IngestPage {
        fetched_at,
        entities: modules
            .iter()
            .map(|(m, complete, items)| module_to_entity(m, course_id, *complete, items))
            .collect(),
    }
}

/// Convert one module and its items into an entity ingest row.
#[must_use]
pub fn module_to_entity(
    module: &Module,
    course_id: i64,
    items_complete: bool,
    items: &[ModuleItem],
) -> EntityIngest {
    let mut fields = Vec::new();
    fields.push(FieldWrite {
        name: "course_id",
        group: FieldGroup::Core,
        value: Some(course_id.to_string()),
    });
    push_api_str(&mut fields, "name", FieldGroup::Core, &module.name);
    push_opt_i64(&mut fields, "position", FieldGroup::Core, module.position);
    if let Some(n) = module.items_count {
        fields.push(FieldWrite {
            name: "items_count",
            group: FieldGroup::Core,
            value: Some(n.to_string()),
        });
    }
    fields.push(FieldWrite {
        name: "items_complete",
        group: FieldGroup::Status,
        value: Some(if items_complete { "true" } else { "false" }.to_owned()),
    });
    push_opt_str(
        &mut fields,
        "state",
        FieldGroup::Status,
        module.state.as_deref(),
    );
    push_api_to_string(
        &mut fields,
        "unlock_at",
        FieldGroup::Detail,
        &module.unlock_at,
    );
    push_opt_bool(
        &mut fields,
        "require_sequential_progress",
        FieldGroup::Detail,
        module.require_sequential_progress,
    );
    push_opt_bool(
        &mut fields,
        "published",
        FieldGroup::Detail,
        module.published,
    );
    push_opt_ts(
        &mut fields,
        "completed_at",
        FieldGroup::Detail,
        module.completed_at,
    );
    let item_entities: Vec<EntityIngest> = items
        .iter()
        .map(|item| module_item_to_entity(item, course_id, module.id, module.state.as_deref()))
        .collect();
    if let Ok(json) = serde_json::to_string(&items_payload(&item_entities)) {
        fields.push(FieldWrite {
            name: "items_payload",
            group: FieldGroup::Detail,
            value: Some(json),
        });
    }
    EntityIngest {
        entity_key: module.id.to_string(),
        fields,
    }
}

/// Convert one module item into an entity ingest row.
#[must_use]
pub fn module_item_to_entity(
    item: &ModuleItem,
    course_id: i64,
    module_id: i64,
    module_state: Option<&str>,
) -> EntityIngest {
    let mut fields = Vec::new();
    fields.push(FieldWrite {
        name: "course_id",
        group: FieldGroup::Core,
        value: Some(course_id.to_string()),
    });
    fields.push(FieldWrite {
        name: "module_id",
        group: FieldGroup::Core,
        value: Some(module_id.to_string()),
    });
    push_opt_str(
        &mut fields,
        "title",
        FieldGroup::Core,
        item.title.as_deref(),
    );
    push_opt_i64(&mut fields, "position", FieldGroup::Core, item.position);
    push_opt_i64(&mut fields, "content_id", FieldGroup::Core, item.content_id);
    push_opt_str(
        &mut fields,
        "type",
        FieldGroup::Core,
        item.item_type.as_deref(),
    );
    push_opt_i64(&mut fields, "indent", FieldGroup::Detail, item.indent);
    push_opt_bool(&mut fields, "published", FieldGroup::Detail, item.published);
    if let Some(ref details) = item.content_details {
        push_opt_bool(
            &mut fields,
            "locked_for_user",
            FieldGroup::Status,
            details.locked_for_user,
        );
        push_opt_str(
            &mut fields,
            "lock_explanation",
            FieldGroup::Status,
            details.lock_explanation.as_deref(),
        );
    }
    if let Some(state) = module_state {
        fields.push(FieldWrite {
            name: "module_state",
            group: FieldGroup::Status,
            value: Some(state.to_owned()),
        });
    }
    EntityIngest {
        entity_key: item.id.to_string(),
        fields,
    }
}

fn items_payload(entities: &[EntityIngest]) -> Vec<Value> {
    entities
        .iter()
        .map(|e| {
            serde_json::json!({
                "entity_key": e.entity_key,
                "fields": json_field_map(&e.fields),
            })
        })
        .collect()
}

fn json_field_map(fields: &[FieldWrite]) -> Map<String, Value> {
    let mut map = Map::new();
    for field in fields {
        map.insert(
            field.name.to_owned(),
            match &field.value {
                Some(v) => Value::String(v.clone()),
                None => Value::Null,
            },
        );
    }
    map
}

struct ModuleSplit {
    course_id: i64,
    column_fields: Vec<FieldWrite>,
    extra: Map<String, Value>,
    items_payload: Option<String>,
}

fn upsert_module(
    tx: &Transaction<'_>,
    dataset_course_id: i64,
    entity: &EntityIngest,
    fetched_at: Timestamp,
) -> Result<(), IngestError> {
    let id = parse_entity_id("module", &entity.entity_key)?;
    let split = split_module_fields(dataset_course_id, &entity.fields)?;
    validate_fields(
        &split.column_fields,
        &[
            "course_id",
            "name",
            "position",
            "items_count",
            "items_complete",
        ],
    )?;
    tx.execute(
        "INSERT INTO modules (id, course_id) VALUES (?1, ?2)
         ON CONFLICT(id) DO NOTHING",
        params![id, split.course_id],
    )?;
    let real_fields: Vec<_> = entity
        .fields
        .iter()
        .filter(|f| f.name != "items_payload")
        .cloned()
        .collect();
    let applied = apply_field_writes(tx, "module", &entity.entity_key, fetched_at, &real_fields)?;
    if applied.fields.contains(&"course_id") {
        tx.execute(
            "UPDATE modules SET course_id = ?1 WHERE id = ?2",
            params![split.course_id, id],
        )?;
    }
    apply_module_columns(tx, id, &split.column_fields, &applied.fields)?;
    let extra = split
        .extra
        .into_iter()
        .filter(|(name, _)| applied.fields.contains(&name.as_str()))
        .collect();
    merge_extra(tx, "modules", id, extra)?;
    touch_observed(
        tx,
        "modules",
        id,
        fetched_at,
        applied.core,
        applied.detail,
        applied.status,
    )?;
    if let Some(payload) = split.items_payload {
        write_items_side_effect(tx, &payload, fetched_at)?;
    }
    Ok(())
}

fn split_module_fields(
    dataset_course_id: i64,
    fields: &[FieldWrite],
) -> Result<ModuleSplit, IngestError> {
    let mut column_fields = Vec::new();
    let mut extra = Map::new();
    let mut course_id = dataset_course_id;
    let mut items_payload = None;
    for field in fields {
        match field.name {
            "name" | "position" | "items_count" | "items_complete" => {
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
            "state"
            | "unlock_at"
            | "require_sequential_progress"
            | "published"
            | "completed_at" => {
                extra.insert(
                    field.name.to_owned(),
                    match &field.value {
                        Some(v) => Value::String(v.clone()),
                        None => Value::Null,
                    },
                );
            }
            "items_payload" => items_payload.clone_from(&field.value),
            other => {
                return Err(DbError::Message(format!("unsupported module field: {other}")).into());
            }
        }
    }
    Ok(ModuleSplit {
        course_id,
        column_fields,
        extra,
        items_payload,
    })
}

fn apply_module_columns(
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
            "name" => {
                tx.execute(
                    "UPDATE modules SET name = ?1 WHERE id = ?2",
                    params![field.value, id],
                )?;
            }
            "position" | "items_count" => {
                let v: Option<i64> = field
                    .value
                    .as_ref()
                    .map(|s| s.parse())
                    .transpose()
                    .map_err(|e: std::num::ParseIntError| {
                        DbError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
                    })?;
                tx.execute(
                    &format!("UPDATE modules SET {} = ?1 WHERE id = ?2", field.name),
                    params![v, id],
                )?;
            }
            "items_complete" => {
                let v: Option<i64> = field
                    .value
                    .as_ref()
                    .map(|s| i64::from(s == "true" || s == "1"));
                tx.execute(
                    "UPDATE modules SET items_complete = ?1 WHERE id = ?2",
                    params![v, id],
                )?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn write_items_side_effect(
    tx: &Transaction<'_>,
    payload: &str,
    fetched_at: Timestamp,
) -> Result<(), IngestError> {
    let items: Vec<Value> = serde_json::from_str(payload).map_err(|e| {
        DbError::Sqlite(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(e),
        ))
    })?;
    for item in items {
        let key = item
            .get("entity_key")
            .and_then(Value::as_str)
            .ok_or_else(|| DbError::Message("items payload missing entity_key".into()))?;
        let field_map = item
            .get("fields")
            .and_then(Value::as_object)
            .ok_or_else(|| DbError::Message("items payload missing fields".into()))?;
        let mut fields = Vec::new();
        for (name, value) in field_map {
            let static_name: &'static str = match name.as_str() {
                "course_id" => "course_id",
                "module_id" => "module_id",
                "title" => "title",
                "position" => "position",
                "content_id" => "content_id",
                "type" => "type",
                "indent" => "indent",
                "published" => "published",
                "locked_for_user" => "locked_for_user",
                "lock_explanation" => "lock_explanation",
                "module_state" => "module_state",
                other => {
                    return Err(DbError::Message(format!(
                        "unsupported module_item field: {other}"
                    ))
                    .into());
                }
            };
            let group = match static_name {
                "locked_for_user" | "lock_explanation" | "module_state" => FieldGroup::Status,
                "indent" | "published" => FieldGroup::Detail,
                _ => FieldGroup::Core,
            };
            fields.push(FieldWrite {
                name: static_name,
                group,
                value: match value {
                    Value::Null => None,
                    Value::String(s) => Some(s.clone()),
                    other => Some(other.to_string()),
                },
            });
        }
        upsert_module_item(
            tx,
            &EntityIngest {
                entity_key: key.to_owned(),
                fields,
            },
            fetched_at,
        )?;
    }
    Ok(())
}

struct ModuleItemSplit {
    course_id: i64,
    module_id: i64,
    column_fields: Vec<FieldWrite>,
    extra: Map<String, Value>,
}

fn split_module_item_fields(fields: &[FieldWrite]) -> Result<ModuleItemSplit, IngestError> {
    let mut course_id = 0i64;
    let mut module_id = 0i64;
    let mut column_fields = Vec::new();
    let mut extra = Map::new();
    for field in fields {
        match field.name {
            "course_id" => {
                if let Some(ref v) = field.value {
                    course_id = v.parse().map_err(|e| {
                        DbError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
                    })?;
                }
                column_fields.push(field.clone());
            }
            "module_id" => {
                if let Some(ref v) = field.value {
                    module_id = v.parse().map_err(|e| {
                        DbError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
                    })?;
                }
                column_fields.push(field.clone());
            }
            "title" | "position" | "content_id" | "type" => column_fields.push(field.clone()),
            "indent" | "published" | "locked_for_user" | "lock_explanation" | "module_state" => {
                extra.insert(
                    field.name.to_owned(),
                    match &field.value {
                        Some(v) => Value::String(v.clone()),
                        None => Value::Null,
                    },
                );
            }
            other => {
                return Err(
                    DbError::Message(format!("unsupported module_item field: {other}")).into(),
                );
            }
        }
    }
    Ok(ModuleItemSplit {
        course_id,
        module_id,
        column_fields,
        extra,
    })
}

fn apply_module_item_columns(
    tx: &Transaction<'_>,
    id: i64,
    column_fields: &[FieldWrite],
    applied: &[&str],
) -> Result<(), IngestError> {
    for field in column_fields {
        if !applied.contains(&field.name) {
            continue;
        }
        match field.name {
            "title" | "type" => {
                let col = if field.name == "type" {
                    "\"type\""
                } else {
                    field.name
                };
                tx.execute(
                    &format!("UPDATE module_items SET {col} = ?1 WHERE id = ?2"),
                    params![field.value, id],
                )?;
            }
            "position" | "content_id" => {
                let v: Option<i64> = field
                    .value
                    .as_ref()
                    .map(|s| s.parse())
                    .transpose()
                    .map_err(|e: std::num::ParseIntError| {
                        DbError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
                    })?;
                tx.execute(
                    &format!("UPDATE module_items SET {} = ?1 WHERE id = ?2", field.name),
                    params![v, id],
                )?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn upsert_module_item(
    tx: &Transaction<'_>,
    entity: &EntityIngest,
    fetched_at: Timestamp,
) -> Result<(), IngestError> {
    let id = parse_entity_id("module_item", &entity.entity_key)?;
    let split = split_module_item_fields(&entity.fields)?;
    validate_fields(
        &split.column_fields,
        &[
            "course_id",
            "module_id",
            "title",
            "position",
            "content_id",
            "type",
        ],
    )?;
    tx.execute(
        "INSERT INTO module_items (id, module_id, course_id) VALUES (?1, ?2, ?3)
         ON CONFLICT(id) DO NOTHING",
        params![id, split.module_id, split.course_id],
    )?;
    let applied = apply_field_writes(
        tx,
        "module_item",
        &entity.entity_key,
        fetched_at,
        &entity.fields,
    )?;
    if applied.fields.contains(&"course_id") {
        tx.execute(
            "UPDATE module_items SET course_id = ?1 WHERE id = ?2",
            params![split.course_id, id],
        )?;
    }
    if applied.fields.contains(&"module_id") {
        tx.execute(
            "UPDATE module_items SET module_id = ?1 WHERE id = ?2",
            params![split.module_id, id],
        )?;
    }
    apply_module_item_columns(tx, id, &split.column_fields, &applied.fields)?;
    let extra = split
        .extra
        .into_iter()
        .filter(|(name, _)| applied.fields.contains(&name.as_str()))
        .collect();
    merge_extra(tx, "module_items", id, extra)?;
    touch_observed(
        tx,
        "module_items",
        id,
        fetched_at,
        applied.core,
        applied.detail,
        applied.status,
    )?;
    Ok(())
}
