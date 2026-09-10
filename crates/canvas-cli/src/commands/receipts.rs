//! `canvas receipts` (class B).

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use canvas_core::journal::State;
use canvas_core::receipts::{ListFilter, acknowledge, export, list_journals, show};
use serde::Serialize;

use super::Globals;
use super::emit::{base_envelope, emit, emit_error, session_error};
use crate::output::SCHEMA_RECEIPTS;

/// Receipts subcommand after clap parsing.
pub enum ReceiptsCmd {
    List {
        course: Option<String>,
        state: Option<String>,
    },
    Show {
        id: String,
    },
    Export {
        receipt_id: String,
        out: Option<PathBuf>,
    },
    Acknowledge {
        journal_id: String,
    },
}

#[derive(Debug, Serialize)]
struct ListResult {
    journals: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct ShowJson {
    journal: serde_json::Value,
    receipt: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct ExportJson {
    receipt_id: String,
    path: Option<String>,
    bytes: usize,
}

#[derive(Debug, Serialize)]
struct AckJson {
    journal_id: String,
    acknowledged_at: String,
}

/// Dispatch receipts commands.
pub fn run(globals: &Globals, cmd: ReceiptsCmd) -> ExitCode {
    match cmd {
        ReceiptsCmd::List { course, state } => list(globals, course.as_deref(), state.as_deref()),
        ReceiptsCmd::Show { id } => show_cmd(globals, &id),
        ReceiptsCmd::Export { receipt_id, out } => export_cmd(globals, &receipt_id, out.as_deref()),
        ReceiptsCmd::Acknowledge { journal_id } => ack_cmd(globals, &journal_id),
    }
}

fn list(globals: &Globals, course: Option<&str>, state: Option<&str>) -> ExitCode {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(globals.json, e, globals.profile.clone()),
    };
    let course_id = match course {
        Some(c) => match c.parse::<i64>() {
            Ok(id) => Some(id),
            Err(_) => {
                return emit_error(
                    globals.json,
                    "usage",
                    "use a numeric ID or a URL",
                    6,
                    session.profile.clone(),
                    Some(session.identity_ref()),
                );
            }
        },
        None => None,
    };
    let state = match state {
        Some(s) => match s.parse::<State>() {
            Ok(st) => Some(st),
            Err(()) => {
                return emit_error(
                    globals.json,
                    "usage",
                    &format!("unknown state {s}"),
                    2,
                    session.profile.clone(),
                    Some(session.identity_ref()),
                );
            }
        },
        None => None,
    };
    let journals = match list_journals(
        &session.open.store,
        &session.paths.identity_dir,
        &ListFilter { course_id, state },
    ) {
        Ok(j) => j,
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
    let payload = ListResult {
        journals: journals
            .iter()
            .filter_map(|j| serde_json::to_value(j).ok())
            .collect(),
    };
    let env = base_envelope(SCHEMA_RECEIPTS, &session, payload);
    emit(globals.json, &env, || {
        for j in &env.result.journals {
            if let Some(id) = j.get("journal_id").and_then(|v| v.as_str()) {
                let state = j.get("state").and_then(|v| v.as_str()).unwrap_or("?");
                writeln!(io::stdout(), "{id}  {state}")?;
            }
        }
        Ok(())
    })
}

fn show_cmd(globals: &Globals, id: &str) -> ExitCode {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(globals.json, e, globals.profile.clone()),
    };
    let shown = match show(&session.open.store, &session.paths.identity_dir, id) {
        Ok(s) => s,
        Err(e) => {
            return emit_error(
                globals.json,
                "not_found",
                &e.to_string(),
                8,
                session.profile.clone(),
                Some(session.identity_ref()),
            );
        }
    };
    let payload = ShowJson {
        journal: serde_json::to_value(&shown.journal).unwrap_or_default(),
        receipt: shown
            .receipt
            .as_ref()
            .and_then(|r| serde_json::to_value(r).ok()),
    };
    let env = base_envelope(SCHEMA_RECEIPTS, &session, payload);
    emit(globals.json, &env, || {
        writeln!(
            io::stdout(),
            "journal {}",
            env.result
                .journal
                .get("journal_id")
                .and_then(|v| v.as_str())
                .unwrap_or(id)
        )?;
        Ok(())
    })
}

fn export_cmd(globals: &Globals, id: &str, out: Option<&std::path::Path>) -> ExitCode {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(globals.json, e, globals.profile.clone()),
    };
    let result = match export(&session.open.store, &session.paths, id, out) {
        Ok(r) => r,
        Err(e) => {
            let exit = if e.to_string().contains("refused") {
                8
            } else {
                13
            };
            return emit_error(
                globals.json,
                "export",
                &e.to_string(),
                exit,
                session.profile.clone(),
                Some(session.identity_ref()),
            );
        }
    };
    if let Some(body) = result.body {
        let _ = io::stdout().write_all(&body);
        return ExitCode::SUCCESS;
    }
    let payload = ExportJson {
        receipt_id: result.receipt_id,
        path: result.path.map(|p| p.display().to_string()),
        bytes: result.bytes,
    };
    let env = base_envelope(SCHEMA_RECEIPTS, &session, payload);
    emit(globals.json, &env, || {
        if let Some(path) = &env.result.path {
            writeln!(io::stdout(), "exported {} ({path})", env.result.receipt_id)?;
        }
        Ok(())
    })
}

fn ack_cmd(globals: &Globals, journal_id: &str) -> ExitCode {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(globals.json, e, globals.profile.clone()),
    };
    let result = match acknowledge(&session.open.store, journal_id) {
        Ok(r) => r,
        Err(e) => {
            return emit_error(
                globals.json,
                "acknowledge",
                &e.to_string(),
                8,
                session.profile.clone(),
                Some(session.identity_ref()),
            );
        }
    };
    let payload = AckJson {
        journal_id: result.journal_id,
        acknowledged_at: result.acknowledged_at,
    };
    let env = base_envelope(SCHEMA_RECEIPTS, &session, payload);
    emit(globals.json, &env, || {
        writeln!(
            io::stdout(),
            "acknowledged {} at {}",
            env.result.journal_id,
            env.result.acknowledged_at
        )?;
        Ok(())
    })
}
