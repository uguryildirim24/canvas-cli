//! `canvas download` (class D, M3-b).

#![allow(clippy::too_many_lines)]

use std::collections::HashSet;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use canvas_api::Client;
use canvas_core::download::{
    Action, ApiTransfer, Destination, InstallOpts, ManifestError, PlannedFile, PlannedSource,
    RemoteMeta, SqliteDestinationRegistry, TransferError, hash_path, install_part_file,
    open_destination, outcome_exit_code, plan_course,
};
use canvas_core::resolve::ResolvedCourse;
use canvas_core::sync::{CoursesScope, discovery_plan_input};
use comfy_table::Row;
use futures_util::stream::{self, StreamExt};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

use super::Globals;
use super::course::{refresh_fail, resolve_with_refresh};
use super::course_load::{ensure_courses, load_courses_for_scope, outcome_freshness};
use super::emit::{base_envelope, emit, emit_error, session_error, sync_error};
use super::files::{ensure_files, ensure_folders, ensure_modules};
use crate::config::Config;
use crate::output::{
    DownloadCourseJson, DownloadFileJson, DownloadResult, DownloadTotalsJson, Freshness, Outcome,
    SCHEMA_DOWNLOAD, apply_two_space_padding, new_table, now_timestamp,
};
use crate::paths::CliPaths;
use crate::session::{Session, ttl_courses};

/// CLI options for `download`.
pub struct DownloadArgs {
    pub course: Option<String>,
    pub all_courses: bool,
    pub dest: Option<PathBuf>,
    pub module: Option<String>,
    pub files: Vec<i64>,
    pub jobs: Option<u32>,
    pub dry_run: bool,
    pub force: bool,
    pub verify: bool,
}

