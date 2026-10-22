//! Privacy-safe records and finite nonblocking handoff. There is deliberately
//! no global tracing subscriber or arbitrary-string logging API. Terminal
//! summaries use acknowledged batches; qlog persists only reconstructed metadata.
pub mod qlog;
use crate::{buffers::Direction, config::Level};
use serde::Serialize;
use std::{
    collections::VecDeque,
    io::{self, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub const OUTPUT_BYTES: usize = 1024 * 1024;
pub const OUTPUT_RECORDS: usize = 1024;
pub const STATUS_BYTES: usize = 256 * 1024;
pub const STATUS_RECORDS: usize = 256;
pub const MAX_RECORD_BYTES: usize = 64 * 1024;
pub const FILTER_BYTES: usize = 512 * 1024;
pub const STORAGE_BYTES: usize = 512 * 1024;
pub const TOTAL_BYTES: usize = OUTPUT_BYTES + FILTER_BYTES + STORAGE_BYTES;
const EVENT_BYTES: usize = OUTPUT_BYTES - STATUS_BYTES;
const EVENT_RECORDS: usize = OUTPUT_RECORDS - STATUS_RECORDS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagnosticError {
    Entropy,
    Arithmetic,
    Full,
    Disconnected,
    Format,
    Clock,
    Interval,
    Allocation,
}
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Client,
    Server,
}
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[repr(usize)]
pub enum Failure {
    InvalidConfiguration,
    Trust,
    Authentication,
    Authorization,
    Capability,
    Resolution,
    SetupTimeout,
    PeerTimeout,
    Transport,
    Socket,
    Size,
    Resource,
    Malformed,
    Internal,
    Stopped,
}
/// Fixed classes only: no caller-controlled labels and no per-session history.
#[derive(Default, Clone, Copy)]
pub struct FailureTotals {
    counts: [u64; 15],
}
impl Serialize for FailureTotals {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let names = [
            "invalid_configuration",
            "trust",
            "authentication",
            "authorization",
            "capability",
            "resolution",
            "setup_timeout",
            "peer_timeout",
            "transport",
            "socket",
            "size",
            "resource",
            "malformed",
            "internal",
            "stopped",
        ];
        let mut map = serializer.serialize_map(Some(names.len()))?;
        for (name, count) in names.iter().zip(self.counts) {
            map.serialize_entry(name, &count)?;
        }
        map.end()
    }
}
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DropReason {
    NotActive,
    Recovering,
    Rejected,
    Oversized,
    Malformed,
    UnknownFlow,
    UnknownContext,
    UnsupportedCarrier,
    QueueFull,
    QueueExpired,
    StaleGeneration,
    SocketTruncated,
    AncillaryInvalid,
    SocketError,
    Pmtu,
}
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventName {
    Starting,
    Active,
    Recovering,
    Rejected,
    Stopped,
    Failure,
    OffloadState,
}
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OffloadReason {
    ConfiguredOff,
    NotImplemented,
    CapabilityUnavailable,
    PathIneligible,
    Enabled,
}
impl crate::config::Offload {
    pub fn deferred_reason(self) -> OffloadReason {
        match self {
            Self::Off => OffloadReason::ConfiguredOff,
            Self::Auto => OffloadReason::NotImplemented,
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct OpaqueId {
    epoch: [u8; 16],
    serial: u64,
}
impl Serialize for OpaqueId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut text = [0u8; 49];
        hex(&self.epoch, &mut text[..32]);
        text[32] = b'-';
        hex(&self.serial.to_be_bytes(), &mut text[33..]);
        serializer.serialize_str(std::str::from_utf8(&text).expect("hex ASCII"))
    }
}
fn hex(bytes: &[u8], out: &mut [u8]) {
    const H: &[u8] = b"0123456789abcdef";
    for (b, pair) in bytes.iter().zip(out.as_chunks_mut::<2>().0) {
        pair[0] = H[(b >> 4) as usize];
        pair[1] = H[(b & 15) as usize];
    }
}
/// One random epoch per process plus monotonic local IDs, unrelated to endpoints.
pub struct Ids {
    epoch: [u8; 16],
    next: u64,
}
impl Ids {
    pub fn new() -> Result<Self, DiagnosticError> {
        let mut epoch = [0; 16];
        getrandom::fill(&mut epoch).map_err(|_| DiagnosticError::Entropy)?;
        Ok(Self { epoch, next: 0 })
    }
    pub fn next_id(&mut self) -> Result<OpaqueId, DiagnosticError> {
        self.next = self
            .next
            .checked_add(1)
            .ok_or(DiagnosticError::Arithmetic)?;
        Ok(OpaqueId {
            epoch: self.epoch,
            serial: self.next,
        })
    }
}
#[derive(Clone, Copy, Serialize)]
pub struct Event {
    pub event: EventName,
    pub level: Level,
    pub failure: Option<Failure>,
    pub numeric_code: Option<u64>,
    pub connection_id: Option<OpaqueId>,
    pub session_id: Option<OpaqueId>,
    pub direction: Option<Direction>,
    pub drop_reason: Option<DropReason>,
    pub offload_reason: Option<OffloadReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<SizeDetails>,
}
impl Event {
    pub fn failure(level: Level, failure: Failure, numeric_code: u64) -> Self {
        Self {
            event: EventName::Failure,
            level,
            failure: Some(failure),
            numeric_code: Some(numeric_code),
            connection_id: None,
            session_id: None,
            direction: None,
            drop_reason: None,
            offload_reason: None,
            size: None,
        }
    }
}
#[derive(Clone, Copy, Serialize)]
pub struct OffloadReport {
    pub requested: crate::config::Offload,
    pub effective_on: bool,
    pub reason: OffloadReason,
}
impl OffloadReport {
    pub fn new(requested: crate::config::Offload) -> Self {
        Self {
            requested,
            effective_on: false,
            reason: requested.deferred_reason(),
        }
    }
}
impl Default for OffloadReport {
    fn default() -> Self {
        Self::new(crate::config::Offload::Off)
    }
}
#[derive(Default, Clone, Copy, Serialize)]
pub struct OffloadState {
    pub gso: OffloadReport,
    pub gro: OffloadReport,
}
#[derive(Default, Clone, Copy, Serialize)]
pub struct SocketBuffers {
    pub outer_receive_max_per_socket: usize,
    pub outer_send_max_per_socket: usize,
    pub application_receive_max_per_socket: usize,
    pub application_send_max_per_socket: usize,
}
#[derive(Default, Clone, Copy, Serialize)]
pub struct Resources {
    pub connections: usize,
    pub sessions: usize,
    pub setup: usize,
    pub relay_fds: usize,
    pub fd_reservations: usize,
    pub connections_peak: usize,
    pub sessions_peak: usize,
    pub setup_peak: usize,
    pub relay_fds_peak: usize,
    pub queued_packets: usize,
    pub queued_bytes: usize,
    pub application_retained_bytes: usize,
    pub ciphertext_retained_bytes: usize,
    pub scratch_retained_bytes: usize,
    pub queue_metadata_bytes: usize,
    pub pool_metadata_bytes: usize,
    pub native_send_bytes: usize,
    pub native_receive_bytes: usize,
    pub control_output_capacity_bytes: usize,
    pub fd_reservations_peak: usize,
    pub queued_packets_observed_peak: usize,
    pub queued_bytes_observed_peak: usize,
    pub session_queue_packets_peak: usize,
    pub session_queue_bytes_peak: usize,
    pub application_retained_peak: usize,
    pub ciphertext_retained_peak: usize,
    pub scratch_retained_peak: usize,
}
#[derive(Default, Clone, Copy, Serialize)]
pub struct Snapshot {
    pub lifecycle: Lifecycle,
    pub histograms: Histograms,
    pub quic: QuicSummary,
    pub qlog: qlog::Report,
    pub io_batch_size: usize,
    pub recovery: RecoverySnapshot,
    pub profile: ProfileCounters,
    pub resources: Resources,
    pub sessions_active: u64,
    pub connections_active: u64,
    pub counters: Counters,
    pub offloads: OffloadState,
    pub socket_buffers: SocketBuffers,
}
/// Fixed buckets: <=0,10,100,1000,10000,100000,1000000,>1000000.
/// Times are microseconds; burst samples use logical UDP messages.
#[derive(Default, Clone, Copy, Serialize)]
pub struct Histogram {
    pub buckets: [u64; 8],
    pub overflow: bool,
}
impl Histogram {
    pub fn observe(&mut self, value: u64) {
        let index = [0, 10, 100, 1000, 10000, 100000, 1000000]
            .iter()
            .position(|bound| value <= *bound)
            .unwrap_or(7);
        if let Some(next) = self.buckets[index].checked_add(1) {
            self.buckets[index] = next;
        } else {
            self.overflow = true;
        }
    }
}
#[derive(Default, Clone, Copy, Serialize)]
pub struct Histograms {
    pub application_queue_age_us: Histogram,
    pub ciphertext_age_us: Histogram,
    pub pacing_lateness_us: Histogram,
    pub emitted_burst_datagrams: Histogram,
    pub recovery_duration_us: Histogram,
}
#[derive(Default, Clone, Copy, Serialize)]
pub struct Lifecycle {
    pub starting: usize,
    pub authenticating: usize,
    pub authorizing: usize,
    pub awaiting_acceptance: usize,
    pub awaiting_commitment: usize,
    pub active: usize,
    pub recovering: usize,
    pub accepted_total: u64,
    // An observed response is a message from the pinned destination, not a
    // reachability promise. Zero can be a silent UDP service.
    pub destination_response_datagrams: u64,
}
#[derive(Default, Clone, Copy, Serialize)]
pub struct QuicSummary {
    pub sampled_connections: usize,
    pub rtt_max_us: u64,
    pub rttvar_max_us: u64,
    pub cwnd_total_bytes: usize,
    pub path_payload_limit_min: usize,
    /// Conservative local-send application ceiling, allowing any association ID.
    pub application_payload_lower_bound_min: usize,
    pub retransmitted_packets: usize,
    pub pto_count: usize,
    pub overflow: bool,
}
#[derive(Default, Clone, Copy, Serialize)]
pub struct RecoverySnapshot {
    pub counter_overflow: bool,
    pub client_state: Option<crate::session::ClientState>,
    pub connection_attempts: u64,
    pub transitions: u64,
    pub keepalive_probes: u64,
    pub watchdog_timeouts: u64,
    pub discarded_ciphertext: u64,
}
/// Controlled boundaries only, not all allocations/copies inside crypto/quiche.
#[derive(Default, Clone, Copy, Serialize)]
pub struct ProfileCounters {
    pub owner_visits: u64,
    pub queue_copy_bytes: u64,
    pub received_datagram_copy_bytes: u64,
    pub payload_slab_allocations: usize,
    pub application_queue_age_max_us: u64,
    pub ciphertext_age_max_us: u64,
    pub pacing_lateness_max_us: u64,
    pub send_burst_datagrams_max: usize,
    pub io: crate::udp::IoStats,
    pub factory: crate::transport::FactoryProfile,
}
#[derive(Default, Clone, Copy, Serialize)]
pub struct Counters {
    pub app_ingress_datagrams: u64,
    pub app_ingress_bytes: u64,
    pub quic_dgram_enqueued: u64,
    pub relay_socket_sent: u64,
    pub app_ingress_by_direction: DirectionCounts,
    pub app_ingress_bytes_by_direction: DirectionCounts,
    pub app_egress_by_direction: DirectionCounts,
    pub app_egress_bytes_by_direction: DirectionCounts,
    pub outer_socket_sent: u64,
    pub outer_socket_sent_bytes: u64,
    pub outer_socket_received: u64,
    pub outer_socket_received_bytes: u64,
    pub kernel_rx_overflow: u64,
    pub outer_pmtu_errors: u64,
    pub outer_send_pressure: u64,
    pub outer_async_errors: u64,
    pub destination_pmtu_updates: u64,
    pub outer_quic_lost_packets: u64,
    pub local_drops: DropCounters,
}
#[derive(Default, Clone, Copy, Serialize)]
pub struct DirectionCounts {
    pub to_target: u64,
    pub to_client: u64,
}
impl DirectionCounts {
    fn add(&mut self, direction: Direction, amount: u64) -> Result<(), DiagnosticError> {
        let slot = match direction {
            Direction::ToTarget => &mut self.to_target,
            Direction::ToClient => &mut self.to_client,
        };
        *slot = slot
            .checked_add(amount)
            .ok_or(DiagnosticError::Arithmetic)?;
        Ok(())
    }
}
impl Counters {
    pub fn ingress(&mut self, direction: Direction, len: usize) -> Result<(), DiagnosticError> {
        self.app_ingress_datagrams = self
            .app_ingress_datagrams
            .checked_add(1)
            .ok_or(DiagnosticError::Arithmetic)?;
        self.app_ingress_bytes = self
            .app_ingress_bytes
            .checked_add(len as u64)
            .ok_or(DiagnosticError::Arithmetic)?;
        self.app_ingress_by_direction.add(direction, 1)?;
        self.app_ingress_bytes_by_direction
            .add(direction, len as u64)
    }
    pub fn egress(&mut self, direction: Direction, len: usize) -> Result<(), DiagnosticError> {
        self.relay_socket_sent = self
            .relay_socket_sent
            .checked_add(1)
            .ok_or(DiagnosticError::Arithmetic)?;
        self.app_egress_by_direction.add(direction, 1)?;
        self.app_egress_bytes_by_direction
            .add(direction, len as u64)
    }
}
#[derive(Default, Clone, Copy, Serialize)]
pub struct DropCounters {
    pub not_active: DirectionCounts,
    pub recovering: DirectionCounts,
    pub rejected: DirectionCounts,
    pub oversized: DirectionCounts,
    pub malformed: DirectionCounts,
    pub unknown_flow: DirectionCounts,
    pub unknown_context: DirectionCounts,
    pub unsupported_carrier: DirectionCounts,
    pub queue_full: DirectionCounts,
    pub queue_expired: DirectionCounts,
    pub stale_generation: DirectionCounts,
    pub socket_truncated: DirectionCounts,
    pub ancillary_invalid: DirectionCounts,
    pub socket_error: DirectionCounts,
    pub pmtu: DirectionCounts,
}
impl DropCounters {
    pub fn increment(
        &mut self,
        reason: DropReason,
        direction: Direction,
    ) -> Result<(), DiagnosticError> {
        self.add(reason, direction, 1)
    }
    pub fn add(
        &mut self,
        reason: DropReason,
        direction: Direction,
        amount: u64,
    ) -> Result<(), DiagnosticError> {
        let counts = match reason {
            DropReason::NotActive => &mut self.not_active,
            DropReason::Recovering => &mut self.recovering,
            DropReason::Rejected => &mut self.rejected,
            DropReason::Oversized => &mut self.oversized,
            DropReason::Malformed => &mut self.malformed,
            DropReason::UnknownFlow => &mut self.unknown_flow,
            DropReason::UnknownContext => &mut self.unknown_context,
            DropReason::UnsupportedCarrier => &mut self.unsupported_carrier,
            DropReason::QueueFull => &mut self.queue_full,
            DropReason::QueueExpired => &mut self.queue_expired,
            DropReason::StaleGeneration => &mut self.stale_generation,
            DropReason::SocketTruncated => &mut self.socket_truncated,
            DropReason::AncillaryInvalid => &mut self.ancillary_invalid,
            DropReason::SocketError => &mut self.socket_error,
            DropReason::Pmtu => &mut self.pmtu,
        };
        let count = match direction {
            Direction::ToTarget => &mut counts.to_target,
            Direction::ToClient => &mut counts.to_client,
        };
        *count = count
            .checked_add(amount)
            .ok_or(DiagnosticError::Arithmetic)?;
        Ok(())
    }
}
#[derive(Serialize)]
struct Envelope<T: Serialize> {
    schema_version: u32,
    timestamp: String,
    sequence: u64,
    role: Role,
    process_epoch: OpaqueId,
    #[serde(flatten)]
    body: T,
}
#[derive(Clone, Copy, Serialize)]
pub struct FailureSummary {
    #[serde(rename = "seq")]
    sequence: u64,
    #[serde(rename = "connection")]
    connection_id: Option<OpaqueId>,
    #[serde(rename = "session")]
    session_id: Option<OpaqueId>,
    #[serde(rename = "class")]
    failure: Failure,
    #[serde(rename = "code")]
    numeric_code: u64,
    terminal: bool,
    repeats: u64,
    #[serde(rename = "ms")]
    monotonic_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<SizeDetails>,
}
/// Numeric metadata only; never retains an application packet or address.
#[derive(Clone, Copy, Serialize)]
pub struct SizeDetails {
    pub direction: Direction,
    pub datagram_bytes: usize,
    pub application_cap_bytes: usize,
    pub outgoing_ceiling_bytes: usize,
}
const _: () = assert!(std::mem::size_of::<FailureSummary>() <= 256);
#[derive(Serialize)]
struct Status<'a> {
    event: &'static str,
    #[serde(flatten)]
    counters: Snapshot,
    diagnostics_dropped: u64,
    status_write_failures: u64,
    counter_overflow: bool,
    failures: &'a [FailureSummary],
    failure_totals: FailureTotals,
    chunk_index: usize,
    chunk_count: usize,
    coverage_lost: u64,
    lost_first: Option<u64>,
    lost_last: Option<u64>,
}

