//! Privacy-safe records and finite nonblocking handoff. There is deliberately
//! no global tracing subscriber or arbitrary-string logging API. Terminal
//! retention/acknowledgements and qlog filtering are later increments.
use crate::{buffers::Direction, config::Level};
use serde::Serialize;
use std::{
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
pub enum Failure {
    InvalidConfiguration,
    Trust,
    Authentication,
    Authorization,
    Capability,
    SetupTimeout,
    PeerTimeout,
    Transport,
    Socket,
    Resource,
    Malformed,
    Internal,
    Stopped,
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
        }
    }
}
#[derive(Default, Clone, Copy, Serialize)]
pub struct Snapshot {
    pub sessions_active: u64,
    pub connections_active: u64,
    pub counters: Counters,
}
#[derive(Default, Clone, Copy, Serialize)]
pub struct Counters {
    pub app_ingress_datagrams: u64,
    pub app_ingress_bytes: u64,
    pub quic_dgram_enqueued: u64,
    pub relay_socket_sent: u64,
    pub local_drops: DropCounters,
}
#[derive(Default, Clone, Copy, Serialize)]
pub struct DirectionCounts {
    pub to_target: u64,
    pub to_client: u64,
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
        *count = count.checked_add(1).ok_or(DiagnosticError::Arithmetic)?;
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
#[derive(Serialize)]
struct Status {
    event: &'static str,
    #[serde(flatten)]
    counters: Snapshot,
    diagnostics_dropped: u64,
    status_write_failures: u64,
    counter_overflow: bool,
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
        let result = self.enqueue_inner(status, value);
        if result.is_err() {
            increment(&self.shared.dropped, &self.shared);
        }
        result
    }
    fn enqueue_inner<T: Serialize>(&self, status: bool, value: &T) -> Result<(), DiagnosticError> {
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
        let (counter, cap, records, record_cap, sender) = if status {
            (
                &self.shared.status_bytes,
                STATUS_BYTES,
                &self.shared.status_records,
                STATUS_RECORDS,
                &self.status,
            )
        } else {
            (
                &self.shared.event_bytes,
                EVENT_BYTES,
                &self.shared.event_records,
                EVENT_RECORDS,
                &self.event,
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
        let sink: &mut dyn Write = if record.status { stdout } else { stderr };
        if sink
            .write_all(record.bytes())
            .and_then(|_| sink.flush())
            .is_err()
        {
            increment(&self.shared.write_failures, &self.shared);
            return Err(DiagnosticError::Disconnected);
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
            },
            receiver,
        ))
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
        if !self.bucket.admit(now)? {
            increment(&self.queue.shared.dropped, &self.queue.shared);
            return Ok(false);
        }
        let record = self.envelope(event)?;
        self.queue.enqueue(false, &record)?;
        Ok(true)
    }
    pub fn snapshot(&mut self, now: Instant, counters: Snapshot) -> Result<bool, DiagnosticError> {
        if let Some(last) = self.last_snapshot
            && now
                .checked_duration_since(last)
                .ok_or(DiagnosticError::Clock)?
                < self.snapshot_interval
        {
            return Ok(false);
        }
        let record = self.envelope(Status {
            event: "snapshot",
            counters,
            diagnostics_dropped: self.queue.dropped(),
            status_write_failures: self.queue.shared.write_failures.load(Ordering::Relaxed),
            counter_overflow: self.queue.shared.overflow.load(Ordering::Relaxed),
        })?;
        self.queue.enqueue(true, &record)?;
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
