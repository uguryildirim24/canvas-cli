//! `canvas files` (class C, M3-a).

use std::collections::{HashMap, HashSet};
use std::io::{self, Write};
use std::process::ExitCode;

use canvas_core::store::{DbError, StoreConns, lookup_dataset};
use canvas_core::sync::{
    FilesDataset, FoldersDataset, ModulesDataset, RefreshOutcome, is_canvas_root,
    listing_denial_status, refresh_files, refresh_folders, refresh_modules,
};
use comfy_table::Row;
use rusqlite::{OptionalExtension, params};
use serde_json::Value;

use super::Globals;
use super::course::{refresh_fail, resolve_with_refresh};
use super::course_load::{RefreshFail, cached_outcome_with_error, outcome_freshness};
use super::emit::{base_envelope, emit, emit_error, session_error};
use crate::output::{
    FileEntryJson, FilesListingJson, FilesResult, Outcome, PartialScope, SCHEMA_FILES,
    apply_two_space_padding, new_table, now_timestamp,
};
use crate::session::{Session, ttl_files, ttl_modules};

/// Run `canvas files <course> [--tree] [--search TEXT]`.
pub async fn run(
    globals: &Globals,
    course: String,
    tree: bool,
    search: Option<String>,
) -> ExitCode {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(globals.json, e, globals.profile.clone()),
    };

    let (resolved, mut freshness, _) = match resolve_with_refresh(globals, &session, &course).await
    {
        Ok(v) => v,
        Err(code) => return code,
    };

    let folders_out = match ensure_folders(globals, &session, resolved.id).await {
        Ok(o) => o,
        Err(e) => return refresh_fail(globals, &session, e),
    };
    freshness.push(outcome_freshness(&folders_out));

    let files_out = match ensure_files(globals, &session, resolved.id).await {
        Ok(o) => o,
        Err(e) => return refresh_fail(globals, &session, e),
    };
    freshness.push(outcome_freshness(&files_out));

    let modules_out = match ensure_modules(globals, &session, resolved.id).await {
        Ok(o) => o,
        Err(e) => return refresh_fail(globals, &session, e),
    };
    freshness.push(outcome_freshness(&modules_out));

    let denial = files_out.error.as_deref().and_then(listing_denial_status);
    let listing = FilesListingJson {
        available: denial.is_none(),
        http_status: denial,
    };

    let mut files = match session
        .open
        .store
        .call(move |conns| load_merged_files(conns, resolved.id))
        .await
    {
        Ok(rows) => rows,
        Err(e) => {
            return emit_error(
                globals.json,
                "local",
                &e.to_string(),
                13,
                session.profile.clone(),
                Some(session.identity_ref()),
            );
        }
    };

    if let Some(needle) = search.as_deref() {
        let needle = needle.to_lowercase();
        files.retain(|f| f.name.to_lowercase().contains(&needle));
    }
    sort_files(&mut files);

    let result = FilesResult {
        course_id: resolved.id.to_string(),
        listing,
        files,
    };
    let mut envelope = base_envelope(SCHEMA_FILES, &session, result);
    envelope.freshness = freshness;
    envelope.requests = session.requests();
    for outcome in [&folders_out, &files_out, &modules_out] {
        if outcome.freshness.stale {
            envelope
                .warnings
                .push(format!("served stale {} cache", outcome.freshness.dataset));
        }
    }
    for outcome in [&folders_out, &files_out] {
        if let Some(status) = outcome.error.as_deref().and_then(listing_denial_status) {
            let message = if outcome.freshness.dataset == "files" {
                format!(
                    "Files listing unavailable (HTTP {status}); showing files linked from modules"
                )
            } else {
                format!("Folders listing unavailable (HTTP {status}); folder paths unavailable")
            };
            envelope.partial.push(PartialScope {
                scope: format!("{}:course:{}", outcome.freshness.dataset, resolved.id),
                http_status: Some(status),
                message: message.clone(),
            });
            envelope.warnings.push(message);
            envelope.outcome = Outcome::Partial;
            envelope.exit = 12;
        }
    }

    emit(globals.json, &envelope, || {
        if tree {
            print_tree(&envelope.result.files)
        } else {
            print_table(&envelope.result.files)
        }
    })
}

