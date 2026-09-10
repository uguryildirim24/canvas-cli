//! `GET /users/self` user profile.

use reqwest::Url;
use serde::Deserialize;

use crate::serde_util::{deserialize_id, deserialize_opt_url};

/// Canvas user.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct User {
    #[serde(deserialize_with = "deserialize_id")]
    pub id: i64,
    pub name: Option<String>,
    pub short_name: Option<String>,
    pub sortable_name: Option<String>,
    pub login_id: Option<String>,
    pub email: Option<String>,
    pub primary_email: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_url")]
    pub avatar_url: Option<Url>,
    pub time_zone: Option<String>,
    pub locale: Option<String>,
    pub bio: Option<String>,
    pub title: Option<String>,
    pub pronouns: Option<String>,
}
