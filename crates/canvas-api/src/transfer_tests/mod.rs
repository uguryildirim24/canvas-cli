//! Upload and download transport tests (§16).

mod tls;

use crate::upload::{UploadMeta, upload_submission_file};
use crate::{Error, download};
use reqwest::Url;
use serde_json::json;
use sha2::Digest;
use tls::{TestServer as MockServer, test_client};
use wiremock::matchers::{method, path};
use wiremock::{Match, Mock, Request, ResponseTemplate};

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
        .respond_with(ResponseTemplate::new(200).set_body_raw(format!(r#"{{"upload_url":"{}/storage","upload_params":{{"key":"abc","Policy":"pol","x-amz":"sig"}}}}"#, server.uri()), "application/json"))
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
    let file_field = body.find("name=\"file\"").expect("multipart file field");
    let key_field = body.find("name=\"key\"").expect("key field");
    let policy_field = body.find("name=\"Policy\"").unwrap();
    let signature_field = body.find("name=\"x-amz\"").unwrap();
    assert!(
        key_field < policy_field && policy_field < signature_field && signature_field < file_field
    );
    assert_eq!(
        result.sha256.as_slice(),
        sha2::Sha256::digest(b"data").as_slice()
    );
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
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests[0].headers["authorization"], "Bearer tok");
    assert_eq!(requests[0].headers["accept-encoding"], "identity");

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

#[tokio::test]
async fn tls_fixture_health() {
    let server = MockServer::start().await;
    let client = test_client(&server);
    client.http().get(server.uri()).send().await.unwrap();
}

async fn session(server: &MockServer, upload_url: &str) {
    Mock::given(method("POST"))
        .and(path(
            "/api/v1/courses/1/assignments/2/submissions/self/files",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"upload_url": upload_url, "upload_params": {}})),
        )
        .mount(server)
        .await;
}
fn meta() -> UploadMeta {
    UploadMeta {
        name: "a\r\n\".txt".into(),
        size: 1,
        content_type: "text/plain".into(),
    }
}

