//! Resources of `canvas mcp`, namespaced by identity **and** generation.
//!
//! The canonical encoding is
//! `canvas://<identity-key>/<generation>/<path>` (REPORT §3.2). Both the key
//! and the generation are part of the name, so a URI minted for one identity
//! generation cannot address another: after `identity remove` and a new
//! login, every earlier URI is simply not found.
//!
//! A read goes through the same command handler the matching tool uses, so a
//! resource and a tool cannot answer differently.

use rmcp::model::{
    CacheScope, ErrorData, ReadResourceResult, Resource, ResourceContents, ResourceTemplate,
};
use serde_json::json;

use crate::commands::{Globals, handled::Handled, here, receipts, todo};
use crate::mcp::result::ttl_ms;

/// The URI scheme of every resource this server serves.
pub const SCHEME: &str = "canvas://";

/// The identity generation this server instance is bound to (§3.2).
///
/// The instance binds one key and one generation at startup and never
/// re-reads them, so nothing served here can drift to another identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub key: String,
    pub generation: String,
}

impl Binding {
    /// Bind to an opened identity.
    #[must_use]
    pub fn new(key: impl Into<String>, generation: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            generation: generation.into(),
        }
    }

    /// The URI of one resource under this binding.
    #[must_use]
    pub fn uri(&self, path: &str) -> String {
        format!("{SCHEME}{}/{}/{}", self.key, self.generation, path)
    }

    /// The path part of a URI, if it belongs to this binding.
    ///
    /// A different key or a different generation is not an error to explain;
    /// it is a name this server does not serve.
    fn path_of<'a>(&self, uri: &'a str) -> Option<&'a str> {
        let rest = uri.strip_prefix(SCHEME)?;
        let (key, rest) = rest.split_once('/')?;
        let (generation, path) = rest.split_once('/')?;
        if key != self.key || generation != self.generation {
            return None;
        }
        (!path.is_empty()).then_some(path)
    }
}

/// What a resource path names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// `todo`: the default window of what is due.
    Todo,
    /// `course/<id>/assignments`: one course's open assignments.
    CourseAssignments(String),
    /// `receipts`: local receipts and unresolved journals.
    Receipts,
    /// `context/<consumer-handle>`: what that consumer attached to, if it
    /// attached at all.
    Context(String),
}

/// Read a path as a target, or `None` when this server has no such resource.
#[must_use]
pub fn target_of(path: &str) -> Option<Target> {
    let parts: Vec<&str> = path.split('/').collect();
    match parts.as_slice() {
        ["todo"] => Some(Target::Todo),
        ["receipts"] => Some(Target::Receipts),
        ["course", id, "assignments"] if !id.is_empty() => {
            Some(Target::CourseAssignments((*id).to_owned()))
        }
        ["context", handle] if !handle.is_empty() => Some(Target::Context((*handle).to_owned())),
        _ => None,
    }
}

/// The resources that exist without a parameter.
#[must_use]
pub fn listed(binding: &Binding) -> Vec<Resource> {
    vec![
        Resource::new(binding.uri("todo"), "todo")
            .with_title("What is due")
            .with_description(
                "The default window of deadlines and missing work, as `todo.list` returns it.",
            )
            .with_mime_type("application/json"),
        Resource::new(binding.uri("receipts"), "receipts")
            .with_title("Receipts and open journals")
            .with_description("Local submission receipts and unresolved journals.")
            .with_mime_type("application/json"),
    ]
}

/// The parameterized resources.
#[must_use]
pub fn templates(binding: &Binding) -> Vec<ResourceTemplate> {
    vec![
        ResourceTemplate::new(
            binding.uri("course/{course_id}/assignments"),
            "course-assignments",
        )
        .with_title("Course assignments")
        .with_description("One course's open assignments, as `assignments.list` returns them.")
        .with_mime_type("application/json"),
        ResourceTemplate::new(binding.uri("context/{consumer_handle}"), "consumer-context")
            .with_title("Consumer context")
            .with_description(
                "The attached working context of one consumer, as `context.here` \
                 returns it, and metadata only. Reading it attaches nothing: a \
                 consumer that has not called `context.attach` reads \
                 `not_attached`, and a subscription never opts anybody in.",
            )
            .with_mime_type("application/json"),
    ]
}

/// Read one resource.
///
/// `Err` means the URI is not one this server serves — a foreign identity
/// key, another generation, or an unknown path. Everything the command itself
/// reports, including a refusal, comes back inside the envelope.
pub async fn read(
    globals: &Globals,
    binding: &Binding,
    uri: &str,
) -> Result<ReadResourceResult, ErrorData> {
    let Some(target) = binding.path_of(uri).and_then(target_of) else {
        return Err(ErrorData::resource_not_found(
            "no such resource",
            Some(json!({ "uri": uri })),
        ));
    };
    let handled = match target {
        Target::Todo => todo::handle(globals, None, false, false, None).await,
        Target::Receipts => receipts::handle(
            globals,
            receipts::ReceiptsCmd::List {
                course: None,
                state: None,
            },
        ),
        Target::CourseAssignments(course) => {
            crate::commands::assignments::handle(globals, course, None, None).await
        }
        // Metadata only. Text is released by an explicit `context.here` with
        // `include_text`, never by reading or subscribing to a resource.
        Target::Context(handle) => here::handle(globals, None, Some(handle), false).await,
    };
    Ok(contents(uri, &handled))
}

