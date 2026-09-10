//! Unit tests for sync conversion and hit-predicate behaviour.

use std::fs;

use jiff::{Span, Timestamp};
use tempfile::TempDir;

use crate::identity::{IdentityDocument, Paths};
use crate::store::{
    Dataset, IngestOpts, LookupResult, OpenIdentity, lookup_dataset, read_scope_epoch,
};

use super::courses::{
    CoursesDataset, CoursesScope, course_to_entity, courses_fetch_paths, courses_to_ingest_page,
    default_ttl_courses,
};
use super::outcome::{FreshnessSource, SyncError};

fn ts(secs: i64) -> Timestamp {
    Timestamp::from_second(secs).unwrap()
}

fn setup() -> (TempDir, OpenIdentity) {
    let dir = TempDir::new().unwrap();
    let doc = IdentityDocument::new(
        "https://courses.example.test",
        12345,
        "2026-01-01T00:00:00Z",
    );
    let paths = Paths::for_identity(dir.path(), &doc.key);
    fs::create_dir_all(&paths.identity_dir).unwrap();
    fs::create_dir_all(paths.lock_path.parent().unwrap()).unwrap();
    doc.write(&paths.identity_json()).unwrap();
    let open = OpenIdentity::open(&paths, &doc).unwrap();
    (dir, open)
}

fn ingest_ok(epoch_seen: i64) -> IngestOpts<'static> {
    IngestOpts {
        epoch_seen,
        complete: true,
        stale: false,
        error: None,
        window: None,
        contexts: None,
    }
}

