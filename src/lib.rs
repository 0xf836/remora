//! Remora's protocol, policy, and reactor modules are implemented in later phases.
//!
//! The package currently provides build infrastructure only, not a UDP relay.

#[cfg(not(all(
    target_os = "linux",
    target_env = "gnu",
    any(target_arch = "x86_64", target_arch = "aarch64")
)))]
compile_error!("Remora initially targets Linux GNU on x86_64 and aarch64");
