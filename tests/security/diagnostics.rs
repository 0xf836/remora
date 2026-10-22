use remora::{
    config::Level,
    diagnostics::{Diagnostics, Event, Failure, MAX_RECORD_BYTES, Role, Snapshot},
};
use std::{
    io,
    time::{Duration, Instant},
};
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
