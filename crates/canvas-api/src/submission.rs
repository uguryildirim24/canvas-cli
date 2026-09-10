//! Submission POST helpers (§12.2 step 9).

use reqwest::header::HeaderMap;
use reqwest::{Method, StatusCode, Url};
use serde_json::json;

use crate::request::ApiRequest;
use crate::{Client, Error};

/// JSON body for `POST …/assignments/:aid/submissions`.
#[derive(Debug, Clone)]
pub enum SubmissionBody {
    /// `online_upload` with previously uploaded file IDs.
    OnlineUpload {
        /// Canvas file IDs from the upload session.
        file_ids: Vec<i64>,
        /// Optional text comment.
        comment: Option<String>,
    },
    /// `online_text_entry` with HTML body.
    OnlineTextEntry {
        /// Outbound HTML body.
        body: String,
        /// Optional text comment.
        comment: Option<String>,
    },
    /// `online_url` with an http(s) URL.
    OnlineUrl {
        /// Submitted URL.
        url: String,
        /// Optional text comment.
        comment: Option<String>,
    },
}

impl SubmissionBody {
    fn to_json(&self) -> serde_json::Value {
        match self {
            Self::OnlineUpload { file_ids, comment } => {
                let submission = json!({
                    "submission_type": "online_upload",
                    "file_ids": file_ids,
                });
                wrap_comment(&submission, comment.as_ref())
            }
            Self::OnlineTextEntry { body, comment } => {
                let submission = json!({
                    "submission_type": "online_text_entry",
                    "body": body,
                });
                wrap_comment(&submission, comment.as_ref())
            }
            Self::OnlineUrl { url, comment } => {
                let submission = json!({
                    "submission_type": "online_url",
                    "url": url,
                });
                wrap_comment(&submission, comment.as_ref())
            }
        }
    }
}

fn wrap_comment(submission: &serde_json::Value, comment: Option<&String>) -> serde_json::Value {
    match comment {
        Some(text) if !text.is_empty() => json!({
            "submission": submission,
            "comment": { "text_comment": text },
        }),
        _ => json!({ "submission": submission }),
    }
}

/// `POST /api/v1/courses/{cid}/assignments/{aid}/submissions` without mapping non-2xx.
///
/// Retries 429 / rate-limit bodies via the shared API executor. Callers classify
/// the status and body per SPEC §12.2 step 9.
pub async fn post_submission(
    client: &Client,
    course_id: i64,
    assignment_id: i64,
    body: &SubmissionBody,
) -> Result<(StatusCode, HeaderMap, Vec<u8>, Url), Error> {
    let path = format!("/api/v1/courses/{course_id}/assignments/{assignment_id}/submissions");
    let url = client.api_url(&path)?;
    let mut request = ApiRequest::new(Method::POST, url).json(&body.to_json())?;
    request.preserve_error_response = true;
    client.execute_api(request).await
}

/// True when `bytes` decode as a Canvas-shaped error object.
///
/// Recognized shapes: `{ "errors": … }`, `{ "error": … }`, and branded
/// `{ "status": string, "message": … }` (optionally with `error_report_id`).
#[must_use]
pub fn is_canvas_error_body(bytes: &[u8]) -> bool {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return false;
    };
    let Some(obj) = value.as_object() else {
        return false;
    };
    if obj.contains_key("errors")
        || obj.contains_key("error")
        || obj.contains_key("error_report_id")
    {
        return true;
    }
    matches!(
        (obj.get("status"), obj.get("message")),
        (Some(serde_json::Value::String(_)), Some(_))
    )
}

/// Build a sanitized diagnostic string from a POST response body (never raw-persisted).
#[must_use]
pub fn sanitize_post_error_text(bytes: &[u8]) -> String {
    if let Some(errors) = crate::parse_validation_errors(bytes) {
        return crate::redact::redact_join(&errors);
    }
    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes)
        && let Some(obj) = value.as_object()
    {
        if let Some(msg) = obj.get("message").and_then(|v| v.as_str()) {
            return crate::redact::redact(msg);
        }
        if let Some(err) = obj.get("error").and_then(|v| v.as_str()) {
            return crate::redact::redact(err);
        }
    }
    "submission response could not be decoded as a Canvas error".into()
}

