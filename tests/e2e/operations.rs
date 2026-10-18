use super::{diagnostics::assert_class, forwarding::Tunnel};
use crate::common::{ChildGuard, Fixtures};
use std::{
    fs,
    net::{SocketAddr, UdpSocket},
    os::unix::fs::PermissionsExt,
    process::Command,
    time::{Duration, Instant},
};
fn check(f: &Fixtures, role: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_remora"))
        .args(["check-config", "--config"])
        .arg(f.file(&format!("{role}.toml")))
        .output()
        .unwrap()
}
fn ports(f: &Fixtures) -> serde_json::Value {
    serde_json::from_str(&fs::read_to_string(f.file("ports.json")).unwrap()).unwrap()
}
#[test]
fn check_config_has_no_bind_or_peer_traffic_and_permission_errors_are_safe() {
    let f = Fixtures::new();
    let p = ports(&f);
    let _client = UdpSocket::bind(format!("127.0.0.1:{}", p["client"])).unwrap();
    let proxy = UdpSocket::bind(format!("127.0.0.1:{}", p["proxy"])).unwrap();
    let target = UdpSocket::bind(format!("127.0.0.1:{}", p["target"])).unwrap();
    for s in [&proxy, &target] {
        s.set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
    }
    for role in ["client", "server"] {
        assert!(check(&f, role).status.success());
    }
    let mut b = [0; 64];
    assert!(proxy.recv_from(&mut b).is_err());
    assert!(target.recv_from(&mut b).is_err());
    fs::set_permissions(f.file("client.token"), fs::Permissions::from_mode(0o644)).unwrap();
    let result = check(&f, "client");
    assert_eq!(result.status.code(), Some(2));
    let stderr = String::from_utf8(result.stderr).unwrap();
    assert!(stderr.contains("Permissions"));
    assert!(!stderr.contains(fs::read_to_string(f.file("client.token")).unwrap().trim()));
}
#[test]
fn occupied_port_is_runtime_failure_after_successful_candidate_validation() {
    let f = Fixtures::new();
    let p = ports(&f);
    let _occupied = UdpSocket::bind(format!("127.0.0.1:{}", p["client"])).unwrap();
    assert!(check(&f, "client").status.success());
    let result = Command::new(env!("CARGO_BIN_EXE_remora"))
        .args(["client", "--config"])
        .arg(f.file("client.toml"))
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8(result.stderr).unwrap().contains("Bind"));
}
fn restart(t: &mut Tunnel, delayed_server: bool) {
    for role in ["server", "client"] {
        let mut command = if role == "server" && delayed_server {
            // A cold/shared runner can schedule the replacement server late.
            // Exercise that ordering deterministically, before Remora starts.
            let mut command = Command::new("bash");
            command
                .arg("-c")
                .arg("sleep 2.2; exec \"$@\"")
                .arg("delayed-remora");
            command
        } else {
            Command::new(env!("CARGO_BIN_EXE_remora"))
        };
        if role == "server" && delayed_server {
            command.arg(env!("CARGO_BIN_EXE_remora"));
        }
        let child = ChildGuard::spawn_logged(
            command
                .arg(role)
                .arg("--config")
                .arg(t.fixtures.file(&format!("{role}.toml"))),
            &t.fixtures.file(&format!("{role}.out")),
            &t.fixtures.file(&format!("{role}.err")),
        )
        .unwrap();
        if role == "server" {
            t.server = child;
        } else {
            t.client = child;
        }
    }
}
fn atomically_replace(f: &Fixtures, name: &str, text: &str) {
    use std::io::Write;
    let candidate = f.file(&format!("{name}.candidate"));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&candidate)
        .unwrap();
    fs::set_permissions(&candidate, fs::Permissions::from_mode(0o600)).unwrap();
    file.write_all(text.as_bytes()).unwrap();
    file.sync_all().unwrap();
    drop(file);
    fs::rename(candidate, f.file(name)).unwrap();
}
#[test]
fn atomic_policy_generation_stop_start_revocation_and_deliberate_rollback() {
    for credential in [true, false] {
        let mut t = Tunnel::start(false, |_| {});
        let sender = t.sender();
        let old_peer = t.ready(&sender, b"old-generation");
        let old_policy = fs::read_to_string(t.fixtures.file("credentials.toml")).unwrap();
        let new_policy = if credential {
            let line = old_policy
                .lines()
                .find(|l| l.starts_with("token_sha256"))
                .unwrap();
            old_policy.replace(line, &format!("token_sha256 = \"{}\"", "0".repeat(64)))
        } else {
            old_policy.replace("host = \"127.0.0.1\"", "host = \"rollback-only.test\"")
        };
        atomically_replace(&t.fixtures, "credentials.toml", &new_policy);
        assert!(check(&t.fixtures, "server").status.success());
        // Disk replacement alone is not revocation: the immutable old policy
        // still accepts existing traffic before its documented exit boundary.
        assert_eq!(t.ready(&sender, b"before-old-exit"), old_peer);
        t.stop();
        for role in ["client", "server"] {
            fs::rename(
                t.fixtures.file(&format!("{role}.out")),
                t.fixtures.file(&format!("{role}.old.out")),
            )
            .unwrap();
        }
        let old_records: Vec<serde_json::Value> =
            fs::read_to_string(t.fixtures.file("server.old.out"))
                .unwrap()
                .lines()
                .map(|l| serde_json::from_str(l).unwrap())
                .collect();
        // Drain any already accepted in-flight target traffic before testing the
        // new policy. It belongs to the prior generation, not new authorization.
        let mut bytes = [0; 2048];
        while t.target.recv_from(&mut bytes).is_ok() {}
        restart(&mut t, true);
        let class = if credential {
            "authentication"
        } else {
            "authorization"
        };
        let code = if credential { 401 } else { 403 };
        // Process scheduling, QUIC setup/retry and status publication are
        // asynchronous. Silence alone cannot establish policy enforcement.
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut probes = 0;
        loop {
            sender.send_to(b"denied-new-generation", t.listen).unwrap();
            assert!(
                t.target.recv_from(&mut bytes).is_err(),
                "removed credential/permission reached destination after old exit"
            );
            probes += 1;
            let rejected = super::diagnostics::snapshots(&t, "server")
                .iter()
                .any(|record| {
                    record["failures"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|failure| failure["class"] == class && failure["code"] == code)
                });
            if probes >= 12 && rejected {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "replacement did not report {class}/{code}; client={} server={}",
                fs::read_to_string(t.fixtures.file("client.out")).unwrap(),
                fs::read_to_string(t.fixtures.file("server.out")).unwrap()
            );
        }
        let _ = t.client.signal(rustix::process::Signal::TERM);
        t.server.signal(rustix::process::Signal::TERM).unwrap();
        let _ = t.client.finish(Duration::from_secs(5));
        assert!(t.server.finish(Duration::from_secs(5)).unwrap().success());
        assert_class(&t, "server", class, code);
        let new_records = super::diagnostics::snapshots(&t, "server");
        assert_ne!(
            old_records[0]["process_epoch"],
            new_records[0]["process_epoch"]
        );
        assert_eq!(
            new_records.last().unwrap()["lifecycle"]["accepted_total"],
            0
        );
        // Only explicit restoration re-enables the removed grant. No automatic
        // fallback to an older, more permissive policy generation occurs.
        atomically_replace(&t.fixtures, "credentials.toml", &old_policy);
        assert!(check(&t.fixtures, "server").status.success());
        restart(&mut t, false);
        t.ready(&sender, b"explicit-rollback-generation");
        t.stop();
    }
}
#[test]
fn examples_parse_after_private_nonsecret_fixture_provisioning() {
    let f = Fixtures::new();
    let p = ports(&f);
    for role in ["client", "server"] {
        let template = fs::read_to_string(format!(
            "{}/examples/{role}.toml",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        let text = template
            .replace("127.0.0.1:7443", &format!("127.0.0.1:{}", p["proxy"]))
            .replace("remora.test:7443", &format!("remora.test:{}", p["proxy"]))
            .replace("127.0.0.1:51821", &format!("127.0.0.1:{}", p["client"]))
            .replace("127.0.0.1:51820", &format!("127.0.0.1:{}", p["target"]));
        atomically_replace(&f, &format!("{role}.toml"), &text);
        assert!(check(&f, role).status.success(), "example {role} invalid");
    }
    let _: SocketAddr = format!("127.0.0.1:{}", p["client"]).parse().unwrap();
}
