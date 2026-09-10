//! The `ServerHandler` of `canvas mcp`.
//!
//! Two protocol revisions are implemented:
//!
//! - `2026-07-28` is primary. It has no `initialize` handshake: every request
//!   carries its own version, client identity, and capabilities in `_meta`,
//!   and a host discovers the server with `server/discover`.
//! - `2025-11-25` is reached through the `initialize` handshake and is the
//!   adapter for hosts that have not moved yet.
//!
//! Any other revision fails the handshake. That refusal is explicit: the SDK
//! would otherwise answer an unknown `initialize` version with its newest
//! legacy version, which would silently pretend to speak something the host
//! did not ask for.

use std::borrow::Cow;
use std::future::Future;

use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CacheScope, CallToolRequestParams, CallToolResponse, ClientCapabilities, DiscoverResult,
    ElicitRequest, ElicitRequestParams, ElicitResult, ElicitationAction, ElicitationSchema,
    ErrorCode, ErrorData, Implementation, InitializeRequestParams, InitializeResult, InputRequest,
    InputRequests, InputRequiredResult, InputResponses, ListResourceTemplatesResult,
    ListResourcesResult, ListToolsResult, PaginatedRequestParams, ProtocolVersion,
    ReadResourceRequestParams, ReadResourceResponse, ServerCapabilities, ServerInfo,
    SubscriptionFilter,
};
use rmcp::service::{RequestContext, RoleServer};

use crate::commands::submit::{Pending, Refusal};
use crate::commands::{Globals, submit};
use crate::mcp::catalog::Dispatched;
use crate::mcp::resources::Binding;
use crate::mcp::{catalog, resources, result};
use crate::output::now_timestamp;

/// The revisions this server implements, newest first.
pub const SUPPORTED: &[ProtocolVersion] =
    &[ProtocolVersion::V_2026_07_28, ProtocolVersion::V_2025_11_25];

/// The `inputRequests` key the approval round trip uses.
///
/// One key, because one decision is asked for. The host echoes it in
/// `inputResponses` on the retry.
const APPROVAL_KEY: &str = "approval";

/// The consumer id recorded when a host does not name itself.
const ANONYMOUS_CONSUMER: &str = "mcp";

/// The one tool that can return `input_required`, and so the only tool whose
/// retry may carry a `requestState`.
const APPROVAL_TOOL: &str = "submission.execute";

/// How long a host may cache the shape of this server, in milliseconds.
///
/// The catalog and the resource list are fixed when the binary is built, and
/// the instance stops when its identity generation changes, so the surface
/// cannot change under a host that is holding a connection. One minute keeps
/// a restart visible quickly all the same.
const SURFACE_TTL_MS: u64 = 60_000;

/// Guidance a host shows the model with the tool list.
const INSTRUCTIONS: &str = "\
Canvas LMS, read-first. Every tool returns one JSON envelope with `outcome`, \
`exit`, `freshness`, and `result`; read `outcome` before the result. \
`freshness` says which cached dataset answered and whether it is stale; \
`sync.run` refreshes the cache. A refusal is an answer, not an error: \
`outcome` `refused` with `exit` 8 means the operation is not allowed as \
asked. Nothing here reveals a credential, changes an identity, or opens a \
browser. A submission needs a recorded human approval before anything \
reaches Canvas.";

/// One `canvas mcp` instance: one identity generation, one catalog.
pub struct CanvasServer {
    globals: Globals,
    binding: Binding,
}

impl CanvasServer {
    /// Bind a server to one identity generation.
    #[must_use]
    pub fn new(globals: Globals, binding: Binding) -> Self {
        Self { globals, binding }
    }

    fn capabilities() -> ServerCapabilities {
        ServerCapabilities::builder()
            .enable_tools()
            .enable_resources()
            // The resource list is fixed for the lifetime of an instance in
            // this release, so `subscriptions/listen` is answered and no
            // notification is ever sent.
            .enable_resources_list_changed()
            .build()
    }

    fn implementation() -> Implementation {
        Implementation::new("canvas-cli", env!("CARGO_PKG_VERSION"))
            .with_title("Canvas CLI")
            .with_description("Canvas LMS for one identity, over the local `canvas` binary.")
    }
}