pub(crate) async fn ensure_folders(
    globals: &Globals,
    session: &Session,
    course_id: i64,
) -> Result<RefreshOutcome, RefreshFail> {
    let now = now_timestamp();
    let ttl = ttl_files();
    let ds = FoldersDataset::new(course_id, ttl);
    let lookup = session
        .open
        .store
        .call(move |conns| lookup_dataset(conns, &ds, now, None))
        .await
        .map_err(RefreshFail::Db)?;
    if let Some(outcome) = cached_outcome_with_error(lookup, globals.fresh, globals.offline)? {
        return Ok(outcome);
    }
    session
        .validate_network_token()
        .await
        .map_err(RefreshFail::Sync)?;
    let client = session.client.as_ref().ok_or(RefreshFail::NeedAuth)?;
    refresh_folders(
        client,
        &session.open.store,
        course_id,
        ttl,
        now,
        globals.fresh,
        false,
    )
    .await
    .map_err(RefreshFail::Sync)
}

pub(crate) async fn ensure_files(
    globals: &Globals,
    session: &Session,
    course_id: i64,
) -> Result<RefreshOutcome, RefreshFail> {
    let now = now_timestamp();
    let ttl = ttl_files();
    let ds = FilesDataset::new(course_id, ttl);
    let lookup = session
        .open
        .store
        .call(move |conns| lookup_dataset(conns, &ds, now, None))
        .await
        .map_err(RefreshFail::Db)?;
    if let Some(outcome) = cached_outcome_with_error(lookup, globals.fresh, globals.offline)? {
        return Ok(outcome);
    }
    session
        .validate_network_token()
        .await
        .map_err(RefreshFail::Sync)?;
    let client = session.client.as_ref().ok_or(RefreshFail::NeedAuth)?;
    refresh_files(
        client,
        &session.open.store,
        course_id,
        ttl,
        now,
        globals.fresh,
        false,
    )
    .await
    .map_err(RefreshFail::Sync)
}

pub(crate) async fn ensure_modules(
    globals: &Globals,
    session: &Session,
    course_id: i64,
) -> Result<RefreshOutcome, RefreshFail> {
    let now = now_timestamp();
    let ttl = ttl_modules();
    let ds = ModulesDataset::new(course_id, ttl);
    let lookup = session
        .open
        .store
        .call(move |conns| lookup_dataset(conns, &ds, now, None))
        .await
        .map_err(RefreshFail::Db)?;
    if let Some(outcome) =
        super::course_load::cached_outcome(lookup, globals.fresh, globals.offline)?
    {
        return Ok(outcome);
    }
    session
        .validate_network_token()
        .await
        .map_err(RefreshFail::Sync)?;
    let client = session.client.as_ref().ok_or(RefreshFail::NeedAuth)?;
    refresh_modules(
        client,
        &session.open.store,
        course_id,
        ttl,
        now,
        globals.fresh,
        false,
    )
    .await
    .map_err(RefreshFail::Sync)
}

fn load_merged_files(conns: &StoreConns, course_id: i64) -> Result<Vec<FileEntryJson>, DbError> {
    let scope = format!("course:{course_id}");
    let folder_paths = load_folder_paths(conns, &scope)?;
    let mut listing_ids = HashSet::new();
    let mut out = load_listing_files(conns, &scope, &folder_paths, &mut listing_ids)?;
    append_module_files(conns, course_id, &scope, &mut listing_ids, &mut out)?;
    Ok(out)
}

