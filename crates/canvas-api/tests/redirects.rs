//! API redirect behaviour tests.

mod common;

use canvas_api::Error;
use serde::Deserialize;
use serde_json::json;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[derive(Debug, Deserialize)]
struct OkBody {
    ok: bool,
}

#[tokio::test]
async fn same_origin_303_get_follows_to_final() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/start"))
        .respond_with(ResponseTemplate::new(303).insert_header("Location", "/final"))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/final"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
        .expect(1)
        .mount(&server)
        .await;

    let client = common::test_client(&server);
    let body: OkBody = client.get("/start").await.expect("get");
    assert!(body.ok);
}

#[tokio::test]
async fn same_origin_307_post_preserves_method_and_body() {
    let server = MockServer::start().await;
    let payload = json!({"n": 1});

    Mock::given(method("POST"))
        .and(path("/start"))
        .respond_with(ResponseTemplate::new(307).insert_header("Location", "/final"))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/final"))
        .and(body_json(&payload))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
        .expect(1)
        .mount(&server)
        .await;

    let client = common::test_client(&server);
    let body: OkBody = client.post("/start", &payload).await.expect("post");
    assert!(body.ok);
}

#[tokio::test]
async fn off_origin_redirect_is_cross_origin() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/start"))
        .respond_with(
            ResponseTemplate::new(303).insert_header("Location", "https://evil.example/x"),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = common::test_client(&server);
    let err = client
        .get::<OkBody>("/start")
        .await
        .expect_err("cross origin");
    assert!(matches!(err, Error::CrossOrigin));
}

#[tokio::test]
async fn post_301_is_unexpected_redirect() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/start"))
        .respond_with(ResponseTemplate::new(301).insert_header("Location", "/final"))
        .expect(1)
        .mount(&server)
        .await;

    let client = common::test_client(&server);
    let err = client
        .post::<OkBody, _>("/start", &json!({}))
        .await
        .expect_err("unexpected redirect");
    assert!(matches!(err, Error::UnexpectedRedirect));
}
