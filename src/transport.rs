//! Exclusive-owner public quiche adapter; no library traces or key logs.
use crate::{
    buffers::{Budget, BufferPool, Ciphertext, Generation, Reservation},
    config::{CongestionControl, RoleConfig, ValidatedConfig},
    masque,
};
use boring::{
    pkey::PKey,
    ssl::{SslContextBuilder, SslMethod, SslVerifyMode},
    x509::X509,
};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::{
    collections::VecDeque,
    net::SocketAddr,
    time::{Duration, Instant},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportError {
    Configuration,
    Tls,
    Capability,
    Protocol,
    Capacity,
    Oversized,
    Expired,
    Stopped,
}
impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "QUIC failed: {self:?}")
    }
}
impl std::error::Error for TransportError {}
pub fn config(settings: &ValidatedConfig) -> Result<quiche::Config, TransportError> {
    let mut tls =
        SslContextBuilder::new(SslMethod::tls()).map_err(|_| TransportError::Configuration)?;
    let server = matches!(settings.role(), RoleConfig::Server(_));
    match settings.role() {
        RoleConfig::Client(c) => {
            for cert in
                X509::stack_from_pem(&c.ca_pem).map_err(|_| TransportError::Configuration)?
            {
                tls.cert_store_mut()
                    .add_cert(cert)
                    .map_err(|_| TransportError::Configuration)?;
            }
            tls.set_verify(SslVerifyMode::PEER);
        }
        RoleConfig::Server(s) => {
            let mut certs = X509::stack_from_pem(&s.certificate_pem)
                .map_err(|_| TransportError::Configuration)?
                .into_iter();
            let leaf = certs.next().ok_or(TransportError::Configuration)?;
            tls.set_certificate(&leaf)
                .map_err(|_| TransportError::Configuration)?;
            for cert in certs {
                tls.add_extra_chain_cert(cert)
                    .map_err(|_| TransportError::Configuration)?;
            }
            let key = PKey::private_key_from_pem(&s.private_key_pem)
                .map_err(|_| TransportError::Configuration)?;
            tls.set_private_key(&key)
                .map_err(|_| TransportError::Configuration)?;
            tls.check_private_key()
                .map_err(|_| TransportError::Configuration)?;
            tls.set_verify(SslVerifyMode::NONE);
        }
    }
    let mut q = quiche::Config::with_boring_ssl_ctx_builder(quiche::PROTOCOL_VERSION, tls)
        .map_err(|_| TransportError::Configuration)?;
    q.verify_peer(!server);
    q.set_application_protos(&[b"h3"])
        .map_err(|_| TransportError::Configuration)?;
    q.set_max_idle_timeout(settings.transport().idle_timeout_ms);
    q.set_max_recv_udp_payload_size(settings.transport().outer_udp_max_bytes);
    q.set_max_send_udp_payload_size(settings.transport().outer_udp_max_bytes);
    q.set_initial_max_data(256 * 1024);
    q.set_initial_max_stream_data_bidi_local(16384);
    q.set_initial_max_stream_data_bidi_remote(16384);
    q.set_initial_max_stream_data_uni(16384);
    q.set_initial_max_streams_bidi(settings.limits().max_sessions_per_connection as u64);
    q.set_initial_max_streams_uni(4);
    q.set_max_connection_window(256 * 1024);
    q.set_max_stream_window(16384);
    q.set_active_connection_id_limit(4);
    q.set_disable_active_migration(true);
    q.set_path_challenge_recv_max_queue_len(4);
    q.enable_dgram(
        true,
        settings.limits().quiche_dgram_packets,
        settings.limits().quiche_dgram_packets,
    );
    q.set_cc_algorithm(match settings.transport().cc {
        CongestionControl::Cubic => quiche::CongestionControlAlgorithm::CUBIC,
        CongestionControl::Reno => quiche::CongestionControlAlgorithm::Reno,
        CongestionControl::Bbr2Gcongestion => quiche::CongestionControlAlgorithm::Bbr2Gcongestion,
    });
    q.enable_pacing(true);
    // Deliberately never enable early data, session resumption, qlog or keylog.
    Ok(q)
}
pub fn h3_config(server: bool) -> Result<quiche::h3::Config, TransportError> {
    let mut c = quiche::h3::Config::new().map_err(|_| TransportError::Configuration)?;
    c.set_max_field_section_size(16384);
    c.set_qpack_max_table_capacity(0);
    c.set_qpack_blocked_streams(0);
    c.enable_extended_connect(server);
    c.set_max_priority_update_size(256);
    Ok(c)
}
pub fn random_cid() -> Result<[u8; 16], TransportError> {
    let mut cid = [0; 16];
    getrandom::fill(&mut cid).map_err(|_| TransportError::Configuration)?;
    Ok(cid)
}

