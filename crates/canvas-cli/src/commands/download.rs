//! `canvas download` (class D, M3-b).

#![allow(clippy::too_many_lines)]

use std::collections::{HashMap, HashSet};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use canvas_api::Client;
use canvas_core::download::{
    Action, ApiTransfer, Destination, InstallOpts, ManifestError, PlannedFile,
    SqliteDestinationRegistry, TransferError, hash_path, install_part_file, open_destination,
    outcome_exit_code, plan_course,
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
    PartialScope, SCHEMA_DOWNLOAD, apply_two_space_padding, error_envelope, new_table,
    now_timestamp,
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

    let (courses, mut freshness) = match select_courses(globals, &session, &args).await {
        Ok(c) => c,
        Err(code) => return code,
    };

    let mut prepared = Vec::new();
    let mut partial = Vec::new();
    for course in courses {
        let (planned, course_fresh, course_partial) =
            match prepare_course(globals, &session, &course, &args).await {
                Ok(v) => v,
                Err(code) => return code,
            };
        freshness.extend(course_fresh);
        partial.extend(course_partial);
        prepared.push((course, planned));
    }
    let mut course_results = Vec::new();
    let mut verify_mismatch = false;
    let mut bytes_total = 0u64;
    let mut abort = None;

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

    let (mut recovery, recovered) = recovery_results(destination.as_ref());
    for (course, planned) in prepared {
        let mut file_rows = recovery.remove(&course.id).unwrap_or_default();

        if args.dry_run {
            for pf in &planned {
                let action = if pf.skipped_external {
                    Action::SkippedExternal
                } else {
                    Action::Planned
                };
                file_rows.push(file_json(pf, action, None, None, None));
                if !pf.skipped_external {
                    bytes_total = bytes_total.saturating_add(pf.size.unwrap_or(0));
                }
            }
        } else {
            let dest = destination.as_ref().expect("destination opened");
            let to_run: Vec<PlannedFile> = planned
                .into_iter()
                .filter(|p| p.skipped_external || !recovered.contains(&p.file_id))
                .collect();

            if let Some(bar) = &total_bar {
                let add: u64 = to_run
                    .iter()
                    .filter(|p| !p.skipped_external)
                    .map(|p| p.size.unwrap_or(0))
                    .fold(0, u64::saturating_add);
                bar.inc_length(add);
            }

            let transfer = ApiTransfer::new(client.clone());
            let force = args.force;
            let verify = args.verify;
            let stopped = Arc::new(AtomicBool::new(false));
            let results = stream::iter(to_run.into_iter().map(|pf| {
                let dest = dest.clone();
                let transfer = transfer.clone();
                let multi = multi.clone();
                let total_bar = total_bar.clone();
                let client = client.clone();
                let course_id = course.id;
                let stopped = stopped.clone();
                async move {
                    if stopped.load(Ordering::Acquire) {
                        return None;
                    }
                    let result = process_one(
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
                    .await;
                    if result.is_err() {
                        stopped.store(true, Ordering::Release);
                    }
                    Some(result)
                }
            }))
            .buffer_unordered(jobs)
            .collect::<Vec<_>>()
            .await;

            for result in results.into_iter().flatten() {
                let row = match result {
                    Ok(row) => row,
                    Err(failure) => {
                        let (row, error) = *failure;
                        if abort.is_none() {
                            abort = Some(error);
                        }
                        row
                    }
                };
                if row.verify.as_deref() == Some("mismatch") {
                    verify_mismatch = true;
                }
                if row.action == Action::Downloaded.as_str() {
                    bytes_total = bytes_total.saturating_add(row.size.unwrap_or(0));
                }
                if let Some(recovery) = file_rows.iter_mut().find(|r| {
                    row.action == "skipped"
                        && r.id == row.id
                        && r.path == row.path
                        && r.action == "moved"
                }) {
                    recovery.verify = row.verify;
                    recovery.size = row.size;
                } else {
                    file_rows.push(row);
                }
            }
        }

        file_rows.sort_by(|a, b| a.path.cmp(&b.path));
        course_results.push(DownloadCourseJson {
            course_id: course.id.to_string(),
            course_code: course.code.clone().unwrap_or_default(),
            files: file_rows,
        });
        if abort.is_some() {
            break;
        }
    }

    // Startup recovery covers the destination, including files outside filters
    // and courses that are no longer in the selected discovery set.
    for (course_id, mut files) in recovery {
        files.sort_by(|a, b| a.path.cmp(&b.path));
        course_results.push(DownloadCourseJson {
            course_id: course_id.to_string(),
            course_code: String::new(),
            files,
        });
    }
    course_results.sort_by(|a, b| {
        a.course_code
            .cmp(&b.course_code)
            .then(a.course_id.cmp(&b.course_id))
    });
    let mut all_actions = Vec::new();
    let mut warnings: Vec<String> = partial.iter().map(|p| p.message.clone()).collect();
    for item in &freshness {
        if item.stale {
            warnings.push(format!("served stale {} cache", item.dataset));
        }
    }
    for course in &course_results {
        for row in &course.files {
            all_actions.push(action_from_str(&row.action));
            if matches!(row.action.as_str(), "unmanaged" | "modified") {
                warnings.push(format!(
                    "{} file at {}; pass --force to replace",
                    row.action, row.path
                ));
            }
        }
    }

    if let Some(bar) = &total_bar {
        bar.finish_and_clear();
    }

    if !partial.is_empty() {
        all_actions.push(Action::Unavailable);
    }
    let exit = outcome_exit_code(&all_actions, verify_mismatch, args.dry_run);
    let totals = sum_totals(&course_results, bytes_total);
    let result = DownloadResult {
        dest: dest_path.display().to_string(),
        dry_run: args.dry_run,
        courses: course_results,
        totals,
    };

    if let Some(error) = abort {
        let mut envelope = error_envelope(
            error.code,
            &error.message,
            error.status,
            serde_json::json!({"download": result}),
            error.exit,
        );
        envelope.profile.clone_from(&session.profile);
        envelope.identity = Some(session.identity_ref());
        envelope.requests = session.requests();
        envelope.freshness = freshness;
        envelope.partial = partial;
        envelope.warnings = warnings;
        return emit(globals.json, &envelope, || {
            writeln!(io::stderr(), "{}", error.message)
        });
    }
    let mut envelope = base_envelope(SCHEMA_DOWNLOAD, &session, result);
    envelope.freshness = freshness;
    envelope.requests = session.requests();
    envelope.warnings = warnings;
    envelope.partial = partial;
    envelope.exit = exit;
    envelope.outcome = match exit {
        12 => Outcome::Partial,
        10 => Outcome::Mismatch,
        _ => Outcome::Ok,
    };

    emit(globals.json, &envelope, || print_human(&envelope.result))
}

fn recovery_results(
    destination: Option<&Destination>,
) -> (HashMap<i64, Vec<DownloadFileJson>>, HashSet<i64>) {
    let mut courses: HashMap<i64, Vec<DownloadFileJson>> = HashMap::new();
    let mut blocked = HashSet::new();
    if let Some(dest) = destination {
        for (id, action) in &dest.recovery_actions {
            let Some(row) = dest.recovery_rows.iter().find(|row| row.file_id == *id) else {
                continue;
            };
            // Only-old-match merely clears the marker; normal planning still runs.
            if *action == Action::Skipped {
                continue;
            }
            if matches!(action, Action::UnsafePath | Action::UnresolvedMove) {
                blocked.insert(*id);
            }
            let moved = *action == Action::Moved;
            courses
                .entry(row.course_id)
                .or_default()
                .push(DownloadFileJson {
                    id: id.to_string(),
                    path: if moved {
                        row.pending_move_to
                            .clone()
                            .unwrap_or_else(|| row.path.clone())
                    } else {
                        row.path.clone()
                    },
                    previous_path: moved.then(|| row.path.clone()),
                    action: action.as_str().into(),
                    size: Some(row.size),
                    error: matches!(action, Action::UnsafePath | Action::UnresolvedMove)
                        .then(|| action.as_str().into()),
                    verify: None,
                });
        }
    }
    (courses, blocked)
}

struct Abort {
    code: &'static str,
    exit: u8,
    status: Option<u16>,
    message: String,
}

fn transfer_failure(error: TransferError) -> (Action, String, Option<Abort>) {
    use canvas_api::Error as Api;
    match error {
        TransferError::Api(
            Api::NotFound
            | Api::Denied { status: 403 | 404 }
            | Api::Forbidden {
                rate_limited: false,
                ..
            },
        ) => (Action::Unavailable, "file unavailable".into(), None),
        TransferError::Api(error) => {
            let error = match error {
                Api::Denied { status: 401 } => Api::Unauthorized,
                error => error,
            };
            let sync = canvas_core::sync::SyncError::Api(error);
            let (code, exit, status) = sync.classification();
            let message = sync.safe_message();
            let abort = matches!(exit, 3..=6).then(|| Abort {
                code,
                exit,
                status,
                message: message.clone(),
            });
            (Action::Failed, message, abort)
        }
        TransferError::Message(message) if message == "locked" => (Action::Locked, message, None),
        other => (Action::Failed, other.to_string(), None),
    }
}

fn install_failure(error: canvas_core::download::InstallError) -> (Action, String, Option<Abort>) {
    use canvas_core::download::InstallError;
    match error {
        InstallError::Transfer(error) => transfer_failure(error),
        InstallError::Manifest(error)
            if !matches!(
                error,
                ManifestError::LockTimeout | ManifestError::UnsafePath
            ) =>
        {
            let message = error.to_string();
            (
                Action::Failed,
                message.clone(),
                Some(Abort {
                    code: "local",
                    exit: 13,
                    status: None,
                    message,
                }),
            )
        }
        other => (other.action(), other.to_string(), None),
    }
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
) -> Result<DownloadFileJson, Box<(DownloadFileJson, Abort)>> {
    if pf.skipped_external {
        return Ok(file_json(&pf, Action::SkippedExternal, None, None, None));
    }
    let meta = match canvas_core::download::file_remote_meta(client, pf.file_id).await {
        Ok(meta) => meta,
        Err(error) => return failed_row(&pf, transfer_failure(error)),
    };
    if meta.locked {
        return Ok(file_json(&pf, Action::Locked, None, None, None));
    }
    let remote = meta.remote.clone();
    let file_bar = multi.map(|m| {
        let bar = m.add(ProgressBar::new(remote.size));
        bar.set_style(
            ProgressStyle::with_template("{msg} {bar:30.green/white} {bytes}/{total_bytes}")
                .unwrap_or_else(|_| ProgressStyle::default_bar()),
        );
        bar.set_message(pf.path.clone());
        bar
    });
    let callback = progress_callback(file_bar.clone(), total_bar.cloned());
    let transfer = transfer.clone().with_metadata(meta).with_progress(callback);
    // The pre-install path supplies previous_path for moves; blocking reads and
    // verification stay off the async executor and under the install locks.
    let d = dest.clone();
    let file_id = pf.file_id;
    let before = match tokio::task::spawn_blocking(move || d.manifest.get(file_id)).await {
        Ok(Ok(row)) => row,
        Ok(Err(error)) => {
            finish_bar(file_bar.as_ref());
            return failed_row(&pf, install_failure(error.into()));
        }
        Err(_) => {
            finish_bar(file_bar.as_ref());
            return failed_row(
                &pf,
                install_failure(canvas_core::download::InstallError::Worker),
            );
        }
    };
    let opts = InstallOpts {
        force,
        verify,
        course_id,
    };
    let action =
        match install_part_file(dest, &transfer, pf.file_id, &pf.path, &remote, &opts).await {
            Ok(action) => action,
            Err(error) => {
                finish_bar(file_bar.as_ref());
                return failed_row(&pf, install_failure(error));
            }
        };
    let verify_status = if verify {
        match hash_and_compare(dest, &pf, action).await {
            Ok(status) => status,
            Err(error) => {
                finish_bar(file_bar.as_ref());
                return failed_row(&pf, install_failure(error));
            }
        }
    } else {
        None
    };
    finish_bar(file_bar.as_ref());
    let previous = before.map(|r| r.path).filter(|path| path != &pf.path);
    let mut row = file_json(&pf, action, previous, None, verify_status);
    row.size = Some(remote.size);
    Ok(row)
}

fn failed_row(
    pf: &PlannedFile,
    (action, message, abort): (Action, String, Option<Abort>),
) -> Result<DownloadFileJson, Box<(DownloadFileJson, Abort)>> {
    let row = file_json(pf, action, None, Some(message), None);
    match abort {
        Some(error) => Err(Box::new((row, error))),
        None => Ok(row),
    }
}

fn progress_callback(
    file: Option<ProgressBar>,
    total: Option<ProgressBar>,
) -> canvas_core::download::ProgressFn {
    let previous = AtomicU64::new(0);
    Arc::new(move |_, bytes| {
        let old = previous.swap(bytes, Ordering::Relaxed);
        if let Some(bar) = &file {
            bar.set_position(bytes);
        }
        if let Some(bar) = &total {
            bar.inc(bytes.saturating_sub(old));
        }
    })
}

fn finish_bar(bar: Option<&ProgressBar>) {
    if let Some(b) = bar {
        b.finish_and_clear();
    }
}

async fn hash_and_compare(
    dest: &Destination,
    pf: &PlannedFile,
    action: Action,
) -> Result<Option<String>, canvas_core::download::InstallError> {
    use canvas_core::download::InstallError;
    if !matches!(
        action,
        Action::Skipped | Action::Downloaded | Action::Moved | Action::Modified
    ) {
        return Ok(None);
    }
    let lock = dest
        .acquire_install_lock(canvas_core::download::LOCK_TIMEOUT)
        .await?;
    let d = dest.clone();
    let pf = pf.clone();
    tokio::task::spawn_blocking(move || {
        let _lock = lock;
        let Some(row) = d.manifest.get(pf.file_id)? else {
            return Ok(None);
        };
        let Some(expected) = row.sha256.as_ref() else {
            return Ok(None);
        };
        // A different path belongs to a different run; do not hash an unrelated file.
        if row.path != pf.path {
            return Ok(None);
        }
        match hash_path(&d.root, &pf.path)? {
            Some(actual) if &actual == expected => Ok(Some("ok".into())),
            Some(_) | None => Ok(Some("mismatch".into())),
        }
    })
    .await
    .map_err(|_| InstallError::Worker)?
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
    table.set_header(vec!["course", "path", "action", "size", "verify"]);
    for course in &result.courses {
        for f in &course.files {
            table.add_row(Row::from(vec![
                course.course_code.clone(),
                f.previous_path
                    .as_ref()
                    .map_or_else(|| f.path.clone(), |old| format!("{old} -> {}", f.path)),
                f.action.clone(),
                f.size.map(|s| s.to_string()).unwrap_or_default(),
                f.verify.clone().unwrap_or_default(),
            ]));
        }
    }
    for course in &result.courses {
        for file in &course.files {
            if let Some(error) = &file.error {
                writeln!(io::stderr(), "{}: {error}", file.path)?;
            }
        }
    }
    apply_two_space_padding(&mut table);
    writeln!(io::stdout(), "{table}")?;
    writeln!(
        io::stdout(),
        "totals: planned={} downloaded={} moved={} skipped={} unmanaged={} modified={} locked={} unavailable={} skipped_external={} unsafe_path={} unresolved_move={} failed={} bytes={}",
        result.totals.planned,
        result.totals.downloaded,
        result.totals.moved,
        result.totals.skipped,
        result.totals.unmanaged,
        result.totals.modified,
        result.totals.locked,
        result.totals.unavailable,
        result.totals.skipped_external,
        result.totals.unsafe_path,
        result.totals.unresolved_move,
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
) -> Result<(Vec<ResolvedCourse>, Vec<Freshness>), ExitCode> {
    if args.all_courses {
        let outcome = match ensure_courses(
            session,
            CoursesScope::Active,
            ttl_courses(),
            now_timestamp(),
            globals.fresh,
            globals.offline,
        )
        .await
        {
            Ok(outcome) => outcome,
            Err(e) => return Err(refresh_fail(globals, session, e)),
        };
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
        return Ok((
            rows.into_iter()
                .map(|r| ResolvedCourse {
                    id: r.id,
                    code: Some(r.code),
                    name: Some(r.name),
                })
                .collect(),
            vec![outcome_freshness(&outcome)],
        ));
    }
    let course = args.course.as_deref().expect("course or all_courses");
    let (resolved, freshness, _) = resolve_with_refresh(globals, session, course).await?;
    Ok((vec![resolved], freshness))
}

async fn prepare_course(
    globals: &Globals,
    session: &Session,
    course: &ResolvedCourse,
    args: &DownloadArgs,
) -> Result<(Vec<PlannedFile>, Vec<Freshness>, Vec<PartialScope>), ExitCode> {
    let mut freshness = Vec::new();
    let mut partial = Vec::new();
    let folders = ensure_folders(globals, session, course.id)
        .await
        .map_err(|e| refresh_fail(globals, session, e))?;
    let files = ensure_files(globals, session, course.id)
        .await
        .map_err(|e| refresh_fail(globals, session, e))?;
    let modules = ensure_modules(globals, session, course.id)
        .await
        .map_err(|e| refresh_fail(globals, session, e))?;
    for outcome in [&folders, &files, &modules] {
        freshness.push(outcome_freshness(outcome));
        if let Some(status) = outcome
            .error
            .as_deref()
            .and_then(canvas_core::sync::listing_denial_status)
        {
            let message = if outcome.freshness.dataset == "files" {
                format!(
                    "Files listing unavailable (HTTP {status}); showing files linked from modules"
                )
            } else {
                format!("Folders listing unavailable (HTTP {status}); folder paths unavailable")
            };
            partial.push(PartialScope {
                scope: format!("{}:course:{}", outcome.freshness.dataset, course.id),
                http_status: Some(status),
                message,
            });
        }
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
        let modules: Vec<_> = input
            .modules
            .iter()
            .filter(|m| m.name.to_lowercase().contains(&needle))
            .collect();
        let file_ids: HashSet<_> = modules
            .iter()
            .flat_map(|m| &m.items)
            .filter(|i| i.item_type.eq_ignore_ascii_case("File"))
            .filter_map(|i| i.content_id)
            .collect();
        let external_ids: HashSet<_> = modules
            .iter()
            .flat_map(|m| &m.items)
            .filter(|i| i.item_type.eq_ignore_ascii_case("ExternalTool"))
            .map(|i| i.content_id.unwrap_or(i.id))
            .collect();
        planned.retain(|p| {
            if p.skipped_external {
                external_ids.contains(&p.file_id)
            } else {
                file_ids.contains(&p.file_id)
            }
        });
    }
    if !args.files.is_empty() {
        let want: HashSet<i64> = args.files.iter().copied().collect();
        planned.retain(|p| want.contains(&p.file_id));
    }

    Ok((planned, freshness, partial))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_updates_active_file_and_total_incrementally() {
        let file = ProgressBar::hidden();
        let total = ProgressBar::hidden();
        let progress = progress_callback(Some(file.clone()), Some(total.clone()));
        progress(50, 2);
        assert_eq!(file.position(), 2);
        assert_eq!(total.position(), 2);
        progress(50, 5);
        assert_eq!(file.position(), 5);
        assert_eq!(total.position(), 5);
    }

    #[test]
    fn every_action_has_counts_and_completed_exit_precedence() {
        for action in [
            Action::Planned,
            Action::Downloaded,
            Action::Moved,
            Action::Skipped,
            Action::Unmanaged,
            Action::Modified,
            Action::Locked,
            Action::Unavailable,
            Action::SkippedExternal,
            Action::UnsafePath,
            Action::UnresolvedMove,
            Action::Failed,
        ] {
            let row = DownloadFileJson {
                id: "1".into(),
                path: "file".into(),
                previous_path: None,
                action: action.as_str().into(),
                size: Some(4),
                error: None,
                verify: None,
            };
            let totals = sum_totals(
                &[DownloadCourseJson {
                    course_id: "101".into(),
                    course_code: "CS".into(),
                    files: vec![row],
                }],
                4,
            );
            assert_eq!(serde_json::to_value(totals).unwrap()[action.as_str()], 1);
            assert_eq!(action_from_str(action.as_str()), action);
            let partial = matches!(
                action,
                Action::Locked
                    | Action::Unavailable
                    | Action::UnsafePath
                    | Action::UnresolvedMove
                    | Action::Failed
            );
            assert_eq!(
                outcome_exit_code(&[action], false, false),
                if partial { 12 } else { 0 }
            );
            assert_eq!(outcome_exit_code(&[action], true, false), 10);
            assert_eq!(outcome_exit_code(&[action], true, true), 0);
        }
    }
}
