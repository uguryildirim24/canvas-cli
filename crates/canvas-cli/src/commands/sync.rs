//! `canvas sync` (class D, M1-b partial).

use std::io::{self, Write};
use std::process::ExitCode;

use canvas_core::sync::{
    CoursesScope, PeriodKey, RefreshOutcome, SyncError, refresh_courses, refresh_enrollment_grades,
    refresh_grading_periods,
};
use comfy_table::Row;

use super::Globals;
use super::course_load::{load_courses_for_scope, map_source, u64_count};
use super::emit::{base_envelope, emit, emit_error, session_error};
use crate::output::{
    FreshnessSource, SCHEMA_SYNC, SyncDatasetJson, SyncResult, apply_two_space_padding, new_table,
    now_timestamp,
};
use crate::session::{Session, ttl_courses, ttl_grades};

/// Run `canvas sync`.
pub async fn run(globals: &Globals, full: bool) -> ExitCode {
    if globals.offline {
        return emit_error(
            globals.json,
            "usage",
            "sync cannot be used with --offline",
            2,
            globals.profile.clone(),
            None,
        );
    }

    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(globals.json, e, globals.profile.clone()),
    };

    let Some(client) = session.client.as_ref() else {
        return emit_error(
            globals.json,
            "auth",
            "no token; set CANVAS_TOKEN or run auth login",
            3,
            session.profile.clone(),
            Some(session.identity_ref()),
        );
    };

    let mut warnings = Vec::new();
    if full {
        warnings.push("--full is assembled in M4-b; refreshing M1-b datasets only".into());
    }

    let datasets = match refresh_all(globals, &session, client).await {
        Ok(d) => d,
        Err(code) => return code,
    };

    let api: u64 = datasets.iter().map(|d| d.requests).sum();
    let result = SyncResult { datasets };
    let mut envelope = base_envelope(SCHEMA_SYNC, &session, result);
    envelope.freshness = envelope
        .result
        .datasets
        .iter()
        .map(SyncDatasetJson::to_freshness)
        .collect();
    envelope.requests.api = api;
    envelope.warnings = warnings;

    emit(globals.json, &envelope, || print_table(&envelope.result))
}

async fn refresh_all(
    globals: &Globals,
    session: &Session,
    client: &canvas_api::Client,
) -> Result<Vec<SyncDatasetJson>, ExitCode> {
    let now = now_timestamp();
    let ttl_c = ttl_courses();
    let ttl_g = ttl_grades();
    let mut datasets = Vec::new();

    let courses = refresh_courses(
        client,
        &session.open.store,
        CoursesScope::Active,
        ttl_c,
        now,
        globals.fresh,
        false,
    )
    .await
    .map_err(|e| sync_err(globals, session, &e))?;
    datasets.push(to_dataset(&courses));

    let grades = refresh_enrollment_grades(
        client,
        &session.open.store,
        PeriodKey::None,
        ttl_g,
        now,
        globals.fresh,
        false,
    )
    .await
    .map_err(|e| sync_err(globals, session, &e))?;
    datasets.push(to_dataset(&grades));

    let course_ids = session
        .open
        .store
        .call(|conns| {
            Ok(load_courses_for_scope(conns, "active")?
                .into_iter()
                .map(|c| c.id)
                .collect::<Vec<_>>())
        })
        .await
        .map_err(|e| {
            emit_error(
                globals.json,
                "local",
                &e.to_string(),
                13,
                session.profile.clone(),
                Some(session.identity_ref()),
            )
        })?;

    for course_id in course_ids {
        let outcome = refresh_grading_periods(
            client,
            &session.open.store,
            course_id,
            ttl_g,
            now,
            globals.fresh,
            false,
        )
        .await
        .map_err(|e| sync_err(globals, session, &e))?;
        datasets.push(to_dataset(&outcome));
    }

    Ok(datasets)
}

fn print_table(result: &SyncResult) -> io::Result<()> {
    let mut table = new_table();
    table.set_header(Row::from(vec![
        "DATASET", "SCOPE", "SOURCE", "COUNT", "STALE", "REQUESTS",
    ]));
    for d in &result.datasets {
        let source = match d.source {
            FreshnessSource::Cache => "cache",
            FreshnessSource::Network => "network",
        };
        table.add_row(Row::from(vec![
            d.dataset.clone(),
            d.scope.clone(),
            source.to_owned(),
            d.count.map(|c| c.to_string()).unwrap_or_default(),
            d.stale.to_string(),
            d.requests.to_string(),
        ]));
    }
    apply_two_space_padding(&mut table);
    writeln!(io::stdout(), "{table}")
}

fn to_dataset(outcome: &RefreshOutcome) -> SyncDatasetJson {
    SyncDatasetJson {
        dataset: outcome.freshness.dataset.clone(),
        scope: outcome.freshness.scope.clone(),
        source: map_source(outcome.freshness.source),
        fetched_at: Some(outcome.freshness.fetched_at.to_string()),
        complete: outcome.freshness.complete,
        count: u64_count(outcome.freshness.count),
        stale: outcome.freshness.stale,
        requests: u64::from(outcome.requests),
        error: outcome.error.clone(),
    }
}

fn sync_err(globals: &Globals, session: &Session, err: &SyncError) -> ExitCode {
    let (code, exit) = match err {
        SyncError::OfflineMiss => ("offline", 7),
        SyncError::Api(_) => ("network", 4),
        SyncError::Db(_) | SyncError::Ingest(_) => ("local", 13),
    };
    emit_error(
        globals.json,
        code,
        &err.to_string(),
        exit,
        session.profile.clone(),
        Some(session.identity_ref()),
    )
}
