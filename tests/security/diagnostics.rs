use remora::{
    config::Level,
    diagnostics::{Diagnostics, Event, Failure, MAX_RECORD_BYTES, Role, Snapshot},
};
use std::{
    io,
    time::{Duration, Instant},
};

#[test]
fn json_without_periodic_metrics_keeps_correlated_failures_and_shutdown() {
    use remora::config::LogFormat;
    let now = Instant::now();
    let (mut d, r) = Diagnostics::new(Role::Server, now, Duration::from_secs(1)).unwrap();
    d.set_output(LogFormat::Json, false);
    d.set_level(Level::Error);
    for tick in 0..3600 {
        assert!(
            !d.snapshot(now + Duration::from_secs(tick), Snapshot::default())
                .unwrap()
        );
    }
    assert_eq!(d.queue().retained_bytes(), 0);
    assert!(
        !d.event(now, Event::failure(Level::Info, Failure::Transport, 0))
            .unwrap()
    );
    let connection = d.next_id().unwrap();
    let session = d.next_id().unwrap();
    for _ in 0..100 {
        d.failure(
            now,
            Some(connection),
            Some(session),
            Failure::Socket,
            111,
            false,
        )
        .unwrap();
    }
    d.finish_session(session).unwrap();
    assert!(d.snapshot(now, Snapshot::default()).unwrap());
    let mut out = Vec::new();
    r.write_one(&mut out, &mut io::sink()).unwrap();
    assert!(d.final_status_complete());
    let record: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(record["failures"][0]["repeats"], 100);
    assert_eq!(record["failures"][0]["code"], 111);
    assert_eq!(
        record["failures"][0]["session"],
        serde_json::to_value(session).unwrap()
    );
    assert_eq!(record["coverage_lost"], 0);
    assert!(
        !d.snapshot(now + Duration::from_secs(1), Snapshot::default())
            .unwrap()
    );
    d.failure(now, None, None, Failure::Stopped, 0, true)
        .unwrap();
    assert!(d.final_snapshot(now, Snapshot::default()).unwrap());
    let mut out = Vec::new();
    r.write_one(&mut out, &mut io::sink()).unwrap();
    let record: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(record["failures"][0]["class"], "stopped");
    assert!(d.final_status_complete());
}

