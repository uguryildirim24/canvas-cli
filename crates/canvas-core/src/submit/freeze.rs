//! Freeze submit inputs (§12.2 step 5).

use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::journal::{IntendedFile, IntendedPayload, IntendedText};

/// Maximum text / HTML input size (1 MiB).
pub const MAX_TEXT_BYTES: usize = 1024 * 1024;
/// Canvas text-column comment limit.
pub const MAX_COMMENT_CHARS: usize = 65_535;

/// Submission input kind after freeze.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    /// `online_upload`.
    OnlineUpload,
    /// `online_text_entry` from `--text` (HTML transform).
    OnlineTextEntry,
    /// `online_text_entry` from `--html` (verbatim).
    OnlineHtml,
    /// `online_url`.
    OnlineUrl,
}

impl InputKind {
    /// Canvas `submission_types` / journal `kind` string.
    ///
    /// Text and HTML share one Canvas kind; they differ only in the transform.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OnlineUpload => "online_upload",
            Self::OnlineTextEntry | Self::OnlineHtml => "online_text_entry",
            Self::OnlineUrl => "online_url",
        }
    }

    /// Storage name that separates every kind, including text from HTML.
    ///
    /// A plan must round-trip the exact kind it froze: the two text kinds send
    /// different bytes, so [`Self::as_str`] cannot name them apart.
    #[must_use]
    pub const fn plan_name(self) -> &'static str {
        match self {
            Self::OnlineUpload => "online_upload",
            Self::OnlineTextEntry => "online_text_entry",
            Self::OnlineHtml => "online_html",
            Self::OnlineUrl => "online_url",
        }
    }

    /// Parse a [`Self::plan_name`].
    #[must_use]
    pub fn parse_plan_name(raw: &str) -> Option<Self> {
        match raw {
            "online_upload" => Some(Self::OnlineUpload),
            "online_text_entry" => Some(Self::OnlineTextEntry),
            "online_html" => Some(Self::OnlineHtml),
            "online_url" => Some(Self::OnlineUrl),
            _ => None,
        }
    }
}

/// Frozen intent ready for journaling.
#[derive(Debug, Clone)]
pub struct FrozenInput {
    /// Submission kind.
    pub kind: InputKind,
    /// Allowlisted payload.
    pub payload: IntendedPayload,
    /// Absolute paths for file uploads (same order as `payload.files`).
    pub file_paths: Vec<PathBuf>,
}

/// Freeze failures.
#[derive(Debug, Error)]
pub enum FreezeError {
    /// User validation (exit 2).
    #[error("{0}")]
    Validation(String),
    /// I/O while reading inputs.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Validate an optional comment against the Canvas `65_535` character limit.
pub fn validate_comment(comment: Option<&str>) -> Result<Option<String>, FreezeError> {
    match comment {
        None => Ok(None),
        Some(c) if c.chars().count() > MAX_COMMENT_CHARS => Err(FreezeError::Validation(format!(
            "comment longer than {MAX_COMMENT_CHARS} characters"
        ))),
        Some(c) => Ok(Some(c.to_owned())),
    }
}

/// Freeze one or more local files (read once, sha256, size).
pub fn freeze_files(paths: &[PathBuf], comment: Option<&str>) -> Result<FrozenInput, FreezeError> {
    if paths.is_empty() {
        return Err(FreezeError::Validation(
            "at least one --file is required".into(),
        ));
    }
    let comment = validate_comment(comment)?;
    let mut files = Vec::with_capacity(paths.len());
    let mut file_paths = Vec::with_capacity(paths.len());
    for path in paths {
        let mut reader = std::fs::File::open(path)?;
        if !reader.metadata()?.is_file() {
            return Err(FreezeError::Validation(
                "input must be a regular file".into(),
            ));
        }
        let mut digest = Sha256::new();
        let mut size = 0_u64;
        let mut buffer = vec![0; 65536];
        loop {
            let n = reader.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            size += n as u64;
            digest.update(&buffer[..n]);
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| {
                FreezeError::Validation(format!("invalid file name: {}", path.display()))
            })?
            .to_owned();
        files.push(IntendedFile {
            name,
            size,
            sha256: format!("{:x}", digest.finalize()),
            canvas_file_id: None,
        });
        file_paths.push(path.clone());
    }
    Ok(FrozenInput {
        kind: InputKind::OnlineUpload,
        payload: IntendedPayload {
            files,
            comment,
            ..IntendedPayload::default()
        },
        file_paths,
    })
}

/// Freeze `--text` input: CRLF→LF, reject empty, HTML transform, digests.
pub fn freeze_text(
    source: &TextSource<'_>,
    comment: Option<&str>,
) -> Result<FrozenInput, FreezeError> {
    let comment = validate_comment(comment)?;
    let raw = read_text_source(source)?;
    let normalized = normalize_newlines(&raw);
    if normalized.is_empty() {
        return Err(FreezeError::Validation("text input is empty".into()));
    }
    let input_sha256 = hex_sha256(normalized.as_bytes());
    let outbound = text_to_html(&normalized);
    let sent_sha256 = hex_sha256(outbound.as_bytes());
    Ok(FrozenInput {
        kind: InputKind::OnlineTextEntry,
        payload: IntendedPayload {
            text: Some(IntendedText {
                input_sha256,
                transform: "text-to-html".into(),
                sent_sha256,
                outbound_bytes: outbound,
            }),
            comment,
            ..IntendedPayload::default()
        },
        file_paths: Vec::new(),
    })
}

/// Freeze `--html` input verbatim.
pub fn freeze_html(path: &Path, comment: Option<&str>) -> Result<FrozenInput, FreezeError> {
    let comment = validate_comment(comment)?;
    let bytes = read_bounded(path)?;
    if bytes.len() > MAX_TEXT_BYTES {
        return Err(FreezeError::Validation(format!(
            "html input exceeds {MAX_TEXT_BYTES} bytes"
        )));
    }
    if bytes.is_empty() {
        return Err(FreezeError::Validation("html input is empty".into()));
    }
    let digest = hex_sha256(&bytes);
    let outbound = String::from_utf8(bytes)
        .map_err(|_| FreezeError::Validation("html input is not valid UTF-8".into()))?;
    Ok(FrozenInput {
        kind: InputKind::OnlineHtml,
        payload: IntendedPayload {
            text: Some(IntendedText {
                input_sha256: digest.clone(),
                transform: "html-verbatim".into(),
                sent_sha256: digest,
                outbound_bytes: outbound,
            }),
            comment,
            ..IntendedPayload::default()
        },
        file_paths: Vec::new(),
    })
}

/// Freeze a URL (http/https only).
pub fn freeze_url(url: &str, comment: Option<&str>) -> Result<FrozenInput, FreezeError> {
    let comment = validate_comment(comment)?;
    let trimmed = url.trim();
    let parsed =
        reqwest::Url::parse(trimmed).map_err(|_| FreezeError::Validation("invalid URL".into()))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(FreezeError::Validation(
            "URL scheme must be http or https with a host".into(),
        ));
    }
    Ok(FrozenInput {
        kind: InputKind::OnlineUrl,
        payload: IntendedPayload {
            url: Some(trimmed.to_owned()),
            comment,
            ..IntendedPayload::default()
        },
        file_paths: Vec::new(),
    })
}