fn load_listing_files(
    conns: &StoreConns,
    scope: &str,
    folder_paths: &HashMap<i64, String>,
    listing_ids: &mut HashSet<i64>,
) -> Result<Vec<FileEntryJson>, DbError> {
    let mut stmt = conns.cache.prepare(
        "SELECT f.id, f.display_name, f.folder_id, f.size, f.data_json
         FROM membership m
         JOIN files f ON f.id = CAST(m.entity_id AS INTEGER)
         WHERE m.dataset = 'files' AND m.scope = ?1 AND m.entity_kind = 'file'
         ORDER BY m.position, f.id",
    )?;
    let mut out = Vec::new();
    let rows = stmt.query_map(params![scope], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<i64>>(2)?,
            r.get::<_, Option<i64>>(3)?,
            r.get::<_, String>(4)?,
        ))
    })?;
    for row in rows {
        let (id, name, folder_id, size, data_raw) = row?;
        listing_ids.insert(id);
        let data = parse_object(&data_raw);
        out.push(FileEntryJson {
            id: id.to_string(),
            source: "listing".into(),
            folder_id: folder_id.map(|f| f.to_string()),
            folder_path: folder_id.and_then(|fid| folder_paths.get(&fid).cloned()),
            module_id: None,
            module_position: None,
            name: name.unwrap_or_default(),
            size: size.and_then(|s| u64::try_from(s).ok()),
            updated_at: json_string(&data, "updated_at"),
            hidden: json_bool(&data, "hidden"),
            locked: json_bool(&data, "locked_for_user"),
            lock_explanation: json_string(&data, "lock_explanation"),
        });
    }
    Ok(out)
}

