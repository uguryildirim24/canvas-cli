//! `canvas mcp`: the Model Context Protocol adapter over stdio.
//!
//! One instance serves one identity generation. The identity is bound at
//! startup from the selected profile (§8), and the instance stops as soon as
//! that identity is replaced or removed (§10), so nothing an agent holds can
//! ever address a later identity.
//!
//! The surface is one tool, `getclitools`, and it performs nothing: it hands
//! the caller the `canvas` command reference and the caller runs the commands
//! itself (§21.2). Nothing here shells out to `canvas`, and nothing here
//! reaches Canvas.

pub mod catalog;
pub mod reference;
pub mod server;

use std::cell::Cell;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::Rc;

use canvas_core::identity::IdentityDocument;
use rmcp::ServiceExt;
use rmcp::transport::stdio;

use crate::commands::Globals;
use crate::commands::emit::session_error;
use crate::mcp::server::CanvasServer;

/// How often the instance re-reads `identity.json` (§10).
const IDENTITY_POLL: std::time::Duration = std::time::Duration::from_secs(2);

/// The identity generation this server instance is bound to (§10).
///
/// The instance binds one key and one generation at startup and never
/// re-reads them, so nothing it serves can drift to another identity.
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
}

/// Run the MCP server on stdin and stdout.
///
/// Without an identity there is nothing to serve, so the server refuses to
/// start with the §14 auth error rather than accepting a connection it cannot
/// answer.
pub async fn run(globals: &Globals) -> ExitCode {
    let (binding, identity_json) = match bind(globals) {
        Ok(bound) => bound,
        Err(handled) => return handled,
    };
    let server = CanvasServer::new();
    // The command cores are `!Send`, so the service runs on this thread.
    let local = tokio::task::LocalSet::new();
    let code = local
        .run_until(Box::pin(serve(server, binding, identity_json)))
        .await;
    // The stdio reader is a blocking read that ends only when the host writes
    // again, so letting the runtime drain it would keep a stopped instance
    // alive. The transport is already closed here, so leave at once.
    let _ = io::stdout().flush();
    std::process::exit(i32::from(code));
}

/// Bind one identity key and generation, or refuse to start.
fn bind(globals: &Globals) -> Result<(Binding, PathBuf), ExitCode> {
    // `mcp` is a local surface: opening offline avoids asking for a token
    // before a tool needs one. Each tool opens its own session afterwards.
    let session = match globals.open_local_session() {
        Ok(session) => session,
        Err(e) => return Err(session_error(e, globals.profile.clone()).emit(globals.json)),
    };
    let binding = Binding::new(
        session.identity.key.as_str(),
        session.identity.generation.to_string(),
    );
    Ok((binding, session.paths.identity_json()))
}

/// Serve until the host disconnects or the identity changes.
async fn serve(server: CanvasServer, binding: Binding, identity_json: PathBuf) -> u8 {
    let service = match server.serve(stdio()).await {
        Ok(service) => service,
        Err(e) => {
            let _ = writeln!(io::stderr(), "cannot start the MCP server: {e}");
            return 13;
        }
    };
    let stop = service.cancellation_token();
    let changed = Rc::new(Cell::new(false));
    let flag = Rc::clone(&changed);
    let watch = tokio::task::spawn_local(async move {
        watch_identity(&binding, &identity_json).await;
        flag.set(true);
        stop.cancel();
    });
    let quit = service.waiting().await;
    // The watchdog holds nothing the process needs after this point.
    watch.abort();
    if changed.get() {
        // §10: the identity is gone or was recreated.
        return 13;
    }
    match quit {
        Ok(_) => 0,
        Err(e) => {
            let _ = writeln!(io::stderr(), "the MCP server stopped: {e}");
            13
        }
    }
}

/// Wait until `identity.json` no longer describes the bound generation.
async fn watch_identity(binding: &Binding, identity_json: &Path) {
    loop {
        tokio::time::sleep(IDENTITY_POLL).await;
        if !still_bound(binding, identity_json) {
            let _ = writeln!(io::stderr(), "identity changed");
            return;
        }
    }
}

/// Whether `identity.json` still describes the bound identity generation.
fn still_bound(binding: &Binding, identity_json: &Path) -> bool {
    match IdentityDocument::read(identity_json) {
        Ok(doc) => {
            doc.key.as_str() == binding.key && doc.generation.to_string() == binding.generation
        }
        // A missing or unreadable document means the identity is gone (§10).
        Err(_) => false,
    }
}
