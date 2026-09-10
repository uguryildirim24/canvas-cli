//! `canvas cache` (class B).

use std::io::{self, Write};
use std::process::ExitCode;

use canvas_core::store::{cache_clear, cache_path, cache_stats, load_fetch_log};
use comfy_table::Row;

use super::Globals;
use super::course_load::u64_count;
use super::emit::{base_envelope, emit_error, session_error};
use super::handled::Handled;
use crate::output::{
    CacheClearResult, CachePathResult, CacheStatsResult, CacheTableJson, Freshness,
    FreshnessSource, SCHEMA_CACHE, apply_two_space_padding, new_table,
};
use crate::session::Session;

/// Cache subcommand.
pub enum CacheCmd {
    Stats,
    Clear,
    Path,
}

/// Run `canvas cache` for the CLI: one envelope, one exit code.
pub async fn run(globals: &Globals, command: CacheCmd) -> ExitCode {
    handle(globals, command).await.emit(globals.json)
}

/// Run `canvas cache …`.
pub async fn handle(globals: &Globals, command: CacheCmd) -> Handled {
    let session = match globals.open_local_session() {
        Ok(s) => s,
        Err(e) => return session_error(e, globals.profile.clone()),
    };

    match command {
        CacheCmd::Stats => stats(&session).await,
        CacheCmd::Clear => clear(&session).await,
        CacheCmd::Path => path_cmd(&session),
    }
}

async fn stats(session: &Session) -> Handled {
    let path = cache_path(&session.paths.cache_db);
    let size_bytes = std::fs::metadata(&path).map_or(0, |m| m.len());

    let result = match session
        .open
        .store
        .call(move |conns| {
            let stats = cache_stats(&conns.cache)?;
            let tables = stats
                .tables
                .into_iter()
                .map(|(name, rows)| CacheTableJson {
                    name,
                    rows: u64_count(rows).unwrap_or(0),
                })
                .collect();

            let mut stmt = conns
                .cache
                .prepare("SELECT dataset, scope FROM fetch_log ORDER BY dataset ASC, scope ASC")?;
            let keys: Vec<(String, String)> = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            let mut datasets = Vec::new();
            for (dataset, scope) in keys {
                if let Some(row) = load_fetch_log(&conns.cache, &dataset, &scope)? {
                    datasets.push(Freshness {
                        dataset: row.dataset,
                        scope: row.scope,
                        source: FreshnessSource::Cache,
                        fetched_at: Some(row.fetched_at.to_string()),
                        complete: row.complete,
                        count: u64_count(row.count),
                        stale: row.stale,
                    });
                }
            }
            Ok(CacheStatsResult {
                path: path.display().to_string(),
                size_bytes,
                tables,
                datasets,
            })
        })
        .await
    {
        Ok(r) => r,
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

    let envelope = base_envelope(SCHEMA_CACHE, session, result);
    Handled::new(envelope, move |envelope| {
        writeln!(
            io::stdout(),
            "path: {}\nsize_bytes: {}",
            envelope.result.path,
            envelope.result.size_bytes
        )?;
        let mut table = new_table();
        table.set_header(Row::from(vec!["TABLE", "ROWS"]));
        for t in &envelope.result.tables {
            table.add_row(Row::from(vec![t.name.clone(), t.rows.to_string()]));
        }
        apply_two_space_padding(&mut table);
        writeln!(io::stdout(), "{table}")
    })
}

async fn clear(session: &Session) -> Handled {
    let result = match session
        .open
        .store
        .call(|conns| {
            let before = cache_stats(&conns.cache)?;
            cache_clear(&mut conns.cache)?;
            Ok(CacheClearResult {
                cleared: true,
                rows_deleted: u64_count(before.total_rows).unwrap_or(0),
            })
        })
        .await
    {
        Ok(r) => r,
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

    let envelope = base_envelope(SCHEMA_CACHE, session, result);
    Handled::new(envelope, move |envelope| {
        writeln!(
            io::stdout(),
            "cleared: true\nrows_deleted: {}",
            envelope.result.rows_deleted
        )
    })
}

fn path_cmd(session: &Session) -> Handled {
    let path = cache_path(&session.paths.cache_db);
    let result = CachePathResult {
        path: path.display().to_string(),
    };
    let envelope = base_envelope(SCHEMA_CACHE, session, result);
    Handled::new(envelope, move |envelope| {
        writeln!(io::stdout(), "{}", envelope.result.path)
    })
}
