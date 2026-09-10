//! Library surface of the `canvas` binary.
//!
//! The binary keeps its own module tree; this library exposes the clap command
//! definition and the packaging helpers built from it, so `cargo xtask
//! dist-assets` generates man pages and completions from the exact command
//! tree the shipped binary parses.

// `cli` is shared verbatim with the binary target, which allows this at the
// crate root; repeat it here so both targets lint the same file the same way.
#![allow(clippy::struct_excessive_bools)]

pub mod cli;
pub mod dist;
