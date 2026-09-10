//! Secret redaction for diagnostics and tracing.
//!
//! Use [`RedactingLayer::layer`] or a formatting sink with `fmt_fields(RedactingLayer)`.

use std::fmt;

use tracing::Subscriber;
use tracing::field::{Field, Visit};
use tracing_subscriber::field::RecordFields;
use tracing_subscriber::fmt::format::{FormatFields, Writer};

/// Join and redact each validation error message.
#[must_use]
pub fn redact_join(errors: &[String]) -> String {
    errors
        .iter()
        .map(|e| redact(e))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Redact secret-bearing diagnostics. Complex blobs are suppressed as a whole:
/// guessing where an untrusted nested value ends can leak a capability.
#[must_use]
pub fn redact(input: &str) -> String {
    // Decode percent escapes for detection only, including encoded query keys.
    let mut decoded = input.to_owned();
    loop {
        let next = decode_percent(&decoded);
        if next == decoded {
            break;
        }
        decoded = next;
    }
    let lower = decoded.to_ascii_lowercase();
    if !lower
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
        .any(is_sensitive_key)
    {
        return input.to_owned();
    }
    // Preserve the label for a single plain assignment, never any of its value.
    if let Some((key, _)) = input.split_once('=')
        && is_sensitive_key(&key.to_ascii_lowercase())
    {
        return format!("{key}=[redacted]");
    }
    "[redacted]".to_owned()
}

fn decode_percent(input: &str) -> String {
    let mut bytes = Vec::with_capacity(input.len());
    let mut iter = input.as_bytes().iter().copied().peekable();
    while let Some(byte) = iter.next() {
        if byte == b'%' {
            let mut lookahead = iter.clone();
            if let (Some(a), Some(b)) = (lookahead.next(), lookahead.next())
                && let (Some(a), Some(b)) = (char::from(a).to_digit(16), char::from(b).to_digit(16))
            {
                bytes.push(u8::try_from(a * 16 + b).expect("hex byte"));
                iter = lookahead;
                continue;
            }
        }
        bytes.push(byte);
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Is this a query-parameter or header name that carries a capability?
///
/// The name is percent-decoded and lowercased first, so an encoded key is
/// recognized. Callers that persist or print a URL use this to drop the
/// capability-bearing parameters and keep the rest (§15).
#[must_use]
pub fn is_capability_key(key: &str) -> bool {
    let mut decoded = key.to_owned();
    loop {
        let next = decode_percent(&decoded);
        if next == decoded {
            break;
        }
        decoded = next;
    }
    is_sensitive_key(&decoded.to_ascii_lowercase())
}

fn is_sensitive_key(key: &str) -> bool {
    matches!(
        key,
        "authorization"
            | "access_token"
            | "upload_params"
            | "signature"
            | "policy"
            | "expires"
            | "verifier"
            | "sig"
            | "token"
    ) || key.starts_with("x-amz-")
}

/// Tracing layer / field formatter that redacts secret-bearing strings.
#[derive(Debug, Default, Clone, Copy)]
pub struct RedactingLayer;

impl RedactingLayer {
    /// Build a formatting layer which redacts both event and span fields.
    /// Add this layer as the diagnostic sink; a sibling unredacted formatter
    /// receives the original tracing events independently.
    pub fn layer<S>() -> tracing_subscriber::fmt::Layer<S, Self>
    where
        S: Subscriber + for<'lookup> tracing_subscriber::registry::LookupSpan<'lookup>,
    {
        tracing_subscriber::fmt::layer().fmt_fields(Self)
    }
}

impl<'writer> FormatFields<'writer> for RedactingLayer {
    fn format_fields<R: RecordFields>(
        &self,
        mut writer: Writer<'writer>,
        fields: R,
    ) -> fmt::Result {
        let mut visitor = FieldWriter {
            writer: &mut writer,
            result: Ok(()),
            empty: true,
        };
        fields.record(&mut visitor);
        visitor.result
    }
}

struct FieldWriter<'a, 'writer> {
    writer: &'a mut Writer<'writer>,
    result: fmt::Result,
    empty: bool,
}

impl Visit for FieldWriter<'_, '_> {
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.write_pair(field, &value);
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.write_pair(field, &value);
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.write_pair(field, &value);
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.write_pair(field, &value);
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.write_pair(field, &redact(value));
    }

    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        self.write_pair(field, &redact(&value.to_string()));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.write_pair(field, &redact(&format!("{value:?}")));
    }
}

impl FieldWriter<'_, '_> {
    fn write_pair(&mut self, field: &Field, value: &dyn fmt::Display) {
        if self.result.is_err() {
            return;
        }
        self.result = (|| {
            if !self.empty {
                self.writer.write_str(" ")?;
            }
            self.empty = false;
            if is_sensitive_key(&field.name().to_ascii_lowercase()) {
                write!(self.writer, "{}=[redacted]", field.name())
            } else if field.name() == "message" {
                write!(self.writer, "{value}")
            } else {
                write!(self.writer, "{}={value}", field.name())
            }
        })();
    }
}