impl CanvasServer {
    /// Record the decision a host collected and finish the tool call.
    ///
    /// `requestState` is untrusted, so it carries only two identifiers. The
    /// handle it names is validated against the stored plan inside
    /// `approve`, which is why an echoed state cannot approve anything on its
    /// own.
    async fn record_decision(
        &self,
        state: &str,
        responses: Option<&InputResponses>,
        consumer: &str,
    ) -> Result<crate::commands::handled::Handled, ErrorData> {
        let state: ApprovalState = serde_json::from_str(state)
            .map_err(|e| ErrorData::invalid_params(format!("requestState: {e}"), None))?;
        let answer = responses
            .and_then(|responses| responses.get(APPROVAL_KEY))
            .ok_or_else(|| {
                ErrorData::invalid_params(
                    format!("the retry carries no `{APPROVAL_KEY}` response"),
                    None,
                )
            })?;
        let answer: ElicitResult = serde_json::from_value(answer.clone())
            .map_err(|e| ErrorData::invalid_params(format!("{APPROVAL_KEY}: {e}"), None))?;
        match answer.action {
            ElicitationAction::Accept => {
                let handle = answer
                    .content
                    .as_ref()
                    .and_then(|content| content.get("handle"))
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        ErrorData::invalid_params(
                            "an accepted approval must echo `handle`".to_owned(),
                            None,
                        )
                    })?
                    .to_owned();
                Ok(submit::agent_approve(&self.globals, &state.plan_id, &handle, consumer).await)
            }
            ElicitationAction::Decline => Ok(submit::agent_refuse(
                &self.globals,
                &state.plan_id,
                Refusal::Declined,
            )),
            // Every remaining action stops the operation.
            _ => Ok(submit::agent_refuse(
                &self.globals,
                &state.plan_id,
                Refusal::Cancelled,
            )),
        }
    }
}

/// What travels in `requestState` across the approval round trip.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct ApprovalState {
    plan_id: String,
    handle: String,
}

/// The consumer id this session records with plans and approval handles.
///
/// It names the host, so an approval audit says who asked. Both halves of an
/// approval round trip come from the same client, so it is stable across them.
fn consumer_of(context: &RequestContext<RoleServer>) -> String {
    context.client_info().map_or_else(
        || ANONYMOUS_CONSUMER.to_owned(),
        |info| format!("{ANONYMOUS_CONSUMER}:{}", info.name),
    )
}

/// Whether this host can show a person a form.
///
/// An empty `elicitation` object is the 2025-06-18 declaration, which is form
/// mode; a host that declares URL mode only cannot answer this server.
fn declares_form_elicitation(capabilities: Option<&ClientCapabilities>) -> bool {
    capabilities
        .and_then(|capabilities| capabilities.elicitation.as_ref())
        .is_some_and(|elicitation| elicitation.form.is_some() || elicitation.url.is_none())
}

/// Ask a person to approve one plan (MRTR `input_required`).
///
/// The message names the exact bytes: the digest of the plan is what the
/// approval binds. The handle is server-issued and travels in `requestState`;
/// the host echoes it in `inputResponses`, so an approval cannot be asserted
/// by a tool argument.
fn ask_approval(pending: &Pending) -> Result<InputRequiredResult, ErrorData> {
    let schema = ElicitationSchema::builder()
        .title("Approve this submission")
        .description("Echo the approval handle from `requestState` to approve.")
        .required_string_property("handle", |handle| {
            handle
                .title("Approval handle")
                .description("The `handle` field of `requestState`, copied verbatim.")
        })
        .build()
        .map_err(|e| ErrorData::internal_error(format!("approval schema: {e}"), None))?;
    let params = ElicitRequestParams::FormElicitationParams {
        meta: None,
        message: approval_message(pending),
        requested_schema: schema,
    };
    let mut requests = InputRequests::new();
    requests.insert(
        APPROVAL_KEY.to_owned(),
        InputRequest::Elicitation(ElicitRequest::new(params)),
    );
    let state = serde_json::to_string(&ApprovalState {
        plan_id: pending.plan_id.clone(),
        handle: pending.handle.clone(),
    })
    .map_err(|e| ErrorData::internal_error(format!("requestState: {e}"), None))?;
    Ok(InputRequiredResult::new(Some(requests), Some(state)))
}

