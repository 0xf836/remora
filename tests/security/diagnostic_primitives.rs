use crate::common::FakeClock;
use remora::{
    config::{Level, Offload, ValidatedConfig},
    diagnostics::*,
};
use std::{
    io::{self, Write},
    time::Duration,
};

#[test]
fn fixed_drop_counters_preserve_direction_and_fail_explicitly_on_overflow() {
    let mut counters = DropCounters::default();
    counters
        .increment(DropReason::QueueFull, remora::buffers::Direction::ToTarget)
        .unwrap();
    assert_eq!(counters.queue_full.to_target, 1);
    assert_eq!(counters.queue_full.to_client, 0);
    counters.queue_full.to_target = u64::MAX;
    assert_eq!(
        counters.increment(DropReason::QueueFull, remora::buffers::Direction::ToTarget),
        Err(DiagnosticError::Arithmetic)
    );
    assert_eq!(counters.queue_full.to_target, u64::MAX);
}

struct BlockedSink {
    entered: std::sync::mpsc::SyncSender<()>,
    release: std::sync::mpsc::Receiver<()>,
}
impl Write for BlockedSink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.entered.try_send(()).unwrap();
        self.release
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| io::ErrorKind::TimedOut)?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
#[test]
fn writer_flush_wait_is_bounded_and_blocked_record_remains_charged() {
    let clock = FakeClock::new();
    let (mut diagnostics, receiver) =
        Diagnostics::new(Role::Client, clock.now(), Duration::from_secs(1)).unwrap();
    let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(1);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
    let writer = receiver
        .spawn(
            Vec::new(),
            BlockedSink {
                entered: entered_tx,
                release: release_rx,
            },
        )
        .unwrap();
    diagnostics
        .event(
            clock.now(),
            Event::failure(Level::Warn, Failure::Socket, 11),
        )
        .unwrap();
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(diagnostics.queue().retained_records(), 1);
    let before = std::time::Instant::now();
    assert!(!writer.finish(Duration::from_millis(5)));
    assert!(before.elapsed() < Duration::from_secs(1));
    assert_eq!(diagnostics.queue().retained_records(), 1);
    release_tx.send(()).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while diagnostics.queue().retained_records() != 0 {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
}
#[test]
fn writer_does_not_report_successful_flush_after_broken_output() {
    let clock = FakeClock::new();
    let (mut diagnostics, receiver) =
        Diagnostics::new(Role::Client, clock.now(), Duration::from_secs(1)).unwrap();
    diagnostics
        .event(
            clock.now(),
            Event::failure(Level::Error, Failure::Socket, 5),
        )
        .unwrap();
    let writer = receiver.spawn(Vec::new(), Broken).unwrap();
    assert!(!writer.finish(Duration::from_secs(2)));
    assert_eq!(diagnostics.queue().retained_records(), 0);
}

#[test]
fn bucket_has_exact_burst_rate_and_rejects_clock_reversal() {
    let mut clock = FakeClock::new();
    let mut bucket = EventBucket::new(clock.now());
    for _ in 0..40 {
        assert!(bucket.admit(clock.now()).unwrap());
    }
    assert!(!bucket.admit(clock.now()).unwrap());
    clock.advance(Duration::from_millis(49));
    assert!(!bucket.admit(clock.now()).unwrap());
    clock.advance(Duration::from_millis(1));
    assert!(bucket.admit(clock.now()).unwrap());
    assert!(!bucket.admit(clock.now()).unwrap());
    clock.advance(Duration::from_secs(100));
    for _ in 0..40 {
        assert!(bucket.admit(clock.now()).unwrap());
    }
    assert!(!bucket.admit(clock.now()).unwrap());
    assert_eq!(
        bucket.admit(clock.now() - Duration::from_secs(1)),
        Err(DiagnosticError::Clock)
    );
}
#[test]
fn typed_schema_and_safe_error_text_exclude_sentinels_at_every_level() {
    let clock = FakeClock::new();
    let (mut diagnostics, receiver) =
        Diagnostics::new(Role::Client, clock.now(), Duration::from_secs(1)).unwrap();
    // No API argument can carry these strings; failure text is reduced to a
    // fixed class/numeric code before crossing the diagnostic boundary.
    let sentinels = [
        "token-SECRET-SENTINEL",
        "Authorization: Bearer SECRET",
        "payload-SECRET",
        "peer-close-SECRET",
    ];
    let connection = diagnostics.next_id().unwrap();
    let session = diagnostics.next_id().unwrap();
    assert!(connection != session);
    let error = ValidatedConfig::load(std::path::Path::new("missing-SECRET-SENTINEL-config.toml"))
        .err()
        .unwrap();
    for level in [
        Level::Error,
        Level::Warn,
        Level::Info,
        Level::Debug,
        Level::Trace,
    ] {
        let mut event = Event::failure(level, Failure::Authentication, 401);
        event.connection_id = Some(connection);
        event.session_id = Some(session);
        assert!(diagnostics.event(clock.now(), event).unwrap());
    }
    let mut events = Vec::new();
    let mut stdout = Vec::new();
    while receiver.write_one(&mut stdout, &mut events).unwrap() {}
    assert!(stdout.is_empty());
    let text = String::from_utf8(events).unwrap();
    for sentinel in sentinels {
        assert!(!text.contains(sentinel));
        assert!(!format!("{error} {error:?}").contains(sentinel));
    }
    for (index, line) in text.lines().enumerate() {
        let json: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(json["schema_version"], 1);
        assert_eq!(json["sequence"], index + 1);
        assert_eq!(json["event"], "failure");
        assert_eq!(json["failure"], "authentication");
        assert_eq!(json["numeric_code"], 401);
        assert!(json["timestamp"].as_str().unwrap().ends_with('Z'));
        for forbidden in [
            "token",
            "token_sha256",
            "payload",
            "headers",
            "target",
            "peer_error",
            "source",
        ] {
            assert!(json.get(forbidden).is_none());
        }
    }
}
#[test]
fn ordinary_saturation_preserves_status_count_reservation_and_inflight_charge() {
    let mut clock = FakeClock::new();
    let (mut diagnostics, receiver) =
        Diagnostics::new(Role::Server, clock.now(), Duration::from_secs(1)).unwrap();
    for _ in 0..768 {
        assert!(
            diagnostics
                .event(
                    clock.now(),
                    Event::failure(Level::Info, Failure::Resource, 0)
                )
                .unwrap()
        );
        clock.advance(Duration::from_millis(50));
    }
    assert_eq!(
        diagnostics.event(
            clock.now(),
            Event::failure(Level::Info, Failure::Resource, 0)
        ),
        Err(DiagnosticError::Full)
    );
    assert!(
        diagnostics
            .snapshot(clock.now(), Snapshot::default())
            .unwrap()
    );
    assert_eq!(diagnostics.queue().retained_records(), 769);
    clock.advance(Duration::from_secs(1));
    assert!(
        !diagnostics
            .snapshot(clock.now(), Snapshot::default())
            .unwrap()
    );
    // Writer-held output stays charged. Dropping without completed writing is
    // not an acknowledgement and cannot authorize another status batch.
    let held = receiver.try_next().unwrap().unwrap();
    assert!(held.is_status());
    assert_eq!(diagnostics.queue().retained_records(), 769);
    drop(held);
    assert_eq!(diagnostics.queue().retained_records(), 768);
    assert!(
        !diagnostics
            .snapshot(clock.now(), Snapshot::default())
            .unwrap()
    );
}
#[test]
fn suppressed_events_do_not_suppress_stdout_snapshot_and_interval_is_enforced() {
    let mut clock = FakeClock::new();
    let (mut diagnostics, receiver) =
        Diagnostics::new(Role::Client, clock.now(), Duration::from_secs(1)).unwrap();
    for _ in 0..40 {
        assert!(
            diagnostics
                .event(
                    clock.now(),
                    Event::failure(Level::Debug, Failure::Socket, 11)
                )
                .unwrap()
        );
    }
    assert!(
        !diagnostics
            .event(
                clock.now(),
                Event::failure(Level::Trace, Failure::Socket, 11)
            )
            .unwrap()
    );
    assert!(
        diagnostics
            .snapshot(clock.now(), Snapshot::default())
            .unwrap()
    );
    assert!(
        !diagnostics
            .snapshot(clock.now(), Snapshot::default())
            .unwrap()
    );
    clock.advance(Duration::from_millis(999));
    assert!(
        !diagnostics
            .snapshot(clock.now(), Snapshot::default())
            .unwrap()
    );
    let mut first_status = Vec::new();
    receiver
        .write_one(&mut first_status, &mut std::io::sink())
        .unwrap();
    clock.advance(Duration::from_millis(1));
    assert!(
        diagnostics
            .snapshot(clock.now(), Snapshot::default())
            .unwrap()
    );
    let mut stdout = first_status;
    let mut stderr = Vec::new();
    while receiver.write_one(&mut stdout, &mut stderr).unwrap() {}
    let text = String::from_utf8(stdout).unwrap();
    assert_eq!(text.lines().count(), 2);
    for line in text.lines() {
        let record: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(record["event"], "snapshot");
        assert_eq!(record["diagnostics_dropped"], 1);
    }
    assert_eq!(String::from_utf8(stderr).unwrap().lines().count(), 40);
}
struct Broken;
impl Write for Broken {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::ErrorKind::BrokenPipe.into())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
#[test]
fn broken_sink_is_counted_and_releases_charge_without_affecting_producer() {
    let clock = FakeClock::new();
    let (mut diagnostics, receiver) =
        Diagnostics::new(Role::Client, clock.now(), Duration::from_secs(1)).unwrap();
    diagnostics
        .event(
            clock.now(),
            Event::failure(Level::Error, Failure::Transport, 7),
        )
        .unwrap();
    assert_eq!(
        receiver.write_one(&mut Vec::new(), &mut Broken),
        Err(DiagnosticError::Disconnected)
    );
    assert_eq!(diagnostics.queue().retained_bytes(), 0);
    diagnostics
        .snapshot(clock.now(), Snapshot::default())
        .unwrap();
    let record = receiver.try_next().unwrap().unwrap();
    let json: serde_json::Value = serde_json::from_slice(record.bytes()).unwrap();
    assert_eq!(json["status_write_failures"], 1);
    drop(record);
    drop(receiver);
    assert_eq!(
        diagnostics.event(
            clock.now(),
            Event::failure(Level::Error, Failure::Socket, 5)
        ),
        Err(DiagnosticError::Disconnected)
    );
    assert_eq!(diagnostics.queue().retained_records(), 0);
}
#[test]
fn process_scoped_ids_offload_reasons_and_memory_ceiling_are_explicit() {
    let clock = FakeClock::new();
    let (mut first, _) =
        Diagnostics::new(Role::Client, clock.now(), Duration::from_secs(1)).unwrap();
    let (mut second, _) =
        Diagnostics::new(Role::Client, clock.now(), Duration::from_secs(1)).unwrap();
    assert!(first.next_id().unwrap() != second.next_id().unwrap());
    assert_eq!(Offload::Off.deferred_reason(), OffloadReason::ConfiguredOff);
    assert_eq!(
        Offload::Auto.deferred_reason(),
        OffloadReason::NotImplemented
    );
    assert_eq!(TOTAL_BYTES, 2 * 1024 * 1024);
    assert!(Diagnostics::new(Role::Client, clock.now(), Duration::from_millis(999)).is_err());
}
