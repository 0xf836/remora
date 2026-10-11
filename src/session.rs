//! A flow retains its original connection/stream generation and relay owner.
use crate::{
    buffers::{Generation, Reservation, SessionQueue},
    diagnostics::OpaqueId,
    masque::CapsuleParser,
    udp::Udp,
};
use std::{
    cell::RefCell,
    net::SocketAddr,
    os::fd::{AsRawFd, RawFd},
    sync::Weak,
    time::Instant,
};
use tokio::io::unix::AsyncFd;
pub struct PollSocket(pub RefCell<Udp>);
impl AsRawFd for PollSocket {
    fn as_raw_fd(&self) -> RawFd {
        self.0.borrow().as_raw_fd()
    }
}
impl PollSocket {
    pub fn register(socket: Udp) -> std::io::Result<AsyncFd<Self>> {
        AsyncFd::new(Self(RefCell::new(socket)))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Starting,
    AwaitingAcceptance,
    Authorizing,
    AwaitingCommitment,
    Active,
    Closed,
}
pub struct Session {
    pub generation: Generation,
    pub connection: Generation,
    pub stream: u64,
    pub state: State,
    pub id: OpaqueId,
    pub sender: Option<SocketAddr>,
    /// Immutable destination on the original client listener, including its
    /// port. Associations are keyed by (sender, local destination).
    pub sender_local: Option<SocketAddr>,
    pub identity: Option<usize>,
    pub relay: Option<AsyncFd<PollSocket>>,
    pub relay_peer: Option<SocketAddr>,
    pub receipt: Option<Weak<()>>,
    pub activity: Instant,
    pub started: Instant,
    pub queue: SessionQueue,
    pub capsules: CapsuleParser,
    pub body_pending: bool,
    pub capsule_counts: crate::masque::CapsuleCounts,
    pub _session: Reservation,
    pub _setup: Option<Reservation>,
    pub _relay: Option<Reservation>,
    pub _fd: Option<Reservation>,
}
#[derive(Default)]
pub struct MappingLatch {
    rejected: bool,
}
impl MappingLatch {
    pub fn reject(&mut self) {
        self.rejected = true;
    }
    pub fn may_setup(&self) -> bool {
        !self.rejected
    }
}
