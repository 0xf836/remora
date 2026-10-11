//! Validated startup configuration, bounded storage and diagnostic primitives.
//! Forwarding and command wiring are implemented in later phases.

#[cfg(not(all(
    target_os = "linux",
    target_env = "gnu",
    any(target_arch = "x86_64", target_arch = "aarch64")
)))]
compile_error!("Remora initially targets Linux GNU on x86_64 and aarch64");

pub mod buffers;
pub mod config;
pub mod diagnostics;
