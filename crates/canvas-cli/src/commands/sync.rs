//! `canvas sync` (class D, M1-b partial).

use std::io::{self, Write};
use std::process::ExitCode;

use canvas_core::sync::{
    ContextWindow, CoursesScope, PeriodKey, PlannerWindow, RefreshOutcome, SyncError,
    refresh_announcements, refresh_assignments, refresh_calendar_events, refresh_courses,
    refresh_enrollment_grades, refresh_files, refresh_folders, refresh_grading_periods,
    refresh_missing, refresh_modules, refresh_planner,
};
use comfy_table::Row;

use super::Globals;
use super::course_load::{load_courses_for_scope, map_source, u64_count};
use super::emit::{base_envelope, emit, emit_error, session_error};
use crate::output::{
    FreshnessSource, SCHEMA_SYNC, SyncDatasetJson, SyncResult, apply_two_space_padding, new_table,
    now_timestamp,
};
use crate::session::{
    Session, ttl_announcements, ttl_assignments, ttl_calendar, ttl_courses, ttl_files, ttl_grades,
    ttl_missing, ttl_modules, ttl_planner,
};

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

    if let Err(e) = session.validate_network_token().await {
        return super::emit::sync_error(globals, &session, &e);
    }

    let warnings = Vec::new();

    let refreshed = match refresh_all(globals, &session, client, full).await {
        Ok(d) => d,
        Err(code) => return code,
    };

    let result = SyncResult {
        datasets: refreshed.datasets,
    };
    let mut envelope = base_envelope(SCHEMA_SYNC, &session, result);
    envelope.freshness = envelope
        .result
        .datasets
        .iter()
        .map(SyncDatasetJson::to_freshness)
        .collect();
    envelope.requests = session.requests();
    envelope.partial = refreshed.partial;
    if !envelope.partial.is_empty() {
        envelope.outcome = crate::output::Outcome::Partial;
        envelope.exit = 12;
    }
    envelope.warnings = warnings;

    emit(globals.json, &envelope, || print_table(&envelope.result))
}

struct Refreshed {
    datasets: Vec<SyncDatasetJson>,
    partial: Vec<crate::output::PartialScope>,
}

