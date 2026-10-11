use super::forwarding::Tunnel;
use std::{fs, time::Duration};
#[test]
fn bad_credential_forbidden_target_and_wrong_tls_name_never_reach_target() {
    for case in 0..8 {
        let mut t = Tunnel::start(false, |f| match case {
            0 => fs::write(f.file("client.token"), "0".repeat(64)).unwrap(),
            1 => f.rewrite(
                "credentials.toml",
                r#"host = "127.0.0.1""#,
                r#"host = "different.test""#,
            ),
            2 => f.rewrite("client.toml", "remora.test", "wrong.test"),
            3 => {
                let other = crate::common::Fixtures::new();
                fs::copy(other.file("ca.pem"), f.file("ca.pem")).unwrap();
            }
            5 => {
                f.rewrite(
                    "credentials.toml",
                    "host = \"127.0.0.1\"",
                    "host = \"localhost\"",
                );
                f.rewrite("credentials.toml", "127.0.0.1/32", "192.0.2.1/32");
                f.rewrite(
                    "client.toml",
                    "target_host = \"127.0.0.1\"",
                    "target_host = \"localhost\"",
                );
            }
            6 => f.rewrite(
                "client.toml",
                "target_host = \"127.0.0.1\"",
                "target_host = \"localhost\"",
            ),
            7 => {
                // Revoke the old token by retaining the identity/rule with a
                // different authorized digest; the former token cannot connect.
                let credentials = fs::read_to_string(f.file("credentials.toml")).unwrap();
                let line = credentials
                    .lines()
                    .find(|l| l.starts_with("token_sha256"))
                    .unwrap();
                fs::write(
                    f.file("credentials.toml"),
                    credentials.replace(line, &format!("token_sha256 = \"{}\"", "0".repeat(64))),
                )
                .unwrap();
            }
            _ => {
                let status = std::process::Command::new("openssl")
                    .args(["x509", "-req", "-in"])
                    .arg(f.file("server.csr"))
                    .arg("-CA")
                    .arg(f.file("ca.pem"))
                    .arg("-CAkey")
                    .arg(f.file("ca-key.pem"))
                    .args(["-set_serial", "12345", "-days", "0", "-extfile"])
                    .arg(f.file("server.ext"))
                    .arg("-out")
                    .arg(f.file("server.pem"))
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .unwrap();
                assert!(status.success());
            }
        });
        let sender = t.sender();
        for _ in 0..8 {
            sender
                .send_to(b"PRIVATE_PAYLOAD_SENTINEL", t.listen)
                .unwrap();
            let mut bytes = [0; 65536];
            assert!(t.target.recv_from(&mut bytes).is_err());
        }
        // The client may exit on failed initial trust; the server stays available.
        let _ = t.client.signal(rustix::process::Signal::TERM);
        t.server.signal(rustix::process::Signal::TERM).unwrap();
        let _ = t.client.finish(Duration::from_secs(5));
        assert!(
            t.server.finish(Duration::from_secs(5)).unwrap().success(),
            "server exit case={case}: {}",
            fs::read_to_string(t.fixtures.file("server.err")).unwrap()
        );
        let token = fs::read_to_string(t.fixtures.file("client.token")).unwrap();
        for role in ["client", "server"] {
            for suffix in ["out", "err"] {
                let data =
                    fs::read_to_string(t.fixtures.file(&format!("{role}.{suffix}"))).unwrap();
                assert!(!data.contains(token.trim()));
                assert!(!data.contains("PRIVATE_PAYLOAD_SENTINEL"));
                assert!(!data.contains("wrong.test"));
            }
        }
    }
}

#[test]
fn healthy_collector_gets_terminal_failure_on_shutdown_before_long_snapshot_cadence() {
    let mut t = Tunnel::start(false, |f| {
        fs::write(f.file("client.token"), "0".repeat(64)).unwrap();
        for file in ["client.toml", "server.toml"] {
            let mut text = fs::read_to_string(f.file(file)).unwrap();
            text.push_str("\n[transport]\nidle_timeout_ms = 30000\n");
            if file == "server.toml" {
                text.push_str("\n[diagnostics]\nlevel = \"error\"\nsnapshot_interval_ms = 10000\n");
            }
            fs::write(f.file(file), text).unwrap();
        }
    });
    let output = t.fixtures.file("server.out");
    let deadline = std::time::Instant::now() + Duration::from_secs(13);
    while !fs::read_to_string(&output).unwrap().contains("snapshot") {
        assert!(
            std::time::Instant::now() < deadline,
            "initial long-interval snapshot missing"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let sender = t.sender();
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        assert!(
            std::time::Instant::now() < deadline,
            "client did not observe auth rejection"
        );
        sender.send_to(b"terminal-test", t.listen).unwrap();
        let text = fs::read_to_string(t.fixtures.file("client.out")).unwrap();
        if text
            .lines()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .any(|v| {
                v["failures"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|f| f["class"] == "authentication"))
            })
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = t.client.signal(rustix::process::Signal::TERM);
    t.server.signal(rustix::process::Signal::TERM).unwrap();
    // Initial permanent authentication rejection can give the client an error
    // exit. The server remains healthy and must flush its pending correlation.
    let _ = t.client.finish(Duration::from_secs(5));
    assert!(
        t.server.finish(Duration::from_secs(5)).unwrap().success(),
        "{}",
        fs::read_to_string(t.fixtures.file("server.err")).unwrap()
    );
    let snapshots: Vec<serde_json::Value> = fs::read_to_string(output)
        .unwrap()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    assert!(
        snapshots
            .iter()
            .any(|v| v["failures"].as_array().is_some_and(|a| a
                .iter()
                .any(|f| f["class"] == "authentication" && f["terminal"] == true))),
        "healthy collector lost terminal correlation at shutdown"
    );
}