/// Run `canvas download`.
pub async fn run(globals: &Globals, args: DownloadArgs) -> ExitCode {
    if globals.offline {
        return emit_error(
            globals.json,
            "usage",
            "download cannot be used with --offline",
            2,
            globals.profile.clone(),
            None,
        );
    }

    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(globals.json, e, globals.profile.clone()),
    };

    let Some(client) = session.client.clone() else {
        return emit_error(
            globals.json,
            "auth",
            "no token; set CANVAS_TOKEN or run auth login",
            3,
            session.profile.clone(),
            Some(session.identity_ref()),
        );
    };

    let config = match CliPaths::resolve().and_then(|p| Config::load(&p)) {
        Ok(c) => c,
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

    let dest_path = match resolve_dest(args.dest.as_deref(), config.download.dest.as_deref()) {
        Ok(p) => p,
        Err(msg) => {
            return emit_error(
                globals.json,
                "usage",
                &msg,
                2,
                session.profile.clone(),
                Some(session.identity_ref()),
            );
        }
    };
    let jobs = args.jobs.unwrap_or(config.download.jobs).max(1) as usize;

    if let Err(e) = session.validate_network_token().await {
        return sync_error(globals, &session, &e);
    }

    let courses = match select_courses(globals, &session, &args).await {
        Ok(c) => c,
        Err(code) => return code,
    };

    let mut freshness = Vec::new();
    let mut course_results = Vec::new();
    let mut all_actions = Vec::new();
    let mut verify_mismatch = false;
    let mut bytes_total = 0u64;
    let mut warnings = Vec::new();

    let destination = if args.dry_run {
        None
    } else {
        let identity_key = session.identity.key.to_string();
        let identity_dir = session.paths.identity_dir.clone();
        let state_db = session.paths.state_db.clone();
        let dest = dest_path.clone();
        match tokio::task::spawn_blocking(move || {
            let registry = SqliteDestinationRegistry::from_migrated_state(&state_db)?;
            open_destination(&dest, &identity_key, &identity_dir, &registry)
        })
        .await
        {
            Ok(Ok(d)) => Some(d),
            Ok(Err(e)) => return manifest_abort(globals, &session, e),
            Err(_) => {
                return emit_error(
                    globals.json,
                    "local",
                    "blocking worker failed",
                    13,
                    session.profile.clone(),
                    Some(session.identity_ref()),
                );
            }
        }
    };

    let show_progress = !globals.json && !globals.quiet;
    let multi = show_progress.then(MultiProgress::new);
    let total_bar = multi.as_ref().map(|m| {
        let bar = m.add(ProgressBar::new(0));
        bar.set_style(
            ProgressStyle::with_template("{msg} {bar:40.cyan/blue} {bytes}/{total_bytes}")
                .unwrap_or_else(|_| ProgressStyle::default_bar()),
        );
        bar.set_message("total");
        bar
    });

    for course in courses {
        let (planned, course_fresh) = match prepare_course(globals, &session, &course, &args).await
        {
            Ok(v) => v,
            Err(code) => return code,
        };
        freshness.extend(course_fresh);

        let mut file_rows = Vec::new();
        let recovered: HashSet<i64> = destination
            .as_ref()
            .map(|d| d.recovery_actions.iter().map(|(id, _)| *id).collect())
            .unwrap_or_default();

        if let Some(dest) = &destination {
            for (file_id, action) in &dest.recovery_actions {
                if let Some(pf) = planned.iter().find(|p| p.file_id == *file_id) {
                    file_rows.push(file_json(pf, *action, None, None, None));
                    all_actions.push(*action);
                }
            }
        }

        if args.dry_run {
            for pf in &planned {
                let action = if pf.skipped_external {
                    Action::SkippedExternal
                } else {
                    Action::Planned
                };
                file_rows.push(file_json(pf, action, None, None, None));
                all_actions.push(action);
            }
        } else {
            let dest = destination.as_ref().expect("destination opened");
            let to_run: Vec<PlannedFile> = planned
                .into_iter()
                .filter(|p| !recovered.contains(&p.file_id))
                .collect();

            if let Some(bar) = &total_bar {
                let add: u64 = to_run
                    .iter()
                    .filter(|p| !p.skipped_external)
                    .map(|p| p.size.unwrap_or(0))
                    .sum();
                bar.inc_length(add);
            }

            let transfer = ApiTransfer::new(client.clone());
            let force = args.force;
            let verify = args.verify;
            let results = stream::iter(to_run.into_iter().map(|pf| {
                let dest = dest.clone();
                let transfer = transfer.clone();
                let multi = multi.clone();
                let total_bar = total_bar.clone();
                let client = client.clone();
                let course_id = course.id;
                async move {
                    process_one(
                        &client,
                        &dest,
                        &transfer,
                        pf,
                        course_id,
                        force,
                        verify,
                        multi.as_ref(),
                        total_bar.as_ref(),
                    )
                    .await
                }
            }))
            .buffer_unordered(jobs)
            .collect::<Vec<_>>()
            .await;

            for row in results {
                if row.verify.as_deref() == Some("mismatch") {
                    verify_mismatch = true;
                }
                if row.action == Action::Downloaded.as_str() {
                    bytes_total = bytes_total.saturating_add(row.size.unwrap_or(0));
                }
                match row.action.as_str() {
                    "unmanaged" => warnings.push(format!(
                        "unmanaged file at {}; pass --force to replace",
                        row.path
                    )),
                    "modified" => warnings.push(format!(
                        "modified local file at {}; pass --force to replace",
                        row.path
                    )),
                    _ => {}
                }
                all_actions.push(action_from_str(&row.action));
                file_rows.push(row);
            }
        }

        file_rows.sort_by(|a, b| a.path.cmp(&b.path));
        course_results.push(DownloadCourseJson {
            course_id: course.id.to_string(),
            course_code: course.code.clone().unwrap_or_default(),
            files: file_rows,
        });
    }

    if let Some(bar) = &total_bar {
        bar.finish_and_clear();
    }

    let exit = outcome_exit_code(&all_actions, verify_mismatch, args.dry_run);
    let totals = sum_totals(&course_results, bytes_total);
    let result = DownloadResult {
        dest: dest_path.display().to_string(),
        dry_run: args.dry_run,
        courses: course_results,
        totals,
    };

    let mut envelope = base_envelope(SCHEMA_DOWNLOAD, &session, result);
    envelope.freshness = freshness;
    envelope.requests = session.requests();
    envelope.warnings = warnings;
    envelope.exit = exit;
    envelope.outcome = match exit {
        12 => Outcome::Partial,
        10 => Outcome::Error,
        _ => Outcome::Ok,
    };

    emit(globals.json, &envelope, || print_human(&envelope.result))
}

