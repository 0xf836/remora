use remora_traffic::{
    Collection, decode,
    measurement::{Profile, Regression, matching, percentile},
    packet,
};
#[test]
fn delayed_duplicate_and_reordered_deliveries_keep_original_sequence_intervals() {
    let mut c = Collection::with_limit(100).unwrap();
    for seq in [0, 21, 10, 21, 29] {
        c.observe_at(&packet(128, 7, 0, seq, 1000).unwrap(), 7, 0, 2500);
    }
    assert_eq!(c.sequence_seconds(10, 3).unwrap(), vec![1, 1, 2]);
    assert_eq!((c.unique, c.duplicates, c.latency_samples), (4, 1, 4));
    assert_eq!(c.one_way_delay_max_us, 1500);
    assert_eq!(c.one_way_delay_min_us, Some(1500));
    c.observe_at(&packet(128, 7, 0, 30, 5000).unwrap(), 7, 0, 4000);
    assert_eq!(c.clock_errors, 1);
    assert_eq!(c.latency_samples, 4);
    assert!(c.sequence_seconds(10, 1801).is_err());
    assert_eq!(c.sequence_intervals(&[1, 9, 20]).unwrap(), vec![1, 0, 3]);
}
#[test]
fn baseline_cli_recognizes_and_validates_outer_buffer_override() {
    for value in ["0", "2147483648"] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_remora-traffic"))
            .args([
                "baseline-client",
                "--listen",
                "127.0.0.1:0",
                "--to",
                "127.0.0.1:9",
                "--outer-socket-buffer-bytes",
                value,
            ])
            .output()
            .unwrap();
        assert!(!output.status.success());
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains("invalid input"), "{stderr}");
        assert!(!stderr.contains("unknown option"), "{stderr}");
    }
}
#[test]
fn integrity_unique_duplicate_reorder_and_direction() {
    let mut c = Collection::default();
    for seq in [1, 3, 2, 3] {
        let p = packet(128, 7, 0, seq, 9).unwrap();
        c.observe(&p, 7, 0);
    }
    assert_eq!(
        (c.unique, c.duplicates, c.reordered, c.bytes),
        (3, 1, 1, 384)
    );
    c.observe(&packet(128, 7, 1, 4, 9).unwrap(), 7, 0);
    assert_eq!(c.wrong_direction, 1);
    let mut b = packet(128, 7, 0, 5, 9).unwrap();
    b[45] ^= 1;
    assert!(decode(&b).is_err());
    c.observe(&b, 7, 0);
    assert_eq!(c.corrupt, 1);
    c.observe(&packet(128, 7, 0, 1_000_000, 9).unwrap(), 7, 0);
    assert_eq!(c.overflow, 1);
}
#[test]
fn profile_pair_and_regression_require_actual_matching_evidence() {
    let mut a = Profile {
        topology: "two-leg".into(),
        io: "mmsg".into(),
        size: 128,
        sessions: 1,
        interval_seconds: 3,
        drain_ms: 200,
        diagnostics: "info".into(),
    };
    let b = Profile {
        topology: "two-leg".into(),
        io: "mmsg".into(),
        size: 128,
        sessions: 1,
        interval_seconds: 3,
        drain_ms: 200,
        diagnostics: "info".into(),
    };
    assert!(matching(&a, &b));
    a.drain_ms = 0;
    assert!(!matching(&a, &b));
    let r = Regression {
        frozen: false,
        reference: "a".into(),
        workload: "one".into(),
        throughput_min: 0.9,
        loss_max_pp: 0.01,
        rtt_p95_max_ms: 1.0,
        cpu_max: 0.1,
        rss_max_bytes: 1024,
    };
    assert!(r.validate("a", "one").is_err());
    let r = Regression { frozen: true, ..r };
    assert!(r.validate("a", "one").is_ok());
    assert!(r.validate("different", "one").is_err());
    assert_eq!(percentile(&mut [1, 2, 3, 4, 5], 95), Some(5));
    assert_eq!(percentile(&mut [], 95), None);
}