/// Exact token-credit accumulation, with intentional burst clamping.
pub struct EventBucket {
    credits: u128,
    last: Instant,
}
impl EventBucket {
    pub fn new(now: Instant) -> Self {
        Self {
            credits: 40 * 1_000_000_000,
            last: now,
        }
    }
    pub fn admit(&mut self, now: Instant) -> Result<bool, DiagnosticError> {
        let elapsed = now
            .checked_duration_since(self.last)
            .ok_or(DiagnosticError::Clock)?;
        let refill = elapsed
            .as_nanos()
            .checked_mul(20)
            .ok_or(DiagnosticError::Arithmetic)?;
        self.credits = self
            .credits
            .checked_add(refill)
            .ok_or(DiagnosticError::Arithmetic)?
            .min(40 * 1_000_000_000);
        self.last = now;
        if self.credits < 1_000_000_000 {
            return Ok(false);
        }
        self.credits -= 1_000_000_000;
        Ok(true)
    }
}
struct Shared {
    event_bytes: AtomicUsize,
    status_bytes: AtomicUsize,
    event_records: AtomicUsize,
    status_records: AtomicUsize,
    dropped: AtomicU64,
    write_failures: AtomicU64,
    overflow: AtomicBool,
    status_ack: AtomicU64,
}
fn increment(counter: &AtomicU64, shared: &Shared) {
    if counter
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
        .is_err()
    {
        shared.overflow.store(true, Ordering::Relaxed);
    }
}
/// A record retains its byte charge until the writer finishes or drops it.
pub struct OutputRecord {
    data: Box<[u8]>,
    shared: Arc<Shared>,
    status: bool,
    acknowledgement: u64,
    qlog: Option<Arc<qlog::Destination>>,
}
impl OutputRecord {
    pub fn bytes(&self) -> &[u8] {
        &self.data
    }
    pub fn is_status(&self) -> bool {
        self.status
    }
}
impl Drop for OutputRecord {
    fn drop(&mut self) {
        let counter = if self.status {
            &self.shared.status_bytes
        } else {
            &self.shared.event_bytes
        };
        counter.fetch_sub(self.data.len(), Ordering::AcqRel);
        let records = if self.status {
            &self.shared.status_records
        } else {
            &self.shared.event_records
        };
        records.fetch_sub(1, Ordering::AcqRel);
    }
}
/// The reactor never waits for a writer. Channel counts include reserved status
/// capacity; bytes include the record being written. Failed enqueue is counted.
#[derive(Clone)]
pub struct OutputQueue {
    event: SyncSender<OutputRecord>,
    status: SyncSender<OutputRecord>,
    shared: Arc<Shared>,
}
pub struct OutputReceiver {
    event: Receiver<OutputRecord>,
    status: Receiver<OutputRecord>,
    shared: Arc<Shared>,
}
impl OutputQueue {
    pub fn new() -> (Self, OutputReceiver) {
        let shared = Arc::new(Shared {
            event_bytes: AtomicUsize::new(0),
            status_bytes: AtomicUsize::new(0),
            event_records: AtomicUsize::new(0),
            status_records: AtomicUsize::new(0),
            dropped: AtomicU64::new(0),
            write_failures: AtomicU64::new(0),
            overflow: AtomicBool::new(false),
            status_ack: AtomicU64::new(0),
        });
        let (event_tx, event_rx) = mpsc::sync_channel(EVENT_RECORDS);
        let (status_tx, status_rx) = mpsc::sync_channel(STATUS_RECORDS);
        (
            Self {
                event: event_tx,
                status: status_tx,
                shared: shared.clone(),
            },
            OutputReceiver {
                event: event_rx,
                status: status_rx,
                shared,
            },
        )
    }
    pub fn retained_bytes(&self) -> usize {
        self.shared.event_bytes.load(Ordering::Acquire)
            + self.shared.status_bytes.load(Ordering::Acquire)
    }
    pub fn dropped(&self) -> u64 {
        self.shared.dropped.load(Ordering::Relaxed)
    }
    pub fn retained_records(&self) -> usize {
        self.shared.event_records.load(Ordering::Acquire)
            + self.shared.status_records.load(Ordering::Acquire)
    }
    // Private: accepting arbitrary JSON/text here would bypass the safe schema.
    fn enqueue<T: Serialize>(&self, status: bool, value: &T) -> Result<(), DiagnosticError> {
        let result = self.enqueue_inner(status, value, 0);
        if result.is_err() {
            increment(&self.shared.dropped, &self.shared);
        }
        result
    }
    fn enqueue_inner<T: Serialize>(
        &self,
        status: bool,
        value: &T,
        acknowledgement: u64,
    ) -> Result<(), DiagnosticError> {
        let record = self.prepare(status, value, acknowledgement)?;
        self.publish(record)
    }
    fn prepare<T: Serialize>(
        &self,
        status: bool,
        value: &T,
        acknowledgement: u64,
    ) -> Result<OutputRecord, DiagnosticError> {
        // Fixed stack formatting is <=64 KiB. There is no unbounded to_string
        // intermediate. Metadata plus one writer record and future status
        // formatting consume the separate 512 KiB storage reservation.
        let mut scratch = [0u8; MAX_RECORD_BYTES];
        let mut cursor = io::Cursor::new(&mut scratch[..]);
        serde_json::to_writer(&mut cursor, value).map_err(|_| DiagnosticError::Format)?;
        cursor
            .write_all(b"\n")
            .map_err(|_| DiagnosticError::Format)?;
        let length = cursor.position() as usize;
        let (counter, cap, records, record_cap) = if status {
            (
                &self.shared.status_bytes,
                STATUS_BYTES,
                &self.shared.status_records,
                STATUS_RECORDS,
            )
        } else {
            (
                &self.shared.event_bytes,
                EVENT_BYTES,
                &self.shared.event_records,
                EVENT_RECORDS,
            )
        };
        records
            .try_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                n.checked_add(1).filter(|next| *next <= record_cap)
            })
            .map_err(|_| DiagnosticError::Full)?;
        if counter
            .try_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                n.checked_add(length).filter(|next| *next <= cap)
            })
            .is_err()
        {
            records.fetch_sub(1, Ordering::AcqRel);
            return Err(DiagnosticError::Full);
        }
        let mut data = Vec::new();
        if data.try_reserve_exact(length).is_err() {
            counter.fetch_sub(length, Ordering::AcqRel);
            records.fetch_sub(1, Ordering::AcqRel);
            return Err(DiagnosticError::Allocation);
        }
        data.extend_from_slice(&scratch[..length]);
        let record = OutputRecord {
            data: data.into_boxed_slice(),
            shared: self.shared.clone(),
            status,
            acknowledgement,
            qlog: None,
        };
        Ok(record)
    }
    fn filtered_qlog<T: Serialize>(
        &self,
        value: &T,
        target: Arc<qlog::Destination>,
    ) -> Result<usize, DiagnosticError> {
        let result = (|| {
            let mut record = self.prepare(false, value, 0)?;
            let length = record.data.len();
            record.qlog = Some(target);
            self.publish(record)?;
            Ok(length)
        })();
        if result.is_err() {
            increment(&self.shared.dropped, &self.shared);
        }
        result
    }
    fn publish(&self, record: OutputRecord) -> Result<(), DiagnosticError> {
        let sender = if record.status {
            &self.status
        } else {
            &self.event
        };
        sender.try_send(record).map_err(|e| match e {
            mpsc::TrySendError::Full(_) => DiagnosticError::Full,
            mpsc::TrySendError::Disconnected(_) => DiagnosticError::Disconnected,
        })
    }
}
impl OutputReceiver {
    /// Prioritize status at record boundaries. None means both queues are empty.
    pub fn try_next(&self) -> Result<Option<OutputRecord>, DiagnosticError> {
        let status_disconnected = match self.status.try_recv() {
            Ok(record) => return Ok(Some(record)),
            Err(TryRecvError::Disconnected) => true,
            Err(TryRecvError::Empty) => false,
        };
        match self.event.try_recv() {
            Ok(record) => Ok(Some(record)),
            Err(TryRecvError::Disconnected) if status_disconnected => {
                Err(DiagnosticError::Disconnected)
            }
            _ => Ok(None),
        }
    }
    /// Blocking writes belong exclusively to the writer thread, never the owner.
    pub fn write_one(
        &self,
        stdout: &mut impl Write,
        stderr: &mut impl Write,
    ) -> Result<bool, DiagnosticError> {
        let Some(record) = self.try_next()? else {
            return Ok(false);
        };
        if let Some(target) = &record.qlog {
            // Optional disk failure disables that trace, never the status writer.
            if target.persist(record.bytes()).is_err() {
                target.fail();
            }
            return Ok(true);
        }
        let sink: &mut dyn Write = if record.status { stdout } else { stderr };
        if sink
            .write_all(record.bytes())
            .and_then(|_| sink.flush())
            .is_err()
        {
            increment(&self.shared.write_failures, &self.shared);
            return Err(DiagnosticError::Disconnected);
        }
        if record.acknowledgement != 0 {
            self.shared
                .status_ack
                .store(record.acknowledgement, Ordering::Release);
        }
        Ok(true)
    }
    pub fn spawn<W: Write + Send + 'static, E: Write + Send + 'static>(
        self,
        mut stdout: W,
        mut stderr: E,
    ) -> io::Result<WriterHandle> {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let thread = thread::Builder::new()
            .name("remora-diagnostics".into())
            // Charge the writer stack within STORAGE_BYTES instead of using
            // the platform's multi-MiB default stack.
            .stack_size(128 * 1024)
            .spawn(move || {
                loop {
                    match self.write_one(&mut stdout, &mut stderr) {
                        Ok(true) => {}
                        Ok(false) if !worker_stop.load(Ordering::Acquire) => {
                            thread::sleep(Duration::from_millis(1))
                        }
                        Ok(false) | Err(DiagnosticError::Disconnected)
                            if self.shared.write_failures.load(Ordering::Relaxed) == 0 =>
                        {
                            return true;
                        }
                        _ => return false,
                    }
                }
            })?;
        Ok(WriterHandle {
            thread: Some(thread),
            stop,
        })
    }
}
pub struct WriterHandle {
    thread: Option<JoinHandle<bool>>,
    stop: Arc<AtomicBool>,
}
impl WriterHandle {
    /// Bound waiting even if a sink blocks. False explicitly means no completed
    /// flush; a blocked thread is detached and can only be reclaimed at exit.
    pub fn finish(mut self, timeout: Duration) -> bool {
        self.stop.store(true, Ordering::Release);
        let Some(deadline) = Instant::now().checked_add(timeout) else {
            return false;
        };
        while let Some(worker) = &self.thread {
            if worker.is_finished() {
                return self
                    .thread
                    .take()
                    .expect("writer handle")
                    .join()
                    .unwrap_or(false);
            }
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Duration::from_millis(1));
        }
        true
    }
}
impl Drop for WriterHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}
/// Finite owner-side emission state. The caller provides a monotonic clock;
/// UTC timestamps are generated locally and never derived from peer input.
pub struct Diagnostics {
    queue: OutputQueue,
    ids: Ids,
    sequence: u64,
    bucket: EventBucket,
    role: Role,
    last_snapshot: Option<Instant>,
    snapshot_interval: Duration,
    failures: VecDeque<FailureSummary>,
    failure_sequence: u64,
    failure_totals: FailureTotals,
    coverage_lost: u64,
    lost_first: Option<u64>,
    lost_last: Option<u64>,
    in_flight: Option<(u64, u64, u64)>,
    started: Instant,
    minimum_level: Level,
}
impl Diagnostics {
    pub fn new(
        role: Role,
        now: Instant,
        snapshot_interval: Duration,
    ) -> Result<(Self, OutputReceiver), DiagnosticError> {
        if snapshot_interval < Duration::from_secs(1) {
            return Err(DiagnosticError::Interval);
        }
        let (queue, receiver) = OutputQueue::new();
        Ok((
            Self {
                queue,
                ids: Ids::new()?,
                sequence: 0,
                bucket: EventBucket::new(now),
                role,
                last_snapshot: None,
                snapshot_interval,
                failures: VecDeque::with_capacity(256),
                failure_sequence: 0,
                failure_totals: FailureTotals::default(),
                coverage_lost: 0,
                lost_first: None,
                lost_last: None,
                in_flight: None,
                started: now,
                minimum_level: Level::Trace,
            },
            receiver,
        ))
    }
    pub fn set_level(&mut self, level: Level) {
        self.minimum_level = level;
    }
    pub fn next_id(&mut self) -> Result<OpaqueId, DiagnosticError> {
        self.ids.next_id()
    }
    pub fn queue(&self) -> &OutputQueue {
        &self.queue
    }
    fn envelope<T: Serialize>(&mut self, body: T) -> Result<Envelope<T>, DiagnosticError> {
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or(DiagnosticError::Arithmetic)?;
        let timestamp = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(|_| DiagnosticError::Format)?;
        Ok(Envelope {
            schema_version: 1,
            timestamp,
            sequence: self.sequence,
            role: self.role,
            process_epoch: OpaqueId {
                epoch: self.ids.epoch,
                serial: 0,
            },
            body,
        })
    }
    pub fn event(&mut self, now: Instant, event: Event) -> Result<bool, DiagnosticError> {
        fn rank(level: Level) -> u8 {
            match level {
                Level::Error => 0,
                Level::Warn => 1,
                Level::Info => 2,
                Level::Debug => 3,
                Level::Trace => 4,
            }
        }
        if rank(event.level) > rank(self.minimum_level) {
            return Ok(false);
        }
        if !self.bucket.admit(now)? {
            increment(&self.queue.shared.dropped, &self.queue.shared);
            return Ok(false);
        }
        let record = self.envelope(event)?;
        self.queue.enqueue(false, &record)?;
        Ok(true)
    }
    /// Independent of ordinary log admission. No arbitrary input is retained.
    pub fn failure(
        &mut self,
        now: Instant,
        connection: Option<OpaqueId>,
        session: Option<OpaqueId>,
        class: Failure,
        code: u64,
        terminal: bool,
    ) -> Result<(), DiagnosticError> {
        let counter = &mut self.failure_totals.counts[class as usize];
        *counter = counter.checked_add(1).ok_or(DiagnosticError::Arithmetic)?;
        self.failure_sequence = self
            .failure_sequence
            .checked_add(1)
            .ok_or(DiagnosticError::Arithmetic)?;
        let sequence = self.failure_sequence;
        if !terminal
            && let Some(entry) = self.failures.iter_mut().find(|e| {
                !e.terminal
                    && e.connection_id == connection
                    && e.session_id == session
                    && e.failure == class
            })
        {
            entry.sequence = sequence;
            entry.numeric_code = code;
            entry.size = None;
            entry.repeats = entry
                .repeats
                .checked_add(1)
                .ok_or(DiagnosticError::Arithmetic)?;
            return Ok(());
        }
        if self.failures.len() == 256 {
            let lost = self.failures.pop_front().expect("full ring");
            self.coverage_lost = self
                .coverage_lost
                .checked_add(1)
                .ok_or(DiagnosticError::Arithmetic)?;
            self.lost_first = Some(
                self.lost_first
                    .map_or(lost.sequence, |old| old.min(lost.sequence)),
            );
            self.lost_last = Some(
                self.lost_last
                    .map_or(lost.sequence, |old| old.max(lost.sequence)),
            );
        }
        self.failures.push_back(FailureSummary {
            sequence,
            connection_id: connection,
            session_id: session,
            failure: class,
            numeric_code: code,
            terminal,
            repeats: 1,
            monotonic_ms: now
                .checked_duration_since(self.started)
                .unwrap_or_default()
                .as_millis()
                .min(u64::MAX as u128) as u64,
            size: None,
        });
        Ok(())
    }
    /// Reuse the bounded failure ring and ordinary event rate limit. Repeats
    /// retain the most recent size observation for this connection/session.
    pub fn size_failure(
        &mut self,
        now: Instant,
        connection: Option<OpaqueId>,
        session: OpaqueId,
        code: u64,
        details: SizeDetails,
    ) -> Result<(), DiagnosticError> {
        self.failure(now, connection, Some(session), Failure::Size, code, false)?;
        if let Some(entry) = self.failures.iter_mut().find(|e| {
            !e.terminal
                && e.connection_id == connection
                && e.session_id == Some(session)
                && e.failure == Failure::Size
        }) {
            entry.size = Some(details);
        }
        let mut event = Event::failure(Level::Warn, Failure::Size, code);
        event.connection_id = connection;
        event.session_id = Some(session);
        event.direction = Some(details.direction);
        event.size = Some(details);
        self.event(now, event)?;
        Ok(())
    }
    pub fn finish_session(&mut self, session: OpaqueId) -> Result<(), DiagnosticError> {
        for entry in self
            .failures
            .iter_mut()
            .filter(|e| e.session_id == Some(session) && !e.terminal)
        {
            self.failure_sequence = self
                .failure_sequence
                .checked_add(1)
                .ok_or(DiagnosticError::Arithmetic)?;
            entry.sequence = self.failure_sequence;
            entry.terminal = true;
        }
        Ok(())
    }
    fn acknowledge(&mut self) {
        if let Some((batch, covered, failures)) = self.in_flight {
            // A later chunk's ACK does not prove an earlier chunk succeeded.
            if self.queue.shared.write_failures.load(Ordering::Acquire) > failures {
                self.in_flight = None;
            } else if self.queue.shared.status_ack.load(Ordering::Acquire) >= batch {
                self.failures.retain(|e| e.sequence > covered);
                self.in_flight = None;
            }
        }
    }
    pub fn pending_failures(&self) -> usize {
        self.failures.len()
    }
    pub fn coverage_lost(&self) -> u64 {
        self.coverage_lost
    }
    pub fn snapshot(&mut self, now: Instant, counters: Snapshot) -> Result<bool, DiagnosticError> {
        self.publish_snapshot(now, counters, false)
    }
    /// Shutdown-only status attempt. Preserve one batch in flight and its ACK
    /// semantics, while bypassing ordinary cadence after packet work has ended.
    pub fn final_snapshot(
        &mut self,
        now: Instant,
        counters: Snapshot,
    ) -> Result<bool, DiagnosticError> {
        self.publish_snapshot(now, counters, true)
    }
    pub fn final_status_complete(&mut self) -> bool {
        self.acknowledge();
        self.failures.is_empty() && self.in_flight.is_none()
    }
    fn publish_snapshot(
        &mut self,
        now: Instant,
        counters: Snapshot,
        final_status: bool,
    ) -> Result<bool, DiagnosticError> {
        self.acknowledge();
        if self.in_flight.is_some() {
            return Ok(false);
        }
        if !final_status
            && let Some(last) = self.last_snapshot
            && now
                .checked_duration_since(last)
                .ok_or(DiagnosticError::Clock)?
                < self.snapshot_interval
        {
            return Ok(false);
        }
        if final_status && self.failures.is_empty() {
            return Ok(false);
        }
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or(DiagnosticError::Arithmetic)?;
        let sequence = self.sequence;
        let covered = self.failure_sequence;
        let entries: Vec<_> = self.failures.iter().copied().collect();
        let count = entries.len().max(1).div_ceil(128);
        let mut prepared = Vec::with_capacity(count);
        for index in 0..count {
            let begin = (index * 128).min(entries.len());
            let end = (begin + 128).min(entries.len());
            let body = Status {
                event: "snapshot",
                counters,
                diagnostics_dropped: self.queue.dropped(),
                status_write_failures: self.queue.shared.write_failures.load(Ordering::Relaxed),
                counter_overflow: self.queue.shared.overflow.load(Ordering::Relaxed)
                    || counters.recovery.counter_overflow
                    || counters.quic.overflow
                    || counters.qlog.overflow
                    || counters.profile.factory.overflowed
                    || counters.profile.io.overflowed
                    || counters.histograms.application_queue_age_us.overflow
                    || counters.histograms.ciphertext_age_us.overflow
                    || counters.histograms.pacing_lateness_us.overflow
                    || counters.histograms.emitted_burst_datagrams.overflow
                    || counters.histograms.recovery_duration_us.overflow,
                failures: &entries[begin..end],
                failure_totals: self.failure_totals,
                chunk_index: index,
                chunk_count: count,
                coverage_lost: self.coverage_lost,
                lost_first: self.lost_first,
                lost_last: self.lost_last,
            };
            let record = Envelope {
                schema_version: 1,
                timestamp: time::OffsetDateTime::now_utc()
                    .format(&time::format_description::well_known::Rfc3339)
                    .map_err(|_| DiagnosticError::Format)?,
                sequence,
                role: self.role,
                process_epoch: OpaqueId {
                    epoch: self.ids.epoch,
                    serial: 0,
                },
                body,
            };
            prepared.push(self.queue.prepare(
                true,
                &record,
                if index + 1 == count { sequence } else { 0 },
            )?);
        }
        self.in_flight = Some((
            sequence,
            covered,
            self.queue.shared.write_failures.load(Ordering::Relaxed),
        ));
        for record in prepared {
            if let Err(error) = self.queue.publish(record) {
                increment(&self.queue.shared.write_failures, &self.queue.shared);
                return Err(error);
            }
        }
        self.last_snapshot = Some(now);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tiny_records_exhaust_counts_with_status_reservation_and_inflight_ownership() {
        let (queue, receiver) = OutputQueue::new();
        for _ in 0..EVENT_RECORDS {
            queue.enqueue(false, &EventName::Starting).unwrap();
        }
        assert_eq!(
            queue.enqueue(false, &EventName::Starting),
            Err(DiagnosticError::Full)
        );
        for _ in 0..STATUS_RECORDS {
            queue.enqueue(true, &EventName::Starting).unwrap();
        }
        assert_eq!(queue.retained_records(), OUTPUT_RECORDS);
        let held = receiver.try_next().unwrap().unwrap();
        assert!(held.is_status());
        assert_eq!(
            queue.enqueue(true, &EventName::Starting),
            Err(DiagnosticError::Full)
        );
        drop(held);
        queue.enqueue(true, &EventName::Starting).unwrap();
        assert_eq!(queue.retained_records(), OUTPUT_RECORDS);
        drop(receiver);
        assert_eq!(queue.retained_records(), 0);
        assert_eq!(queue.retained_bytes(), 0);
    }
    #[test]
    fn byte_caps_include_inflight_records_and_status_is_reserved() {
        let (queue, receiver) = OutputQueue::new();
        let event = "x".repeat(60000);
        while queue.enqueue(false, &event).is_ok() {}
        assert!(queue.shared.event_bytes.load(Ordering::Relaxed) <= EVENT_BYTES);
        queue.enqueue(true, &Snapshot::default()).unwrap();
        let status = receiver.try_next().unwrap().unwrap();
        assert!(status.is_status());
        let retained = queue.retained_bytes();
        drop(status);
        assert!(queue.retained_bytes() < retained);
        assert!(queue.retained_bytes() <= OUTPUT_BYTES);
        drop(receiver);
        assert_eq!(queue.retained_bytes(), 0);
    }
}
