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
    cell::Cell,
    collections::VecDeque,
    net::SocketAddr,
    time::{Duration, Instant},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportError {
    Native(u64),
    Configuration,
    Tls,
    Capability,
    Protocol,
    Capacity,
    Oversized,
    Pmtu,
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
    q.discover_pmtu(true);
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
    q.enable_hystart(settings.transport().hystart);
    // Deliberately never enable early data, session resumption or keylog.
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
#[derive(Default, Clone, Copy, serde::Serialize)]
pub struct FactoryProfile {
    pub datagram_allocations: u64,
    pub datagram_copy_bytes: u64,
    pub stream_allocations: u64,
    pub stream_copy_bytes: u64,
    pub encoded_payload_bytes: u64,
    pub overflowed: bool,
}
std::thread_local! { static FACTORY_PROFILE: std::cell::Cell<FactoryProfile> = std::cell::Cell::new(FactoryProfile::default()); }
pub fn factory_profile() -> FactoryProfile {
    FACTORY_PROFILE.with(std::cell::Cell::get)
}
fn count_factory(select: fn(&mut FactoryProfile) -> &mut u64, amount: usize) {
    FACTORY_PROFILE.with(|cell| {
        let mut stats = cell.get();
        let slot = select(&mut stats);
        if let Some(next) = slot.checked_add(amount as u64) {
            *slot = next;
        } else {
            stats.overflowed = true;
        }
        cell.set(stats);
    });
}
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
        count_factory(|s| &mut s.stream_allocations, 1);
        count_factory(|s| &mut s.stream_copy_bytes, bytes.len());
        TrackedBuf {
            bytes: std::sync::Arc::from(bytes),
            _receipt: RESPONSE_RECEIPT.with(|slot| slot.borrow().clone()),
        }
    }
    fn dgram_buf_from_slice(bytes: &[u8]) -> Vec<u8> {
        count_factory(
            |s| &mut s.datagram_allocations,
            usize::from(!bytes.is_empty()),
        );
        count_factory(|s| &mut s.datagram_copy_bytes, bytes.len());
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
    length: usize,
    expires: Instant,
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
    expired_native_pending: usize,
    received_expiry: Option<Instant>,
    next_sequence: u64,
    writable_cursor: crate::session::FairCursor,
    closing_remaining: usize,
    closed_streams: VecDeque<ClosedStream>,
    closed_limit: usize,
    application_residence: Duration,
    output_residence: Duration,
    generation_pacing: Option<Instant>,
    application_max: usize,
    outer_max: usize,
    started: Instant,
    setup_timeout: Duration,
    stopped: bool,
    critical_streams: [Option<u64>; 3],
    critical_failed: bool,
    pmtu_drops: usize,
    lost_observed: usize,
    ready: bool,
    peer_progress: Instant,
    next_probe: Instant,
    keepalive_interval: Duration,
    progress_timeout: Duration,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LivenessAction {
    None,
    Probe,
    Expired,
}
impl Connection {
    pub fn attach_qlog(
        quic: &mut quiche::Connection<TrackedFactory>,
        pool: Option<&crate::diagnostics::qlog::Pool>,
    ) {
        #[cfg(feature = "qlog")]
        if let Some(filter) = pool.and_then(crate::diagnostics::qlog::Pool::activate) {
            quic.set_qlog(
                Box::new(filter),
                "Remora".into(),
                "Filtered transport metadata".into(),
            );
        }
        #[cfg(not(feature = "qlog"))]
        let _ = (quic, pool);
    }
    /// Aggregate active-path numeric data without serializing addresses/CIDs.
    pub fn operational_stats(&self) -> crate::diagnostics::QuicSummary {
        let mut result = crate::diagnostics::QuicSummary {
            sampled_connections: 1,
            spurious_lost_packets: self.quic.stats().spurious_lost,
            application_payload_lower_bound_min: self
                .native_datagram_ceiling()
                .saturating_sub(9)
                .min(self.application_max),
            ..Default::default()
        };
        for p in self.quic.path_stats().filter(|p| p.active) {
            if let Some(exit) = p.startup_exit {
                match exit.reason {
                    quiche::StartupExitReason::ConservativeSlowStartRounds => {
                        result.startup_exit_hystart_paths += 1
                    }
                    quiche::StartupExitReason::Loss => result.startup_exit_loss_paths += 1,
                    _ => result.startup_exit_other_paths += 1,
                }
                result.startup_exit_cwnd_min_bytes = if result.startup_exit_cwnd_min_bytes == 0 {
                    exit.cwnd
                } else {
                    result.startup_exit_cwnd_min_bytes.min(exit.cwnd)
                };
            }
            result.rtt_max_us = result.rtt_max_us.max(p.rtt.as_micros() as u64);
            result.rttvar_max_us = result.rttvar_max_us.max(p.rttvar.as_micros() as u64);
            if let Some(next) = result
                .delivery_rate_total_bytes_per_second
                .checked_add(p.delivery_rate)
            {
                result.delivery_rate_total_bytes_per_second = next;
            } else {
                result.overflow = true;
            }
            result.path_payload_limit_min = if result.path_payload_limit_min == 0 {
                p.pmtu
            } else {
                result.path_payload_limit_min.min(p.pmtu)
            };
            for (slot, n) in [
                (&mut result.cwnd_total_bytes, p.cwnd),
                (&mut result.pto_count, p.total_pto_count),
                (&mut result.retransmitted_packets, p.retrans),
            ] {
                if let Some(next) = slot.checked_add(n) {
                    *slot = next;
                } else {
                    result.overflow = true;
                }
            }
        }
        result
    }
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
            expired_native_pending: 0,
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
            generation_pacing: None,
            application_max: settings.transport().application_max_bytes,
            outer_max: settings.transport().outer_udp_max_bytes,
            started: now,
            setup_timeout: Duration::from_millis(settings.recovery().setup_timeout_ms),
            stopped: false,
            critical_streams: [None; 3],
            critical_failed: false,
            pmtu_drops: 0,
            lost_observed: 0,
            ready: false,
            peer_progress: now,
            next_probe: now + Duration::from_millis(settings.recovery().keepalive_interval_ms),
            keepalive_interval: Duration::from_millis(settings.recovery().keepalive_interval_ms),
            progress_timeout: Duration::from_millis(settings.recovery().peer_progress_timeout_ms),
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
        // Pinned quiche increments stats.recv only after decrypting, processing
        // frames and accepting a fresh packet number. Duplicates/unauthenticated
        // packets and local sends do not reset this application watchdog.
        let received = self.quic.stats().recv;
        let result = self.quic.recv(bytes, info);
        if self.quic.is_established() && self.quic.stats().recv > received {
            self.peer_progress = self.peer_progress.max(now);
        }
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
                    .map_err(|error| TransportError::Native(h3_error_code(error)))?,
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
        self.ready = true;
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
        self.native_datagram_ceiling()
            .saturating_sub(masque::varint_len(stream / 4).unwrap_or(8) + 1)
            .min(self.application_max)
    }
    fn native_datagram_ceiling(&self) -> usize {
        // Writable capacity already excludes QUIC overhead. During revalidation
        // or probing, recovery's allowance can exceed the verified active PMTU.
        // Subtract only that allowance gap; never subtract QUIC headers twice.
        let verified = self
            .quic
            .path_stats()
            .find(|p| p.active)
            .map_or(1200, |p| p.pmtu)
            .min(self.outer_max);
        self.quic
            .dgram_max_writable_len()
            .unwrap_or(0)
            .saturating_sub(
                self.quic
                    .max_send_udp_payload_size()
                    .saturating_sub(verified),
            )
    }
    /// Discard still-cancellable oversized native messages after path reduction.
    /// Reservation order stays aligned with the actual quiche queue predicate.
    fn prune_pmtu(&mut self) -> Result<(), TransportError> {
        let limit = self.native_datagram_ceiling();
        let count = self.native.iter().filter(|n| n.length > limit).count();
        self.quic.dgram_purge_outgoing(|bytes| bytes.len() > limit);
        self.native.retain(|n| n.length <= limit);
        // Each accumulation is bounded by the admitted native queue capacity;
        // the owner consumes this count after every packet generation visit.
        self.pmtu_drops = self
            .pmtu_drops
            .checked_add(count)
            .ok_or(TransportError::Capacity)?;
        self.refresh_native_expiry();
        Ok(())
    }
    pub fn take_pmtu_drops(&mut self) -> usize {
        std::mem::take(&mut self.pmtu_drops)
    }
    /// Outer QUIC estimates, separate from endpoint sequence loss/kernel overflow.
    pub fn take_lost_packets(&mut self) -> Result<usize, TransportError> {
        let lost = self.quic.stats().lost;
        let delta = lost
            .checked_sub(self.lost_observed)
            .ok_or(TransportError::Protocol)?;
        self.lost_observed = lost;
        Ok(delta)
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
        if payload.len() > self.application_max {
            return Err(TransportError::Oversized);
        }
        if payload.len() > self.payload_ceiling(stream) {
            return Err(TransportError::Pmtu);
        }
        // Keep application work cancellable in its session queue while the
        // committed FIFO cannot accept another packet. Expiry above still
        // retires stale work instead of indefinitely returning capacity.
        if self.output_full() {
            return Err(TransportError::Capacity);
        }
        let len = masque::encode_datagram(stream, payload, scratch)
            .map_err(|_| TransportError::Protocol)?;
        count_factory(|s| &mut s.encoded_payload_bytes, payload.len());
        let reservation = self
            .native_budget
            .reserve(1, len)
            .map_err(|_| TransportError::Capacity)?;
        self.quic
            .dgram_send(&scratch[..len])
            .map_err(|_| TransportError::Capacity)?;
        self.native.push_back(Native {
            stream,
            length: len,
            expires: expiry,
            _reservation: reservation,
        });
        self.native_expiry = Some(self.native_expiry.map_or(expiry, |old| old.min(expiry)));
        Ok(())
    }
    /// Full committed output blocks both generation and native application
    /// admission on this connection, without blocking control/timer service.
    pub(crate) fn output_full(&self) -> bool {
        self.output.len() >= 32
    }
    pub fn purge_stream_datagrams(&mut self, stream: u64) {
        self.quic.dgram_purge_outgoing(|bytes| {
            masque::decode_datagram(bytes).map_or(true, |d| d.is_none_or(|d| d.stream == stream))
        });
        self.native.retain(|n| n.stream != stream);
        self.refresh_native_expiry();
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
    fn refresh_native_expiry(&mut self) {
        self.native_expiry = self.native.iter().map(|n| n.expires).min();
    }
    /// Expire only native datagrams whose original application deadline is due.
    /// The informative return value must not be counted again by the owner;
    /// `take_expired_datagrams` is the single diagnostic handoff, including
    /// expirations performed inside generation or followed by an error/stop.
    pub fn expire_native(&mut self, now: Instant) -> Result<usize, TransportError> {
        if self.closed_streams.iter().any(|s| s.expires <= now) {
            self.stop();
        }
        if self.native_expiry.is_some_and(|at| at <= now) {
            let count = self.native.iter().filter(|n| n.expires <= now).count();
            let pending = self
                .expired_native_pending
                .checked_add(count)
                .ok_or(TransportError::Capacity)?;
            // Pinned quiche DatagramQueue::purge uses VecDeque::retain in FIFO
            // order. Match ordinal metadata, never bytes: identical payloads
            // may have different deadlines. No wire tags or extra copies.
            let index = Cell::new(0);
            let native = &self.native;
            self.quic.dgram_purge_outgoing(|_| {
                let i = index.get();
                index.set(i + 1);
                native.get(i).is_none_or(|n| n.expires <= now)
            });
            self.native.retain(|n| n.expires > now);
            self.refresh_native_expiry();
            self.expired_native_pending = pending;
            Ok(count)
        } else {
            Ok(0)
        }
    }
    pub fn take_expired_datagrams(&mut self) -> usize {
        std::mem::take(&mut self.expired_native_pending)
    }
    pub fn stop(&mut self) {
        self.stopped = true;
        self.output.clear();
        self.closed_streams.clear();
        self.native.clear();
        self.native_expiry = None;
        self.generation_pacing = None;
        self.quic.dgram_purge_outgoing(|_| true);
        let _ = self.quic.close(true, 0x100, b"");
    }
    pub fn service_liveness(
        &mut self,
        now: Instant,
        active_flows: bool,
    ) -> Result<LivenessAction, TransportError> {
        if !active_flows || !self.ready || self.stopped || self.quic.is_closed() {
            return Ok(LivenessAction::None);
        }
        if now >= self.peer_progress + self.progress_timeout {
            return Ok(LivenessAction::Expired);
        }
        if now >= self.next_probe {
            self.quic
                .send_ack_eliciting()
                .map_err(|e| TransportError::Native(native_error_code(e)))?;
            self.next_probe = now + self.keepalive_interval;
            return Ok(LivenessAction::Probe);
        }
        Ok(LivenessAction::None)
    }
    pub fn is_ready(&self) -> bool {
        self.ready
    }
    pub fn deadline(&self, now: Instant) -> Option<Instant> {
        self.deadline_for(now, false)
    }
    pub fn deadline_for(&self, now: Instant, active_flows: bool) -> Option<Instant> {
        [
            self.quic.timeout_instant(),
            self.native_expiry,
            self.generation_pacing,
            self.closed_streams.iter().map(|s| s.expires).min(),
            self.received_expiry,
            self.output.front().map(|p| p.expires_at()),
            self.output.front().map(|p| p.at()).filter(|at| *at > now),
            self.started
                .checked_add(self.setup_timeout)
                .filter(|_| !self.ready),
            (active_flows && self.ready).then_some(self.next_probe),
            (active_flows && self.ready).then_some(self.peer_progress + self.progress_timeout),
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
        self.expire_native(now)?;
        self.prune_pmtu()?;
        if self
            .output
            .front()
            .is_some_and(|packet| packet.expires_at() <= now)
        {
            return Err(TransportError::Expired);
        }
        if !self.ready && now.duration_since(self.started) >= self.setup_timeout {
            return Err(TransportError::Expired);
        }
        if quota == 0 {
            return Ok(0);
        }
        let mut produced = 0;
        self.generation_pacing = None;
        let gcongestion =
            self.quic.pacing_enabled() && self.quic.gcongestion_enabled() == Some(true);
        while produced < quota && !self.output_full() && !self.stopped {
            // gcongestion's SendInfo.at is read after on_packet_sent advances
            // its pacer. Capture this packet's release decision before send,
            // as the pinned tokio-quiche adapter does. Do not precommit an
            // arbitrary FIFO of slow-paced packets that would expire while
            // waiting for their own pacing slot. Classic CUBIC keeps SendInfo.
            let release = gcongestion
                .then(|| self.quic.get_next_release_time())
                .flatten()
                .and_then(|decision| decision.time(now));
            if let Some(at) = release
                && at.duration_since(now)
                    >= self
                        .quic
                        .max_release_into_future()
                        .min(self.output_residence / 2)
            {
                self.generation_pacing = Some(at);
                break;
            }
            let mut slab = match pool.acquire() {
                Ok(s) => s,
                Err(_) => break,
            };
            let limit = slab.capacity().min(self.outer_max);
            match self.quic.send(&mut slab.bytes_mut()[..limit]) {
                Ok((length, info)) => {
                    self.next_probe = now + self.keepalive_interval;
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
                        if gcongestion {
                            release.unwrap_or(now)
                        } else {
                            info.at
                        },
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
                    self.refresh_native_expiry();
                }
                Err(quiche::Error::Done) => break,
                Err(error) => return Err(TransportError::Native(native_error_code(error))),
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

/// Fixed privacy-safe native error categories; associated peer values omitted.
pub fn native_error_code(error: quiche::Error) -> u64 {
    use quiche::Error::*;
    match error {
        Done => 0,
        BufferTooShort => 1,
        UnknownVersion => 2,
        InvalidFrame => 3,
        InvalidPacket => 4,
        InvalidState => 5,
        InvalidStreamState(_) => 6,
        InvalidTransportParam => 7,
        CryptoFail => 8,
        TlsFail => 9,
        FlowControl => 10,
        StreamLimit => 11,
        StreamStopped(_) => 12,
        StreamReset(_) => 13,
        FinalSize => 14,
        CongestionControl => 15,
        IdLimit => 16,
        OutOfIdentifiers => 17,
        KeyUpdate => 18,
        CryptoBufferExceeded => 19,
        InvalidAckRange => 20,
        OptimisticAckDetected => 21,
        InvalidDcidInitialization => 22,
    }
}

/// Standard H3 wire categories plus fixed local API categories. No native
/// message text or peer-associated stream/error data is reflected.
pub fn h3_error_code(error: quiche::h3::Error) -> u64 {
    use quiche::h3::Error::*;
    match error {
        TransportError(error) => 0x10000 + native_error_code(error),
        StreamBlocked => 0x20000,
        BufferTooShort => 0x20001,
        Done => 0,
        InternalError => 0x102,
        ExcessiveLoad => 0x107,
        IdError => 0x108,
        StreamCreationError => 0x103,
        ClosedCriticalStream => 0x104,
        MissingSettings => 0x10a,
        FrameUnexpected => 0x105,
        FrameError => 0x106,
        QpackDecompressionFailed => 0x200,
        SettingsError => 0x109,
        RequestRejected => 0x10b,
        RequestCancelled => 0x10c,
        RequestIncomplete => 0x10d,
        MessageError => 0x10e,
        ConnectError => 0x10f,
        VersionFallback => 0x110,
    }
}
