use remora::{
    config::Level,
    diagnostics::{Diagnostics, Event, Failure, Role, Snapshot},
};
use std::{
    io,
    time::{Duration, Instant},
};

#[test]
fn every_fixed_class_survives_suppression_cleanup_with_zero_coverage_loss() {
    let now = Instant::now();
    let (mut d, receiver) = Diagnostics::new(Role::Server, now, Duration::from_secs(1)).unwrap();
    for _ in 0..80 {
        let _ = d.event(now, Event::failure(Level::Warn, Failure::Transport, 7));
    }
    let classes = [
        Failure::InvalidConfiguration,
        Failure::Trust,
        Failure::Authentication,
        Failure::Authorization,
        Failure::Capability,
        Failure::Resolution,
        Failure::SetupTimeout,
        Failure::PeerTimeout,
        Failure::Transport,
        Failure::Socket,
        Failure::Size,
        Failure::Resource,
        Failure::Malformed,
        Failure::Internal,
        Failure::Stopped,
    ];
    let mut expected = Vec::new();
    for class in classes {
        let conn = d.next_id().unwrap();
        let session = d.next_id().unwrap();
        for _ in 0..3 {
            d.failure(now, Some(conn), Some(session), class, 17, false)
                .unwrap();
        }
        d.finish_session(session).unwrap();
        expected.push((
            serde_json::to_value(class).unwrap(),
            serde_json::to_value(session).unwrap(),
        ));
    }
    assert_eq!(d.pending_failures(), classes.len());
    assert!(d.snapshot(now, Snapshot::default()).unwrap());
    assert!(
        !d.snapshot(now + Duration::from_secs(1), Snapshot::default())
            .unwrap()
    );
    let mut out = Vec::new();
    receiver.write_one(&mut out, &mut io::sink()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["coverage_lost"], 0);
    assert_eq!(v["status_write_failures"], 0);
    assert!(v["diagnostics_dropped"].as_u64().unwrap() > 0);
    for (class, session) in expected {
        assert!(
            v["failures"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["class"] == class
                    && f["session"] == session
                    && f["repeats"] == 3
                    && f["terminal"] == true)
        );
    }
    assert!(d.final_status_complete());
}
struct Broken;
impl io::Write for Broken {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::ErrorKind::BrokenPipe.into())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
#[test]
fn failed_first_chunk_cannot_be_retired_by_successful_last_chunk_ack() {
    let now = Instant::now();
    let (mut d, receiver) = Diagnostics::new(Role::Server, now, Duration::from_secs(1)).unwrap();
    for _ in 0..256 {
        let session = d.next_id().unwrap();
        d.failure(now, None, Some(session), Failure::Socket, 5, true)
            .unwrap();
    }
    d.snapshot(now, Snapshot::default()).unwrap();
    assert!(receiver.write_one(&mut Broken, &mut io::sink()).is_err());
    receiver
        .write_one(&mut Vec::new(), &mut io::sink())
        .unwrap();
    assert!(!d.final_status_complete());
    assert_eq!(d.pending_failures(), 256);
    d.snapshot(now + Duration::from_secs(1), Snapshot::default())
        .unwrap();
    let mut out = Vec::new();
    receiver.write_one(&mut out, &mut io::sink()).unwrap();
    receiver.write_one(&mut out, &mut io::sink()).unwrap();
    let chunks: Vec<serde_json::Value> = out
        .split(|b| *b == b'\n')
        .filter(|s| !s.is_empty())
        .map(|s| serde_json::from_slice(s).unwrap())
        .collect();
    assert_eq!(
        chunks
            .iter()
            .map(|c| c["failures"].as_array().unwrap().len())
            .sum::<usize>(),
        256
    );
    assert!(
        chunks
            .iter()
            .all(|c| c["status_write_failures"] == 1 && c["coverage_lost"] == 0)
    );
    assert!(d.final_status_complete());
}
#[test]
fn overflow_exposes_exact_lost_sequence_interval_without_false_correlation() {
    let now = Instant::now();
    let (mut d, receiver) = Diagnostics::new(Role::Server, now, Duration::from_secs(1)).unwrap();
    for _ in 0..300 {
        let session = d.next_id().unwrap();
        d.failure(now, None, Some(session), Failure::Socket, 5, true)
            .unwrap();
    }
    assert_eq!(d.pending_failures(), 256);
    assert_eq!(d.coverage_lost(), 44);
    d.snapshot(now, Snapshot::default()).unwrap();
    let mut out = Vec::new();
    receiver.write_one(&mut out, &mut io::sink()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["lost_first"], 1);
    assert_eq!(v["lost_last"], 44);
    assert_eq!(v["coverage_lost"], 44);
}
#[test]
fn histogram_fixed_bounds_and_overflow_are_machine_visible() {
    let mut s = Snapshot::default();
    for n in [0, 1, 10, 11, 100, 101, 1001, 10001, 100001, 1000001] {
        s.histograms.pacing_lateness_us.observe(n);
    }
    assert_eq!(
        s.histograms.pacing_lateness_us.buckets,
        [1, 2, 2, 1, 1, 1, 1, 1]
    );
    s.histograms.pacing_lateness_us.buckets[0] = u64::MAX;
    s.histograms.pacing_lateness_us.observe(0);
    let now = Instant::now();
    let (mut d, r) = Diagnostics::new(Role::Server, now, Duration::from_secs(1)).unwrap();
    d.snapshot(now, s).unwrap();
    let record = r.try_next().unwrap().unwrap();
    let v: serde_json::Value = serde_json::from_slice(record.bytes()).unwrap();
    assert_eq!(v["counter_overflow"], true);
}

#[test]
fn coalesced_live_sequences_keep_loss_envelope_after_overflow_and_finish() {
    for finish in [false, true] {
        let now = Instant::now();
        let (mut d, receiver) =
            Diagnostics::new(Role::Server, now, Duration::from_secs(1)).unwrap();
        let mut sessions = Vec::new();
        for _ in 0..256 {
            let session = d.next_id().unwrap();
            d.failure(now, None, Some(session), Failure::Socket, 5, false)
                .unwrap();
            sessions.push(session);
        }
        d.failure(now, None, Some(sessions[0]), Failure::Socket, 6, false)
            .unwrap();
        if finish {
            d.finish_session(sessions[0]).unwrap();
        }
        for _ in 0..2 {
            let session = d.next_id().unwrap();
            d.failure(now, None, Some(session), Failure::Socket, 5, true)
                .unwrap();
        }
        d.snapshot(now, Snapshot::default()).unwrap();
        let record = receiver.try_next().unwrap().unwrap();
        let v: serde_json::Value = serde_json::from_slice(record.bytes()).unwrap();
        assert_eq!(v["coverage_lost"], 2);
        assert_eq!(v["lost_first"], 2);
        assert_eq!(v["lost_last"], if finish { 258 } else { 257 });
        assert!(
            !v["failures"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["session"] == serde_json::to_value(sessions[0]).unwrap())
        );
    }
}
