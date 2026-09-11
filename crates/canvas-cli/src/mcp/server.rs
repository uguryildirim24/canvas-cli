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
    CacheScope, CallToolRequestParams, CallToolResponse, DiscoverResult, ErrorCode, ErrorData,
    Implementation, InitializeRequestParams, InitializeResult, ListResourceTemplatesResult,
    ListResourcesResult, ListToolsResult, PaginatedRequestParams, ProtocolVersion,
    ReadResourceRequestParams, ReadResourceResponse, ServerCapabilities, ServerInfo,
    SubscriptionFilter,
};
use rmcp::service::{RequestContext, RoleServer, SubscriptionContext};

use crate::commands::Globals;
use crate::mcp::resources::Binding;
use crate::mcp::{catalog, resources, result, subscribe};
use crate::output::now_timestamp;

/// The revisions this server implements, newest first.
pub const SUPPORTED: &[ProtocolVersion] =
    &[ProtocolVersion::V_2026_07_28, ProtocolVersion::V_2025_11_25];

/// The consumer id recorded when a host does not name itself.
const ANONYMOUS_CONSUMER: &str = "mcp";

/// How long a host may cache the shape of this server, in milliseconds.
///
/// The catalog and the resource list are fixed when the binary is built, and
/// the instance stops when its identity generation changes, so the surface
/// cannot change under a host that is holding a connection. One minute keeps
/// a restart visible quickly all the same.
const SURFACE_TTL_MS: u64 = 60_000;

/// Guidance a host shows the model with the tool list.
const INSTRUCTIONS: &str = "\
Canvas LMS, read-only. Every tool returns one JSON envelope with `outcome`, \
`exit`, `freshness`, and `result`; read `outcome` before the result. \
`freshness` says which cached dataset answered and whether it is stale; run \
`canvas sync` in a terminal to refresh the cache. A refusal is an answer, \
not an error: `outcome` `refused` with `exit` 8 means the operation is not \
allowed as asked. Nothing here writes: no tool submits, replies, sends, \
downloads, retires a receipt, or touches the browser. Those live on the \
`canvas` command line, where a person approves them. Nothing here reveals a \
credential or changes an identity.";

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
            // The resource list is fixed for the lifetime of an instance: one
            // instance serves one identity generation. Per-resource updates
            // are not fixed, and `subscriptions/listen` sends them from the
            // event log (REPORT §3.6).
            .enable_resources_list_changed()
            .enable_resources_subscribe()
            .build()
    }

    fn implementation() -> Implementation {
        Implementation::new("canvas-cli", env!("CARGO_PKG_VERSION"))
            .with_title("Canvas CLI")
            .with_description("Canvas LMS for one identity, over the local `canvas` binary.")
    }
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
    /// No tool here ever asks a person for a decision: the catalog is reads
    /// only (§21.2), so this server never answers `input_required` and a
    /// request that carries `requestState` is refused rather than run.
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let name = request.name.to_string();
        if catalog::spec(&name).is_none() {
            return Err(ErrorData::new(
                ErrorCode::METHOD_NOT_FOUND,
                format!("unknown tool {name}"),
                None,
            ));
        }
        if request.request_state.is_some() {
            // Nothing on this surface asks for an approval, so a state can
            // only be a host bug or a replay. Running the tool anyway would
            // treat a stale decision as if it belonged to this call.
            return Err(ErrorData::invalid_params(
                format!("{name} never asks for an approval: this catalog is read-only"),
                Some(serde_json::json!({ "tool": name })),
            ));
        }
        let handled = catalog::dispatch(&self.globals, &name, request.arguments)
            .await
            .map_err(|message| {
                ErrorData::invalid_params(
                    format!("{name}: {message}"),
                    Some(serde_json::json!({ "tool": name })),
                )
            })?;
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
        context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        // The consumer handle is this session's own, exactly as it is for a
        // tool call: a model cannot read a resource under another handle.
        let consumer = consumer_of(&context);
        resources::read(&self.globals, &self.binding, &request.uri, &consumer)
            .await
            .map(ReadResourceResponse::Complete)
    }

    /// Accept the resource-list category, plus every subscribed URI this
    /// instance can actually invalidate.
    ///
    /// A foreign identity key, another generation, and an unknown path all
    /// address nothing here, so they are dropped from the accepted filter:
    /// the host is told exactly which of its names this server can update.
    fn accepted_subscription_filter(
        &self,
        requested: &SubscriptionFilter,
    ) -> Option<SubscriptionFilter> {
        let mut filter = SubscriptionFilter::builder().resources_list_changed();
        if let Some(uris) = requested.resource_subscriptions.as_deref() {
            let served = subscribe::served(&self.binding, uris);
            if !served.is_empty() {
                filter = filter.resource_subscriptions(served);
            }
        }
        Some(filter.build())
    }

    /// Serve one subscription from the event log until the host cancels it.
    ///
    /// An event on a dataset scope invalidates the resources that read that
    /// scope, and the cursor follows the same replay rules `watch --since`
    /// follows (REPORT §3.6).
    async fn listen(&self, context: SubscriptionContext) -> Result<(), ErrorData> {
        let consumer = consumer_of(context.request_context());
        subscribe::listen(&self.globals, &self.binding, &consumer, context).await
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

    /// A host that asks for the list category alone gets nothing more.
    #[test]
    fn a_list_only_subscription_promises_no_resource_events() {
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

    /// Only names this instance serves survive into the accepted filter.
    #[test]
    fn a_subscription_keeps_only_the_uris_this_instance_serves() {
        let server = server();
        let binding = &server.binding;
        let foreign = Binding::new("other.test-9-99999999", &binding.generation);
        let accepted = server
            .accepted_subscription_filter(
                &SubscriptionFilter::builder()
                    .resource_subscriptions([
                        binding.uri("todo"),
                        foreign.uri("todo"),
                        binding.uri("auth/token"),
                    ])
                    .build(),
            )
            .expect("subscriptions/listen is declared");
        assert_eq!(
            accepted.resource_subscriptions,
            Some(vec![binding.uri("todo")])
        );
        // The resource capability must advertise subscriptions, or the SDK
        // strips them from the accepted filter before the acknowledgment.
        let capabilities = CanvasServer::capabilities();
        let resources = capabilities.resources.expect("resources are enabled");
        assert_eq!(resources.subscribe, Some(true));
    }
}