// quiche has no public per-stream packet-generation receipt. Track a bounded
// receipt on buffers created synchronously while queuing a successful response.
// Release after ACK is a conservative proof of transport commitment. A closed
// stream must discard its receipt rather than treating buffer drop as success.
std::thread_local! { static RESPONSE_RECEIPT: std::cell::RefCell<Option<std::sync::Arc<()>>> = const { std::cell::RefCell::new(None) }; }
#[derive(Clone, Default, Debug)]
pub struct TrackedFactory;
#[derive(Clone)]
pub struct TrackedBuf {
    bytes: std::sync::Arc<[u8]>,
    _receipt: Option<std::sync::Arc<()>>,
}
impl std::fmt::Debug for TrackedBuf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuicBuffer")
            .field("length", &self.bytes.len())
            .finish_non_exhaustive()
    }
}
impl AsRef<[u8]> for TrackedBuf {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}
impl quiche::BufFactory for TrackedFactory {
    type Buf = TrackedBuf;
    type DgramBuf = Vec<u8>;
    fn buf_from_slice(bytes: &[u8]) -> TrackedBuf {
        TrackedBuf {
            bytes: std::sync::Arc::from(bytes),
            _receipt: RESPONSE_RECEIPT.with(|slot| slot.borrow().clone()),
        }
    }
    fn dgram_buf_from_slice(bytes: &[u8]) -> Vec<u8> {
        bytes.to_vec()
    }
}
pub fn track_response<T>(send: impl FnOnce() -> T) -> (T, std::sync::Weak<()>) {
    struct Scope;
    impl Drop for Scope {
        fn drop(&mut self) {
            RESPONSE_RECEIPT.with(|slot| *slot.borrow_mut() = None);
        }
    }
    let receipt = std::sync::Arc::new(());
    let weak = std::sync::Arc::downgrade(&receipt);
    RESPONSE_RECEIPT.with(|slot| {
        assert!(slot.borrow().is_none(), "single-owner response scope");
        *slot.borrow_mut() = Some(receipt);
    });
    let _scope = Scope;
    let result = send();
    (result, weak)
}

