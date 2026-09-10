use super::wire::Observed;
use super::*;
use crate::identity::{IdentityDocument, Paths};
use crate::store::{Dataset, IngestOpts, IngestPage, LookupResult, OpenIdentity, lookup_dataset};
use canvas_api::{Client, Secret, models::Course};
use jiff::Timestamp;
use serde_json::json;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{path, query_param},
};

fn at(seconds: i64) -> Timestamp {
    Timestamp::from_second(seconds).unwrap()
}
fn setup() -> (tempfile::TempDir, OpenIdentity) {
    let dir = tempfile::tempdir().unwrap();
    let doc = IdentityDocument::new("https://canvas.test", 1, "2026-01-01T00:00:00Z");
    let paths = Paths::for_identity(dir.path(), &doc.key);
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
fn opts(epoch_seen: i64) -> IngestOpts<'static> {
    IngestOpts {
        epoch_seen,
        complete: true,
        stale: false,
        error: None,
        window: None,
        contexts: None,
    }
}

#[tokio::test]
async fn real_refresh_empty_cache_offline_and_forced_refresh() {
    let server = MockServer::start().await;
    Mock::given(path("/api/v1/courses"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(2)
        .mount(&server)
        .await;
    let (_dir, open) = setup();
    let client = client(&server);
    let refresh = |fresh, offline| {
        refresh_courses(
            &client,
            &open.store,
            CoursesScope::Active,
            default_ttl_courses(),
            at(100),
            fresh,
            offline,
        )
    };
    assert_eq!(refresh(false, false).await.unwrap().requests, 1);
    let cached = refresh(false, false).await.unwrap();
    assert_eq!(cached.requests, 0);
    assert!(!cached.freshness.stale);
    let offline = refresh(false, true).await.unwrap();
    assert!(offline.freshness.stale);
    assert_eq!(offline.freshness.count, 0);
    assert_eq!(refresh(true, false).await.unwrap().requests, 1);
}

#[tokio::test]
async fn all_three_requests_deduplicate_and_publish_derived_coverage() {
    let server = MockServer::start().await;
    for state in ["active", "completed", "invited_or_pending"] {
        Mock::given(path("/api/v1/courses"))
            .and(query_param("enrollment_state", state))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!([{"id": 1, "course_code": "A", "term": {"id": 7, "name": "Fall"}}]),
            ))
            .expect(1)
            .mount(&server)
            .await;
    }
    let (_dir, open) = setup();
    let out = refresh_courses(
        &client(&server),
        &open.store,
        CoursesScope::All,
        default_ttl_courses(),
        at(100),
        false,
        false,
    )
    .await
    .unwrap();
    assert_eq!(out.requests, 3);
    assert_eq!(out.freshness.count, 1);
    open.store
        .call(|conns| {
            for ds in ["terms", "course_totals"] {
                let complete: bool = conns.cache.query_row(
                    "SELECT complete FROM fetch_log WHERE dataset=?1",
                    [ds],
                    |r| r.get(0),
                )?;
                assert!(complete);
            }
            Ok(())
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn failed_later_page_serves_stale_without_partial_ingest_and_counts_requests() {
    let server = MockServer::start().await;
    let (_dir, open) = setup();
    let client = client(&server);
    Mock::given(path("/api/v1/courses"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!([{"id": 1, "name": "original"}])),
        )
        .mount(&server)
        .await;
    refresh_courses(
        &client,
        &open.store,
        CoursesScope::Active,
        default_ttl_courses(),
        at(100),
        true,
        false,
    )
    .await
    .unwrap();
    server.reset().await;
    Mock::given(path("/api/v1/courses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("link", format!("<{}/page2>; rel=\"next\"", server.uri()))
                .set_body_json(json!([{"id": 1, "name": "partial"}])),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/page2"))
        .respond_with(ResponseTemplate::new(403).set_body_string("private body test-token"))
        .expect(1)
        .mount(&server)
        .await;
    let out = refresh_courses(
        &client,
        &open.store,
        CoursesScope::Active,
        default_ttl_courses(),
        at(200),
        true,
        false,
    )
    .await
    .unwrap();
    assert!(out.freshness.stale);
    assert_eq!(out.requests, 2);
    open.store
        .call(|conns| {
            let name: String =
                conns
                    .cache
                    .query_row("SELECT name FROM courses WHERE id=1", [], |r| r.get(0))?;
            assert_eq!(name, "original");
            let error: String = conns.cache.query_row(
                "SELECT error FROM fetch_log WHERE dataset='courses'",
                [],
                |r| r.get(0),
            )?;
            assert!(!error.contains("private body"));
            assert!(!error.contains("test-token"));
            Ok(())
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn grading_periods_wrapper_is_followed_across_pages() {
    let server = MockServer::start().await;
    Mock::given(path("/api/v1/courses/1/grading_periods"))
        .and(query_param("per_page", "100"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("link", format!("<{}/page2>; rel=\"next\"", server.uri()))
                .set_body_json(json!({"grading_periods": [{"id": 1, "title": "Q1"}], "meta": {}})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/page2"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"grading_periods": [{"id": 2, "title": "Q2"}]})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let (_dir, open) = setup();
    let out = refresh_grading_periods(
        &client(&server),
        &open.store,
        1,
        default_ttl_grades(),
        at(100),
        true,
        false,
    )
    .await
    .unwrap();
    assert_eq!(out.requests, 2);
    assert_eq!(out.freshness.count, 2);
}

#[tokio::test]
async fn per_field_null_absent_and_out_of_order_course_metadata() {
    let (_dir, open) = setup();
    for (time, raw) in [
        (
            30,
            json!({"id":1,"is_favorite":false,"course_code":null,"term":{"id":8,"name":"New"}, "enrollments":[{"type":"student","current_grading_period_id":2,"current_grading_period_title":"Q2"}]}),
        ),
        (
            20,
            json!({"id":1,"is_favorite":true,"course_code":"OLD","term":{"id":7,"name":"Old"},"enrollments":[{"type":"student","computed_current_score":95,"current_grading_period_id":1,"current_grading_period_title":"Q1"}]}),
        ),
        (40, json!({"id":1,"term":{"id":8,"name":null}})),
    ] {
        let observed: Observed<Course> = serde_json::from_value(raw).unwrap();
        let entity = observed.entity("active").await.unwrap();
        open.store
            .call(move |conns| {
                let ds = CoursesDataset::new(CoursesScope::Active, default_ttl_courses());
                ds.ingest(
                    &[IngestPage {
                        fetched_at: at(time),
                        entities: vec![entity],
                    }],
                    &opts(0),
                    conns,
                )
                .map_err(|e| crate::store::DbError::Message(e.to_string()))
            })
            .await
            .unwrap();
    }
    open.store.call(|conns| {
        let row: (Option<String>, i64, bool) = conns.cache.query_row("SELECT course_code,term_id,json_extract(data_json,'$.is_favorite') FROM courses WHERE id=1", [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
        assert_eq!(row, (None,8,false));
        let title: Option<String> = conns.cache.query_row("SELECT name FROM terms WHERE id=8", [], |r| r.get(0))?;
        assert_eq!(title, None);
        let rows: (f64, Option<String>) = conns.cache.query_row("SELECT current_score,json_extract(data_json,'$.period_title') FROM course_totals WHERE mode='all'", [], |r| Ok((r.get(0)?,r.get(1)?)))?;
        assert_eq!(rows, (95.0,None));
        let title: String = conns.cache.query_row("SELECT json_extract(data_json,'$.period_title') FROM course_totals WHERE mode='current'", [], |r| r.get(0))?;
        assert_eq!(title, "Q2");
        Ok(())
    }).await.unwrap();
}

#[test]
fn courses_refresh_guards_derived_totals_epoch() {
    let (_dir, open) = setup();
    open.store
        .call_blocking(|conns| {
            let ds = CoursesDataset::new(CoursesScope::Active, default_ttl_courses());
            let before = ds.current_epoch(&conns.state)?;
            conns.state.execute(
                "INSERT INTO scope_epoch VALUES ('course_totals:course:1', 1)",
                [],
            )?;
            let page = courses_to_ingest_page(
                &[Course {
                    id: 1,
                    ..Default::default()
                }],
                "active",
                at(100),
            );
            assert!(matches!(
                ds.ingest(std::slice::from_ref(&page), &opts(before), conns),
                Err(crate::store::IngestError::Epoch(_))
            ));
            ds.ingest(&[page], &opts(ds.current_epoch(&conns.state)?), conns)
                .unwrap();
            conns.state.execute("UPDATE scope_epoch SET epoch=2", [])?;
            assert!(matches!(
                lookup_dataset(conns, &ds, at(100), None)?,
                LookupResult::Stale(_)
            ));
            Ok(())
        })
        .unwrap();
}
