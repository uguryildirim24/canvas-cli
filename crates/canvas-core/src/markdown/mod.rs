//! HTML → Markdown/plain text.
//!
//! Choice: **`htmd`** (Apache-2.0) for HTML → Markdown. Assignment descriptions and
//! announcements use Markdown in JSON schemas; plain text is derived by stripping
//! when callers need it.

use htmd::HtmlToMarkdown;

/// Convert HTML to Markdown using `htmd`.
pub fn html_to_markdown(html: &str) -> Result<String, MarkdownError> {
    let converter = HtmlToMarkdown::new();
    converter
        .convert(html)
        .map(|s| s.trim().to_string())
        .map_err(|e| MarkdownError::Convert(e.to_string()))
}

/// Convert HTML to a plain-text approximation (Markdown then strip simple markers).
pub fn html_to_plain(html: &str) -> Result<String, MarkdownError> {
    let md = html_to_markdown(html)?;
    Ok(strip_simple_markdown(&md))
}

fn strip_simple_markdown(md: &str) -> String {
    let mut out = String::with_capacity(md.len());
    let mut chars = md.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '#' | '*' | '_' | '`' => {}
            '[' => {
                // [text](url) → text
                let mut text = String::new();
                for c2 in chars.by_ref() {
                    if c2 == ']' {
                        break;
                    }
                    text.push(c2);
                }
                if chars.peek() == Some(&'(') {
                    chars.next();
                    for c2 in chars.by_ref() {
                        if c2 == ')' {
                            break;
                        }
                    }
                }
                out.push_str(&text);
            }
            _ => out.push(c),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Markdown conversion error.
#[derive(Debug, thiserror::Error)]
pub enum MarkdownError {
    /// Underlying converter failure.
    #[error("html convert: {0}")]
    Convert(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_basic_html() {
        let md = html_to_markdown("<p>Hello <strong>world</strong></p>").unwrap();
        assert!(md.to_lowercase().contains("hello"));
        assert!(md.contains("world"));
    }
}