#[allow(clippy::too_many_arguments)]
async fn process_one(
    client: &Client,
    dest: &Destination,
    transfer: &ApiTransfer,
    pf: PlannedFile,
    course_id: i64,
    force: bool,
    verify: bool,
    multi: Option<&MultiProgress>,
    total_bar: Option<&ProgressBar>,
) -> DownloadFileJson {
    if pf.skipped_external {
        return file_json(&pf, Action::SkippedExternal, None, None, None);
    }

    let file_bar = multi.map(|m| {
        let bar = m.add(ProgressBar::new(pf.size.unwrap_or(0)));
        bar.set_style(
            ProgressStyle::with_template("{msg} {bar:30.green/white} {bytes}/{total_bytes}")
                .unwrap_or_else(|_| ProgressStyle::default_bar()),
        );
        bar.set_message(pf.path.clone());
        bar
    });

    let meta = match canvas_core::download::file_remote_meta(client, pf.file_id).await {
        Ok(m) => m,
        Err(TransferError::Message(m)) if m.starts_with("unavailable:") => {
            finish_bar(file_bar.as_ref());
            return file_json(&pf, Action::Unavailable, None, Some(m), None);
        }
        Err(TransferError::Message(m)) if m == "locked" => {
            finish_bar(file_bar.as_ref());
            return file_json(&pf, Action::Locked, None, None, None);
        }
        Err(TransferError::Message(m)) if m == "unauthorized" => {
            finish_bar(file_bar.as_ref());
            return file_json(&pf, Action::Failed, None, Some(m), None);
        }
        Err(e) => {
            finish_bar(file_bar.as_ref());
            return file_json(&pf, Action::Failed, None, Some(e.to_string()), None);
        }
    };
    if meta.locked {
        finish_bar(file_bar.as_ref());
        return file_json(&pf, Action::Locked, None, None, None);
    }

    let remote = if meta.remote.size == 0 {
        RemoteMeta {
            size: pf.size.unwrap_or(0),
            updated_at: pf.updated_at.clone().or(meta.remote.updated_at),
        }
    } else {
        meta.remote
    };
    if remote.size == 0 {
        finish_bar(file_bar.as_ref());
        return file_json(
            &pf,
            Action::Failed,
            None,
            Some("missing remote size".into()),
            None,
        );
    }
    if let Some(bar) = &file_bar {
        bar.set_length(remote.size);
    }

    let opts = InstallOpts {
        force,
        verify,
        course_id,
    };
    let action = match install_part_file(dest, transfer, pf.file_id, &pf.path, &remote, &opts).await
    {
        Ok(a) => a,
        Err(e) => map_install_err(e),
    };

    let verify_status = if verify {
        hash_and_compare(dest, &pf, action).unwrap_or(Some("mismatch".into()))
    } else {
        None
    };

    if let Some(bar) = total_bar
        && matches!(action, Action::Downloaded | Action::Skipped | Action::Moved)
    {
        bar.inc(remote.size);
    }
    finish_bar(file_bar.as_ref());

    let mut row = file_json(&pf, action, None, None, verify_status);
    if matches!(
        action,
        Action::Downloaded | Action::Skipped | Action::Moved | Action::Planned
    ) {
        row.size = Some(remote.size);
    }
    row
}

fn finish_bar(bar: Option<&ProgressBar>) {
    if let Some(b) = bar {
        b.finish_and_clear();
    }
}

