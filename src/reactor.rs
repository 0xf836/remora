//! One current-thread owner. Finite visits precede timer/cancellation work.
use crate::{
    buffers::{Budget, Direction, Generations, Pools, ResourceBudgets, SessionQueue},
    config::{RoleConfig, ValidatedConfig},
    diagnostics::{Diagnostics, Event, Failure, Role, Snapshot},
    masque::{self, CapsuleParser, Response, WireError},
    policy::{self, PolicyError, SetupKey, SystemResolver},
    session::{
        ClientState, FairCursor, MappingLatch, PollSocket, RetrySchedule, Session, State,
        round_robin, round_robin_recorded,
    },
    transport::{self, Connection, TrackedFactory, TransportError},
    udp::{MonotonicTimer, ReceiveFault, SendPacket, Udp},
};
use std::{
    collections::HashMap,
    future::Future,
    io,
    net::SocketAddr,
    pin::Pin,
    task::Poll,
    time::{Duration, Instant},
};
use tokio::{
    io::unix::AsyncFd,
    signal::unix::{SignalKind, signal},
};
// Generation and physical submission share one 32-item owner visit.
const OUTPUT_QUOTA: usize = 16;
const DENSITY_HISTORY: usize = 32;

/// Recent nonempty outer-packet observations, local to one connection owner.
/// ACK-only packets and queue drains must not age out coalescing evidence.
#[derive(Default)]
struct ReceiveDensity {
    recent: [u8; DENSITY_HISTORY],
    next: usize,
    maximum: u8,
}
impl ReceiveDensity {
    fn observe(&mut self, datagrams: usize) {
        if datagrams == 0 {
            return;
        }
        // Every density >=32 already requests one outer packet per visit.
        let density = datagrams.min(32) as u8;
        let previous = self.recent[self.next];
        self.recent[self.next] = density;
        self.next = (self.next + 1) % DENSITY_HISTORY;
        if density >= self.maximum {
            self.maximum = density;
        } else if previous == self.maximum {
            self.maximum = self.recent.iter().copied().max().unwrap_or(0);
        }
    }
    fn estimate(&self) -> usize {
        usize::from(self.maximum.max(1))
    }
}
type SetupFuture<'a> =
    Pin<Box<dyn Future<Output = Result<(usize, policy::ResolvedTarget), PolicyError>> + 'a>>;