#[allow(clippy::too_many_lines)]
async fn refresh_all(
    globals: &Globals,
    session: &Session,
    client: &canvas_api::Client,
    full: bool,
) -> Result<Refreshed, ExitCode> {
    let now = now_timestamp();
    let ttl_c = ttl_courses();
    let ttl_g = ttl_grades();
    let mut datasets = Vec::new();
    let mut partial = Vec::new();

    let courses = refresh_courses(
        client,
        &session.open.store,
        CoursesScope::Active,
        ttl_c,
        now,
        true,
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
        true,
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

    for course_id in course_ids.iter().copied() {
        let before = client.telemetry().api;
        match refresh_grading_periods(
            client,
            &session.open.store,
            course_id,
            ttl_g,
            now,
            true,
            false,
        )
        .await
        {
            Ok(outcome) => datasets.push(to_dataset(&outcome)),
            Err(e) if e.classification().1 == 8 || e.classification().1 == 6 => {
                partial.push(crate::output::PartialScope {
                    scope: format!("grading_periods:course:{course_id}"),
                    http_status: e.classification().2,
                    message: e.safe_message(),
                });
                datasets.push(SyncDatasetJson {
                    dataset: "grading_periods".into(),
                    scope: format!("course:{course_id}"),
                    source: FreshnessSource::Network,
                    fetched_at: None,
                    complete: false,
                    count: None,
                    stale: true,
                    requests: client.telemetry().api.saturating_sub(before),
                    error: Some(e.safe_message()),
                });
            }
            Err(e) => return Err(sync_err(globals, session, &e)),
        }
    }
    // §5: the base refresh also covers assignments (with `include[]=submission`,
    // which is where submission status comes from), missing, the default
    // planner window, and announcements.
    let window = PlannerWindow::todo_default(now.to_zoned(session.time_zone()).date(), 14);
    for course_id in &course_ids {
        let outcome = refresh_assignments(
            client,
            &session.open.store,
            *course_id,
            ttl_assignments(),
            now,
            true,
            false,
        )
        .await
        .map_err(|e| sync_err(globals, session, &e))?;
        datasets.push(to_dataset(&outcome));
    }
    let missing = refresh_missing(client, &session.open.store, ttl_missing(), now, true, false)
        .await
        .map_err(|e| sync_err(globals, session, &e))?;
    datasets.push(to_dataset(&missing));
    let planner = refresh_planner(
        client,
        &session.open.store,
        window.clone(),
        ttl_planner(),
        now,
        true,
        false,
    )
    .await
    .map_err(|e| sync_err(globals, session, &e))?;
    datasets.push(to_dataset(&planner));

    let announcements = refresh_announcements(
        client,
        &session.open.store,
        ContextWindow::courses(window.clone(), &course_ids),
        ttl_announcements(),
        now,
        true,
        false,
    )
    .await
    .map_err(|e| sync_err(globals, session, &e))?;
    datasets.push(to_dataset(&announcements.outcome));
    partial.extend(denial_partials("announcements", &announcements.denials));

    if full {
        for course_id in &course_ids {
            for outcome in [
                refresh_folders(
                    client,
                    &session.open.store,
                    *course_id,
                    ttl_files(),
                    now,
                    true,
                    false,
                )
                .await
                .map_err(|e| sync_err(globals, session, &e))?,
                refresh_files(
                    client,
                    &session.open.store,
                    *course_id,
                    ttl_files(),
                    now,
                    true,
                    false,
                )
                .await
                .map_err(|e| sync_err(globals, session, &e))?,
                // `modules` fetches the module items it needs in the same
                // refresh (§10).
                refresh_modules(
                    client,
                    &session.open.store,
                    *course_id,
                    ttl_modules(),
                    now,
                    true,
                    false,
                )
                .await
                .map_err(|e| sync_err(globals, session, &e))?,
            ] {
                datasets.push(to_dataset(&outcome));
            }
        }
        let mut contexts = vec![format!("user_{}", session.identity.user_id)];
        contexts.extend(course_ids.iter().map(|id| format!("course_{id}")));
        let events = refresh_calendar_events(
            client,
            &session.open.store,
            ContextWindow::contexts(window, &contexts),
            ttl_calendar(),
            now,
            true,
            false,
        )
        .await
        .map_err(|e| sync_err(globals, session, &e))?;
        datasets.push(to_dataset(&events.outcome));
        partial.extend(denial_partials("calendar_events", &events.denials));
    }

    let derived = session.open.store.call(|conns| {
        let mut stmt = conns.cache.prepare("SELECT dataset,scope FROM fetch_log WHERE (dataset='terms' AND scope='active') OR (dataset='course_totals' AND scope IN (SELECT 'course:' || entity_id FROM membership WHERE dataset='courses' AND scope='active'))")?;
        let keys = stmt.query_map([], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?.collect::<Result<Vec<_>,_>>()?;
        keys.into_iter().map(|(d,s)| super::course_load::fetch_log_freshness(conns, &d, &s)).collect::<Result<Vec<_>,_>>()
    }).await.map_err(|e| sync_err(globals, session, &SyncError::Db(e)))?;
    for row in derived.into_iter().flatten() {
        datasets.push(SyncDatasetJson {
            dataset: row.dataset,
            scope: row.scope,
            source: if courses.error.is_some() {
                FreshnessSource::Cache
            } else {
                FreshnessSource::Network
            },
            fetched_at: row.fetched_at,
            complete: row.complete,
            count: row.count,
            stale: courses.freshness.stale || row.stale,
            requests: 0,
            error: courses.error.clone(),
        });
    }
    datasets.sort_by(|a, b| (&a.dataset, &a.scope).cmp(&(&b.dataset, &b.scope)));

    for dataset in &datasets {
        let scope = format!("{}:{}", dataset.dataset, dataset.scope);
        if let Some(message) = &dataset
            .error
            // Per-context denials are already reported per course.
            .as_deref()
            .filter(|e| !canvas_core::store::is_context_denial(e))
            && !partial.iter().any(|p| p.scope == scope)
        {
            partial.push(crate::output::PartialScope {
                scope,
                http_status: None,
                message: (*message).to_owned(),
            });
        }
    }
    partial.sort_by(|a, b| a.scope.cmp(&b.scope));
    partial.dedup_by(|a, b| a.scope == b.scope);
    Ok(Refreshed { datasets, partial })
}

/// One `partial[]` row per context a batch could not read (§12.6).
fn denial_partials(
    dataset: &str,
    denials: &[canvas_core::sync::ContextDenial],
) -> Vec<crate::output::PartialScope> {
    denials
        .iter()
        .map(|denial| {
            let scope = denial
                .course_id()
                .map_or_else(|| denial.context.clone(), |id| format!("course:{id}"));
            crate::output::PartialScope {
                scope: format!("{dataset}:{scope}"),
                http_status: Some(denial.http_status),
                message: format!(
                    "{dataset} for {} unavailable (HTTP {})",
                    denial.context, denial.http_status
                ),
            }
        })
        .collect()
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
    super::emit::sync_error(globals, session, err)
}
