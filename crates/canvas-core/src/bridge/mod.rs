//! The companion broker: framing, protocol, endpoint, and state (M7-a).
//!
//! `canvas bridge host` is the process; everything it decides lives in this
//! module, so the decisions are tested without a browser and without a socket.
//!
//! The trust boundary runs through here. The extension is the only side that
//! can see the page, and it may be an extension this host did not install.
//! The host is the only side that knows the CLI identity, and it re-derives
//! the route and the zone from the sanitized URL before it believes either.
//! Cookies never leave Chrome: the one browser-session request in this design
//! is a fixed same-origin `GET /api/v1/users/self` performed in extension
//! code, whose body is reduced to `{ id }` before it is sent.

pub mod endpoint;
pub mod framing;
pub mod ipc;
pub mod note;
pub mod state;
pub mod text;
pub mod wire;

pub use endpoint::Endpoint;
pub use state::{Accepted, Broker, OwnedIdentity};