#[test]
fn courses_all_collects_three_enrollment_states() {
    let paths = courses_fetch_paths(CoursesScope::All);
    assert_eq!(paths.len(), 3);
    assert!(paths[0].contains("enrollment_state=active"));
    assert!(paths[1].contains("enrollment_state=completed"));
    assert!(paths[2].contains("enrollment_state=invited_or_pending"));
    for path in &paths {
        assert!(path.contains("include[]=term"));
        assert!(path.contains("include[]=total_scores"));
        assert!(path.contains("include[]=current_grading_period_scores"));
        assert!(path.contains("include[]=favorites"));
        assert!(path.contains("enrollment_type=student"));
    }

    // Page→entity conversion merges three state hints into one `all` membership.
    let active = sample_course(1, "A");
    let completed = sample_course(2, "B");
    let invited = sample_course(3, "C");
    let page_a = courses_to_ingest_page(&[active], "active", ts(100));
    let page_b = courses_to_ingest_page(&[completed], "completed", ts(100));
    let page_c = courses_to_ingest_page(&[invited], "invited_or_pending", ts(100));
    assert_eq!(
        page_a.entities.len() + page_b.entities.len() + page_c.entities.len(),
        3
    );

    let (_dir, open) = setup();
    let ds = CoursesDataset::new(CoursesScope::All, default_ttl_courses());
    open.store
        .call_blocking({
            let pages = [page_a, page_b, page_c];
            let ds = ds.clone();
            move |conns| {
                let epoch = read_scope_epoch(&conns.state, &ds.epoch_scope())?;
                ds.ingest(&pages, &ingest_ok(epoch), conns)
                    .map_err(|e| crate::store::DbError::Message(e.to_string()))?;
                Ok(())
            }
        })
        .unwrap();

    let count: i64 = open
        .store
        .call_blocking(|conns| {
            Ok(conns.cache.query_row(
                "SELECT count FROM fetch_log WHERE dataset='courses' AND scope='all'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(count, 3);

    let states: Vec<String> = open
        .store
        .call_blocking(|conns| {
            let mut stmt = conns.cache.prepare(
                "SELECT json_extract(data_json, '$.enrollment_state') FROM courses ORDER BY id",
            )?;
            let rows = stmt
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .unwrap();
    assert_eq!(
        states,
        vec![
            "active".to_owned(),
            "completed".to_owned(),
            "invited_or_pending".to_owned()
        ]
    );
}

#[test]
fn count_zero_complete_dataset_is_offline_hit() {
    let (_dir, open) = setup();
    let ds = CoursesDataset::new(CoursesScope::Active, default_ttl_courses());
    let now = ts(1_000);

    open.store
        .call_blocking({
            let ds = ds.clone();
            move |conns| {
                let epoch = read_scope_epoch(&conns.state, &ds.epoch_scope())?;
                // Empty page: complete coverage with count = 0.
                let page = courses_to_ingest_page(&[], "active", now);
                ds.ingest(&[page], &ingest_ok(epoch), conns)
                    .map_err(|e| crate::store::DbError::Message(e.to_string()))?;
                Ok(())
            }
        })
        .unwrap();

    let lookup = open
        .store
        .call_blocking({
            let ds = ds.clone();
            move |conns| lookup_dataset(conns, &ds, now, None)
        })
        .unwrap();
    match lookup {
        LookupResult::Hit(row) => {
            assert!(row.complete);
            assert_eq!(row.count, 0);
            assert!(!row.stale);
        }
        other => panic!("expected Hit, got {other:?}"),
    }

    // Offline refresh must serve the empty complete row (not Offline miss).
    let outcome = serve_offline(&open, &ds, now).unwrap();
    assert_eq!(outcome.freshness.source, FreshnessSource::Cache);
    assert!(outcome.freshness.complete);
    assert_eq!(outcome.freshness.count, 0);
    assert!(outcome.freshness.stale);
}

#[test]
fn stale_serve_on_failed_refresh() {
    let (_dir, open) = setup();
    let ds = CoursesDataset::new(CoursesScope::Active, Span::new().seconds(1));
    let fetched_at = ts(100);
    let later = ts(10_000);

    open.store
        .call_blocking({
            let ds = ds.clone();
            let course = sample_course(9, "Stale");
            move |conns| {
                let epoch = read_scope_epoch(&conns.state, &ds.epoch_scope())?;
                let page = courses_to_ingest_page(&[course], "active", fetched_at);
                ds.ingest(&[page], &ingest_ok(epoch), conns)
                    .map_err(|e| crate::store::DbError::Message(e.to_string()))?;
                Ok(())
            }
        })
        .unwrap();

    // TTL expired → Stale lookup.
    let lookup = open
        .store
        .call_blocking({
            let ds = ds.clone();
            move |conns| lookup_dataset(conns, &ds, later, None)
        })
        .unwrap();
    assert!(matches!(lookup, LookupResult::Stale(_)));

    // Offline after TTL expiry still serves the complete row as stale.
    let outcome = serve_offline(&open, &ds, later).unwrap();
    assert!(outcome.freshness.stale);
    assert!(outcome.freshness.complete);
    assert_eq!(outcome.freshness.count, 1);
    assert_eq!(outcome.freshness.source, FreshnessSource::Cache);

    // Simulate failed refresh marking: ingest incomplete marks stale; prior row still served.
    open.store
        .call_blocking({
            let ds = ds.clone();
            move |conns| {
                let epoch = read_scope_epoch(&conns.state, &ds.epoch_scope())?;
                ds.ingest(
                    &[],
                    &IngestOpts {
                        epoch_seen: epoch,
                        complete: false,
                        stale: true,
                        error: Some("network"),
                        window: None,
                        contexts: None,
                    },
                    conns,
                )
                .map_err(|e| crate::store::DbError::Message(e.to_string()))?;
                Ok(())
            }
        })
        .unwrap();

    let outcome = serve_offline(&open, &ds, later).unwrap();
    assert!(outcome.freshness.stale);
    assert_eq!(outcome.freshness.count, 1);
}

#[test]
fn course_to_entity_carries_enrollment_hint() {
    let course = sample_course(42, "Calc");
    let entity = course_to_entity(&course, "completed");
    assert_eq!(entity.entity_key, "42");
    let state = entity
        .fields
        .iter()
        .find(|f| f.name == "enrollment_state")
        .and_then(|f| f.value.as_deref());
    assert_eq!(state, Some("completed"));
    assert!(
        entity
            .fields
            .iter()
            .any(|f| f.name == "name" && f.value.as_deref() == Some("Calc"))
    );
}

/// Offline branch of the hit predicate without a network client.
fn serve_offline(
    open: &OpenIdentity,
    dataset: &CoursesDataset,
    now: Timestamp,
) -> Result<super::RefreshOutcome, SyncError> {
    let lookup = open
        .store
        .call_blocking({
            let dataset = dataset.clone();
            move |conns| lookup_dataset(conns, &dataset, now, None)
        })
        .map_err(SyncError::from)?;
    match lookup {
        LookupResult::Hit(row) | LookupResult::Stale(row) if row.complete => {
            Ok(super::RefreshOutcome {
                freshness: super::FreshnessInfo {
                    dataset: row.dataset,
                    scope: row.scope,
                    source: FreshnessSource::Cache,
                    fetched_at: row.fetched_at,
                    complete: row.complete,
                    count: row.count,
                    stale: true,
                },
                requests: 0,
                error: None,
            })
        }
        _ => Err(SyncError::OfflineMiss),
    }
}

fn sample_course(id: i64, name: &str) -> canvas_api::models::Course {
    use canvas_api::Supplied;
    canvas_api::models::Course {
        id,
        name: Supplied::Value(name.to_owned()),
        course_code: Some(format!("CODE{id}")),
        workflow_state: Supplied::Value("available".into()),
        enrollment_state: None,
        term: Some(canvas_api::models::Term {
            id: Some(7),
            name: Some("Fall".into()),
            start_at: None,
            end_at: None,
        }),
        enrollments: Some(vec![canvas_api::models::CourseEnrollment {
            enrollment_type: Some("StudentEnrollment".into()),
            enrollment_state: None,
            computed_current_score: Supplied::Value(95.0),
            computed_final_score: Supplied::Value(90.0),
            computed_current_grade: Supplied::Value("A".into()),
            computed_final_grade: Supplied::Value("A-".into()),
            current_period_computed_current_score: Supplied::Value(92.0),
            current_period_computed_final_score: Supplied::Null,
            current_period_computed_current_grade: Supplied::Value("A".into()),
            current_period_computed_final_grade: Supplied::Absent,
            current_grading_period_id: Some(3),
            current_grading_period_title: Some("Q1".into()),
            ..Default::default()
        }]),
        is_favorite: Some(true),
        restricted: Some(false),
        html_url: Supplied::Absent,
        ..Default::default()
    }
}
