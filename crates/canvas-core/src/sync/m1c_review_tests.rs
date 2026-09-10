//! M1-c cache regression tests using real ingestion and HTTP fixtures.
use super::wire::Observed;
use super::*;
use crate::identity::{IdentityDocument, Paths};
use crate::store::{Dataset, IngestOpts, IngestPage, OpenIdentity};
use canvas_api::{Client, Secret, models::Assignment};
use jiff::Timestamp;
use serde_json::{Value, json};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{path, query_param},
};
fn at(s: i64) -> Timestamp {
    Timestamp::from_second(s).unwrap()
}
fn setup() -> (tempfile::TempDir, OpenIdentity) {
    let dir = tempfile::tempdir().unwrap();
    let doc = IdentityDocument::new("https://canvas.test", 1, "2026-01-01T00:00:00Z");
    let paths = Paths::for_identity(dir.path(), &doc.key);
    doc.write(&paths.identity_json()).unwrap();
    let open = OpenIdentity::open(&paths, &doc).unwrap();
    (dir, open)
}
fn opts() -> IngestOpts<'static> {
    IngestOpts {
        epoch_seen: 0,
        complete: true,
        stale: false,
        error: None,
        window: None,
        contexts: None,
    }
}
fn client(s: &MockServer) -> Client {
    Client::new(s.uri().parse().unwrap(), Secret::new("secret"), "test").unwrap()
}

#[test]
fn full_thin_null_and_out_of_order_assignment_fields() {
    let (_dir, open) = setup();
    open.store.call_blocking(|conns|{
        let ds=AssignmentsDataset::with_default_ttl(1);
        for (time,raw) in [
            (100,json!({"id":2,"course_id":1,"name":"old","can_submit":false,"locked_for_user":true,"submission":{"workflow_state":"pending_review","score":7,"grade":"B","attempt":1},"workflow_state":"published"})),
            (300,json!({"id":2,"name":"thin"})),
            (200,json!({"id":2,"name":"slow","submission":{"score":9,"grade":null},"locked_for_user":null})),
        ] {
            let row:Observed<Assignment>=serde_json::from_value(raw).unwrap();
            ds.ingest(&[IngestPage{fetched_at:at(time),entities:vec![row.entity(Some(1))]}],&opts(),conns).unwrap();
        }
        let item=crate::todo::load_assignment_item(conns,2,at(301),default_ttl_assignments())?.unwrap();
        assert_eq!(item.title,"thin"); assert_eq!(item.status.score,Some(9.0)); assert_eq!(item.status.grade,None);
        assert_eq!(item.status.submitted,Some(true));assert_eq!(item.status.workflow_state.as_deref(),Some("pending_review"));
        assert_eq!(item.status.locked,None);assert_eq!(item.availability.submittable,Some(false));
        assert_eq!(crate::todo::assignment_observations(conns)?[&2],at(100));
        let old=crate::todo::load_assignment_item(conns,2,at(2001),default_ttl_assignments())?.unwrap();
        assert_eq!(old.availability.submittable,None);
        let row:Observed<Assignment>=serde_json::from_value(json!({"id":2,"can_submit":null,"submission":null})).unwrap();
        ds.ingest(&[IngestPage{fetched_at:at(400),entities:vec![row.entity(Some(1))]}],&opts(),conns).unwrap();
        let cleared=crate::todo::load_assignment_item(conns,2,at(401),default_ttl_assignments())?.unwrap();
        assert_eq!(cleared.status.submitted,None);assert_eq!(cleared.status.score,None);assert_eq!(cleared.availability.submittable,None);
        assert_eq!(crate::todo::assignment_observations(conns)?[&2],at(400));
        Ok(())
    }).unwrap();
}

