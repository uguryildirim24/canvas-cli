//! Bounded rich-text projection: Markdown plus what the Markdown cannot show.
//!
//! An agent that reads a page body must be told what it did not get. Every
//! `<iframe>`, LTI launch, `<video>`, and `<audio>` becomes an `embedded` row
//! and a one-line placeholder in the Markdown; a same-origin `/files/:id`
//! reference becomes a `files` row; anything on another origin is listed as an
//! external link and is never fetched.

use std::sync::{Arc, Mutex};

use htmd::{Element, HtmlToMarkdown, element_handler::Handlers};
use markup5ever_rcdom::{Handle, NodeData};

use super::MarkdownError;

/// Text bodies are bounded at 64 KiB per document (M8-a).
pub const BODY_LIMIT: usize = 64 * 1024;

/// One piece of content the Markdown cannot carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedRef {
    /// `iframe`, `lti`, `video`, `audio`, or `unknown`.
    pub kind: String,
    /// Origin of the source when it is absolute; `None` when it is relative.
    pub src_origin: Option<String>,
}

/// One same-origin Canvas file the body refers to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRef {
    pub file_id: String,
    pub name: Option<String>,
    pub url: String,
}

/// One reference that leaves the Canvas origin; never fetched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalLink {
    pub url: String,
}

/// A converted body and everything the conversion had to leave out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RichText {
    /// `None` when the source HTML held no text at all.
    pub markdown: Option<String>,
    /// True when the body was cut at [`BODY_LIMIT`].
    pub truncated: bool,
    pub embedded: Vec<EmbeddedRef>,
    pub files: Vec<FileRef>,
    pub external_links: Vec<ExternalLink>,
}

/// Convert one HTML body on the blocking pool and report what it hides.
///
/// `origin` is the active identity origin; a reference to any other origin is
/// external by definition and stays unfetched.
pub async fn rich_text(html: &str, origin: &str) -> Result<RichText, MarkdownError> {
    let html = html.to_owned();
    let origin = origin.to_owned();
    tokio::task::spawn_blocking(move || convert(&html, &origin))
        .await
        .map_err(|_| MarkdownError::Worker)?
}

/// Same as [`rich_text`], for an optional body that may be absent or blank.
pub async fn rich_text_opt(
    html: Option<&str>,
    origin: &str,
) -> Result<RichText, MarkdownError> {
    match html.filter(|s| !s.trim().is_empty()) {
        Some(html) => rich_text(html, origin).await,
        None => Ok(RichText::default()),
    }
}

fn convert(html: &str, origin: &str) -> Result<RichText, MarkdownError> {
    let embedded: Arc<Mutex<Vec<EmbeddedRef>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&embedded);
    let converter = HtmlToMarkdown::builder()
        .add_handler(
            vec!["iframe", "video", "audio", "embed", "object"],
            move |_: &dyn Handlers, element: Element| {
                let get = |name: &str| {
                    element
                        .attrs
                        .iter()
                        .find(|a| a.name.local.as_ref() == name)
                        .map(|a| a.value.to_string())
                };
                let src = get("src").or_else(|| get("data")).unwrap_or_default();
                let kind = embed_kind(element.tag, &src);
                let row = EmbeddedRef {
                    kind: kind.to_owned(),
                    src_origin: absolute_origin(&src),
                };
                let line = format!("\n\n[embedded {kind}: unavailable to this tool]\n\n");
                if let Ok(mut rows) = sink.lock()
                    && !rows.contains(&row)
                {
                    rows.push(row);
                }
                Some(line.into())
            },
        )
        .build();

    let tree = converter
        .html_to_tree(html)
        .map_err(|_| MarkdownError::Convert)?;
    let mut refs = Refs::default();
    collect_refs(&tree, origin, &mut refs);
    let markdown = converter.tree_to_markdown(&tree);
    let markdown = markdown.trim().to_owned();
    let (markdown, truncated) = bound(markdown);

    Ok(RichText {
        markdown: if markdown.is_empty() {
            None
        } else {
            Some(markdown)
        },
        truncated,
        embedded: embedded
            .lock()
            .map(|rows| rows.clone())
            .unwrap_or_default(),
        files: refs.files,
        external_links: refs.external,
    })
}

/// Cut at [`BODY_LIMIT`] bytes, never inside a character.
fn bound(markdown: String) -> (String, bool) {
    if markdown.len() <= BODY_LIMIT {
        return (markdown, false);
    }
    let mut cut = BODY_LIMIT;
    while cut > 0 && !markdown.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut out = markdown;
    out.truncate(cut);
    (out, true)
}

#[derive(Default)]
struct Refs {
    files: Vec<FileRef>,
    external: Vec<ExternalLink>,
}

fn collect_refs(node: &Handle, origin: &str, out: &mut Refs) {
    if let NodeData::Element { name, attrs, .. } = &node.data {
        let attrs = attrs.borrow();
        let get = |wanted: &str| {
            attrs
                .iter()
                .find(|a| a.name.local.as_ref() == wanted)
                .map(|a| a.value.to_string())
        };
        let tag = name.local.as_ref();
        let raw = match tag {
            "a" | "area" => get("href"),
            "img" | "source" => get("src"),
            _ => None,
        };
        if let Some(raw) = raw.filter(|r| !r.trim().is_empty()) {
            let label = get("title")
                .filter(|s| !s.trim().is_empty())
                .or_else(|| text_of(node))
                .or_else(|| get("alt"));
            classify(&raw, label, origin, out);
        }
    }
    for child in node.children.borrow().iter() {
        collect_refs(child, origin, out);
    }
}