struct Pending<'a> {
    session: u64,
    key: SetupKey,
    future: SetupFuture<'a>,
}
struct OwnedConnection {
    transport: Connection,
    socket: usize,
    _permit: crate::buffers::Reservation,
    _socket_permit: Option<crate::buffers::Reservation>,
    id: crate::diagnostics::OpaqueId,
    initial_cid: Vec<u8>,
    cursors: [FairCursor; 7],
    receive_density: ReceiveDensity,
    enqueue_pending: bool,
    output_pending: bool,
}
impl OwnedConnection {
    fn generate_output(
        &mut self,
        pool: &crate::buffers::BufferPool,
        now: Instant,
    ) -> Result<usize, TransportError> {
        // One attempted output visit consumes this continuation, even if
        // congestion blocks progress. Credit/readiness/timers wake it again.
        self.output_pending = false;
        self.transport.generate(pool, now, OUTPUT_QUOTA)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeError {
    Bind,
    Socket,
    SocketErrno(i32),
    Transport,
    TransportCode(u64),
    Capability,
    Trust,
    Resource,
    Diagnostics,
    DiagnosticCollectionIncomplete,
    Signal,
}
impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "runtime failed: {self:?}")
    }
}
impl std::error::Error for RuntimeError {}
#[derive(Debug)]
enum OutputError {
    Transport(TransportError),
    Socket(RuntimeError),
}
struct SourceBucket {
    last: Instant,
    credits: u32,
}
#[cfg(test)]
type OuterSendHook = Box<dyn for<'p> FnMut(&[SendPacket<'p>]) -> io::Result<usize>>;
struct Runner<'a> {
    cfg: &'a ValidatedConfig,
    server: bool,
    outer: Vec<AsyncFd<PollSocket>>,
    listener: Option<AsyncFd<PollSocket>>,
    timer: AsyncFd<MonotonicTimer>,
    quic_config: quiche::Config,
    connections: HashMap<u64, OwnedConnection>,
    sessions: HashMap<u64, Session>,
    senders: HashMap<(SocketAddr, SocketAddr), u64>,
    pending: Vec<Pending<'a>>,
    resolver: &'a SystemResolver,
    generations: Generations,
    budgets: ResourceBudgets,
    identities: Vec<Budget>,
    pools: Pools,
    diagnostics: Diagnostics,
    snapshot: Snapshot,
    retired_io: crate::udp::IoStats,
    latch: MappingLatch,
    retry: transport::Retry,
    source_buckets: [SourceBucket; 64],
    turn: usize,
    setup_cursor: usize,
    next_snapshot: Instant,
    next_quic_sample: Instant,
    recovery_started: Option<Instant>,
    qlog: Option<crate::diagnostics::qlog::Pool>,
    stopping: bool,
    ever_active: bool,
    primary: Option<u64>,
    reconnect: RetrySchedule,
    setup_retry: RetrySchedule,
    stop_started: Option<Instant>,
    #[cfg(test)]
    outer_send_hook: Option<OuterSendHook>,
}
fn native_drop(error: TransportError) -> crate::diagnostics::DropReason {
    use crate::diagnostics::DropReason;
    match error {
        TransportError::Oversized => DropReason::Oversized,
        TransportError::Pmtu => DropReason::Pmtu,
        TransportError::Capacity => DropReason::QueueFull,
        TransportError::Expired => DropReason::QueueExpired,
        TransportError::Stopped => DropReason::StaleGeneration,
        _ => DropReason::Malformed,
    }
}
fn resource<T, E>(result: Result<T, E>) -> Result<T, RuntimeError> {
    result.map_err(|_| RuntimeError::Resource)
}
fn read_io<T>(
    socket: &AsyncFd<PollSocket>,
    mut operation: impl FnMut(&PollSocket) -> io::Result<T>,
) -> io::Result<T> {
    let mut attempted = false;
    let result = socket.try_io(tokio::io::Interest::READABLE, |inner| {
        attempted = true;
        operation(inner)
    });
    // New sockets and quota continuations must probe even before Tokio reports
    // an edge. This fallback never clears readiness. Cached EAGAIN is cleared
    // atomically by try_io, so an edge racing with the syscall is retained.
    if !attempted
        && result
            .as_ref()
            .is_err_and(|e| e.kind() == io::ErrorKind::WouldBlock)
    {
        operation(socket.get_ref())
    } else {
        result
    }
}
fn poll_timer(
    timer: &AsyncFd<MonotonicTimer>,
    cx: &mut std::task::Context<'_>,
) -> Poll<io::Result<u64>> {
    for _ in 0..2 {
        match timer.poll_read_ready(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
            Poll::Ready(Ok(mut guard)) => match timer.get_ref().consume() {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    guard.clear_ready();
                    // A cached-ready poll does not register this task's waker.
                    // Poll again after clearing to register before sleeping.
                }
                result => return Poll::Ready(result),
            },
        }
    }
    // Bound owner work if another edge raced with the two polls; never sleep
    // without either a registered waker or an explicit continuation.
    cx.waker().wake_by_ref();
    Poll::Pending
}
/// A continuation token covers every owner work item before going idle.
pub fn service_window(total: usize, cursor: usize) -> (std::ops::Range<usize>, usize, bool) {
    let start = if cursor >= total { 0 } else { cursor };
    let end = start.saturating_add(8).min(total);
    (start..end, if end == total { 0 } else { end }, end < total)
}
impl<'a> Runner<'a> {
    fn new(
        cfg: &'a ValidatedConfig,
        resolver: &'a SystemResolver,
        diagnostics: Diagnostics,
    ) -> Result<Self, RuntimeError> {
        let now = Instant::now();
        let server = matches!(cfg.role(), RoleConfig::Server(_));
        let qlog = cfg
            .diagnostics()
            .qlog_directory
            .as_ref()
            .map(|p| {
                crate::diagnostics::qlog::Pool::new(p, diagnostics.queue().clone(), now)
                    .map_err(|_| RuntimeError::Diagnostics)
            })
            .transpose()?;
        let mut outer = Vec::new();
        let mut listener = None;
        match cfg.role() {
            RoleConfig::Server(s) => {
                for bind in &s.listen {
                    outer.push(
                        PollSocket::register(
                            Udp::bind(*bind, cfg.io(), false).map_err(|_| RuntimeError::Bind)?,
                        )
                        .map_err(|_| RuntimeError::Socket)?,
                    );
                }
            }
            RoleConfig::Client(c) => {
                listener = Some(
                    PollSocket::register(
                        Udp::bind(c.listen, cfg.io(), true).map_err(|_| RuntimeError::Bind)?,
                    )
                    .map_err(|_| RuntimeError::Socket)?,
                );
            }
        }
        let pools = resource(Pools::new(
            cfg.limits(),
            cfg.transport().application_max_bytes,
            cfg.transport().outer_udp_max_bytes,
            cfg.transport().outer_udp_max_bytes,
        ))?;
        let mut runner = Self {
            cfg,
            server,
            outer,
            listener,
            timer: AsyncFd::new(MonotonicTimer::new().map_err(|_| RuntimeError::Socket)?)
                .map_err(|_| RuntimeError::Socket)?,
            quic_config: transport::config(cfg).map_err(|_| RuntimeError::Transport)?,
            connections: HashMap::new(),
            sessions: HashMap::new(),
            senders: HashMap::new(),
            pending: Vec::new(),
            resolver,
            generations: Generations::default(),
            budgets: ResourceBudgets::new(cfg.limits(), cfg.required_fds()),
            identities: match cfg.role() {
                RoleConfig::Server(server) => {
                    ResourceBudgets::identity_budgets(cfg.limits(), server.identities.len())
                }
                RoleConfig::Client(_) => Vec::new(),
            },
            pools,
            diagnostics,
            snapshot: Snapshot::default(),
            retired_io: crate::udp::IoStats::default(),
            latch: MappingLatch::default(),
            retry: transport::Retry::new(now).map_err(|_| RuntimeError::Resource)?,
            source_buckets: std::array::from_fn(|_| SourceBucket {
                last: now,
                credits: 16,
            }),
            turn: 0,
            setup_cursor: 0,
            next_snapshot: now + Duration::from_millis(cfg.diagnostics().snapshot_interval_ms),
            next_quic_sample: now,
            recovery_started: None,
            qlog,
            stopping: false,
            ever_active: false,
            primary: None,
            reconnect: RetrySchedule::new(
                cfg.recovery().retry_initial_ms,
                cfg.recovery().retry_max_ms,
            ),
            setup_retry: RetrySchedule::new(
                cfg.recovery().retry_initial_ms,
                cfg.recovery().retry_max_ms,
            ),
            stop_started: None,
            #[cfg(test)]
            outer_send_hook: None,
        };
        runner.snapshot.io_batch_size = cfg.io().batch_max;
        runner.snapshot.offloads = crate::diagnostics::OffloadState {
            gso: crate::diagnostics::OffloadReport::new(cfg.io().gso),
            gro: crate::diagnostics::OffloadReport::new(cfg.io().gro),
        };
        for socket in &runner.outer {
            let udp = socket.get_ref().0.borrow();
            runner.snapshot.socket_buffers.outer_receive_max_per_socket = runner
                .snapshot
                .socket_buffers
                .outer_receive_max_per_socket
                .max(udp.receive_buffer);
            runner.snapshot.socket_buffers.outer_send_max_per_socket = runner
                .snapshot
                .socket_buffers
                .outer_send_max_per_socket
                .max(udp.send_buffer);
        }
        if let Some(socket) = &runner.listener {
            let udp = socket.get_ref().0.borrow();
            runner
                .snapshot
                .socket_buffers
                .application_receive_max_per_socket = udp.receive_buffer;
            runner
                .snapshot
                .socket_buffers
                .application_send_max_per_socket = udp.send_buffer;
        }
        resource(runner.connections.try_reserve(cfg.limits().max_connections))?;
        resource(runner.sessions.try_reserve(cfg.limits().max_sessions))?;
        resource(runner.senders.try_reserve(cfg.limits().max_sessions))?;
        resource(runner.pending.try_reserve(cfg.limits().max_pending_setup))?;
        if !runner.server {
            resource(runner.outer.try_reserve(cfg.limits().max_connections))?;
            runner.client_state(ClientState::Starting, now)?;
            if let Err(error) = runner.start_client_connection(now) {
                runner.failure(Failure::Socket, 0, None, None);
                if error == RuntimeError::Diagnostics {
                    return Err(error);
                }
                runner.schedule_reconnect(now)?;
            }
        }
        Ok(runner)
    }
    fn client_state(&mut self, state: ClientState, now: Instant) -> Result<(), RuntimeError> {
        if self.snapshot.recovery.client_state == Some(state) {
            return Ok(());
        }
        if state == ClientState::Recovering {
            self.recovery_started.get_or_insert(now);
        }
        if state == ClientState::Active
            && let Some(started) = self.recovery_started.take()
        {
            self.snapshot
                .histograms
                .recovery_duration_us
                .observe(now.saturating_duration_since(started).as_micros() as u64);
        }
        self.snapshot.recovery.client_state = Some(state);
        self.snapshot.recovery.transitions = self
            .snapshot
            .recovery
            .transitions
            .checked_add(1)
            .ok_or(RuntimeError::Diagnostics)?;
        let name = match state {
            ClientState::Active => crate::diagnostics::EventName::Active,
            ClientState::Recovering => crate::diagnostics::EventName::Recovering,
            ClientState::Rejected => crate::diagnostics::EventName::Rejected,
            ClientState::Stopped => crate::diagnostics::EventName::Stopped,
            _ => crate::diagnostics::EventName::Starting,
        };
        let mut event = Event::failure(crate::config::Level::Info, Failure::Transport, 0);
        event.event = name;
        event.failure = None;
        event.numeric_code = None;
        let _ = self.diagnostics.event(now, event);
        Ok(())
    }
    fn jitter(limit: u64) -> Result<u64, RuntimeError> {
        let span = limit.checked_add(1).ok_or(RuntimeError::Resource)?;
        let zone = u64::MAX - u64::MAX % span;
        // Unbiased bounded draw. There is no unbounded entropy retry on owner.
        for _ in 0..4 {
            let mut bytes = [0; 8];
            getrandom::fill(&mut bytes).map_err(|_| RuntimeError::Resource)?;
            let value = u64::from_ne_bytes(bytes);
            if value < zone {
                return Ok(value % span);
            }
        }
        Err(RuntimeError::Resource)
    }
    fn schedule_reconnect(&mut self, now: Instant) -> Result<(), RuntimeError> {
        if self.server || self.stopping || !self.latch.may_setup() {
            return Ok(());
        }
        if self.reconnect.deadline().is_none() {
            let jitter = Self::jitter(self.reconnect.jitter_ceiling_ms())?;
            self.reconnect.schedule(now, jitter);
        }
        self.client_state(ClientState::Recovering, now)
    }
    fn start_client_connection(&mut self, now: Instant) -> Result<(), RuntimeError> {
        if self.primary.is_some() || self.stopping || !self.latch.may_setup() {
            return Ok(());
        }
        self.reconnect.started();
        self.snapshot.recovery.connection_attempts = self
            .snapshot
            .recovery
            .connection_attempts
            .checked_add(1)
            .ok_or(RuntimeError::Diagnostics)?;
        let RoleConfig::Client(client) = self.cfg.role() else {
            return Err(RuntimeError::Transport);
        };
        let permit = resource(self.budgets.connections.reserve(1, 0))?;
        let socket_permit = resource(self.budgets.fds.reserve(1, 0))?;
        let address = if client.server_address.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        };
        let mut socket = Udp::bind(
            address.parse().map_err(|_| RuntimeError::Bind)?,
            self.cfg.io(),
            false,
        )
        .map_err(|_| RuntimeError::Socket)?;
        socket
            .connect(client.server_address)
            .map_err(|_| RuntimeError::Socket)?;
        let local = socket.local_addr();
        let cid = transport::random_cid().map_err(|_| RuntimeError::Resource)?;
        let generation = resource(self.generations.next_generation())?;
        let mut quic = quiche::connect_with_buffer_factory::<TrackedFactory>(
            Some(&client.server_name),
            &quiche::ConnectionId::from_ref(&cid),
            local,
            client.server_address,
            &mut self.quic_config,
        )
        .map_err(|_| RuntimeError::Transport)?;
        let id = self
            .diagnostics
            .next_id()
            .map_err(|_| RuntimeError::Diagnostics)?;
        Connection::attach_qlog(&mut quic, self.qlog.as_ref());
        let socket_index = self.outer.len();
        let socket = PollSocket::register(socket).map_err(|_| RuntimeError::Socket)?;
        self.snapshot.socket_buffers.outer_receive_max_per_socket = self
            .snapshot
            .socket_buffers
            .outer_receive_max_per_socket
            .max(socket.get_ref().0.borrow().receive_buffer);
        self.snapshot.socket_buffers.outer_send_max_per_socket = self
            .snapshot
            .socket_buffers
            .outer_send_max_per_socket
            .max(socket.get_ref().0.borrow().send_buffer);
        self.outer.push(socket);
        self.connections.insert(
            generation.0,
            OwnedConnection {
                transport: Connection::new(quic, generation, false, self.cfg, now),
                socket: socket_index,
                _permit: permit,
                _socket_permit: Some(socket_permit),
                id,
                initial_cid: cid.to_vec(),
                cursors: [FairCursor::default(); 7],
                receive_density: ReceiveDensity::default(),
                enqueue_pending: false,
                output_pending: false,
            },
        );
        self.primary = Some(generation.0);
        self.client_state(ClientState::Handshaking, now)
    }
    fn service_recovery(&mut self, now: Instant) -> Result<bool, RuntimeError> {
        if !self.server
            && !self.stopping
            && self.latch.may_setup()
            && self.primary.is_none()
            && self.reconnect.due(now)
        {
            if let Err(error) = self.start_client_connection(now) {
                if error == RuntimeError::Diagnostics {
                    return Err(error);
                }
                self.failure(Failure::Socket, 0, None, None);
                self.schedule_reconnect(now)?;
            }
            return Ok(true);
        }
        Ok(false)
    }
    fn permanent_client_failure(
        &mut self,
        now: Instant,
        reason: Failure,
        code: u64,
    ) -> Result<(), RuntimeError> {
        self.latch.reject();
        self.reconnect.reset();
        self.setup_retry.reset();
        // Only already accepted flows survive terminal mapping rejection.
        // Retire every pending generation before polling H3 again, cancelling
        // its callbacks, sender association and setup permits together.
        let pending: Vec<_> = self
            .sessions
            .iter()
            .filter_map(|(id, s)| (s.state != State::Active).then_some(*id))
            .collect();
        for id in pending {
            self.request_failure(
                reason,
                code,
                self.sessions.get(&id).map(|s| s.id),
                self.sessions
                    .get(&id)
                    .and_then(|s| self.connections.get(&s.connection.0))
                    .map(|c| c.id),
            );
            self.close_session(id, 0x10b);
        }
        self.client_state(ClientState::Rejected, now)
    }
    fn remove_connection(&mut self, id: u64, now: Instant) -> Result<(), RuntimeError> {
        let affected: Vec<_> = self
            .sessions
            .iter()
            .filter_map(|(sid, s)| (s.connection.0 == id).then_some(*sid))
            .collect();
        for sid in affected {
            if let Some(s) = self.sessions.get_mut(&sid) {
                s.state = State::Recovering;
            }
            self.close_session(sid, 0x100);
        }
        if let Some(mut c) = self.connections.remove(&id) {
            self.snapshot.recovery.discarded_ciphertext = self
                .snapshot
                .recovery
                .discarded_ciphertext
                .checked_add(c.transport.output.len() as u64)
                .ok_or(RuntimeError::Diagnostics)?;
            c.transport.stop();
            if !self.server {
                self.retired_io = self
                    .retired_io
                    .merge(self.outer[c.socket].get_ref().0.borrow().stats());
                self.outer.remove(c.socket);
                for other in self.connections.values_mut() {
                    if other.socket > c.socket {
                        other.socket -= 1;
                    }
                }
            }
        }
        if self.primary == Some(id) {
            self.primary = None;
            self.schedule_reconnect(now)?;
        }
        Ok(())
    }
    fn stop_connection(&mut self, id: u64) -> Result<(), RuntimeError> {
        if let Some(c) = self.connections.get_mut(&id) {
            self.snapshot.recovery.discarded_ciphertext = self
                .snapshot
                .recovery
                .discarded_ciphertext
                .checked_add(c.transport.output.len() as u64)
                .ok_or(RuntimeError::Diagnostics)?;
            c.transport.stop();
        }
        Ok(())
    }
    fn drop_packet(
        &mut self,
        reason: crate::diagnostics::DropReason,
        direction: Direction,
    ) -> Result<(), RuntimeError> {
        self.snapshot
            .counters
            .local_drops
            .increment(reason, direction)
            .map_err(|_| RuntimeError::Diagnostics)
    }
    fn failure(
        &mut self,
        reason: Failure,
        code: u64,
        session: Option<crate::diagnostics::OpaqueId>,
        connection: Option<crate::diagnostics::OpaqueId>,
    ) {
        if session.is_none() && connection.is_some() {
            let ids: Vec<_> = self
                .sessions
                .values()
                .filter(|s| {
                    self.connections
                        .get(&s.connection.0)
                        .is_some_and(|c| Some(c.id) == connection)
                })
                .map(|s| s.id)
                .collect();
            for id in ids {
                let _ = self.diagnostics.failure(
                    Instant::now(),
                    connection,
                    Some(id),
                    reason,
                    code,
                    false,
                );
            }
        }
        let _ = self
            .diagnostics
            .failure(Instant::now(), connection, session, reason, code, false);
        let mut event = Event::failure(crate::config::Level::Warn, reason, code);
        event.session_id = session;
        event.connection_id = connection;
        let _ = self.diagnostics.event(Instant::now(), event);
    }
    // A denied new request has no admitted session ID. Do not turn this into
    // a shared-connection failure and misattribute it to accepted streams.
    fn request_failure(
        &mut self,
        reason: Failure,
        code: u64,
        session: Option<crate::diagnostics::OpaqueId>,
        connection: Option<crate::diagnostics::OpaqueId>,
    ) {
        let _ = self
            .diagnostics
            .failure(Instant::now(), connection, session, reason, code, false);
        let mut event = Event::failure(crate::config::Level::Warn, reason, code);
        event.session_id = session;
        event.connection_id = connection;
        let _ = self.diagnostics.event(Instant::now(), event);
    }
    fn close_session(&mut self, generation: u64, code: u64) {
        self.retire_session(generation, code, true);
    }
    fn retire_session(&mut self, generation: u64, code: u64, abort_write: bool) {
        if let Some(mut s) = self.sessions.remove(&generation) {
            self.snapshot.resources.session_queue_packets_peak = self
                .snapshot
                .resources
                .session_queue_packets_peak
                .max(s.queue.peak().count);
            self.snapshot.resources.session_queue_bytes_peak = self
                .snapshot
                .resources
                .session_queue_bytes_peak
                .max(s.queue.peak().bytes);
            let recovering = s.state == State::Recovering;
            if recovering {
                for direction in [Direction::ToTarget, Direction::ToClient] {
                    while s.queue.pop(direction).is_some() {
                        if self
                            .snapshot
                            .counters
                            .local_drops
                            .add(crate::diagnostics::DropReason::Recovering, direction, 1)
                            .is_err()
                        {
                            self.snapshot.recovery.counter_overflow = true;
                        }
                    }
                }
            }
            if let Some(relay) = &s.relay {
                self.retired_io = self.retired_io.merge(relay.get_ref().0.borrow().stats());
            }
            if [masque::H3_MESSAGE_ERROR, masque::H3_DATAGRAM_ERROR].contains(&code) {
                let _ = self.diagnostics.failure(
                    Instant::now(),
                    self.connections.get(&s.connection.0).map(|c| c.id),
                    Some(s.id),
                    Failure::Malformed,
                    code,
                    false,
                );
            }
            let _ = self.diagnostics.finish_session(s.id);
            s.state = State::Closed;
            if let (Some(sender), Some(local)) = (s.sender, s.sender_local)
                && self.senders.get(&(sender, local)) == Some(&generation)
            {
                self.senders.remove(&(sender, local));
            }
            if let Some(c) = self.connections.get_mut(&s.connection.0)
                && s.stream != u64::MAX
            {
                let native = c.transport.quic.dgram_send_queue_len();
                c.transport
                    .retain_cleanup(s.stream, Some(s._session), s._identity.take());
                if abort_write {
                    c.transport.close_stream(s.stream, code);
                } else {
                    c.transport.purge_stream_datagrams(s.stream);
                }
                if recovering {
                    let purged = native.saturating_sub(c.transport.quic.dgram_send_queue_len());
                    if self
                        .snapshot
                        .counters
                        .local_drops
                        .add(
                            crate::diagnostics::DropReason::Recovering,
                            if self.server {
                                Direction::ToClient
                            } else {
                                Direction::ToTarget
                            },
                            purged as u64,
                        )
                        .is_err()
                    {
                        self.snapshot.recovery.counter_overflow = true;
                    }
                }
            }
            self.pending.retain(|p| p.session != generation);
        }
    }
    fn reject(&mut self, connection: u64, stream: u64, status: u16) {
        let mut sent = false;
        if let Some(c) = self.connections.get_mut(&connection) {
            if let Some(h3) = c.transport.h3.as_mut() {
                sent = h3
                    .send_response(
                        &mut c.transport.quic,
                        stream,
                        &masque::response_headers(status),
                        true,
                    )
                    .is_ok();
            }
            c.transport.retain_cleanup(stream, None, None);
            let _ = c
                .transport
                .quic
                .stream_shutdown(stream, quiche::Shutdown::Read, 0x10b);
        }
        let existing = self
            .lookup(connection, stream)
            .and_then(|id| self.sessions.get(&id).map(|s| s.id));
        let correlation = existing.or_else(|| self.diagnostics.next_id().ok());
        self.request_failure(
            match status {
                401 => Failure::Authentication,
                403 | 404 => Failure::Authorization,
                501 => Failure::Capability,
                502 => Failure::Transport,
                503 => Failure::Resource,
                504 => Failure::SetupTimeout,
                _ => Failure::Malformed,
            },
            u64::from(status),
            correlation,
            self.connections.get(&connection).map(|c| c.id),
        );
        if existing.is_none()
            && let Some(id) = correlation
        {
            let _ = self.diagnostics.finish_session(id);
        }
        if sent && let Some(id) = self.lookup(connection, stream) {
            // Queued final rejection headers/FIN must survive local flow cleanup.
            // Read side is shut down, all cancellable app data and permits retire;
            // no accepted association remains and no UDP forwarding is possible.
            self.retire_session(id, 0x10b, false);
        }
    }
    fn lookup(&self, connection: u64, stream: u64) -> Option<u64> {
        self.sessions
            .iter()
            .find_map(|(id, s)| (s.connection.0 == connection && s.stream == stream).then_some(*id))
    }
    fn create_session(
        &mut self,
        connection: u64,
        stream: u64,
        sender: Option<(SocketAddr, SocketAddr)>,
        now: Instant,
    ) -> Result<u64, RuntimeError> {
        if self.stopping
            || !self.connections.contains_key(&connection)
            || sender.is_some_and(|tuple| self.senders.contains_key(&tuple))
            || (stream != u64::MAX && self.lookup(connection, stream).is_some())
        {
            return Err(RuntimeError::Resource);
        }
        if self
            .sessions
            .values()
            .filter(|s| s.connection.0 == connection)
            .count()
            + self
                .connections
                .get(&connection)
                .map_or(0, |c| c.transport.cleanup_count())
            >= self.cfg.limits().max_sessions_per_connection
        {
            return Err(RuntimeError::Resource);
        }
        let session = resource(self.budgets.sessions.reserve(1, 0))?;
        let setup = resource(self.budgets.setup.reserve(1, 0))?;
        let generation = resource(self.generations.next_generation())?;
        let id = self
            .diagnostics
            .next_id()
            .map_err(|_| RuntimeError::Diagnostics)?;
        self.sessions.insert(
            generation.0,
            Session {
                generation,
                connection: crate::buffers::Generation(connection),
                stream,
                state: if self.server {
                    State::Authorizing
                } else {
                    State::Starting
                },
                id,
                sender: sender.map(|(peer, _)| peer),
                sender_local: sender.map(|(_, local)| local),
                identity: None,
                relay: None,
                relay_peer: None,
                receipt: None,
                activity: now,
                started: now,
                queue: SessionQueue::new(
                    self.cfg.limits().session_queue_packets,
                    self.cfg.limits().session_queue_bytes,
                ),
                capsules: CapsuleParser::default(),
                deferred_headers: None,
                body_pending: false,
                capsule_counts: crate::masque::CapsuleCounts::default(),
                _identity: None,
                _session: session,
                _setup: Some(setup),
                _relay: None,
                _fd: None,
            },
        );
        if let Some(sender) = sender {
            self.senders.insert(sender, generation.0);
        }
        Ok(generation.0)
    }
    fn source_admit(&mut self, peer: SocketAddr, now: Instant) -> bool {
        // Fixed hashed source buckets bound state even for spoofed/churning addresses.
        // Collisions share quota; ports cannot create a new source allowance.
        let mut hash = 0usize;
        match peer.ip() {
            std::net::IpAddr::V4(v) => {
                for b in &v.octets()[..3] {
                    hash = hash.wrapping_mul(31) ^ usize::from(*b);
                }
            }
            std::net::IpAddr::V6(v) => {
                for b in &v.octets()[..7] {
                    hash = hash.wrapping_mul(31) ^ usize::from(*b);
                }
            }
        }
        let bucket = &mut self.source_buckets[hash % 64];
        if now.duration_since(bucket.last) >= Duration::from_secs(1) {
            bucket.credits = 16;
            bucket.last = now;
        }
        if bucket.credits == 0 {
            return false;
        }
        bucket.credits -= 1;
        true
    }
    fn outer_packet(
        &mut self,
        index: usize,
        bytes: &mut [u8],
        meta: crate::udp::Received,
        now: Instant,
    ) -> Result<(), RuntimeError> {
        let Ok(header) = quiche::Header::from_slice(bytes, 16) else {
            return Ok(());
        };
        let known = self.connections.iter().find_map(|(id, c)| {
            ((c.initial_cid == header.dcid.as_ref())
                || c.transport
                    .quic
                    .source_ids()
                    .any(|cid| cid.as_ref() == header.dcid.as_ref()))
            .then_some(*id)
        });
        let connection = if let Some(id) = known {
            if !self.server && self.connections[&id].socket != index {
                self.drop_packet(
                    crate::diagnostics::DropReason::StaleGeneration,
                    Direction::ToClient,
                )?;
                return Ok(());
            }
            id
        } else {
            if !self.server
                || bytes.len() < 1200
                || header.ty != quiche::Type::Initial
                || header.dcid.len() < 8
                || !self.source_admit(meta.peer, now)
            {
                return Ok(());
            }
            let mut response = resource(self.pools.scratch.acquire())?;
            if !quiche::version_is_supported(header.version) {
                if let Ok(n) =
                    quiche::negotiate_version(&header.scid, &header.dcid, response.bytes_mut())
                {
                    let _ = self.outer[index]
                        .get_ref()
                        .0
                        .borrow_mut()
                        .send_batch(&[SendPacket {
                            bytes: &response.bytes()[..n],
                            from: meta.local,
                            to: meta.peer,
                        }]);
                }
                return Ok(());
            }
            let token = header.token.as_deref().unwrap_or(&[]);
            if token.is_empty() {
                let cid = transport::random_cid().map_err(|_| RuntimeError::Resource)?;
                let token = self
                    .retry
                    .mint(meta.peer, header.dcid.as_ref(), &cid, now)
                    .map_err(|_| RuntimeError::Transport)?;
                if let Ok(n) = quiche::retry(
                    &header.scid,
                    &header.dcid,
                    &quiche::ConnectionId::from_ref(&cid),
                    &token,
                    header.version,
                    response.bytes_mut(),
                ) {
                    let _ = self.outer[index]
                        .get_ref()
                        .0
                        .borrow_mut()
                        .send_batch(&[SendPacket {
                            bytes: &response.bytes()[..n],
                            from: meta.local,
                            to: meta.peer,
                        }]);
                }
                return Ok(());
            }
            let Ok(original) = self
                .retry
                .verify(meta.peer, header.dcid.as_ref(), token, now)
            else {
                return Ok(());
            };
            let Ok(permit) = self.budgets.connections.reserve(1, 0) else {
                return Ok(());
            };
            let generation = resource(self.generations.next_generation())?;
            let cid = transport::random_cid().map_err(|_| RuntimeError::Resource)?;
            let original = quiche::ConnectionId::from_ref(&original);
            let retry = quiche::ConnectionId::from_ref(header.dcid.as_ref());
            let mut q = quiche::accept_with_retry::<TrackedFactory>(
                &quiche::ConnectionId::from_ref(&cid),
                quiche::RetryConnectionIds {
                    original_destination_cid: &original,
                    retry_source_cid: &retry,
                },
                meta.local,
                meta.peer,
                &mut self.quic_config,
            )
            .map_err(|_| RuntimeError::Transport)?;
            let id = self
                .diagnostics
                .next_id()
                .map_err(|_| RuntimeError::Diagnostics)?;
            Connection::attach_qlog(&mut q, self.qlog.as_ref());
            self.connections.insert(
                generation.0,
                OwnedConnection {
                    transport: Connection::new(q, generation, true, self.cfg, now),
                    socket: index,
                    _permit: permit,
                    _socket_permit: None,
                    id,
                    initial_cid: header.dcid.as_ref().to_vec(),
                    cursors: [FairCursor::default(); 7],
                    receive_density: ReceiveDensity::default(),
                    enqueue_pending: false,
                    output_pending: false,
                },
            );
            generation.0
        };
        if let Some(c) = self.connections.get_mut(&connection) {
            let queued_before = c.transport.quic.dgram_recv_queue_len();
            let received = c.transport.receive(
                bytes,
                quiche::RecvInfo {
                    from: meta.peer,
                    to: meta.local,
                },
                now,
            );
            // Retain recent decoded DATAGRAM density, not encrypted packet
            // length. Sparse tails/ACKs do not immediately erase a dense burst.
            c.receive_density.observe(
                c.transport
                    .quic
                    .dgram_recv_queue_len()
                    .saturating_sub(queued_before),
            );
            match received {
                Ok(_) | Err(quiche::Error::Done) => {}
                Err(error) => {
                    self.failure(
                        if matches!(error, quiche::Error::TlsFail) {
                            Failure::Trust
                        } else {
                            Failure::Transport
                        },
                        if matches!(error, quiche::Error::TlsFail) {
                            0
                        } else {
                            transport::native_error_code(error)
                        },
                        None,
                        self.connections.get(&connection).map(|c| c.id),
                    );
                    if !self.server && matches!(error, quiche::Error::TlsFail) {
                        self.permanent_client_failure(now, Failure::Trust, 0)?;
                        self.stop_connection(connection)?;
                        if !self.ever_active {
                            return Err(RuntimeError::Trust);
                        }
                    }
                }
            }
        }
        Ok(())
    }
    fn receive_outer(&mut self, index: usize, now: Instant) -> Result<bool, RuntimeError> {
        if !self.outer[index].get_ref().read_pending() {
            return Ok(false);
        }
        let errors = self.outer[index]
            .get_ref()
            .0
            .borrow_mut()
            .drain_errors()
            .map_err(|_| RuntimeError::Socket)?;
        for error in errors.errors.into_iter().flatten() {
            if error.errno == libc::EMSGSIZE as u32 {
                self.snapshot.counters.outer_pmtu_errors = self
                    .snapshot
                    .counters
                    .outer_pmtu_errors
                    .checked_add(1)
                    .ok_or(RuntimeError::Diagnostics)?;
                for c in self.connections.values_mut().filter(|c| c.socket == index) {
                    if c.transport
                        .quic
                        .path_stats()
                        .any(|path| path.active && path.peer_addr == error.destination)
                    {
                        c.transport.quic.revalidate_pmtu();
                    }
                }
            }
        }
        self.snapshot
            .counters
            .local_drops
            .add(
                crate::diagnostics::DropReason::AncillaryInvalid,
                if self.server {
                    Direction::ToTarget
                } else {
                    Direction::ToClient
                },
                errors.malformed as u64,
            )
            .map_err(|_| RuntimeError::Diagnostics)?;
        // Match a socket visit to the incoming-DATAGRAM service quota. Keep
        // large-payload batches broad enough to coalesce ACK work; coalesced
        // small payloads reduce the visit before the bounded native queue fills.
        let quota = self.outer_receive_quota(index);
        let mut slabs = Vec::with_capacity(quota);
        for _ in 0..quota {
            if let Ok(s) = self.pools.scratch.acquire() {
                slabs.push(s);
            } else {
                break;
            }
        }
        if slabs.is_empty() {
            return Ok(false);
        }
        let mut packets = Vec::with_capacity(slabs.len());
        let result = read_io(&self.outer[index], |inner| {
            inner.0.borrow_mut().recv_batch(&mut slabs, &mut packets)
        });
        let overflow = self.outer[index].get_ref().0.borrow_mut().take_overflow();
        self.snapshot.counters.kernel_rx_overflow = self
            .snapshot
            .counters
            .kernel_rx_overflow
            .checked_add(overflow)
            .ok_or(RuntimeError::Diagnostics)?;
        match result {
            Ok(n) => {
                self.snapshot.counters.outer_socket_received = self
                    .snapshot
                    .counters
                    .outer_socket_received
                    .checked_add(n as u64)
                    .ok_or(RuntimeError::Diagnostics)?;
                for packet in packets {
                    self.snapshot.counters.outer_socket_received_bytes = self
                        .snapshot
                        .counters
                        .outer_socket_received_bytes
                        .checked_add(packet.length as u64)
                        .ok_or(RuntimeError::Diagnostics)?;
                    if let Some(fault) = packet.fault {
                        self.drop_packet(
                            if fault == ReceiveFault::Truncated {
                                crate::diagnostics::DropReason::SocketTruncated
                            } else {
                                crate::diagnostics::DropReason::AncillaryInvalid
                            },
                            if self.server {
                                Direction::ToTarget
                            } else {
                                Direction::ToClient
                            },
                        )?;
                        continue;
                    }
                    if packet.length > self.cfg.transport().outer_udp_max_bytes {
                        self.drop_packet(
                            crate::diagnostics::DropReason::Oversized,
                            if self.server {
                                Direction::ToTarget
                            } else {
                                Direction::ToClient
                            },
                        )?;
                        continue;
                    }
                    self.outer_packet(
                        index,
                        &mut slabs[packet.index].bytes_mut()[..packet.length],
                        packet,
                        now,
                    )?;
                }
                Ok(n > 0 || errors.pending)
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                self.outer[index].get_ref().set_read_pending(errors.pending);
                Ok(errors.pending)
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => Ok(true),
            Err(e) if crate::udp::is_peer_error(&e) => {
                self.snapshot.counters.outer_async_errors = self
                    .snapshot
                    .counters
                    .outer_async_errors
                    .checked_add(1)
                    .ok_or(RuntimeError::Diagnostics)?;
                Ok(true)
            }
            Err(_) => Err(RuntimeError::Socket),
        }
    }
    fn outer_receive_quota(&self, index: usize) -> usize {
        // A connection without an admitted active flow must not influence
        // the service quantum of other clients on a shared server socket.
        let density = self
            .sessions
            .values()
            .filter(|s| s.state == State::Active)
            .filter_map(|s| self.connections.get(&s.connection.0))
            .filter(|c| c.socket == index)
            .map(|c| c.receive_density.estimate())
            .max()
            .unwrap_or(1);
        self.cfg.io().batch_max.min((32 / density).max(1))
    }
    fn client_input(&mut self, now: Instant) -> Result<bool, RuntimeError> {
        let Some(listener) = self.listener.as_ref() else {
            return Ok(false);
        };
        if !listener.get_ref().read_pending() {
            return Ok(false);
        }
        let mut slabs = Vec::with_capacity(self.cfg.io().batch_max.min(32));
        for _ in 0..self.cfg.io().batch_max.min(32) {
            if let Ok(s) = self.pools.application.acquire() {
                slabs.push(s);
            } else {
                break;
            }
        }
        if slabs.is_empty() {
            return Ok(false);
        }
        let mut packets = Vec::with_capacity(slabs.len());
        let result = read_io(listener, |inner| {
            inner.0.borrow_mut().recv_batch(&mut slabs, &mut packets)
        });
        let overflow = listener.get_ref().0.borrow_mut().take_overflow();
        self.snapshot.counters.kernel_rx_overflow = self
            .snapshot
            .counters
            .kernel_rx_overflow
            .checked_add(overflow)
            .ok_or(RuntimeError::Diagnostics)?;
        match result {
            Ok(n) => {
                for p in packets {
                    if let Some(fault) = p.fault {
                        if let Some(s) = self
                            .senders
                            .get(&(p.peer, p.local))
                            .and_then(|id| self.sessions.get(id))
                        {
                            let session = s.id;
                            let connection = self.connections.get(&s.connection.0).map(|c| c.id);
                            self.failure(
                                if fault == ReceiveFault::Truncated {
                                    Failure::Size
                                } else {
                                    Failure::Malformed
                                },
                                3,
                                Some(session),
                                connection,
                            );
                        }
                        self.drop_packet(
                            if fault == ReceiveFault::Truncated {
                                crate::diagnostics::DropReason::SocketTruncated
                            } else {
                                crate::diagnostics::DropReason::AncillaryInvalid
                            },
                            Direction::ToTarget,
                        )?;
                        continue;
                    }
                    self.snapshot
                        .counters
                        .ingress(Direction::ToTarget, p.length)
                        .map_err(|_| RuntimeError::Diagnostics)?;
                    let existing = self.senders.get(&(p.peer, p.local)).copied();
                    let id = if let Some(id) = existing {
                        id
                    } else {
                        if !self.latch.may_setup() {
                            self.drop_packet(
                                crate::diagnostics::DropReason::Rejected,
                                Direction::ToTarget,
                            )?;
                            continue;
                        }
                        if self.setup_retry.deadline().is_some() && !self.setup_retry.due(now) {
                            self.drop_packet(
                                crate::diagnostics::DropReason::Recovering,
                                Direction::ToTarget,
                            )?;
                            continue;
                        }
                        self.setup_retry.started();
                        let Some(connection) = self.primary else {
                            self.drop_packet(
                                crate::diagnostics::DropReason::Recovering,
                                Direction::ToTarget,
                            )?;
                            continue;
                        };
                        match self.create_session(
                            connection,
                            u64::MAX,
                            Some((p.peer, p.local)),
                            now,
                        ) {
                            Ok(id) => id,
                            Err(_) => {
                                self.drop_packet(
                                    crate::diagnostics::DropReason::QueueFull,
                                    Direction::ToTarget,
                                )?;
                                continue;
                            }
                        }
                    };
                    let Some(s) = self.sessions.get(&id) else {
                        continue;
                    };
                    if s.state != State::Active {
                        self.drop_packet(
                            if self.snapshot.recovery.connection_attempts > 1 {
                                crate::diagnostics::DropReason::Recovering
                            } else {
                                crate::diagnostics::DropReason::NotActive
                            },
                            Direction::ToTarget,
                        )?;
                        continue;
                    }
                    self.queue_outgoing(id, &slabs[p.index].bytes()[..p.length], now)?;
                }
                Ok(n > 0)
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                listener.get_ref().set_read_pending(false);
                Ok(false)
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => Ok(true),
            Err(_) => Err(RuntimeError::Socket),
        }
    }
    fn headers(
        &mut self,
        connection: u64,
        stream: u64,
        list: Vec<quiche::h3::Header>,
        more: bool,
        now: Instant,
    ) -> Result<(), RuntimeError> {
        if self.server {
            let RoleConfig::Server(server) = self.cfg.role() else {
                return Err(RuntimeError::Transport);
            };
            let reserved = self.lookup(connection, stream);
            if let Some(id) = reserved {
                if self.sessions[&id].state != State::Starting
                    || self.sessions[&id].deferred_headers.is_some()
                {
                    self.close_session(id, masque::H3_MESSAGE_ERROR);
                    return Ok(());
                }
                self.sessions.get_mut(&id).unwrap().state = State::Authorizing;
            }
            let request = match masque::request(&list, &server.proxy_authority, &server.path_prefix)
            {
                Ok(r) => r,
                Err(WireError::Status(code)) => {
                    self.reject(connection, stream, code);
                    if let Some(id) = reserved {
                        self.close_session(id, 0x10b);
                    }
                    return Ok(());
                }
                Err(_) => {
                    if let Some(id) = reserved {
                        self.close_session(id, masque::H3_MESSAGE_ERROR);
                    }
                    if let Some(c) = self.connections.get_mut(&connection) {
                        c.transport.close_stream(stream, masque::H3_MESSAGE_ERROR);
                    }
                    return Ok(());
                }
            };
            if !more {
                self.reject(connection, stream, 400);
                if let Some(id) = reserved {
                    self.close_session(id, 0x10b);
                }
                return Ok(());
            }
            let id = match reserved
                .map_or_else(|| self.create_session(connection, stream, None, now), Ok)
            {
                Ok(id) => id,
                Err(_) => {
                    self.reject(connection, stream, 503);
                    return Ok(());
                }
            };
            let identity = match policy::authenticate(&server.identities, &request.token) {
                Ok(identity) => identity,
                Err(_) => {
                    self.reject(connection, stream, 401);
                    self.close_session(id, 0x10b);
                    return Ok(());
                }
            };
            if self.bind_identity(id, identity).is_err() {
                self.reject(connection, stream, 503);
                self.close_session(id, 0x10b);
                return Ok(());
            }
            let key = SetupKey {
                connection: crate::buffers::Generation(connection),
                session: crate::buffers::Generation(id),
            };
            let timeout = Duration::from_millis(self.cfg.recovery().setup_timeout_ms);
            let resolver = self.resolver;
            let identities = &server.identities;
            self.pending.push(Pending {
                session: id,
                key,
                future: Box::pin(async move {
                    policy::prepare_authenticated(
                        &identities[identity],
                        &request.host,
                        request.port,
                        resolver,
                        key,
                        timeout,
                    )
                    .await
                    .map(|target| (identity, target))
                }),
            });
        } else if let Some(id) = self.lookup(connection, stream) {
            let result = masque::response(&list, !more);
            match result {
                Ok(Response::Informational) => {}
                Ok(Response::Accepted)
                    if !self.latch.may_setup() && self.sessions[&id].state != State::Active =>
                {
                    // A valid but delayed success after mapping cancellation
                    // is discarded, not reported as a malformed peer response.
                    self.close_session(id, 0x10b);
                }
                Ok(Response::Accepted) => {
                    if let Some(s) = self.sessions.get_mut(&id) {
                        if s.state == State::AwaitingAcceptance {
                            self.ever_active = true;
                            self.setup_retry.reset();
                            s.state = State::Active;
                            self.snapshot.lifecycle.accepted_total = self
                                .snapshot
                                .lifecycle
                                .accepted_total
                                .checked_add(1)
                                .ok_or(RuntimeError::Diagnostics)?;
                            s.udp_activity(now);
                            s._setup = None;
                        } else {
                            self.close_session(id, masque::H3_MESSAGE_ERROR);
                        }
                    }
                }
                Ok(Response::Rejected(code)) => {
                    let permanent = [400, 401, 403, 404, 405, 501].contains(&code);
                    let reason = if code == 401 {
                        Failure::Authentication
                    } else if code == 501 {
                        Failure::Capability
                    } else {
                        Failure::Authorization
                    };
                    if !permanent && self.setup_retry.deadline().is_none() {
                        let jitter = Self::jitter(self.setup_retry.jitter_ceiling_ms())?;
                        self.setup_retry.schedule(now, jitter);
                    }
                    self.failure(
                        reason,
                        u64::from(code),
                        self.sessions.get(&id).map(|s| s.id),
                        self.connections.get(&connection).map(|c| c.id),
                    );
                    // Correlate the refusing Session before mapping-wide
                    // cancellation removes its opaque id and pending status.
                    if permanent {
                        self.permanent_client_failure(now, reason, u64::from(code))?;
                    }
                    self.close_session(id, 0x10b);
                    if permanent && !self.ever_active {
                        return Err(RuntimeError::Transport);
                    }
                }
                Err(_) => {
                    self.failure(
                        Failure::Malformed,
                        masque::H3_MESSAGE_ERROR,
                        self.sessions.get(&id).map(|s| s.id),
                        self.connections.get(&connection).map(|c| c.id),
                    );
                    self.close_session(id, masque::H3_MESSAGE_ERROR);
                }
            }
        }
        Ok(())
    }
    fn bind_identity(&mut self, id: u64, identity: usize) -> Result<(), RuntimeError> {
        let s = self.sessions.get_mut(&id).ok_or(RuntimeError::Resource)?;
        if let Some(previous) = s.identity {
            return if previous == identity {
                Ok(())
            } else {
                Err(RuntimeError::Resource)
            };
        }
        let permit = resource(
            self.identities
                .get(identity)
                .ok_or(RuntimeError::Resource)?
                .reserve(1, 0),
        )?;
        s.identity = Some(identity);
        s._identity = Some(permit);
        Ok(())
    }
    fn setup_done(
        &mut self,
        id: u64,
        key: SetupKey,
        result: Result<(usize, policy::ResolvedTarget), PolicyError>,
        now: Instant,
    ) -> Result<(), RuntimeError> {
        let Some(s) = self.sessions.get(&id) else {
            return Ok(());
        };
        if s.generation != key.session
            || s.connection != key.connection
            || s.state != State::Authorizing
        {
            return Ok(());
        }
        let connection = s.connection.0;
        let stream = s.stream;
        let (identity, target) = match result {
            Ok(v) => v,
            Err(e) => {
                let status = match e {
                    PolicyError::Authentication => 401,
                    PolicyError::Denied => 403,
                    PolicyError::Timeout => 504,
                    PolicyError::Capacity => 503,
                    _ => 502,
                };
                if e == PolicyError::Resolution {
                    self.failure(
                        Failure::Resolution,
                        502,
                        self.sessions.get(&id).map(|s| s.id),
                        self.connections.get(&connection).map(|c| c.id),
                    );
                }
                self.reject(connection, stream, status);
                self.close_session(id, 0x10b);
                return Ok(());
            }
        };
        if self.bind_identity(id, identity).is_err() {
            self.reject(connection, stream, 503);
            self.close_session(id, 0x10b);
            return Ok(());
        }
        let Ok(address) = target.for_current(key) else {
            return Ok(());
        };
        let relay = match self.budgets.relay_fds.reserve(1, 0) {
            Ok(r) => r,
            Err(_) => {
                self.reject(connection, stream, 503);
                self.close_session(id, 0x10b);
                return Ok(());
            }
        };
        let fd = match self.budgets.fds.reserve(1, 0) {
            Ok(fd) => fd,
            Err(_) => {
                self.reject(connection, stream, 503);
                self.close_session(id, 0x10b);
                return Ok(());
            }
        };
        let bind = if address.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        };
        let mut socket = match Udp::bind(
            bind.parse().map_err(|_| RuntimeError::Bind)?,
            self.cfg.io(),
            true,
        ) {
            Ok(s) => s,
            Err(_) => {
                self.failure(
                    Failure::Socket,
                    502,
                    self.sessions.get(&id).map(|s| s.id),
                    self.connections.get(&connection).map(|c| c.id),
                );
                self.reject(connection, stream, 502);
                self.close_session(id, 0x10b);
                return Ok(());
            }
        };
        if socket.connect(address).is_err() {
            self.failure(
                Failure::Socket,
                502,
                self.sessions.get(&id).map(|s| s.id),
                self.connections.get(&connection).map(|c| c.id),
            );
            self.reject(connection, stream, 502);
            self.close_session(id, 0x10b);
            return Ok(());
        }
        self.snapshot
            .socket_buffers
            .application_receive_max_per_socket = self
            .snapshot
            .socket_buffers
            .application_receive_max_per_socket
            .max(socket.receive_buffer);
        self.snapshot.socket_buffers.application_send_max_per_socket = self
            .snapshot
            .socket_buffers
            .application_send_max_per_socket
            .max(socket.send_buffer);
        let socket = match PollSocket::register(socket) {
            Ok(socket) => socket,
            Err(_) => {
                self.reject(connection, stream, 502);
                self.close_session(id, 0x10b);
                return Ok(());
            }
        };
        let s = self.sessions.get_mut(&id).ok_or(RuntimeError::Transport)?;
        s.identity = Some(identity);
        s.relay = Some(socket);
        s.relay_peer = Some(address);
        s._relay = Some(relay);
        s._fd = Some(fd);
        s.state = State::AwaitingCommitment;
        s.udp_activity(now);
        let c = self
            .connections
            .get_mut(&connection)
            .ok_or(RuntimeError::Transport)?;
        let h3 = c.transport.h3.as_mut().ok_or(RuntimeError::Transport)?;
        let cid = c.id;
        let (result, receipt) = transport::track_response(|| {
            h3.send_response(
                &mut c.transport.quic,
                stream,
                &masque::response_headers(200),
                false,
            )
        });
        if result.is_err() {
            if result == Err(quiche::h3::Error::StreamBlocked) {
                return Ok(());
            }
            self.failure(
                Failure::Resource,
                503,
                self.sessions.get(&id).map(|s| s.id),
                Some(cid),
            );
            self.close_session(id, 0x10b);
        } else {
            self.sessions
                .get_mut(&id)
                .ok_or(RuntimeError::Transport)?
                .receipt = Some(receipt);
        }
        Ok(())
    }
    fn negotiate_transport(&mut self, connection: u64) -> Result<bool, RuntimeError> {
        if !self.server
            && self.primary == Some(connection)
            && self
                .connections
                .get(&connection)
                .is_some_and(|c| c.transport.quic.is_established())
            && self.snapshot.recovery.client_state == Some(ClientState::Handshaking)
        {
            self.client_state(ClientState::Negotiating, Instant::now())?;
        }
        let result = self
            .connections
            .get_mut(&connection)
            .ok_or(RuntimeError::Transport)?
            .transport
            .negotiate();
        match result {
            Ok(value) => {
                if value
                    && !self.server
                    && self.primary == Some(connection)
                    && self.latch.may_setup()
                {
                    self.reconnect.reset();
                    self.client_state(ClientState::Active, Instant::now())?;
                }
                Ok(value)
            }
            Err(TransportError::Capability) => {
                if !self.server {
                    self.permanent_client_failure(Instant::now(), Failure::Capability, 0)?;
                }
                Err(RuntimeError::Capability)
            }
            Err(TransportError::Tls) => Err(RuntimeError::Trust),
            Err(TransportError::Native(code)) => Err(RuntimeError::TransportCode(code)),
            Err(_) => Err(RuntimeError::Transport),
        }
    }
    fn queue_outgoing(&mut self, id: u64, bytes: &[u8], now: Instant) -> Result<(), RuntimeError> {
        let direction = if self.server {
            Direction::ToClient
        } else {
            Direction::ToTarget
        };
        let Some(s) = self.sessions.get_mut(&id) else {
            return Ok(());
        };
        if s.state != State::Active {
            return Ok(());
        }
        let connection = s.connection.0;
        let ceiling = self
            .connections
            .get(&s.connection.0)
            .map_or(0, |c| c.transport.payload_ceiling(s.stream));
        if bytes.len() > ceiling {
            let session_id = s.id;
            let connection_id = self.connections.get(&s.connection.0).map(|c| c.id);
            self.failure(
                Failure::Size,
                if bytes.len() > self.cfg.transport().application_max_bytes {
                    1
                } else {
                    2
                },
                Some(session_id),
                connection_id,
            );
            self.drop_packet(
                if bytes.len() > self.cfg.transport().application_max_bytes {
                    crate::diagnostics::DropReason::Oversized
                } else {
                    crate::diagnostics::DropReason::Pmtu
                },
                direction,
            )?;
            return Ok(());
        }
        s.udp_activity(now);
        let session_id = s.id;
        let connection_id = self.connections.get(&s.connection.0).map(|c| c.id);
        let mut slab = match self.pools.application.acquire() {
            Ok(slab) => slab,
            Err(_) => {
                self.failure(Failure::Resource, 1, Some(session_id), connection_id);
                self.drop_packet(crate::diagnostics::DropReason::QueueFull, direction)?;
                return Ok(());
            }
        };
        slab.bytes_mut()[..bytes.len()].copy_from_slice(bytes);
        self.snapshot.profile.queue_copy_bytes = self
            .snapshot
            .profile
            .queue_copy_bytes
            .checked_add(bytes.len() as u64)
            .ok_or(RuntimeError::Diagnostics)?;
        let packet = resource(crate::buffers::Datagram::new(
            slab,
            0,
            bytes.len(),
            direction,
            s.generation,
            s.connection,
            now,
            now + Duration::from_millis(self.cfg.limits().application_residence_ms),
        ))?;
        if s.queue.push(packet).is_err() {
            self.failure(Failure::Resource, 2, Some(session_id), connection_id);
            self.drop_packet(crate::diagnostics::DropReason::QueueFull, direction)?;
        } else if let Some(c) = self.connections.get_mut(&connection) {
            c.enqueue_pending = true;
        }
        Ok(())
    }
    fn select_sessions(
        &mut self,
        connection: u64,
        class: usize,
        ids: Vec<u64>,
        limit: usize,
    ) -> Vec<u64> {
        let Some(c) = self.connections.get_mut(&connection) else {
            return Vec::new();
        };
        round_robin(ids, &mut c.cursors[class], limit)
    }
    /// One datagram/session/round before the shared native queue. A blocked
    /// native queue retains each original packet and expiry, without replay.
    fn schedule_datagrams(&mut self, connection: u64, now: Instant) -> Result<bool, RuntimeError> {
        let direction = if self.server {
            Direction::ToClient
        } else {
            Direction::ToTarget
        };
        let ids = self
            .sessions
            .iter()
            .filter_map(|(id, s)| {
                (s.connection.0 == connection
                    && s.state == State::Active
                    && s.queue.front(direction).is_some())
                .then_some(*id)
            })
            .collect();
        let Some(c) = self.connections.get_mut(&connection) else {
            return Ok(false);
        };
        c.enqueue_pending = false;
        // Preview a finite round, but do not commit unserved producers when a
        // partially full native queue interrupts it. The fixed array bounds
        // progress metadata without an extra heap allocation.
        let mut preview = c.cursors[3];
        let mut steps = [FairCursor::default(); 32];
        let mut selected = 0;
        let ids = round_robin_recorded(ids, &mut preview, 32, |step| {
            steps[selected] = step;
            selected += 1;
        });
        let horizon = ids.iter().copied().max();
        let mut scratch = resource(self.pools.scratch.acquire())?;
        let mut serviced = 0;
        for round in 0..32 {
            let before = serviced;
            for (index, &id) in ids.iter().enumerate() {
                if serviced == 32 {
                    break;
                }
                let s = self.sessions.get_mut(&id).ok_or(RuntimeError::Transport)?;
                let Some(packet) = s.queue.front(direction) else {
                    continue;
                };
                let cursor = &mut self
                    .connections
                    .get_mut(&connection)
                    .ok_or(RuntimeError::Transport)?
                    .cursors[3];
                let previous = *cursor;
                *cursor = if round == 0 {
                    steps[index]
                } else {
                    FairCursor {
                        last: Some(id),
                        ceiling: horizon,
                    }
                };
                serviced += 1;
                self.snapshot.profile.application_queue_age_max_us =
                    self.snapshot.profile.application_queue_age_max_us.max(
                        now.saturating_duration_since(packet.received_at())
                            .as_micros() as u64,
                    );
                self.snapshot.histograms.application_queue_age_us.observe(
                    now.saturating_duration_since(packet.received_at())
                        .as_micros() as u64,
                );
                if packet.session() != s.generation || packet.connection() != s.connection {
                    s.queue.pop(direction);
                    self.drop_packet(crate::diagnostics::DropReason::StaleGeneration, direction)?;
                    continue;
                }
                let result = self
                    .connections
                    .get_mut(&connection)
                    .ok_or(RuntimeError::Transport)?
                    .transport
                    .enqueue(
                        s.stream,
                        packet.bytes(),
                        packet.received_at(),
                        now,
                        scratch.bytes_mut(),
                    );
                match result {
                    Ok(()) => {
                        s.queue.pop(direction);
                        self.connections
                            .get_mut(&connection)
                            .ok_or(RuntimeError::Transport)?
                            .output_pending = true;
                        self.snapshot.counters.quic_dgram_enqueued = self
                            .snapshot
                            .counters
                            .quic_dgram_enqueued
                            .checked_add(1)
                            .ok_or(RuntimeError::Diagnostics)?;
                    }
                    Err(TransportError::Capacity) => {
                        // No packet was consumed: retry this producer first
                        // when a native slot next becomes available.
                        let c = self
                            .connections
                            .get_mut(&connection)
                            .ok_or(RuntimeError::Transport)?;
                        c.cursors[3] = previous;
                        // A full committed FIFO needs physical/pacing progress,
                        // not an immediate application-enqueue continuation.
                        // Original queue expiries remain owner deadlines.
                        return Ok(!c.transport.output_full());
                    }
                    Err(error) => {
                        s.queue.pop(direction);
                        self.drop_packet(native_drop(error), direction)?;
                    }
                }
            }
            if serviced == before || serviced == 32 {
                break;
            }
        }
        let pending = self
            .sessions
            .values()
            .any(|s| s.connection.0 == connection && s.queue.front(direction).is_some());
        if pending {
            self.connections
                .get_mut(&connection)
                .ok_or(RuntimeError::Transport)?
                .enqueue_pending = true;
        }
        Ok(pending)
    }
    fn defer_headers(
        &mut self,
        connection: u64,
        stream: u64,
        list: Vec<quiche::h3::Header>,
        more: bool,
        now: Instant,
    ) -> Result<(), RuntimeError> {
        use quiche::h3::NameValue;
        if !self.server {
            return Err(RuntimeError::Capability);
        }
        if let Some(id) = self.lookup(connection, stream) {
            self.close_session(id, masque::H3_MESSAGE_ERROR);
            return Ok(());
        }
        let size = list.iter().try_fold(0usize, |n, h| {
            n.checked_add(h.name().len())
                .and_then(|n| n.checked_add(h.value().len()))
        });
        if list.len() > 32 || size.is_none_or(|n| n > 16384) {
            if let Some(c) = self.connections.get_mut(&connection) {
                c.transport.close_stream(stream, masque::H3_MESSAGE_ERROR);
            }
            return Ok(());
        }
        let id = match self.create_session(connection, stream, None, now) {
            Ok(id) => id,
            Err(_) => {
                self.reject(connection, stream, 503);
                return Ok(());
            }
        };
        // Repack into exact bounded native buffers; retain no decoder capacity.
        let headers = list
            .iter()
            .map(|h| quiche::h3::Header::new(h.name(), h.value()))
            .collect();
        let s = self.sessions.get_mut(&id).unwrap();
        s.state = State::Starting;
        s.deferred_headers = Some((headers, more));
        Ok(())
    }
    fn service_deferred_headers(
        &mut self,
        connection: u64,
        now: Instant,
    ) -> Result<usize, RuntimeError> {
        let ids = self
            .sessions
            .iter()
            .filter_map(|(id, s)| {
                (s.connection.0 == connection && s.deferred_headers.is_some()).then_some(*id)
            })
            .collect();
        let ids = self.select_sessions(connection, 6, ids, 4);
        let count = ids.len();
        for id in ids {
            let s = self.sessions.get_mut(&id).unwrap();
            let stream = s.stream;
            let (headers, more) = s.deferred_headers.take().unwrap();
            self.headers(connection, stream, headers, more, now)?;
        }
        Ok(count)
    }
    fn h3_work(&mut self, connection: u64, now: Instant) -> Result<bool, RuntimeError> {
        if self
            .connections
            .get(&connection)
            .is_some_and(|c| c.transport.critical_failed())
        {
            return Ok(false);
        }
        let ready = self.negotiate_transport(connection)?;
        if self
            .connections
            .get(&connection)
            .is_none_or(|c| c.transport.h3.is_none())
        {
            return Ok(false);
        }
        let mut setup_progress = if ready && self.server {
            self.service_deferred_headers(connection, now)?
        } else {
            0
        };
        if ready && !self.server && self.latch.may_setup() {
            let ids: Vec<_> = self
                .sessions
                .iter()
                .filter_map(|(id, s)| {
                    (s.connection.0 == connection && s.state == State::Starting).then_some(*id)
                })
                .collect();
            let ids = self.select_sessions(connection, 0, ids, 4);
            for id in ids {
                let RoleConfig::Client(client) = self.cfg.role() else {
                    return Err(RuntimeError::Transport);
                };
                let path =
                    masque::UriTemplate::parse(&client.uri_template, &client.proxy_authority)
                        .and_then(|t| t.expand(&client.target_host, client.target_port))
                        .map_err(|_| RuntimeError::Transport)?;
                let headers = masque::request_headers(
                    &client.proxy_authority,
                    &path,
                    client.token.canonical_text(),
                );
                let c = self
                    .connections
                    .get_mut(&connection)
                    .ok_or(RuntimeError::Transport)?;
                if !c.transport.may_open() {
                    continue;
                }
                let stream = c
                    .transport
                    .h3
                    .as_mut()
                    .ok_or(RuntimeError::Transport)?
                    .send_request(&mut c.transport.quic, &headers, false);
                match stream {
                    Ok(stream) => {
                        setup_progress += 1;
                        let s = self.sessions.get_mut(&id).ok_or(RuntimeError::Transport)?;
                        s.stream = stream;
                        s.state = State::AwaitingAcceptance;
                    }
                    Err(
                        quiche::h3::Error::StreamBlocked
                        | quiche::h3::Error::TransportError(quiche::Error::StreamLimit),
                    ) => {}
                    Err(_) => self.close_session(id, 0x10b),
                }
            }
        }
        if self.server {
            let ids: Vec<_> = self
                .sessions
                .iter()
                .filter(|(_, s)| {
                    s.connection.0 == connection
                        && s.state == State::AwaitingCommitment
                        && s.receipt.is_none()
                })
                .map(|(id, _)| *id)
                .collect();
            let ids = self.select_sessions(connection, 1, ids, 4);
            for id in ids {
                let stream = self.sessions[&id].stream;
                let c = self
                    .connections
                    .get_mut(&connection)
                    .ok_or(RuntimeError::Transport)?;
                let h3 = c.transport.h3.as_mut().ok_or(RuntimeError::Transport)?;
                let (result, receipt) = transport::track_response(|| {
                    h3.send_response(
                        &mut c.transport.quic,
                        stream,
                        &masque::response_headers(200),
                        false,
                    )
                });
                match result {
                    Ok(()) => {
                        setup_progress += 1;
                        self.sessions
                            .get_mut(&id)
                            .ok_or(RuntimeError::Transport)?
                            .receipt = Some(receipt)
                    }
                    Err(quiche::h3::Error::StreamBlocked) => {}
                    Err(_) => self.close_session(id, 0x10b),
                }
            }
        }
        let mut serviced = 0;
        while serviced < 4 {
            let event = {
                let c = self
                    .connections
                    .get_mut(&connection)
                    .ok_or(RuntimeError::Transport)?;
                c.transport
                    .h3
                    .as_mut()
                    .ok_or(RuntimeError::Transport)?
                    .poll(&mut c.transport.quic)
            };
            match event {
                Ok((stream, quiche::h3::Event::Headers { list, more_frames })) => {
                    if !self.negotiate_transport(connection)? {
                        self.defer_headers(connection, stream, list, more_frames, now)?;
                    } else {
                        self.headers(connection, stream, list, more_frames, now)?;
                    }
                }
                Ok((stream, quiche::h3::Event::Data)) => {
                    if let Some(id) = self.lookup(connection, stream)
                        && let Some(s) = self.sessions.get_mut(&id)
                    {
                        s.body_pending = true;
                    }
                }
                Ok((stream, quiche::h3::Event::Finished)) => {
                    if let Some(id) = self.lookup(connection, stream) {
                        let code = self.sessions.get_mut(&id).map_or(0x100, |s| {
                            s.capsules
                                .feed(&[], true)
                                .err()
                                .and_then(|e| e.code())
                                .unwrap_or(0x100)
                        });
                        self.close_session(id, code);
                    }
                }
                Ok((stream, quiche::h3::Event::Reset(code))) => {
                    if let Some(id) = self.lookup(connection, stream) {
                        // RESET aborts an incomplete message; it is not FIN and
                        // must not turn legitimate cancellation into a parser error.
                        self.failure(
                            Failure::Transport,
                            code,
                            self.sessions.get(&id).map(|s| s.id),
                            self.connections.get(&connection).map(|c| c.id),
                        );
                        self.close_session(id, 0x100);
                    }
                }
                Ok((cutoff, quiche::h3::Event::GoAway)) => {
                    self.connections
                        .get_mut(&connection)
                        .ok_or(RuntimeError::Transport)?
                        .transport
                        .goaway(cutoff)
                        .map_err(|_| RuntimeError::Transport)?;
                    if !self.server {
                        let ids: Vec<_> = self
                            .sessions
                            .iter()
                            .filter_map(|(id, s)| {
                                (s.connection.0 == connection && s.stream >= cutoff).then_some(*id)
                            })
                            .collect();
                        for id in ids {
                            self.close_session(id, 0x10b);
                        }
                        if self.primary == Some(connection) {
                            self.primary = None;
                            self.schedule_reconnect(now)?;
                        }
                    }
                }
                Ok((stream, quiche::h3::Event::PriorityUpdate)) => {
                    if let Some(h3) = self
                        .connections
                        .get_mut(&connection)
                        .and_then(|c| c.transport.h3.as_mut())
                    {
                        let _ = h3.take_last_priority_update(stream);
                    }
                }
                Err(quiche::h3::Error::Done) => break,
                Err(error) => {
                    return Err(RuntimeError::TransportCode(transport::h3_error_code(error)));
                }
            }
            serviced += 1;
        }
        let body_ids: Vec<_> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.connection.0 == connection && s.body_pending)
            .map(|(id, _)| *id)
            .collect();
        let body_ids = self.select_sessions(connection, 2, body_ids, 4);
        for id in body_ids {
            let stream = self.sessions[&id].stream;
            let mut scratch = resource(self.pools.scratch.acquire())?;
            let c = self
                .connections
                .get_mut(&connection)
                .ok_or(RuntimeError::Transport)?;
            let result = c
                .transport
                .h3
                .as_mut()
                .ok_or(RuntimeError::Transport)?
                .recv_body(&mut c.transport.quic, stream, scratch.bytes_mut());
            match result {
                Ok(n) => {
                    let s = self.sessions.get_mut(&id).ok_or(RuntimeError::Transport)?;
                    match s.capsules.feed(&scratch.bytes()[..n], false) {
                        Ok(counts) => {
                            let old = s.capsule_counts;
                            s.capsule_counts = counts;
                            let direction = if self.server {
                                Direction::ToTarget
                            } else {
                                Direction::ToClient
                            };
                            self.snapshot
                                .counters
                                .local_drops
                                .add(
                                    crate::diagnostics::DropReason::UnsupportedCarrier,
                                    direction,
                                    counts.unsupported - old.unsupported,
                                )
                                .map_err(|_| RuntimeError::Diagnostics)?;
                            self.snapshot
                                .counters
                                .local_drops
                                .add(
                                    crate::diagnostics::DropReason::UnknownContext,
                                    direction,
                                    counts.unknown_context - old.unknown_context,
                                )
                                .map_err(|_| RuntimeError::Diagnostics)?;
                        }
                        Err(error) => {
                            self.close_session(id, error.code().unwrap_or(masque::H3_MESSAGE_ERROR))
                        }
                    }
                }
                Err(quiche::h3::Error::Done) => {
                    if let Some(s) = self.sessions.get_mut(&id) {
                        s.body_pending = false;
                    }
                }
                Err(_) => self.close_session(id, masque::H3_MESSAGE_ERROR),
            }
        }
        let controls_pending = self.service_session_controls(connection)?;
        let mut scratch = resource(self.pools.scratch.acquire())?;
        for _ in 0..32 {
            let expiry = self
                .connections
                .get(&connection)
                .and_then(|c| c.transport.incoming_expiry())
                .unwrap_or(now + Duration::from_millis(self.cfg.limits().application_residence_ms));
            let result = self
                .connections
                .get_mut(&connection)
                .ok_or(RuntimeError::Transport)?
                .transport
                .quic
                .dgram_recv(scratch.bytes_mut());
            match result {
                Ok(n) => {
                    self.connections
                        .get_mut(&connection)
                        .ok_or(RuntimeError::Transport)?
                        .transport
                        .incoming_consumed();
                    self.datagram(connection, &scratch.bytes()[..n], expiry, Instant::now())?;
                    self.snapshot.profile.received_datagram_copy_bytes = self
                        .snapshot
                        .profile
                        .received_datagram_copy_bytes
                        .checked_add(n as u64)
                        .ok_or(RuntimeError::Diagnostics)?;
                }
                Err(quiche::Error::Done) => break,
                Err(_) => return Err(RuntimeError::Transport),
            }
        }
        // Outgoing application queues belong to the enqueue stage. Its staged
        // flags carry real progress across owner windows; merely retaining a
        // packet behind full ciphertext output must not force another turn.
        Ok(self
            .connections
            .get(&connection)
            .is_some_and(|c| c.transport.quic.dgram_recv_queue_len() > 0)
            || self
                .sessions
                .values()
                .any(|s| s.connection.0 == connection && s.body_pending)
            || setup_progress == 4
            || serviced == 4
            || controls_pending
            || (!ready && self.negotiate_transport(connection)?))
    }
    fn datagram(
        &mut self,
        connection: u64,
        bytes: &[u8],
        expiry: Instant,
        now: Instant,
    ) -> Result<(), RuntimeError> {
        let direction = if self.server {
            Direction::ToTarget
        } else {
            Direction::ToClient
        };
        if let Ok((quarter, width)) = masque::get_varint(bytes)
            && let Some(stream) = quarter
                .checked_mul(4)
                .filter(|id| *id <= masque::MAX_VARINT)
            && let Ok((0, context)) = masque::get_varint(&bytes[width..])
            && bytes.len() - width - context > 65527
        {
            if let Some(id) = self.lookup(connection, stream) {
                let session_id = self.sessions.get(&id).map(|s| s.id);
                self.failure(
                    Failure::Size,
                    1,
                    session_id,
                    self.connections.get(&connection).map(|c| c.id),
                );
                self.close_session(id, masque::H3_MESSAGE_ERROR);
            }
            return Ok(());
        }
        let decoded = match masque::decode_datagram(bytes) {
            Ok(Some(d)) => d,
            Ok(None) => {
                self.drop_packet(crate::diagnostics::DropReason::Malformed, direction)?;
                return Ok(());
            }
            Err(_) => {
                let c = self
                    .connections
                    .get_mut(&connection)
                    .ok_or(RuntimeError::Transport)?;
                let _ = c.transport.quic.close(true, masque::H3_DATAGRAM_ERROR, b"");
                return Ok(());
            }
        };
        if decoded.context != 0 {
            self.drop_packet(crate::diagnostics::DropReason::UnknownContext, direction)?;
            return Ok(());
        }
        if expiry <= now {
            self.drop_packet(crate::diagnostics::DropReason::QueueExpired, direction)?;
            return Ok(());
        }
        let Some(id) = self.lookup(connection, decoded.stream) else {
            self.drop_packet(crate::diagnostics::DropReason::UnknownFlow, direction)?;
            return Ok(());
        };
        let s = self.sessions.get_mut(&id).ok_or(RuntimeError::Transport)?;
        if s.state != State::Active {
            self.drop_packet(crate::diagnostics::DropReason::NotActive, direction)?;
            return Ok(());
        }
        if decoded.payload.len() > self.cfg.transport().application_max_bytes {
            let session_id = s.id;
            self.failure(
                Failure::Size,
                1,
                Some(session_id),
                self.connections.get(&connection).map(|c| c.id),
            );
            self.drop_packet(crate::diagnostics::DropReason::Oversized, direction)?;
            return Ok(());
        }
        s.udp_activity(now);
        let result = if self.server {
            let relay = s.relay.as_ref().ok_or(RuntimeError::Socket)?;
            let mut socket = relay.get_ref().0.borrow_mut();
            let from = socket.local_addr();
            socket.send_batch(&[SendPacket {
                bytes: decoded.payload,
                from,
                to: s.relay_peer.ok_or(RuntimeError::Socket)?,
            }])
        } else {
            let listener = self.listener.as_ref().ok_or(RuntimeError::Socket)?;
            let mut socket = listener.get_ref().0.borrow_mut();
            let from = s.sender_local.ok_or(RuntimeError::Socket)?;
            socket.send_batch(&[SendPacket {
                bytes: decoded.payload,
                from,
                to: s.sender.ok_or(RuntimeError::Socket)?,
            }])
        };
        self.complete_datagram_send(id, decoded.payload, expiry, now, result)
    }
    // A single outcome path keeps real UDP submissions and deterministic failure
    // tests on the same bounded queue/correlation logic; it never resubmits data.
    fn complete_datagram_send(
        &mut self,
        id: u64,
        payload: &[u8],
        expiry: Instant,
        now: Instant,
        result: io::Result<usize>,
    ) -> Result<(), RuntimeError> {
        let direction = if self.server {
            Direction::ToTarget
        } else {
            Direction::ToClient
        };
        let s = self.sessions.get_mut(&id).ok_or(RuntimeError::Transport)?;
        let connection = s.connection.0;
        match result {
            Ok(1) => {
                s.udp_activity(now);
                self.snapshot
                    .counters
                    .egress(direction, payload.len())
                    .map_err(|_| RuntimeError::Diagnostics)?;
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                let session = s.generation;
                let conn = s.connection;
                let mut slab = match self.pools.application.acquire() {
                    Ok(s) => s,
                    Err(_) => {
                        let session_id = s.id;
                        self.failure(
                            Failure::Resource,
                            1,
                            Some(session_id),
                            self.connections.get(&connection).map(|c| c.id),
                        );
                        self.drop_packet(crate::diagnostics::DropReason::QueueFull, direction)?;
                        return Ok(());
                    }
                };
                slab.bytes_mut()[..payload.len()].copy_from_slice(payload);
                self.snapshot.profile.queue_copy_bytes = self
                    .snapshot
                    .profile
                    .queue_copy_bytes
                    .checked_add(payload.len() as u64)
                    .ok_or(RuntimeError::Diagnostics)?;
                let packet = resource(crate::buffers::Datagram::new(
                    slab,
                    0,
                    payload.len(),
                    direction,
                    session,
                    conn,
                    expiry - Duration::from_millis(self.cfg.limits().application_residence_ms),
                    expiry,
                ))?;
                if self
                    .sessions
                    .get_mut(&id)
                    .ok_or(RuntimeError::Transport)?
                    .queue
                    .push(packet)
                    .is_err()
                {
                    let session_id = self.sessions.get(&id).map(|s| s.id);
                    self.failure(
                        Failure::Resource,
                        2,
                        session_id,
                        self.connections.get(&connection).map(|c| c.id),
                    );
                    self.drop_packet(crate::diagnostics::DropReason::QueueFull, direction)?;
                }
            }
            Err(e) if e.raw_os_error() == Some(libc::EMSGSIZE) => {
                self.drop_packet(crate::diagnostics::DropReason::Pmtu, direction)?;
            }
            Err(e) if e.raw_os_error() == Some(libc::ENOBUFS) => {
                // Kernel egress pressure drops this unreliable packet, not the
                // immutable association. No accepted payload is replayed.
                self.drop_packet(crate::diagnostics::DropReason::SocketError, direction)?;
            }
            Err(e) if crate::udp::is_peer_error(&e) => {
                // A delayed ICMP describes an earlier submission, not loss of
                // this immutable association. This call accepted no payload.
                let sid = s.id;
                self.failure(
                    Failure::Socket,
                    e.raw_os_error().unwrap_or(0) as u64,
                    Some(sid),
                    self.connections.get(&connection).map(|c| c.id),
                );
                self.drop_packet(crate::diagnostics::DropReason::SocketError, direction)?;
            }
            Err(_) => {
                let sid = s.id;
                self.failure(
                    Failure::Socket,
                    0,
                    Some(sid),
                    self.connections.get(&connection).map(|c| c.id),
                );
                self.close_session(id, 0x10f);
            }
            _ => {}
        }
        Ok(())
    }
    fn flush_relay(&mut self, id: u64, now: Instant) -> Result<(), RuntimeError> {
        let direction = if self.server {
            Direction::ToTarget
        } else {
            Direction::ToClient
        };
        for _ in 0..4 {
            let Some(s) = self.sessions.get_mut(&id) else {
                return Ok(());
            };
            let Some(packet) = s.queue.front(direction) else {
                break;
            };
            if packet.expires_at() <= now {
                s.queue.pop(direction);
                self.drop_packet(crate::diagnostics::DropReason::QueueExpired, direction)?;
                continue;
            }
            let socket = if self.server {
                s.relay.as_ref()
            } else {
                self.listener.as_ref()
            }
            .ok_or(RuntimeError::Socket)?;
            let to =
                if self.server { s.relay_peer } else { s.sender }.ok_or(RuntimeError::Socket)?;
            let result = {
                let mut udp = socket.get_ref().0.borrow_mut();
                let from = if self.server {
                    udp.local_addr()
                } else {
                    s.sender_local.ok_or(RuntimeError::Socket)?
                };
                udp.send_batch(&[SendPacket {
                    bytes: packet.bytes(),
                    from,
                    to,
                }])
            };
            match result {
                Ok(1) => {
                    let len = packet.bytes().len();
                    s.queue.pop(direction);
                    s.udp_activity(now);
                    self.snapshot
                        .counters
                        .egress(direction, len)
                        .map_err(|_| RuntimeError::Diagnostics)?;
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    let _ = socket.try_io(tokio::io::Interest::WRITABLE, |_| {
                        Err::<(), _>(io::ErrorKind::WouldBlock.into())
                    });
                    break;
                }
                Err(e) if e.raw_os_error() == Some(libc::EMSGSIZE) => {
                    s.queue.pop(direction);
                    self.drop_packet(crate::diagnostics::DropReason::Pmtu, direction)?;
                }
                Err(e) if e.raw_os_error() == Some(libc::ENOBUFS) => {
                    s.queue.pop(direction);
                    self.drop_packet(crate::diagnostics::DropReason::SocketError, direction)?;
                }
                Err(e) if crate::udp::is_peer_error(&e) => {
                    let sid = s.id;
                    let conn = s.connection.0;
                    s.queue.pop(direction);
                    self.failure(
                        Failure::Socket,
                        e.raw_os_error().unwrap_or(0) as u64,
                        Some(sid),
                        self.connections.get(&conn).map(|c| c.id),
                    );
                    self.drop_packet(crate::diagnostics::DropReason::SocketError, direction)?;
                }
                _ => {
                    let sid = s.id;
                    let conn = s.connection.0;
                    self.failure(
                        Failure::Socket,
                        0,
                        Some(sid),
                        self.connections.get(&conn).map(|c| c.id),
                    );
                    self.close_session(id, 0x10f);
                    break;
                }
            }
        }
        Ok(())
    }
    fn relay_input(&mut self, id: u64, now: Instant) -> Result<bool, RuntimeError> {
        // The visit list may predate a FIN/RESET/GOAWAY cancellation in the
        // same bounded turn. A stale visit cannot resurrect or fail a new flow.
        if !self.sessions.contains_key(&id) {
            return Ok(false);
        }
        if self
            .sessions
            .get(&id)
            .and_then(|s| s.relay.as_ref())
            .is_none_or(|relay| !relay.get_ref().read_pending())
        {
            return Ok(false);
        }
        let error_info = self.sessions.get(&id).and_then(|s| {
            s.relay.as_ref().map(|relay| {
                (
                    relay.get_ref().0.borrow_mut().drain_errors(),
                    s.id,
                    s.connection.0,
                )
            })
        });
        let mut error_pending = false;
        if let Some((errors, sid, connection)) = error_info {
            let errors = errors.map_err(|_| RuntimeError::Socket)?;
            error_pending = errors.pending;
            for error in errors.errors.into_iter().flatten() {
                if error.mtu.is_some() {
                    self.snapshot.counters.destination_pmtu_updates = self
                        .snapshot
                        .counters
                        .destination_pmtu_updates
                        .checked_add(1)
                        .ok_or(RuntimeError::Diagnostics)?;
                } else if error.errno != libc::EMSGSIZE as u32 {
                    self.failure(
                        Failure::Socket,
                        u64::from(error.errno),
                        Some(sid),
                        self.connections.get(&connection).map(|c| c.id),
                    );
                }
            }
            self.snapshot
                .counters
                .local_drops
                .add(
                    crate::diagnostics::DropReason::AncillaryInvalid,
                    Direction::ToTarget,
                    errors.malformed as u64,
                )
                .map_err(|_| RuntimeError::Diagnostics)?;
        }
        let Some(s) = self.sessions.get(&id) else {
            return Ok(false);
        };
        if s.state != State::Active {
            return Ok(false);
        }
        let Some(relay) = s.relay.as_ref() else {
            return Ok(false);
        };
        let connection = s.connection.0;
        let mut slabs = Vec::with_capacity(self.cfg.io().batch_max.min(28));
        for _ in 0..self.cfg.io().batch_max.min(28) {
            if let Ok(s) = self.pools.application.acquire() {
                slabs.push(s);
            } else {
                break;
            }
        }
        if slabs.is_empty() {
            return Ok(false);
        }
        let mut packets = Vec::with_capacity(slabs.len());
        let result = read_io(relay, |inner| {
            inner.0.borrow_mut().recv_batch(&mut slabs, &mut packets)
        });
        let overflow = relay.get_ref().0.borrow_mut().take_overflow();
        self.snapshot.counters.kernel_rx_overflow = self
            .snapshot
            .counters
            .kernel_rx_overflow
            .checked_add(overflow)
            .ok_or(RuntimeError::Diagnostics)?;
        match result {
            Ok(n) => {
                for p in packets {
                    if let Some(fault) = p.fault {
                        let session_id = self.sessions.get(&id).map(|s| s.id);
                        self.failure(
                            if fault == ReceiveFault::Truncated {
                                Failure::Size
                            } else {
                                Failure::Malformed
                            },
                            3,
                            session_id,
                            self.connections.get(&connection).map(|c| c.id),
                        );
                        self.drop_packet(
                            if fault == ReceiveFault::Truncated {
                                crate::diagnostics::DropReason::SocketTruncated
                            } else {
                                crate::diagnostics::DropReason::AncillaryInvalid
                            },
                            Direction::ToClient,
                        )?;
                        continue;
                    }
                    self.snapshot
                        .counters
                        .ingress(Direction::ToClient, p.length)
                        .map_err(|_| RuntimeError::Diagnostics)?;
                    self.queue_outgoing(id, &slabs[p.index].bytes()[..p.length], now)?;
                }
                Ok(n > 0 || error_pending)
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                relay.get_ref().set_read_pending(error_pending);
                Ok(error_pending)
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => Ok(true),
            Err(e) if crate::udp::is_peer_error(&e) => {
                let session_id = self.sessions.get(&id).map(|s| s.id);
                self.failure(
                    Failure::Socket,
                    e.raw_os_error().unwrap_or(0) as u64,
                    session_id,
                    self.connections.get(&connection).map(|c| c.id),
                );
                Ok(true)
            }
            Err(_) => {
                let session_id = self.sessions.get(&id).map(|s| s.id);
                self.failure(
                    Failure::Socket,
                    0,
                    session_id,
                    self.connections.get(&connection).map(|c| c.id),
                );
                self.close_session(id, 0x10f);
                Ok(false)
            }
        }
    }
    fn service_output(&mut self, connection: u64, now: Instant) -> Result<bool, OutputError> {
        let ready = self
            .connections
            .get(&connection)
            .ok_or(OutputError::Socket(RuntimeError::Transport))?
            .transport
            .output
            .front()
            .is_some_and(|p| p.at() <= now);
        let mut remaining = OUTPUT_QUOTA;
        let mut more = false;
        if ready {
            // A zero-generation preparation preserves expiry, setup and PMTU
            // checks before any committed packet is physically submitted.
            self.connections
                .get_mut(&connection)
                .ok_or(OutputError::Socket(RuntimeError::Transport))?
                .transport
                .generate(&self.pools.ciphertext, now, 0)
                .map_err(OutputError::Transport)?;
            let offered = self.connections[&connection]
                .transport
                .output
                .iter()
                .take(remaining)
                .take_while(|p| p.at() <= now)
                .count();
            more |= self
                .flush_limit(connection, now, remaining)
                .map_err(OutputError::Socket)?;
            // Spend the descriptors offered, including unaccepted suffixes.
            // Partial sends/errors cannot enlarge this visit's send budget.
            remaining -= offered;
        }
        let generated = self
            .connections
            .get_mut(&connection)
            .ok_or(OutputError::Socket(RuntimeError::Transport))?
            .generate_output(&self.pools.ciphertext, Instant::now())
            .map_err(OutputError::Transport)?;
        if remaining != 0 {
            more |= self
                .flush_limit(connection, Instant::now(), remaining)
                .map_err(OutputError::Socket)?;
        }
        Ok(more || generated == OUTPUT_QUOTA)
    }
    #[cfg(test)]
    fn flush(&mut self, connection: u64, now: Instant) -> Result<bool, RuntimeError> {
        self.flush_limit(connection, now, OUTPUT_QUOTA)
    }
    fn flush_limit(
        &mut self,
        connection: u64,
        now: Instant,
        limit: usize,
    ) -> Result<bool, RuntimeError> {
        let c = self
            .connections
            .get_mut(&connection)
            .ok_or(RuntimeError::Transport)?;
        let mut socket = self.outer[c.socket].get_ref().0.borrow_mut();
        let packets: Vec<_> = c
            .transport
            .output
            .iter()
            .take(limit)
            .take_while(|p| p.at() <= now)
            .map(|p| SendPacket {
                bytes: p.bytes(),
                from: p.from(),
                to: p.to(),
            })
            .collect();
        if packets.is_empty() {
            return Ok(false);
        }
        let count = packets.len();
        let lengths: Vec<_> = packets.iter().map(|p| p.bytes.len()).collect();
        #[cfg(test)]
        let result = if let Some(send) = &mut self.outer_send_hook {
            send(&packets)
        } else {
            socket.send_batch(&packets)
        };
        #[cfg(not(test))]
        let result = socket.send_batch(&packets);
        drop(packets);
        match result {
            Ok(n) => {
                self.snapshot.counters.outer_socket_sent = self
                    .snapshot
                    .counters
                    .outer_socket_sent
                    .checked_add(n as u64)
                    .ok_or(RuntimeError::Diagnostics)?;
                self.snapshot.profile.send_burst_datagrams_max =
                    self.snapshot.profile.send_burst_datagrams_max.max(n);
                if n != 0 {
                    self.snapshot
                        .histograms
                        .emitted_burst_datagrams
                        .observe(n as u64);
                }
                for packet in c.transport.output.iter().take(n) {
                    self.snapshot.histograms.ciphertext_age_us.observe(
                        now.saturating_duration_since(packet.generated_at())
                            .as_micros() as u64,
                    );
                    self.snapshot
                        .histograms
                        .pacing_lateness_us
                        .observe(now.saturating_duration_since(packet.at()).as_micros() as u64);
                    self.snapshot.profile.ciphertext_age_max_us =
                        self.snapshot.profile.ciphertext_age_max_us.max(
                            now.saturating_duration_since(packet.generated_at())
                                .as_micros() as u64,
                        );
                    self.snapshot.profile.pacing_lateness_max_us = self
                        .snapshot
                        .profile
                        .pacing_lateness_max_us
                        .max(now.saturating_duration_since(packet.at()).as_micros() as u64);
                }
                for length in lengths.iter().take(n) {
                    self.snapshot.counters.outer_socket_sent_bytes = self
                        .snapshot
                        .counters
                        .outer_socket_sent_bytes
                        .checked_add(*length as u64)
                        .ok_or(RuntimeError::Diagnostics)?;
                }
                for _ in 0..n {
                    c.transport.output.pop_front();
                }
                Ok(n == count)
            }
            // The adapter bounds EINTR retries. A remaining interrupted call
            // consumed nothing: continue in another bounded owner turn.
            Err(e) if e.kind() == io::ErrorKind::Interrupted => Ok(true),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                let _ = self.outer[c.socket].try_io(tokio::io::Interest::WRITABLE, |_| {
                    Err::<(), _>(io::ErrorKind::WouldBlock.into())
                });
                Ok(false)
            }
            Err(e) if e.raw_os_error() == Some(libc::EMSGSIZE) => {
                // Exact failed FIFO front only; no accepted-prefix replay and
                // no invented per-Session attribution for encrypted packets.
                c.transport.output.pop_front();
                // EMSGSIZE can also be an old shared-socket error. Only the
                // checked error-queue destination may revalidate a QUIC path.
                self.snapshot.counters.outer_pmtu_errors = self
                    .snapshot
                    .counters
                    .outer_pmtu_errors
                    .checked_add(1)
                    .ok_or(RuntimeError::Diagnostics)?;
                Ok(true)
            }
            Err(e) if e.raw_os_error() == Some(libc::ENOBUFS) => {
                // Native netem/qdisc pressure can reject a send synchronously.
                // None of this FIFO front was accepted. Let QUIC observe normal
                // unreliable loss and recover its control frames; keep peers
                // and unaffected Sessions alive, with bounded owner work.
                c.transport.output.pop_front();
                self.snapshot.counters.outer_send_pressure = self
                    .snapshot
                    .counters
                    .outer_send_pressure
                    .checked_add(1)
                    .ok_or(RuntimeError::Diagnostics)?;
                Ok(true)
            }
            Err(e) if crate::udp::is_peer_error(&e) => {
                // Shared outer sockets can report an ICMP for destination A
                // while submitting B. No bytes were accepted; discard only
                // this FIFO front, without attributing peer/Session failure.
                // The bounded error-queue path alone has checked destination
                // metadata. Remaining records are serviced on later turns.
                c.transport.output.pop_front();
                self.snapshot.counters.outer_async_errors = self
                    .snapshot
                    .counters
                    .outer_async_errors
                    .checked_add(1)
                    .ok_or(RuntimeError::Diagnostics)?;
                Ok(true)
            }
            Err(e) => Err(e
                .raw_os_error()
                .map_or(RuntimeError::Socket, RuntimeError::SocketErrno)),
        }
    }
    fn service_transport_timers(&mut self, now: Instant) -> Result<bool, RuntimeError> {
        let mut more = false;
        let mut expired_native = 0usize;
        for c in self.connections.values_mut() {
            expired_native += c.transport.expire_native(now);
            let lost = c
                .transport
                .take_lost_packets()
                .map_err(|_| RuntimeError::Transport)?;
            self.snapshot.counters.outer_quic_lost_packets = self
                .snapshot
                .counters
                .outer_quic_lost_packets
                .checked_add(lost as u64)
                .ok_or(RuntimeError::Diagnostics)?;
            let pmtu = c.transport.take_pmtu_drops();
            self.snapshot
                .counters
                .local_drops
                .add(
                    crate::diagnostics::DropReason::Pmtu,
                    if self.server {
                        Direction::ToClient
                    } else {
                        Direction::ToTarget
                    },
                    pmtu as u64,
                )
                .map_err(|_| RuntimeError::Diagnostics)?;
        }
        self.snapshot
            .counters
            .local_drops
            .add(
                crate::diagnostics::DropReason::QueueExpired,
                if self.server {
                    Direction::ToClient
                } else {
                    Direction::ToTarget
                },
                expired_native as u64,
            )
            .map_err(|_| RuntimeError::Diagnostics)?;
        for c in self.connections.values_mut() {
            if c.transport
                .quic
                .timeout_instant()
                .is_some_and(|at| at <= now)
            {
                c.transport.quic.on_timeout();
            }
            more |= c.transport.drain_path_events();
        }
        let connections: Vec<_> = self.connections.keys().copied().collect();
        for id in connections {
            let active = self
                .sessions
                .values()
                .any(|s| s.connection.0 == id && s.state == State::Active);
            let action = self
                .connections
                .get_mut(&id)
                .unwrap()
                .transport
                .service_liveness(now, active);
            match action {
                Ok(transport::LivenessAction::None) => {}
                Ok(transport::LivenessAction::Probe) => {
                    self.snapshot.recovery.keepalive_probes = self
                        .snapshot
                        .recovery
                        .keepalive_probes
                        .checked_add(1)
                        .ok_or(RuntimeError::Diagnostics)?;
                    more = true;
                }
                Ok(transport::LivenessAction::Expired) => {
                    self.snapshot.recovery.watchdog_timeouts = self
                        .snapshot
                        .recovery
                        .watchdog_timeouts
                        .checked_add(1)
                        .ok_or(RuntimeError::Diagnostics)?;
                    self.failure(
                        Failure::PeerTimeout,
                        1,
                        None,
                        self.connections.get(&id).map(|c| c.id),
                    );
                    self.stop_connection(id)?;
                    more = true;
                }
                Err(_) => {
                    self.failure(
                        Failure::Transport,
                        0,
                        None,
                        self.connections.get(&id).map(|c| c.id),
                    );
                    self.stop_connection(id)?;
                    more = true;
                }
            }
        }
        Ok(more)
    }
    /// Each class selects at most four sessions, including under source churn.
    /// Enumeration is bounded by configured counts, not an instruction budget.
    fn service_session_controls(&mut self, connection: u64) -> Result<bool, RuntimeError> {
        let mut more = false;
        if self.server {
            let ids: Vec<_> = self
                .sessions
                .iter()
                .filter_map(|(id, s)| {
                    (s.connection.0 == connection && s.state == State::AwaitingCommitment)
                        .then_some(*id)
                })
                .collect();
            let remaining = ids.len() > 4;
            let mut activated = 0;
            for id in self.select_sessions(connection, 4, ids, 4) {
                let s = self.sessions.get_mut(&id).ok_or(RuntimeError::Transport)?;
                if self.connections.get_mut(&connection).is_some_and(|c| {
                    !c.transport.quic.stream_closed(s.stream)
                        && c.transport.quic.stream_capacity(s.stream).is_ok()
                }) && s.receipt.as_ref().is_some_and(|r| r.strong_count() == 0)
                {
                    s.receipt = None;
                    s.state = State::Active;
                    self.snapshot.lifecycle.accepted_total = self
                        .snapshot
                        .lifecycle
                        .accepted_total
                        .checked_add(1)
                        .ok_or(RuntimeError::Diagnostics)?;
                    s._setup = None;
                    activated += 1;
                }
            }
            more |= remaining && activated == 4;
        }
        let ids: Vec<_> = self
            .sessions
            .iter()
            .filter_map(|(id, s)| {
                (s.connection.0 == connection && s.state == State::Active).then_some(*id)
            })
            .collect();
        let remaining = ids.len() > 4;
        let mut retired = 0;
        for id in self.select_sessions(connection, 5, ids, 4) {
            let stream = self.sessions[&id].stream;
            let c = self
                .connections
                .get_mut(&connection)
                .ok_or(RuntimeError::Transport)?;
            if c.transport.quic.stream_capacity(stream).is_err() {
                self.close_session(id, 0x10c);
                retired += 1;
            }
        }
        // Continue only after a full quota made progress with candidates left.
        // ACK-pending receipts and ordinary live capacity never busy-spin.
        Ok(more || (remaining && retired == 4))
    }
    fn reclaim_control(&mut self, connection: u64) -> bool {
        let Some(c) = self.connections.get_mut(&connection) else {
            return false;
        };
        let previously_failed = c.transport.critical_failed();
        let (pending, stopped) = c.transport.reap_stopped_streams();
        if !previously_failed && c.transport.critical_failed() {
            let connection_id = c.id;
            self.failure(Failure::Transport, 0x104, None, Some(connection_id));
            let ids: Vec<_> = self
                .sessions
                .iter()
                .filter_map(|(id, s)| (s.connection.0 == connection).then_some(*id))
                .collect();
            for id in ids {
                self.close_session(id, 0x104);
            }
        }
        for (stream, code) in stopped.into_iter().flatten() {
            if let Some(id) = self.lookup(connection, stream) {
                self.close_session(id, code);
            }
        }
        pending
    }
    fn sample_quic(&mut self) {
        let mut total = crate::diagnostics::QuicSummary::default();
        for c in self.connections.values() {
            let v = c.transport.operational_stats();
            total.sampled_connections += v.sampled_connections;
            total.rtt_max_us = total.rtt_max_us.max(v.rtt_max_us);
            total.rttvar_max_us = total.rttvar_max_us.max(v.rttvar_max_us);
            if v.path_payload_limit_min != 0 {
                total.path_payload_limit_min = if total.path_payload_limit_min == 0 {
                    v.path_payload_limit_min
                } else {
                    total.path_payload_limit_min.min(v.path_payload_limit_min)
                };
            }
            for (slot, n) in [
                (&mut total.cwnd_total_bytes, v.cwnd_total_bytes),
                (&mut total.retransmitted_packets, v.retransmitted_packets),
                (&mut total.pto_count, v.pto_count),
            ] {
                if let Some(next) = slot.checked_add(n) {
                    *slot = next;
                } else {
                    total.overflow = true;
                }
            }
            total.overflow |= v.overflow;
        }
        self.snapshot.quic = total;
    }
    fn record_resources(&mut self) {
        let mut io = self.retired_io;
        for socket in self.outer.iter().chain(self.listener.iter()) {
            io = io.merge(socket.get_ref().0.borrow().stats());
        }
        for s in self.sessions.values() {
            if let Some(socket) = &s.relay {
                io = io.merge(socket.get_ref().0.borrow().stats());
            }
        }
        self.snapshot.profile.io = io;
        self.snapshot.profile.factory = transport::factory_profile();
        if let Some(allocations) = self
            .pools
            .application
            .allocations()
            .checked_add(self.pools.ciphertext.allocations())
            .and_then(|n| n.checked_add(self.pools.scratch.allocations()))
        {
            self.snapshot.profile.payload_slab_allocations = allocations;
        } else {
            self.snapshot.profile.factory.overflowed = true;
        }
        use crate::diagnostics::Resources;
        let mut r = Resources {
            connections: self.budgets.connections.used().count,
            sessions: self.budgets.sessions.used().count,
            setup: self.budgets.setup.used().count,
            relay_fds: self.budgets.relay_fds.used().count,
            fd_reservations: self.budgets.fds.used().count,
            connections_peak: self.budgets.connections.peak().count,
            sessions_peak: self.budgets.sessions.peak().count,
            setup_peak: self.budgets.setup.peak().count,
            relay_fds_peak: self.budgets.relay_fds.peak().count,
            application_retained_bytes: self.pools.application.retained().bytes,
            ciphertext_retained_bytes: self.pools.ciphertext.retained().bytes,
            scratch_retained_bytes: self.pools.scratch.retained().bytes,
            pool_metadata_bytes: self.pools.application.metadata_capacity_bytes()
                + self.pools.ciphertext.metadata_capacity_bytes()
                + self.pools.scratch.metadata_capacity_bytes(),
            ..Resources::default()
        };
        for s in self.sessions.values() {
            r.queued_packets += s.queue.used().count;
            r.queued_bytes += s.queue.used().bytes;
            r.queue_metadata_bytes += s.queue.metadata_capacity_bytes();
            if let Some((headers, _)) = &s.deferred_headers {
                use quiche::h3::NameValue;
                r.control_output_capacity_bytes += headers.capacity()
                    * std::mem::size_of::<quiche::h3::Header>()
                    + headers
                        .iter()
                        .map(|h| h.name().len() + h.value().len())
                        .sum::<usize>();
            }
        }
        for c in self.connections.values() {
            let (send, receive, metadata) = c.transport.resource_usage();
            r.native_send_bytes += send;
            r.native_receive_bytes += receive;
            r.control_output_capacity_bytes += metadata;
        }
        let old = self.snapshot.resources;
        r.fd_reservations_peak = self.budgets.fds.peak().count;
        r.queued_packets_observed_peak = old.queued_packets_observed_peak.max(r.queued_packets);
        r.queued_bytes_observed_peak = old.queued_bytes_observed_peak.max(r.queued_bytes);
        r.session_queue_packets_peak = old.session_queue_packets_peak.max(
            self.sessions
                .values()
                .map(|s| s.queue.peak().count)
                .max()
                .unwrap_or(0),
        );
        r.session_queue_bytes_peak = old.session_queue_bytes_peak.max(
            self.sessions
                .values()
                .map(|s| s.queue.peak().bytes)
                .max()
                .unwrap_or(0),
        );
        r.application_retained_peak = old
            .application_retained_peak
            .max(self.pools.application.retained_peak().bytes);
        r.ciphertext_retained_peak = old
            .ciphertext_retained_peak
            .max(self.pools.ciphertext.retained_peak().bytes);
        r.scratch_retained_peak = old
            .scratch_retained_peak
            .max(self.pools.scratch.retained_peak().bytes);
        self.snapshot.resources = r;
        let mut lifecycle = crate::diagnostics::Lifecycle {
            accepted_total: self.snapshot.lifecycle.accepted_total,
            destination_response_datagrams: if self.server {
                self.snapshot.counters.app_ingress_by_direction.to_client
            } else {
                0
            },
            ..Default::default()
        };
        for s in self.sessions.values() {
            match s.state {
                State::Starting => lifecycle.starting += 1,
                State::Authorizing => lifecycle.authorizing += 1,
                State::AwaitingAcceptance => lifecycle.awaiting_acceptance += 1,
                State::AwaitingCommitment => lifecycle.awaiting_commitment += 1,
                State::Active => lifecycle.active += 1,
                State::Recovering => lifecycle.recovering += 1,
                State::Closed => {}
            }
        }
        self.snapshot.lifecycle = lifecycle;
        self.snapshot.qlog = self.qlog.as_ref().map_or_default(|p| p.report());
    }
    fn expire_sessions(&mut self, now: Instant) {
        let expired: Vec<_> = self
            .sessions
            .iter()
            .filter_map(|(id, s)| {
                (s.deadline(
                    Duration::from_millis(self.cfg.limits().idle_session_ms),
                    Duration::from_millis(self.cfg.recovery().setup_timeout_ms),
                ) <= now)
                    .then_some(*id)
            })
            .collect();
        for id in expired {
            if self
                .sessions
                .get(&id)
                .is_some_and(|s| s.state != State::Active)
            {
                self.failure(
                    Failure::SetupTimeout,
                    0,
                    self.sessions.get(&id).map(|s| s.id),
                    self.sessions
                        .get(&id)
                        .and_then(|s| self.connections.get(&s.connection.0))
                        .map(|c| c.id),
                );
            }
            self.close_session(id, 0x100);
        }
    }
    fn stop_owned(&mut self, now: Instant) -> Result<(), RuntimeError> {
        self.stopping = true;
        self.stop_started.get_or_insert(now);
        self.reconnect.reset();
        self.setup_retry.reset();
        if !self.server {
            self.client_state(ClientState::Stopping, now)?;
        }
        self.pending.clear();
        let ids: Vec<_> = self.sessions.keys().copied().collect();
        for id in ids {
            self.close_session(id, 0x100);
        }
        for c in self.connections.values_mut() {
            c.transport.stop();
        }
        Ok(())
    }
    fn owner_visits(&self) -> Vec<(u8, u64)> {
        let mut visits: Vec<(u8, u64)> = (0..self.outer.len()).map(|i| (0, i as u64)).collect();
        if self.listener.is_some() {
            visits.push((1, 0));
        }
        for &id in self.connections.keys() {
            visits.extend([(2, id), (4, id), (5, id)]);
        }
        visits.extend(
            self.sessions
                .iter()
                // Keep active-session membership stable across packet
                // arrivals/drains: the owner window cursor is positional.
                // Readiness gating makes an empty visit cheap.
                .filter(|(_, s)| s.state == State::Active)
                .map(|(id, _)| (3, *id)),
        );
        // Release available native output capacity and service admitted queues
        // before reading another producer batch. Otherwise a full application
        // queue can drop fresh input even when this same turn can drain it.
        // Each stage keeps its original quota and stable ring membership.
        visits.sort_unstable_by_key(|&(kind, id)| {
            let priority = match kind {
                0 => 0, // outer input may return transport credit
                2 => 1, // process control/close before committing output
                5 => 2, // generate/submit the previous native batch
                4 => 3, // make room in application queues
                1 => 4, // client listener
                _ => 5, // server relay producers
            };
            (priority, id)
        });
        visits
    }
    fn packet_continuation(&self) -> bool {
        self.connections
            .values()
            .any(|c| c.enqueue_pending || c.output_pending)
    }
    /// Poll physical readiness independently of staged continuation. A retained
    /// application queue alone cannot wake an owner blocked on ciphertext output.
    fn poll_io(&self, cx: &mut std::task::Context<'_>) -> bool {
        let mut ready = false;
        if poll_timer(&self.timer, cx).is_ready() {
            ready = true;
        }
        for socket in &self.outer {
            if socket.poll_read_ready(cx).is_ready() {
                socket.get_ref().set_read_pending(true);
                ready = true;
            }
        }
        if let Some(socket) = &self.listener {
            if socket.poll_read_ready(cx).is_ready() {
                socket.get_ref().set_read_pending(true);
                ready = true;
            }
            if self
                .sessions
                .values()
                .any(|s| s.queue.front(Direction::ToClient).is_some())
                && socket.poll_write_ready(cx).is_ready()
            {
                ready = true;
            }
        }
        for s in self.sessions.values() {
            if s.state == State::Active
                && let Some(socket) = &s.relay
            {
                if socket.poll_read_ready(cx).is_ready() {
                    socket.get_ref().set_read_pending(true);
                    ready = true;
                }
                if s.queue.front(Direction::ToTarget).is_some()
                    && socket.poll_write_ready(cx).is_ready()
                {
                    ready = true;
                }
            }
        }
        for c in self.connections.values() {
            if c.transport
                .output
                .front()
                .is_some_and(|p| p.at() <= Instant::now())
                && self.outer[c.socket].poll_write_ready(cx).is_ready()
            {
                ready = true;
            }
        }
        ready
    }
    async fn run(
        &mut self,
        mut term: tokio::signal::unix::Signal,
        mut interrupt: tokio::signal::unix::Signal,
    ) -> Result<(), RuntimeError> {
        loop {
            let now = Instant::now();
            let stop = std::future::poll_fn(|cx| {
                Poll::Ready(term.poll_recv(cx).is_ready() || interrupt.poll_recv(cx).is_ready())
            })
            .await;
            if stop {
                self.stop_owned(now)?;
                return Ok(());
            }
            // Timers/cancellation are checked between bounded owner turns.
            let mut more = self.service_recovery(now)?;
            more |= self.service_transport_timers(now)?;
            // Keep one complete owner window of idle payload storage for
            // reuse. Startup/overload high-water caches must not pin all their
            // slabs for the connection lifetime; maintenance itself is bounded.
            self.pools.application.trim_cached(256, 32);
            // At most eight owner visits before timers/cancellation. Connection
            // work has per-class item quotas; metadata enumeration and internal
            // quiche work are not a fixed number of API calls/instructions.
            let visits = self.owner_visits();
            let (window, next, continuation) = service_window(visits.len(), self.turn);
            for &(kind, id) in &visits[window] {
                self.snapshot.profile.owner_visits = self
                    .snapshot
                    .profile
                    .owner_visits
                    .checked_add(1)
                    .ok_or(RuntimeError::Diagnostics)?;
                match kind {
                    0 => more |= self.receive_outer(id as usize, now)?,
                    1 => more |= self.client_input(now)?,
                    2 | 4 => {
                        if !self.connections.contains_key(&id) {
                            continue;
                        }
                        if kind == 2 {
                            more |= self.reclaim_control(id);
                        }
                        let result = if kind == 2 {
                            self.h3_work(id, now)
                        } else {
                            self.schedule_datagrams(id, now)
                        };
                        match result {
                            Ok(pending) => more |= pending,
                            Err(error) => {
                                self.failure(
                                    match error {
                                        RuntimeError::Capability => Failure::Capability,
                                        RuntimeError::Trust => Failure::Trust,
                                        _ => Failure::Transport,
                                    },
                                    match error {
                                        RuntimeError::TransportCode(code) => code,
                                        _ => 0,
                                    },
                                    None,
                                    self.connections.get(&id).map(|c| c.id),
                                );
                                self.stop_connection(id)?;
                                if !self.server
                                    && matches!(
                                        error,
                                        RuntimeError::Capability | RuntimeError::Trust
                                    )
                                {
                                    self.permanent_client_failure(
                                        Instant::now(),
                                        if error == RuntimeError::Trust {
                                            Failure::Trust
                                        } else {
                                            Failure::Capability
                                        },
                                        0,
                                    )?;
                                }
                                // HTTP refusal already latched the immutable mapping in
                                // client_headers. Preserve fatal initial rejection even
                                // when its propagated error is the generic Transport variant.
                                if !self.server && !self.ever_active && !self.latch.may_setup() {
                                    return Err(error);
                                }
                            }
                        }
                    }
                    5 => {
                        if !self.connections.contains_key(&id) {
                            continue;
                        }
                        if let Err(error) = self
                            .service_output(id, Instant::now())
                            .map(|pending| more |= pending)
                        {
                            let (class, code) = match error {
                                OutputError::Transport(error) => (
                                    Failure::Transport,
                                    match error {
                                        TransportError::Native(code) => code,
                                        TransportError::Expired => 1001,
                                        TransportError::Capacity => 1002,
                                        TransportError::Protocol => 1003,
                                        _ => 0,
                                    },
                                ),
                                OutputError::Socket(error) => (
                                    Failure::Socket,
                                    match error {
                                        RuntimeError::SocketErrno(code) => code as u64,
                                        _ => 0,
                                    },
                                ),
                            };
                            self.failure(
                                class,
                                code,
                                None,
                                self.connections.get(&id).map(|c| c.id),
                            );
                            self.stop_connection(id)?;
                        }
                    }
                    _ => {
                        self.flush_relay(id, now)?;
                        more |= self.relay_input(id, now)?;
                    }
                }
            }
            self.turn = next;
            more |= continuation;
            for c in self.connections.values_mut() {
                if c.transport.quic.is_established() {
                    c.initial_cid.clear();
                }
            }
            self.expire_sessions(Instant::now());
            let closed: Vec<_> = self
                .connections
                .iter()
                .filter_map(|(id, c)| {
                    (c.transport.quic.is_closed() || c.transport.is_stopped()).then_some(*id)
                })
                .collect();
            for id in closed {
                // Local failures already have their class recorded. Attribute a
                // timeout only when quiche reports one, before session cleanup.
                if let Some(c) = self.connections.get(&id)
                    && !c.transport.is_stopped()
                {
                    self.failure(
                        if c.transport.quic.is_timed_out() {
                            Failure::PeerTimeout
                        } else {
                            Failure::Transport
                        },
                        c.transport
                            .quic
                            .local_error()
                            .or_else(|| c.transport.quic.peer_error())
                            .map_or(0, |error| error.error_code),
                        None,
                        Some(c.id),
                    );
                }
                self.remove_connection(id, Instant::now())?;
            }
            if !self.server {
                let retired: Vec<_> = self
                    .connections
                    .iter()
                    .filter_map(|(id, c)| {
                        (c.transport.draining.is_some()
                            && c.transport.cleanup_count() == 0
                            && !self.sessions.values().any(|s| s.connection.0 == *id))
                        .then_some(*id)
                    })
                    .collect();
                for id in retired {
                    self.remove_connection(id, Instant::now())?;
                }
            }
            if now >= self.next_quic_sample {
                self.sample_quic();
                self.next_quic_sample = now + Duration::from_secs(5);
            }
            if now >= self.next_snapshot {
                self.record_resources();
                self.snapshot.sessions_active = self
                    .sessions
                    .values()
                    .filter(|s| s.state == State::Active)
                    .count() as u64;
                self.snapshot.connections_active = self
                    .connections
                    .values()
                    .filter(|c| c.transport.h3.is_some())
                    .count() as u64;
                let _ = self.diagnostics.snapshot(now, self.snapshot);
                self.next_snapshot =
                    now + Duration::from_millis(self.cfg.diagnostics().snapshot_interval_ms);
            }
            let deadline = self
                .connections
                .iter()
                .filter_map(|(id, c)| {
                    c.transport.deadline_for(
                        now,
                        self.sessions
                            .values()
                            .any(|s| s.connection.0 == *id && s.state == State::Active),
                    )
                })
                .chain(self.sessions.values().map(|s| {
                    s.deadline(
                        Duration::from_millis(self.cfg.limits().idle_session_ms),
                        Duration::from_millis(self.cfg.recovery().setup_timeout_ms),
                    )
                }))
                .chain(self.sessions.values().filter_map(|s| {
                    [Direction::ToTarget, Direction::ToClient]
                        .into_iter()
                        .filter_map(|d| s.queue.front(d).map(|p| p.expires_at()))
                        .min()
                }))
                .chain([self.next_snapshot, self.next_quic_sample])
                .chain(self.reconnect.deadline())
                .chain(self.setup_retry.deadline())
                .filter(|at| *at > now)
                .min();
            self.timer
                .get_ref()
                .arm(deadline)
                .map_err(|_| RuntimeError::Socket)?;
            // A stage may have queued work in an earlier eight-visit window.
            // Do not sleep at the ring boundary before its downstream visit.
            more |= self.packet_continuation();
            if more {
                tokio::task::yield_now().await;
            }
            let mut completions = Vec::new();
            let stop = std::future::poll_fn(|cx| {
                if term.poll_recv(cx).is_ready() || interrupt.poll_recv(cx).is_ready() {
                    return Poll::Ready(true);
                }
                let mut ready = more;
                // Register/poll a bounded setup slice after cancellation checks.
                // Continue until every future has a waker; then await readiness.
                let mut index = self.setup_cursor.min(self.pending.len());
                for _ in 0..32 {
                    if index >= self.pending.len() {
                        break;
                    }
                    match self.pending[index].future.as_mut().poll(cx) {
                        Poll::Ready(result) => {
                            let pending = self.pending.swap_remove(index);
                            completions.push((pending.session, pending.key, result));
                            ready = true;
                        }
                        Poll::Pending => index += 1,
                    }
                }
                self.setup_cursor = if index < self.pending.len() {
                    ready = true;
                    index
                } else {
                    0
                };
                ready |= self.poll_io(cx);
                if ready {
                    Poll::Ready(false)
                } else {
                    Poll::Pending
                }
            })
            .await;
            if stop {
                self.stop_owned(Instant::now())?;
                return Ok(());
            }
            for (id, key, result) in completions {
                self.setup_done(id, key, result, Instant::now())?;
            }
        }
    }
}
async fn drain_terminal_status(
    diagnostics: &mut Diagnostics,
    snapshot: Snapshot,
    deadline: Instant,
) -> Result<(), RuntimeError> {
    loop {
        if diagnostics.final_status_complete() {
            return Ok(());
        }
        let now = Instant::now();
        if now >= deadline {
            return Err(RuntimeError::DiagnosticCollectionIncomplete);
        }
        diagnostics
            .final_snapshot(now, snapshot)
            .map_err(|_| RuntimeError::DiagnosticCollectionIncomplete)?;
        tokio::time::sleep(Duration::from_millis(1).min(deadline - now)).await;
    }
}
pub async fn run(settings: &ValidatedConfig) -> Result<(), RuntimeError> {
    // Register before socket/TLS initialization so a stop during Starting is
    // observed before the first owner generation/send or recovery attempt.
    let term = signal(SignalKind::terminate()).map_err(|_| RuntimeError::Signal)?;
    let interrupt = signal(SignalKind::interrupt()).map_err(|_| RuntimeError::Signal)?;
    let role = if matches!(settings.role(), RoleConfig::Server(_)) {
        Role::Server
    } else {
        Role::Client
    };
    let resolver = SystemResolver::new(settings.limits().max_pending_setup)
        .map_err(|_| RuntimeError::Resource)?;
    let (mut diagnostics, receiver) = Diagnostics::new(
        role,
        Instant::now(),
        Duration::from_millis(settings.diagnostics().snapshot_interval_ms),
    )
    .map_err(|_| RuntimeError::Diagnostics)?;
    diagnostics.set_level(settings.diagnostics().level);
    let writer = receiver
        .spawn(io::stdout(), io::stderr())
        .map_err(|_| RuntimeError::Diagnostics)?;
    let mut runner = Runner::new(settings, &resolver, diagnostics)?;
    let result = runner.run(term, interrupt).await;
    runner.pending.clear();
    let ids: Vec<_> = runner.sessions.keys().copied().collect();
    for id in ids {
        runner.close_session(id, 0x100);
    }
    runner.sample_quic();
    for socket in runner.outer.iter().chain(runner.listener.iter()) {
        runner.retired_io = runner.retired_io.merge(socket.get_ref().0.borrow().stats());
    }
    runner.connections.clear();
    runner.sample_quic();
    runner.outer.clear();
    runner.listener = None;
    // One shared final-collection deadline, independent of periodic cadence.
    // This bounded wait occurs only after sessions/sockets and packet work end.
    let now = Instant::now();
    if !runner.server {
        runner.client_state(ClientState::Stopped, now)?;
    }
    runner.record_resources();
    runner.snapshot.sessions_active = 0;
    runner.snapshot.connections_active = 0;
    if runner.stop_started.is_some() {
        // A routine stop is a fixed terminal status, independent of ordinary
        // event suppression/cadence. Publish released-resource state even when
        // stopped before the first periodic snapshot.
        runner
            .diagnostics
            .failure(now, None, None, Failure::Stopped, 0, true)
            .map_err(|_| RuntimeError::Diagnostics)?;
    }
    let shared_deadline = runner.stop_started.unwrap_or(now) + Duration::from_secs(5);
    let deadline = (now + Duration::from_secs(1)).min(shared_deadline);
    let collection =
        drain_terminal_status(&mut runner.diagnostics, runner.snapshot, deadline).await;
    drop(runner);
    let flushed = writer.finish(deadline.saturating_duration_since(Instant::now()));
    if collection.is_err() || !flushed {
        return Err(RuntimeError::DiagnosticCollectionIncomplete);
    }
    result
}
#[cfg(test)]
mod tests {
    #[test]
    fn timer_rearm_after_cached_readiness_wakes_without_network_input() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let timer =
                tokio::io::unix::AsyncFd::new(crate::udp::MonotonicTimer::new().unwrap()).unwrap();
            timer
                .get_ref()
                .arm(Some(
                    std::time::Instant::now() + std::time::Duration::from_millis(5),
                ))
                .unwrap();
            // Consume the real expiration without clearing AsyncFd's cache,
            // exactly as the owner does before rearming its next deadline.
            let guard = timer.readable().await.unwrap();
            assert!(timer.get_ref().consume().unwrap() > 0);
            drop(guard);
            let start = std::time::Instant::now();
            timer
                .get_ref()
                .arm(Some(start + std::time::Duration::from_millis(10)))
                .unwrap();
            let result = tokio::time::timeout(
                std::time::Duration::from_millis(500),
                std::future::poll_fn(|cx| super::poll_timer(&timer, cx)),
            )
            .await;
            assert!(result.unwrap().unwrap() > 0);
            assert!(
                start.elapsed() < std::time::Duration::from_millis(250),
                "only the watchdog woke a timer with a lost readiness waker"
            );
        });
    }
    use super::*;
    use crate::common::{Fixtures, quic::Pair};
    use std::{cell::Cell, rc::Rc};
    proptest::proptest! {
        #[test]
        fn receive_density_matches_bounded_nonempty_history(
            observations in proptest::collection::vec(0usize..256, 0..2048)
        ) {
            let mut density = ReceiveDensity::default();
            let mut model = std::collections::VecDeque::new();
            for observed in observations {
                density.observe(observed);
                if observed != 0 {
                    model.push_back(observed.min(32));
                    if model.len() > DENSITY_HISTORY { model.pop_front(); }
                }
                proptest::prop_assert_eq!(density.estimate(), model.iter().copied().max().unwrap_or(1));
            }
        }
    }
    #[test]
    fn receive_density_retains_bursts_and_ignores_empty_observations() {
        let mut density = ReceiveDensity::default();
        assert_eq!(density.estimate(), 1);
        density.observe(usize::MAX);
        for _ in 0..4096 {
            density.observe(0);
        }
        assert_eq!(density.estimate(), 32);
        for _ in 1..DENSITY_HISTORY {
            density.observe(1);
        }
        assert_eq!(density.estimate(), 32);
        density.observe(1);
        assert_eq!(density.estimate(), 1);
        for _ in 0..8 {
            density.observe(8);
            for _ in 1..DENSITY_HISTORY {
                density.observe(1);
            }
            assert_eq!(
                density.estimate(),
                8,
                "recurring dense bursts stay protected"
            );
        }
        for _ in 0..DENSITY_HISTORY {
            density.observe(1);
        }
        assert_eq!(density.estimate(), 1);
    }

    // Exchange real QUIC packets while keeping the native receive queue drained.
    // Reverse ACKs give the sender credit for a complete history of large packets.
    fn density_native_round(
        runner: &mut Runner<'_>,
        client: &mut Connection,
        payloads: &[&[u8]],
    ) -> (usize, usize) {
        for payload in payloads {
            client.quic.dgram_send(payload).unwrap();
        }
        let mut bytes = [0; 1350];
        let mut packets = 0;
        while let Ok((length, info)) = client.quic.send(&mut bytes) {
            runner
                .outer_packet(
                    0,
                    &mut bytes[..length],
                    crate::udp::Received {
                        index: 0,
                        length,
                        peer: info.from,
                        local: info.to,
                        overflow: None,
                        fault: None,
                    },
                    Instant::now(),
                )
                .unwrap();
            packets += 1;
        }
        let server = &mut runner.connections.get_mut(&2).unwrap().transport.quic;
        let mut delivered = 0;
        while let Ok(n) = server.dgram_recv(&mut bytes) {
            assert_eq!(&bytes[..n], payloads[delivered]);
            delivered += 1;
        }
        while let Ok((length, info)) = server.send(&mut bytes) {
            client
                .quic
                .recv(
                    &mut bytes[..length],
                    quiche::RecvInfo {
                        from: info.from,
                        to: info.to,
                    },
                )
                .unwrap();
        }
        assert_eq!(
            delivered,
            payloads.len(),
            "fixture must deliver each real DATAGRAM"
        );
        (delivered, packets)
    }

    #[test]
    fn outer_read_quota_recovers_after_probe_burst_and_protects_new_bursts() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let f = Fixtures::new();
            let cfg = ValidatedConfig::load(&f.file("server.toml")).unwrap();
            let resolver = SystemResolver::new(1).unwrap();
            let now = Instant::now();
            let (diag, _receiver) =
                Diagnostics::new(Role::Server, now, Duration::from_secs(1)).unwrap();
            let mut r = Runner::new(&cfg, &resolver, diag).unwrap();
            let mut p = Pair::new(true);
            p.negotiate().unwrap();
            session_tests::install(&mut r, p.server);
            let id = r.create_session(2, 0, None, now).unwrap();
            r.sessions.get_mut(&id).unwrap().state = State::Active;
            let short = [7; 128];
            let large = [9; 1200];
            density_native_round(&mut r, &mut p.client, &[&short[..]; 8]);
            let reduced = r.outer_receive_quota(0);
            assert!(
                reduced < 32,
                "real short-packet coalescing must lower quota"
            );
            // A control-only packet and its ACK must not age the history.
            p.client.quic.send_ack_eliciting().unwrap();
            let (_, control_packets) = density_native_round(&mut r, &mut p.client, &[]);
            assert!(control_packets > 0);
            assert_eq!(r.outer_receive_quota(0), reduced);
            for _ in 1..32 {
                density_native_round(&mut r, &mut p.client, &[&large]);
                assert_eq!(r.outer_receive_quota(0), reduced);
            }
            density_native_round(&mut r, &mut p.client, &[&large]);
            assert_eq!(
                r.outer_receive_quota(0),
                32,
                "a historical probe burst must not permanently throttle large packets"
            );
            density_native_round(&mut r, &mut p.client, &[&short[..]; 8]);
            assert!(
                r.outer_receive_quota(0) < 32,
                "large-to-small transition must protect the queue"
            );
            assert_eq!(
                r.outer_receive_quota(1),
                32,
                "unrelated sockets stay independent"
            );
            r.sessions.get_mut(&id).unwrap().state = State::Closed;
            assert_eq!(
                r.outer_receive_quota(0),
                32,
                "closed/unadmitted flows cannot constrain a shared socket"
            );
        });
    }
    #[test]
    fn density_history_is_per_connection_and_only_active_flows_limit_shared_socket() {
        session_tests::runtime().block_on(async {
            let f = Fixtures::new();
            let cfg = ValidatedConfig::load(&f.file("server.toml")).unwrap();
            let resolver = SystemResolver::new(1).unwrap();
            let now = Instant::now();
            let (diag, _receiver) =
                Diagnostics::new(Role::Server, now, Duration::from_secs(1)).unwrap();
            let mut r = Runner::new(&cfg, &resolver, diag).unwrap();
            let p = Pair::new(true);
            session_tests::install(&mut r, p.server);
            let first = r.create_session(2, 0, None, now).unwrap();
            r.sessions.get_mut(&first).unwrap().state = State::Active;
            r.connections
                .get_mut(&2)
                .unwrap()
                .receive_density
                .observe(4);
            let mut other_pair = Pair::new(true);
            other_pair.negotiate().unwrap();
            let mut other = other_pair.server;
            other.generation = crate::buffers::Generation(3);
            let mut density = ReceiveDensity::default();
            density.observe(8);
            let permit = r.budgets.connections.reserve(1, 0).unwrap();
            let connection_id = r.diagnostics.next_id().unwrap();
            r.connections.insert(
                3,
                OwnedConnection {
                    transport: other,
                    socket: 0,
                    _permit: permit,
                    _socket_permit: None,
                    id: connection_id,
                    initial_cid: vec![],
                    cursors: [FairCursor::default(); 7],
                    receive_density: density,
                    enqueue_pending: false,
                    output_pending: false,
                },
            );
            assert_eq!(
                r.outer_receive_quota(0),
                8,
                "unadmitted connection has no influence"
            );
            let second = r.create_session(3, 4, None, now).unwrap();
            r.sessions.get_mut(&second).unwrap().state = State::Active;
            assert_eq!(r.outer_receive_quota(0), 4);
            for _ in 0..32 {
                r.connections
                    .get_mut(&2)
                    .unwrap()
                    .receive_density
                    .observe(1);
            }
            assert_eq!(r.connections[&2].receive_density.estimate(), 1);
            assert_eq!(r.connections[&3].receive_density.estimate(), 8);
            assert_eq!(
                r.outer_receive_quota(0),
                4,
                "aging one connection must not erase another's burst"
            );
            r.sessions.get_mut(&second).unwrap().state = State::Closed;
            assert_eq!(r.outer_receive_quota(0), 32);
            assert_eq!(r.outer_receive_quota(1), 32);
        });
    }
    #[test]
    fn ready_ciphertext_backlog_releases_native_credit_before_fresh_ingress() {
        session_tests::runtime().block_on(async {
            let f = Fixtures::new();
            let cfg = ValidatedConfig::load(&f.file("client.toml")).unwrap();
            let resolver = SystemResolver::new(1).unwrap();
            let (diag, _receiver) =
                Diagnostics::new(Role::Client, Instant::now(), Duration::from_secs(1)).unwrap();
            let mut r = Runner::new(&cfg, &resolver, diag).unwrap();
            let mut peer = Pair::new(true);
            let (stream, _) = peer.requests();
            peer.client.generation = crate::buffers::Generation(2);
            r.connections.clear();
            session_tests::install(&mut r, peer.client);
            r.primary = Some(2);
            let sender = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
            let local = r
                .listener
                .as_ref()
                .unwrap()
                .get_ref()
                .0
                .borrow()
                .local_addr();
            let id = r
                .create_session(
                    2,
                    stream,
                    Some((sender.local_addr().unwrap(), local)),
                    Instant::now(),
                )
                .unwrap();
            r.sessions.get_mut(&id).unwrap().state = State::Active;
            let now = Instant::now();
            let payload = [7; 128];
            let mut scratch = [0; 1350];
            for _ in 0..64 {
                r.connections
                    .get_mut(&2)
                    .unwrap()
                    .transport
                    .enqueue(stream, &payload, now, now, &mut scratch)
                    .unwrap();
            }
            for _ in 0..32 {
                r.queue_outgoing(id, &payload, now).unwrap();
            }
            // A full already-committed FIFO can be writable while the native
            // queue and application queue are full. Keep the original records
            // observable through the scripted physical-send boundary.
            for sequence in 0..32 {
                let mut slab = r.pools.ciphertext.acquire().unwrap();
                slab.bytes_mut()[0] = sequence as u8;
                let packet = crate::buffers::Ciphertext::new(
                    slab,
                    1,
                    local,
                    sender.local_addr().unwrap(),
                    now,
                    crate::buffers::Generation(2),
                    sequence,
                    now,
                    now + Duration::from_secs(1),
                )
                .unwrap();
                r.connections
                    .get_mut(&2)
                    .unwrap()
                    .transport
                    .output
                    .push_back(packet);
            }
            let sent = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
            let observed = sent.clone();
            r.outer_send_hook = Some(Box::new(move |packets| {
                for packet in packets {
                    observed.borrow_mut().push(packet.bytes.to_vec());
                }
                Ok(packets.len())
            }));
            for _ in 0..6 {
                sender.send_to(&payload, local).unwrap();
            }
            r.service_output(2, Instant::now()).unwrap();
            assert_eq!(
                sent.borrow().len(),
                OUTPUT_QUOTA,
                "one submission budget across the visit"
            );
            for (sequence, bytes) in sent.borrow().iter().enumerate() {
                assert_eq!(
                    bytes.as_slice(),
                    &[sequence as u8],
                    "committed FIFO prefix must not be replayed/reordered"
                );
            }
            r.schedule_datagrams(2, Instant::now()).unwrap();
            r.client_input(Instant::now()).unwrap();
            assert_eq!(
                r.snapshot.counters.local_drops.queue_full.to_target, 0,
                "writable committed output must release native credit before reading new ingress"
            );
            assert_eq!(r.sessions[&id].queue.used().count, 6);
        });
    }
    #[test]
    fn output_visit_shares_submission_budget_after_partial_send_or_pressure() {
        for old_count in [8, 32] {
            for first_result in [
                Ok(1),
                Err(libc::EAGAIN),
                Err(libc::EINTR),
                Err(libc::ENOBUFS),
            ] {
                session_tests::runtime().block_on(async {
                    let f = Fixtures::new();
                    let cfg = ValidatedConfig::load(&f.file("client.toml")).unwrap();
                    let resolver = SystemResolver::new(1).unwrap();
                    let (diag, _receiver) =
                        Diagnostics::new(Role::Client, Instant::now(), Duration::from_secs(1))
                            .unwrap();
                    let mut r = Runner::new(&cfg, &resolver, diag).unwrap();
                    let mut peer = Pair::new(true);
                    let (stream, _) = peer.requests();
                    peer.client.generation = crate::buffers::Generation(2);
                    r.connections.clear();
                    session_tests::install(&mut r, peer.client);
                    let now = Instant::now();
                    let local = r.outer[0].get_ref().0.borrow().local_addr();
                    let remote = "127.0.0.1:4433".parse().unwrap();
                    let mut scratch = [0; 1350];
                    for _ in 0..64 {
                        r.connections
                            .get_mut(&2)
                            .unwrap()
                            .transport
                            .enqueue(stream, &[7; 128], now, now, &mut scratch)
                            .unwrap();
                    }
                    for sequence in 0..old_count {
                        let mut slab = r.pools.ciphertext.acquire().unwrap();
                        slab.bytes_mut()[0] = sequence as u8;
                        let packet = crate::buffers::Ciphertext::new(
                            slab,
                            1,
                            local,
                            remote,
                            now,
                            crate::buffers::Generation(2),
                            sequence,
                            now,
                            now + Duration::from_secs(1),
                        )
                        .unwrap();
                        r.connections
                            .get_mut(&2)
                            .unwrap()
                            .transport
                            .output
                            .push_back(packet);
                    }
                    let offered = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
                    let observed = offered.clone();
                    r.outer_send_hook = Some(Box::new(move |packets| {
                        let first = observed.borrow().is_empty();
                        observed
                            .borrow_mut()
                            .push(packets.iter().map(|p| p.bytes.to_vec()).collect::<Vec<_>>());
                        if first {
                            first_result.map_err(io::Error::from_raw_os_error)
                        } else {
                            Ok(packets.len())
                        }
                    }));
                    r.service_output(2, now).unwrap();
                    let calls = offered.borrow();
                    assert_eq!(calls[0].len(), old_count.min(OUTPUT_QUOTA as u64) as usize);
                    assert_eq!(
                        calls.iter().map(Vec::len).sum::<usize>(),
                        OUTPUT_QUOTA,
                        "partial/error results must not refund offered descriptors"
                    );
                    if old_count == 32 {
                        assert_eq!(calls.len(), 1);
                    } else {
                        assert_eq!(calls.len(), 2);
                        let first_remaining = match first_result {
                            Ok(1) | Err(libc::ENOBUFS) => 1,
                            _ => 0,
                        };
                        assert_eq!(
                            calls[1][0],
                            vec![first_remaining],
                            "accepted/discarded prefix cannot be replayed"
                        );
                    }
                    assert!(r.connections[&2].transport.output.len() <= 32);
                });
            }
        }
    }

    #[test]
    fn output_visit_checks_expiry_and_pacing_before_preflush() {
        for expired in [false, true] {
            session_tests::runtime().block_on(async {
                let f = Fixtures::new();
                let cfg = ValidatedConfig::load(&f.file("client.toml")).unwrap();
                let resolver = SystemResolver::new(1).unwrap();
                let (diag, _receiver) =
                    Diagnostics::new(Role::Client, Instant::now(), Duration::from_secs(1)).unwrap();
                let mut r = Runner::new(&cfg, &resolver, diag).unwrap();
                let mut peer = Pair::new(true);
                peer.requests();
                peer.client.generation = crate::buffers::Generation(2);
                r.connections.clear();
                session_tests::install(&mut r, peer.client);
                let now = Instant::now();
                let local = r.outer[0].get_ref().0.borrow().local_addr();
                let mut slab = r.pools.ciphertext.acquire().unwrap();
                slab.bytes_mut()[0] = 42;
                let packet = crate::buffers::Ciphertext::new(
                    slab,
                    1,
                    local,
                    "127.0.0.1:4433".parse().unwrap(),
                    if expired {
                        now
                    } else {
                        now + Duration::from_secs(1)
                    },
                    crate::buffers::Generation(2),
                    0,
                    now,
                    now + Duration::from_secs(2),
                )
                .unwrap();
                r.connections
                    .get_mut(&2)
                    .unwrap()
                    .transport
                    .output
                    .push_back(packet);
                r.outer_send_hook = Some(Box::new(|_| panic!("ineligible ciphertext submitted")));
                let result = r.service_output(
                    2,
                    if expired {
                        now + Duration::from_secs(3)
                    } else {
                        now
                    },
                );
                if expired {
                    assert!(matches!(
                        result,
                        Err(OutputError::Transport(TransportError::Expired))
                    ));
                } else {
                    result.unwrap();
                    assert_eq!(
                        r.connections[&2].transport.output.front().unwrap().bytes(),
                        &[42]
                    );
                }
                assert_eq!(r.snapshot.counters.outer_socket_sent, 0);
            });
        }
    }

    #[test]
    fn available_output_credit_is_serviced_before_new_ingress_fills_a_session() {
        session_tests::runtime().block_on(async {
            let f = Fixtures::new();
            let cfg = ValidatedConfig::load(&f.file("client.toml")).unwrap();
            let resolver = SystemResolver::new(1).unwrap();
            let now = Instant::now();
            let (diag, _receiver) =
                Diagnostics::new(Role::Client, now, Duration::from_secs(1)).unwrap();
            let mut r = Runner::new(&cfg, &resolver, diag).unwrap();
            let mut p = Pair::new(true);
            let (stream, _) = p.requests();
            p.client.generation = crate::buffers::Generation(2);
            // Replace the initial outbound handshake with this established peer.
            r.connections.clear();
            session_tests::install(&mut r, p.client);
            r.primary = Some(2);
            let sender = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
            let local = r
                .listener
                .as_ref()
                .unwrap()
                .get_ref()
                .0
                .borrow()
                .local_addr();
            let id = r
                .create_session(2, stream, Some((sender.local_addr().unwrap(), local)), now)
                .unwrap();
            r.sessions.get_mut(&id).unwrap().state = State::Active;
            // Start packet residence after TLS/fixture setup has completed.
            let now = Instant::now();
            let payload = |n: u8| {
                let mut bytes = [0; 128];
                bytes[0] = n;
                bytes
            };
            let mut scratch = [0; 1350];
            for n in 0..64 {
                r.connections
                    .get_mut(&2)
                    .unwrap()
                    .transport
                    .enqueue(stream, &payload(n), now, now, &mut scratch)
                    .unwrap();
            }
            for n in 64..96 {
                r.queue_outgoing(id, &payload(n), now).unwrap();
            }
            assert_eq!(r.connections[&2].transport.quic.dgram_send_queue_len(), 64);
            assert_eq!(r.sessions[&id].queue.used().count, 32);
            for n in 96..102 {
                sender.send_to(&payload(n), local).unwrap();
            }
            let mut delivered = Vec::new();
            for round in 0..3 {
                let visits = r.owner_visits();
                let (window, _, continuation) = service_window(visits.len(), 0);
                assert!(!continuation, "fixture must use one bounded owner window");
                for &(kind, key) in &visits[window] {
                    let now = Instant::now();
                    match kind {
                        0 => {
                            r.receive_outer(key as usize, now).unwrap();
                        }
                        1 => {
                            r.client_input(now).unwrap();
                        }
                        2 => {
                            r.h3_work(key, now).unwrap();
                        }
                        3 => {
                            r.flush_relay(key, now).unwrap();
                            r.relay_input(key, now).unwrap();
                        }
                        4 => {
                            r.schedule_datagrams(key, now).unwrap();
                        }
                        5 => {
                            r.connections
                                .get_mut(&key)
                                .unwrap()
                                .generate_output(&r.pools.ciphertext, now)
                                .unwrap();
                            // Native protocol pump: pacing/native partial submissions
                            // remain covered by the existing real-UDP output tests.
                            while let Some(packet) = r
                                .connections
                                .get_mut(&key)
                                .unwrap()
                                .transport
                                .output
                                .pop_front()
                            {
                                let mut bytes = packet.bytes().to_vec();
                                p.server
                                    .quic
                                    .recv(
                                        &mut bytes,
                                        quiche::RecvInfo {
                                            from: packet.from(),
                                            to: packet.to(),
                                        },
                                    )
                                    .unwrap();
                                while let Ok(n) = p.server.quic.dgram_recv(&mut scratch) {
                                    let datagram =
                                        masque::decode_datagram(&scratch[..n]).unwrap().unwrap();
                                    delivered.push(datagram.payload[0]);
                                }
                                while let Ok((n, info)) = p.server.quic.send(&mut scratch) {
                                    r.connections
                                        .get_mut(&key)
                                        .unwrap()
                                        .transport
                                        .receive(
                                            &mut scratch[..n],
                                            quiche::RecvInfo {
                                                from: info.from,
                                                to: info.to,
                                            },
                                            Instant::now(),
                                        )
                                        .unwrap();
                                }
                            }
                        }
                        _ => unreachable!(),
                    }
                }
                assert_eq!(
                    r.snapshot.counters.local_drops.queue_full.to_target, 0,
                    "new ingress was dropped before available native output/queue service"
                );
                assert_eq!(r.packet_continuation(), round < 2,
                    "new staged work must survive the ring boundary, then stop requesting empty rounds");
            }
            assert_eq!(
                delivered,
                (0..102).collect::<Vec<u8>>(),
                "accepted bytes remain FIFO without loss or duplication"
            );
            assert_eq!(r.sessions[&id].queue.used().count, 0);
        });
    }
    #[test]
    fn outer_read_quota_observes_native_datagram_density_and_retains_large_batches() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            for size in [128, 1200] {
                let f = Fixtures::new();
                let cfg = ValidatedConfig::load(&f.file("server.toml")).unwrap();
                let resolver = SystemResolver::new(1).unwrap();
                let now = Instant::now();
                let (diag, _receiver) =
                    Diagnostics::new(Role::Server, now, Duration::from_secs(1)).unwrap();
                let mut r = Runner::new(&cfg, &resolver, diag).unwrap();
                let mut p = Pair::new(true);
                p.negotiate().unwrap();
                session_tests::install(&mut r, p.server);
                assert_eq!(r.outer_receive_quota(0), cfg.io().batch_max.min(32));
                for _ in 0..8 {
                    p.client.quic.dgram_send(&vec![7; size]).unwrap();
                }
                let mut bytes = [0; 1350];
                let mut max_density = 0;
                loop {
                    let (length, info) = match p.client.quic.send(&mut bytes) {
                        Ok(packet) => packet,
                        Err(quiche::Error::Done) => break,
                        Err(error) => panic!("native density fixture: {error:?}"),
                    };
                    let before = r.connections[&2].transport.quic.dgram_recv_queue_len();
                    r.outer_packet(
                        0,
                        &mut bytes[..length],
                        crate::udp::Received {
                            index: 0,
                            length,
                            peer: info.from,
                            local: info.to,
                            overflow: None,
                            fault: None,
                        },
                        Instant::now(),
                    )
                    .unwrap();
                    let after = r.connections[&2].transport.quic.dgram_recv_queue_len();
                    max_density = max_density.max(after - before);
                }
                assert_eq!(r.connections[&2].transport.quic.dgram_recv_queue_len(), 8);
                assert_eq!(r.connections[&2].receive_density.estimate(), max_density);
                // Unadmitted connections must not constrain a shared listener.
                assert_eq!(r.outer_receive_quota(0), cfg.io().batch_max.min(32));
                let id = r.create_session(2, 0, None, now).unwrap();
                r.sessions.get_mut(&id).unwrap().state = State::Active;
                if size == 128 {
                    assert!(max_density > 1, "native coalescing must be exercised");
                    assert!(r.outer_receive_quota(0) < cfg.io().batch_max.min(32));
                } else {
                    assert_eq!(max_density, 1);
                    assert_eq!(r.outer_receive_quota(0), cfg.io().batch_max.min(32));
                }
                let quota = r.outer_receive_quota(0);
                while r
                    .connections
                    .get_mut(&2)
                    .unwrap()
                    .transport
                    .quic
                    .dgram_recv(&mut bytes)
                    .is_ok()
                {}
                // Draining the queue must not forget the observed coalescing.
                assert_eq!(r.outer_receive_quota(0), quota);
                // Other sockets do not inherit a connection's density.
                assert_eq!(r.outer_receive_quota(1), cfg.io().batch_max.min(32));
                r.sessions.get_mut(&id).unwrap().state = State::Closed;
                assert_eq!(r.outer_receive_quota(0), cfg.io().batch_max.min(32));
            }
        });
    }
    #[test]
    fn incoming_datagram_size_and_egress_pressure_keep_correlation() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let f = Fixtures::new();
            let cfg = ValidatedConfig::load(&f.file("server.toml")).unwrap();
            let resolver = SystemResolver::new(1).unwrap();
            let now = Instant::now();
            let (diag, receiver) =
                Diagnostics::new(Role::Server, now, Duration::from_secs(1)).unwrap();
            let mut r = Runner::new(&cfg, &resolver, diag).unwrap();
            let p = Pair::new(true);
            let connection_id = r.diagnostics.next_id().unwrap();
            r.connections.insert(
                2,
                OwnedConnection {
                    transport: p.server,
                    socket: 0,
                    _permit: r.budgets.connections.reserve(1, 0).unwrap(),
                    _socket_permit: None,
                    id: connection_id,
                    initial_cid: vec![],
                    cursors: [FairCursor::default(); 7],
                    receive_density: ReceiveDensity::default(),
                    enqueue_pending: false,
                    output_pending: false,
                },
            );
            let id = r.create_session(2, 0, None, now).unwrap();
            r.sessions.get_mut(&id).unwrap().state = State::Active;
            let sid = r.sessions[&id].id;
            let expiry = now + Duration::from_millis(cfg.limits().application_residence_ms);
            let payload = vec![0; cfg.transport().application_max_bytes + 1];
            let mut bytes = vec![0; payload.len() + 16];
            let n = masque::encode_datagram(0, &payload, &mut bytes).unwrap();
            r.datagram(2, &bytes[..n], expiry, now).unwrap();
            // Exercise the real post-syscall path without relying on nondeterministic
            // loopback EAGAIN. Resource exhaustion must not submit or replay data.
            let mut held = Vec::new();
            while let Ok(slab) = r.pools.application.acquire() {
                held.push(slab);
            }
            r.complete_datagram_send(
                id,
                b"held",
                expiry,
                now,
                Err(io::ErrorKind::WouldBlock.into()),
            )
            .unwrap();
            assert_eq!(r.sessions[&id].queue.used().count, 0);
            r.diagnostics.snapshot(now, r.snapshot).unwrap();
            let mut first = Vec::new();
            receiver.write_one(&mut first, &mut io::sink()).unwrap();
            let first: serde_json::Value = serde_json::from_slice(&first).unwrap();
            assert!(
                first["failures"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|f| f["class"] == "resource"
                        && f["code"] == 1
                        && f["session"] == serde_json::to_value(sid).unwrap()
                        && f["connection"] == serde_json::to_value(connection_id).unwrap()),
                "{first}"
            );
            assert!(
                first["failures"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|f| f["class"] == "size"
                        && f["code"] == 1
                        && f["session"] == serde_json::to_value(sid).unwrap()
                        && f["connection"] == serde_json::to_value(connection_id).unwrap()),
                "{first}"
            );
            drop(held);
            loop {
                let before = r.sessions[&id].queue.used().count;
                r.complete_datagram_send(
                    id,
                    b"queued",
                    expiry,
                    now,
                    Err(io::ErrorKind::WouldBlock.into()),
                )
                .unwrap();
                if r.sessions[&id].queue.used().count == before {
                    break;
                }
            }
            let queued = r.sessions[&id].queue.used().count;
            assert!(queued > 0);
            r.close_session(id, 0x100);
            r.diagnostics
                .snapshot(now + Duration::from_secs(1), r.snapshot)
                .unwrap();
            let record = receiver.try_next().unwrap().unwrap();
            let v: serde_json::Value = serde_json::from_slice(record.bytes()).unwrap();
            assert_eq!(v["coverage_lost"], 0);
            {
                let (class, code) = ("resource", 2);
                assert!(
                    v["failures"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|f| f["class"] == class
                            && f["code"] == code
                            && f["session"] == serde_json::to_value(sid).unwrap()
                            && f["connection"] == serde_json::to_value(connection_id).unwrap()
                            && f["terminal"] == true),
                    "{v}"
                );
            }
            // Both resource faults count even though the live summary coalesces them.
            assert!(v["failure_totals"]["resource"].as_u64().unwrap() >= 2);
        });
    }
    #[test]
    fn diagnostic_setup_fault_matrix_preserves_classes_after_cleanup() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let f = Fixtures::new();
            let cfg = ValidatedConfig::load(&f.file("server.toml")).unwrap();
            let resolver = SystemResolver::new(1).unwrap();
            let now = Instant::now();
            let (mut diag, receiver) =
                Diagnostics::new(Role::Server, now, Duration::from_secs(1)).unwrap();
            for _ in 0..80 {
                let _ = diag.event(
                    now,
                    Event::failure(crate::config::Level::Warn, Failure::Transport, 0),
                );
            }
            let mut r = Runner::new(&cfg, &resolver, diag).unwrap();
            let p = Pair::new(true);
            let connection_id = r.diagnostics.next_id().unwrap();
            r.connections.insert(
                2,
                OwnedConnection {
                    transport: p.server,
                    socket: 0,
                    _permit: r.budgets.connections.reserve(1, 0).unwrap(),
                    _socket_permit: None,
                    id: connection_id,
                    initial_cid: vec![],
                    cursors: [FairCursor::default(); 7],
                    receive_density: ReceiveDensity::default(),
                    enqueue_pending: false,
                    output_pending: false,
                },
            );
            let cases = [
                (PolicyError::Resolution, "resolution", 502),
                (PolicyError::Timeout, "setup_timeout", 504),
                (PolicyError::Capacity, "resource", 503),
                (PolicyError::Authentication, "authentication", 401),
                (PolicyError::Denied, "authorization", 403),
            ];
            let mut expected = Vec::new();
            for (index, (fault, class, code)) in cases.into_iter().enumerate() {
                let id = r.create_session(2, (index * 4) as u64, None, now).unwrap();
                let opaque = serde_json::to_value(r.sessions[&id].id).unwrap();
                let key = SetupKey {
                    connection: crate::buffers::Generation(2),
                    session: crate::buffers::Generation(id),
                };
                r.setup_done(id, key, Err(fault), now).unwrap();
                assert!(!r.sessions.contains_key(&id));
                expected.push((opaque, class, code));
            }
            r.record_resources();
            r.diagnostics.snapshot(now, r.snapshot).unwrap();
            let mut out = Vec::new();
            receiver.write_one(&mut out, &mut std::io::sink()).unwrap();
            let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
            for (session, class, code) in expected {
                assert!(
                    v["failures"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|f| f["session"] == session
                            && f["class"] == class
                            && f["code"] == code
                            && f["terminal"] == true)
                );
            }
            assert_eq!(v["coverage_lost"], 0);
            assert_eq!(v["status_write_failures"], 0);
            assert_eq!(v["resources"]["setup"], 0);
            assert!(v["diagnostics_dropped"].as_u64().unwrap() > 0);
        });
    }
    #[test]
    fn intact_outer_above_configured_cap_and_truncation_have_role_specific_drop_counters() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            for (role, name) in [(Role::Client, "client"), (Role::Server, "server")] {
                let f = Fixtures::new();
                let file = f.file(&format!("{name}.toml"));
                let text = std::fs::read_to_string(&file).unwrap();
                std::fs::write(&file, text + "\n[transport]\nouter_udp_max_bytes = 1200\n")
                    .unwrap();
                let cfg = ValidatedConfig::load(&file).unwrap();
                let resolver = SystemResolver::new(1).unwrap();
                let now = Instant::now();
                let (diag, _receiver) =
                    Diagnostics::new(role, now, Duration::from_secs(1)).unwrap();
                let mut r = Runner::new(&cfg, &resolver, diag).unwrap();
                // Larger controlled scratch distinguishes an intact message
                // above the configured cap from a genuinely truncated one.
                r.pools.scratch = crate::buffers::BufferPool::new(2048, 4096).unwrap();
                let port = r.outer[0].get_ref().0.borrow().local_addr().port();
                let sender = if let RoleConfig::Client(client) = cfg.role() {
                    std::net::UdpSocket::bind(client.server_address).unwrap()
                } else {
                    std::net::UdpSocket::bind("127.0.0.1:0").unwrap()
                };
                sender.send_to(&[0; 1300], ("127.0.0.1", port)).unwrap();
                sender.send_to(&[0; 2100], ("127.0.0.1", port)).unwrap();
                r.receive_outer(0, now).unwrap();
                let drops = &r.snapshot.counters.local_drops;
                if name == "client" {
                    assert_eq!(drops.oversized.to_client, 1);
                    assert_eq!(drops.socket_truncated.to_client, 1);
                    assert_eq!(drops.oversized.to_target, 0);
                    assert_eq!(drops.socket_truncated.to_target, 0);
                } else {
                    assert_eq!(drops.oversized.to_target, 1);
                    assert_eq!(drops.socket_truncated.to_target, 1);
                    assert_eq!(drops.oversized.to_client, 0);
                    assert_eq!(drops.socket_truncated.to_client, 0);
                }
                assert_eq!(r.snapshot.counters.outer_pmtu_errors, 0);
                assert_eq!(r.snapshot.counters.outer_quic_lost_packets, 0);
                assert_eq!(r.snapshot.counters.kernel_rx_overflow, 0);
            }
        });
    }

    #[test]
    fn requests_before_peer_settings_are_bounded_cancellable_and_do_not_start_policy_work() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let f = Fixtures::new();
            let path = f.file("server.toml");
            let text = std::fs::read_to_string(&path).unwrap();
            std::fs::write(&path, text + "\n[limits]\nmax_pending_setup = 2\n").unwrap();
            let cfg = ValidatedConfig::load(&path).unwrap();
            let resolver = SystemResolver::new(1).unwrap();
            let now = Instant::now();
            let (diag, _receiver) =
                Diagnostics::new(Role::Server, now, Duration::from_secs(1)).unwrap();
            let mut r = Runner::new(&cfg, &resolver, diag).unwrap();
            let mut p = Pair::new(true);
            let (a, b) = p.requests();
            let permit = r.budgets.connections.reserve(1, 0).unwrap();
            let cid = r.diagnostics.next_id().unwrap();
            r.connections.insert(
                2,
                OwnedConnection {
                    transport: p.server,
                    socket: 0,
                    _permit: permit,
                    _socket_permit: None,
                    id: cid,
                    initial_cid: vec![],
                    cursors: [FairCursor::default(); 7],
                    receive_density: ReceiveDensity::default(),
                    enqueue_pending: false,
                    output_pending: false,
                },
            );
            let token = std::fs::read_to_string(f.file("client.token")).unwrap();
            let ports: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(f.file("ports.json")).unwrap())
                    .unwrap();
            let RoleConfig::Server(server) = cfg.role() else {
                panic!()
            };
            let headers = masque::request_headers(
                &server.proxy_authority,
                &format!("/.well-known/masque/udp/127.0.0.1/{}/", ports["target"]),
                token.trim().as_bytes().try_into().unwrap(),
            );
            for stream in [a, b] {
                r.defer_headers(2, stream, headers.clone(), true, now)
                    .unwrap();
            }
            assert_eq!(r.sessions.len(), 2);
            assert_eq!(r.budgets.setup.used().count, 2);
            assert!(r.pending.is_empty());
            assert_eq!(r.budgets.relay_fds.used().count, 0);
            assert!(
                r.sessions.values().all(|s| s.state == State::Starting
                    && s.identity.is_none()
                    && s.relay.is_none())
            );
            r.defer_headers(2, 8, headers.clone(), true, now).unwrap();
            assert_eq!(r.sessions.len(), 2);
            assert_eq!(r.budgets.setup.used().count, 2);
            let old = r.lookup(2, a).unwrap();
            // A real reset/closure cancels its raw header reservation before
            // capability/policy work, without affecting the other request.
            r.close_session(old, 0x10c);
            assert_eq!(r.budgets.setup.used().count, 1);
            assert!(r.lookup(2, b).is_some());
            assert_eq!(r.service_deferred_headers(2, now).unwrap(), 1);
            assert_eq!(r.pending.len(), 1);
            assert!(r.sessions.values().all(|s| s.deferred_headers.is_none()));
            assert_eq!(r.budgets.relay_fds.used().count, 0);
            let id = r.lookup(2, b).unwrap();
            assert_eq!(r.sessions[&id].started, now);
            r.expire_sessions(now + Duration::from_millis(cfg.recovery().setup_timeout_ms));
            assert!(r.sessions.is_empty());
            assert!(r.pending.is_empty());
            assert_eq!(r.budgets.setup.used().count, 0);
        });
    }

    #[test]
    fn reset_cancels_partial_capsule_without_inventing_malformed_fin() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let f = Fixtures::new();
            let cfg = ValidatedConfig::load(&f.file("server.toml")).unwrap();
            let resolver = SystemResolver::new(1).unwrap();
            let now = Instant::now();
            let (diag, receiver) =
                Diagnostics::new(Role::Server, now, Duration::from_secs(1)).unwrap();
            let mut r = Runner::new(&cfg, &resolver, diag).unwrap();
            let mut p = Pair::new(true);
            let (stream, _) = p.requests();
            p.client
                .quic
                .stream_shutdown(stream, quiche::Shutdown::Write, 0x10c)
                .unwrap();
            for _ in 0..20 {
                p.pump();
            }
            let permit = r.budgets.connections.reserve(1, 0).unwrap();
            let cid = r.diagnostics.next_id().unwrap();
            r.connections.insert(
                2,
                OwnedConnection {
                    transport: p.server,
                    socket: 0,
                    _permit: permit,
                    _socket_permit: None,
                    id: cid,
                    initial_cid: vec![],
                    cursors: [FairCursor::default(); 7],
                    receive_density: ReceiveDensity::default(),
                    enqueue_pending: false,
                    output_pending: false,
                },
            );
            let id = r.create_session(2, stream, None, now).unwrap();
            r.sessions
                .get_mut(&id)
                .unwrap()
                .capsules
                .feed(&[0, 10, 0], false)
                .unwrap();
            r.h3_work(2, Instant::now()).unwrap();
            assert!(!r.sessions.contains_key(&id));
            r.diagnostics.snapshot(Instant::now(), r.snapshot).unwrap();
            let mut out = Vec::new();
            receiver.write_one(&mut out, &mut std::io::sink()).unwrap();
            let status: serde_json::Value = serde_json::from_slice(&out).unwrap();
            let failures = status["failures"].as_array().unwrap();
            assert!(
                failures
                    .iter()
                    .any(|f| f["class"] == "transport" && f["code"] == 0x10c)
            );
            assert!(!failures.iter().any(|f| f["class"] == "malformed"));
        });
    }
    #[test]
    fn invalid_and_unvalidated_initials_do_not_allocate_connections() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let f = Fixtures::new();
            let cfg = ValidatedConfig::load(&f.file("server.toml")).unwrap();
            let client = ValidatedConfig::load(&f.file("client.toml")).unwrap();
            let resolver = SystemResolver::new(1).unwrap();
            let (diag, _receiver) =
                Diagnostics::new(Role::Server, Instant::now(), Duration::from_secs(1)).unwrap();
            let mut r = Runner::new(&cfg, &resolver, diag).unwrap();
            let local = r.outer[0].get_ref().0.borrow().local_addr();
            let peer = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
            peer.set_read_timeout(Some(Duration::from_millis(100)))
                .unwrap();
            let remote = peer.local_addr().unwrap();
            let mut config = transport::config(&client).unwrap();
            let cid = transport::random_cid().unwrap();
            let mut q = quiche::connect_with_buffer_factory::<TrackedFactory>(
                Some("remora.test"),
                &quiche::ConnectionId::from_ref(&cid),
                remote,
                local,
                &mut config,
            )
            .unwrap();
            let mut initial = [0; 1350];
            let (n, _) = q.send(&mut initial).unwrap();
            assert!(n >= 1200);
            let meta = crate::udp::Received {
                index: 0,
                length: n,
                peer: remote,
                local,
                overflow: None,
                fault: None,
            };
            r.outer_packet(0, &mut [0xff; 31], meta, Instant::now())
                .unwrap();
            r.outer_packet(0, &mut initial[..1199], meta, Instant::now())
                .unwrap();
            let mut version = initial;
            version[1..5].copy_from_slice(&0xbabababau32.to_be_bytes());
            r.outer_packet(0, &mut version[..n], meta, Instant::now())
                .unwrap();
            r.outer_packet(0, &mut initial[..n], meta, Instant::now())
                .unwrap();
            assert!(r.connections.is_empty());
            assert!(r.sessions.is_empty());
            assert_eq!(r.budgets.connections.used().count, 0);
            let mut response = [0; 1350];
            let mut retry_seen = false;
            while let Ok((n, _)) = peer.recv_from(&mut response) {
                if let Ok(header) = quiche::Header::from_slice(&mut response[..n], 16) {
                    retry_seen |= header.ty == quiche::Type::Retry;
                }
            }
            assert!(retry_seen, "unvalidated initial requires stateless Retry");
        });
    }
    #[test]
    fn rejection_preserves_final_response_after_session_cleanup() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let f = Fixtures::new();
            let cfg = ValidatedConfig::load(&f.file("server.toml")).unwrap();
            let resolver = SystemResolver::new(1).unwrap();
            let (diag, _receiver) =
                Diagnostics::new(Role::Server, Instant::now(), Duration::from_secs(1)).unwrap();
            let mut r = Runner::new(&cfg, &resolver, diag).unwrap();
            let mut p = Pair::new(true);
            let (stream, _) = p.requests();
            let permit = r.budgets.connections.reserve(1, 0).unwrap();
            let cid = r.diagnostics.next_id().unwrap();
            r.connections.insert(
                2,
                OwnedConnection {
                    transport: p.server,
                    socket: 0,
                    _permit: permit,
                    _socket_permit: None,
                    id: cid,
                    initial_cid: vec![],
                    cursors: [FairCursor::default(); 7],
                    receive_density: ReceiveDensity::default(),
                    enqueue_pending: false,
                    output_pending: false,
                },
            );
            let id = r.create_session(2, stream, None, Instant::now()).unwrap();
            // Deliver the peer FIN without delivering any server output/ACK.
            p.client
                .h3
                .as_mut()
                .unwrap()
                .send_body(&mut p.client.quic, stream, &[], true)
                .unwrap();
            let mut bytes = [0; 1350];
            while let Ok((n, info)) = p.client.quic.send(&mut bytes) {
                r.connections
                    .get_mut(&2)
                    .unwrap()
                    .transport
                    .receive(
                        &mut bytes[..n],
                        quiche::RecvInfo {
                            from: info.from,
                            to: info.to,
                        },
                        Instant::now(),
                    )
                    .unwrap();
            }
            r.reject(2, stream, 401);
            r.close_session(id, 0x10b);
            assert!(!r.sessions.contains_key(&id));
            assert_eq!(r.budgets.sessions.used().count, 1);
            assert_eq!(r.connections[&2].transport.cleanup_count(), 1);
            assert_eq!(r.budgets.relay_fds.used().count, 0);
            assert!(r.connections[&2].transport.quic.stream_closed(stream));
            r.connections
                .get_mut(&2)
                .unwrap()
                .transport
                .reap_stopped_streams();
            r.connections
                .get_mut(&2)
                .unwrap()
                .transport
                .reap_stopped_streams();
            assert_eq!(
                r.connections[&2].transport.cleanup_count(),
                1,
                "FIN on both sides is not native collection before ACK"
            );
            assert_eq!(r.budgets.sessions.used().count, 1);
            p.server = r.connections.remove(&2).unwrap().transport;
            for _ in 0..50 {
                p.pump();
                while let Ok((id, event)) = p.client.h3.as_mut().unwrap().poll(&mut p.client.quic) {
                    if id == stream
                        && let quiche::h3::Event::Headers { list, .. } = event
                    {
                        assert_eq!(masque::response(&list, false), Ok(Response::Rejected(401)));
                        p.server
                            .expire_native(Instant::now() + Duration::from_secs(61));
                        assert!(p.server.is_stopped());
                        assert_eq!(p.server.cleanup_count(), 0);
                        assert_eq!(r.budgets.sessions.used().count, 0);
                        return;
                    }
                }
            }
            panic!("rejection FIN response was discarded during flow cleanup");
        });
    }
    #[test]
    fn single_relay_read_requests_continuation_and_stale_setup_cannot_allocate() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let f = Fixtures::new();
            let cfg = ValidatedConfig::load(&f.file("server.toml")).unwrap();
            let resolver = SystemResolver::new(1).unwrap();
            let (diag, _receiver) =
                Diagnostics::new(Role::Server, Instant::now(), Duration::from_secs(1)).unwrap();
            let mut r = Runner::new(&cfg, &resolver, diag).unwrap();
            let mut p = Pair::new(true);
            let (a, _) = p.requests();
            let permit = r.budgets.connections.reserve(1, 0).unwrap();
            let cid = r.diagnostics.next_id().unwrap();
            r.connections.insert(
                2,
                OwnedConnection {
                    transport: p.server,
                    socket: 0,
                    _permit: permit,
                    _socket_permit: None,
                    id: cid,
                    initial_cid: vec![],
                    cursors: [FairCursor::default(); 7],
                    receive_density: ReceiveDensity::default(),
                    enqueue_pending: false,
                    output_pending: false,
                },
            );
            let now = Instant::now();
            let id = r.create_session(2, a, None, now).unwrap();
            let key = SetupKey {
                connection: crate::buffers::Generation(2),
                session: crate::buffers::Generation(id),
            };
            let stale = SetupKey {
                session: crate::buffers::Generation(id + 1),
                ..key
            };
            r.setup_done(id, stale, Err(PolicyError::Denied), now)
                .unwrap();
            assert!(r.sessions[&id].relay.is_none());
            assert_eq!(r.budgets.relay_fds.used().count, 0);
            let target = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
            let address = target.local_addr().unwrap();
            r.setup_done(
                id,
                key,
                Ok((0, policy::ResolvedTarget { address, key })),
                now,
            )
            .unwrap();
            let relay = r.sessions[&id]
                .relay
                .as_ref()
                .unwrap()
                .get_ref()
                .0
                .borrow()
                .local_addr();
            r.sessions.get_mut(&id).unwrap().state = State::Active;
            target.send_to(b"reply", relay).unwrap();
            assert!(
                r.relay_input(id, Instant::now()).unwrap(),
                "new native work must not wait for another edge"
            );
            assert_eq!(r.sessions[&id].queue.used().count, 1);
            r.schedule_datagrams(2, Instant::now()).unwrap();
            assert_eq!(r.connections[&2].transport.quic.dgram_send_queue_len(), 1);
            assert_eq!(r.snapshot.counters.app_ingress_by_direction.to_client, 1);
            assert_eq!(
                r.snapshot.counters.app_ingress_bytes_by_direction.to_client,
                5
            );
            for _ in 0..32 {
                r.queue_outgoing(id, b"bounded-round", Instant::now())
                    .unwrap();
            }
            assert_eq!(r.sessions[&id].queue.used().count, 32);
            assert!(!r.schedule_datagrams(2, Instant::now()).unwrap());
            assert_eq!(
                r.sessions[&id].queue.used().count,
                0,
                "one active flow uses exactly the 32-item packet quota"
            );
            assert_eq!(r.connections[&2].transport.quic.dgram_send_queue_len(), 33);
            assert!(!r.schedule_datagrams(2, Instant::now()).unwrap());
            assert_eq!(r.connections[&2].transport.quic.dgram_send_queue_len(), 33);

            struct Cancel(Rc<Cell<bool>>);
            impl Drop for Cancel {
                fn drop(&mut self) {
                    self.0.set(true);
                }
            }
            let cancelled = Rc::new(Cell::new(false));
            let guard = Cancel(cancelled.clone());
            r.pending.push(Pending {
                session: id,
                key,
                future: Box::pin(async move {
                    let _guard = guard;
                    std::future::pending().await
                }),
            });
            r.close_session(id, 0x100);
            assert!(cancelled.get());
            assert_eq!(r.pending.len(), 0);
            assert_eq!(r.budgets.relay_fds.used().count, 0);
            assert_eq!(r.connections[&2].transport.quic.dgram_send_queue_len(), 0);
            r.setup_done(
                id,
                key,
                Ok((0, policy::ResolvedTarget { address, key })),
                now,
            )
            .unwrap();
            assert!(r.sessions.is_empty());
            // Spoof/churn cannot grow source bookkeeping or mint unlimited Retry.
            let mut admitted = 0;
            for n in 0..4096 {
                let ip = std::net::Ipv4Addr::new(192, 0, (n / 256) as u8, (n % 256) as u8);
                admitted += usize::from(r.source_admit(SocketAddr::new(ip.into(), 12345), now));
            }
            assert!(admitted <= 64 * 16);
            assert_eq!(r.source_buckets.len(), 64);
        });
    }
    #[test]
    fn final_collection_waits_for_old_ack_and_exposes_blocked_deadline() {
        #[derive(Clone)]
        struct Capture(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
        impl io::Write for Capture {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let now = Instant::now();
            let (mut diag, receiver) =
                Diagnostics::new(Role::Server, now, Duration::from_secs(10)).unwrap();
            let id = diag.next_id().unwrap();
            diag.failure(now, None, Some(id), Failure::Authentication, 401, false)
                .unwrap();
            diag.snapshot(now, Snapshot::default()).unwrap();
            diag.failure(now, None, Some(id), Failure::Socket, 5, false)
                .unwrap();
            diag.finish_session(id).unwrap();
            let capture = Capture(Default::default());
            let writer = receiver.spawn(capture.clone(), io::sink()).unwrap();
            drain_terminal_status(&mut diag, Snapshot::default(), now + Duration::from_secs(1))
                .await
                .unwrap();
            assert_eq!(diag.pending_failures(), 0);
            drop(diag);
            assert!(writer.finish(Duration::from_secs(1)));
            let data = capture.0.lock().unwrap().clone();
            let rows: Vec<serde_json::Value> = data
                .split(|b| *b == b'\n')
                .filter(|b| !b.is_empty())
                .map(|b| serde_json::from_slice(b).unwrap())
                .collect();
            assert_eq!(rows.len(), 2);
            assert_eq!(rows[1]["failures"].as_array().unwrap().len(), 2);
            assert!(
                rows[1]["failures"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|f| f["terminal"] == true)
            );

            let now = Instant::now();
            let (mut diag, _blocked_receiver) =
                Diagnostics::new(Role::Server, now, Duration::from_secs(10)).unwrap();
            diag.failure(now, None, None, Failure::Socket, 5, true)
                .unwrap();
            diag.snapshot(now, Snapshot::default()).unwrap();
            let result = drain_terminal_status(
                &mut diag,
                Snapshot::default(),
                now + Duration::from_millis(20),
            )
            .await;
            assert!(matches!(
                result,
                Err(RuntimeError::DiagnosticCollectionIncomplete)
            ));
            assert_eq!(diag.pending_failures(), 1);
            assert!(now.elapsed() < Duration::from_millis(250));
        });
    }

    #[test]
    fn outer_pmtu_or_pressure_discards_only_failed_front_without_application_attribution() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        for errno in [
            libc::EMSGSIZE,
            libc::ENOBUFS,
            libc::ECONNREFUSED,
            libc::EHOSTUNREACH,
        ] {
            rt.block_on(async {
                let f = Fixtures::new();
                let cfg = ValidatedConfig::load(&f.file("client.toml")).unwrap();
                let resolver = SystemResolver::new(1).unwrap();
                let now = Instant::now();
                let (diag, _receiver) =
                    Diagnostics::new(Role::Client, now, Duration::from_secs(1)).unwrap();
                let mut r = Runner::new(&cfg, &resolver, diag).unwrap();
                let mut p = Pair::new(true);
                let (a, _) = p.requests();
                let generation = *r.connections.keys().next().unwrap();
                for index in 0..3 {
                    let at = now + Duration::from_millis(index);
                    p.client
                        .enqueue(a, &[index as u8], at, at, &mut [0; 1350])
                        .unwrap();
                    assert_eq!(p.client.generate(&p.pool, at, 1).unwrap(), 1);
                }
                let original: Vec<_> = p.client.output.iter().map(|p| p.bytes().to_vec()).collect();
                let due = p.client.output.iter().map(|p| p.at()).max().unwrap();
                r.connections.get_mut(&generation).unwrap().transport = p.client;
                let accepted = Rc::new(std::cell::RefCell::new(Vec::new()));
                let capture = accepted.clone();
                let mut calls = 0;
                r.outer_send_hook = Some(Box::new(move |packets| {
                    calls += 1;
                    if calls == 2 {
                        return Err(io::Error::from_raw_os_error(errno));
                    }
                    capture.borrow_mut().push(packets[0].bytes.to_vec());
                    Ok(1)
                }));
                assert!(!r.flush(generation, due).unwrap());
                assert!(r.flush(generation, due).unwrap());
                assert_eq!(
                    r.connections[&generation]
                        .transport
                        .output
                        .front()
                        .unwrap()
                        .bytes(),
                    original[2]
                );
                assert!(r.flush(generation, due).unwrap());
                assert_eq!(
                    *accepted.borrow(),
                    vec![original[0].clone(), original[2].clone()]
                );
                assert_eq!(r.snapshot.counters.outer_socket_sent, 2);
                assert_eq!(
                    r.snapshot.counters.outer_pmtu_errors,
                    u64::from(errno == libc::EMSGSIZE)
                );
                assert_eq!(
                    r.snapshot.counters.outer_send_pressure,
                    u64::from(errno == libc::ENOBUFS)
                );
                assert_eq!(r.snapshot.counters.local_drops.pmtu.to_target, 0);
                assert_eq!(r.snapshot.counters.local_drops.pmtu.to_client, 0);
                assert_eq!(r.snapshot.counters.kernel_rx_overflow, 0);
                assert!(!r.connections[&generation].transport.is_stopped());
            });
        }
    }

    #[test]
    fn shared_outer_socket_error_queue_suffix_cannot_stop_another_connection() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let f = Fixtures::new();
            let cfg = ValidatedConfig::load(&f.file("server.toml")).unwrap();
            let resolver = SystemResolver::new(1).unwrap();
            let now = Instant::now();
            let (diag, _receiver) =
                Diagnostics::new(Role::Server, now, Duration::from_secs(1)).unwrap();
            let mut r = Runner::new(&cfg, &resolver, diag).unwrap();
            for generation in [2, 3] {
                let mut p = Pair::new(true);
                let (stream, _) = p.requests();
                p.server.generation = crate::buffers::Generation(generation);
                r.connections.insert(
                    generation,
                    OwnedConnection {
                        transport: p.server,
                        socket: 0,
                        _permit: r.budgets.connections.reserve(1, 0).unwrap(),
                        _socket_permit: None,
                        id: r.diagnostics.next_id().unwrap(),
                        initial_cid: vec![],
                        cursors: [FairCursor::default(); 7],
                        receive_density: ReceiveDensity::default(),
                        enqueue_pending: false,
                        output_pending: false,
                    },
                );
                let id = r.create_session(generation, stream, None, now).unwrap();
                r.sessions.get_mut(&id).unwrap().state = State::Active;
            }
            let closed = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
            let old_peer = closed.local_addr().unwrap();
            drop(closed);
            let live = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
            live.set_read_timeout(Some(Duration::from_millis(100)))
                .unwrap();
            let local = r.outer[0].get_ref().0.borrow().local_addr();
            for _ in 0..6 {
                let mut socket = r.outer[0].get_ref().0.borrow_mut();
                let packet = [SendPacket {
                    bytes: b"old-peer",
                    from: local,
                    to: old_peer,
                }];
                match socket.send_batch(&packet) {
                    Ok(1) => {}
                    Err(e) if crate::udp::is_peer_error(&e) => {
                        assert_eq!(socket.send_batch(&packet).unwrap(), 1);
                    }
                    result => panic!("ICMP fixture submission: {result:?}"),
                }
            }
            let _ready = tokio::time::timeout(
                Duration::from_secs(1),
                r.outer[0].ready(tokio::io::Interest::ERROR),
            )
            .await
            .unwrap()
            .unwrap();
            // Consume one bounded visit only. The remaining error is still for
            // A, but the next native send belongs to connection B.
            let first = r.outer[0].get_ref().0.borrow_mut().drain_errors().unwrap();
            assert!(first.pending);
            assert_eq!(first.errors.iter().flatten().count(), 4);
            assert!(
                first
                    .errors
                    .iter()
                    .flatten()
                    .all(|e| e.destination == old_peer && e.errno == libc::ECONNREFUSED as u32)
            );
            for (sequence, payload) in [b"unaccepted".as_slice(), b"fresh".as_slice()]
                .into_iter()
                .enumerate()
            {
                let mut slab = r.pools.ciphertext.acquire().unwrap();
                slab.bytes_mut()[..payload.len()].copy_from_slice(payload);
                let packet = crate::buffers::Ciphertext::new(
                    slab,
                    payload.len(),
                    local,
                    live.local_addr().unwrap(),
                    now,
                    crate::buffers::Generation(3),
                    sequence as u64,
                    now,
                    now + Duration::from_secs(2),
                )
                .unwrap();
                r.connections
                    .get_mut(&3)
                    .unwrap()
                    .transport
                    .output
                    .push_back(packet);
            }
            assert!(r.flush(3, now).unwrap());
            assert_eq!(r.snapshot.counters.outer_async_errors, 1);
            assert_eq!(r.connections[&3].transport.output.len(), 1);
            r.receive_outer(0, now).unwrap();
            assert!(r.flush(3, now).unwrap());
            assert_eq!(r.snapshot.counters.outer_socket_sent, 1);
            assert_eq!(r.sessions.len(), 2);
            assert!(r.sessions.values().all(|s| s.state == State::Active));
            assert!(r.connections.values().all(|c| !c.transport.is_stopped()));
            let mut bytes = [0; 64];
            let (n, from) = live.recv_from(&mut bytes).unwrap();
            assert_eq!(&bytes[..n], b"fresh");
            assert_eq!(from, local);
            assert!(
                live.recv_from(&mut bytes).is_err(),
                "unaccepted data was replayed"
            );
            r.outer_send_hook = Some(Box::new(|_| Err(io::Error::from_raw_os_error(libc::EBADF))));
            let mut slab = r.pools.ciphertext.acquire().unwrap();
            slab.bytes_mut()[0] = 42;
            r.connections
                .get_mut(&3)
                .unwrap()
                .transport
                .output
                .push_back(
                    crate::buffers::Ciphertext::new(
                        slab,
                        1,
                        local,
                        live.local_addr().unwrap(),
                        now,
                        crate::buffers::Generation(3),
                        2,
                        now,
                        now + Duration::from_secs(2),
                    )
                    .unwrap(),
                );
            assert_eq!(r.flush(3, now), Err(RuntimeError::SocketErrno(libc::EBADF)));
        });
    }

    #[test]
    fn production_flush_submits_a_bounded_sixteen_packet_prefix() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let f = Fixtures::new();
            let cfg = ValidatedConfig::load(&f.file("client.toml")).unwrap();
            let resolver = SystemResolver::new(1).unwrap();
            let now = Instant::now();
            let (diag, _rx) = Diagnostics::new(Role::Client, now, Duration::from_secs(1)).unwrap();
            let mut r = Runner::new(&cfg, &resolver, diag).unwrap();
            let id = *r.connections.keys().next().unwrap();
            let local = r.outer[r.connections[&id].socket]
                .get_ref()
                .0
                .borrow()
                .local_addr();
            let sink = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
            sink.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
            for sequence in 0..20 {
                let mut slab = r.pools.ciphertext.acquire().unwrap();
                slab.bytes_mut()[0] = sequence as u8;
                let packet = crate::buffers::Ciphertext::new(
                    slab,
                    1,
                    local,
                    sink.local_addr().unwrap(),
                    now,
                    r.connections[&id].transport.generation,
                    sequence,
                    now,
                    now + Duration::from_secs(2),
                )
                .unwrap();
                r.connections
                    .get_mut(&id)
                    .unwrap()
                    .transport
                    .output
                    .push_back(packet);
            }
            assert!(r.flush(id, now).unwrap());
            assert_eq!(
                r.snapshot.counters.outer_socket_sent, 16,
                "physical submission must keep pace with the generation visit"
            );
            assert_eq!(r.connections[&id].transport.output.len(), 4);
            let mut bytes = [0; 16];
            for sequence in 0..16 {
                let (n, _) = sink.recv_from(&mut bytes).unwrap();
                assert_eq!(&bytes[..n], &[sequence]);
            }
            sink.set_nonblocking(true).unwrap();
            assert_eq!(
                sink.recv_from(&mut bytes).unwrap_err().kind(),
                io::ErrorKind::WouldBlock
            );
            assert!(r.flush(id, now).unwrap());
            assert_eq!(r.snapshot.counters.outer_socket_sent, 20);
            assert!(r.connections[&id].transport.output.is_empty());
        });
    }
    #[test]
    fn production_flush_keeps_mixed_ciphertext_prefix_metadata_and_unrelated_flow() {
        use crate::common::{IoStep, ScriptedIo};
        use std::cell::RefCell;
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let f = Fixtures::new();
            let cfg = ValidatedConfig::load(&f.file("client.toml")).unwrap();
            let resolver = SystemResolver::new(1).unwrap();
            let now = Instant::now();
            let (diag, _receiver) =
                Diagnostics::new(Role::Client, now, Duration::from_secs(1)).unwrap();
            let mut r = Runner::new(&cfg, &resolver, diag).unwrap();
            let mut p = Pair::new(true);
            let (a, b) = p.requests();
            let generation = *r.connections.keys().next().unwrap();
            assert_eq!(p.client.generation.0, generation);
            let sender = "127.0.0.1:42000".parse().unwrap();
            let local = "127.0.0.2:41000".parse().unwrap();
            let sa = r
                .create_session(generation, a, Some((sender, local)), now)
                .unwrap();
            let sb = r
                .create_session(
                    generation,
                    b,
                    Some(("127.0.0.1:42001".parse().unwrap(), local)),
                    now,
                )
                .unwrap();
            r.sessions.get_mut(&sa).unwrap().state = State::Active;
            r.sessions.get_mut(&sb).unwrap().state = State::Active;
            for index in 0..3 {
                let at = now + Duration::from_millis(index);
                p.client
                    .enqueue(a, &[b'a', index as u8], at, at, &mut [0; 1350])
                    .unwrap();
                p.client
                    .enqueue(b, &[b'b', index as u8], at, at, &mut [0; 1350])
                    .unwrap();
                assert_eq!(p.client.generate(&p.pool, at, 1).unwrap(), 1);
            }
            r.connections.get_mut(&generation).unwrap().transport = p.client;
            let describe = |packet: &crate::buffers::Ciphertext| {
                (
                    packet.bytes().to_vec(),
                    packet.from(),
                    packet.to(),
                    packet.at(),
                    packet.connection(),
                    packet.sequence(),
                    packet.generated_at(),
                    packet.expires_at(),
                )
            };
            let original: Vec<_> = r.connections[&generation]
                .transport
                .output
                .iter()
                .map(&describe)
                .collect();
            assert_eq!(original.len(), 3);
            let flush_at = original.iter().map(|p| p.3).max().unwrap();
            // Work before the pacing deadline must not even attempt a syscall.
            r.outer_send_hook = Some(Box::new(|_| panic!("early pacing submission")));
            assert!(
                !r.flush(generation, original[0].3 - Duration::from_nanos(1))
                    .unwrap()
            );
            r.close_session(sa, 0x100);
            assert!(!r.sessions.contains_key(&sa));
            assert_eq!(r.sessions[&sb].state, State::Active);
            assert_eq!(
                r.connections[&generation]
                    .transport
                    .output
                    .iter()
                    .map(&describe)
                    .collect::<Vec<_>>(),
                original
            );

            let io = Rc::new(RefCell::new(ScriptedIo::new([
                IoStep::Interrupted,
                IoStep::Accepted(1),
                IoStep::WouldBlock,
                IoStep::HardError,
                IoStep::Accepted(1),
                IoStep::Accepted(1),
            ])));
            let accepted = Rc::new(RefCell::new(Vec::new()));
            let script = io.clone();
            let capture = accepted.clone();
            r.outer_send_hook = Some(Box::new(move |packets| {
                let n = script.borrow_mut().send(packets.len())?;
                capture.borrow_mut().extend(
                    packets[..n]
                        .iter()
                        .map(|p| (p.bytes.to_vec(), p.from, p.to)),
                );
                Ok(n)
            }));
            for (step, remaining) in [3, 2, 2, 2, 1, 0].into_iter().enumerate() {
                let outcome = r.flush(generation, flush_at);
                if step == 3 {
                    assert!(matches!(outcome, Err(RuntimeError::Socket)));
                } else {
                    assert!(outcome.is_ok());
                }
                let suffix: Vec<_> = r.connections[&generation]
                    .transport
                    .output
                    .iter()
                    .map(&describe)
                    .collect();
                assert_eq!(suffix, original[3 - remaining..]);
                assert_eq!(r.sessions[&sb].state, State::Active);
            }
            assert_eq!(io.borrow().accepted(), 3);
            assert_eq!(r.snapshot.counters.outer_socket_sent, 3);
            assert_eq!(
                *accepted.borrow(),
                original
                    .iter()
                    .map(|p| (p.0.clone(), p.1, p.2))
                    .collect::<Vec<_>>()
            );
            // Deliver the captured, accepted ciphertext to the real peer:
            // every committed mixed packet must still contain B's datagram.
            for (bytes, from, to) in accepted.borrow_mut().iter_mut() {
                p.server
                    .receive(
                        bytes,
                        quiche::RecvInfo {
                            from: *from,
                            to: *to,
                        },
                        flush_at,
                    )
                    .unwrap();
            }
            let mut seen_b = Vec::new();
            let mut bytes = [0; 1350];
            while let Ok(n) = p.server.quic.dgram_recv(&mut bytes) {
                let d = masque::decode_datagram(&bytes[..n]).unwrap().unwrap();
                if d.stream == b {
                    seen_b.push(d.payload.to_vec());
                }
            }
            assert_eq!(seen_b, vec![vec![b'b', 0], vec![b'b', 1], vec![b'b', 2]]);
            r.connections
                .get_mut(&generation)
                .unwrap()
                .transport
                .enqueue(
                    b,
                    b"unrelated-flow-continues",
                    flush_at,
                    flush_at,
                    &mut [0; 1350],
                )
                .unwrap();
        });
    }
    #[test]
    fn read_continuation_stops_empty_probes_and_rearms_on_native_readiness() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let f = Fixtures::new();
            let cfg = ValidatedConfig::load(&f.file("client.toml")).unwrap();
            let resolver = SystemResolver::new(1).unwrap();
            let now = Instant::now();
            let (diag, _receiver) =
                Diagnostics::new(Role::Client, now, Duration::from_secs(1)).unwrap();
            let mut r = Runner::new(&cfg, &resolver, diag).unwrap();
            assert!(!r.client_input(now).unwrap());
            let before = r
                .listener
                .as_ref()
                .unwrap()
                .get_ref()
                .0
                .borrow()
                .stats()
                .receive_syscalls;
            assert_eq!(before, 1);
            for _ in 0..16 {
                assert!(!r.client_input(now).unwrap());
            }
            assert_eq!(
                r.listener
                    .as_ref()
                    .unwrap()
                    .get_ref()
                    .0
                    .borrow()
                    .stats()
                    .receive_syscalls,
                before
            );
            let sender = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
            let address = r
                .listener
                .as_ref()
                .unwrap()
                .get_ref()
                .0
                .borrow()
                .local_addr();
            for _ in 0..64 {
                sender.send_to(&[7; 128], address).unwrap();
            }
            // Exercise the real readiness edge used by the owner wait phase.
            let socket = r.listener.as_ref().unwrap();
            let guard = tokio::time::timeout(Duration::from_secs(1), socket.readable())
                .await
                .unwrap()
                .unwrap();
            socket.get_ref().set_read_pending(true);
            drop(guard);
            assert!(r.client_input(Instant::now()).unwrap());
            assert!(
                r.client_input(Instant::now()).unwrap(),
                "quota exhaustion must continue without another edge"
            );
            assert!(!r.client_input(Instant::now()).unwrap());
            assert_eq!(r.snapshot.counters.app_ingress_datagrams, 64);
            let after = r
                .listener
                .as_ref()
                .unwrap()
                .get_ref()
                .0
                .borrow()
                .stats()
                .receive_syscalls;
            assert_eq!(after - before, 3);
            assert!(!r.client_input(Instant::now()).unwrap());
            assert_eq!(
                r.listener
                    .as_ref()
                    .unwrap()
                    .get_ref()
                    .0
                    .borrow()
                    .stats()
                    .receive_syscalls,
                after
            );
        });
    }

    #[test]
    fn owner_expires_native_input_while_congestion_window_is_blocked() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let f = Fixtures::new();
            let cfg = ValidatedConfig::load(&f.file("client.toml")).unwrap();
            let resolver = SystemResolver::new(1).unwrap();
            let now = Instant::now();
            let (diag, _receiver) =
                Diagnostics::new(Role::Client, now, Duration::from_secs(1)).unwrap();
            let mut r = Runner::new(&cfg, &resolver, diag).unwrap();
            let mut p = Pair::new(true);
            let (a, _) = p.requests();
            let generation = *r.connections.keys().next().unwrap();
            let mut blocked = false;
            for _ in 0..64 {
                p.client
                    .enqueue(a, &[7; 1000], now, now, &mut [0; 1350])
                    .unwrap();
                if p.client.generate(&p.pool, now, 1).unwrap() == 0 {
                    blocked = true;
                    break;
                }
            }
            assert!(
                blocked,
                "peer must withhold ACKs to exhaust the send window"
            );
            assert!(p.client.quic.dgram_send_queue_len() > 0);
            let committed: Vec<_> = p
                .client
                .output
                .iter()
                .map(|p| (p.sequence(), p.bytes().to_vec(), p.expires_at()))
                .collect();
            r.connections.get_mut(&generation).unwrap().transport = p.client;
            let expired = now + Duration::from_millis(cfg.limits().application_residence_ms);
            r.service_transport_timers(expired).unwrap();
            let c = &r.connections[&generation].transport;
            assert_eq!(c.quic.dgram_send_queue_len(), 0);
            assert_eq!(
                c.output
                    .iter()
                    .map(|p| (p.sequence(), p.bytes().to_vec(), p.expires_at()))
                    .collect::<Vec<_>>(),
                committed
            );
            assert!(r.snapshot.counters.local_drops.queue_expired.to_target > 0);
        });
    }
    #[test]
    fn relay_peer_error_on_immediate_or_queued_send_keeps_same_association_without_replay() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        for queued in [false, true] {
            rt.block_on(async {
                let f = Fixtures::new();
                let cfg = ValidatedConfig::load(&f.file("server.toml")).unwrap();
                let resolver = SystemResolver::new(1).unwrap();
                let now = Instant::now();
                let (diag, _receiver) =
                    Diagnostics::new(Role::Server, now, Duration::from_secs(1)).unwrap();
                let mut r = Runner::new(&cfg, &resolver, diag).unwrap();
                let mut p = Pair::new(true);
                let (stream, _) = p.requests();
                r.connections.insert(
                    2,
                    OwnedConnection {
                        transport: p.server,
                        socket: 0,
                        _permit: r.budgets.connections.reserve(1, 0).unwrap(),
                        _socket_permit: None,
                        id: r.diagnostics.next_id().unwrap(),
                        initial_cid: vec![],
                        cursors: [FairCursor::default(); 7],
                        receive_density: ReceiveDensity::default(),
                        enqueue_pending: false,
                        output_pending: false,
                    },
                );
                let id = r.create_session(2, stream, None, now).unwrap();
                let key = SetupKey {
                    connection: crate::buffers::Generation(2),
                    session: crate::buffers::Generation(id),
                };
                let target = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
                let address = target.local_addr().unwrap();
                r.setup_done(
                    id,
                    key,
                    Ok((0, policy::ResolvedTarget { address, key })),
                    now,
                )
                .unwrap();
                r.sessions.get_mut(&id).unwrap().state = State::Active;
                drop(target);
                let mut wire = [0; 128];
                let len = masque::encode_datagram(stream, b"old-target", &mut wire).unwrap();
                let expires = now + Duration::from_secs(2);
                r.datagram(2, &wire[..len], expires, now).unwrap();
                assert_eq!(
                    r.snapshot.counters.app_egress_by_direction.to_target, 1,
                    "initial submission"
                );
                // POLLERR is readiness too. Leave the native error undrained so
                // send, rather than the receive/error-queue path, consumes it.
                let relay = r.sessions[&id].relay.as_ref().unwrap();
                let _ready = tokio::time::timeout(
                    Duration::from_secs(1),
                    relay.ready(tokio::io::Interest::ERROR),
                )
                .await
                .unwrap()
                .unwrap();
                let target = std::net::UdpSocket::bind(address).unwrap();
                target
                    .set_read_timeout(Some(Duration::from_millis(100)))
                    .unwrap();
                let relay_local = relay.get_ref().0.borrow().local_addr();
                if queued {
                    for payload in [b"unaccepted".as_slice(), b"fresh".as_slice()] {
                        let mut slab = r.pools.application.acquire().unwrap();
                        slab.bytes_mut()[..payload.len()].copy_from_slice(payload);
                        let s = r.sessions.get_mut(&id).unwrap();
                        s.queue
                            .push(
                                crate::buffers::Datagram::new(
                                    slab,
                                    0,
                                    payload.len(),
                                    Direction::ToTarget,
                                    s.generation,
                                    s.connection,
                                    now,
                                    expires,
                                )
                                .unwrap(),
                            )
                            .unwrap();
                    }
                    r.flush_relay(id, now).unwrap();
                } else {
                    let len = masque::encode_datagram(stream, b"unaccepted", &mut wire).unwrap();
                    r.datagram(2, &wire[..len], expires, now).unwrap();
                    assert!(
                        r.sessions.contains_key(&id),
                        "ICMP on immediate send closed the association"
                    );
                    let len = masque::encode_datagram(stream, b"fresh", &mut wire).unwrap();
                    r.datagram(2, &wire[..len], expires, now).unwrap();
                }
                assert!(
                    r.sessions.contains_key(&id),
                    "ICMP on queued send closed the association"
                );
                assert_eq!(r.sessions[&id].relay_peer, Some(address));
                assert_eq!(r.sessions[&id].state, State::Active);
                assert_eq!(r.sessions[&id].queue.used().count, 0);
                let mut bytes = [0; 128];
                let (len, from) = target.recv_from(&mut bytes).unwrap();
                assert_eq!(&bytes[..len], b"fresh");
                assert_eq!(from, relay_local);
                assert!(
                    target.recv_from(&mut bytes).is_err(),
                    "failed packet was replayed"
                );
                assert_eq!(r.snapshot.counters.local_drops.socket_error.to_target, 1);
                assert_eq!(r.snapshot.counters.app_egress_by_direction.to_target, 2);
            });
        }
    }

    #[test]
    fn queued_wildcard_reply_uses_immutable_local_destination() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let f = Fixtures::new();
            let text = std::fs::read_to_string(f.file("client.toml"))
                .unwrap()
                .replacen("listen = \"127.0.0.1:", "listen = \"0.0.0.0:", 1)
                .replace("allow_non_loopback = false", "allow_non_loopback = true");
            std::fs::write(f.file("client.toml"), text).unwrap();
            let cfg = ValidatedConfig::load(&f.file("client.toml")).unwrap();
            let resolver = SystemResolver::new(1).unwrap();
            let now = Instant::now();
            let (diag, _receiver) =
                Diagnostics::new(Role::Client, now, Duration::from_secs(1)).unwrap();
            let mut r = Runner::new(&cfg, &resolver, diag).unwrap();
            let generation = *r.connections.keys().next().unwrap();
            let sender = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
            sender
                .set_read_timeout(Some(Duration::from_millis(100)))
                .unwrap();
            let mut local = r
                .listener
                .as_ref()
                .unwrap()
                .get_ref()
                .0
                .borrow()
                .local_addr();
            for (stream, ip) in [(0, "127.0.0.2"), (4, "127.0.0.3")] {
                local.set_ip(ip.parse().unwrap());
                sender.connect(local).unwrap();
                let id = r
                    .create_session(
                        generation,
                        stream,
                        Some((sender.local_addr().unwrap(), local)),
                        now,
                    )
                    .unwrap();
                let mut slab = r.pools.application.acquire().unwrap();
                let payload = ip.as_bytes();
                slab.bytes_mut()[..payload.len()].copy_from_slice(payload);
                let s = r.sessions.get_mut(&id).unwrap();
                s.state = State::Active;
                s.queue
                    .push(
                        crate::buffers::Datagram::new(
                            slab,
                            0,
                            payload.len(),
                            Direction::ToClient,
                            s.generation,
                            s.connection,
                            now,
                            now + Duration::from_millis(20),
                        )
                        .unwrap(),
                    )
                    .unwrap();
                r.flush_relay(id, now).unwrap();
                let mut bytes = [0; 128];
                let n = sender.recv(&mut bytes).unwrap();
                assert_eq!(&bytes[..n], payload);
                assert_eq!(r.sessions[&id].sender_local, Some(local));
                assert_eq!(r.sessions[&id].queue.used().count, 0);
            }
            assert_eq!(r.senders.len(), 2);
        });
    }
}

#[cfg(test)]
#[path = "../tests/lifecycle/recovery.rs"]
mod recovery_tests;
#[cfg(test)]
#[path = "../tests/lifecycle/sessions.rs"]
mod session_tests;
#[cfg(test)]
#[path = "../tests/lifecycle/udp_reuse.rs"]
mod udp_reuse_tests;
