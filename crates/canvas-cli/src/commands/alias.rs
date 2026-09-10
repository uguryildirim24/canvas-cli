//! `canvas alias` (class B).

use std::io::{self, Write};
use std::process::ExitCode;

use canvas_core::resolve::{
    CommandClass, ResolveError, alias_list, alias_remove, alias_set, resolve_course,
};
use canvas_core::store::DbError;
use comfy_table::Row;

use super::Globals;
use super::emit::{base_envelope, emit, emit_error, session_error};
use crate::output::{AliasJson, AliasResult, SCHEMA_ALIAS, apply_two_space_padding, new_table};
use crate::session::Session;

/// Alias subcommand.
pub enum AliasCmd {
    Set { name: String, course: String },
    List,
    Remove { name: String },
}

/// Run `canvas alias …`.
pub async fn run(globals: &Globals, command: AliasCmd) -> ExitCode {
    let session = match globals.open_local_session() {
        Ok(s) => s,
        Err(e) => return session_error(globals.json, e, globals.profile.clone()),
    };

    match command {
        AliasCmd::Set { name, course } => set(globals, &session, name, course).await,
        AliasCmd::List => list(globals, &session).await,
        AliasCmd::Remove { name } => remove(globals, &session, name).await,
    }
}

async fn set(globals: &Globals, session: &Session, name: String, course: String) -> ExitCode {
    let origin = session.identity.origin.clone();
    let resolved = match session
        .open
        .store
        .call(
            move |conns| match resolve_course(conns, &course, &origin, CommandClass::B) {
                Ok(r) => Ok(Ok(r)),
                Err(ResolveError::Db(e)) => Err(e),
                Err(e) => Ok(Err(e)),
            },
        )
        .await
    {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => return super::emit::resolve_error(globals, session, &e),
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

    let course_id = resolved.id;
    match session
        .open
        .store
        .call(move |conns| {
            alias_set(&conns.state, &name, course_id).map_err(|e| match e {
                ResolveError::Db(db) => db,
                other => DbError::Message(other.to_string()),
            })?;
            load_alias_result(conns)
        })
        .await
    {
        Ok(result) => emit_alias(globals, session, result),
        Err(e) => emit_error(
            globals.json,
            "local",
            &e.to_string(),
            13,
            session.profile.clone(),
            Some(session.identity_ref()),
        ),
    }
}

async fn list(globals: &Globals, session: &Session) -> ExitCode {
    match session.open.store.call(load_alias_result).await {
        Ok(result) => emit_alias(globals, session, result),
        Err(e) => emit_error(
            globals.json,
            "local",
            &e.to_string(),
            13,
            session.profile.clone(),
            Some(session.identity_ref()),
        ),
    }
}

async fn remove(globals: &Globals, session: &Session, name: String) -> ExitCode {
    match session
        .open
        .store
        .call(move |conns| {
            alias_remove(&conns.state, &name).map_err(|e| match e {
                ResolveError::Db(db) => db,
                other => DbError::Message(other.to_string()),
            })?;
            load_alias_result(conns)
        })
        .await
    {
        Ok(result) => emit_alias(globals, session, result),
        Err(e) => emit_error(
            globals.json,
            "local",
            &e.to_string(),
            13,
            session.profile.clone(),
            Some(session.identity_ref()),
        ),
    }
}

fn load_alias_result(conns: &mut canvas_core::store::StoreConns) -> Result<AliasResult, DbError> {
    let rows = alias_list(&conns.state).map_err(|e| match e {
        ResolveError::Db(db) => db,
        other => DbError::Message(other.to_string()),
    })?;
    let mut aliases = Vec::with_capacity(rows.len());
    for row in rows {
        if row.target_kind != "course" {
            continue;
        }
        let course_id = row.target_id.clone();
        let course_code = course_id.parse::<i64>().ok().and_then(|id| {
            conns
                .cache
                .query_row("SELECT course_code FROM courses WHERE id = ?1", [id], |r| {
                    r.get::<_, Option<String>>(0)
                })
                .ok()
                .flatten()
        });
        aliases.push(AliasJson {
            name: row.name,
            course_id,
            course_code,
        });
    }
    Ok(AliasResult { aliases })
}

fn emit_alias(globals: &Globals, session: &Session, result: AliasResult) -> ExitCode {
    let envelope = base_envelope(SCHEMA_ALIAS, session, result);
    emit(globals.json, &envelope, || {
        let mut table = new_table();
        table.set_header(Row::from(vec!["NAME", "COURSE_ID", "CODE"]));
        for a in &envelope.result.aliases {
            table.add_row(Row::from(vec![
                a.name.clone(),
                a.course_id.clone(),
                a.course_code.clone().unwrap_or_default(),
            ]));
        }
        apply_two_space_padding(&mut table);
        writeln!(io::stdout(), "{table}")
    })
}
