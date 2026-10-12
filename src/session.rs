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
    pub _identity: Option<Reservation>,
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

impl Session {
    /// Only validated UDP activity refreshes this clock; transport probes do not.
    pub fn udp_activity(&mut self, now: Instant) {
        self.activity = self.activity.max(now);
    }
    pub fn deadline(&self, idle: std::time::Duration, setup: std::time::Duration) -> Instant {
        if self.state == State::Active {
            self.activity + idle
        } else {
            self.started + setup
        }
    }
}
/// A finite generation horizon prevents arrivals from continually extending a
/// round ahead of an older producer. No per-source history is retained.
#[derive(Default, Clone, Copy)]
pub(crate) struct FairCursor {
    pub last: Option<u64>,
    pub ceiling: Option<u64>,
}
/// Select a bounded cyclic visit, freezing new arrivals out of the current round.
pub(crate) fn round_robin(mut ids: Vec<u64>, cursor: &mut FairCursor, limit: usize) -> Vec<u64> {
    ids.sort_unstable();
    let Some(maximum) = ids.last().copied() else {
        return Vec::new();
    };
    cursor.ceiling.get_or_insert(maximum);
    let mut selected = Vec::with_capacity(ids.len().min(limit));
    for _ in 0..ids.len().min(limit) {
        let start = cursor
            .last
            .map_or(0, |last| ids.partition_point(|id| *id <= last));
        let id = match ids[start..]
            .iter()
            .copied()
            .find(|id| cursor.ceiling.is_some_and(|end| *id <= end) && !selected.contains(id))
        {
            Some(id) => id,
            None => {
                cursor.ceiling = Some(maximum);
                *ids.iter().find(|id| !selected.contains(id)).unwrap()
            }
        };
        cursor.last = Some(id);
        selected.push(id);
    }
    selected
}
