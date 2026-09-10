//! Redaction unit coverage for every listed secret key.

use canvas_api::redact::{RedactingLayer, redact};
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use tracing_subscriber::Registry;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::layer::SubscriberExt;

#[test]
fn redact_each_listed_key() {
    assert_eq!(
        redact("Authorization=BearerSECRET"),
        "Authorization=[redacted]"
    );
    assert_eq!(redact("access_token=tok123"), "access_token=[redacted]");
    let upload = redact("upload_params={a:SECRET,b:2}");
    assert!(!upload.contains("SECRET"), "{upload}");
    assert!(upload.contains("[redacted]"), "{upload}");
    assert_eq!(redact("Signature=sigvalue"), "Signature=[redacted]");
    assert_eq!(
        redact("X-Amz-Credential=cred"),
        "X-Amz-Credential=[redacted]"
    );
    // A single plain assignment preserves only its key.
    assert_eq!(
        redact("X-Amz-Signature=amzsig"),
        "X-Amz-Signature=[redacted]"
    );
    assert_eq!(redact("Policy=pol"), "Policy=[redacted]");
    assert_eq!(redact("Expires=999"), "Expires=[redacted]");
    assert_eq!(redact("verifier=ver"), "verifier=[redacted]");
    assert_eq!(redact("sig=s"), "sig=[redacted]");
    assert_eq!(redact("token=t"), "token=[redacted]");
}

#[test]
fn redact_collapses_multi_secret_blob() {
    let input = concat!(
        "Authorization=a access_token=b upload_params={x} Signature=c ",
        "X-Amz-Credential=d Policy=e Expires=f verifier=g sig=h token=i"
    );
    assert!(!redact(input).contains("a access_token"));
}

#[derive(Clone, Default)]
struct Buf(Arc<Mutex<Vec<u8>>>);

impl Write for Buf {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().expect("lock").write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Buf {
    type Writer = Buf;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

#[test]
fn redacting_layer_scrubs_secret_fields() {
    let buf = Buf::default();
    let sink = buf.clone();

    let subscriber = Registry::default().with(
        RedactingLayer::layer()
            .with_writer(sink)
            .with_ansi(false)
            .with_span_events(FmtSpan::NONE)
            .fmt_fields(RedactingLayer),
    );

    tracing::subscriber::with_default(subscriber, || {
        tracing::info!(msg = "access_token=super-secret-value", "event");
        let span = tracing::info_span!("upload", upload_params = ?["FIELD_SECRET"], token = "FIELD_SECRET");
        let _entered = span.enter();
        tracing::info!(
            Authorization = "FIELD_SECRET",
            Expires = 987654321_u64,
            "inside span"
        );
        span.record("token", "UPDATED_SECRET");
        tracing::info!("after record");
    });

    let rendered = String::from_utf8(buf.0.lock().expect("lock").clone()).expect("utf8");
    assert!(
        !rendered.contains("FIELD_SECRET")
            && !rendered.contains("UPDATED_SECRET")
            && !rendered.contains("987654321"),
        "{rendered}"
    );
    assert!(
        !rendered.contains("super-secret-value"),
        "secret leaked: {rendered}"
    );
    assert!(
        rendered.contains("[redacted]") || !rendered.contains("access_token=super"),
        "expected redaction in: {rendered}"
    );
}

#[test]
fn nested_unicode_and_encoded_secrets_are_safe() {
    for input in [
        r#"{"upload_params":{"a":"SECRET","b":"SECRET"}}"#,
        "upload_params={a:SECRET,b:SECRET}",
        "Türkçe🦀?token=SECRET",
        "https://storage.test/x?%74oken=SECRET",
        "https://storage.test/x?X%2dAmz%2dCredential=SECRET",
        "https://storage.test/x?%2574oken=SECRET",
        "Authorization\tBearer\tSECRET",
    ] {
        assert!(!redact(input).contains("SECRET"), "{input}");
    }
    assert_eq!(
        redact("Türkçe🦀: ordinary error"),
        "Türkçe🦀: ordinary error"
    );
}
