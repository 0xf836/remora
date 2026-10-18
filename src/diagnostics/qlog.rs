//! Fail-closed JSON-SEQ boundary for pinned quiche 0.30.0 / qlog 0.18.1.
//! Raw bytes exist only in a fixed scratch buffer. Serde ignores unlisted fields
//! without materializing strings, objects or arrays; persistence receives a new
//! typed record. No HTTP/3 event, frame, CID, reason text or user data survives.
use super::OutputQueue;
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::{self, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
pub const SCRATCH_BYTES: usize = 64 * 1024;
pub const MAX_CONNECTIONS: usize = 8;
pub const CONNECTION_FILE_BYTES: usize = 16 * 1024 * 1024;
pub const DIRECTORY_BYTES: usize = 128 * 1024 * 1024;
const INPUT_BYTES: usize = SCRATCH_BYTES / 2;
const SECOND_BYTES: usize = 1024 * 1024;
const SECOND_EVENTS: usize = 1000;

#[derive(Default, Clone, Copy, Serialize)]
pub struct Report {
    pub active_filters: usize,
    pub scratch_bytes: usize,
    pub accepted_events: u64,
    pub discarded_events: u64,
    pub disabled_filters: u64,
    pub activation_denied: u64,
    pub reserved_file_bytes: usize,
    pub output_failures: u64,
    pub overflow: bool,
    pub stops: StopCounts,
}
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(usize)]
enum StopReason {
    Schema,
    Oversize,
    Rate,
    Retention,
    Output,
    Queue,
    Incomplete,
    Clock,
}
#[derive(Default, Clone, Copy)]
pub struct StopCounts {
    counts: [u64; 8],
}
impl Serialize for StopCounts {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(8))?;
        for (name, count) in [
            "schema",
            "oversize",
            "rate",
            "retention",
            "output",
            "queue",
            "incomplete",
            "clock",
        ]
        .into_iter()
        .zip(self.counts)
        {
            map.serialize_entry(name, &count)?;
        }
        map.end()
    }
}
struct State {
    report: Report,
    next: u64,
    window: Instant,
    events: usize,
    bytes: usize,
}
struct Shared {
    directory: Arc<File>,
    state: Mutex<State>,
    failures: AtomicU64,
}
/// Startup-only secure directory open. Existing output is refused rather than
/// silently ignoring its retention. Owner supplies a fresh mode-0700 directory.
pub fn open_directory(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC)
        .open(path)?;
    let m = file.metadata()?;
    if m.uid() != rustix::process::geteuid().as_raw() || m.mode() & 0o777 != 0o700 {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    // Inspect via the held directory, avoiding substitution of the pathname.
    use std::os::fd::AsRawFd;
    if std::fs::read_dir(format!("/proc/self/fd/{}", file.as_raw_fd()))?
        .next()
        .is_some()
    {
        return Err(io::ErrorKind::AlreadyExists.into());
    }
    Ok(file)
}
#[derive(Clone)]
pub struct Pool {
    shared: Arc<Shared>,
    queue: OutputQueue,
}
impl Pool {
    pub fn new(path: &Path, queue: OutputQueue, now: Instant) -> io::Result<Self> {
        Ok(Self {
            shared: Arc::new(Shared {
                directory: Arc::new(open_directory(path)?),
                state: Mutex::new(State {
                    report: Report::default(),
                    next: 0,
                    window: now,
                    events: 0,
                    bytes: 0,
                }),
                failures: AtomicU64::new(0),
            }),
            queue,
        })
    }
    /// Reserve global scratch before allocation; failure has no packet effect.
    /// Half of the 64 KiB charge holds input, half bounds serde's transient
    /// escape/nesting scratch (at most input length, rounded to power of two).
    pub fn activate(&self) -> Option<Filter> {
        let mut state = self.shared.state.lock().ok()?;
        if state.report.active_filters >= MAX_CONNECTIONS
            || state.next == u64::MAX
            || state.report.reserved_file_bytes >= DIRECTORY_BYTES
        {
            bump(&mut state.report, |r| &mut r.activation_denied);
            return None;
        }
        state.next += 1;
        let destination = Arc::new(Destination {
            shared: self.shared.clone(),
            serial: state.next,
            file: Mutex::new(None),
            failed: AtomicBool::new(false),
        });
        state.report.active_filters += 1;
        state.report.scratch_bytes += SCRATCH_BYTES;
        drop(state);
        Some(Filter {
            scratch: vec![0; INPUT_BYTES].into_boxed_slice(),
            used: 0,
            collecting: false,
            header: false,
            disabled: false,
            destination,
            file_bytes: 0,
            pool: self.clone(),
        })
    }
    pub fn report(&self) -> Report {
        let mut r = self.shared.state.lock().expect("qlog state").report;
        r.output_failures = self.shared.failures.load(Ordering::Relaxed);
        r
    }
}
fn bump(report: &mut Report, select: fn(&mut Report) -> &mut u64) {
    let value = select(report);
    if let Some(next) = value.checked_add(1) {
        *value = next;
    } else {
        report.overflow = true;
    }
}
// A destination is shared only with bounded queued records; file creation and
// writes occur solely on the diagnostic writer, after sanitization.
pub(super) struct Destination {
    shared: Arc<Shared>,
    serial: u64,
    file: Mutex<Option<File>>,
    failed: AtomicBool,
}
impl Destination {
    pub(super) fn persist(&self, bytes: &[u8]) -> io::Result<()> {
        if self.failed.load(Ordering::Acquire) {
            return Ok(());
        }
        let mut slot = self.file.lock().map_err(|_| io::ErrorKind::Other)?;
        if slot.is_none() {
            use rustix::fs::{Mode, OFlags, openat};
            let fd = openat(
                &*self.shared.directory,
                format!("trace-{}.ndjson", self.serial),
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::RUSR | Mode::WUSR,
            )?;
            *slot = Some(File::from(fd));
        }
        slot.as_mut().expect("opened qlog file").write_all(bytes)
    }
    pub(super) fn fail(&self) {
        if !self.failed.swap(true, Ordering::AcqRel) {
            let _ = self
                .shared
                .failures
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1));
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Copy, PartialEq)]
enum Schema {
    #[serde(rename = "urn:ietf:params:qlog:file:sequential")]
    Sequential,
    #[serde(other)]
    Unknown,
}
#[derive(Deserialize, Clone, Copy, PartialEq)]
enum Format {
    #[serde(rename = "JSON-SEQ")]
    JsonSeq,
    #[serde(other)]
    Unknown,
}
#[derive(Deserialize, Serialize, Clone, Copy, PartialEq)]
enum Name {
    #[serde(rename = "quic:packet_sent")]
    Sent,
    #[serde(rename = "quic:packet_received")]
    Received,
    #[serde(rename = "quic:packet_lost")]
    Lost,
    #[serde(rename = "quic:recovery_metrics_updated")]
    Metrics,
    #[serde(other)]
    Unknown,
}
#[derive(Deserialize, Serialize, Clone, Copy)]
enum PacketType {
    #[serde(rename = "initial")]
    Initial,
    #[serde(rename = "handshake")]
    Handshake,
    #[serde(rename = "1RTT")]
    OneRtt,
    #[serde(rename = "1rtt")]
    OneRttLower,
    #[serde(rename = "retry")]
    Retry,
    #[serde(rename = "version_negotiation")]
    VersionNegotiation,
    #[serde(other)]
    Unknown,
}
#[derive(Default, Deserialize, Serialize)]
struct Header {
    #[serde(skip_serializing_if = "Option::is_none")]
    packet_number: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    packet_type: Option<PacketType>,
}
#[derive(Default, Deserialize)]
struct Raw {
    length: Option<u64>,
    payload_length: Option<u64>,
}
#[derive(Default, Deserialize, Serialize)]
struct Data {
    #[serde(skip_serializing_if = "Option::is_none")]
    header: Option<Header>,
    #[serde(skip_serializing)]
    raw: Option<Raw>,
    #[serde(skip_serializing_if = "Option::is_none")]
    smoothed_rtt: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    latest_rtt: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rtt_variance: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    congestion_window: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bytes_in_flight: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pto_count: Option<u64>,
}
#[derive(Deserialize)]
struct Input {
    file_schema: Option<Schema>,
    serialization_format: Option<Format>,
    time: Option<f64>,
    name: Option<Name>,
    data: Option<Data>,
}
#[derive(Serialize)]
struct SafeRecord {
    schema_version: u32,
    event: &'static str,
    time: f64,
    name: Name,
    data: Data,
    packet_length: Option<u64>,
    payload_length: Option<u64>,
}
pub struct Filter {
    scratch: Box<[u8]>,
    used: usize,
    collecting: bool,
    header: bool,
    disabled: bool,
    destination: Arc<Destination>,
    file_bytes: usize,
    pool: Pool,
}
impl Filter {
    pub fn disabled(&self) -> bool {
        self.disabled || self.destination.failed.load(Ordering::Acquire)
    }
    fn disable(&mut self, reason: StopReason) {
        if !self.disabled {
            self.disabled = true;
            self.used = 0;
            let mut state = self.pool.shared.state.lock().expect("qlog state");
            bump(&mut state.report, |r| &mut r.disabled_filters);
            let counter = &mut state.report.stops.counts[reason as usize];
            if let Some(next) = counter.checked_add(1) {
                *counter = next;
            } else {
                state.report.overflow = true;
            }
        }
    }
    fn record(&mut self, now: Instant) -> Result<(), StopReason> {
        let input: Input =
            serde_json::from_slice(&self.scratch[..self.used]).map_err(|_| StopReason::Schema)?;
        if !self.header {
            if input.file_schema != Some(Schema::Sequential)
                || input.serialization_format != Some(Format::JsonSeq)
            {
                return Err(StopReason::Schema);
            }
            self.header = true;
            return Ok(());
        }
        if input.file_schema.is_some() {
            return Err(StopReason::Schema);
        }
        let name = input.name.ok_or(StopReason::Schema)?;
        if name == Name::Unknown {
            bump(
                &mut self.pool.shared.state.lock().expect("qlog state").report,
                |r| &mut r.discarded_events,
            );
            return Ok(());
        }
        let time = input
            .time
            .filter(|n| n.is_finite() && *n >= 0.0)
            .ok_or(StopReason::Schema)?;
        let mut data = input.data.ok_or(StopReason::Schema)?;
        for metric in [data.smoothed_rtt, data.latest_rtt, data.rtt_variance]
            .into_iter()
            .flatten()
        {
            if !metric.is_finite() || metric < 0.0 {
                return Err(StopReason::Schema);
            }
        }
        let raw = data.raw.take().unwrap_or_default();
        let record = SafeRecord {
            schema_version: 1,
            event: "filtered_qlog",
            time,
            name,
            data,
            packet_length: raw.length,
            payload_length: raw.payload_length,
        };
        // Two passes into finite storage, never an unbounded JSON Value tree.
        let mut buffer = [0u8; 2048];
        let mut cursor = io::Cursor::new(&mut buffer[..]);
        serde_json::to_writer(&mut cursor, &record).map_err(|_| StopReason::Schema)?;
        let size = cursor.position() as usize + 1;
        let mut state = self
            .pool
            .shared
            .state
            .lock()
            .map_err(|_| StopReason::Output)?;
        if now
            .checked_duration_since(state.window)
            .ok_or(StopReason::Clock)?
            >= Duration::from_secs(1)
        {
            state.window = now;
            state.events = 0;
            state.bytes = 0;
        }
        if state.events >= SECOND_EVENTS || state.bytes + size > SECOND_BYTES {
            return Err(StopReason::Rate);
        }
        if self.file_bytes + size > CONNECTION_FILE_BYTES
            || state.report.reserved_file_bytes + size > DIRECTORY_BYTES
        {
            return Err(StopReason::Retention);
        }
        // Keep lifetime reservation on failed disk writes: partial output also
        // occupies capacity. No deletion/rotation or per-connection history.
        state.events += 1;
        state.bytes += size;
        state.report.reserved_file_bytes += size;
        self.file_bytes += size;
        drop(state);
        self.pool
            .queue
            .filtered_qlog(&record, self.destination.clone())
            .map_err(|_| StopReason::Queue)?;
        bump(
            &mut self.pool.shared.state.lock().expect("qlog state").report,
            |r| &mut r.accepted_events,
        );
        Ok(())
    }
    /// Deterministic clock entrypoint used by pressure tests; no sink I/O here.
    pub fn ingest(&mut self, bytes: &[u8], now: Instant) {
        if self.disabled() {
            self.disable(StopReason::Output);
            return;
        }
        for &byte in bytes {
            if byte == 0x1e {
                if self.collecting || self.used != 0 {
                    self.disable(StopReason::Schema);
                    break;
                }
                self.collecting = true;
            } else if byte == b'\n' {
                let result = if !self.collecting || self.used == 0 {
                    Err(StopReason::Schema)
                } else {
                    self.record(now)
                };
                if let Err(reason) = result {
                    self.disable(reason);
                    break;
                }
                self.used = 0;
                self.collecting = false;
            } else if self.collecting && self.used < INPUT_BYTES {
                self.scratch[self.used] = byte;
                self.used += 1;
            } else {
                self.disable(if self.collecting {
                    StopReason::Oversize
                } else {
                    StopReason::Schema
                });
                break;
            }
        }
    }
}
impl Write for Filter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.ingest(bytes, Instant::now());
        // Always consume upstream input: disabled tracing cannot break QUIC.
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl Drop for Filter {
    fn drop(&mut self) {
        if self.used != 0 || self.collecting {
            self.disable(StopReason::Incomplete);
        }
        let mut state = self.pool.shared.state.lock().expect("qlog state");
        state.report.active_filters -= 1;
        state.report.scratch_bytes -= SCRATCH_BYTES;
    }
}
