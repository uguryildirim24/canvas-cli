//! End-to-end snapshot suite (SPEC §16 row 3).
//!
//! One binary so the fixture server and the environment helpers are shared.
//! Every test gets its own config and data root, so tests run in parallel.

mod commands;
mod exits;
mod harness;
mod m6c;
mod precedence;
mod raw_output;
mod schema;
mod selection;
