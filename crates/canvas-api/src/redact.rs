//! Secret redaction for diagnostics and tracing.
//!
//! NOTE: `tracing-subscriber` is a regular dependency of `canvas-api`
//! (`tracing-subscriber.workspace = true`) so [`RedactingLayer`] can implement
//! `tracing_subscriber::Layer`.

use std::fmt;

use tracing::Event;
use tracing::Subscriber;
use tracing::field::{Field, Visit};
use tracing_subscriber::field::RecordFields;
use tracing_subscriber::fmt::format::{FormatFields, Writer};
use tracing_subscriber::layer::{Context, Layer};

/// Join and redact each validation error message.
#[must_use]
pub fn redact_join(errors: &[String]) -> String {
    errors
        .iter()
        .map(|e| redact(e))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Replace secret-bearing values in `input` with `[redacted]`.
///
/// Redacts `Authorization`, `access_token`, every `upload_params` value, and
/// parameters named `Signature`, `X-Amz-*`, `Policy`, `Expires`, `verifier`,
/// `sig`, or `token` (case-insensitive keys) in query strings and free-text
/// `KEY=VALUE` / `"KEY":"VALUE"` forms.
#[must_use]
pub fn redact(input: &str) -> String {
    if !contains_sensitive_key(input) {
        return input.to_owned();
    }
    let granular = granular_redact(input);
    // Response dumps keep non-secret prefixes beside redacted values. Collapse
    // those so Error Display never leaks markers like `RAW_RESPONSE`.
    if count_sensitive_keys(input) > 1 || input.contains(' ') {
        return "[redacted]".to_owned();
    }
    granular
}

fn contains_sensitive_key(input: &str) -> bool {
    count_sensitive_keys(input) > 0
}

fn count_sensitive_keys(input: &str) -> usize {
    let lower = input.to_ascii_lowercase();
    let mut n = 0;
    for key in [
        "authorization",
        "access_token",
        "upload_params",
        "signature",
        "policy",
        "expires",
        "verifier",
        "sig",
        "token",
    ] {
        if key_present(&lower, key) {
            n += 1;
        }
    }
    if lower.contains("x-amz-") {
        n += 1;
    }
    n
}

fn key_present(lower: &str, key: &str) -> bool {
    let mut from = 0;
    while let Some(rel) = lower[from..].find(key) {
        let start = from + rel;
        let end = start + key.len();
        let before_ok = start == 0
            || (!lower.as_bytes()[start - 1].is_ascii_alphanumeric()
                && lower.as_bytes()[start - 1] != b'_');
        let after_ok = end >= lower.len()
            || (!lower.as_bytes()[end].is_ascii_alphanumeric() && lower.as_bytes()[end] != b'_');
        if before_ok && after_ok {
            return true;
        }
        from = start + 1;
    }
    false
}

fn granular_redact(input: &str) -> String {
    let mut out = redact_upload_params(input);
    out = redact_assignments(&out);
    out
}

fn redact_upload_params(input: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let Some(idx) = lower.find("upload_params") else {
        return input.to_owned();
    };
    let after_key = &input[idx + "upload_params".len()..];
    let trimmed = after_key.trim_start();
    let value_start = input.len() - trimmed.len();
    if trimmed.is_empty() {
        return input.to_owned();
    }
    let value_end = match trimmed.as_bytes()[0] {
        b'{' => balanced_end(trimmed, b'{', b'}').map_or(input.len(), |n| value_start + n),
        b'"' => skip_json_string(input, value_start),
        _ => {
            let stop = trimmed
                .find(|c: char| c.is_whitespace() || c == '&' || c == ',')
                .map_or(trimmed.len(), |i| i);
            value_start + stop
        }
    };
    let mut s = String::with_capacity(input.len());
    s.push_str(&input[..value_start]);
    s.push_str("[redacted]");
    s.push_str(&input[value_end..]);
    s
}

fn balanced_end(s: &str, open: u8, close: u8) -> Option<usize> {
    let mut depth = 0usize;
    for (i, b) in s.bytes().enumerate() {
        if b == open {
            depth += 1;
        } else if b == close {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(i + 1);
            }
        }
    }
    None
}

