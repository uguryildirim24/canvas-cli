//! HTML → Markdown/plain text.
//!
//! Choice: **`htmd`** (Apache-2.0) for HTML → Markdown. Assignment descriptions and
//! announcements use Markdown in JSON schemas; plain text is extracted from the parsed HTML tree
//! when callers need it.

pub mod extract;

use htmd::HtmlToMarkdown;

pub use extract::{
    BODY_LIMIT, BodyRefs, EmbeddedRef, ExternalLink, FileRef, RawEmbed, RawLink, ResolvedRefs,
    RichText, rich_text, rich_text_opt,
};

/// Convert HTML on the blocking pool.
pub async fn html_to_markdown(html: &str) -> Result<String, MarkdownError> {
    let html = html.to_owned();
    tokio::task::spawn_blocking(move || {
        HtmlToMarkdown::new()
            .convert(&html)
            .map(|s| s.trim().to_owned())
            .map_err(|_| MarkdownError::Convert)
    })
    .await
    .map_err(|_| MarkdownError::Worker)?
}

/// Extract decoded text from the HTML tree, preserving literal punctuation.
pub async fn html_to_plain(html: &str) -> Result<String, MarkdownError> {
    let html = html.to_owned();
    tokio::task::spawn_blocking(move || {
        use markup5ever_rcdom::NodeData;
        let tree = HtmlToMarkdown::new()
            .html_to_tree(&html)
            .map_err(|_| MarkdownError::Convert)?;
        let mut stack = vec![(tree.clone(), false)];
        let mut out = String::new();
        while let Some((node, end)) = stack.pop() {
            if end {
                out.push(' ');
                continue;
            }
            match &node.data {
                NodeData::Text { contents } => out.push_str(&contents.borrow()),
                NodeData::Element { name, .. }
                    if matches!(name.local.as_ref(), "script" | "style" | "head") =>
                {
                    continue;
                }
                NodeData::Element { name, .. }
                    if matches!(
                        name.local.as_ref(),
                        "p" | "div"
                            | "br"
                            | "li"
                            | "tr"
                            | "td"
                            | "th"
                            | "h1"
                            | "h2"
                            | "h3"
                            | "h4"
                            | "pre"
                            | "blockquote"
                    ) =>
                {
                    out.push(' ');
                    stack.push((node.clone(), true));
                }
                _ => {}
            }
            stack.extend(
                node.children
                    .borrow()
                    .iter()
                    .rev()
                    .map(|child| (child.clone(), false)),
            );
        }
        Ok(out.split_whitespace().collect::<Vec<_>>().join(" "))
    })
    .await
    .map_err(|_| MarkdownError::Worker)?
}

/// Markdown conversion error.
#[derive(Debug, thiserror::Error)]
pub enum MarkdownError {
    /// Underlying converter failure.
    #[error("HTML conversion failed")]
    Convert,
    #[error("blocking worker failed")]
    Worker,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn converts_basic_html() {
        let md = html_to_markdown("<p>Hello <strong>world</strong></p>")
            .await
            .unwrap();
        assert!(md.to_lowercase().contains("hello"));
        assert!(md.contains("world"));
    }
    #[tokio::test]
    async fn plain_text_preserves_punctuation_and_entities() {
        assert_eq!(html_to_plain("<p>C# a_b * [literal] &amp; <a href='https://x.test/a(b)'>link</a></p><script>secret()</script><p>end</p>").await.unwrap(), "C# a_b * [literal] & link end");
        assert_eq!(html_to_plain("").await.unwrap(), "");
    }
}
