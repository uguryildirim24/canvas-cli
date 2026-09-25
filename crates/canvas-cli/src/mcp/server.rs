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
//!
//! The surface behind the handshake is one tool (§21.2). There are no
//! resources and no subscriptions, so this handler implements `tools/list`
//! and `tools/call` and nothing else.

use std::borrow::Cow;
use std::future::Future;

use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CacheScope, CallToolRequestParams, CallToolResponse, DiscoverResult, ErrorCode, ErrorData,
    Implementation, InitializeRequestParams, InitializeResult, ListToolsResult,
    PaginatedRequestParams, ProtocolVersion, ServerCapabilities, ServerInfo,
};
use rmcp::service::{RequestContext, RoleServer};

use crate::mcp::catalog;

/// The revisions this server implements, newest first.
pub const SUPPORTED: &[ProtocolVersion] =
    &[ProtocolVersion::V_2026_07_28, ProtocolVersion::V_2025_11_25];

/// How long a host may cache the shape of this server, in milliseconds.
///
/// The tool is fixed when the binary is built, and the instance stops when its
/// identity generation changes, so the surface cannot change under a host that
/// is holding a connection. One minute keeps a restart visible quickly all the
/// same.
const SURFACE_TTL_MS: u64 = 60_000;

/// Guidance a host shows the model with the tool list.
const INSTRUCTIONS: &str = "\
Canvas LMS for one student identity. This server has one tool, \
`getclitools`, and it performs nothing. Call it once: it returns the complete \
`canvas` command-line reference — every command, its operands and flags, and \
the JSON envelope each one returns. After that, run `canvas <command> ...` \
yourself with whatever shell you have. There is no second tool, no resource, \
and no subscription. Reads and writes are all commands; every write prints \
what it is about to do and asks a person at the terminal, so never pass \
`--yes`. Nothing here reveals a credential or changes an identity.";

/// One `canvas mcp` instance: one identity generation, one tool.
///
/// The instance holds no state. The identity it is bound to is enforced in
/// [`crate::mcp::run`], which refuses to start without one and stops the
/// process when it is replaced; the tool itself reads nothing but the
/// binary's own command tree.
pub struct CanvasServer;

impl CanvasServer {
    /// Build the handler.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    fn capabilities() -> ServerCapabilities {
        ServerCapabilities::builder().enable_tools().build()
    }

    fn implementation() -> Implementation {
        Implementation::new("canvas-cli", env!("CARGO_PKG_VERSION"))
            .with_title("Canvas CLI")
            .with_description("Canvas LMS for one identity, over the local `canvas` binary.")
    }

    /// Answer `tools/call`.
    ///
    /// Only an unroutable request — a name that is not the one tool, or
    /// arguments its schema does not admit — becomes a JSON-RPC error. The
    /// tool itself cannot fail: it reads the binary's own command tree, with
    /// no session, no network, and no cache behind it. Nothing here waits on
    /// anything, which is why the handler answers from a ready future.
    ///
    /// Nothing here ever asks a person for a decision, so this server never
    /// answers `input_required` and a request that carries `requestState` is
    /// refused rather than run.
    fn answer(request: CallToolRequestParams) -> Result<CallToolResponse, ErrorData> {
        let name = request.name.to_string();
        if name != catalog::TOOL {
            return Err(ErrorData::new(
                ErrorCode::METHOD_NOT_FOUND,
                format!("unknown tool {name}"),
                None,
            ));
        }
        if request.request_state.is_some() {
            // Nothing on this surface asks for an approval, so a state can
            // only be a host bug or a replay.
            return Err(ErrorData::invalid_params(
                format!("{name} never asks for an approval: it only describes the CLI"),
                Some(serde_json::json!({ "tool": name })),
            ));
        }
        let result = catalog::dispatch(&name, request.arguments).map_err(|message| {
            ErrorData::invalid_params(
                format!("{name}: {message}"),
                Some(serde_json::json!({ "tool": name })),
            )
        })?;
        Ok(CallToolResponse::Complete(result))
    }
}

impl Default for CanvasServer {
    fn default() -> Self {
        Self::new()
    }
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
        std::future::ready(Ok(ListToolsResult::with_all_items(vec![
            catalog::spec().tool(),
        ])
        .with_ttl_ms(SURFACE_TTL_MS)
        .with_cache_scope(CacheScope::Private)))
    }

    fn get_tool(&self, name: &str) -> Option<rmcp::model::Tool> {
        (name == catalog::TOOL).then(|| catalog::spec().tool())
    }

    /// Return the `canvas` command reference. See [`CanvasServer::answer`].
    fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<CallToolResponse, ErrorData>> {
        std::future::ready(Self::answer(request))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> CanvasServer {
        CanvasServer::new()
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

    /// One tool resolves, and nothing else does.
    #[test]
    fn the_one_tool_is_resolvable_and_no_other_name_is() {
        let server = server();
        assert!(server.get_tool(catalog::TOOL).is_some());
        for gone in [
            "todo.list",
            "submission.prepare",
            "context.attach",
            "auth.token",
        ] {
            assert!(server.get_tool(gone).is_none(), "{gone} resolves");
        }
    }

    /// The server declares tools and nothing else: no resources, and so no
    /// resource list-changed notification and no subscription (§21.2).
    #[test]
    fn the_server_declares_tools_and_nothing_else() {
        let capabilities = CanvasServer::capabilities();
        assert!(capabilities.tools.is_some());
        assert!(capabilities.resources.is_none());
        assert!(capabilities.prompts.is_none());
        assert!(capabilities.completions.is_none());
    }
}