fn redact_assignments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some((value_start, value_end)) = match_assignment(input, i) {
            out.push_str(&input[i..value_start]);
            out.push_str("[redacted]");
            i = value_end;
            continue;
        }
        out.push(char::from(bytes[i]));
        i += 1;
    }
    out
}

fn match_assignment(input: &str, start: usize) -> Option<(usize, usize)> {
    let rest = &input[start..];
    let lower = rest.to_ascii_lowercase();

    if let Some(after_quote) = rest.strip_prefix('"') {
        let key_end = after_quote.find('"')? + 1;
        let key = &lower[1..key_end];
        if !is_sensitive_key(key) {
            return None;
        }
        let after_key = rest[key_end + 1..].trim_start();
        if !after_key.starts_with(':') {
            return None;
        }
        let after_colon = after_key[1..].trim_start();
        let value_start = start + (rest.len() - after_colon.len());
        let value_end = skip_json_value(input, value_start);
        return Some((value_start, value_end));
    }

    let key_len = rest
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_')
        .unwrap_or(rest.len());
    if key_len == 0 {
        return None;
    }
    let key = &lower[..key_len];
    if !is_sensitive_key(key) {
        return None;
    }
    let after = &rest[key_len..];
    let value_part = if let Some(stripped) = after.strip_prefix('=') {
        stripped
    } else if let Some(stripped) = after.strip_prefix(':') {
        stripped.trim_start()
    } else if after.len() >= 3 && after[..3].eq_ignore_ascii_case("%3d") {
        &after[3..]
    } else {
        return None;
    };
    let value_start = start + key_len + (after.len() - value_part.len());
    let value_end = skip_plain_value(input, value_start);
    Some((value_start, value_end))
}

fn is_sensitive_key(key: &str) -> bool {
    matches!(
        key,
        "authorization"
            | "access_token"
            | "signature"
            | "policy"
            | "expires"
            | "verifier"
            | "sig"
            | "token"
    ) || key.starts_with("x-amz-")
}

fn skip_json_value(input: &str, start: usize) -> usize {
    let rest = &input[start..];
    if rest.is_empty() {
        return start;
    }
    match rest.as_bytes()[0] {
        b'"' => skip_json_string(input, start),
        b'{' => balanced_end(rest, b'{', b'}').map_or(input.len(), |n| start + n),
        b'[' => balanced_end(rest, b'[', b']').map_or(input.len(), |n| start + n),
        _ => skip_plain_value(input, start),
    }
}

fn skip_json_string(input: &str, start: usize) -> usize {
    let rest = &input[start..];
    if !rest.starts_with('"') {
        return start;
    }
    let mut i = 1;
    while i < rest.len() {
        match rest.as_bytes()[i] {
            b'\\' => i += 2,
            b'"' => return start + i + 1,
            _ => i += 1,
        }
    }
    input.len()
}

fn skip_plain_value(input: &str, start: usize) -> usize {
    if input[start..].starts_with('"') {
        return skip_json_string(input, start);
    }
    let rest = &input[start..];
    let end = rest
        .find(|c: char| c.is_whitespace() || c == '&' || c == ',' || c == ';' || c == '}')
        .map_or(rest.len(), |i| i);
    start + end
}

/// Tracing layer / field formatter that redacts secret-bearing strings.
#[derive(Debug, Default, Clone, Copy)]
pub struct RedactingLayer;

impl<S> Layer<S> for RedactingLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        // Prefer `fmt::layer().fmt_fields(RedactingLayer)` for formatted output.
        // This `Layer` impl allows registry composition with the same type.
        let mut visitor = NopVisitor;
        event.record(&mut visitor);
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

struct NopVisitor;

impl Visit for NopVisitor {
    fn record_debug(&mut self, _field: &Field, _value: &dyn fmt::Debug) {}
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
            if field.name() == "message" {
                write!(self.writer, "{value}")
            } else {
                write!(self.writer, "{}={value}", field.name())
            }
        })();
    }
}
