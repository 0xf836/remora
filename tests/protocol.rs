//! Entry point for protocol contract tests added with their implementation.
#[path = "protocol/capsules.rs"]
mod capsules;
#[path = "protocol/masque.rs"]
mod masque;

mod common;
#[path = "protocol/transport.rs"]
mod transport;

#[path = "protocol/interop_cases.rs"]
mod interop_cases;