struct Native {
    stream: u64,
    _reservation: Reservation,
}
struct ClosedStream {
    stream: u64,
    expires: Instant,
    _session: Option<Reservation>,
    _identity: Option<Reservation>,
}
pub struct Connection {
    pub quic: quiche::Connection<TrackedFactory>,
    pub h3: Option<quiche::h3::Connection>,
    pub generation: Generation,
    pub server: bool,
    pub draining: Option<u64>,
    pub output: VecDeque<Ciphertext>,
    native: VecDeque<Native>,
    native_budget: Budget,
    native_expiry: Option<Instant>,
    received_expiry: Option<Instant>,
    next_sequence: u64,
    writable_cursor: crate::session::FairCursor,
    closing_remaining: usize,
    closed_streams: VecDeque<ClosedStream>,
    closed_limit: usize,
    application_residence: Duration,
    output_residence: Duration,
    application_max: usize,
    outer_max: usize,
    started: Instant,
    setup_timeout: Duration,
    stopped: bool,
    critical_streams: [Option<u64>; 3],
    critical_failed: bool,
}
impl Connection {
    pub fn new(
        quic: quiche::Connection<TrackedFactory>,
        generation: Generation,
        server: bool,
        settings: &ValidatedConfig,
        now: Instant,
    ) -> Self {
        Self {
            quic,
            h3: None,
            generation,
            server,
            draining: None,
            output: VecDeque::with_capacity(32),
            native: VecDeque::with_capacity(settings.limits().quiche_dgram_packets),
            native_budget: Budget::new(
                settings.limits().quiche_dgram_packets,
                settings.limits().connection_dgram_bytes / 2,
            ),
            native_expiry: None,
            received_expiry: None,
            next_sequence: 0,
            writable_cursor: crate::session::FairCursor::default(),
            closing_remaining: 0,
            closed_streams: VecDeque::with_capacity(settings.limits().max_sessions_per_connection),
            closed_limit: settings.limits().max_sessions_per_connection,
            application_residence: Duration::from_millis(
                settings.limits().application_residence_ms,
            ),
            output_residence: Duration::from_millis(settings.limits().ciphertext_residence_ms),
            application_max: settings.transport().application_max_bytes,
            outer_max: settings.transport().outer_udp_max_bytes,
            started: now,
            setup_timeout: Duration::from_millis(settings.recovery().setup_timeout_ms),
            stopped: false,
            critical_streams: [None; 3],
            critical_failed: false,
        }
    }
    /// All incoming native messages share a conservative oldest receive epoch.
    /// Public quiche queues expose no individual receive timestamps.
    pub fn receive(
        &mut self,
        bytes: &mut [u8],
        info: quiche::RecvInfo,
        now: Instant,
    ) -> Result<usize, quiche::Error> {
        if self.quic.dgram_recv_queue_len() == 0 {
            self.received_expiry = None;
        }
        let result = self.quic.recv(bytes, info);
        if self.quic.dgram_recv_queue_len() > 0 {
            let expiry = now + self.application_residence;
            self.received_expiry = Some(self.received_expiry.map_or(expiry, |old| old.min(expiry)));
        }
        self.drain_path_events();
        result
    }
    /// Consume native path notifications without retaining peer-controlled
    /// history or producing per-event logs. True requests another bounded turn.
    pub fn drain_path_events(&mut self) -> bool {
        for _ in 0..32 {
            if self.quic.path_event_next().is_none() {
                return false;
            }
        }
        true
    }
    pub fn incoming_expiry(&self) -> Option<Instant> {
        self.received_expiry
    }
    pub fn incoming_consumed(&mut self) {
        if self.quic.dgram_recv_queue_len() == 0 {
            self.received_expiry = None;
        }
    }
    pub fn negotiate(&mut self) -> Result<bool, TransportError> {
        if self.stopped || self.critical_failed {
            return Err(TransportError::Stopped);
        }
        if !self.quic.is_established() || self.quic.is_in_early_data() {
            return Ok(false);
        }
        if self.quic.application_proto() != b"h3"
            || (!self.server && self.quic.peer_cert().is_none())
        {
            return Err(TransportError::Tls);
        }
        if self.h3.is_none() {
            self.h3 = Some(
                quiche::h3::Connection::with_transport(&mut self.quic, &h3_config(self.server)?)
                    .map_err(|_| TransportError::Protocol)?,
            );
            // Pinned quiche opens control, optional QPACK, then optional GREASE
            // uni streams synchronously in with_transport. Remora opens no
            // other local uni streams. Capture the existing unfinished streams
            // through public QUIC state, excluding GREASE's already queued FIN.
            // Do not infer that every uni stream (including GREASE) is critical.
            let base = if self.server { 3 } else { 2 };
            let mut critical = (0..4).map(|n| base + 4 * n).filter(|&id| {
                !self.quic.stream_closed(id) && self.quic.stream_capacity(id).is_ok()
            });
            for slot in &mut self.critical_streams {
                *slot = critical.next();
            }
        }
        let h3 = self.h3.as_ref().ok_or(TransportError::Protocol)?;
        if h3.peer_settings_raw().is_none() {
            return Ok(false);
        }
        if !h3.dgram_enabled_by_peer(&self.quic)
            || (!self.server && !h3.extended_connect_enabled_by_peer())
        {
            return Err(TransportError::Capability);
        }
        Ok(true)
    }
    pub fn goaway(&mut self, id: u64) -> Result<(), TransportError> {
        if !self.server
            && (!id.is_multiple_of(4)
                || id > masque::MAX_VARINT
                || self.draining.is_some_and(|previous| id > previous))
        {
            return Err(TransportError::Protocol);
        }
        self.draining = Some(id);
        Ok(())
    }
    /// Observe stopped write sides even after the application Session retired.
    /// quiche only collects a completed STOP_SENDING stream when its writable
    /// notification is consumed through stream_capacity/send. H3 polls reads.
    /// Visit four IDs at a time and finish a finite cycle before sleeping.
    pub fn reap_stopped_streams(&mut self) -> (bool, [Option<(u64, u64)>; 2]) {
        if self.critical_failed || self.stopped {
            return (false, [None; 2]);
        }
        // Reserved checks cannot be delayed by a large request writable set.
        // Observing capacity can collect a stopped uni stream; remember IDs
        // at H3 creation so native absence remains a critical error too.
        for id in self.critical_streams.into_iter().flatten() {
            if matches!(
                self.quic.stream_capacity(id),
                Err(quiche::Error::StreamStopped(_) | quiche::Error::InvalidStreamState(_))
            ) {
                self.critical_failed = true;
                // Connection failure discards unsent old ciphertext, unlike a
                // scoped Session close, which preserves committed mixed output.
                self.output.clear();
                self.native.clear();
                self.native_expiry = None;
                self.quic.dgram_purge_outgoing(|_| true);
                let _ = self.quic.close(true, 0x104, b"");
                // Keep close-packet generation enabled. The owner cancels only
                // this connection's sessions and skips further H3 work.
                return (false, [None; 2]);
            }
        }
        // Closing control work owns two reserved visits independently of active
        // writable streams. FIFO rotation covers older IDs despite continual
        // additions; a finite initial count prevents an ACK-wait busy spin.
        if self.closing_remaining == 0 {
            self.closing_remaining = self.closed_streams.len();
        }
        self.closing_remaining = self.closing_remaining.min(self.closed_streams.len());
        let count = self.closing_remaining.min(2);
        let mut ids: Vec<_> = self
            .closed_streams
            .iter()
            .take(count)
            .map(|s| s.stream)
            .collect();
        self.closed_streams.rotate_left(count);
        self.closing_remaining -= count;
        let writable: Vec<_> = self
            .quic
            .writable()
            .filter(|id| !self.closed_streams.iter().any(|s| s.stream == *id))
            .collect();
        ids.extend(crate::session::round_robin(
            writable.clone(),
            &mut self.writable_cursor,
            2,
        ));
        let mut collected = Vec::with_capacity(4);
        let mut stopped = [None; 2];
        let mut notifications = 0;
        for id in ids {
            let closing = self.closed_streams.iter().any(|s| s.stream == id);
            let capacity = self.quic.stream_capacity(id);
            if let Err(quiche::Error::StreamStopped(code)) = capacity
                && !closing
            {
                stopped[notifications] = Some((id, code));
                notifications += 1;
            }
            if matches!(
                capacity,
                Err(quiche::Error::StreamStopped(_) | quiche::Error::InvalidStreamState(_))
            ) && closing
                && let Some(h3) = self.h3.as_mut()
            {
                // Only the stopped/error paths can remove completed H3 state.
                // Both calls fail before enqueueing bytes on this write side.
                let _ = h3.send_body(&mut self.quic, id, &[], true);
                if self.server {
                    let _ =
                        h3.send_response(&mut self.quic, id, &masque::response_headers(503), true);
                }
            }
            // stream_closed only means FIN is signalled, not ACK/collection.
            // Keep the ID and its permits until the native entry is absent,
            // including one final H3 error-path visit after ACK collection.
            if matches!(
                self.quic.stream_capacity(id),
                Err(quiche::Error::InvalidStreamState(_))
            ) {
                collected.push(id);
            }
        }
        self.closed_streams
            .retain(|s| !collected.contains(&s.stream));
        self.closing_remaining = self.closing_remaining.min(self.closed_streams.len());
        (
            // Ordinary live writability is not pending control work. Requesting
            // another owner turn for it can perpetually restart a cyclic visit.
            // A new STOP and a finite closing cycle do require continuation.
            self.closing_remaining > 0 || notifications > 0,
            stopped,
        )
    }
    pub fn cleanup_count(&self) -> usize {
        self.closed_streams.len()
    }
    /// Retain only bounded control metadata/permits after relay/payload release.
    /// Peer omission cannot retain it beyond 60s; a genuinely stalled native
    /// control association fails its connection, never reassigns old payload.
    pub(crate) fn retain_cleanup(
        &mut self,
        stream: u64,
        session: Option<Reservation>,
        identity: Option<Reservation>,
    ) {
        if self.stopped {
            return;
        }
        if let Some(closed) = self.closed_streams.iter_mut().find(|s| s.stream == stream) {
            if session.is_some() {
                closed._session = session;
                closed._identity = identity;
            }
            return;
        }
        if self.closed_streams.len() == self.closed_limit {
            self.stop();
            return;
        }
        self.closed_streams.push_back(ClosedStream {
            stream,
            expires: Instant::now() + Duration::from_secs(60),
            _session: session,
            _identity: identity,
        });
        // Even a natively collected stream needs one final public H3 error-path
        // visit to release its metadata. Do not silently omit that cleanup.
    }
    /// Retained transport queue ownership, separate from application slabs.
    pub fn resource_usage(&self) -> (usize, usize, usize) {
        (
            self.native_budget.used().bytes,
            self.quic.dgram_recv_queue_byte_size(),
            self.output.capacity() * std::mem::size_of::<Ciphertext>()
                + self.native.capacity() * std::mem::size_of::<Native>()
                + self.closed_streams.capacity() * std::mem::size_of::<ClosedStream>(),
        )
    }
    pub fn is_stopped(&self) -> bool {
        self.stopped
    }
    pub(crate) fn critical_failed(&self) -> bool {
        self.critical_failed
    }
    pub fn may_open(&self) -> bool {
        !self.stopped && !self.critical_failed && self.draining.is_none()
    }
    pub fn allowed_stream(&self, id: u64) -> bool {
        self.server || self.draining.is_none_or(|cutoff| id < cutoff)
    }
    pub fn payload_ceiling(&self, stream: u64) -> usize {
        self.quic
            .dgram_max_writable_len()
            .unwrap_or(0)
            .saturating_sub(masque::varint_len(stream / 4).unwrap_or(8) + 1)
            .min(self.application_max)
    }
    pub fn enqueue(
        &mut self,
        stream: u64,
        payload: &[u8],
        received: Instant,
        now: Instant,
        scratch: &mut [u8],
    ) -> Result<(), TransportError> {
        if self.stopped
            || self.critical_failed
            || !self.allowed_stream(stream)
            || self.quic.stream_capacity(stream).is_err()
        {
            return Err(TransportError::Stopped);
        }
        let expiry = received
            .checked_add(self.application_residence)
            .ok_or(TransportError::Expired)?;
        if expiry <= now {
            return Err(TransportError::Expired);
        }
        if payload.len() > self.payload_ceiling(stream) {
            return Err(TransportError::Oversized);
        }
        let len = masque::encode_datagram(stream, payload, scratch)
            .map_err(|_| TransportError::Protocol)?;
        let reservation = self
            .native_budget
            .reserve(1, len)
            .map_err(|_| TransportError::Capacity)?;
        self.quic
            .dgram_send(&scratch[..len])
            .map_err(|_| TransportError::Capacity)?;
        self.native.push_back(Native {
            stream,
            _reservation: reservation,
        });
        self.native_expiry = Some(self.native_expiry.map_or(expiry, |old| old.min(expiry)));
        Ok(())
    }
    pub fn purge_stream_datagrams(&mut self, stream: u64) {
        self.quic.dgram_purge_outgoing(|bytes| {
            masque::decode_datagram(bytes).map_or(true, |d| d.is_none_or(|d| d.stream == stream))
        });
        self.native.retain(|n| n.stream != stream);
        if self.native.is_empty() {
            self.native_expiry = None;
        }
    }
    pub fn close_stream(&mut self, stream: u64, code: u64) {
        self.retain_cleanup(stream, None, None);
        self.purge_stream_datagrams(stream);
        let _ = self
            .quic
            .stream_shutdown(stream, quiche::Shutdown::Read, code);
        // Mark H3's local side finished using its public API before reset.
        // Any newly queued empty FIN is immediately discarded by Write shutdown
        // below, before packet generation under exclusive ownership. This keeps
        // H3 from retaining closed entries when the peer's RESET is polled.
        if let Some(h3) = self.h3.as_mut() {
            let _ = h3.send_body(&mut self.quic, stream, &[], true);
        }
        let _ = self
            .quic
            .stream_shutdown(stream, quiche::Shutdown::Write, code);
        // Immutable already-generated mixed ciphertext deliberately survives.
    }
    pub fn expire_native(&mut self, now: Instant) -> usize {
        if self.closed_streams.iter().any(|s| s.expires <= now) {
            self.stop();
        }
        if self.native_expiry.is_some_and(|at| at <= now) {
            let count = self.quic.dgram_send_queue_len();
            self.quic.dgram_purge_outgoing(|_| true);
            self.native.clear();
            self.native_expiry = None;
            count
        } else {
            0
        }
    }
    pub fn stop(&mut self) {
        self.stopped = true;
        self.output.clear();
        self.closed_streams.clear();
        self.native.clear();
        self.native_expiry = None;
        self.quic.dgram_purge_outgoing(|_| true);
        let _ = self.quic.close(true, 0x100, b"");
    }
    pub fn deadline(&self, now: Instant) -> Option<Instant> {
        [
            self.quic.timeout_instant(),
            self.native_expiry,
            self.closed_streams.iter().map(|s| s.expires).min(),
            self.received_expiry,
            self.output.front().map(|p| p.expires_at()),
            self.output.front().map(|p| p.at()).filter(|at| *at > now),
            self.started
                .checked_add(self.setup_timeout)
                .filter(|_| self.h3.is_none()),
        ]
        .into_iter()
        .flatten()
        .min()
    }
    pub fn generate(
        &mut self,
        pool: &BufferPool,
        now: Instant,
        quota: usize,
    ) -> Result<usize, TransportError> {
        self.expire_native(now);
        if self
            .output
            .front()
            .is_some_and(|packet| packet.expires_at() <= now)
        {
            return Err(TransportError::Expired);
        }
        if self.h3.is_none() && now.duration_since(self.started) >= self.setup_timeout {
            return Err(TransportError::Expired);
        }
        let mut produced = 0;
        while produced < quota && self.output.len() < 32 && !self.stopped {
            let mut slab = match pool.acquire() {
                Ok(s) => s,
                Err(_) => break,
            };
            let limit = slab.capacity().min(self.outer_max);
            match self.quic.send(&mut slab.bytes_mut()[..limit]) {
                Ok((length, info)) => {
                    let expiry = now
                        .checked_add(self.output_residence)
                        .ok_or(TransportError::Expired)?;
                    let sequence = self.next_sequence;
                    self.next_sequence = self
                        .next_sequence
                        .checked_add(1)
                        .ok_or(TransportError::Capacity)?;
                    let packet = Ciphertext::new(
                        slab,
                        length,
                        info.from,
                        info.to,
                        info.at,
                        self.generation,
                        sequence,
                        now,
                        expiry,
                    )
                    .map_err(|_| TransportError::Capacity)?;
                    self.output.push_back(packet);
                    produced += 1;
                    while self.native.len() > self.quic.dgram_send_queue_len() {
                        self.native.pop_front();
                    }
                    if self.native.is_empty() {
                        self.native_expiry = None;
                    }
                }
                Err(quiche::Error::Done) => break,
                Err(_) => return Err(TransportError::Protocol),
            }
        }
        Ok(produced)
    }
}