/// Assignment GET with the submit preflight includes.
pub async fn get_assignment_for_submit(
    client: &Client,
    course_id: i64,
    assignment_id: i64,
) -> Result<crate::models::Assignment, Error> {
    let path = format!("/api/v1/courses/{course_id}/assignments/{assignment_id}");
    let mut url = client.api_url(&path)?;
    url.query_pairs_mut()
        .append_pair("include[]", "submission")
        .append_pair("include[]", "can_submit");
    let request = ApiRequest::new(Method::GET, url);
    client.send_api(request).await
}

/// History GET for reconcile / verify / readback.
pub async fn get_submission_history(
    client: &Client,
    course_id: i64,
    assignment_id: i64,
) -> Result<crate::models::Submission, Error> {
    let path = format!("/api/v1/courses/{course_id}/assignments/{assignment_id}/submissions/self");
    let mut url = client.api_url(&path)?;
    url.query_pairs_mut()
        .append_pair("include[]", "submission_history");
    let request = ApiRequest::new(Method::GET, url);
    client.send_api(request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_bodies_are_not_persistable_diagnostics() {
        for body in [
            b"private raw response".as_slice(),
            br#"{"unlisted":"secret"}"#,
            b"<html>secret</html>",
        ] {
            let diagnostic = sanitize_post_error_text(body);
            assert!(!diagnostic.contains("secret"));
            assert!(!diagnostic.contains("private raw"));
        }
    }

    #[test]
    fn canvas_error_shapes() {
        assert!(is_canvas_error_body(
            br#"{"errors":{"base":[{"message":"bad"}]}}"#
        ));
        assert!(is_canvas_error_body(br#"{"error":"nope"}"#));
        assert!(is_canvas_error_body(
            br#"{"status":"gateway_timeout","message":"upstream","error_report_id":1}"#
        ));
        assert!(!is_canvas_error_body(br#"{"id":1,"attempt":2}"#));
        assert!(!is_canvas_error_body(b"not json"));
    }

    #[test]
    fn submission_json_shapes() {
        let upload = SubmissionBody::OnlineUpload {
            file_ids: vec![7, 8],
            comment: Some("note".into()),
        }
        .to_json();
        assert_eq!(upload["submission"]["submission_type"], "online_upload");
        assert_eq!(upload["submission"]["file_ids"], json!([7, 8]));
        assert_eq!(upload["comment"]["text_comment"], "note");

        let text = SubmissionBody::OnlineTextEntry {
            body: "<p>hi</p>".into(),
            comment: None,
        }
        .to_json();
        assert_eq!(text["submission"]["body"], "<p>hi</p>");
        assert!(text.get("comment").is_none());
    }
}

#[cfg(test)]
mod response_tests {
    use super::*;
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};
    #[tokio::test]
    async fn submission_retains_final_throttle_and_refused_redirect_responses() {
        for status in [429, 302] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .respond_with(
                    ResponseTemplate::new(status)
                        .insert_header("Location", "https://elsewhere.test/")
                        .set_body_json(json!({"error":"try later"})),
                )
                .expect(if status == 429 { 5 } else { 1 })
                .mount(&server)
                .await;
            let client = Client::with_governor(
                server.uri().parse().unwrap(),
                crate::Secret::new("test"),
                "test",
                crate::GovernorConfig {
                    jitter: false,
                    ..Default::default()
                },
            )
            .unwrap();
            let response = post_submission(
                &client,
                1,
                2,
                &SubmissionBody::OnlineUrl {
                    url: "https://example.test".into(),
                    comment: None,
                },
            )
            .await
            .unwrap();
            assert_eq!(response.0.as_u16(), status);
            assert!(is_canvas_error_body(&response.2));
        }
    }
}
