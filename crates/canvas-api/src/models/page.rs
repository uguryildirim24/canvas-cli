//! Pagination page assembled from a JSON array plus a Link header.

use reqwest::Url;
use serde::Deserialize;

/// One page of a Canvas collection.
///
/// `items` come from the JSON body. `next` is filled from `Link: rel="next"`
/// and is never read from JSON.
#[derive(Debug, Clone, Deserialize)]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
pub struct Page<T> {
    /// Page items.
    #[serde(default)]
    pub items: Vec<T>,
    /// Next page URL from the Link header (not JSON).
    #[serde(skip_deserializing, default)]
    pub next: Option<Url>,
}

impl<T> Default for Page<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            next: None,
        }
    }
}
