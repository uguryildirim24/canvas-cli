//! Bounded rich-text projection: Markdown plus what the Markdown cannot show.
//!
//! An agent that reads a body must be told what it did not get. Every
//! `<iframe>`, LTI launch, `<video>`, and `<audio>` becomes an embedded row and
//! a one-line placeholder in the Markdown; every link and image source is kept
//! as written apart from its capability-bearing parts, so the reader can
//! resolve it against its own identity origin without a signed URL ever
//! reaching the cache or the JSON (§15).
//!
//! Conversion never keeps the source HTML: [`BodyRefs`] is the projection the
//! cache may store, and [`BodyRefs::resolve`] turns it into same-origin file
//! references and external links at read time.

use std::sync::{Arc, Mutex};

use htmd::{Element, HtmlToMarkdown, element_handler::Handlers};
use markup5ever_rcdom::{Handle, NodeData};
use serde::{Deserialize, Serialize};

use super::MarkdownError;

/// Text bodies are bounded at 64 KiB per document (M8-a).
pub const BODY_LIMIT: usize = 64 * 1024;

/// One piece of content the Markdown cannot carry, as written in the source.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawEmbed {
    /// `iframe`, `lti`, `video`, `audio`, or `unknown`.
    pub kind: String,
    /// The source as the document wrote it, without its capability parameters;
    /// may be relative.
    pub src: Option<String>,
}

/// One link or image source, as written in the source, without its
/// capability-bearing query parameters or userinfo.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawLink {
    pub href: String,
    pub label: Option<String>,
}

/// Everything a converted body refers to but does not contain.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BodyRefs {
    /// True when the Markdown these came from was cut at [`BODY_LIMIT`].
    #[serde(default)]
    pub truncated: bool,
    #[serde(default)]
    pub embedded: Vec<RawEmbed>,
    #[serde(default)]
    pub links: Vec<RawLink>,
}

/// A converted body and everything the conversion had to leave out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RichText {
    /// `None` when the source HTML held no text at all.
    pub markdown: Option<String>,
    pub refs: BodyRefs,
}

/// One piece of content this tool will not fetch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedRef {
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

/// References resolved against one identity origin.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolvedRefs {
    pub embedded: Vec<EmbeddedRef>,
    pub files: Vec<FileRef>,
    pub external_links: Vec<ExternalLink>,
}

/// Convert one HTML body on the blocking pool and report what it hides.
pub async fn rich_text(html: &str) -> Result<RichText, MarkdownError> {
    let html = html.to_owned();
    tokio::task::spawn_blocking(move || convert(&html))
        .await
        .map_err(|_| MarkdownError::Worker)?
}

/// Same as [`rich_text`], for an optional body that may be absent or blank.
pub async fn rich_text_opt(html: Option<&str>) -> Result<RichText, MarkdownError> {
    match html.filter(|s| !s.trim().is_empty()) {
        Some(html) => rich_text(html).await,
        None => Ok(RichText::default()),
    }
}

