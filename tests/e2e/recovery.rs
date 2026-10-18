use super::forwarding::Tunnel;
use crate::common::ChildGuard;
use std::{
    fs,
    net::SocketAddr,
    process::Command,
    time::{Duration, Instant},
};

pub(crate) fn listener_inode(pid: u32, listen: SocketAddr) -> String {
    let mut inodes = Vec::new();
    for entry in fs::read_dir(format!("/proc/{pid}/fd")).unwrap().flatten() {
        if let Ok(link) = fs::read_link(entry.path()) {
            let link = link.to_string_lossy();
            if let Some(value) = link
                .strip_prefix("socket:[")
                .and_then(|v| v.strip_suffix(']'))
            {
                inodes.push(value.to_owned());
            }
        }
    }
    let family = if listen.is_ipv4() { "udp" } else { "udp6" };
    for line in fs::read_to_string(format!("/proc/{pid}/net/{family}"))
        .unwrap()
        .lines()
        .skip(1)
    {
        let fields: Vec<_> = line.split_whitespace().collect();
        if u16::from_str_radix(fields[1].rsplit(':').next().unwrap(), 16) == Ok(listen.port())
            && inodes.iter().any(|i| i == fields[9])
        {
            return fields[9].to_owned();
        }
    }
    panic!("owned listener is not bound");
}

#[test]
fn initial_http_authentication_rejection_exits_one_and_releases_listener() {
    let mut t = Tunnel::start(false, |f| {
        fs::write(f.file("client.token"), "0".repeat(64)).unwrap();
    });
    let sender = t.sender();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(
            Instant::now() < deadline,
            "initial HTTP rejection did not exit"
        );
        sender.send_to(b"initial-rejection", t.listen).unwrap();
        let mut bytes = [0; 128];
        assert!(t.target.recv_from(&mut bytes).is_err());
        if fs::read_to_string(t.fixtures.file("client.err"))
            .unwrap()
            .contains("\"numeric_code\":401")
        {
            break;
        }
    }
    assert_eq!(
        t.client.finish(Duration::from_secs(5)).unwrap().code(),
        Some(1)
    );
    let rows = fs::read_to_string(t.fixtures.file("client.err")).unwrap();
    assert!(
        rows.lines()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .any(|r| {
                r["event"] == "failure"
                    && r["failure"] == "authentication"
                    && r["numeric_code"] == 401
            })
    );
    let _released = std::net::UdpSocket::bind(t.listen).unwrap();
    t.server.signal(rustix::process::Signal::TERM).unwrap();
    assert!(t.server.finish(Duration::from_secs(5)).unwrap().success());
}

#[test]
fn ten_second_restart_preserves_ipv4_ipv6_listener_and_never_replays_outage_payloads() {
    for ipv6 in [false, true] {
        let mut t = Tunnel::start(ipv6, |_| {});
        let sender = t.sender();
        t.ready(&sender, b"before-restart");
        let inode = listener_inode(t.client.id(), t.listen);
        let failure = Instant::now();
        t.server.signal(rustix::process::Signal::TERM).unwrap();
        assert!(t.server.finish(Duration::from_secs(5)).unwrap().success());
        let mut sequence = 0u64;
        let mut bytes = [0; 65536];
        while failure.elapsed() < Duration::from_secs(10) {
            let mut tag = vec![0x66; 128];
            tag[..8].copy_from_slice(&sequence.to_be_bytes());
            sender.send_to(&tag, t.listen).unwrap();
            sequence += 1;
            assert!(
                t.target.recv_from(&mut bytes).is_err(),
                "server-down payload reached endpoint"
            );
        }
        let states = fs::read_to_string(t.fixtures.file("client.out")).unwrap();
        assert!(
            states.contains("\"client_state\":\"recovering\"")
                || states.contains("\"connection_attempts\":2"),
            "no observed failure detection by ten seconds"
        );
        let restore = Instant::now();
        t.server = ChildGuard::spawn_logged(
            Command::new(env!("CARGO_BIN_EXE_remora"))
                .arg("server")
                .arg("--config")
                .arg(t.fixtures.file("server.toml")),
            &t.fixtures.file("server-after.out"),
            &t.fixtures.file("server-after.err"),
        )
        .unwrap();
        // Spawn is an earlier, conservative timing origin than actual server
        // socket readiness: a pass cannot hide server initialization time.
        let deadline = restore + Duration::from_secs(10);
        loop {
            assert!(
                Instant::now() < deadline,
                "restored availability recovery deadline"
            );
            sender.send_to(b"fresh-after-restart", t.listen).unwrap();
            if let Ok((n, peer)) = t.target.recv_from(&mut bytes) {
                assert_eq!(&bytes[..n], b"fresh-after-restart", "outage payload replay");
                t.target.send_to(&bytes[..n], peer).unwrap();
                let (n, _) = sender.recv_from(&mut bytes).unwrap();
                assert_eq!(&bytes[..n], b"fresh-after-restart");
                break;
            }
        }
        assert_eq!(listener_inode(t.client.id(), t.listen), inode);
        println!(
            "{{\"event\":\"recovery_process\",\"ipv6\":{ipv6},\"outage_seconds\":10,\"resume_ms\":{},\"outage_replay\":0,\"listener_unchanged\":true}}",
            restore.elapsed().as_millis()
        );
        t.stop();
    }
}

#[test]
fn idle_active_transport_uses_probes_without_refreshing_application_inactivity() {
    let mut t = Tunnel::start(false, |f| {
        let p = f.file("client.toml");
        fs::write(
            &p,
            fs::read_to_string(&p).unwrap() + "\n[limits]\nidle_session_ms=2500\n",
        )
        .unwrap();
        let p = f.file("server.toml");
        fs::write(
            &p,
            fs::read_to_string(&p).unwrap() + "\n[limits]\nidle_session_ms=2500\n",
        )
        .unwrap();
    });
    let sender = t.sender();
    t.ready(&sender, b"application-activity");
    std::thread::sleep(Duration::from_secs(4));
    let mut probes = 0;
    for role in ["client", "server"] {
        let records = fs::read_to_string(t.fixtures.file(&format!("{role}.out"))).unwrap();
        let last: serde_json::Value =
            serde_json::from_str(records.lines().last().unwrap()).unwrap();
        assert_eq!(last["sessions_active"], 0);
        let role_probes = last["recovery"]["keepalive_probes"].as_u64().unwrap();
        probes += role_probes;
        println!("idle role={role} probes={role_probes}");
        if role == "client" {
            assert_eq!(last["recovery"]["connection_attempts"], 1);
        }
    }
    // A peer's probe elicits an ACK that legitimately renews the other
    // endpoint's otherwise-idle send deadline. Both sides need not independently
    // probe inside this short window; at least one probe must occur, and neither
    // side may retain an application Session because of it.
    assert!(
        probes > 0,
        "no idle transport probe observed on either endpoint"
    );
    t.stop();
}
