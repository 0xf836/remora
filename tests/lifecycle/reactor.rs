use remora::session::MappingLatch;
// Production flush/partial-send coverage lives in src/reactor.rs tests.
#[test]
fn rejection_latch_survives_flow_cleanup() {
    let mut latch = MappingLatch::default();
    assert!(latch.may_setup());
    latch.reject();
    assert!(!latch.may_setup());
    assert!(!latch.may_setup());
}
#[test]
fn continuation_finishes_a_bounded_cycle_before_sleeping() {
    for total in [0, 1, 8, 9, 32, 257] {
        let mut cursor = 0;
        let mut visited = Vec::new();
        loop {
            let (range, next, more) = remora::reactor::service_window(total, cursor);
            assert!(range.len() <= 8);
            visited.extend(range);
            cursor = next;
            if !more {
                break;
            }
        }
        assert_eq!(visited, (0..total).collect::<Vec<_>>());
        assert_eq!(cursor, 0);
    }
}
