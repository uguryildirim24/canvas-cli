//! Wiki page model (`GET /courses/:id/pages`).

use jiff::Timestamp;
use serde::Deserialize;

use crate::serde_util::{deserialize_opt_id, deserialize_opt_timestamp};

/// One Canvas wiki page.
///
/// A page is addressed by its `url` slug, not only by `page_id`; both are kept
/// so a listing and a detail fetch agree on one entity.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct WikiPage {
    #[serde(deserialize_with = "deserialize_opt_id")]
    pub page_id: Option<i64>,
    /// Slug in the course, unique per course.
    pub url: Option<String>,
    pub title: Option<String>,
    #[serde(deserialize_with = "deserialize_opt_timestamp")]
    pub created_at: Option<Timestamp>,
    #[serde(deserialize_with = "deserialize_opt_timestamp")]
    pub updated_at: Option<Timestamp>,
    pub published: Option<bool>,
    pub front_page: Option<bool>,
    pub hide_from_students: Option<bool>,
    pub editing_roles: Option<String>,
    pub locked_for_user: Option<bool>,
    /// Present only on the detail fetch; the listing never asks for it.
    pub body: Option<String>,
    #[serde(deserialize_with = "crate::serde_util::deserialize_opt_url")]
    pub html_url: Option<reqwest::Url>,
}
