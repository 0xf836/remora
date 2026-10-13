use super::forwarding::Tunnel;
use std::{
    fs,
    time::{Duration, Instant},
};
pub fn snapshots(t: &Tunnel, role: &str) -> Vec<serde_json::Value> {
    fs::read_to_string(t.fixtures.file(&format!("{role}.out")))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
pub fn assert_class(t: &Tunnel, role: &str, class: &str, code: u64) {
    let records = snapshots(t, role);
    assert!(records.iter().all(|s| s["coverage_lost"] == 0
        && s["status_write_failures"] == 0
        && s["counter_overflow"] == false));
    assert!(
        records.iter().any(
            |s| s["failures"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["class"] == class
                    && f["code"] == code
                    && f["session"].is_string()
                    && f["connection"].is_string())
        ),
        "missing {role} {class}/{code}: {records:?}"
    );
}
#[test]
fn collected_auth_policy_size_and_resource_faults_keep_opaque_correlation() {
    for case in 0..4 {
        let mut t = Tunnel::start(false, |f| {
            for role in ["client", "server"] {
                let p = f.file(&format!("{role}.toml"));
                let mut text = fs::read_to_string(&p).unwrap();
                text.push_str("\n[diagnostics]\nlevel = \"error\"\nsnapshot_interval_ms = 10000\n");
                if case == 3 && role == "server" {
                    text.push_str("\n[limits]\nmax_sessions_per_identity = 1\n");
                }
                fs::write(p, text).unwrap();
            }
            if case == 0 {
                fs::write(f.file("client.token"), "0".repeat(64)).unwrap();
            }
            if case == 1 {
                f.rewrite(
                    "credentials.toml",
                    "host = \"127.0.0.1\"",
                    "host = \"different.test\"",
                );
            }
        });
        let sender = t.sender();
        if case >= 2 {
            t.ready(&sender, b"PRIVATE_RESPONSE_SENTINEL");
            if case == 2 {
                sender.send_to(&[17; 1201], t.listen).unwrap();
                let mut buf = [0; 2048];
                assert!(t.target.recv_from(&mut buf).is_err());
            } else {
                let b = t.sender();
                for _ in 0..12 {
                    b.send_to(b"PRIVATE_PAYLOAD_SENTINEL", t.listen).unwrap();
                    std::thread::sleep(Duration::from_millis(30));
                }
                let mut buf = [0; 2048];
                assert!(t.target.recv_from(&mut buf).is_err());
            }
        } else {
            for _ in 0..8 {
                sender
                    .send_to(b"PRIVATE_PAYLOAD_SENTINEL", t.listen)
                    .unwrap();
                let mut buf = [0; 2048];
                assert!(t.target.recv_from(&mut buf).is_err());
            }
        }
        let _ = t.client.signal(rustix::process::Signal::TERM);
        t.server.signal(rustix::process::Signal::TERM).unwrap();
        let _ = t.client.finish(Duration::from_secs(5));
        assert!(t.server.finish(Duration::from_secs(5)).unwrap().success());
        match case {
            0 => assert_class(&t, "server", "authentication", 401),
            1 => assert_class(&t, "server", "authorization", 403),
            2 => assert_class(&t, "client", "size", 3),
            _ => assert_class(&t, "server", "resource", 503),
        }
        assert!(!t.fixtures.file("forbidden.keylog").exists());
        let token = fs::read_to_string(t.fixtures.file("client.token")).unwrap();
        for role in ["client", "server"] {
            for ext in ["out", "err"] {
                let text = fs::read_to_string(t.fixtures.file(&format!("{role}.{ext}"))).unwrap();
                assert!(!text.contains(token.trim()));
                assert!(!text.contains("PRIVATE_"));
            }
        }
    }
}
#[test]
fn accepted_silent_destination_and_observed_socket_error_have_distinct_status() {
    let mut t = Tunnel::start(false, |_| {});
    let sender = t.sender();
    t.ready(&sender, b"warmup");
    // A bound silent target produces no socket error and no responsiveness claim.
    sender.send_to(b"silent", t.listen).unwrap();
    let mut buf = [0; 64];
    t.target.recv_from(&mut buf).unwrap();
    std::thread::sleep(Duration::from_millis(1100));
    let records = snapshots(&t, "server");
    assert!(
        records
            .iter()
            .any(|r| r["lifecycle"]["accepted_total"].as_u64().unwrap() > 0)
    );
    assert!(records.iter().all(|r| {
        !r["failures"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["class"] == "socket")
    }));
    // Close the destination while preserving the tunnel; connected relay sees
    // an actual Linux ICMP ECONNREFUSED, reduced to fixed socket/111.
    let closed = std::mem::replace(
        &mut t.target,
        std::net::UdpSocket::bind("127.0.0.1:0").unwrap(),
    );
    drop(closed);
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        sender.send_to(b"closed-port", t.listen).unwrap();
        if snapshots(&t, "server").iter().any(|r| {
            r["failures"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["class"] == "socket" && f["code"] == 111)
        }) {
            break;
        }
        assert!(Instant::now() < deadline, "no observed Linux socket error");
        std::thread::sleep(Duration::from_millis(40));
    }
    t.stop();
    assert_class(&t, "server", "socket", 111);
}
#[cfg(feature = "qlog")]
#[test]
fn real_quiche_qlog_is_filtered_at_every_level_without_keylogs_or_raw_traces() {
    use std::os::unix::fs::DirBuilderExt;
    for level in ["error", "warn", "info", "debug", "trace"] {
        let mut t = Tunnel::start(false, |f| {
            for role in ["client", "server"] {
                fs::DirBuilder::new()
                    .mode(0o700)
                    .create(f.file(&format!("qlog-{role}")))
                    .unwrap();
                let p = f.file(&format!("{role}.toml"));
                let text = fs::read_to_string(&p).unwrap();
                fs::write(p, format!("{text}\n[diagnostics]\nlevel = \"{level}\"\nqlog_directory = \"qlog-{role}\"\n")).unwrap();
            }
        });
        t.ready(&t.sender(), b"QLOG_PAYLOAD_SENTINEL");
        std::thread::sleep(Duration::from_millis(100));
        t.stop();
        assert!(!t.fixtures.file("forbidden.keylog").exists());
        let token = fs::read_to_string(t.fixtures.file("client.token")).unwrap();
        for role in ["client", "server"] {
            let mut count = 0;
            for entry in fs::read_dir(t.fixtures.file(&format!("qlog-{role}"))).unwrap() {
                let text = fs::read_to_string(entry.unwrap().path()).unwrap();
                count += 1;
                assert!(!text.contains("QLOG_PAYLOAD_SENTINEL"));
                assert!(!text.contains(token.trim()));
                assert!(!text.contains("authorization"));
                assert!(!text.contains("http3"));
                assert!(!text.contains("dcid"));
                assert!(text.lines().any(
                    |l| serde_json::from_str::<serde_json::Value>(l).unwrap()["name"]
                        == "quic:packet_sent"
                ));
            }
            assert!(
                count > 0,
                "pinned upstream schema failed to produce sanitized records"
            );
            let records = snapshots(&t, role);
            assert!(
                records.iter().all(
                    |r| r["qlog"]["disabled_filters"] == 0 && r["qlog"]["output_failures"] == 0
                )
            );
        }
    }
}

#[test]
fn oversized_destination_reply_keeps_established_session_correlation() {
    let mut t = Tunnel::start(false, |_| {});
    let sender = t.sender();
    let relay = t.ready(&sender, b"warmup");
    t.target.send_to(&[17; 1201], relay).unwrap();
    let mut buf = [0; 2048];
    assert!(sender.recv_from(&mut buf).is_err());
    // Verify the unreliable drop neither closes the association nor replays it.
    t.ready(&sender, b"after-drop");
    t.stop();
    assert_class(&t, "server", "size", 3);
    let records = snapshots(&t, "server");
    assert_eq!(
        records.last().unwrap()["counters"]["local_drops"]["socket_truncated"]["to_client"],
        1
    );
}
