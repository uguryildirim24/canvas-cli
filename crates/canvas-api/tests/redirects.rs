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

#[tokio::test]
async fn every_redirect_hop_has_a_new_admission_and_observation() {
    let server = MockServer::start().await;
    Mock::given(path("/start"))
        .respond_with(
            ResponseTemplate::new(303)
                .insert_header("Location", "/final")
                .insert_header("X-Rate-Limit-Remaining", "400")
                .insert_header("X-Request-Cost", "2"),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/final"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok":true}))
                .insert_header("X-Rate-Limit-Remaining", "500")
                .insert_header("X-Request-Cost", "3"),
        )
        .expect(1)
        .mount(&server)
        .await;
    let client = common::test_client(&server);
    client.get::<OkBody>("/start").await.unwrap();
    assert_eq!(client.telemetry().api, 2);
    assert_eq!(client.telemetry().cost, Some(5.0));
    assert_eq!(client.governor().watermark(), 2);
    assert!(client.governor().estimate() >= 500.0);
}

#[tokio::test]
async fn post_303_drops_body_and_content_headers_but_cannot_then_follow_302() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/start"))
        .respond_with(ResponseTemplate::new(303).insert_header("Location", "/second"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/second"))
        .respond_with(ResponseTemplate::new(302).insert_header("Location", "/final"))
        .expect(1)
        .mount(&server)
        .await;
    let client = common::test_client(&server);
    assert!(matches!(
        client.post::<OkBody, _>("/start", &json!({"n":1})).await,
        Err(Error::UnexpectedRedirect)
    ));
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests[1].body.is_empty());
    assert!(!requests[1].headers.contains_key("content-type"));
}

#[tokio::test]
async fn five_redirects_allowed_sixth_rejected() {
    for hops in [5, 6] {
        let server = MockServer::start().await;
        for n in 0..hops {
            Mock::given(path(format!("/{n}")))
                .respond_with(
                    ResponseTemplate::new(308).insert_header("Location", format!("/{}", n + 1)),
                )
                .expect(1)
                .mount(&server)
                .await;
        }
        Mock::given(path(format!("/{hops}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true})))
            .expect(u64::from(hops == 5))
            .mount(&server)
            .await;
        let client = common::test_client(&server);
        let result = client.get::<OkBody>("/0").await;
        if hops == 5 {
            assert!(result.unwrap().ok);
        } else {
            assert!(matches!(result, Err(Error::UnexpectedRedirect)));
        }
        assert_eq!(client.telemetry().api, 6);
    }
}

#[tokio::test]
async fn delete_no_content_and_public_api_builder_work() {
    use canvas_api::ApiRequest;
    use reqwest::{Method, Url};
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/item"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/item"))
        .and(body_json(json!({"n":1})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true})))
        .expect(1)
        .mount(&server)
        .await;
    let client = common::test_client(&server);
    client.delete::<()>("/item").await.unwrap();
    let request = ApiRequest::new(
        Method::PUT,
        Url::parse(&format!("{}/item", server.uri())).unwrap(),
    )
    .json(&json!({"n":1}))
    .unwrap();
    assert!(client.send_api::<OkBody>(request).await.unwrap().ok);
}