/// Stateless Retry binding: short process-monotonic expiry, source tuple,
/// original DCID and the exact issued Retry SCID. Tokens never grant authority.
pub struct Retry {
    key: [u8; 32],
    started: Instant,
}
impl Retry {
    pub fn new(now: Instant) -> Result<Self, TransportError> {
        let mut key = [0; 32];
        getrandom::fill(&mut key).map_err(|_| TransportError::Configuration)?;
        Ok(Self { key, started: now })
    }
    fn address(peer: SocketAddr) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(23);
        match peer {
            SocketAddr::V4(v) => {
                bytes.push(4);
                bytes.extend(v.ip().octets());
            }
            SocketAddr::V6(v) => {
                bytes.push(6);
                bytes.extend(v.ip().octets());
                bytes.extend(v.scope_id().to_be_bytes());
            }
        }
        bytes.extend(peer.port().to_be_bytes());
        bytes
    }
    pub fn mint(
        &self,
        peer: SocketAddr,
        original: &[u8],
        retry: &[u8],
        now: Instant,
    ) -> Result<Vec<u8>, TransportError> {
        if !(8..=20).contains(&original.len()) || retry.len() != 16 {
            return Err(TransportError::Protocol);
        }
        let age = now
            .checked_duration_since(self.started)
            .ok_or(TransportError::Protocol)?
            .as_secs();
        let mut token = Vec::with_capacity(96);
        token.extend(age.to_be_bytes());
        token.push(original.len() as u8);
        token.extend(original);
        token.extend(retry);
        let mut mac =
            Hmac::<Sha256>::new_from_slice(&self.key).map_err(|_| TransportError::Configuration)?;
        mac.update(&Self::address(peer));
        mac.update(&token);
        token.extend(mac.finalize().into_bytes());
        Ok(token)
    }
    pub fn verify(
        &self,
        peer: SocketAddr,
        dcid: &[u8],
        token: &[u8],
        now: Instant,
    ) -> Result<Vec<u8>, TransportError> {
        if token.len() < 65 || token.len() > 77 {
            return Err(TransportError::Protocol);
        }
        let n = usize::from(token[8]);
        if !(8..=20).contains(&n) || token.len() != 9 + n + 16 + 32 {
            return Err(TransportError::Protocol);
        }
        let mut mac =
            Hmac::<Sha256>::new_from_slice(&self.key).map_err(|_| TransportError::Configuration)?;
        mac.update(&Self::address(peer));
        mac.update(&token[..9 + n + 16]);
        mac.verify_slice(&token[9 + n + 16..])
            .map_err(|_| TransportError::Protocol)?;
        if dcid != &token[9 + n..9 + n + 16] {
            return Err(TransportError::Protocol);
        }
        let age = u64::from_be_bytes(
            token[..8]
                .try_into()
                .map_err(|_| TransportError::Protocol)?,
        );
        let current = now
            .checked_duration_since(self.started)
            .ok_or(TransportError::Protocol)?
            .as_secs();
        if current.checked_sub(age).is_none_or(|elapsed| elapsed > 10) {
            return Err(TransportError::Expired);
        }
        Ok(token[9..9 + n].to_vec())
    }
}
