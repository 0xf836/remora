//! External qualification runs separately from ordinary native/container CI.
use crate::common::{ChildGuard, Fixtures};
use std::{path::PathBuf, process::Command, time::Duration};

#[test]
#[ignore = "requires built Go 1.27.2 masque-go fixture; run with REMORA_INTEROP_BIN"]
fn independent_go_roles_preserve_bytes_and_deny_monitored_negatives() {
    let peer =
        PathBuf::from(std::env::var_os("REMORA_INTEROP_BIN").expect("pinned peer executable"));
    assert!(peer.is_file());
    let f = Fixtures::new();
    let output = std::env::var_os("REMORA_INTEROP_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| f.file("independent-results"));
    let mut runner = ChildGuard::spawn(
        Command::new("bash")
            .arg("scripts/validate/interop.sh")
            .arg("--fixtures")
            .arg(f.path())
            .arg("--peer-binary")
            .arg(peer)
            .arg("--direction")
            .arg("both")
            .arg("--output")
            .arg(&output),
    )
    .unwrap();
    assert!(runner.finish(Duration::from_secs(120)).unwrap().success());
    let records: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output.join("summary.json")).unwrap()).unwrap();
    let records = records.as_array().unwrap();
    assert!(records.len() >= 29);
    for record in records {
        assert_eq!(record["status"], "pass");
        if record["case"] != "positive" && record["case"] != "hostname-positive" {
            assert_eq!(record["target_datagrams"], 0);
        } else {
            for size in ["0", "1", "128", "512", "1200"] {
                assert!(record["target_length_counts"][size].as_u64().unwrap() >= 2);
            }
        }
    }
}