fn hash_and_compare(
    dest: &Destination,
    pf: &PlannedFile,
    action: Action,
) -> Result<Option<String>, canvas_core::download::InstallError> {
    if !matches!(
        action,
        Action::Skipped | Action::Downloaded | Action::Moved | Action::Modified
    ) {
        return Ok(None);
    }
    let Some(row) = dest.manifest.get(pf.file_id)? else {
        return Ok(None);
    };
    let Some(expected) = row.sha256.as_ref() else {
        return Ok(None);
    };
    match hash_path(&dest.root, &pf.path)? {
        Some(actual) if &actual == expected => Ok(Some("ok".into())),
        Some(_) | None => Ok(Some("mismatch".into())),
    }
}

fn map_install_err(e: canvas_core::download::InstallError) -> Action {
    use canvas_core::download::InstallError;
    match &e {
        InstallError::Transfer(TransferError::Message(m)) if m == "locked" => Action::Locked,
        InstallError::Transfer(TransferError::Message(m)) if m.starts_with("unavailable:") => {
            Action::Unavailable
        }
        InstallError::Transfer(TransferError::Message(m)) if m == "failed" => Action::Failed,
        other => other.action(),
    }
}

fn action_from_str(s: &str) -> Action {
    match s {
        "planned" => Action::Planned,
        "downloaded" => Action::Downloaded,
        "moved" => Action::Moved,
        "skipped" => Action::Skipped,
        "unmanaged" => Action::Unmanaged,
        "modified" => Action::Modified,
        "locked" => Action::Locked,
        "unavailable" => Action::Unavailable,
        "skipped_external" => Action::SkippedExternal,
        "unsafe_path" => Action::UnsafePath,
        "unresolved_move" => Action::UnresolvedMove,
        _ => Action::Failed,
    }
}

fn file_json(
    pf: &PlannedFile,
    action: Action,
    previous_path: Option<String>,
    error: Option<String>,
    verify: Option<String>,
) -> DownloadFileJson {
    DownloadFileJson {
        id: pf.file_id.to_string(),
        path: pf.path.clone(),
        previous_path,
        action: action.as_str().to_owned(),
        size: pf.size,
        error,
        verify,
    }
}

fn sum_totals(courses: &[DownloadCourseJson], bytes: u64) -> DownloadTotalsJson {
    let mut t = DownloadTotalsJson {
        planned: 0,
        downloaded: 0,
        moved: 0,
        skipped: 0,
        unmanaged: 0,
        modified: 0,
        locked: 0,
        unavailable: 0,
        skipped_external: 0,
        unsafe_path: 0,
        unresolved_move: 0,
        failed: 0,
        bytes,
    };
    for c in courses {
        for f in &c.files {
            match f.action.as_str() {
                "planned" => t.planned += 1,
                "downloaded" => t.downloaded += 1,
                "moved" => t.moved += 1,
                "skipped" => t.skipped += 1,
                "unmanaged" => t.unmanaged += 1,
                "modified" => t.modified += 1,
                "locked" => t.locked += 1,
                "unavailable" => t.unavailable += 1,
                "skipped_external" => t.skipped_external += 1,
                "unsafe_path" => t.unsafe_path += 1,
                "unresolved_move" => t.unresolved_move += 1,
                _ => t.failed += 1,
            }
        }
    }
    t
}

fn print_human(result: &DownloadResult) -> io::Result<()> {
    let mut table = new_table();
    table.set_header(vec!["course", "path", "action", "size"]);
    for course in &result.courses {
        for f in &course.files {
            table.add_row(Row::from(vec![
                course.course_code.clone(),
                f.path.clone(),
                f.action.clone(),
                f.size.map(|s| s.to_string()).unwrap_or_default(),
            ]));
        }
    }
    apply_two_space_padding(&mut table);
    writeln!(io::stdout(), "{table}")?;
    writeln!(
        io::stdout(),
        "totals: downloaded={} moved={} skipped={} failed={} bytes={}",
        result.totals.downloaded,
        result.totals.moved,
        result.totals.skipped,
        result.totals.failed,
        result.totals.bytes
    )?;
    Ok(())
}

