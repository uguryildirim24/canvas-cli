//! `canvas modules` (class C, M3-a).

use std::io::{self, Write};
use std::process::ExitCode;

use canvas_core::store::{DbError, StoreConns};
use comfy_table::Row;
use rusqlite::params;
use serde_json::Value;

use super::Globals;
use super::course::{refresh_fail, resolve_with_refresh};
use super::course_load::outcome_freshness;
use super::emit::{base_envelope, emit_error, session_error};
use super::files::ensure_modules;
use super::handled::Handled;
use crate::output::{
    ModuleEntryJson, ModuleItemJson, ModulesResult, SCHEMA_MODULES, apply_two_space_padding,
    new_table,
};

/// Run `canvas modules` for the CLI: one envelope, one exit code.
pub async fn run(globals: &Globals, course: String, items: bool) -> ExitCode {
    handle(globals, course, items).await.emit(globals.json)
}

/// Run `canvas modules <course> [--items]`.
pub async fn handle(globals: &Globals, course: String, items: bool) -> Handled {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(e, globals.profile.clone()),
    };

    let (resolved, mut freshness, _) = match resolve_with_refresh(globals, &session, &course).await
    {
        Ok(v) => v,
        Err(code) => return code,
    };

    let outcome = match ensure_modules(globals, &session, resolved.id).await {
        Ok(o) => o,
        Err(e) => return refresh_fail(&session, e),
    };
    freshness.push(outcome_freshness(&outcome));

    let mut modules = match session
        .open
        .store
        .call(move |conns| load_modules(conns, resolved.id, items))
        .await
    {
        Ok(rows) => rows,
        Err(e) => {
            return emit_error(
                "local",
                &e.to_string(),
                13,
                session.profile.clone(),
                Some(session.identity_ref()),
            );
        }
    };
    modules.sort_by_key(|m| (m.position, m.id.parse::<i64>().unwrap_or_default()));

    let result = ModulesResult {
        course_id: resolved.id.to_string(),
        modules,
    };
    let mut envelope = base_envelope(SCHEMA_MODULES, &session, result);
    envelope.freshness = freshness;
    envelope.requests = session.requests();
    if outcome.freshness.stale {
        envelope.warnings.push("served stale modules cache".into());
    }

    Handled::new(envelope, move |envelope| {
        print_table(&envelope.result, items)
    })
}

fn load_modules(
    conns: &StoreConns,
    course_id: i64,
    include_items: bool,
) -> Result<Vec<ModuleEntryJson>, DbError> {
    let scope = format!("course:{course_id}");
    let mut stmt = conns.cache.prepare(
        "SELECT mo.id, mo.name, mo.position, mo.items_count, mo.items_complete, mo.data_json
         FROM membership m
         JOIN modules mo ON mo.id = CAST(m.entity_id AS INTEGER)
         WHERE m.dataset = 'modules' AND m.scope = ?1 AND m.entity_kind = 'module'
         ORDER BY mo.position, mo.id",
    )?;
    let rows = stmt.query_map(params![scope], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<i64>>(2)?,
            r.get::<_, Option<i64>>(3)?,
            r.get::<_, Option<i64>>(4)?,
            r.get::<_, String>(5)?,
        ))
    })?;

    let mut out = Vec::new();
    for row in rows {
        let (id, name, position, items_count, items_complete, data_raw) = row?;
        let data = parse_object(&data_raw);
        let items = if include_items {
            load_items(conns, course_id, id)?
        } else {
            Vec::new()
        };
        out.push(ModuleEntryJson {
            id: id.to_string(),
            name: name.unwrap_or_default(),
            position: position.unwrap_or(0),
            state: json_string(&data, "state"),
            items_count: items_count.and_then(|n| u64::try_from(n).ok()),
            items_complete: items_complete.is_some_and(|v| v != 0),
            items,
        });
    }
    Ok(out)
}

fn load_items(
    conns: &StoreConns,
    course_id: i64,
    module_id: i64,
) -> Result<Vec<ModuleItemJson>, DbError> {
    let mut stmt = conns.cache.prepare(
        "SELECT id, title, position, content_id, \"type\", data_json
         FROM module_items
         WHERE course_id = ?1 AND module_id = ?2
           AND id IN (SELECT CAST(entity_id AS INTEGER) FROM membership
                      WHERE dataset = 'module_items' AND entity_kind = 'module_item'
                        AND scope = 'course:' || ?1 || ':module:' || ?2)
         ORDER BY position, id",
    )?;
    let rows = stmt.query_map(params![course_id, module_id], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<i64>>(2)?,
            r.get::<_, Option<i64>>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, String>(5)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, title, position, content_id, type_, data_raw) = row?;
        let data = parse_object(&data_raw);
        out.push(ModuleItemJson {
            id: id.to_string(),
            type_: type_.unwrap_or_default(),
            content_id: content_id.map(|c| c.to_string()),
            title: title.unwrap_or_default(),
            position: position.unwrap_or(0),
            locked: json_bool(&data, "locked_for_user"),
            lock_explanation: json_string(&data, "lock_explanation"),
            completed: json_bool(&data, "completed"),
            html_url: json_string(&data, "html_url"),
        });
    }
    Ok(out)
}

fn print_table(result: &ModulesResult, show_items: bool) -> io::Result<()> {
    let mut table = new_table();
    if show_items {
        table.set_header(Row::from(vec![
            "ID", "NAME", "POS", "STATE", "ITEMS", "COMPLETE",
        ]));
    } else {
        table.set_header(Row::from(vec!["ID", "NAME", "POS", "STATE", "ITEMS"]));
    }
    for m in &result.modules {
        let state = m.state.clone().unwrap_or_else(|| "—".into());
        let count = m.items_count.map_or_else(|| "—".into(), |n| n.to_string());
        if show_items {
            table.add_row(Row::from(vec![
                m.id.clone(),
                m.name.clone(),
                m.position.to_string(),
                state,
                count,
                if m.items_complete { "yes" } else { "no" }.into(),
            ]));
            for item in &m.items {
                table.add_row(Row::from(vec![
                    format!("  {}", item.id),
                    format!("  {}", item.title),
                    item.position.to_string(),
                    item.type_.clone(),
                    item.content_id.clone().unwrap_or_default(),
                    String::new(),
                ]));
            }
        } else {
            table.add_row(Row::from(vec![
                m.id.clone(),
                m.name.clone(),
                m.position.to_string(),
                state,
                count,
            ]));
        }
    }
    apply_two_space_padding(&mut table);
    writeln!(io::stdout(), "{table}")
}

fn parse_object(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|_| Value::Object(serde_json::Map::default()))
}

fn json_string(data: &Value, key: &str) -> Option<String> {
    match data.get(key)? {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}

fn json_bool(data: &Value, key: &str) -> Option<bool> {
    match data.get(key)? {
        Value::Bool(b) => Some(*b),
        Value::String(s) => match s.as_str() {
            "true" | "1" => Some(true),
            "false" | "0" => Some(false),
            _ => None,
        },
        Value::Number(n) => n.as_i64().map(|v| v != 0),
        _ => None,
    }
}
