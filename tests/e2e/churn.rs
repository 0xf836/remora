#[test]
fn repeated_setup_expiry_and_cleanup_release_fds_without_reassigning_queued_data() {
    let summary = super::concurrency::run_case("churn", 1, 64);
    assert_eq!(summary["cycles"], 64);
    assert!(summary["settled_seconds"].as_f64().unwrap() <= 60.0);
    for growth in summary["rss_growth_bytes"].as_array().unwrap() {
        assert!(growth.as_i64().unwrap() <= 10 * 1024 * 1024);
    }
}
