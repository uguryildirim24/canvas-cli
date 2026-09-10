//! Unit tests for the batched window datasets (M4-b).

use std::fs;
use std::sync::Mutex;

use canvas_api::{Client, Secret};
use jiff::civil::date;
use jiff::{Span, Timestamp};
use serde_json::{Value, json};
use tempfile::TempDir;
use wiremock::{
    Mock, MockServer, Request, Respond, ResponseTemplate,
    matchers::{method, path},
};

use crate::identity::{IdentityDocument, Paths};
use crate::store::{LookupResult, OpenIdentity, WindowQuery, lookup_dataset};

use super::announcements::{
    AnnouncementsDataset, announcement_path, announcements_path, course_id_from_context,
    default_ttl_announcements,
};
use super::batched::{refresh_announcements, refresh_calendar_events};
use super::calendar_events::{CalendarEventsDataset, calendar_events_path, default_ttl_calendar};
use super::context_window::ContextWindow;
use super::outcome::{FreshnessSource, SyncError};
use super::planner::PlannerWindow;

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

fn window() -> PlannerWindow {
    PlannerWindow {
        start: date(2026, 9, 1),
        end: date(2026, 9, 14),
    }
}

/// Canvas answers a batch as a unit: one unreadable context fails all ten.
struct BatchResponder {
    denied: Vec<String>,
    /// Every batch this responder was asked for, in order.
    seen: Mutex<Vec<Vec<String>>>,
    body: fn(&str) -> Value,
}

impl BatchResponder {
    fn new(denied: &[&str], body: fn(&str) -> Value) -> Self {
        Self {
            denied: denied.iter().map(|s| (*s).to_owned()).collect(),
            seen: Mutex::new(Vec::new()),
            body,
        }
    }

    fn contexts_of(req: &Request) -> Vec<String> {
        req.url
            .query_pairs()
            .filter(|(key, _)| key == "context_codes[]")
            .map(|(_, value)| value.into_owned())
            .collect()
    }
}

impl Respond for BatchResponder {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let codes = Self::contexts_of(req);
        self.seen.lock().unwrap().push(codes.clone());
        if codes.iter().any(|code| self.denied.contains(code)) {
            return ResponseTemplate::new(403).set_body_string("user not authorized");
        }
        let items: Vec<Value> = codes.iter().map(|code| (self.body)(code)).collect();
        ResponseTemplate::new(200).set_body_json(items)
    }
}

fn announcement_body(code: &str) -> Value {
    let id = course_id_from_context(code).unwrap_or(0);
    json!({
        "id": id * 10,
        "context_code": code,
        "title": format!("Notice for {code}"),
        "message": "<p>Hello</p>",
        "posted_at": "2026-09-05T12:00:00Z",
        "read_state": if id % 2 == 0 { "read" } else { "unread" },
        "html_url": format!("https://courses.example.test/courses/{id}/discussion_topics/{}", id * 10),
    })
}

fn event_body(code: &str) -> Value {
    let id = course_id_from_context(code).unwrap_or(999);
    json!({
        "id": id * 100,
        "title": format!("Event in {code}"),
        "context_code": code,
        "start_at": "2026-09-06T15:00:00Z",
        "end_at": "2026-09-06T16:00:00Z",
        "workflow_state": "active",
    })
}

#[test]
fn request_paths_carry_the_window_and_the_batch() {
    let cw = ContextWindow::courses(window(), &[7, 8]);
    let batch = cw.batches().remove(0);
    let announcements = announcements_path(&batch, &cw);
    assert!(announcements.starts_with("/api/v1/announcements?"));
    assert!(announcements.contains("start_date=2026-09-01T00:00:00Z"));
    assert!(announcements.contains("end_date=2026-09-15T00:00:00Z"));
    assert!(announcements.contains("&context_codes[]=course_7&context_codes[]=course_8"));

    let events = calendar_events_path(&batch, &cw);
    assert!(events.starts_with("/api/v1/calendar_events?type=event"));
    assert!(events.contains("&context_codes[]=course_7"));

    assert_eq!(
        announcement_path(5, 90),
        "/api/v1/courses/5/discussion_topics/90"
    );
    assert_eq!(course_id_from_context("course_45679"), Some(45679));
    assert_eq!(course_id_from_context("user_1"), None);
}