#[tokio::test]
async fn detail_is_one_request_and_does_not_claim_list_coverage() {
    let server = MockServer::start().await;
    let (_dir, open) = setup();
    Mock::given(path("/api/v1/courses/1/assignments/2"))
        .and(query_param("include[]", "can_submit"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"id":2,"course_id":1,"name":"Detail","can_submit":false})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let c = client(&server);
    assert_eq!(
        refresh_assignment(
            &c,
            &open.store,
            1,
            2,
            default_ttl_assignments(),
            at(100),
            false,
            false
        )
        .await
        .unwrap()
        .requests,
        1
    );
    assert_eq!(
        refresh_assignment(
            &c,
            &open.store,
            1,
            2,
            default_ttl_assignments(),
            at(101),
            false,
            false
        )
        .await
        .unwrap()
        .requests,
        0
    );
    open.store
        .call(|conns| {
            assert!(
                crate::store::load_fetch_log(&conns.cache, "assignments", "course:1")?.is_none()
            );
            assert_eq!(
                crate::todo::load_assignment_item(conns, 2, at(101), default_ttl_assignments())?
                    .unwrap()
                    .availability
                    .submittable,
                Some(false)
            );
            Ok(())
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn submission_dataset_preserves_history_and_safe_read_fields() {
    let server = MockServer::start().await;
    let (_dir, open) = setup();
    let c = client(&server);
    let attachment = json!({"id":"5","display_name":"answer.txt","size":4,"content-type":"text/plain","url":"https://storage.test/?Signature=capability","secret":"never-cache"});
    Mock::given(path("/api/v1/courses/1/assignments/2/submissions/self"))
        .and(query_param("include[]","submission_history")).and(query_param("include[]","submission_comments")).and(query_param("include[]","rubric_assessment"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"assignment_id":2,"user_id":1,"workflow_state":"graded","body":"private raw body","submission_type":"online_text_entry","attachments":[attachment.clone()],"submission_history":[{"attempt":1,"attachments":[attachment]}],"submission_comments":[{"id":7,"comment":"Good","author_name":"Teacher","created_at":"2026-01-01T01:00:00+01:00"}],"rubric_assessment":{"c1":{"points":4,"comments":"Nice","secret":"never-cache"}}})))
        .expect(1).mount(&server).await;
    refresh_submission(
        &c,
        &open.store,
        1,
        2,
        default_ttl_assignments(),
        at(100),
        true,
        false,
    )
    .await
    .unwrap();
    open.store.call(|conns|{
        let (id,raw):(i64,String)=conns.cache.query_row("SELECT s.id,s.data_json FROM membership m JOIN submissions s ON CAST(m.entity_id AS INTEGER)=s.id WHERE m.dataset='submission'",[],|r|Ok((r.get(0)?,r.get(1)?)))?;
        assert_eq!(id,-2);
        for secret in ["capability","never-cache","private raw body"]{assert!(!raw.contains(secret));}
        let v:Value=serde_json::from_str(&raw).unwrap();
        assert_eq!(v["attachments_json"][0]["id"],"5");assert_eq!(v["submission_history_json"][0]["attachments"][0]["id"],"5");
        assert_eq!(v["submission_comments_json"][0]["created_at"],"2026-01-01T00:00:00Z");
        assert_eq!(v["rubric_assessment_json"][0]["criterion_id"],"c1");assert_eq!(v["body_sha256"].as_str().unwrap().len(),64);
        let observation_key:String=conns.cache.query_row("SELECT entity_key FROM field_obs WHERE entity_kind='submission' LIMIT 1",[],|r|r.get(0))?;
        assert_eq!(observation_key,id.to_string());Ok(())
    }).await.unwrap();
}

#[tokio::test]
async fn wider_planner_cache_has_real_exclusive_window_coverage() {
    let server = MockServer::start().await;
    let (_dir, open) = setup();
    let c = client(&server);
    Mock::given(path("/api/v1/planner/items"))
        .and(query_param("start_date", "2026-09-01T00:00:00Z"))
        .and(query_param("end_date", "2026-09-11T00:00:00Z"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .mount(&server)
        .await;
    let wide = PlannerWindow::from_days("2026-09-01".parse().unwrap(), 10);
    let narrow = PlannerWindow::from_days("2026-09-03".parse().unwrap(), 2);
    refresh_planner(
        &c,
        &open.store,
        wide.clone(),
        default_ttl_planner(),
        at(100),
        false,
        false,
    )
    .await
    .unwrap();
    let hit = refresh_planner(
        &c,
        &open.store,
        narrow,
        default_ttl_planner(),
        at(101),
        false,
        true,
    )
    .await
    .unwrap();
    assert_eq!(hit.requests, 0);
    assert_eq!(hit.freshness.scope, wide.scope_key());
    assert!(hit.freshness.stale);
}