fn resolve_dest(flag: Option<&Path>, config: Option<&str>) -> Result<PathBuf, String> {
    let raw = flag
        .map(|p| p.display().to_string())
        .or_else(|| config.map(str::to_owned))
        .ok_or_else(|| "download requires --dest or download.dest in config".to_owned())?;
    Ok(expand_tilde(&raw))
}

fn expand_tilde(raw: &str) -> PathBuf {
    if let Some(rest) = raw.strip_prefix("~/")
        && let Ok(home) = std::env::var("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    if raw == "~"
        && let Ok(home) = std::env::var("HOME")
    {
        return PathBuf::from(home);
    }
    PathBuf::from(raw)
}

fn manifest_abort(globals: &Globals, session: &Session, err: ManifestError) -> ExitCode {
    let code = match &err {
        ManifestError::IdentityMismatch { .. } | ManifestError::UnregisteredRoot(_) => "refused",
        _ => "local",
    };
    emit_error(
        globals.json,
        code,
        &err.to_string(),
        err.exit_code(),
        session.profile.clone(),
        Some(session.identity_ref()),
    )
}

async fn select_courses(
    globals: &Globals,
    session: &Session,
    args: &DownloadArgs,
) -> Result<Vec<ResolvedCourse>, ExitCode> {
    if args.all_courses {
        match ensure_courses(
            session,
            CoursesScope::Active,
            ttl_courses(),
            now_timestamp(),
            globals.fresh,
            globals.offline,
        )
        .await
        {
            Ok(_) => {}
            Err(e) => return Err(refresh_fail(globals, session, e)),
        }
        let rows = session
            .open
            .store
            .call(|conns| load_courses_for_scope(conns, "active"))
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
        return Ok(rows
            .into_iter()
            .map(|r| ResolvedCourse {
                id: r.id,
                code: Some(r.code),
                name: Some(r.name),
            })
            .collect());
    }
    let course = args.course.as_deref().expect("course or all_courses");
    let (resolved, _, _) = resolve_with_refresh(globals, session, course).await?;
    Ok(vec![resolved])
}

async fn prepare_course(
    globals: &Globals,
    session: &Session,
    course: &ResolvedCourse,
    args: &DownloadArgs,
) -> Result<(Vec<PlannedFile>, Vec<Freshness>), ExitCode> {
    let mut freshness = Vec::new();
    match ensure_folders(globals, session, course.id).await {
        Ok(o) => freshness.push(outcome_freshness(&o)),
        Err(e) => return Err(refresh_fail(globals, session, e)),
    }
    match ensure_files(globals, session, course.id).await {
        Ok(o) => freshness.push(outcome_freshness(&o)),
        Err(e) => return Err(refresh_fail(globals, session, e)),
    }
    match ensure_modules(globals, session, course.id).await {
        Ok(o) => freshness.push(outcome_freshness(&o)),
        Err(e) => return Err(refresh_fail(globals, session, e)),
    }

    let course_id = course.id;
    let code = course.code.clone().unwrap_or_else(|| course.id.to_string());
    let input = session
        .open
        .store
        .call(move |conns| discovery_plan_input(conns, course_id, &code))
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

    let mut planned = plan_course(&input).map_err(|e| {
        emit_error(
            globals.json,
            "local",
            &e.to_string(),
            13,
            session.profile.clone(),
            Some(session.identity_ref()),
        )
    })?;

    if let Some(module_filter) = args.module.as_deref() {
        let needle = module_filter.to_lowercase();
        let module_ids: HashSet<i64> = input
            .modules
            .iter()
            .filter(|m| m.name.to_lowercase().contains(&needle))
            .map(|m| m.id)
            .collect();
        planned.retain(|p| match p.source {
            PlannedSource::Module { module_id } => module_ids.contains(&module_id),
            PlannedSource::ExternalTool | PlannedSource::FilesListing => false,
        });
    }
    if !args.files.is_empty() {
        let want: HashSet<i64> = args.files.iter().copied().collect();
        planned.retain(|p| want.contains(&p.file_id));
    }

    Ok((planned, freshness))
}