#[test]
fn json_without_periodic_metrics_reports_new_collection_gaps_without_repeating_unchanged_health() {
    use remora::config::LogFormat;
    let now = Instant::now();
    let (mut d, r) = Diagnostics::new(Role::Server, now, Duration::from_secs(1)).unwrap();
    d.set_output(LogFormat::Json, false);
    for _ in 0..41 {
        let _ = d.event(now, Event::failure(Level::Warn, Failure::Socket, 5));
    }
    assert!(d.snapshot(now, Snapshot::default()).unwrap());
    let mut out = Vec::new();
    r.write_one(&mut out, &mut io::sink()).unwrap();
    assert!(
        String::from_utf8(out)
            .unwrap()
            .contains("\"diagnostics_dropped\":1")
    );
    assert!(
        !d.snapshot(now + Duration::from_secs(1), Snapshot::default())
            .unwrap()
    );
    let mut counters = Snapshot::default();
    counters.quic.overflow = true;
    assert!(d.snapshot(now + Duration::from_secs(2), counters).unwrap());
    let mut out = Vec::new();
    r.write_one(&mut out, &mut io::sink()).unwrap();
    assert!(
        String::from_utf8(out)
            .unwrap()
            .contains("\"counter_overflow\":true")
    );
}
#[test]
fn repeated_size_details_are_bounded_and_survive_event_suppression() {
    use remora::{buffers::Direction, diagnostics::SizeDetails};
    let now = Instant::now();
    let (mut d, r) = Diagnostics::new(Role::Client, now, Duration::from_secs(1)).unwrap();
    let session = d.next_id().unwrap();
    let connection = d.next_id().unwrap();
    for _ in 0..50 {
        let _ = d.event(now, Event::failure(Level::Warn, Failure::Transport, 0));
    }
    for size in 1312..1412 {
        d.size_failure(
            now,
            Some(connection),
            session,
            2,
            SizeDetails {
                direction: Direction::ToTarget,
                datagram_bytes: size,
                application_cap_bytes: 1400,
                outgoing_ceiling_bytes: 1158,
            },
        )
        .unwrap();
    }
    assert_eq!(d.pending_failures(), 1);
    d.snapshot(now, Snapshot::default()).unwrap();
    let mut out = Vec::new();
    let mut err = Vec::new();
    for _ in 0..51 {
        r.write_one(&mut out, &mut err).unwrap();
        if !out.is_empty() {
            break;
        }
    }
    let value: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let failure = &value["failures"][0];
    assert_eq!(failure["repeats"], 100);
    assert_eq!(
        failure["connection"],
        serde_json::to_value(connection).unwrap()
    );
    assert_eq!(failure["size"]["direction"], "to_target");
    assert_eq!(failure["size"]["datagram_bytes"], 1411);
    assert_eq!(failure["size"]["application_cap_bytes"], 1400);
    assert_eq!(failure["size"]["outgoing_ceiling_bytes"], 1158);
    assert_eq!(value["coverage_lost"], 0);
    assert!(out.len() <= MAX_RECORD_BYTES);
    d.failure(
        now,
        Some(connection),
        Some(session),
        Failure::Size,
        3,
        false,
    )
    .unwrap();
    d.snapshot(now + Duration::from_secs(1), Snapshot::default())
        .unwrap();
    out.clear();
    r.write_one(&mut out, &mut err).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert!(value["failures"][0].get("size").is_none());
}
#[test]
fn terminal_correlation_survives_event_suppression_cleanup_and_old_ack() {
    let now = Instant::now();
    let (mut d, r) = Diagnostics::new(Role::Server, now, Duration::from_secs(1)).unwrap();
    let id = d.next_id().unwrap();
    let conn = d.next_id().unwrap();
    for _ in 0..50 {
        let _ = d.event(now, Event::failure(Level::Trace, Failure::Transport, 1));
    }
    d.failure(
        now,
        Some(conn),
        Some(id),
        Failure::Authentication,
        401,
        false,
    )
    .unwrap();
    d.failure(
        now,
        Some(conn),
        Some(id),
        Failure::Authorization,
        403,
        false,
    )
    .unwrap();
    assert_eq!(d.pending_failures(), 2);
    d.finish_session(id).unwrap();
    d.snapshot(now, Snapshot::default()).unwrap();
    // A repeat/new status recorded while the old snapshot is queued survives ACK.
    d.failure(now, Some(conn), Some(id), Failure::Socket, 5, false)
        .unwrap();
    let mut out = Vec::new();
    let mut err = Vec::new();
    r.write_one(&mut out, &mut err).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(value["failures"].as_array().unwrap().len(), 2);
    assert_eq!(value["coverage_lost"], 0);
    assert_eq!(
        value["failures"][0]["session"],
        serde_json::to_value(id).unwrap()
    );
    d.snapshot(now + Duration::from_secs(1), Snapshot::default())
        .unwrap();
    assert_eq!(d.pending_failures(), 1);
    out.clear();
    r.write_one(&mut out, &mut err).unwrap();
    assert!(String::from_utf8(out).unwrap().contains("socket"));
    assert_eq!(d.coverage_lost(), 0);
}
#[test]
fn full_status_ring_is_bounded_and_overflow_is_explicit() {
    let now = Instant::now();
    let (mut d, r) = Diagnostics::new(Role::Client, now, Duration::from_secs(1)).unwrap();
    for _ in 0..256 {
        let conn = d.next_id().unwrap();
        let id = d.next_id().unwrap();
        d.size_failure(
            now,
            Some(conn),
            id,
            u64::MAX,
            remora::diagnostics::SizeDetails {
                direction: remora::buffers::Direction::ToClient,
                datagram_bytes: usize::MAX,
                application_cap_bytes: usize::MAX,
                outgoing_ceiling_bytes: usize::MAX,
            },
        )
        .unwrap();
    }
    assert_eq!(d.pending_failures(), 256);
    assert_eq!(d.coverage_lost(), 0);
    d.snapshot(now, Snapshot::default()).unwrap();
    let mut out = Vec::new();
    r.write_one(&mut out, &mut io::sink()).unwrap();
    assert!(out.len() <= MAX_RECORD_BYTES);
    r.write_one(&mut out, &mut io::sink()).unwrap();
    assert!(
        out.split(|b| *b == b'\n')
            .all(|line| line.len() <= MAX_RECORD_BYTES)
    );
    assert!(d.queue().retained_bytes() <= 256 * 1024);
    let id = d.next_id().unwrap();
    d.failure(now, None, Some(id), Failure::Socket, 0, true)
        .unwrap();
    assert_eq!(d.pending_failures(), 256);
    assert_eq!(d.coverage_lost(), 1);
    d.snapshot(now + Duration::from_secs(1), Snapshot::default())
        .unwrap();
    assert_eq!(d.pending_failures(), 1);
}
#[test]
fn broken_writer_does_not_retire_status_and_same_class_update_survives_ack() {
    struct Broken;
    impl io::Write for Broken {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let now = Instant::now();
    let (mut d, r) = Diagnostics::new(Role::Client, now, Duration::from_secs(1)).unwrap();
    let id = d.next_id().unwrap();
    d.failure(now, None, Some(id), Failure::Transport, 1, false)
        .unwrap();
    d.snapshot(now, Snapshot::default()).unwrap();
    d.failure(now, None, Some(id), Failure::Transport, 2, false)
        .unwrap();
    assert!(r.write_one(&mut Broken, &mut io::sink()).is_err());
    assert_eq!(d.pending_failures(), 1);
    d.snapshot(now + Duration::from_secs(1), Snapshot::default())
        .unwrap();
    let mut out = Vec::new();
    r.write_one(&mut out, &mut io::sink()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["failures"][0]["repeats"], 2);
    assert_eq!(v["failures"][0]["code"], 2);
    assert_eq!(v["status_write_failures"], 1);
}

#[test]
fn operator_mode_never_collects_hidden_status_and_size_warning_is_once_per_process() {
    use remora::{config::LogFormat, diagnostics::EventName};
    let now = Instant::now();
    let (mut d, r) = Diagnostics::new(Role::Server, now, Duration::from_secs(1)).unwrap();
    d.set_output(LogFormat::Text, false);
    d.set_level(Level::Info);
    for _ in 0..10000 {
        let id = d.next_id().unwrap();
        d.failure(now, None, Some(id), Failure::Authentication, 401, true)
            .unwrap();
        assert!(
            !d.event(
                now,
                Event::failure(Level::Warn, Failure::Authentication, 401)
            )
            .unwrap()
        );
    }
    assert_eq!(d.pending_failures(), 0);
    assert_eq!(d.coverage_lost(), 0); // No capture requested, not complete audit evidence.
    assert_eq!(d.queue().retained_bytes(), 0);
    for _ in 0..100 {
        let _ = d.event(now, Event::failure(Level::Warn, Failure::Size, 3));
    }
    d.operator_event(
        now,
        Event::lifecycle(EventName::ProcessStopped, Level::Info),
    );
    let mut err = Vec::new();
    let mut out = Vec::new();
    while r.write_one(&mut out, &mut err).unwrap() {}
    let text = String::from_utf8(err).unwrap();
    assert_eq!(text.matches("WARN").count(), 1);
    assert!(text.contains("Reduce the application's datagram size"));
    assert!(text.contains("Remora stopped"));
    assert!(out.is_empty());
    for tick in 0..7200 {
        assert!(
            !d.snapshot(now + Duration::from_secs(tick), Snapshot::default())
                .unwrap()
        );
    }
    assert!(d.final_status_complete());
}

#[test]
fn operator_debug_flood_cannot_consume_important_admission_or_queue_headroom() {
    use remora::{config::LogFormat, diagnostics::EventName};
    let now = Instant::now();
    let (mut d, r) = Diagnostics::new(Role::Client, now, Duration::from_secs(1)).unwrap();
    d.set_output(LogFormat::Text, false);
    d.set_level(Level::Debug);
    for tick in 0..2000 {
        let _ = d.event(
            now + Duration::from_millis(tick * 50),
            Event::failure(Level::Debug, Failure::Transport, 0),
        );
    }
    assert!(
        d.event(
            now + Duration::from_secs(100),
            Event::lifecycle(EventName::ConnectionLost, Level::Warn)
        )
        .unwrap()
    );
    let mut err = Vec::new();
    while r.write_one(&mut io::sink(), &mut err).unwrap() {}
    let text = String::from_utf8(err).unwrap();
    assert!(text.contains("Connection to the server lost"));
    assert!(text.lines().all(|line| line.len() < 1024));
    assert!(d.queue().dropped() > 0);
    assert_eq!(d.queue().retained_bytes(), 0);
}