#[tokio::test]
async fn a_failed_batch_is_isolated_and_the_other_batches_are_served() {
    let server = MockServer::start().await;
    let responder = std::sync::Arc::new(BatchResponder::new(
        &["course_12"],
        announcement_body as fn(&str) -> Value,
    ));
    Mock::given(method("GET"))
        .and(path("/api/v1/announcements"))
        .respond_with(BatchProxy(responder.clone()))
        .mount(&server)
        .await;

    let (_dir, open) = setup();
    let ids: Vec<i64> = (1..=12).collect();
    let cw = ContextWindow::courses(window(), &ids);
    let out = refresh_announcements(
        &client(&server),
        &open.store,
        cw.clone(),
        default_ttl_announcements(),
        ts(1000),
        true,
        false,
    )
    .await
    .unwrap();

    // Batch one (ten courses) answered; batch two failed and was retried one
    // course at a time.
    let seen = responder.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 4);
    assert_eq!(seen[0].len(), 10);
    assert_eq!(seen[1], ["course_11", "course_12"]);
    assert_eq!(seen[2], ["course_11"]);
    assert_eq!(seen[3], ["course_12"]);
    assert_eq!(out.outcome.requests, 4);

    // Coverage stays complete: every batch was stored or isolated.
    assert_eq!(out.outcome.freshness.source, FreshnessSource::Network);
    assert!(out.outcome.freshness.complete);
    assert!(!out.outcome.freshness.stale);
    assert_eq!(out.outcome.freshness.count, 11);

    // The course that still failed is named, and no other course is.
    assert_eq!(out.denials.len(), 1);
    assert_eq!(out.denials[0].context, "course_12");
    assert_eq!(out.denials[0].http_status, 403);
    assert_eq!(out.denials[0].course_id(), Some(12));

    let stored = open
        .store
        .call_blocking({
            let scope = cw.scope_key().to_owned();
            move |conns| {
                let mut stmt = conns.cache.prepare(
                    "SELECT entity_id FROM membership WHERE dataset='announcements' AND scope=?1 ORDER BY position",
                )?;
                let ids = stmt
                    .query_map([scope], |r| r.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(ids)
            }
        })
        .unwrap();
    assert_eq!(stored.len(), 11);
    assert!(!stored.contains(&"120".to_owned()));

    // A later cached read still reports the denied course.
    let cached = refresh_announcements(
        &client(&server),
        &open.store,
        cw,
        default_ttl_announcements(),
        ts(1010),
        false,
        false,
    )
    .await
    .unwrap();
    assert_eq!(cached.outcome.freshness.source, FreshnessSource::Cache);
    assert_eq!(cached.outcome.requests, 0);
    assert_eq!(cached.denials, out.denials);
}

/// `Respond` on a shared responder, so the test can read what was asked.
struct BatchProxy(std::sync::Arc<BatchResponder>);

impl Respond for BatchProxy {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        self.0.respond(req)
    }
}

#[tokio::test]
async fn a_window_hit_needs_the_same_context_hash() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/announcements"))
        .respond_with(BatchProxy(std::sync::Arc::new(BatchResponder::new(
            &[],
            announcement_body as fn(&str) -> Value,
        ))))
        .mount(&server)
        .await;
    let (_dir, open) = setup();
    let two = ContextWindow::courses(window(), &[1, 2]);
    refresh_announcements(
        &client(&server),
        &open.store,
        two.clone(),
        default_ttl_announcements(),
        ts(1000),
        true,
        false,
    )
    .await
    .unwrap();

    let hit = open
        .store
        .call_blocking({
            let cw = two.clone();
            move |conns| {
                lookup_dataset(
                    conns,
                    &AnnouncementsDataset::new(cw.clone(), default_ttl_announcements()),
                    ts(1001),
                    Some(WindowQuery {
                        start: cw.window().start_timestamp(),
                        end: cw.window().end_timestamp(),
                        contexts: cw.context_hash(),
                    }),
                )
            }
        })
        .unwrap();
    assert!(matches!(hit, LookupResult::Hit(_)));

    // The same window over one more course is different coverage.
    let three = ContextWindow::courses(window(), &[1, 2, 3]);
    let miss = open
        .store
        .call_blocking({
            let cw = three.clone();
            move |conns| {
                lookup_dataset(
                    conns,
                    &AnnouncementsDataset::new(cw.clone(), default_ttl_announcements()),
                    ts(1001),
                    Some(WindowQuery {
                        start: cw.window().start_timestamp(),
                        end: cw.window().end_timestamp(),
                        contexts: cw.context_hash(),
                    }),
                )
            }
        })
        .unwrap();
    assert!(matches!(miss, LookupResult::Miss));

    // A narrower window inside the stored one is a hit; a wider one is not.
    let inside = ContextWindow::courses(
        PlannerWindow {
            start: date(2026, 9, 2),
            end: date(2026, 9, 10),
        },
        &[1, 2],
    );
    let outside = ContextWindow::courses(
        PlannerWindow {
            start: date(2026, 8, 20),
            end: date(2026, 9, 14),
        },
        &[1, 2],
    );
    for (cw, want_hit) in [(inside, true), (outside, false)] {
        let looked = open
            .store
            .call_blocking({
                let cw = cw.clone();
                move |conns| {
                    lookup_dataset(
                        conns,
                        &AnnouncementsDataset::new(cw.clone(), default_ttl_announcements()),
                        ts(1001),
                        Some(WindowQuery {
                            start: cw.window().start_timestamp(),
                            end: cw.window().end_timestamp(),
                            contexts: cw.context_hash(),
                        }),
                    )
                }
            })
            .unwrap();
        assert_eq!(matches!(looked, LookupResult::Hit(_)), want_hit);
    }
}

