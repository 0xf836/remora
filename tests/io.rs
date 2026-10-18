//! Entry point for Linux syscall and buffer tests added with their implementation.
#[path = "io/buffers.rs"]
mod buffers;
mod common;
#[path = "io/linux_udp.rs"]
mod linux_udp;
#[path = "io/pmtu.rs"]
mod pmtu;

#[path = "io/offloads.rs"]
mod offloads;
