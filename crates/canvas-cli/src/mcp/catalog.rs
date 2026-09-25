//! The one tool of `canvas mcp` (§21.2).
//!
//! `canvas mcp` serves exactly one tool, `getclitools`, and it performs
//! nothing: it returns the `canvas` command reference and the caller runs the
//! commands itself (project decision, 2026-09-10; §19 item 50). There is no
//! catalog of agent-facing actions any more, so there is no per-tool argument
//! struct, no per-tool output schema, and no effect annotation system — a
//! surface with one read-only discovery tool has nothing to classify.
//!
//! What is absent is absent by design and now absent by construction:
//! credentials, token reveal, identity administration, arbitrary HTTP or
//! shell, `--yes`, cache clearing, `download --force`, and every browser
//! action. None of them has a tool, because nothing has a tool.

use std::borrow::Cow;
use std::sync::Arc;

use rmcp::model::{CallToolResult, ContentBlock, JsonObject, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::mcp::reference;

/// The name of the only tool this server serves.
///
/// It is the name the tool was asked for, used verbatim rather than bent into the dotted
/// convention the removed catalog used. A host that has learned the name must
/// keep finding it.
pub const TOOL: &str = "getclitools";

/// The only tool: its name, its title, its description, and its arguments.
pub struct ToolSpec {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    input_schema: fn() -> JsonObject,
}

impl ToolSpec {
    /// The MCP tool definition.
    ///
    /// There is no `outputSchema`. The answer is one Markdown document, not a
    /// §7 envelope, so a schema would describe nothing a host could validate.
    #[must_use]
    pub fn tool(&self) -> Tool {
        Tool::new(
            Cow::Borrowed(self.name),
            Cow::Borrowed(self.description),
            Arc::new((self.input_schema)()),
        )
        .with_title(self.title)
        .with_annotations(annotations(self.title))
    }
}

/// The tool reads a reference the binary already holds: nothing changes, it
/// is the same answer every time, and it reaches nothing outside the process.
fn annotations(title: &str) -> ToolAnnotations {
    let mut annotations = ToolAnnotations::new();
    annotations.title = Some(title.to_owned());
    annotations.read_only_hint = Some(true);
    annotations.destructive_hint = Some(false);
    annotations.idempotent_hint = Some(true);
    annotations.open_world_hint = Some(false);
    annotations
}

/// `getclitools` takes no arguments: it has one answer, and this is it.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetCliToolsArgs {}

fn schema_of<T: JsonSchema>() -> JsonObject {
    let mut settings = schemars::generate::SchemaSettings::draft2020_12();
    settings.inline_subschemas = true;
    settings.meta_schema = None;
    match schemars::SchemaGenerator::new(settings)
        .into_root_schema_for::<T>()
        .to_value()
    {
        Value::Object(object) => object,
        _ => JsonObject::new(),
    }
}

/// The single tool this server exposes.
static SPEC: ToolSpec = ToolSpec {
    name: TOOL,
    title: "Get the canvas CLI tools",
    description: "Get the complete `canvas` command-line reference: every command, its \
                  operands and flags, and the JSON envelope each one returns. Call it once, \
                  then run `canvas <command> ...` yourself in a shell. This is the only tool \
                  this server has: nothing here reads or writes Canvas.",
    input_schema: schema_of::<GetCliToolsArgs>,
};

/// The single tool this server exposes.
#[must_use]
pub fn spec() -> &'static ToolSpec {
    &SPEC
}

/// Parse `arguments`, rejecting anything the schema does not name.
fn parse<T: for<'de> Deserialize<'de>>(arguments: Option<JsonObject>) -> Result<T, String> {
    let value = Value::Object(arguments.unwrap_or_default());
    serde_json::from_value(value).map_err(|e| e.to_string())
}

/// Run the one tool.
///
/// `Err` is an argument failure: the caller turns it into a JSON-RPC error,
/// because the tool never ran. Nothing else can fail — the reference is built
/// from the binary's own command tree, with no session, no network, and no
/// cache behind it.
pub fn dispatch(name: &str, arguments: Option<JsonObject>) -> Result<CallToolResult, String> {
    if name != TOOL {
        return Err(format!("unknown tool {name}"));
    }
    let _args: GetCliToolsArgs = parse(arguments)?;
    Ok(CallToolResult::success(vec![ContentBlock::text(
        reference::reference(),
    )]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One tool, under the name it was asked for.
    ///
    /// A change that adds a second tool has to delete this test to pass,
    /// which is the point (§19 item 50).
    #[test]
    fn the_only_tool_is_getclitools() {
        let spec = spec();
        assert_eq!(spec.name, "getclitools");
        assert_eq!(spec.tool().name, "getclitools");
    }

    /// The tool fetches a document the binary already holds: it changes
    /// nothing, answers the same way every time, and reaches nothing.
    #[test]
    fn the_tool_is_annotated_as_a_closed_read() {
        let tool = spec().tool();
        let annotations = tool.annotations.expect("annotations");
        assert_eq!(annotations.read_only_hint, Some(true));
        assert_eq!(annotations.destructive_hint, Some(false));
        assert_eq!(annotations.idempotent_hint, Some(true));
        assert_eq!(annotations.open_world_hint, Some(false));
    }

    /// No output schema, because the answer is prose and not an envelope.
    /// The whole point of this round is that a host pays for one tool
    /// definition, not for 22 inlined §7 envelopes.
    #[test]
    fn the_tool_definition_is_small_and_claims_no_output_schema() {
        let tool = spec().tool();
        assert!(tool.output_schema.is_none());
        let bytes = serde_json::to_vec(&tool).expect("serialize").len();
        assert!(bytes < 2_000, "the one tool definition is {bytes} bytes");
    }

    #[test]
    fn the_tool_takes_no_arguments_and_refuses_the_ones_it_is_given() {
        let result = dispatch(TOOL, None).expect("no arguments is the call");
        assert_eq!(result.is_error, Some(false));
        let refused = dispatch(
            TOOL,
            Some(
                serde_json::json!({ "command": "todo" })
                    .as_object()
                    .cloned()
                    .expect("object"),
            ),
        );
        assert!(refused.is_err(), "an unknown argument must not deserialize");
        assert!(dispatch("todo.list", None).is_err(), "there is one tool");
    }

    /// The answer is the command reference, not a stub.
    #[test]
    fn the_answer_enumerates_the_command_line() {
        let result = dispatch(TOOL, None).expect("the call");
        let text = result
            .content
            .first()
            .and_then(ContentBlock::as_text)
            .map(|text| text.text.clone())
            .expect("one text block");
        for command in [
            "### canvas todo",
            "### canvas assignments",
            "### canvas submit",
            "### canvas discussion reply",
            "### canvas inbox send",
            "### canvas sync",
            "### canvas download",
            "### canvas schema",
        ] {
            assert!(text.contains(command), "the answer never names {command}");
        }
    }
}
