//! Pagination and cross-origin Link header tests.

mod common;

use canvas_api::Error;
use futures_util::StreamExt;
use serde::Deserialize;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[derive(Debug, Deserialize)]
struct Item {
    id: i64,
}

#[tokio::test]
async fn get_all_yields_three_pages() {
    let server = MockServer::start().await;
    let base = server.uri();

    Mock::given(method("GET"))
        .and(path("/api/v1/items"))
        .and(query_param("per_page", "100"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(include_str!("fixtures/page1.json"))
                .insert_header(
                    "Link",
                    format!("<{base}/api/v1/items?page=2&per_page=100>; rel=\"next\""),
                ),
        )
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/v1/items"))
        .and(query_param("page", "2"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(include_str!("fixtures/page2.json"))
                .insert_header(
                    "Link",
                    format!("<{base}/api/v1/items?page=3&per_page=100>; rel=\"next\""),
                ),
        )
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/v1/items"))
        .and(query_param("page", "3"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(include_str!("fixtures/page3.json")),
        )
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;

    let client = common::test_client(&server);
    let mut pages = std::pin::pin!(client.get_all::<Item>("/api/v1/items"));
    let mut ids = Vec::new();
    let mut page_count = 0usize;
    while let Some(page) = pages.next().await {
        let page = page.expect("page");
        page_count += 1;
        ids.extend(page.items.into_iter().map(|i| i.id));
    }
    assert_eq!(page_count, 3);
    assert_eq!(ids, vec![1, 2, 3, 4, 5]);
}

#[tokio::test]
async fn cross_origin_next_link_is_rejected() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/items"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(include_str!("fixtures/page1.json"))
                .insert_header(
                    "Link",
                    "<https://evil.example/api/v1/items?page=2>; rel=\"next\"",
                ),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = common::test_client(&server);
    let mut pages = std::pin::pin!(client.get_all::<Item>("/api/v1/items"));
    let first = pages.next().await.expect("one result");
    assert!(matches!(first, Err(Error::CrossOrigin)));
}