/// What a person reads before approving.
fn approval_message(pending: &Pending) -> String {
    use std::fmt::Write;

    let plan = &pending.plan;
    let mut message = pending.summary.clone();
    for file in &plan.files {
        let _ = write!(
            message,
            "\n  {} ({} bytes, sha256 {})",
            file.name, file.size, file.sha256
        );
    }
    if let Some(text) = &plan.text {
        let _ = write!(message, "\n  text sent_sha256 {}", text.sent_sha256);
    }
    if let Some(url) = &plan.url {
        let _ = write!(message, "\n  url {url}");
    }
    let _ = write!(
        message,
        "\n  plan {}  digest {}  expires {}",
        plan.plan_id, plan.plan_sha256, plan.expires_at
    );
    message.push_str("\nAccept to submit. Decline or cancel and the plan is invalidated.");
    message
}

impl ServerHandler for CanvasServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(Self::capabilities())
            .with_protocol_version(ProtocolVersion::V_2026_07_28)
            .with_server_info(Self::implementation())
            .with_instructions(INSTRUCTIONS)
    }

    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(SUPPORTED)
    }

    /// Refuse a revision this server does not implement.
    ///
    /// The SDK's own negotiation falls back to the newest legacy version when
    /// it does not recognize what the host asked for. This server answers only
    /// what it implements, so an unknown version fails the handshake here.
    fn initialize(
        &self,
        request: InitializeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<InitializeResult, ErrorData>> {
        let requested = request.protocol_version.clone();
        let supported = self.supported_protocol_versions();
        let info = self.get_info();
        async move {
            if !supported.contains(&requested) {
                return Err(ErrorData::unsupported_protocol_version(
                    requested, &supported,
                ));
            }
            context.peer.set_peer_info(request);
            Ok(info.with_protocol_version(requested))
        }
    }

    fn discover(
        &self,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<DiscoverResult, ErrorData>> {
        let result = DiscoverResult::from_server_info(SUPPORTED.to_vec(), self.get_info())
            .with_ttl_ms(SURFACE_TTL_MS)
            // One instance serves one identity: never a shared cache entry.
            .with_cache_scope(CacheScope::Private);
        std::future::ready(Ok(result))
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, ErrorData>> {
        let tools = catalog::specs()
            .iter()
            .map(catalog::ToolSpec::tool)
            .collect();
        std::future::ready(Ok(ListToolsResult::with_all_items(tools)
            .with_ttl_ms(SURFACE_TTL_MS)
            .with_cache_scope(CacheScope::Private)))
    }

    fn get_tool(&self, name: &str) -> Option<rmcp::model::Tool> {
        catalog::spec(name).map(catalog::ToolSpec::tool)
    }

    /// Run one tool and return its §7 envelope.
    ///
    /// A domain failure is a result, not a protocol error: the envelope keeps
    /// `outcome` and `exit`, and the result is marked `isError` so the host
    /// shows it. Only an unroutable request — an unknown tool, or arguments
    /// the tool's schema does not admit — becomes a JSON-RPC error, because
    /// then the tool never ran.
    ///
    /// A retry that carries `requestState` is the second half of an approval
    /// round trip: it records the person's decision instead of dispatching
    /// the tool again, so a plan is never frozen twice.
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let name = request.name.to_string();
        if catalog::spec(&name).is_none() {
            return Err(ErrorData::new(
                ErrorCode::METHOD_NOT_FOUND,
                format!("unknown tool {name}"),
                None,
            ));
        }
        let consumer = consumer_of(&context);
        if let Some(state) = request.request_state.as_deref() {
            // Only one tool ever asks for a decision, so only that tool can be
            // the second half of a round trip. A retry that names any other
            // tool is a host bug or a replayed state, and answering it would
            // record an approval against a call that never asked for one.
            if name != APPROVAL_TOOL {
                return Err(ErrorData::invalid_params(
                    format!(
                        "{name} never asks for an approval: `requestState` belongs to {APPROVAL_TOOL}"
                    ),
                    Some(serde_json::json!({ "tool": name })),
                ));
            }
            let handled = self
                .record_decision(state, request.input_responses.as_ref(), &consumer)
                .await?;
            return Ok(CallToolResponse::Complete(result::tool_result(
                handled.envelope(),
                now_timestamp(),
            )));
        }
        let dispatched = catalog::dispatch(&self.globals, &consumer, &name, request.arguments)
            .await
            .map_err(|message| {
                ErrorData::invalid_params(
                    format!("{name}: {message}"),
                    Some(serde_json::json!({ "tool": name })),
                )
            })?;
        let handled = match dispatched {
            Dispatched::Done(handled) => handled,
            Dispatched::Approval(pending) => {
                // A host that cannot ask a person gets a refusal, and nothing
                // is dispatched (REPORT §3.2).
                if declares_form_elicitation(context.client_capabilities().as_ref()) {
                    return Ok(CallToolResponse::InputRequired(ask_approval(&pending)?));
                }
                submit::agent_approval_required(&self.globals, &pending)
            }
        };
        Ok(CallToolResponse::Complete(result::tool_result(
            handled.envelope(),
            now_timestamp(),
        )))
    }

    fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListResourcesResult, ErrorData>> {
        std::future::ready(Ok(ListResourcesResult::with_all_items(resources::listed(
            &self.binding,
        ))
        .with_ttl_ms(SURFACE_TTL_MS)
        .with_cache_scope(CacheScope::Private)))
    }

    fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListResourceTemplatesResult, ErrorData>> {
        std::future::ready(Ok(ListResourceTemplatesResult::with_all_items(
            resources::templates(&self.binding),
        )
        .with_ttl_ms(SURFACE_TTL_MS)
        .with_cache_scope(CacheScope::Private)))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        resources::read(&self.globals, &self.binding, &request.uri)
            .await
            .map(ReadResourceResponse::Complete)
    }

    /// Declare `subscriptions/listen` without promising notifications.
    ///
    /// Returning `None` here would leave the method unimplemented. This
    /// server accepts the resource-list category only, and nothing in this
    /// release changes that list, so a host's subscription is answered and
    /// stays quiet. Per-resource change events are a later package.
    fn accepted_subscription_filter(
        &self,
        _requested: &SubscriptionFilter,
    ) -> Option<SubscriptionFilter> {
        Some(
            SubscriptionFilter::builder()
                .resources_list_changed()
                .build(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> CanvasServer {
        CanvasServer::new(
            Globals {
                json: true,
                color: crate::output::ColorMode::Never,
                profile: Some("default".to_owned()),
                fresh: false,
                offline: true,
                quiet: false,
            },
            Binding::new(
                "canvas.test-7-abcd1234",
                "11111111-1111-4111-8111-111111111111",
            ),
        )
    }

    #[test]
    fn exactly_two_revisions_are_implemented() {
        assert_eq!(
            server().supported_protocol_versions().as_ref(),
            &[ProtocolVersion::V_2026_07_28, ProtocolVersion::V_2025_11_25]
        );
    }

    #[test]
    fn the_primary_revision_is_the_newest_one() {
        assert_eq!(
            server().get_info().protocol_version,
            ProtocolVersion::V_2026_07_28
        );
        assert_eq!(SUPPORTED[0], ProtocolVersion::V_2026_07_28);
    }

    #[test]
    fn every_catalog_tool_is_listed_and_resolvable() {
        let server = server();
        for spec in catalog::specs() {
            assert!(server.get_tool(spec.name).is_some(), "{}", spec.name);
        }
        assert!(server.get_tool("auth.token").is_none());
    }

    #[test]
    fn the_declared_subscription_filter_promises_no_resource_events() {
        let accepted = server()
            .accepted_subscription_filter(
                &SubscriptionFilter::builder()
                    .resources_list_changed()
                    .build(),
            )
            .expect("subscriptions/listen is declared");
        assert_eq!(accepted.resources_list_changed, Some(true));
        assert_eq!(accepted.resource_subscriptions, None);
    }
}
