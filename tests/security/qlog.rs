use crate::common::Fixtures;
use remora::diagnostics::{
    Diagnostics, Role, Snapshot,
    qlog::{CONNECTION_FILE_BYTES, DIRECTORY_BYTES, MAX_CONNECTIONS, Pool, SCRATCH_BYTES},
};
use std::{
    fs, io,
    os::unix::fs::DirBuilderExt,
    time::{Duration, Instant},
};
const HEADER: &[u8] = b"\x1e{\"file_schema\":\"urn:ietf:params:qlog:file:sequential\",\"serialization_format\":\"JSON-SEQ\",\"title\":\"SECRET_TITLE\",\"trace\":{\"custom\":\"SECRET_CID\"}}\n";
const EVENT: &[u8] = b"\x1e{\"time\":1.25,\"name\":\"quic:packet_sent\",\"data\":{\"header\":{\"packet_number\":7,\"packet_type\":\"handshake\",\"dcid\":\"SECRET_CID\"},\"raw\":{\"length\":1234,\"payload_length\":1200,\"data\":\"SECRET_PAYLOAD\"},\"frames\":[{\"frame_type\":\"stream\",\"data\":\"SECRET_PAYLOAD\",\"headers\":{\"authorization\":\"SECRET_TOKEN\"}}],\"reason\":\"SECRET_REASON\"},\"custom\":\"SECRET_CUSTOM\"}\n";
fn directory(f: &Fixtures) -> std::path::PathBuf {
    let p = f.file("qlog");
    fs::DirBuilder::new().mode(0o700).create(&p).unwrap();
    p
}
#[test]
fn every_partial_boundary_reconstructs_only_permitted_fields_before_disk() {
    let f = Fixtures::new();
    let path = directory(&f);
    let now = Instant::now();
    let (mut d, receiver) = Diagnostics::new(Role::Client, now, Duration::from_secs(1)).unwrap();
    let pool = Pool::new(&path, d.queue().clone(), now).unwrap();
    // One byte writes cover every incremental boundary, including framing.
    let mut filter = pool.activate().unwrap();
    for b in HEADER.iter().chain(EVENT) {
        filter.ingest(&[*b], now);
    }
    filter.ingest(b"\x1e{\"time\":2,\"name\":\"http3:frame_created\",\"data\":{\"headers\":[\"SECRET_TOKEN\"]}}\n", now);
    filter.ingest(
        b"\x1e{\"time\":2,\"name\":\"custom:new_event\",\"data\":{\"secret\":\"SECRET_CUSTOM\"}}\n",
        now,
    );
    assert!(!filter.disabled());
    assert_eq!(pool.report().accepted_events, 1);
    assert_eq!(pool.report().discarded_events, 2);
    assert_eq!(
        fs::read_dir(&path).unwrap().count(),
        0,
        "no synchronous persistence or temporary raw file"
    );
    d.snapshot(now, Snapshot::default()).unwrap();
    let mut out = Vec::new();
    let mut err = Vec::new();
    assert!(receiver.write_one(&mut out, &mut err).unwrap()); // status before qlog
    assert!(!out.is_empty());
    assert!(receiver.write_one(&mut out, &mut err).unwrap());
    assert!(err.is_empty());
    let files: Vec<_> = fs::read_dir(&path).unwrap().collect();
    assert_eq!(files.len(), 1);
    let text = fs::read_to_string(files[0].as_ref().unwrap().path()).unwrap();
    assert!(!text.contains("SECRET"));
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["packet_length"], 1234);
    assert_eq!(v["data"]["header"]["packet_number"], 7);
    for key in ["raw", "frames", "reason", "custom", "headers"] {
        assert!(v["data"].get(key).is_none());
    }
    drop(filter);
    assert_eq!(pool.report().scratch_bytes, 0);
}
#[test]
fn malformed_oversize_and_schema_changes_disable_without_raw_fallback() {
    let cases = [
        b"\x1e{invalid}\n".to_vec(),
        b"\x1e{\"file_schema\":\"new-schema\",\"serialization_format\":\"JSON-SEQ\"}\n".to_vec(),
        b"\x1e{\"file_schema\":\"urn:ietf:params:qlog:file:sequential\",\"serialization_format\":\"new-format\"}\n".to_vec(),
        [HEADER, b"\x1e{\"time\":1,\"name\":\"quic:packet_sent\",\"data\":{\"header\":{\"packet_number\":\"SECRET\"}}}\n"].concat(),
        [HEADER, b"\x1e{\"time\":-1,\"name\":\"quic:packet_sent\",\"data\":{}}\n"].concat(),
        [HEADER, b"\x1e", &vec![b'x'; SCRATCH_BYTES + 1]].concat(),
        [HEADER, b"\x1e\x1e"].concat(),
    ];
    let f = Fixtures::new();
    let path = directory(&f);
    let now = Instant::now();
    let (d, receiver) = Diagnostics::new(Role::Client, now, Duration::from_secs(1)).unwrap();
    let pool = Pool::new(&path, d.queue().clone(), now).unwrap();
    for input in cases {
        let mut filter = pool.activate().unwrap();
        filter.ingest(&input, now);
        assert!(filter.disabled());
        filter.ingest(EVENT, now);
        drop(filter);
    }
    assert_eq!(pool.report().accepted_events, 0);
    assert_eq!(pool.report().scratch_bytes, 0);
    assert!(
        !receiver
            .write_one(&mut io::sink(), &mut io::sink())
            .unwrap()
    );
    assert_eq!(fs::read_dir(&path).unwrap().count(), 0);
}
#[test]
fn eight_connection_scratch_pool_and_churn_remain_bounded() {
    let f = Fixtures::new();
    let path = directory(&f);
    let now = Instant::now();
    let (d, _) = Diagnostics::new(Role::Client, now, Duration::from_secs(1)).unwrap();
    let pool = Pool::new(&path, d.queue().clone(), now).unwrap();
    let mut filters: Vec<_> = (0..MAX_CONNECTIONS)
        .map(|_| pool.activate().unwrap())
        .collect();
    assert!(pool.activate().is_none());
    assert_eq!(pool.report().scratch_bytes, 512 * 1024);
    filters.pop();
    assert!(pool.activate().is_some());
    drop(filters);
    for _ in 0..1000 {
        drop(pool.activate().unwrap());
    }
    assert_eq!(pool.report().active_filters, 0);
    assert_eq!(pool.report().scratch_bytes, 0);
    assert_eq!(fs::read_dir(&path).unwrap().count(), 0);
}
#[test]
fn qlog_saturation_cannot_use_status_reservation_or_wait_for_sink() {
    let f = Fixtures::new();
    let path = directory(&f);
    let now = Instant::now();
    let (mut d, receiver) = Diagnostics::new(Role::Client, now, Duration::from_secs(1)).unwrap();
    let pool = Pool::new(&path, d.queue().clone(), now).unwrap();
    let mut filter = pool.activate().unwrap();
    filter.ingest(HEADER, now);
    for _ in 0..768 {
        filter.ingest(EVENT, now);
    }
    assert_eq!(d.queue().retained_records(), 768);
    filter.ingest(EVENT, now);
    assert!(filter.disabled());
    assert!(d.snapshot(now, Snapshot::default()).unwrap());
    let status = receiver.try_next().unwrap().unwrap();
    assert!(status.is_status());
    assert!(d.queue().retained_bytes() <= 1024 * 1024);
    assert_eq!(fs::read_dir(&path).unwrap().count(), 0);
}
#[test]
fn process_event_rate_is_shared_and_does_not_reset_on_connection_churn() {
    let f = Fixtures::new();
    let path = directory(&f);
    let now = Instant::now();
    let (d, receiver) = Diagnostics::new(Role::Client, now, Duration::from_secs(1)).unwrap();
    let pool = Pool::new(&path, d.queue().clone(), now).unwrap();
    for n in 0..1001 {
        let mut filter = pool.activate().unwrap();
        filter.ingest(HEADER, now);
        filter.ingest(EVENT, now);
        if n < 1000 {
            assert!(!filter.disabled());
            drop(receiver.try_next().unwrap().unwrap());
        } else {
            assert!(filter.disabled());
        }
    }
    assert_eq!(pool.report().accepted_events, 1000);
    let mut filter = pool.activate().unwrap();
    filter.ingest(HEADER, now + Duration::from_secs(1));
    filter.ingest(EVENT, now + Duration::from_secs(1));
    assert!(!filter.disabled());
}
#[test]
fn lifetime_file_and_directory_caps_hold_across_churn_without_deleting_output() {
    let f = Fixtures::new();
    let path = directory(&f);
    let now = Instant::now();
    let (d, receiver) = Diagnostics::new(Role::Client, now, Duration::from_secs(1)).unwrap();
    let pool = Pool::new(&path, d.queue().clone(), now).unwrap();
    let mut events = 0u64;
    for _ in 0..9 {
        let Some(mut filter) = pool.activate() else {
            break;
        };
        filter.ingest(HEADER, now);
        let before = pool.report().reserved_file_bytes;
        loop {
            let at = now + Duration::from_secs(events / 900);
            events += 1;
            filter.ingest(EVENT, at);
            if filter.disabled() {
                break;
            }
            // Consume sanitized records without disk: reservation still bounds
            // potential retained files, even with deliberately missing output.
            drop(receiver.try_next().unwrap().unwrap());
        }
        assert!(pool.report().reserved_file_bytes - before <= CONNECTION_FILE_BYTES);
    }
    assert!(pool.report().reserved_file_bytes <= DIRECTORY_BYTES);
    assert!(pool.report().reserved_file_bytes > DIRECTORY_BYTES - 2048);
}
#[test]
fn existing_symlink_or_public_directory_is_refused_without_modification() {
    let f = Fixtures::new();
    let path = directory(&f);
    let now = Instant::now();
    let (d, _) = Diagnostics::new(Role::Client, now, Duration::from_secs(1)).unwrap();
    std::os::unix::fs::symlink(&path, f.file("link")).unwrap();
    assert!(Pool::new(&f.file("link"), d.queue().clone(), now).is_err());
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(Pool::new(&path, d.queue().clone(), now).is_err());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(path.join("existing"), b"keep").unwrap();
    assert!(Pool::new(&path, d.queue().clone(), now).is_err());
    assert_eq!(fs::read(path.join("existing")).unwrap(), b"keep");
}

