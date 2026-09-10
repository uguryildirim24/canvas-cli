//! The companion broker on the CLI side (M7-a).
//!
//! `canvas-core::bridge` decides what an attachment is and what may leave the
//! browser. This module is the machinery around those decisions: the
//! ownership lock, the native-messaging host manifest, the process that
//! serves `bridge-ipc@1`, and the blocking client every other command uses to
//! reach it.

pub mod client;
pub mod host;
pub mod manifest;
pub mod owner;

pub mod release;
