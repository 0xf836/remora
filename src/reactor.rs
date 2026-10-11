//! One current-thread owner. Finite visits precede timer/cancellation work.
use crate::{
    buffers::{Direction, Generations, Pools, ResourceBudgets, SessionQueue},
    config::{RoleConfig, ValidatedConfig},
    diagnostics::{Diagnostics, Event, Failure, Role, Snapshot},
    masque::{self, CapsuleParser, Response, WireError},
    policy::{self, PolicyError, SetupKey, SystemResolver},
    session::{MappingLatch, PollSocket, Session, State},
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
    id: crate::diagnostics::OpaqueId,
    initial_cid: Vec<u8>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeError {
    Bind,
    Socket,
    Transport,
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
    pools: Pools,
    diagnostics: Diagnostics,
    snapshot: Snapshot,
    latch: MappingLatch,
    retry: transport::Retry,
    source_buckets: [SourceBucket; 64],
    turn: usize,
    setup_cursor: usize,
    next_snapshot: Instant,
    stopping: bool,
    ever_active: bool,
    #[cfg(test)]
    outer_send_hook: Option<OuterSendHook>,
}
fn native_drop(error: TransportError) -> crate::diagnostics::DropReason {
    use crate::diagnostics::DropReason;
    match error {
        TransportError::Oversized => DropReason::Oversized,
        TransportError::Capacity => DropReason::QueueFull,
        TransportError::Expired => DropReason::QueueExpired,
        TransportError::Stopped => DropReason::StaleGeneration,
        _ => DropReason::Malformed,
    }
}
fn resource<T, E>(result: Result<T, E>) -> Result<T, RuntimeError> {
    result.map_err(|_| RuntimeError::Resource)
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
                let address = if c.server_address.is_ipv4() {
                    "0.0.0.0:0"
                } else {
                    "[::]:0"
                };
                let mut socket = Udp::bind(
                    address.parse().map_err(|_| RuntimeError::Bind)?,
                    cfg.io(),
                    false,
                )
                .map_err(|_| RuntimeError::Bind)?;
                socket
                    .connect(c.server_address)
                    .map_err(|_| RuntimeError::Socket)?;
                outer.push(PollSocket::register(socket).map_err(|_| RuntimeError::Socket)?);
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
            pools,
            diagnostics,
            snapshot: Snapshot::default(),
            latch: MappingLatch::default(),
            retry: transport::Retry::new(now).map_err(|_| RuntimeError::Resource)?,
            source_buckets: std::array::from_fn(|_| SourceBucket {
                last: now,
                credits: 16,
            }),
            turn: 0,
            setup_cursor: 0,
            next_snapshot: now + Duration::from_millis(cfg.diagnostics().snapshot_interval_ms),
            stopping: false,
            ever_active: false,
            #[cfg(test)]
            outer_send_hook: None,
        };
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
        if let RoleConfig::Client(c) = cfg.role() {
            let cid = transport::random_cid().map_err(|_| RuntimeError::Resource)?;
            let permit = resource(runner.budgets.connections.reserve(1, 0))?;
            let generation = resource(runner.generations.next_generation())?;
            let local = runner.outer[0].get_ref().0.borrow().local_addr();
            let q = quiche::connect_with_buffer_factory::<TrackedFactory>(
                Some(&c.server_name),
                &quiche::ConnectionId::from_ref(&cid),
                local,
                c.server_address,
                &mut runner.quic_config,
            )
            .map_err(|_| RuntimeError::Transport)?;
            let id = runner
                .diagnostics
                .next_id()
                .map_err(|_| RuntimeError::Diagnostics)?;
            runner.connections.insert(
                generation.0,
                OwnedConnection {
                    transport: Connection::new(q, generation, false, cfg, now),
                    socket: 0,
                    _permit: permit,
                    id,
                    initial_cid: cid.to_vec(),
                },
            );
        }
        Ok(runner)
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
    fn close_session(&mut self, generation: u64, code: u64) {
        self.retire_session(generation, code, true);
    }
    fn retire_session(&mut self, generation: u64, code: u64, abort_write: bool) {
        if let Some(mut s) = self.sessions.remove(&generation) {
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
            if let (Some(sender), Some(local)) = (s.sender, s.sender_local) {
                self.senders.remove(&(sender, local));
            }
            if let Some(c) = self.connections.get_mut(&s.connection.0)
                && s.stream != u64::MAX
            {
                if abort_write {
                    c.transport.close_stream(s.stream, code);
                } else {
                    c.transport.purge_stream_datagrams(s.stream);
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
            let _ = c
                .transport
                .quic
                .stream_shutdown(stream, quiche::Shutdown::Read, 0x10b);
        }
        self.failure(
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
            self.lookup(connection, stream)
                .and_then(|id| self.sessions.get(&id).map(|s| s.id)),
            self.connections.get(&connection).map(|c| c.id),
        );
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
        if self
            .sessions
            .values()
            .filter(|s| s.connection.0 == connection)
            .count()
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
                body_pending: false,
                capsule_counts: crate::masque::CapsuleCounts::default(),
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
            let q = quiche::accept_with_retry::<TrackedFactory>(
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
            self.connections.insert(
                generation.0,
                OwnedConnection {
                    transport: Connection::new(q, generation, true, self.cfg, now),
                    socket: index,
                    _permit: permit,
                    id,
                    initial_cid: header.dcid.as_ref().to_vec(),
                },
            );
            generation.0
        };
        if let Some(c) = self.connections.get_mut(&connection) {
            match c.transport.receive(
                bytes,
                quiche::RecvInfo {
                    from: meta.peer,
                    to: meta.local,
                },
                now,
            ) {
                Ok(_) | Err(quiche::Error::Done) => {}
                Err(error) => {
                    self.failure(
                        if matches!(error, quiche::Error::TlsFail) {
                            Failure::Trust
                        } else {
                            Failure::Transport
                        },
                        0,
                        None,
                        self.connections.get(&connection).map(|c| c.id),
                    );
                    if !self.server && matches!(error, quiche::Error::TlsFail) {
                        return Err(RuntimeError::Transport);
                    }
                }
            }
        }
        Ok(())
    }
    fn receive_outer(&mut self, index: usize, now: Instant) -> Result<bool, RuntimeError> {
        let mut slabs = Vec::with_capacity(self.cfg.io().batch_max.min(32));
        for _ in 0..self.cfg.io().batch_max.min(32) {
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
        let result = self.outer[index]
            .get_ref()
            .0
            .borrow_mut()
            .recv_batch(&mut slabs, &mut packets);
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
                            Direction::ToTarget,
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
                Ok(n > 0)
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                if let Ok(mut guard) = self.outer[index]
                    .try_io(tokio::io::Interest::READABLE, |_| {
                        Err::<(), _>(io::ErrorKind::WouldBlock.into())
                    })
                {
                    let _ = &mut guard;
                }
                Ok(false)
            }
            Err(_) => Err(RuntimeError::Socket),
        }
    }
    fn client_input(&mut self, now: Instant) -> Result<bool, RuntimeError> {
        let Some(listener) = self.listener.as_ref() else {
            return Ok(false);
        };
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
        let result = listener
            .get_ref()
            .0
            .borrow_mut()
            .recv_batch(&mut slabs, &mut packets);
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
                    if p.fault.is_some() {
                        self.drop_packet(
                            crate::diagnostics::DropReason::SocketTruncated,
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
                        let Some(connection) = self.connections.keys().next().copied() else {
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
                            crate::diagnostics::DropReason::NotActive,
                            Direction::ToTarget,
                        )?;
                        continue;
                    }
                    let connection = s.connection.0;
                    let stream = s.stream;
                    let mut scratch = resource(self.pools.scratch.acquire())?;
                    let result = self
                        .connections
                        .get_mut(&connection)
                        .ok_or(RuntimeError::Transport)?
                        .transport
                        .enqueue(
                            stream,
                            &slabs[p.index].bytes()[..p.length],
                            now,
                            now,
                            scratch.bytes_mut(),
                        );
                    if result.is_ok() {
                        if let Some(s) = self.sessions.get_mut(&id) {
                            s.activity = now;
                        }
                        self.snapshot.counters.quic_dgram_enqueued = self
                            .snapshot
                            .counters
                            .quic_dgram_enqueued
                            .checked_add(1)
                            .ok_or(RuntimeError::Diagnostics)?;
                    } else if let Err(error) = result {
                        self.drop_packet(native_drop(error), Direction::ToTarget)?;
                    }
                }
                Ok(n > 0)
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                let _ = listener.try_io(tokio::io::Interest::READABLE, |_| {
                    Err::<(), _>(io::ErrorKind::WouldBlock.into())
                });
                Ok(false)
            }
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
            if self.lookup(connection, stream).is_some() {
                if let Some(c) = self.connections.get_mut(&connection) {
                    c.transport.close_stream(stream, masque::H3_MESSAGE_ERROR);
                }
                return Ok(());
            }
            let request = match masque::request(&list, &server.proxy_authority, &server.path_prefix)
            {
                Ok(r) => r,
                Err(WireError::Status(code)) => {
                    self.reject(connection, stream, code);
                    return Ok(());
                }
                Err(_) => {
                    if let Some(c) = self.connections.get_mut(&connection) {
                        c.transport.close_stream(stream, masque::H3_MESSAGE_ERROR);
                    }
                    return Ok(());
                }
            };
            if !more {
                self.reject(connection, stream, 400);
                return Ok(());
            }
            let id = match self.create_session(connection, stream, None, now) {
                Ok(id) => id,
                Err(_) => {
                    self.reject(connection, stream, 503);
                    return Ok(());
                }
            };
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
                    policy::prepare(
                        identities,
                        &request.token,
                        &request.host,
                        request.port,
                        resolver,
                        key,
                        timeout,
                    )
                    .await
                }),
            });
        } else if let Some(id) = self.lookup(connection, stream) {
            let result = masque::response(&list, !more);
            match result {
                Ok(Response::Informational) => {}
                Ok(Response::Accepted) => {
                    if let Some(s) = self.sessions.get_mut(&id) {
                        if s.state == State::AwaitingAcceptance {
                            self.ever_active = true;
                            s.state = State::Active;
                            s.activity = now;
                            s._setup = None;
                        } else {
                            self.close_session(id, masque::H3_MESSAGE_ERROR);
                        }
                    }
                }
                Ok(Response::Rejected(code)) => {
                    if [400, 401, 403, 404, 405].contains(&code) {
                        self.latch.reject();
                    }
                    self.failure(
                        if code == 401 {
                            Failure::Authentication
                        } else {
                            Failure::Authorization
                        },
                        u64::from(code),
                        self.sessions.get(&id).map(|s| s.id),
                        self.connections.get(&connection).map(|c| c.id),
                    );
                    self.close_session(id, 0x10b);
                    if !self.ever_active {
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
        if s.generation != key.session || s.connection != key.connection {
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
        if self
            .sessions
            .values()
            .filter(|s| s.identity == Some(identity))
            .count()
            >= self.cfg.limits().max_sessions_per_identity
        {
            self.reject(connection, stream, 503);
            self.close_session(id, 0x10b);
            return Ok(());
        }
        let address = target
            .for_current(key)
            .map_err(|_| RuntimeError::Transport)?;
        let relay = match self.budgets.relay_fds.reserve(1, 0) {
            Ok(r) => r,
            Err(_) => {
                self.reject(connection, stream, 503);
                self.close_session(id, 0x10b);
                return Ok(());
            }
        };
        let fd = resource(self.budgets.fds.reserve(1, 0))?;
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
        let socket = PollSocket::register(socket).map_err(|_| RuntimeError::Socket)?;
        let s = self.sessions.get_mut(&id).ok_or(RuntimeError::Transport)?;
        s.identity = Some(identity);
        s.relay = Some(socket);
        s.relay_peer = Some(address);
        s._relay = Some(relay);
        s._fd = Some(fd);
        s.state = State::AwaitingCommitment;
        s.activity = now;
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
        let result = self
            .connections
            .get_mut(&connection)
            .ok_or(RuntimeError::Transport)?
            .transport
            .negotiate();
        match result {
            Ok(value) => Ok(value),
            Err(TransportError::Capability) => {
                if !self.server {
                    self.latch.reject();
                }
                Err(RuntimeError::Capability)
            }
            Err(TransportError::Tls) => Err(RuntimeError::Trust),
            Err(_) => Err(RuntimeError::Transport),
        }
    }
    fn h3_work(&mut self, connection: u64, now: Instant) -> Result<bool, RuntimeError> {
        let ready = self.negotiate_transport(connection)?;
        if self
            .connections
            .get(&connection)
            .is_none_or(|c| c.transport.h3.is_none())
        {
            return Ok(false);
        }
        if ready && !self.server {
            let ids: Vec<_> = self
                .sessions
                .iter()
                .filter_map(|(id, s)| {
                    (s.connection.0 == connection && s.state == State::Starting).then_some(*id)
                })
                .collect();
            for id in ids.into_iter().take(4) {
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
                        let s = self.sessions.get_mut(&id).ok_or(RuntimeError::Transport)?;
                        s.stream = stream;
                        s.state = State::AwaitingAcceptance;
                    }
                    Err(quiche::h3::Error::StreamBlocked) => {}
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
                .take(4)
                .map(|(id, _)| *id)
                .collect();
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
                        return Err(RuntimeError::Transport);
                    }
                    self.headers(connection, stream, list, more_frames, now)?
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
                Err(_) => return Err(RuntimeError::Transport),
            }
            serviced += 1;
        }
        let body_ids: Vec<_> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.connection.0 == connection && s.body_pending)
            .take(4)
            .map(|(id, _)| *id)
            .collect();
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
        if self.server {
            for s in self
                .sessions
                .values_mut()
                .filter(|s| s.connection.0 == connection && s.state == State::AwaitingCommitment)
            {
                if self.connections.get_mut(&connection).is_some_and(|c| {
                    !c.transport.quic.stream_closed(s.stream)
                        && c.transport.quic.stream_capacity(s.stream).is_ok()
                }) && s.receipt.as_ref().is_some_and(|r| r.strong_count() == 0)
                {
                    s.receipt = None;
                    s.state = State::Active;
                    s._setup = None;
                }
            }
        }
        let ids: Vec<_> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.connection.0 == connection && s.state == State::Active)
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            let stream = self.sessions[&id].stream;
            let c = self
                .connections
                .get_mut(&connection)
                .ok_or(RuntimeError::Transport)?;
            if c.transport.quic.stream_capacity(stream).is_err() {
                self.close_session(id, 0x10c);
            }
        }
        let mut scratch = resource(self.pools.scratch.acquire())?;
        for _ in 0..4 {
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
                }
                Err(quiche::Error::Done) => break,
                Err(_) => return Err(RuntimeError::Transport),
            }
        }
        Ok(self
            .connections
            .get(&connection)
            .is_some_and(|c| c.transport.quic.dgram_recv_queue_len() > 0)
            || self
                .sessions
                .values()
                .any(|s| s.connection.0 == connection && s.body_pending)
            || serviced == 4
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
            self.drop_packet(crate::diagnostics::DropReason::Oversized, direction)?;
            return Ok(());
        }
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
        match result {
            Ok(1) => {
                s.activity = now;
                self.snapshot
                    .counters
                    .egress(direction, decoded.payload.len())
                    .map_err(|_| RuntimeError::Diagnostics)?;
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                let session = s.generation;
                let conn = s.connection;
                let mut slab = match self.pools.application.acquire() {
                    Ok(s) => s,
                    Err(_) => {
                        self.drop_packet(crate::diagnostics::DropReason::QueueFull, direction)?;
                        return Ok(());
                    }
                };
                slab.bytes_mut()[..decoded.payload.len()].copy_from_slice(decoded.payload);
                let packet = resource(crate::buffers::Datagram::new(
                    slab,
                    0,
                    decoded.payload.len(),
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
                    self.drop_packet(crate::diagnostics::DropReason::QueueFull, direction)?;
                }
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
                    s.activity = now;
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
        let stream = s.stream;
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
        let result = relay
            .get_ref()
            .0
            .borrow_mut()
            .recv_batch(&mut slabs, &mut packets);
        let overflow = relay.get_ref().0.borrow_mut().take_overflow();
        self.snapshot.counters.kernel_rx_overflow = self
            .snapshot
            .counters
            .kernel_rx_overflow
            .checked_add(overflow)
            .ok_or(RuntimeError::Diagnostics)?;
        match result {
            Ok(n) => {
                let mut scratch = resource(self.pools.scratch.acquire())?;
                for p in packets {
                    if p.fault.is_some() {
                        self.drop_packet(
                            crate::diagnostics::DropReason::SocketTruncated,
                            Direction::ToClient,
                        )?;
                        continue;
                    }
                    self.snapshot
                        .counters
                        .ingress(Direction::ToClient, p.length)
                        .map_err(|_| RuntimeError::Diagnostics)?;
                    let result = self
                        .connections
                        .get_mut(&connection)
                        .ok_or(RuntimeError::Transport)?
                        .transport
                        .enqueue(
                            stream,
                            &slabs[p.index].bytes()[..p.length],
                            now,
                            now,
                            scratch.bytes_mut(),
                        );
                    if result.is_ok() {
                        if let Some(s) = self.sessions.get_mut(&id) {
                            s.activity = now;
                        }
                        self.snapshot.counters.quic_dgram_enqueued = self
                            .snapshot
                            .counters
                            .quic_dgram_enqueued
                            .checked_add(1)
                            .ok_or(RuntimeError::Diagnostics)?;
                    } else if let Err(error) = result {
                        self.drop_packet(native_drop(error), Direction::ToClient)?;
                    }
                }
                Ok(n > 0)
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                let _ = relay.try_io(tokio::io::Interest::READABLE, |_| {
                    Err::<(), _>(io::ErrorKind::WouldBlock.into())
                });
                Ok(false)
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
    fn flush(&mut self, connection: u64, now: Instant) -> Result<bool, RuntimeError> {
        let c = self
            .connections
            .get_mut(&connection)
            .ok_or(RuntimeError::Transport)?;
        let mut socket = self.outer[c.socket].get_ref().0.borrow_mut();
        let packets: Vec<_> = c
            .transport
            .output
            .iter()
            .take(4)
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
            Err(_) => Err(RuntimeError::Socket),
        }
    }
    fn service_transport_timers(&mut self, now: Instant) -> Result<bool, RuntimeError> {
        let mut more = false;
        let mut expired_native = 0usize;
        for c in self.connections.values_mut() {
            expired_native += c.transport.expire_native(now);
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
        Ok(more)
    }
    async fn run(&mut self) -> Result<(), RuntimeError> {
        let mut term = signal(SignalKind::terminate()).map_err(|_| RuntimeError::Signal)?;
        let mut interrupt = signal(SignalKind::interrupt()).map_err(|_| RuntimeError::Signal)?;
        loop {
            let now = Instant::now();
            // Timers/cancellation are checked between bounded owner turns.
            let mut more = self.service_transport_timers(now)?;
            // Each visit consumes at most 32 packet/control operations. Rotate
            // the complete work set; cancellation/timers run every <=256 work.
            let mut visits: Vec<(u8, u64)> = (0..self.outer.len()).map(|i| (0, i as u64)).collect();
            if self.listener.is_some() {
                visits.push((1, 0));
            }
            visits.extend(self.connections.keys().map(|id| (2, *id)));
            visits.extend(
                self.sessions
                    .iter()
                    .filter(|(_, s)| s.state == State::Active)
                    .map(|(id, _)| (3, *id)),
            );
            visits.sort_unstable();
            let (window, next, continuation) = service_window(visits.len(), self.turn);
            for &(kind, id) in &visits[window] {
                match kind {
                    0 => more |= self.receive_outer(id as usize, now)?,
                    1 => more |= self.client_input(now)?,
                    2 => {
                        match self.h3_work(id, now) {
                            Ok(pending) => more |= pending,
                            Err(error) => {
                                self.failure(
                                    match error {
                                        RuntimeError::Capability => Failure::Capability,
                                        RuntimeError::Trust => Failure::Trust,
                                        _ => Failure::Transport,
                                    },
                                    0,
                                    None,
                                    self.connections.get(&id).map(|c| c.id),
                                );
                                if let Some(c) = self.connections.get_mut(&id) {
                                    c.transport.stop();
                                }
                                if !self.server {
                                    return Err(error);
                                }
                            }
                        }
                        let generated = self.connections.get_mut(&id).map(|c| {
                            c.transport
                                .generate(&self.pools.ciphertext, Instant::now(), 4)
                        });
                        match generated {
                            Some(Ok(n)) => more |= n == 4,
                            Some(Err(_)) => {
                                self.failure(
                                    Failure::Transport,
                                    0,
                                    None,
                                    self.connections.get(&id).map(|c| c.id),
                                );
                                if let Some(c) = self.connections.get_mut(&id) {
                                    c.transport.stop();
                                }
                            }
                            None => {}
                        }
                        if self.flush(id, Instant::now()).is_err() {
                            self.failure(
                                Failure::Socket,
                                0,
                                None,
                                self.connections.get(&id).map(|c| c.id),
                            );
                            if let Some(c) = self.connections.get_mut(&id) {
                                c.transport.stop();
                            }
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
            let expired: Vec<_> = self
                .sessions
                .iter()
                .filter_map(|(id, s)| {
                    ((s.state != State::Active
                        && now.duration_since(s.started)
                            >= Duration::from_millis(self.cfg.recovery().setup_timeout_ms))
                        || (s.state == State::Active
                            && now.duration_since(s.activity)
                                >= Duration::from_millis(self.cfg.limits().idle_session_ms)))
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
                        0,
                        None,
                        Some(c.id),
                    );
                }
                let sessions: Vec<_> = self
                    .sessions
                    .iter()
                    .filter_map(|(sid, s)| (s.connection.0 == id).then_some(*sid))
                    .collect();
                for sid in sessions {
                    self.close_session(sid, 0x100);
                }
                self.connections.remove(&id);
                if !self.server && !self.ever_active {
                    return Err(RuntimeError::Transport);
                }
            }
            if now >= self.next_snapshot {
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
                .values()
                .filter_map(|c| c.transport.deadline(now))
                .chain(self.sessions.values().map(|s| {
                    if s.state == State::Active {
                        s.activity + Duration::from_millis(self.cfg.limits().idle_session_ms)
                    } else {
                        s.started + Duration::from_millis(self.cfg.recovery().setup_timeout_ms)
                    }
                }))
                .chain(self.sessions.values().filter_map(|s| {
                    s.queue
                        .front(if self.server {
                            Direction::ToTarget
                        } else {
                            Direction::ToClient
                        })
                        .map(|p| p.expires_at())
                }))
                .chain(std::iter::once(self.next_snapshot))
                .filter(|at| *at > now)
                .min();
            self.timer
                .get_ref()
                .arm(deadline)
                .map_err(|_| RuntimeError::Socket)?;
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
                if poll_timer(&self.timer, cx).is_ready() {
                    ready = true;
                }
                for socket in &self.outer {
                    if socket.poll_read_ready(cx).is_ready() {
                        ready = true;
                    }
                }
                if let Some(socket) = &self.listener {
                    if socket.poll_read_ready(cx).is_ready() {
                        ready = true;
                    }
                    if self.sessions.values().any(|s| s.queue.used().count > 0)
                        && socket.poll_write_ready(cx).is_ready()
                    {
                        ready = true;
                    }
                }
                for s in self.sessions.values() {
                    if s.state == State::Active
                        && let Some(socket) = &s.relay
                        && (socket.poll_read_ready(cx).is_ready()
                            || (s.queue.used().count > 0 && socket.poll_write_ready(cx).is_ready()))
                    {
                        ready = true;
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
                if ready {
                    Poll::Ready(false)
                } else {
                    Poll::Pending
                }
            })
            .await;
            if stop {
                self.stopping = true;
                self.pending.clear();
                let ids: Vec<_> = self.sessions.keys().copied().collect();
                for id in ids {
                    self.close_session(id, 0x100);
                }
                for c in self.connections.values_mut() {
                    c.transport.stop();
                }
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
    let result = runner.run().await;
    runner.pending.clear();
    let ids: Vec<_> = runner.sessions.keys().copied().collect();
    for id in ids {
        runner.close_session(id, 0x100);
    }
    runner.connections.clear();
    runner.outer.clear();
    runner.listener = None;
    // One shared final-collection deadline, independent of periodic cadence.
    // This bounded wait occurs only after sessions/sockets and packet work end.
    let deadline = Instant::now() + Duration::from_secs(1);
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
                    id: cid,
                    initial_cid: vec![],
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
                    id: cid,
                    initial_cid: vec![],
                },
            );
            let id = r.create_session(2, stream, None, Instant::now()).unwrap();
            r.reject(2, stream, 401);
            r.close_session(id, 0x10b);
            assert!(!r.sessions.contains_key(&id));
            assert_eq!(r.budgets.sessions.used().count, 0);
            assert_eq!(r.budgets.relay_fds.used().count, 0);
            p.server = r.connections.remove(&2).unwrap().transport;
            for _ in 0..50 {
                p.pump();
                while let Ok((id, event)) = p.client.h3.as_mut().unwrap().poll(&mut p.client.quic) {
                    if id == stream
                        && let quiche::h3::Event::Headers { list, .. } = event
                    {
                        assert_eq!(masque::response(&list, false), Ok(Response::Rejected(401)));
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
                    id: cid,
                    initial_cid: vec![],
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
            assert_eq!(r.connections[&2].transport.quic.dgram_send_queue_len(), 1);
            assert_eq!(r.snapshot.counters.app_ingress_by_direction.to_client, 1);
            assert_eq!(
                r.snapshot.counters.app_ingress_bytes_by_direction.to_client,
                5
            );
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