#[tokio::test]
async fn upload_handoff_matrix_never_replays_post() {
    for (status, location, body, success) in [
        (301, Some("/api/v1/files/7"), "", true),
        (302, Some("/api/v1/files/7"), "", true),
        (303, Some("/api/v1/files/7"), "", true),
        (307, Some("/api/v1/files/7"), "", true),
        (308, Some("/api/v1/files/7"), "", true),
        (201, Some("/api/v1/files/7"), "not JSON", true),
        (201, None, r#"{"id":"7"}"#, true),
        (201, None, "[]", false),
        (201, Some("https://off-origin.invalid/"), "", false),
        (303, None, "", false),
        (308, Some("https://off-origin.invalid/"), "", false),
        (304, Some("/api/v1/files/7"), "", false),
        (429, None, "", false),
        (403, None, "Rate Limit Exceeded", false),
    ] {
        let server = MockServer::start().await;
        let client = test_client(&server);
        session(&server, &format!("{}/storage", server.uri())).await;
        let mut response = ResponseTemplate::new(status).set_body_string(body);
        if let Some(location) = location {
            response = response.insert_header("Location", location);
        }
        Mock::given(method("POST"))
            .and(path("/storage"))
            .and(NoAuthorization)
            .respond_with(response)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v1/files/7"))
            .and(wiremock::matchers::header("authorization", "Bearer tok"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id":7})))
            .mount(&server)
            .await;
        let result = upload_submission_file(&client, 1, 2, &meta(), &b"x"[..]).await;
        if success {
            assert_eq!(result.unwrap().file_id, 7);
        } else {
            assert!(
                matches!(result, Err(Error::UploadIncomplete { status: code }) if code == status),
                "{status}: {result:?}"
            );
        }
        let requests = server.received_requests().await.unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|r| r.method == "POST" && r.url.path() == "/storage")
                .count(),
            1
        );
    }
}

#[tokio::test]
async fn transfers_reject_http_and_userinfo_before_storage_request() {
    let server = MockServer::start().await;
    let client = test_client(&server);
    for bad in [
        server.uri().replacen("https", "http", 1),
        server
            .uri()
            .replacen("https://", "https://user:password@", 1),
    ] {
        assert!(matches!(
            download::download(&client, bad.parse().unwrap(), &mut Vec::new(), None, |_| {}).await,
            Err(Error::Network)
        ));
        session(&server, &bad).await;
        assert!(matches!(
            upload_submission_file(&client, 1, 2, &meta(), &b"x"[..]).await,
            Err(Error::Network)
        ));
        server.reset().await;
    }
}

#[tokio::test]
async fn download_hops_strip_auth_count_requests_and_refuse_downgrade() {
    let canvas = MockServer::start().await;
    let storage = MockServer::start().await;
    let client = test_client(&canvas);
    Mock::given(method("GET"))
        .and(path("/start"))
        .and(wiremock::matchers::header("authorization", "Bearer tok"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("Location", format!("{}/object", storage.uri())),
        )
        .expect(1)
        .mount(&canvas)
        .await;
    Mock::given(method("GET"))
        .and(path("/object"))
        .and(NoAuthorization)
        .and(wiremock::matchers::header("accept-encoding", "identity"))
        .respond_with(ResponseTemplate::new(200).set_body_string("payload"))
        .expect(1)
        .mount(&storage)
        .await;
    let mut sink = Vec::new();
    let mut progress = Vec::new();
    assert_eq!(
        download::download(
            &client,
            format!("{}/start", canvas.uri()).parse().unwrap(),
            &mut sink,
            Some(7),
            |n| progress.push(n)
        )
        .await
        .unwrap(),
        7
    );
    assert_eq!(client.telemetry().storage, 2);
    assert_eq!(progress.last(), Some(&7));
    assert_eq!(sink, b"payload");
    canvas.reset().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(302).insert_header("Location", "http://127.0.0.1:1/"))
        .mount(&canvas)
        .await;
    assert!(matches!(
        download::download(
            &client,
            canvas.uri().parse().unwrap(),
            &mut Vec::new(),
            None,
            |_| {}
        )
        .await,
        Err(Error::Network)
    ));
}

#[tokio::test]
async fn downloads_check_missing_and_truncated_content_length() {
    for response in [
        b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nhi".as_slice(),
        b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\nhi".as_slice(),
    ] {
        let server = MockServer::raw(response).await;
        let client = test_client(&server);
        let mut sink = Vec::new();
        assert!(matches!(
            download::download(
                &client,
                server.uri().parse().unwrap(),
                &mut sink,
                Some(99),
                |_| {}
            )
            .await,
            Err(Error::SizeMismatch)
        ));
    }
}

#[tokio::test]
async fn throttle_403_precedes_denial_and_expiry_honors_retry_after() {
    for off_origin in [false, true] {
        let canvas = MockServer::start().await;
        let storage = MockServer::start().await;
        let target = if off_origin { &storage } else { &canvas };
        let client = test_client(&canvas);
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(403)
                    .set_body_string("Rate Limit Exceeded")
                    .insert_header("Retry-After", "0"),
            )
            .expect(5)
            .mount(target)
            .await;
        let mut sink = Vec::new();
        assert!(matches!(
            download::download(
                &client,
                target.uri().parse().unwrap(),
                &mut sink,
                None,
                |_| {}
            )
            .await,
            Err(Error::RateLimited)
        ));
        assert!(sink.is_empty());
    }
}

#[tokio::test]
async fn error_body_failure_does_not_hide_final_denial() {
    for status in [401, 404] {
        let response = if status == 401 {
            b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 100\r\nConnection: close\r\n\r\nx"
                .as_slice()
        } else {
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 100\r\nConnection: close\r\n\r\nx"
                .as_slice()
        };
        let server = MockServer::raw(response).await;
        let client = test_client(&server);
        assert!(
            matches!(download::download(&client, server.uri().parse().unwrap(), &mut Vec::new(), None, |_| {}).await, Err(Error::Denied { status: code }) if code == status)
        );
    }
}
