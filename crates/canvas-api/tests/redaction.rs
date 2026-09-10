use canvas_api::{Client, Error, Secret};
use reqwest::Url;

#[test]
fn secret_formatting_is_redacted() {
    let secret = Secret::new("synthetic-token-for-tests");
    assert_eq!(secret.expose(), "synthetic-token-for-tests");
    assert_eq!(format!("{secret}"), "[redacted]");
    assert_eq!(format!("{secret:?}"), "[redacted]");
    assert_eq!(format!("{secret:#?}"), "[redacted]");
    assert_eq!(format!("{:?}", secret.clone()), "[redacted]");
}

#[test]
fn client_stores_fields_without_printing_token() {
    let origin = Url::parse("https://canvas.example.test").unwrap();
    let client = Client::new(
        origin,
        Secret::new("synthetic-token-for-tests"),
        "canvas-cli/test",
    )
    .unwrap();
    assert_eq!(client.origin().as_str(), "https://canvas.example.test/");
    assert_eq!(client.user_agent(), "canvas-cli/test");
    assert_eq!(client.token().expose(), "synthetic-token-for-tests");
    for rendered in [format!("{client:?}"), format!("{client:#?}")] {
        assert!(rendered.contains("[redacted]"));
        assert!(!rendered.contains("synthetic-token-for-tests"));
    }
}

#[test]
fn error_formatting_never_prints_response_content() {
    // Synthetic data only; error bodies and validation messages can both echo
    // credentials or transfer parameters supplied by a remote server.
    let sensitive = concat!(
        "RAW_RESPONSE Authorization=FAKE_BEARER access_token=FAKE_ACCESS ",
        "upload_params={arbitrary:FAKE_UPLOAD} Signature=FAKE_SIGNATURE ",
        "X-Amz-Credential=FAKE_CREDENTIAL X-Amz-Signature=FAKE_AMZ_SIGNATURE ",
        "Policy=FAKE_POLICY Expires=FAKE_EXPIRY verifier=FAKE_VERIFIER ",
        "sig=FAKE_SIG token=FAKE_TOKEN"
    );
    for error in [
        Error::Forbidden {
            rate_limited: true,
            body: sensitive.to_owned(),
        },
        Error::Validation {
            status: 422,
            errors: vec![sensitive.to_owned()],
        },
    ] {
        for rendered in [
            format!("{error}"),
            format!("{error:?}"),
            format!("{error:#?}"),
        ] {
            assert!(!rendered.contains("RAW_RESPONSE"), "{rendered}");
            assert!(!rendered.contains("FAKE_"), "{rendered}");
            assert!(rendered.contains("rate_limited=true") || rendered.contains("422"));
        }
    }
}

#[test]
fn request_response_debug_omits_capabilities_and_raw_bodies() {
    use canvas_api::{ApiRequest, TransferRequest, TransferResponse};
    use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
    use reqwest::{Method, StatusCode};
    let url = Url::parse("https://storage.test/?token=SECRET").unwrap();
    let api = ApiRequest::new(Method::POST, url.clone()).body(b"RAW_BODY".to_vec());
    let transfer = TransferRequest::upload(url.clone())
        .header(AUTHORIZATION, HeaderValue::from_static("SECRET"))
        .body(b"RAW_BODY".to_vec());
    let response = TransferResponse {
        status: StatusCode::OK,
        headers: HeaderMap::new(),
        body: b"RAW_BODY".to_vec(),
        final_url: url,
    };
    for diagnostic in [
        format!("{api:?}"),
        format!("{transfer:?}"),
        format!("{response:?}"),
    ] {
        assert!(
            !diagnostic.contains("SECRET")
                && !diagnostic.contains("RAW_BODY")
                && !diagnostic.contains("82, 65, 87"),
            "{diagnostic}"
        );
    }
}
