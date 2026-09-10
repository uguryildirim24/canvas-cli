//! Unit tests for the grades datasets (M4-a, SPEC §10 / §12.4 / §16).

use std::fs;

use canvas_api::models::{Assignment, AssignmentGroup, AssignmentGroupRules, Enrollment};
use canvas_api::{Client, Secret, Supplied};
use jiff::{Span, Timestamp};
use serde_json::{Value, json};
use tempfile::TempDir;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, query_param},
};

use crate::identity::{IdentityDocument, Paths};
use crate::store::{Dataset, IngestOpts, IngestPage, OpenIdentity, read_scope_epoch};

use super::assignment_groups::{
    AssignmentGroupsDataset, assignment_group_to_entity, assignment_groups_path,
};
use super::enrollment_grades::{EnrollmentGradesDataset, PeriodKey, enrollment_to_entity};
use super::refresh::{refresh_assignment_groups, refresh_grading_periods};

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

fn client(server: &MockServer) -> Client {
    Client::new(
        server.uri().parse().unwrap(),
        Secret::new("test-token"),
        "canvas-cli/test",
    )
    .unwrap()
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

/// Ingest one page into a dataset at `fetched_at`.
fn ingest(open: &OpenIdentity, ds: &(impl Dataset + Clone + Send + 'static), page: IngestPage) {
    let ds = ds.clone();
    open.store
        .call_blocking(move |conns| {
            let epoch = read_scope_epoch(&conns.state, &ds.epoch_scope())?;
            ds.ingest(&[page], &ingest_ok(epoch), conns)
                .map_err(|e| crate::store::DbError::Message(e.to_string()))
        })
        .unwrap();
}

fn group(id: i64, name: &str, assignments: Option<Vec<Assignment>>) -> AssignmentGroup {
    AssignmentGroup {
        id,
        name: Supplied::Value(name.to_owned()),
        position: Supplied::Value(1),
        group_weight: Supplied::Value(40.0),
        course_id: Some(7),
        rules: Some(AssignmentGroupRules {
            drop_lowest: Some(1),
            drop_highest: None,
            never_drop: Some(vec![31]),
        }),
        assignments,
    }
}

fn assignment(id: i64, name: &str) -> Assignment {
    Assignment {
        id,
        name: Supplied::Value(name.to_owned()),
        points_possible: Supplied::Value(25.0),
        ..Assignment::default()
    }
}

fn data_json(open: &OpenIdentity, group_id: i64) -> Value {
    open.store
        .call_blocking(move |conns| {
            let raw: String = conns.cache.query_row(
                "SELECT data_json FROM assignment_groups WHERE id = ?1",
                [group_id],
                |r| r.get(0),
            )?;
            Ok(serde_json::from_str(&raw).unwrap())
        })
        .unwrap()
}

// --- the path carries the period ---

#[test]
fn the_group_path_adds_grading_period_id_only_for_an_explicit_period() {
    let none = assignment_groups_path(7, PeriodKey::None);
    assert!(none.contains("include[]=assignments"));
    assert!(none.contains("include[]=submission"));
    assert!(none.contains("override_assignment_dates=true"));
    assert!(!none.contains("grading_period_id"));

    let five = assignment_groups_path(7, PeriodKey::Id(5));
    assert!(five.contains("grading_period_id=5"), "{five}");
}

#[test]
fn the_group_scope_is_per_course_and_period() {
    let ds = AssignmentGroupsDataset::new(7, PeriodKey::Id(5), Span::new().minutes(10));
    assert_eq!(ds.scope_key(), "course:7:period:5");
    // The journal bumps `assignment_groups:course:7:*`, which prefix-matches.
    assert_eq!(ds.epoch_scope(), "assignment_groups:course:7:period:5");
    let none = AssignmentGroupsDataset::new(7, PeriodKey::None, Span::new().minutes(10));
    assert_eq!(none.scope_key(), "course:7:period:none");
}

// --- period scoping of the assignment list ---

#[test]
fn each_period_keeps_its_own_assignment_list_and_clock() {
    let (_dir, open) = setup();
    let ttl = Span::new().minutes(10);
    let none = AssignmentGroupsDataset::new(7, PeriodKey::None, ttl);
    let five = AssignmentGroupsDataset::new(7, PeriodKey::Id(5), ttl);

    // Whole course: two assignments.
    ingest(
        &open,
        &none,
        IngestPage {
            fetched_at: ts(10),
            entities: vec![assignment_group_to_entity(
                &group(
                    20,
                    "Homework",
                    Some(vec![assignment(31, "PS1"), assignment(32, "PS2")]),
                ),
                7,
                ts(10),
            )],
        },
    );
    // Period 5: only the first.
    ingest(
        &open,
        &five,
        IngestPage {
            fetched_at: ts(20),
            entities: vec![assignment_group_to_entity(
                &group(20, "Homework", Some(vec![assignment(31, "PS1")])),
                7,
                ts(20),
            )],
        },
    );

    let data = data_json(&open, 20);
    let by_period = &data["assignments_by_period"];
    assert_eq!(
        by_period["none"]["assignments"].as_array().unwrap().len(),
        2,
        "period none keeps both: {data}"
    );
    assert_eq!(
        by_period["5"]["assignments"].as_array().unwrap().len(),
        1,
        "period 5 keeps one: {data}"
    );
    // Each period carries its own observation clock.
    assert_eq!(by_period["none"]["observed_at"], ts(10).to_string());
    assert_eq!(by_period["5"]["observed_at"], ts(20).to_string());
}

#[test]
fn an_out_of_order_arrival_never_replaces_a_newer_list_for_its_period() {
    let (_dir, open) = setup();
    let ttl = Span::new().minutes(10);
    let none = AssignmentGroupsDataset::new(7, PeriodKey::None, ttl);

    // Newer response arrives first.
    ingest(
        &open,
        &none,
        IngestPage {
            fetched_at: ts(30),
            entities: vec![assignment_group_to_entity(
                &group(
                    20,
                    "Homework",
                    Some(vec![assignment(31, "PS1"), assignment(32, "PS2")]),
                ),
                7,
                ts(30),
            )],
        },
    );
    // A slower, older response must not win.
    ingest(
        &open,
        &none,
        IngestPage {
            fetched_at: ts(20),
            entities: vec![assignment_group_to_entity(
                &group(20, "Homework", Some(vec![assignment(31, "PS1")])),
                7,
                ts(20),
            )],
        },
    );

    let data = data_json(&open, 20);
    assert_eq!(
        data["assignments_by_period"]["none"]["assignments"]
            .as_array()
            .unwrap()
            .len(),
        2,
        "the t=30 list must survive: {data}"
    );
    assert_eq!(
        data["assignments_by_period"]["none"]["observed_at"],
        ts(30).to_string()
    );
    // A skipped write must not drag the group's detail clock backwards.
    let observed: Option<String> = open
        .store
        .call_blocking(|conns| {
            Ok(conns.cache.query_row(
                "SELECT observed_at_detail FROM assignment_groups WHERE id = 20",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(observed.as_deref(), Some(ts(30).to_string().as_str()));
}

#[test]
fn an_absent_assignment_list_leaves_the_stored_one_alone() {
    let (_dir, open) = setup();
    let ttl = Span::new().minutes(10);
    let none = AssignmentGroupsDataset::new(7, PeriodKey::None, ttl);
    ingest(
        &open,
        &none,
        IngestPage {
            fetched_at: ts(10),
            entities: vec![assignment_group_to_entity(
                &group(20, "Homework", Some(vec![assignment(31, "PS1")])),
                7,
                ts(10),
            )],
        },
    );
    // A later response without `assignments` says nothing about the list.
    ingest(
        &open,
        &none,
        IngestPage {
            fetched_at: ts(40),
            entities: vec![assignment_group_to_entity(
                &group(20, "Homework", None),
                7,
                ts(40),
            )],
        },
    );
    let data = data_json(&open, 20);
    assert_eq!(
        data["assignments_by_period"]["none"]["assignments"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "{data}"
    );
    assert_eq!(
        data["assignments_by_period"]["none"]["observed_at"],
        ts(10).to_string(),
        "an absent field never refreshes the clock: {data}"
    );
}

#[test]
fn group_columns_and_rules_are_projected() {
    let (_dir, open) = setup();
    let ttl = Span::new().minutes(10);
    let none = AssignmentGroupsDataset::new(7, PeriodKey::None, ttl);
    ingest(
        &open,
        &none,
        IngestPage {
            fetched_at: ts(10),
            entities: vec![assignment_group_to_entity(
                &group(20, "Homework", Some(vec![])),
                7,
                ts(10),
            )],
        },
    );
    let (name, position, weight, rules): (String, i64, f64, String) = open
        .store
        .call_blocking(|conns| {
            Ok(conns.cache.query_row(
                "SELECT name, position, group_weight, rules_json
                 FROM assignment_groups WHERE id = 20",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?)
        })
        .unwrap();
    assert_eq!(name, "Homework");
    assert_eq!(position, 1);
    assert!((weight - 40.0).abs() < f64::EPSILON);
    let rules: Value = serde_json::from_str(&rules).unwrap();
    assert_eq!(rules["drop_lowest"], 1);
    assert!(rules["drop_highest"].is_null());
    assert_eq!(rules["never_drop"][0], "31");
}

// --- scoped enrollment values: P → Q → P with out-of-order observations ---

#[test]
fn scoped_enrollment_values_keep_each_period_separate() {
    let (_dir, open) = setup();
    let ttl = Span::new().minutes(10);
    let p = EnrollmentGradesDataset::new(PeriodKey::Id(5), ttl);
    let q = EnrollmentGradesDataset::new(PeriodKey::Id(6), ttl);

    let enrollment = |score: f64| Enrollment {
        id: 900,
        course_id: Some(7),
        computed_current_score: Supplied::Value(score),
        ..Enrollment::default()
    };

    // P at t=10, then Q at t=20, then a late P observation at t=30.
    ingest(
        &open,
        &p,
        IngestPage {
            fetched_at: ts(10),
            entities: vec![enrollment_to_entity(&enrollment(70.0), "5")],
        },
    );
    ingest(
        &open,
        &q,
        IngestPage {
            fetched_at: ts(20),
            entities: vec![enrollment_to_entity(&enrollment(80.0), "6")],
        },
    );
    ingest(
        &open,
        &p,
        IngestPage {
            fetched_at: ts(30),
            entities: vec![enrollment_to_entity(&enrollment(75.0), "5")],
        },
    );

    let scores: Vec<(String, Option<f64>)> = open
        .store
        .call_blocking(|conns| {
            let mut stmt = conns.cache.prepare(
                "SELECT period, current_score FROM enrollment_grades
                 WHERE enrollment_id = 900 ORDER BY period",
            )?;
            let rows = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .unwrap();
    assert_eq!(
        scores,
        vec![("5".to_owned(), Some(75.0)), ("6".to_owned(), Some(80.0))],
        "each period keeps its own value"
    );

    // The composite observation key keeps the clocks apart.
    let observed: Vec<(String, String)> = open
        .store
        .call_blocking(|conns| {
            let mut stmt = conns.cache.prepare(
                "SELECT entity_key, observed_at FROM field_obs
                 WHERE entity_kind = 'enrollment_grades' AND field = 'current_score'
                 ORDER BY entity_key",
            )?;
            let rows = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .unwrap();
    assert_eq!(
        observed,
        vec![
            ("900|5".to_owned(), ts(30).to_string()),
            ("900|6".to_owned(), ts(20).to_string()),
        ],
        "a period-P observation never freshens period Q"
    );
}

/// SPEC §16: out-of-order P (t=30) then Q (t=20) on the same enrollment.
/// The composite observation key keeps them apart, so P's newer clock can
/// never suppress the older Q write.
#[test]
fn a_newer_observation_for_one_period_never_suppresses_an_older_one_for_another() {
    let (_dir, open) = setup();
    let ttl = Span::new().minutes(10);
    let p = EnrollmentGradesDataset::new(PeriodKey::Id(5), ttl);
    let q = EnrollmentGradesDataset::new(PeriodKey::Id(6), ttl);
    let enrollment = |score: f64| Enrollment {
        id: 900,
        course_id: Some(7),
        computed_current_score: Supplied::Value(score),
        ..Enrollment::default()
    };

    // P arrives first and carries the newer clock.
    ingest(
        &open,
        &p,
        IngestPage {
            fetched_at: ts(30),
            entities: vec![enrollment_to_entity(&enrollment(75.0), "5")],
        },
    );
    // Q arrives second with an older clock and must still be written.
    ingest(
        &open,
        &q,
        IngestPage {
            fetched_at: ts(20),
            entities: vec![enrollment_to_entity(&enrollment(80.0), "6")],
        },
    );

    let rows: Vec<(String, Option<f64>, String)> = open
        .store
        .call_blocking(|conns| {
            let mut stmt = conns.cache.prepare(
                "SELECT g.period, g.current_score, o.observed_at
                 FROM enrollment_grades g
                 JOIN field_obs o
                   ON o.entity_kind = 'enrollment_grades'
                  AND o.entity_key = g.enrollment_id || '|' || g.period
                  AND o.field = 'current_score'
                 WHERE g.enrollment_id = 900 ORDER BY g.period",
            )?;
            let rows = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .unwrap();
    assert_eq!(
        rows,
        vec![
            ("5".to_owned(), Some(75.0), ts(30).to_string()),
            ("6".to_owned(), Some(80.0), ts(20).to_string()),
        ],
        "period Q keeps its own value and its own older clock"
    );
}

#[test]
fn an_older_arrival_does_not_overwrite_a_newer_value_in_the_same_period() {
    let (_dir, open) = setup();
    let ttl = Span::new().minutes(10);
    let p = EnrollmentGradesDataset::new(PeriodKey::Id(5), ttl);
    let enrollment = |score: f64| Enrollment {
        id: 900,
        course_id: Some(7),
        computed_current_score: Supplied::Value(score),
        ..Enrollment::default()
    };
    ingest(
        &open,
        &p,
        IngestPage {
            fetched_at: ts(30),
            entities: vec![enrollment_to_entity(&enrollment(75.0), "5")],
        },
    );
    ingest(
        &open,
        &p,
        IngestPage {
            fetched_at: ts(20),
            entities: vec![enrollment_to_entity(&enrollment(70.0), "5")],
        },
    );
    let score: Option<f64> = open
        .store
        .call_blocking(|conns| {
            Ok(conns.cache.query_row(
                "SELECT current_score FROM enrollment_grades
                 WHERE enrollment_id = 900 AND period = '5'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(score, Some(75.0));
}

// --- wrapped grading-period pagination ---

#[tokio::test]
async fn the_grading_periods_wrapper_is_followed_across_two_pages() {
    let server = MockServer::start().await;
    let (_dir, open) = setup();

    let next = format!("{}/api/v1/courses/7/grading_periods?page=2", server.uri());
    Mock::given(method("GET"))
        .and(path("/api/v1/courses/7/grading_periods"))
        .and(query_param("per_page", "100"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({
                    "grading_periods": [
                        {"id": "5", "title": "Fall Term 1",
                         "start_date": "2026-09-01T00:00:00Z", "end_date": "2026-10-31T00:00:00Z"}
                    ],
                    "meta": {"primaryCollection": "grading_periods"}
                }))
                .insert_header("Link", format!("<{next}>; rel=\"next\"").as_str()),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/courses/7/grading_periods"))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "grading_periods": [
                {"id": "6", "title": "Fall Term 2",
                 "start_date": "2026-11-01T00:00:00Z", "end_date": "2026-12-20T00:00:00Z"}
            ]
        })))
        .mount(&server)
        .await;

    let outcome = refresh_grading_periods(
        &client(&server),
        &open.store,
        7,
        Span::new().minutes(10),
        ts(100),
        false,
        false,
    )
    .await
    .unwrap();
    assert_eq!(outcome.freshness.count, 2, "both pages ingested");
    assert!(outcome.freshness.complete);

    let titles: Vec<String> = open
        .store
        .call_blocking(|conns| {
            let mut stmt = conns
                .cache
                .prepare("SELECT title FROM grading_periods ORDER BY id")?;
            let rows = stmt
                .query_map([], |r| r.get(0))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .unwrap();
    assert_eq!(titles, vec!["Fall Term 1", "Fall Term 2"]);
}

#[tokio::test]
async fn a_group_refresh_sends_the_period_and_stores_the_inline_assignments() {
    let server = MockServer::start().await;
    let (_dir, open) = setup();

    Mock::given(method("GET"))
        .and(path("/api/v1/courses/7/assignment_groups"))
        .and(query_param("grading_period_id", "5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {
                "id": "20",
                "name": "Homework",
                "position": 1,
                "group_weight": 40.0,
                "rules": {"drop_lowest": 1, "never_drop": ["31"]},
                "assignments": [
                    {"id": "31", "name": "PS1", "points_possible": 25.0,
                     "due_at": "2026-09-14T03:59:00Z", "omit_from_final_grade": false,
                     "submission": {"score": 23.0, "grade": "23", "workflow_state": "graded"}}
                ]
            }
        ])))
        .mount(&server)
        .await;

    let outcome = refresh_assignment_groups(
        &client(&server),
        &open.store,
        7,
        PeriodKey::Id(5),
        Span::new().minutes(10),
        ts(100),
        false,
        false,
    )
    .await
    .unwrap();
    assert_eq!(outcome.freshness.count, 1);
    assert_eq!(outcome.freshness.scope, "course:7:period:5");

    let data = data_json(&open, 20);
    let stored = &data["assignments_by_period"]["5"]["assignments"][0];
    assert_eq!(stored["id"], "31");
    assert_eq!(stored["name"], "PS1");
    assert_eq!(stored["score"], 23.0);
    assert_eq!(stored["workflow_state"], "graded");
    // The projection is an allowlist: no capability-bearing fields ride along.
    assert!(stored.get("html_url").is_none(), "{stored}");
    assert!(stored.get("submissions_download_url").is_none(), "{stored}");
}

#[tokio::test]
async fn an_explicit_null_group_field_clears_the_stored_value() {
    let server = MockServer::start().await;
    let (_dir, open) = setup();
    Mock::given(method("GET"))
        .and(path("/api/v1/courses/7/assignment_groups"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id": "20", "name": "Homework", "position": 1, "group_weight": 40.0, "rules": {}}
        ])))
        .mount(&server)
        .await;
    let ttl = Span::new().minutes(10);
    refresh_assignment_groups(
        &client(&server),
        &open.store,
        7,
        PeriodKey::None,
        ttl,
        ts(100),
        false,
        false,
    )
    .await
    .unwrap();
    server.reset().await;

    // A later payload with an explicit null weight must clear it, not keep 40.
    Mock::given(method("GET"))
        .and(path("/api/v1/courses/7/assignment_groups"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id": "20", "name": "Homework", "position": 1, "group_weight": null, "rules": {}}
        ])))
        .mount(&server)
        .await;
    refresh_assignment_groups(
        &client(&server),
        &open.store,
        7,
        PeriodKey::None,
        ttl,
        ts(200),
        true,
        false,
    )
    .await
    .unwrap();

    let weight: Option<f64> = open
        .store
        .call_blocking(|conns| {
            Ok(conns.cache.query_row(
                "SELECT group_weight FROM assignment_groups WHERE id = 20",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(weight, None, "an explicit null overwrites");
}
