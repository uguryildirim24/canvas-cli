//! Conversation (inbox) models.

use jiff::Timestamp;
use serde::{Deserialize, Deserializer};

use crate::serde_util::{deserialize_id, deserialize_opt_id, deserialize_opt_timestamp};

/// One participant of a conversation.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct ConversationParticipant {
    #[serde(deserialize_with = "deserialize_opt_id")]
    pub id: Option<i64>,
    pub name: Option<String>,
    pub full_name: Option<String>,
}

/// One attachment on a conversation message.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct ConversationAttachment {
    #[serde(deserialize_with = "deserialize_opt_id")]
    pub id: Option<i64>,
    pub display_name: Option<String>,
    pub filename: Option<String>,
    pub size: Option<u64>,
}

/// One message inside a conversation.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct ConversationMessage {
    #[serde(deserialize_with = "deserialize_opt_id")]
    pub id: Option<i64>,
    #[serde(deserialize_with = "deserialize_opt_id")]
    pub author_id: Option<i64>,
    #[serde(deserialize_with = "deserialize_opt_timestamp")]
    pub created_at: Option<Timestamp>,
    pub body: Option<String>,
    pub generated: Option<bool>,
    pub attachments: Vec<ConversationAttachment>,
}

/// One conversation, as the listing or the detail fetch reports it.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Conversation {
    #[serde(deserialize_with = "deserialize_id")]
    pub id: i64,
    pub subject: Option<String>,
    pub workflow_state: Option<String>,
    /// Listing preview of the newest message; never a full body.
    pub last_message: Option<String>,
    #[serde(deserialize_with = "deserialize_opt_timestamp")]
    pub last_message_at: Option<Timestamp>,
    pub message_count: Option<u64>,
    pub subscribed: Option<bool>,
    pub private: Option<bool>,
    pub starred: Option<bool>,
    pub context_name: Option<String>,
    pub participants: Vec<ConversationParticipant>,
    /// Present only on the detail fetch.
    pub messages: Vec<ConversationMessage>,
}

/// `GET /conversations/unread_count`.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct UnreadCount {
    /// Canvas documents this as a string and some deployments send a number,
    /// so both are accepted and anything else stays unknown.
    #[serde(deserialize_with = "deserialize_lenient_count")]
    pub unread_count: Option<String>,
}

/// A count as a string, as a number, or unknown; never a decode failure.
///
/// The count is the whole answer of `inbox unread-count`; refusing the read
/// because the wire type moved would be worse than reporting it unknown.
fn deserialize_lenient_count<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    Ok(
        match Option::<serde_json::Value>::deserialize(deserializer)? {
            Some(serde_json::Value::String(s)) => Some(s),
            Some(serde_json::Value::Number(n)) => Some(n.to_string()),
            _ => None,
        },
    )
}