#[test]
fn disk_failure_disables_only_optional_trace_and_status_ack_keeps_working() {
    let f = Fixtures::new();
    let path = directory(&f);
    let now = Instant::now();
    let (mut d, r) = Diagnostics::new(Role::Server, now, Duration::from_secs(1)).unwrap();
    let pool = Pool::new(&path, d.queue().clone(), now).unwrap();
    let mut filter = pool.activate().unwrap();
    filter.ingest(HEADER, now);
    filter.ingest(EVENT, now);
    // Compete for the writer-owned fixed filename after safe startup. EXCL
    // prevents overwrite; no raw data or fallback is persisted.
    fs::write(path.join("trace-1.ndjson"), b"preserve").unwrap();
    assert!(r.write_one(&mut io::sink(), &mut io::sink()).unwrap());
    assert_eq!(pool.report().output_failures, 1);
    assert!(filter.disabled());
    d.snapshot(now, Snapshot::default()).unwrap();
    assert!(r.write_one(&mut Vec::new(), &mut io::sink()).unwrap());
    assert!(d.final_status_complete());
    assert_eq!(fs::read(path.join("trace-1.ndjson")).unwrap(), b"preserve");
}
proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(24))]
    #[test]
    fn arbitrary_incremental_input_is_bounded_and_cannot_reach_disk(input in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..2048), chunk in 1usize..65) {
        let f = Fixtures::new(); let path = directory(&f); let now = Instant::now();
        let (d, r) = Diagnostics::new(Role::Client, now, Duration::from_secs(1)).unwrap();
        let pool = Pool::new(&path, d.queue().clone(), now).unwrap();
        let mut filter = pool.activate().unwrap();
        for bytes in input.chunks(chunk) { filter.ingest(bytes, now); }
        proptest::prop_assert!(d.queue().retained_bytes() <= 1024 * 1024);
        proptest::prop_assert!(pool.report().scratch_bytes <= SCRATCH_BYTES);
        proptest::prop_assert!(!r.write_one(&mut io::sink(), &mut io::sink()).unwrap());
        proptest::prop_assert_eq!(fs::read_dir(&path).unwrap().count(), 0);
    }
}
