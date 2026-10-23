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

#[test]
fn operator_profile_is_idle_silent_warns_once_on_size_and_preserves_forwarding() {
    let mut t = Tunnel::start(false, |f| {
        for role in ["client", "server"] {
            let file = format!("{role}.toml");
            fs::write(f.file(&file), f.without_diagnostics(&file)).unwrap();
        }
    });
    let sender = t.sender();
    t.ready(&sender, b"PRIVATE_PAYLOAD_SENTINEL");
    let deadline = Instant::now() + Duration::from_secs(3);
    while !fs::read_to_string(t.fixtures.file("client.err"))
        .unwrap()
        .contains("UDP tunnel accepted by the server")
    {
        assert!(Instant::now() < deadline, "accepted message not written");
        std::thread::sleep(Duration::from_millis(10));
    }
    let initial = fs::read_to_string(t.fixtures.file("client.err")).unwrap();
    assert!(initial.contains("Secure connection established"));
    assert!(initial.contains("UDP tunnel accepted by the server"));
    std::thread::sleep(Duration::from_millis(1100));
    assert_eq!(
        initial,
        fs::read_to_string(t.fixtures.file("client.err")).unwrap()
    );
    for _ in 0..100 {
        sender.send_to(&[17; 1401], t.listen).unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let text = fs::read_to_string(t.fixtures.file("client.err")).unwrap();
        if text.contains("UDP datagram dropped because it exceeds") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "missing useful size warning: {text}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    t.ready(&sender, b"STILL_PRIVATE_AFTER_FAILURE");
    t.stop();
    let token = fs::read_to_string(t.fixtures.file("client.token")).unwrap();
    for role in ["client", "server"] {
        assert_eq!(
            fs::read_to_string(t.fixtures.file(&format!("{role}.out"))).unwrap(),
            ""
        );
        let text = fs::read_to_string(t.fixtures.file(&format!("{role}.err"))).unwrap();
        assert!(text.contains("Remora stopped"));
        for forbidden in [
            "histograms",
            "PRIVATE",
            "epoch=",
            "failures=",
            "sequence=",
            "remora.test",
            token.trim(),
        ] {
            assert!(
                !text.contains(forbidden),
                "unexpected private/machine field: {forbidden}"
            );
        }
        assert!(
            text.lines().count() <= 6,
            "unexpected operator noise: {text}"
        );
        assert!(text.lines().all(|line| line.len() < 1024));
        if role == "client" {
            assert_eq!(text.matches("WARN").count(), 1);
        }
    }
}

#[test]
fn operator_auth_failure_explains_exit_without_spamming_the_server() {
    let mut t = Tunnel::start(false, |f| {
        for role in ["client", "server"] {
            let file = format!("{role}.toml");
            fs::write(f.file(&file), f.without_diagnostics(&file)).unwrap();
        }
        fs::write(f.file("client.token"), "0".repeat(64)).unwrap();
    });
    let sender = t.sender();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        sender.send_to(b"PRIVATE_REJECTED", t.listen).unwrap();
        let text = fs::read_to_string(t.fixtures.file("client.err")).unwrap();
        if text.contains("HTTP 401") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "missing authentication rejection: {text}"
        );
        std::thread::sleep(Duration::from_millis(30));
    }
    assert_eq!(
        t.client.finish(Duration::from_secs(5)).unwrap().code(),
        Some(1)
    );
    let error = fs::read_to_string(t.fixtures.file("client.err")).unwrap();
    assert!(
        error.contains("HTTP 401") && error.contains("client token") && error.contains("Exiting")
    );
    assert_eq!(error.matches("ERROR").count(), 1);
    t.server.signal(rustix::process::Signal::TERM).unwrap();
    assert!(t.server.finish(Duration::from_secs(5)).unwrap().success());
    let server = fs::read_to_string(t.fixtures.file("server.err")).unwrap();
    assert!(!server.contains("WARN") && !server.contains("ERROR"));
}

#[test]
fn operator_retries_are_silent_and_recovery_requires_tunnel_acceptance() {
    use crate::common::ChildGuard;
    use std::process::Command;
    let mut t = Tunnel::start(false, |f| {
        for role in ["client", "server"] {
            let file = format!("{role}.toml");
            fs::write(f.file(&file), f.without_diagnostics(&file)
                + "\n[transport]\nidle_timeout_ms=1000\n[recovery]\nkeepalive_interval_ms=200\npeer_progress_timeout_ms=1500\nsetup_timeout_ms=500\nretry_initial_ms=100\nretry_max_ms=200\n").unwrap();
        }
    });
    let sender = t.sender();
    t.ready(&sender, b"before-outage");
    t.server.signal(rustix::process::Signal::KILL).unwrap();
    assert!(!t.server.finish(Duration::from_secs(5)).unwrap().success());
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let text = fs::read_to_string(t.fixtures.file("client.err")).unwrap();
        if text.contains("Connection to the server lost") {
            break;
        }
        assert!(Instant::now() < deadline, "missing outage notice: {text}");
        std::thread::sleep(Duration::from_millis(20));
    }
    // Several failed setup attempts, without reminding the operator about each.
    std::thread::sleep(Duration::from_millis(1600));
    let down = fs::read_to_string(t.fixtures.file("client.err")).unwrap();
    assert_eq!(down.matches("Connection to the server lost").count(), 1);
    assert!(!down.contains("re-established") && !down.contains("Still reconnecting"));
    t.server = ChildGuard::spawn_logged(
        Command::new(env!("CARGO_BIN_EXE_remora"))
            .args(["server", "--config"])
            .arg(t.fixtures.file("server.toml")),
        &t.fixtures.file("server-after.out"),
        &t.fixtures.file("server-after.err"),
    )
    .unwrap();
    // A successful handshake alone must not announce restored UDP forwarding.
    std::thread::sleep(Duration::from_millis(700));
    assert!(
        !fs::read_to_string(t.fixtures.file("client.err"))
            .unwrap()
            .contains("re-established")
    );
    t.ready(&sender, b"fresh-after-outage");
    t.stop();
    let text = fs::read_to_string(t.fixtures.file("client.err")).unwrap();
    assert_eq!(text.matches("UDP tunnel re-established").count(), 1);
    assert_eq!(text.matches("Connection to the server lost").count(), 1);
    assert!(!text.contains("Still reconnecting") && !text.contains("attempts"));
}

