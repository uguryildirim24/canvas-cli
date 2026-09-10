//! Discussion topic and entry models.

use jiff::Timestamp;
use serde::Deserialize;
use serde_json::Value;

use crate::serde_util::{deserialize_id, deserialize_opt_id, deserialize_opt_timestamp};

/// One child topic of a group discussion.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct GroupTopicChild {
    #[serde(deserialize_with = "deserialize_opt_id")]
    pub id: Option<i64>,
    #[serde(deserialize_with = "deserialize_opt_id")]
    pub group_id: Option<i64>,
}

/// A discussion topic (`only_announcements=false` listing, or one topic).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct DiscussionTopic {
    #[serde(deserialize_with = "deserialize_id")]
    pub id: i64,
    pub title: Option<String>,
    /// Topic body HTML; the listing supplies it too.
    pub message: Option<String>,
    #[serde(deserialize_with = "deserialize_opt_timestamp")]
    pub posted_at: Option<Timestamp>,
    #[serde(deserialize_with = "deserialize_opt_timestamp")]
    pub last_reply_at: Option<Timestamp>,
    #[serde(deserialize_with = "deserialize_opt_timestamp")]
    pub delayed_post_at: Option<Timestamp>,
    pub discussion_type: Option<String>,
    pub user_name: Option<String>,
    pub author: Option<Value>,
    pub read_state: Option<String>,
    pub unread_count: Option<u64>,
    pub discussion_subentry_count: Option<u64>,
    pub published: Option<bool>,
    pub locked: Option<bool>,
    pub locked_for_user: Option<bool>,
    pub pinned: Option<bool>,
    pub require_initial_post: Option<bool>,
    pub user_can_see_posts: Option<bool>,
    pub is_announcement: Option<bool>,
    pub subscribed: Option<bool>,
    #[serde(deserialize_with = "deserialize_opt_id")]
    pub assignment_id: Option<i64>,
    pub points_possible: Option<f64>,
    #[serde(deserialize_with = "deserialize_opt_id")]
    pub group_category_id: Option<i64>,
    pub group_topic_children: Vec<GroupTopicChild>,
    pub context_code: Option<String>,
    pub assignment: Option<Value>,
    #[serde(deserialize_with = "crate::serde_util::deserialize_opt_url")]
    pub html_url: Option<reqwest::Url>,
}

/// One entry in a discussion, or one reply to an entry.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct DiscussionEntry {
    #[serde(deserialize_with = "deserialize_id")]
    pub id: i64,
    #[serde(deserialize_with = "deserialize_opt_id")]
    pub parent_id: Option<i64>,
    #[serde(deserialize_with = "deserialize_opt_id")]
    pub user_id: Option<i64>,
    pub user_name: Option<String>,
    pub message: Option<String>,
    #[serde(deserialize_with = "deserialize_opt_timestamp")]
    pub created_at: Option<Timestamp>,
    #[serde(deserialize_with = "deserialize_opt_timestamp")]
    pub updated_at: Option<Timestamp>,
    pub read_state: Option<String>,
    pub recent_replies: Vec<DiscussionEntry>,
    pub has_more_replies: Option<bool>,
    /// Canvas names this `deleted` on a removed entry.
    pub deleted: Option<bool>,
}

impl DiscussionEntry {
    /// Replies Canvas already counted for this entry.
    #[must_use]
    pub fn replies_count(&self) -> u64 {
        self.recent_replies.len() as u64
    }
}
