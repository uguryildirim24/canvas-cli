//! Upload and download transport tests (§16).

mod common;

use canvas_api::upload::{UploadMeta, upload_submission_file};
use canvas_api::{Error, download};
use common::test_client;
use reqwest::Url;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Match, Mock, MockServer, Request, ResponseTemplate};

struct NoAuthorization;
impl Match for NoAuthorization {
    fn matches(&self, req: &Request) -> bool {
        !req.headers
            .keys()
            .any(|k| k.as_str().eq_ignore_ascii_case("authorization"))
    }
}

#[tokio::test]
async fn upload_params_order_file_last_no_bearer_201_body() {
    let server = MockServer::start().await;
    let client = test_client(&server);

    Mock::given(method("POST"))
        .and(path(
            "/api/v1/courses/1/assignments/2/submissions/self/files",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "upload_url": format!("{}/storage", server.uri()),
            "upload_params": {
                "key": "abc",
                "Policy": "pol",
                "x-amz": "sig"
            }
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/storage"))
        .and(NoAuthorization)
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({"id": 99})))
        .expect(1)
        .mount(&server)
        .await;

    let meta = UploadMeta {
        name: "a.pdf".into(),
        size: 4,
        content_type: "application/pdf".into(),
    };
    let result = upload_submission_file(&client, 1, 2, &meta, &b"data"[..])
        .await
        .unwrap();
    assert_eq!(result.file_id, 99);

    let requests = server.received_requests().await.unwrap();
    let storage = requests
        .iter()
        .find(|r| r.url.path() == "/storage")
        .expect("storage POST");
    let body = String::from_utf8_lossy(&storage.body);
    let file_field = body
        .find("name=\"file\"")
        .expect("multipart file field");
    let key_field = body.find("name=\"key\"").expect("key field");
    assert!(key_field < file_field, "file must be last");
    assert!(
        !storage
            .headers
            .keys()
            .any(|k| k.as_str().eq_ignore_ascii_case("authorization"))
    );
}

#[tokio::test]
async fn upload_3xx_handoff_and_201_location() {
    let server = MockServer::start().await;
    let client = test_client(&server);

    Mock::given(method("POST"))
        .and(path(
            "/api/v1/courses/1/assignments/2/submissions/self/files",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "upload_url": format!("{}/storage", server.uri()),
            "upload_params": { "k": "v" }
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/storage"))
        .respond_with(ResponseTemplate::new(303).insert_header("Location", "/api/v1/files/77"))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/v1/files/77"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": 77})))
        .mount(&server)
        .await;

    let meta = UploadMeta {
        name: "a.pdf".into(),
        size: 1,
        content_type: "application/pdf".into(),
    };
    let result = upload_submission_file(&client, 1, 2, &meta, &b"x"[..])
        .await
        .unwrap();
    assert_eq!(result.file_id, 77);

    let server2 = MockServer::start().await;
    let client2 = test_client(&server2);
    Mock::given(method("POST"))
        .and(path(
            "/api/v1/courses/1/assignments/2/submissions/self/files",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "upload_url": format!("{}/storage", server2.uri()),
            "upload_params": { "k": "v" }
        })))
        .mount(&server2)
        .await;
    Mock::given(method("POST"))
        .and(path("/storage"))
        .respond_with(
            ResponseTemplate::new(201)
                .insert_header("Location", "/api/v1/files/55")
                .set_body_bytes(""),
        )
        .mount(&server2)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/files/55"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": 55})))
        .mount(&server2)
        .await;
    let result = upload_submission_file(
        &client2,
        1,
        2,
        &UploadMeta {
            name: "b.pdf".into(),
            size: 1,
            content_type: "application/pdf".into(),
        },
        &b"y"[..],
    )
    .await
    .unwrap();
    assert_eq!(result.file_id, 55);
}

#[tokio::test]
async fn upload_missing_and_off_origin_location() {
    let server = MockServer::start().await;
    let client = test_client(&server);

    Mock::given(method("POST"))
        .and(path(
            "/api/v1/courses/1/assignments/2/submissions/self/files",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "upload_url": format!("{}/storage", server.uri()),
            "upload_params": { "k": "v" }
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/storage"))
        .respond_with(ResponseTemplate::new(201).set_body_bytes(""))
        .mount(&server)
        .await;

    let err = upload_submission_file(
        &client,
        1,
        2,
        &UploadMeta {
            name: "a".into(),
            size: 1,
            content_type: "text/plain".into(),
        },
        &b"z"[..],
    )
    .await
    .unwrap_err();
    assert!(matches!(err, Error::UploadIncomplete { .. }));

    let server2 = MockServer::start().await;
    let client2 = test_client(&server2);
    Mock::given(method("POST"))
        .and(path(
            "/api/v1/courses/1/assignments/2/submissions/self/files",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "upload_url": format!("{}/storage", server2.uri()),
            "upload_params": { "k": "v" }
        })))
        .mount(&server2)
        .await;
    Mock::given(method("POST"))
        .and(path("/storage"))
        .respond_with(
            ResponseTemplate::new(302).insert_header("Location", "https://evil.example/x"),
        )
        .mount(&server2)
        .await;
    let err = upload_submission_file(
        &client2,
        1,
        2,
        &UploadMeta {
            name: "a".into(),
            size: 1,
            content_type: "text/plain".into(),
        },
        &b"z"[..],
    )
    .await
    .unwrap_err();
    assert!(matches!(err, Error::UploadIncomplete { status: 302 }));
}

#[tokio::test]
async fn download_token_encoding_size_and_denied() {
    let server = MockServer::start().await;
    let client = test_client(&server);

    Mock::given(method("GET"))
        .and(path("/file.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "4")
                .set_body_bytes("abcd"),
        )
        .mount(&server)
        .await;

    let url = Url::parse(&format!("{}/file.bin", server.uri())).unwrap();
    let mut out = Vec::new();
    let n = download::download(&client, url, &mut out, Some(4), |_| {})
        .await
        .unwrap();
    assert_eq!(n, 4);
    assert_eq!(out, b"abcd");

    let server2 = MockServer::start().await;
    let client2 = test_client(&server2);
    Mock::given(method("GET"))
        .and(path("/x"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes("hi"))
        .mount(&server2)
        .await;
    let url = Url::parse(&format!("{}/x", server2.uri())).unwrap();
    let err = download::download(&client2, url, &mut Vec::new(), Some(99), |_| {})
        .await
        .unwrap_err();
    assert!(matches!(err, Error::SizeMismatch));

    let server3 = MockServer::start().await;
    let client3 = test_client(&server3);
    Mock::given(method("GET"))
        .and(path("/missing"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server3)
        .await;
    let url = Url::parse(&format!("{}/missing", server3.uri())).unwrap();
    let err = download::download(&client3, url, &mut Vec::new(), None, |_| {})
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Denied { status: 404 }));
}

#[tokio::test]
async fn download_off_origin_strips_token_storage_403() {
    let canvas = MockServer::start().await;
    let storage = MockServer::start().await;
    let client = test_client(&canvas);

    Mock::given(method("GET"))
        .and(path("/obj"))
        .and(NoAuthorization)
        .respond_with(ResponseTemplate::new(403))
        .mount(&storage)
        .await;

    let url = Url::parse(&format!("{}/obj", storage.uri())).unwrap();
    let err = download::download(&client, url, &mut Vec::new(), None, |_| {})
        .await
        .unwrap_err();
    assert!(matches!(err, Error::StorageExpired));
}