/// Wrap a finished command as resource contents with its freshness budget.
fn contents(uri: &str, handled: &Handled) -> ReadResourceResult {
    let envelope = handled.envelope();
    let document = envelope.to_value();
    let text = serde_json::to_string(&document).unwrap_or_else(|_| "{}".to_owned());
    let budget = ttl_ms(envelope.freshness(), crate::output::now_timestamp());
    ReadResourceResult::new(vec![ResourceContents::TextResourceContents {
        uri: uri.to_owned(),
        mime_type: Some("application/json".to_owned()),
        text,
        meta: None,
    }])
    .with_ttl_ms(budget)
    // Every resource here is one identity's own data.
    .with_cache_scope(CacheScope::Private)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::{Outcome, error_envelope};

    fn binding() -> Binding {
        Binding::new(
            "canvas.test-7-abcd1234",
            "11111111-1111-4111-8111-111111111111",
        )
    }

    #[test]
    fn a_uri_carries_the_key_and_the_generation() {
        assert_eq!(
            binding().uri("todo"),
            "canvas://canvas.test-7-abcd1234/11111111-1111-4111-8111-111111111111/todo"
        );
    }

    #[test]
    fn every_listed_and_templated_uri_is_namespaced() {
        let binding = binding();
        let prefix = format!("{SCHEME}{}/{}/", binding.key, binding.generation);
        for resource in listed(&binding) {
            assert!(resource.uri.starts_with(&prefix), "{}", resource.uri);
        }
        for template in templates(&binding) {
            assert!(
                template.uri_template.starts_with(&prefix),
                "{}",
                template.uri_template
            );
        }
    }

    #[test]
    fn a_second_generation_addresses_nothing() {
        let binding = binding();
        let other = Binding::new(&binding.key, "22222222-2222-4222-8222-222222222222");
        let uri = other.uri("todo");
        assert!(binding.path_of(&uri).is_none(), "{uri}");
        assert_eq!(other.path_of(&uri), Some("todo"));
    }

    #[test]
    fn another_identity_addresses_nothing() {
        let binding = binding();
        let other = Binding::new("other.test-9-99999999", &binding.generation);
        assert!(binding.path_of(&other.uri("receipts")).is_none());
    }

    #[test]
    fn only_the_documented_paths_resolve() {
        assert_eq!(target_of("todo"), Some(Target::Todo));
        assert_eq!(target_of("receipts"), Some(Target::Receipts));
        assert_eq!(
            target_of("course/1234/assignments"),
            Some(Target::CourseAssignments("1234".to_owned()))
        );
        assert_eq!(
            target_of("context/abc"),
            Some(Target::Context("abc".to_owned()))
        );
        for path in [
            "",
            "todo/extra",
            "course/1234",
            "course//assignments",
            "context/",
            "../todo",
            "identity",
            "auth/token",
        ] {
            assert_eq!(target_of(path), None, "{path}");
        }
    }

    #[test]
    fn a_uri_without_a_generation_resolves_to_nothing() {
        let binding = binding();
        for uri in [
            format!("{SCHEME}{}/todo", binding.key),
            format!("{SCHEME}{}", binding.key),
            "canvas://".to_owned(),
            format!("file:///{}/todo", binding.key),
        ] {
            assert!(binding.path_of(&uri).is_none(), "{uri}");
        }
    }

    #[test]
    fn resource_contents_are_private_and_carry_the_budget() {
        let uri = binding().uri("context/consumer-1");
        // Any unresolved answer will do: what is asserted is the wrapper.
        let mut envelope = error_envelope(
            "refused",
            "no context is attached",
            None,
            json!({ "reason": "not_attached" }),
            8,
        );
        envelope.outcome = Outcome::Refused;
        let handled = Handled::error_envelope(envelope, "no context is attached".to_owned());
        let result = contents(&uri, &handled);
        assert_eq!(result.cache_scope, Some(CacheScope::Private));
        // Nothing is resolved, so the result may not be cached at all.
        assert_eq!(result.ttl_ms, Some(0));
        match &result.contents[0] {
            ResourceContents::TextResourceContents { uri: at, text, .. } => {
                assert_eq!(at, &uri);
                assert!(text.contains("\"not_attached\""), "{text}");
            }
            other => panic!("expected text contents, got {other:?}"),
        }
    }
}