fn classify(raw: &str, label: Option<String>, origin: &str, out: &mut Refs) {
    // A fragment or a mail link is neither a Canvas file nor a fetchable page.
    if raw.starts_with('#') || raw.starts_with("mailto:") || raw.starts_with("javascript:") {
        return;
    }
    match resolved(raw, origin) {
        Resolution::SameOrigin(url) => {
            if let Some(file_id) = file_id_of(url.path()) {
                let row = FileRef {
                    file_id,
                    name: label,
                    url: url.to_string(),
                };
                if !out.files.iter().any(|f| f.file_id == row.file_id) {
                    out.files.push(row);
                }
            }
        }
        Resolution::CrossOrigin(url) => {
            // An `<img>` on another origin is a reference too; it stays listed
            // and unfetched, exactly like a cross-origin link.
            let row = ExternalLink { url };
            if !out.external.contains(&row) {
                out.external.push(row);
            }
        }
        Resolution::Unusable => {}
    }
}

enum Resolution {
    SameOrigin(reqwest::Url),
    CrossOrigin(String),
    Unusable,
}

fn resolved(raw: &str, origin: &str) -> Resolution {
    let Ok(base) = reqwest::Url::parse(origin) else {
        return Resolution::Unusable;
    };
    let Ok(url) = base.join(raw) else {
        return Resolution::Unusable;
    };
    if url.scheme() != "https" && url.scheme() != "http" {
        return Resolution::Unusable;
    }
    if url.origin() == base.origin() {
        Resolution::SameOrigin(url)
    } else {
        Resolution::CrossOrigin(url.to_string())
    }
}

/// The Canvas file id in `/files/:id` or `/courses/:cid/files/:id`.
fn file_id_of(path: &str) -> Option<String> {
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    let index = parts.iter().rposition(|p| *p == "files")?;
    let id = parts.get(index + 1)?;
    if id.chars().all(|c| c.is_ascii_digit()) && !id.is_empty() {
        Some((*id).to_owned())
    } else {
        None
    }
}

fn embed_kind(tag: &str, src: &str) -> &'static str {
    match tag {
        "video" => "video",
        "audio" => "audio",
        "iframe" if src.contains("external_tools") || src.contains("/lti/") => "lti",
        "iframe" => "iframe",
        _ => "unknown",
    }
}

/// Origin of an absolute source; `None` for a relative one.
fn absolute_origin(src: &str) -> Option<String> {
    let url = reqwest::Url::parse(src).ok()?;
    let origin = url.origin();
    if origin.is_tuple() {
        Some(origin.ascii_serialization())
    } else {
        None
    }
}

fn text_of(node: &Handle) -> Option<String> {
    let mut out = String::new();
    push_text(node, &mut out);
    let text = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() { None } else { Some(text) }
}

fn push_text(node: &Handle, out: &mut String) {
    if let NodeData::Text { contents } = &node.data {
        out.push_str(&contents.borrow());
    }
    for child in node.children.borrow().iter() {
        push_text(child, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORIGIN: &str = "https://school.instructure.test";

    #[tokio::test]
    async fn reports_embedded_content_and_leaves_a_placeholder() {
        let html = r#"<p>Watch this.</p><iframe src="https://player.test/v/1"></iframe>"#;
        let out = rich_text(html, ORIGIN).await.unwrap();
        assert_eq!(out.embedded.len(), 1);
        assert_eq!(out.embedded[0].kind, "iframe");
        assert_eq!(
            out.embedded[0].src_origin.as_deref(),
            Some("https://player.test")
        );
        let md = out.markdown.unwrap();
        assert!(md.contains("[embedded iframe: unavailable to this tool]"));
        assert!(md.contains("Watch this."));
    }

    #[tokio::test]
    async fn an_lti_launch_is_named_as_one() {
        let html = r#"<iframe src="/courses/1/external_tools/retrieve?url=x"></iframe>"#;
        let out = rich_text(html, ORIGIN).await.unwrap();
        assert_eq!(out.embedded[0].kind, "lti");
        // A relative source has no origin of its own.
        assert!(out.embedded[0].src_origin.is_none());
    }

    #[tokio::test]
    async fn same_origin_files_are_listed_and_cross_origin_links_are_not_files() {
        let html = concat!(
            r#"<a href="/courses/7/files/42?wrap=1">Syllabus.pdf</a>"#,
            r#"<a href="https://elsewhere.test/files/9">away</a>"#,
        );
        let out = rich_text(html, ORIGIN).await.unwrap();
        assert_eq!(out.files.len(), 1);
        assert_eq!(out.files[0].file_id, "42");
        assert_eq!(out.files[0].name.as_deref(), Some("Syllabus.pdf"));
        assert!(out.files[0].url.starts_with(ORIGIN));
        assert_eq!(out.external_links.len(), 1);
        assert_eq!(out.external_links[0].url, "https://elsewhere.test/files/9");
    }

    #[tokio::test]
    async fn a_long_body_is_cut_and_says_so() {
        let html = format!("<p>{}</p>", "x".repeat(BODY_LIMIT + 100));
        let out = rich_text(&html, ORIGIN).await.unwrap();
        assert!(out.truncated);
        assert!(out.markdown.unwrap().len() <= BODY_LIMIT);
    }

    #[tokio::test]
    async fn an_empty_body_has_no_markdown() {
        let out = rich_text_opt(Some("   "), ORIGIN).await.unwrap();
        assert!(out.markdown.is_none());
        assert!(!out.truncated);
        assert!(out.embedded.is_empty());
    }
}
