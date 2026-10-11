//! Entry point for authentication, policy, and diagnostic privacy tests.
#[path = "security/child_cleanup.rs"]
mod child_cleanup;
mod common;
#[path = "security/config.rs"]
mod config;
#[path = "security/diagnostic_primitives.rs"]
mod diagnostic_primitives;
