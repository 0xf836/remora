use crate::common::{ChildGuard, Fixtures};
use std::{fs, process::Command, time::Duration};
static CASE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
pub fn run_case(scenario: &str, seconds: usize, cycles: usize) -> serde_json::Value {
    // Preserve mutual exclusion after a failed case, without turning its
    // panic into unrelated failures that obscure subsequent evidence.
    let _guard = CASE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
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

#[test]
fn unexpected_packet_failure_survives_fixture_cleanup_without_secrets() {
    let f = Fixtures::new();
    let secret = fs::read_to_string(f.file("client.token")).unwrap();
    let output = f.file("concurrency-result");
    let retained = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/test-failures")
        .join(f.path().parent().unwrap().file_name().unwrap());
    let old_root = f.path().parent().unwrap().to_path_buf();
    let failed = std::panic::catch_unwind(|| {
        let mut child = ChildGuard::spawn_logged(
            Command::new("python3")
                .arg("-c")
                .arg(r#"
import json, pathlib, sys, time
sys.path.insert(0, 'scripts/validate')
from concurrency import Lab, udp
out = pathlib.Path(sys.argv[1]); out.mkdir(mode=0o700)
lab = Lab.__new__(Lab)
lab.logs = out; lab.scenario = 'injected-test'; lab.packet_failure = None
lab.targets = [udp()]; lab.senders = []; lab.corrupt = 0
sender = udp()
try:
    sender.sendto(b'private-payload-sentinel', lab.targets[0].getsockname())
    try:
        lab.exchange(time.monotonic() + 1)
    except AssertionError as error:
        assert str(error) == 'unexpected application payload'
        (out / 'summary.json').write_text(json.dumps(dict(status='failed', integrity_errors=lab.corrupt)))
        (out / 'manifest.json').write_text(json.dumps(dict(schema_version=1, scenario='injected-test')))
        (out / 'configs.json').write_text('never retain this config')
        (out / 'server.stderr.ndjson').write_text('never retain raw logs')
        raise SystemExit(1)
    raise AssertionError('bad datagram was accepted')
finally:
    sender.close(); lab.targets[0].close()
"#)
                .arg(&output),
            &f.file("injection.out"),
            &f.file("injection.err"),
        ).unwrap();
        assert_eq!(
            child.finish(Duration::from_secs(5)).unwrap().code(),
            Some(1)
        );
        assert!(
            output.join("failure.json").is_file(),
            "{}",
            fs::read_to_string(f.file("injection.err")).unwrap()
        );
        // Exercise the real unwinding/Drop path, not only an explicit copy.
        let _owned_fixture = f;
        panic!("injected concurrency assertion");
    });
    assert!(failed.is_err());
    assert!(!old_root.exists(), "private fixtures must still be removed");
    let names: std::collections::BTreeSet<_> = fs::read_dir(&retained)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(
        names,
        ["failure.json", "manifest.json", "summary.json"]
            .map(str::to_owned)
            .into()
    );
    let failure: serde_json::Value =
        serde_json::from_slice(&fs::read(retained.join("failure.json")).unwrap()).unwrap();
    assert_eq!(failure["reason"], "unexpected application payload");
    assert_eq!(failure["receiver_kind"], "target");
    assert_eq!(failure["datagram_bytes"], 24);
    assert!(failure["local_address"][1].as_u64().unwrap() > 0);
    assert!(failure["peer_address"][1].as_u64().unwrap() > 0);
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        fs::metadata(&retained).unwrap().permissions().mode() & 0o777,
        0o700
    );
    for name in names {
        let path = retained.join(name);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let text = fs::read_to_string(path).unwrap();
        assert!(!text.contains(secret.trim()));
        assert!(!text.contains("private-payload-sentinel"));
    }
    fs::remove_dir_all(retained).unwrap();
}

#[test]
fn a_panicked_case_does_not_poison_later_real_scenarios() {
    let failed = std::panic::catch_unwind(|| {
        let _guard = CASE_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        panic!("injected earlier case failure");
    });
    assert!(failed.is_err());
    assert_eq!(run_case("churn", 1, 1)["status"], "passed");
}
