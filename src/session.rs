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
    Recovering,
    Closed,
}

/// Mapping-wide client state; Active means verified transport/settings, not
/// destination responsiveness. Rejected never becomes retryable without restart.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientState {
    Starting,
    Handshaking,
    Negotiating,
    Active,
    Recovering,
    Rejected,
    Stopping,
    Stopped,
}

/// One pending deadline, not a timer/task per attempt. RNG is supplied separately
/// so minimum-jitter and arbitrarily long outages have deterministic tests.
pub struct RetrySchedule {
    floor_ms: u64,
    maximum_ms: u64,
    attempt: u32,
    next: Option<Instant>,
}
impl RetrySchedule {
    /// Bounds come from validated RecoverySettings: 100 <= floor <= maximum
    /// <= 2000 ms. The reactor is the only production caller.
    pub fn new(floor_ms: u64, maximum_ms: u64) -> Self {
        Self {
            floor_ms,
            maximum_ms,
            attempt: 0,
            next: None,
        }
    }
    pub fn jitter_ceiling_ms(&self) -> u64 {
        self.floor_ms
            .saturating_mul(1u64 << self.attempt.min(31))
            .min(self.maximum_ms.saturating_sub(self.floor_ms))
    }
    pub fn schedule(&mut self, now: Instant, jitter_ms: u64) {
        let delay = self
            .floor_ms
            .saturating_add(jitter_ms.min(self.jitter_ceiling_ms()));
        self.next = now.checked_add(std::time::Duration::from_millis(delay));
        self.attempt = self.attempt.saturating_add(1);
    }
    pub fn deadline(&self) -> Option<Instant> {
        self.next
    }
    pub fn due(&self, now: Instant) -> bool {
        self.next.is_some_and(|at| at <= now)
    }
    pub fn started(&mut self) {
        self.next = None;
    }
    pub fn reset(&mut self) {
        self.next = None;
        self.attempt = 0;
    }
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
    /// At most 32 headers/16 KiB, charged to pending-setup admission. QUIC
    /// stream delivery can present a request before peer SETTINGS arrives.
    pub deferred_headers: Option<(Vec<quiche::h3::Header>, bool)>,
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