#[test]
fn blocked_operator_stderr_does_not_hang_shutdown_or_packet_forwarding() {
    use crate::common::ChildGuard;
    use std::{
        os::{fd::OwnedFd, unix::net::UnixStream},
        process::{Command, Stdio},
    };
    let mut t = Tunnel::start(false, |f| {
        for role in ["client", "server"] {
            let file = format!("{role}.toml");
            fs::write(
                f.file(&file),
                f.without_diagnostics(&file) + "\n[diagnostics]\nlevel='debug'\n",
            )
            .unwrap();
        }
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while !fs::read_to_string(t.fixtures.file("client.err"))
        .unwrap()
        .contains("Remora")
    {
        assert!(
            Instant::now() < deadline,
            "client did not register signals and start"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    t.client.signal(rustix::process::Signal::TERM).unwrap();
    assert!(t.client.finish(Duration::from_secs(5)).unwrap().success());
    let (_unread, writer) = UnixStream::pair().unwrap();
    socket2::SockRef::from(&writer)
        .set_send_buffer_size(1024)
        .unwrap();
    t.client = ChildGuard::spawn_with_io(
        Command::new(env!("CARGO_BIN_EXE_remora"))
            .args(["client", "--config"])
            .arg(t.fixtures.file("client.toml")),
        Stdio::null(),
        Stdio::from(OwnedFd::from(writer)),
    )
    .unwrap();
    let sender = t.sender();
    t.ready(&sender, b"before-blocked-output");
    for _ in 0..400 {
        sender.send_to(&[17; 1401], t.listen).unwrap();
    }
    std::thread::sleep(Duration::from_millis(150));
    t.ready(&sender, b"forwarding-with-unread-output");
    let started = Instant::now();
    t.client.signal(rustix::process::Signal::TERM).unwrap();
    assert_eq!(
        t.client.finish(Duration::from_secs(5)).unwrap().code(),
        Some(1)
    );
    assert!(started.elapsed() < Duration::from_secs(3));
    t.server.signal(rustix::process::Signal::TERM).unwrap();
    assert!(t.server.finish(Duration::from_secs(5)).unwrap().success());
}

#[test]
fn blocked_json_capture_preserves_the_executable_runtime_failure_marker() {
    use crate::common::ChildGuard;
    use std::{
        os::{fd::OwnedFd, unix::net::UnixStream},
        process::{Command, Stdio},
    };
    let mut t = Tunnel::start(false, |_| {});
    t.ready(&t.sender(), b"before-replacing-collector");
    t.client.signal(rustix::process::Signal::TERM).unwrap();
    assert!(t.client.finish(Duration::from_secs(5)).unwrap().success());
    let (_unread, writer) = UnixStream::pair().unwrap();
    socket2::SockRef::from(&writer)
        .set_send_buffer_size(1024)
        .unwrap();
    let error_file = t.fixtures.file("blocked-json.err");
    t.client = ChildGuard::spawn_with_io(
        Command::new(env!("CARGO_BIN_EXE_remora"))
            .args(["client", "--config"])
            .arg(t.fixtures.file("client.toml")),
        Stdio::from(OwnedFd::from(writer)),
        fs::File::create(&error_file).unwrap().into(),
    )
    .unwrap();
    t.ready(&t.sender(), b"forwarding-with-blocked-json-collector");
    std::thread::sleep(Duration::from_millis(1100));
    t.client.signal(rustix::process::Signal::TERM).unwrap();
    assert_eq!(
        t.client.finish(Duration::from_secs(5)).unwrap().code(),
        Some(1)
    );
    let text = fs::read_to_string(error_file).unwrap();
    assert_eq!(
        text.lines()
            .filter(|line| line.starts_with("runtime failed: "))
            .collect::<Vec<_>>(),
        ["runtime failed: DiagnosticCollectionIncomplete"]
    );
    assert!(!text.contains("ERROR") && !text.contains("could not be completely"));
    t.server.signal(rustix::process::Signal::TERM).unwrap();
    assert!(t.server.finish(Duration::from_secs(5)).unwrap().success());
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
                let mut text = f.without_diagnostics(&format!("{role}.toml"));
                text.push_str("\n[diagnostics]\nformat = \"json\"\nsnapshots = true\nlevel = \"error\"\nsnapshot_interval_ms = 10000\n");
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
                sender.send_to(&[17; 1401], t.listen).unwrap();
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
                let text = f.without_diagnostics(&format!("{role}.toml"));
                fs::write(p, format!("{text}\n[diagnostics]\nformat = \"json\"\nsnapshots = true\nlevel = \"{level}\"\nqlog_directory = \"qlog-{role}\"\n")).unwrap();
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
    t.target.send_to(&[17; 1401], relay).unwrap();
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