fn convert(html: &str) -> Result<RichText, MarkdownError> {
    let embedded: Arc<Mutex<Vec<RawEmbed>>> = Arc::new(Mutex::new(Vec::new()));
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
                let src = get("src")
                    .or_else(|| get("data"))
                    .map(|raw| sanitize_ref(&raw));
                let kind = embed_kind(element.tag, src.as_deref().unwrap_or_default());
                let row = RawEmbed {
                    kind: kind.to_owned(),
                    src,
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
    let mut links = Vec::new();
    // The sweep rewrites each reference in the tree before the Markdown is
    // rendered from it, so the capability is gone from the body as well as
    // from the projection.
    collect_links(&tree, &mut links);
    let markdown = converter.tree_to_markdown(&tree).trim().to_owned();
    let (markdown, truncated) = bound(markdown);

    Ok(RichText {
        markdown: if markdown.is_empty() {
            None
        } else {
            Some(markdown)
        },
        refs: BodyRefs {
            truncated,
            embedded: embedded.lock().map(|rows| rows.clone()).unwrap_or_default(),
            links,
        },
    })
}

impl BodyRefs {
    /// Resolve every reference against `origin`.
    ///
    /// A `/files/:id` reference on the identity origin is a Canvas file this
    /// tool could read; anything on another origin is listed and never fetched.
    #[must_use]
    pub fn resolve(&self, origin: &str) -> ResolvedRefs {
        let mut out = ResolvedRefs {
            embedded: self
                .embedded
                .iter()
                .map(|e| EmbeddedRef {
                    kind: e.kind.clone(),
                    src_origin: e.src.as_deref().and_then(absolute_origin),
                })
                .collect(),
            ..ResolvedRefs::default()
        };
        for link in &self.links {
            match resolved(&link.href, origin) {
                Resolution::SameOrigin(url) => {
                    let Some(file_id) = file_id_of(url.path()) else {
                        continue;
                    };
                    if out.files.iter().any(|f| f.file_id == file_id) {
                        continue;
                    }
                    out.files.push(FileRef {
                        file_id,
                        name: link.label.clone(),
                        url: url.to_string(),
                    });
                }
                Resolution::CrossOrigin(url) => {
                    let row = ExternalLink { url };
                    if !out.external_links.contains(&row) {
                        out.external_links.push(row);
                    }
                }
                Resolution::Unusable => {}
            }
        }
        out
    }
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

/// Collect every reference, rewriting each one in the tree without its
/// capability-bearing parts so the rendered Markdown carries none either.
fn collect_links(node: &Handle, out: &mut Vec<RawLink>) {
    if let NodeData::Element { name, attrs, .. } = &node.data {
        let wanted = match name.local.as_ref() {
            "a" | "area" => Some("href"),
            "img" | "source" => Some("src"),
            _ => None,
        };
        let mut attrs = attrs.borrow_mut();
        let mut href = None;
        if let Some(wanted) = wanted
            && let Some(attr) = attrs.iter_mut().find(|a| a.name.local.as_ref() == wanted)
        {
            let clean = sanitize_ref(&attr.value);
            attr.value = clean.as_str().into();
            href = Some(clean);
        }
        let get = |wanted: &str| {
            attrs
                .iter()
                .find(|a| a.name.local.as_ref() == wanted)
                .map(|a| a.value.to_string())
        };
        if let Some(href) = href.filter(|h| usable(h)) {
            let label = get("title")
                .filter(|s| !s.trim().is_empty())
                .or_else(|| text_of(node))
                .or_else(|| get("alt"));
            let row = RawLink { href, label };
            if !out.contains(&row) {
                out.push(row);
            }
        }
        drop(attrs);
    }
    for child in node.children.borrow().iter() {
        collect_links(child, out);
    }
}

/// Drop the capability-bearing parts of a reference before it is stored or shown.
///
/// A Canvas body can link to a signed or `verifier`-bearing URL. §15 keeps a
/// capability out of the cache and out of the JSON, so the userinfo and every
/// capability-bearing query parameter go; the rest of the reference is kept as
/// written, because it is what tells the reader where the link points.
fn sanitize_ref(raw: &str) -> String {
    let Some((head, tail)) = raw.split_once('?') else {
        return strip_userinfo(raw);
    };
    let (query, fragment) = match tail.split_once('#') {
        Some((query, fragment)) => (query, Some(fragment)),
        None => (tail, None),
    };
    let kept: Vec<&str> = query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .filter(|pair| {
            let key = pair.split_once('=').map_or(*pair, |(key, _)| key);
            !canvas_api::redact::is_capability_key(key)
        })
        .collect();
    let mut out = strip_userinfo(head);
    if !kept.is_empty() {
        out.push('?');
        out.push_str(&kept.join("&"));
    }
    if let Some(fragment) = fragment {
        out.push('#');
        out.push_str(fragment);
    }
    out
}

/// Remove `user:password@` from an absolute reference.
fn strip_userinfo(raw: &str) -> String {
    let Some((scheme, rest)) = raw.split_once("://") else {
        return raw.to_owned();
    };
    let (authority, path) = rest.find('/').map_or((rest, ""), |i| rest.split_at(i));
    match authority.rsplit_once('@') {
        Some((_, host)) => format!("{scheme}://{host}{path}"),
        None => raw.to_owned(),
    }
}

/// A fragment, a mail link, or a script URL is not a fetchable reference.
fn usable(href: &str) -> bool {
    let href = href.trim();
    !href.is_empty()
        && !href.starts_with('#')
        && !href.starts_with("mailto:")
        && !href.starts_with("javascript:")
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
    if !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()) {
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
        let out = rich_text(html).await.unwrap();
        let refs = out.refs.resolve(ORIGIN);
        assert_eq!(refs.embedded.len(), 1);
        assert_eq!(refs.embedded[0].kind, "iframe");
        assert_eq!(
            refs.embedded[0].src_origin.as_deref(),
            Some("https://player.test")
        );
        let md = out.markdown.unwrap();
        assert!(md.contains("[embedded iframe: unavailable to this tool]"));
        assert!(md.contains("Watch this."));
    }

    #[tokio::test]
    async fn an_lti_launch_is_named_as_one() {
        let html = r#"<iframe src="/courses/1/external_tools/retrieve?url=x"></iframe>"#;
        let refs = rich_text(html).await.unwrap().refs.resolve(ORIGIN);
        assert_eq!(refs.embedded[0].kind, "lti");
        // A relative source has no origin of its own.
        assert!(refs.embedded[0].src_origin.is_none());
    }

    #[tokio::test]
    async fn same_origin_files_are_listed_and_cross_origin_links_are_not_files() {
        let html = concat!(
            r#"<a href="/courses/7/files/42?wrap=1">Syllabus.pdf</a>"#,
            r#"<a href="https://elsewhere.test/files/9">away</a>"#,
        );
        let refs = rich_text(html).await.unwrap().refs.resolve(ORIGIN);
        assert_eq!(refs.files.len(), 1);
        assert_eq!(refs.files[0].file_id, "42");
        assert_eq!(refs.files[0].name.as_deref(), Some("Syllabus.pdf"));
        assert!(refs.files[0].url.starts_with(ORIGIN));
        assert_eq!(refs.external_links.len(), 1);
        assert_eq!(refs.external_links[0].url, "https://elsewhere.test/files/9");
    }

    /// The stored projection carries no HTML, so a cache that keeps it never
    /// keeps a raw response body.
    #[tokio::test]
    async fn the_stored_projection_is_json_without_markup() {
        let html = r#"<p>Hi</p><iframe src="https://player.test/v/1"></iframe>"#;
        let refs = rich_text(html).await.unwrap().refs;
        let stored = serde_json::to_string(&refs).unwrap();
        assert!(!stored.contains('<'));
        let back: BodyRefs = serde_json::from_str(&stored).unwrap();
        assert_eq!(back, refs);
    }

    /// A body can link to a signed URL; the capability never reaches the
    /// projection the cache stores or the references the JSON reports.
    #[tokio::test]
    async fn a_capability_bearing_reference_loses_its_capability() {
        let html = concat!(
            r#"<a href="/courses/7/files/42/download?verifier=private-capability&wrap=1">h</a>"#,
            r#"<a href="https://elsewhere.test/x?sig=private-sig&page=2#top">away</a>"#,
            r#"<iframe src="https://player.test/v/1?access_token=private-token"></iframe>"#,
            r#"<a href="https://user:private-password@elsewhere.test/y">who</a>"#,
        );
        let out = rich_text(html).await.unwrap();
        let markdown = out.markdown.clone().unwrap();
        assert!(!markdown.contains("private-"), "{markdown}");
        let stored = serde_json::to_string(&out.refs).unwrap();
        assert!(!stored.contains("private-"), "{stored}");
        let refs = out.refs.resolve(ORIGIN);
        assert_eq!(refs.files.len(), 1);
        assert_eq!(refs.files[0].file_id, "42");
        assert!(refs.files[0].url.ends_with("/download?wrap=1"));
        let external: Vec<&str> = refs.external_links.iter().map(|l| l.url.as_str()).collect();
        assert_eq!(
            external,
            [
                "https://elsewhere.test/x?page=2#top",
                "https://elsewhere.test/y"
            ]
        );
        assert_eq!(
            refs.embedded[0].src_origin.as_deref(),
            Some("https://player.test")
        );
    }

    #[tokio::test]
    async fn a_long_body_is_cut_and_says_so() {
        let html = format!("<p>{}</p>", "x".repeat(BODY_LIMIT + 100));
        let out = rich_text(&html).await.unwrap();
        assert!(out.refs.truncated);
        assert!(out.markdown.unwrap().len() <= BODY_LIMIT);
    }

    #[tokio::test]
    async fn an_empty_body_has_no_markdown() {
        let out = rich_text_opt(Some("   ")).await.unwrap();
        assert!(out.markdown.is_none());
        assert!(!out.refs.truncated);
        assert!(out.refs.embedded.is_empty());
    }
}