/// Where `--text` bytes come from.
#[derive(Debug)]
pub enum TextSource<'a> {
    /// Read a file path.
    Path(&'a Path),
    /// Already-buffered stdin / caller bytes.
    Bytes(&'a [u8]),
}

fn read_text_source(source: &TextSource<'_>) -> Result<String, FreezeError> {
    let bytes = match source {
        TextSource::Path(path) => read_bounded(path)?,
        TextSource::Bytes(b) => b.to_vec(),
    };
    if bytes.len() > MAX_TEXT_BYTES {
        return Err(FreezeError::Validation(format!(
            "text input exceeds {MAX_TEXT_BYTES} bytes"
        )));
    }
    String::from_utf8(bytes)
        .map_err(|_| FreezeError::Validation("text input is not valid UTF-8".into()))
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, FreezeError> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take((MAX_TEXT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_TEXT_BYTES {
        return Err(FreezeError::Validation("input exceeds 1 MiB".into()));
    }
    Ok(bytes)
}

fn normalize_newlines(input: &str) -> String {
    input.replace("\r\n", "\n")
}

/// Escape `& < > "`; blank-line blocks → `<p>`; single LF → `<br>`.
pub fn text_to_html(normalized_lf: &str) -> String {
    let blocks: Vec<&str> = normalized_lf
        .split("\n\n")
        .filter(|b| !b.is_empty())
        .collect();
    let mut out = String::new();
    for block in blocks {
        out.push_str("<p>");
        let mut lines = block.split('\n');
        if let Some(first) = lines.next() {
            out.push_str(&escape_html(first));
        }
        for line in lines {
            out.push_str("<br>");
            out.push_str(&escape_html(line));
        }
        out.push_str("</p>");
    }
    out
}

fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            other => out.push(other),
        }
    }
    out
}

pub(crate) fn hex_sha256(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        let _ = write!(out, "{b:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comment_too_long() {
        let long = "x".repeat(MAX_COMMENT_CHARS + 1);
        let err = validate_comment(Some(&long)).unwrap_err();
        assert!(matches!(err, FreezeError::Validation(_)));
    }

    #[test]
    fn text_transform_and_empty() {
        assert!(matches!(
            freeze_text(&TextSource::Bytes(b""), None),
            Err(FreezeError::Validation(_))
        ));
        let frozen = freeze_text(&TextSource::Bytes(b"a\r\n\r\nb\nc"), None).unwrap();
        let text = frozen.payload.text.unwrap();
        assert_eq!(text.transform, "text-to-html");
        assert_eq!(text.outbound_bytes, "<p>a</p><p>b<br>c</p>");
        assert_eq!(text.input_sha256, hex_sha256(b"a\n\nb\nc"));
        assert_eq!(text.sent_sha256, hex_sha256(text.outbound_bytes.as_bytes()));
    }

    #[test]
    fn transform_preserves_whitespace_and_only_normalizes_crlf() {
        let input =
            freeze_text(&TextSource::Bytes(b"  a & < > \" \r\nline\r end  "), None).unwrap();
        assert_eq!(
            input.payload.text.unwrap().outbound_bytes,
            "<p>  a &amp; &lt; &gt; &quot; <br>line\r end  </p>"
        );
        let long = vec![b'x'; MAX_TEXT_BYTES + 1];
        assert!(freeze_text(&TextSource::Bytes(&long), None).is_err());
        assert!(freeze_url("https://?query", None).is_err());
        assert!(freeze_url("HTTPS://example.test/work", None).is_ok());
    }

    #[test]
    fn url_scheme() {
        assert!(freeze_url("https://example.test/x", None).is_ok());
        assert!(freeze_url("ftp://example.test/x", None).is_err());
    }
}
