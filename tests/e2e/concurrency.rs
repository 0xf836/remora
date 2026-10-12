use crate::common::{ChildGuard, Fixtures};
use std::{fs, process::Command, time::Duration};
static CASE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
pub fn run_case(scenario: &str, seconds: usize, cycles: usize) -> serde_json::Value {
    let _guard = CASE_LOCK.lock().unwrap();
    let f = Fixtures::new();
    let output = f.file("concurrency-result");
    let mut child = ChildGuard::spawn_logged(
        Command::new("python3")
            .arg("scripts/validate/concurrency.py")
            .args([
                "--scenario",
                scenario,
                "--seconds",
                &seconds.to_string(),
                "--cycles",
                &cycles.to_string(),
                "--binary",
                env!("CARGO_BIN_EXE_remora"),
                "--output",
            ])
            .arg(&output),
        &f.file("suite.out"),
        &f.file("suite.err"),
    )
    .unwrap();
    let status = child.finish(Duration::from_secs(45)).unwrap();
    let summary: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(output.join("summary.json")).unwrap()).unwrap();
    assert!(
        status.success(),
        "{summary}; {}",
        fs::read_to_string(f.file("suite.err")).unwrap()
    );
    assert_eq!(summary["status"], "passed");
    assert_eq!(summary["cross_delivery"], 0);
    assert_eq!(summary["integrity_errors"], 0);
    assert_eq!(summary["generator_errors"], 0);
    summary
}
#[test]
fn thirty_two_live_associations_share_and_distinguish_targets_with_flow_recreation() {
    let summary = run_case("isolation", 8, 32);
    for ratio in summary["within_one_second_ratio"]
        .as_array()
        .unwrap()
        .iter()
        .skip(1)
    {
        assert!(ratio.as_f64().unwrap() >= 0.99);
    }
}
