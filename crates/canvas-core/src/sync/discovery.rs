//! Discovery view: store rows → download planner [`PlanInput`].

use rusqlite::params;

use crate::download::{PlanFile, PlanFolder, PlanInput, PlanModule, PlanModuleItem};
use crate::store::{DbError, StoreConns};

/// Load folders, files, modules, and module items for the download planner.
pub fn discovery_plan_input(
    conns: &StoreConns,
    course_id: i64,
    course_code: &str,
) -> Result<PlanInput, DbError> {
    let scope = format!("course:{course_id}");
    let folders = load_folders(conns, &scope)?;
    let files = load_files(conns, &scope)?;
    let modules = load_modules(conns, &scope, course_id)?;
    Ok(PlanInput {
        course_code: course_code.to_owned(),
        course_id,
        modules,
        folders,
        files,
    })
}

fn load_folders(conns: &StoreConns, scope: &str) -> Result<Vec<PlanFolder>, DbError> {
    let mut stmt = conns.cache.prepare(
        "SELECT f.id, f.name, f.full_name, f.parent_folder_id
         FROM membership m
         JOIN folders f ON f.id = CAST(m.entity_id AS INTEGER)
         WHERE m.dataset = 'folders' AND m.scope = ?1 AND m.entity_kind = 'folder'
         ORDER BY m.position, f.id",
    )?;
    let rows = stmt.query_map(params![scope], |r| {
        let id: i64 = r.get(0)?;
        let name: Option<String> = r.get(1)?;
        let full_name: Option<String> = r.get(2)?;
        let parent_folder_id: Option<i64> = r.get(3)?;
        Ok(PlanFolder {
            id,
            name: name.unwrap_or_default(),
            parent_folder_id,
            is_root: is_canvas_root(parent_folder_id, full_name.as_deref()),
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
}

fn load_files(conns: &StoreConns, scope: &str) -> Result<Vec<PlanFile>, DbError> {
    let mut stmt = conns.cache.prepare(
        "SELECT f.id, f.display_name, f.folder_id, f.size, f.data_json
         FROM membership m
         JOIN files f ON f.id = CAST(m.entity_id AS INTEGER)
         WHERE m.dataset = 'files' AND m.scope = ?1 AND m.entity_kind = 'file'
         ORDER BY m.position, f.id",
    )?;
    let rows = stmt.query_map(params![scope], |r| {
        let id: i64 = r.get(0)?;
        let display_name: Option<String> = r.get(1)?;
        let folder_id: Option<i64> = r.get(2)?;
        let size: Option<i64> = r.get(3)?;
        let data_json: String = r.get(4)?;
        let updated_at = data_json_string(&data_json, "updated_at");
        Ok(PlanFile {
            id,
            display_name: display_name.unwrap_or_default(),
            folder_id: folder_id.unwrap_or(0),
            size: size.and_then(|s| u64::try_from(s).ok()),
            updated_at,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
}

fn load_modules(
    conns: &StoreConns,
    scope: &str,
    course_id: i64,
) -> Result<Vec<PlanModule>, DbError> {
    let mut stmt = conns.cache.prepare(
        "SELECT mo.id, mo.name, mo.position
         FROM membership m
         JOIN modules mo ON mo.id = CAST(m.entity_id AS INTEGER)
         WHERE m.dataset = 'modules' AND m.scope = ?1 AND m.entity_kind = 'module'
         ORDER BY m.position, mo.id",
    )?;
    let modules = stmt
        .query_map(params![scope], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<i64>>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut out = Vec::with_capacity(modules.len());
    for (id, name, position) in modules {
        let items = load_module_items(conns, course_id, id)?;
        out.push(PlanModule {
            id,
            name: name.unwrap_or_default(),
            position: position.unwrap_or(0),
            items,
        });
    }
    Ok(out)
}

fn load_module_items(
    conns: &StoreConns,
    course_id: i64,
    module_id: i64,
) -> Result<Vec<PlanModuleItem>, DbError> {
    let mut stmt = conns.cache.prepare(
        "SELECT id, title, position, content_id, \"type\"
         FROM module_items
         WHERE course_id = ?1 AND module_id = ?2
         ORDER BY position, id",
    )?;
    let rows = stmt.query_map(params![course_id, module_id], |r| {
        Ok(PlanModuleItem {
            id: r.get(0)?,
            title: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
            position: r.get::<_, Option<i64>>(2)?.unwrap_or(0),
            content_id: r.get(3)?,
            item_type: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
}

/// Canvas course-files root: no parent, or `full_name` like `"course files"`.
#[must_use]
pub fn is_canvas_root(parent_folder_id: Option<i64>, full_name: Option<&str>) -> bool {
    if parent_folder_id.is_none() {
        return true;
    }
    full_name.is_some_and(|n| n.eq_ignore_ascii_case("course files"))
}

fn data_json_string(raw: &str, key: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    match value.get(key)? {
        serde_json::Value::Null => None,
        serde_json::Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}
