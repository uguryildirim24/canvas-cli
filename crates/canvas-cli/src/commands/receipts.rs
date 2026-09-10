//! `canvas receipts` (class B).

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use canvas_core::journal::State;
use canvas_core::receipts::{ListFilter, acknowledge, export, list_journals, show};
use serde::Serialize;

use super::Globals;
use super::emit::{base_envelope, emit_error, session_error};
use super::handled::Handled;
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

/// Run `canvas receipts` for the CLI: one envelope, one exit code.
pub fn run(globals: &Globals, cmd: ReceiptsCmd) -> ExitCode {
    handle(globals, cmd).emit(globals.json)
}

/// Dispatch receipts commands.
pub fn handle(globals: &Globals, cmd: ReceiptsCmd) -> Handled {
    match cmd {
        ReceiptsCmd::List { course, state } => list(globals, course.as_deref(), state.as_deref()),
        ReceiptsCmd::Show { id } => show_cmd(globals, &id),
        ReceiptsCmd::Export { receipt_id, out } => export_cmd(globals, &receipt_id, out.as_deref()),
        ReceiptsCmd::Acknowledge { journal_id } => ack_cmd(globals, &journal_id),
    }
}

fn list(globals: &Globals, course: Option<&str>, state: Option<&str>) -> Handled {
    let session = match globals.open_local_session() {
        Ok(s) => s,
        Err(e) => return session_error(e, globals.profile.clone()),
    };
    let course_id = if let Some(course) = course {
        let course = course.to_owned();
        let origin = session.identity.origin.clone();
        let resolved = session.open.store.call_blocking(move |conns| {
            use canvas_core::resolve::{CommandClass, ResolveError, resolve_course};
            match resolve_course(conns, &course, &origin, CommandClass::B) {
                Ok(c) => Ok(Ok(c.id)),
                Err(ResolveError::Db(e)) => Err(e),
                Err(e) => Ok(Err(e)),
            }
        });
        match resolved {
            Ok(Ok(id)) => Some(id),
            Ok(Err(e)) => return super::emit::resolve_error(&session, &e),
            Err(e) => {
                return emit_error(
                    "local",
                    &e.to_string(),
                    13,
                    session.profile.clone(),
                    Some(session.identity_ref()),
                );
            }
        }
    } else {
        None
    };
    let state = match state {
        Some(s) => match s.parse::<State>() {
            Ok(st) => Some(st),
            Err(()) => {
                return emit_error(
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
    Handled::new(env, move |env| {
        for j in &env.result.journals {
            if let Some(id) = j.get("journal_id").and_then(|v| v.as_str()) {
                let state = j.get("state").and_then(|v| v.as_str()).unwrap_or("?");
                let label = if state == "outcome_unknown" {
                    j["server_match"]["attempt"].as_i64().map_or_else(
                        || j["error"].as_str().unwrap_or("unknown").to_owned(),
                        |a| format!("unknown (server match: attempt {a})"),
                    )
                } else {
                    state.to_owned()
                };
                // The kind column says which machine wrote the journal: a
                // submission kind, or one of the three M8-b operations.
                writeln!(
                    io::stdout(),
                    "{id}  {}  {label}  owner={}  superseded={}  acknowledged={}",
                    j["kind"].as_str().unwrap_or("?"),
                    j["owner"].as_str().unwrap_or("n/a"),
                    j["superseded"],
                    !j["acknowledged_at"].is_null()
                )?;
            }
        }
        Ok(())
    })
}

fn show_cmd(globals: &Globals, id: &str) -> Handled {
    let session = match globals.open_local_session() {
        Ok(s) => s,
        Err(e) => return session_error(e, globals.profile.clone()),
    };
    let shown = match show(&session.open.store, &session.paths.identity_dir, id) {
        Ok(s) => s,
        Err(e) => {
            return emit_error(
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
    Handled::new(env, move |env| {
        let mut table = crate::output::new_table();
        for field in [
            "journal_id",
            "state",
            "owner",
            "course_id",
            "assignment_id",
            "kind",
            "superseded",
            "acknowledged_at",
            "error",
        ] {
            table.add_row([
                field.to_owned(),
                env.result.journal[field]
                    .as_str()
                    .map_or_else(|| env.result.journal[field].to_string(), str::to_owned),
            ]);
        }
        crate::output::apply_two_space_padding(&mut table);
        writeln!(io::stdout(), "{table}")?;
        if let Some(receipt) = &env.result.receipt {
            writeln!(
                io::stdout(),
                "receipt {}  attempt {}  attribution={}",
                receipt["receipt_id"].as_str().unwrap_or(""),
                receipt["posted"]["attempt"],
                receipt["attribution"].as_str().unwrap_or("")
            )?;
        }
        Ok(())
    })
}

fn export_cmd(globals: &Globals, id: &str, out: Option<&std::path::Path>) -> Handled {
    let session = match globals.open_local_session() {
        Ok(s) => s,
        Err(e) => return session_error(e, globals.profile.clone()),
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
                "export",
                &e.to_string(),
                exit,
                session.profile.clone(),
                Some(session.identity_ref()),
            );
        }
    };
    // `--out -` streams the receipt with no envelope (§7).
    if let Some(body) = result.body {
        let payload = ExportJson {
            receipt_id: result.receipt_id,
            path: None,
            bytes: result.bytes,
        };
        let envelope = base_envelope(SCHEMA_RECEIPTS, &session, payload);
        return Handled::raw(envelope, body);
    }
    let payload = ExportJson {
        receipt_id: result.receipt_id,
        path: result.path.map(|p| p.display().to_string()),
        bytes: result.bytes,
    };
    let env = base_envelope(SCHEMA_RECEIPTS, &session, payload);
    Handled::new(env, move |env| {
        if let Some(path) = &env.result.path {
            writeln!(io::stdout(), "exported {} ({path})", env.result.receipt_id)?;
        }
        Ok(())
    })
}

fn ack_cmd(globals: &Globals, journal_id: &str) -> Handled {
    let session = match globals.open_local_session() {
        Ok(s) => s,
        Err(e) => return session_error(e, globals.profile.clone()),
    };
    let result = match acknowledge(&session.open.store, journal_id) {
        Ok(r) => r,
        Err(e) => {
            return emit_error(
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
    Handled::new(env, move |env| {
        writeln!(
            io::stdout(),
            "acknowledged {} at {}",
            env.result.journal_id,
            env.result.acknowledged_at
        )?;
        Ok(())
    })
}
