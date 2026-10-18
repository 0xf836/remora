//! Entry point for authentication, policy, and diagnostic privacy tests.
#[path = "security/child_cleanup.rs"]
mod child_cleanup;
mod common;
#[path = "security/config.rs"]
mod config;
#[path = "security/diagnostic_primitives.rs"]
mod diagnostic_primitives;
#[path = "security/policy.rs"]
mod policy;

#[path = "security/diagnostics.rs"]
mod diagnostics;
#[path = "security/qlog.rs"]
mod qlog;
#[path = "security/status_writer.rs"]
mod status_writer;