fn append_module_files(
    conns: &StoreConns,
    course_id: i64,
    scope: &str,
    listing_ids: &mut HashSet<i64>,
    out: &mut Vec<FileEntryJson>,
) -> Result<(), DbError> {
    let mut mod_stmt = conns.cache.prepare(
        "SELECT mo.id, mo.name, mo.position
         FROM membership m
         JOIN modules mo ON mo.id = CAST(m.entity_id AS INTEGER)
         WHERE m.dataset = 'modules' AND m.scope = ?1 AND m.entity_kind = 'module'
         ORDER BY mo.position, mo.id",
    )?;
    let modules = mod_stmt
        .query_map(params![scope], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<i64>>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    for (module_id, module_name, module_position) in modules {
        push_module_file_items(
            conns,
            course_id,
            module_id,
            module_name.as_deref(),
            module_position,
            listing_ids,
            out,
        )?;
    }
    Ok(())
}

fn push_module_file_items(
    conns: &StoreConns,
    course_id: i64,
    module_id: i64,
    module_name: Option<&str>,
    module_position: Option<i64>,
    listing_ids: &mut HashSet<i64>,
    out: &mut Vec<FileEntryJson>,
) -> Result<(), DbError> {
    let mut item_stmt = conns.cache.prepare(
        "SELECT id, title, content_id, \"type\", data_json
         FROM module_items
         WHERE course_id = ?1 AND module_id = ?2
           AND id IN (SELECT CAST(entity_id AS INTEGER) FROM membership
                      WHERE dataset = 'module_items' AND entity_kind = 'module_item'
                        AND scope = 'course:' || ?1 || ':module:' || ?2)
         ORDER BY position, id",
    )?;
    let items = item_stmt.query_map(params![course_id, module_id], |r| {
        Ok((
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<i64>>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, String>(4)?,
        ))
    })?;
    for item in items {
        let (title, content_id, item_type, data_raw) = item?;
        if !item_type
            .as_deref()
            .is_some_and(|t| t.eq_ignore_ascii_case("File"))
        {
            continue;
        }
        let Some(file_id) = content_id else {
            continue;
        };
        if listing_ids.contains(&file_id) {
            continue;
        }
        let data = parse_object(&data_raw);
        let meta = file_row_meta(conns, file_id)?;
        let name = meta.name.unwrap_or_else(|| title.unwrap_or_default());
        let folder_path =
            Some(module_name.map_or_else(|| format!("module-{module_id}"), str::to_owned));
        out.push(FileEntryJson {
            id: file_id.to_string(),
            source: "module".into(),
            folder_id: None,
            folder_path,
            module_id: Some(module_id.to_string()),
            module_position,
            name,
            size: meta.size,
            updated_at: meta.updated_at,
            hidden: meta.hidden,
            locked: json_bool(&data, "locked_for_user"),
            lock_explanation: json_string(&data, "lock_explanation"),
        });
        listing_ids.insert(file_id);
    }
    Ok(())
}

struct FileMeta {
    name: Option<String>,
    size: Option<u64>,
    updated_at: Option<String>,
    hidden: Option<bool>,
}

fn file_row_meta(conns: &StoreConns, file_id: i64) -> Result<FileMeta, DbError> {
    let row: Option<(Option<String>, Option<i64>, String)> = conns
        .cache
        .query_row(
            "SELECT display_name, size, data_json FROM files WHERE id = ?1",
            [file_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((name, size, data_raw)) = row else {
        return Ok(FileMeta {
            name: None,
            size: None,
            updated_at: None,
            hidden: None,
        });
    };
    let data = parse_object(&data_raw);
    Ok(FileMeta {
        name,
        size: size.and_then(|s| u64::try_from(s).ok()),
        updated_at: json_string(&data, "updated_at"),
        hidden: json_bool(&data, "hidden"),
    })
}

struct FolderRow {
    id: i64,
    name: String,
    full_name: Option<String>,
    parent_folder_id: Option<i64>,
}

fn load_folder_paths(conns: &StoreConns, scope: &str) -> Result<HashMap<i64, String>, DbError> {
    let mut stmt = conns.cache.prepare(
        "SELECT f.id, f.name, f.full_name, f.parent_folder_id
         FROM membership m
         JOIN folders f ON f.id = CAST(m.entity_id AS INTEGER)
         WHERE m.dataset = 'folders' AND m.scope = ?1 AND m.entity_kind = 'folder'",
    )?;
    let folders: Vec<FolderRow> = stmt
        .query_map(params![scope], |r| {
            Ok(FolderRow {
                id: r.get(0)?,
                name: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                full_name: r.get(2)?,
                parent_folder_id: r.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let by_id: HashMap<i64, &FolderRow> = folders.iter().map(|f| (f.id, f)).collect();
    let mut paths = HashMap::new();
    for folder in &folders {
        let mut seen = HashSet::new();
        let mut components = Vec::new();
        let mut current = Some(folder.id);
        while let Some(id) = current {
            if !seen.insert(id) {
                break;
            }
            let Some(f) = by_id.get(&id) else {
                break;
            };
            if is_canvas_root(f.parent_folder_id, f.full_name.as_deref()) {
                break;
            }
            components.push(f.name.clone());
            current = f.parent_folder_id;
        }
        components.reverse();
        paths.insert(folder.id, components.join("/"));
    }
    Ok(paths)
}

fn sort_files(files: &mut [FileEntryJson]) {
    files.sort_by(|a, b| {
        path_key(a)
            .cmp(path_key(b))
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| {
                a.id.parse::<i64>()
                    .unwrap_or_default()
                    .cmp(&b.id.parse::<i64>().unwrap_or_default())
            })
    });
}

fn path_key(f: &FileEntryJson) -> &str {
    f.folder_path.as_deref().unwrap_or("")
}

fn print_table(files: &[FileEntryJson]) -> io::Result<()> {
    let mut table = new_table();
    table.set_header(Row::from(vec!["ID", "SOURCE", "PATH", "NAME", "SIZE"]));
    for f in files {
        let path = f.folder_path.clone().unwrap_or_default();
        let size = f.size.map_or_else(|| "—".into(), |s| s.to_string());
        table.add_row(Row::from(vec![
            f.id.clone(),
            f.source.clone(),
            path,
            f.name.clone(),
            size,
        ]));
    }
    apply_two_space_padding(&mut table);
    writeln!(io::stdout(), "{table}")
}

fn print_tree(files: &[FileEntryJson]) -> io::Result<()> {
    let mut previous: Vec<&str> = Vec::new();
    for f in files {
        let components: Vec<_> = f
            .folder_path
            .as_deref()
            .unwrap_or("")
            .split('/')
            .filter(|part| !part.is_empty())
            .collect();
        let common = previous
            .iter()
            .zip(&components)
            .take_while(|(a, b)| a == b)
            .count();
        for (depth, component) in components.iter().enumerate().skip(common) {
            writeln!(io::stdout(), "{}{component}/", "  ".repeat(depth))?;
        }
        let size = f.size.map(|s| format!(" ({s})")).unwrap_or_default();
        writeln!(
            io::stdout(),
            "{}{}{size}",
            "  ".repeat(components.len()),
            f.name
        )?;
        previous = components;
    }
    Ok(())
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
