//! Unpublished source-including owner adapter; no replacement state machine.
#[path = "../../src/buffers.rs"]
pub mod buffers;
#[path = "../../src/config.rs"]
pub mod config;
#[path = "../../src/masque.rs"]
pub mod masque;
#[path = "../../src/policy.rs"]
pub mod policy;
#[path = "../../src/session.rs"]
pub mod session;
#[path = "../../src/transport.rs"]
pub mod transport;
#[path = "../../src/udp.rs"]
pub mod udp;
pub mod diagnostics {
    include!(concat!(env!("OUT_DIR"), "/diagnostics.rs"));
}
pub mod reactor {
    include!(concat!(env!("OUT_DIR"), "/reactor.rs"));
}