#[tokio::test]
async fn calendar_events_cover_every_context_and_page() {
    use wiremock::matchers::query_param;
    let server = MockServer::start().await;
    Mock::given(path("/api/v1/calendar_events"))
        .and(query_param("page", "2"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(
                json!([{"id": 9001, "title": "Page two", "context_code": "user_1"}]),
            ),
        )
        .with_priority(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/api/v1/calendar_events"))
        .and(query_param("context_codes[]", "user_1"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header(
                    "Link",
                    format!(
                        "<{}/api/v1/calendar_events?page=2>; rel=\"next\"",
                        server.uri()
                    ),
                )
                .set_body_json(
                    json!([{"id": 9000, "title": "Page one", "context_code": "user_1"}]),
                ),
        )
        .with_priority(2)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/api/v1/calendar_events"))
        .respond_with(BatchProxy(std::sync::Arc::new(BatchResponder::new(
            &[],
            event_body as fn(&str) -> Value,
        ))))
        .with_priority(3)
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, open) = setup();
    let mut contexts: Vec<String> = (1..=10).map(|id| format!("course_{id}")).collect();
    contexts.push("user_1".into());
    let cw = ContextWindow::contexts(window(), &contexts);
    assert_eq!(cw.batches().len(), 2);

    let out = refresh_calendar_events(
        &client(&server),
        &open.store,
        cw.clone(),
        default_ttl_calendar(),
        ts(2000),
        true,
        false,
    )
    .await
    .unwrap();
    // Ten courses in the first batch, then `user_1` alone across two pages.
    assert_eq!(out.outcome.requests, 3);
    assert_eq!(out.outcome.freshness.count, 12);
    assert!(out.denials.is_empty());

    let titles = open
        .store
        .call_blocking({
            let scope = cw.scope_key().to_owned();
            move |conns| {
                let mut stmt = conns.cache.prepare(
                    "SELECT c.title FROM membership m JOIN calendar_events c ON c.id = CAST(m.entity_id AS INTEGER)
                     WHERE m.dataset='calendar_events' AND m.scope=?1 AND c.context_code='user_1' ORDER BY c.id",
                )?;
                let out = stmt
                    .query_map([scope], |r| r.get::<_, Option<String>>(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(out)
            }
        })
        .unwrap();
    assert_eq!(
        titles,
        [Some("Page one".to_owned()), Some("Page two".to_owned())]
    );

    let _ = CalendarEventsDataset::new(cw, default_ttl_calendar());
}

#[tokio::test]
async fn auth_and_throttling_abort_the_whole_refresh() {
    for status in [401, 403] {
        let server = MockServer::start().await;
        let body = if status == 401 {
            String::new()
        } else {
            json!({"errors":[{"message":"403 Forbidden (Rate Limit Exceeded)"}]}).to_string()
        };
        Mock::given(path("/api/v1/announcements"))
            .respond_with(
                // `Retry-After: 0` keeps the client's throttle backoff out of
                // the test clock; the classification is what matters here.
                ResponseTemplate::new(status)
                    .insert_header("Retry-After", "0")
                    .set_body_string(body),
            )
            .mount(&server)
            .await;
        let (_dir, open) = setup();
        let err = refresh_announcements(
            &client(&server),
            &open.store,
            ContextWindow::courses(window(), &[1]),
            default_ttl_announcements(),
            ts(1000),
            true,
            false,
        )
        .await
        .unwrap_err();
        assert!(
            matches!(
                err,
                SyncError::Api(canvas_api::Error::Unauthorized | canvas_api::Error::RateLimited)
            ),
            "status {status} gave {err:?}"
        );
    }
}

#[tokio::test]
async fn offline_without_a_row_is_an_offline_miss() {
    let server = MockServer::start().await;
    let (_dir, open) = setup();
    let err = refresh_calendar_events(
        &client(&server),
        &open.store,
        ContextWindow::contexts(window(), &["user_1".into()]),
        default_ttl_calendar(),
        ts(1000),
        false,
        true,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, SyncError::OfflineMiss));
}

#[tokio::test]
async fn a_transport_failure_serves_the_stale_row_instead_of_dropping_it() {
    let server = MockServer::start().await;
    Mock::given(path("/api/v1/announcements"))
        .respond_with(BatchProxy(std::sync::Arc::new(BatchResponder::new(
            &[],
            announcement_body as fn(&str) -> Value,
        ))))
        .expect(1)
        .mount(&server)
        .await;
    let (_dir, open) = setup();
    let cw = ContextWindow::courses(window(), &[4]);
    let first = refresh_announcements(
        &client(&server),
        &open.store,
        cw.clone(),
        default_ttl_announcements(),
        ts(1000),
        true,
        false,
    )
    .await
    .unwrap();
    assert_eq!(first.outcome.freshness.count, 1);

    server.reset().await;
    Mock::given(path("/api/v1/announcements"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not JSON"))
        .mount(&server)
        .await;
    let stale = refresh_announcements(
        &client(&server),
        &open.store,
        cw,
        default_ttl_announcements(),
        ts(1000) + Span::new().hours(3),
        true,
        false,
    )
    .await
    .unwrap();
    assert_eq!(stale.outcome.freshness.source, FreshnessSource::Cache);
    assert!(stale.outcome.freshness.stale);
    assert_eq!(stale.outcome.freshness.count, 1);
}
