use super::{forwarding::Tunnel, recovery::listener_inode};
use crate::common::{ChildGuard, Fixtures};
use std::{
    fs,
    net::{SocketAddr, UdpSocket},
    process::Command,
    time::{Duration, Instant},
};

fn client_listen(f: &Fixtures) -> SocketAddr {
    let ports: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(f.file("ports.json")).unwrap()).unwrap();
    format!("127.0.0.1:{}", ports["client"]).parse().unwrap()
}
fn wait_bound(child: &ChildGuard, listen: SocketAddr) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let family = if listen.is_ipv4() { "udp" } else { "udp6" };
        let text = fs::read_to_string(format!("/proc/{}/net/{family}", child.id())).unwrap();
        if text.lines().skip(1).any(|l| {
            u16::from_str_radix(
                l.split_whitespace()
                    .nth(1)
                    .unwrap()
                    .rsplit(':')
                    .next()
                    .unwrap(),
                16,
            ) == Ok(listen.port())
        }) {
            return;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn stopped(
    child: &mut ChildGuard,
    signal: rustix::process::Signal,
    listen: SocketAddr,
    state: &str,
) {
    let started = Instant::now();
    child.signal(signal).unwrap();
    assert!(child.finish(Duration::from_secs(5)).unwrap().success());
    assert!(started.elapsed() <= Duration::from_secs(5));
    let _rebound = UdpSocket::bind(listen).expect("listener/FD not released");
    println!(
        "{}",
        serde_json::json!({"event":"shutdown_process","state":state,"signal":if signal==rustix::process::Signal::INT {"SIGINT"} else {"SIGTERM"},"stop_ms":started.elapsed().as_millis(),"listener_released":true})
    );
}

#[test]
fn both_signals_stop_starting_active_and_recovering_without_retry_or_fd_leaks() {
    for signal in [rustix::process::Signal::INT, rustix::process::Signal::TERM] {
        let f = Fixtures::new();
        let listen = client_listen(&f);
        let mut client = ChildGuard::spawn_logged(
            Command::new(env!("CARGO_BIN_EXE_remora"))
                .arg("client")
                .arg("--config")
                .arg(f.file("client.toml")),
            &f.file("client.out"),
            &f.file("client.err"),
        )
        .unwrap();
        wait_bound(&client, listen);
        stopped(&mut client, signal, listen, "starting");
        for recovering in [false, true] {
            let mut t = Tunnel::start(false, |_| {});
            let sender = t.sender();
            t.ready(&sender, b"shutdown-test");
            listener_inode(t.client.id(), t.listen);
            if recovering {
                t.server.signal(rustix::process::Signal::TERM).unwrap();
                assert!(t.server.finish(Duration::from_secs(5)).unwrap().success());
                let deadline = Instant::now() + Duration::from_secs(15);
                while !fs::read_to_string(t.fixtures.file("client.err"))
                    .unwrap()
                    .contains("\"event\":\"recovering\"")
                {
                    assert!(Instant::now() < deadline, "did not enter Recovering");
                    sender.send_to(b"outage", t.listen).unwrap();
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
            stopped(
                &mut t.client,
                signal,
                t.listen,
                if recovering { "recovering" } else { "active" },
            );
            if !recovering {
                t.server.signal(signal).unwrap();
                assert!(t.server.finish(Duration::from_secs(5)).unwrap().success());
            }
            let rows = fs::read_to_string(t.fixtures.file("client.out")).unwrap();
            let final_status: serde_json::Value =
                serde_json::from_str(rows.lines().last().unwrap()).unwrap();
            assert_eq!(final_status["recovery"]["client_state"], "stopped");
            assert_eq!(final_status["resources"]["sessions"], 0);
            assert_eq!(final_status["resources"]["fd_reservations"], 0);
        }
    }
}

#[test]
#[ignore = "requires built pinned independent Go fixture with hold-response case"]
fn both_signals_cancel_awaiting_acceptance_setup_with_independent_peer() {
    let peer = std::env::var_os("REMORA_INTEROP_BIN").expect("pinned independent peer path");
    for signal in [rustix::process::Signal::INT, rustix::process::Signal::TERM] {
        let f = Fixtures::new();
        let ports: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(f.file("ports.json")).unwrap()).unwrap();
        let credentials: toml::Value =
            toml::from_str(&fs::read_to_string(f.file("credentials.toml")).unwrap()).unwrap();
        let listen = client_listen(&f);
        let proxy: SocketAddr = format!("127.0.0.1:{}", ports["proxy"]).parse().unwrap();
        let config = serde_json::json!({"role":"server","listen":proxy.to_string(),"authority":format!("remora.test:{}",ports["proxy"]),"target":format!("127.0.0.1:{}",ports["target"]),"certificate_file":f.file("server.pem"),"key_file":f.file("server-key.pem"),"digest":credentials["identities"][0]["token_sha256"].as_str().unwrap(),"case":"hold-response"});
        fs::write(f.file("peer.json"), serde_json::to_vec(&config).unwrap()).unwrap();
        let mut server = ChildGuard::spawn_logged(
            Command::new(&peer).arg("--config").arg(f.file("peer.json")),
            &f.file("peer.out"),
            &f.file("peer.err"),
        )
        .unwrap();
        wait_bound(&server, proxy);
        let mut client = ChildGuard::spawn_logged(
            Command::new(env!("CARGO_BIN_EXE_remora"))
                .arg("client")
                .arg("--config")
                .arg(f.file("client.toml")),
            &f.file("client.out"),
            &f.file("client.err"),
        )
        .unwrap();
        wait_bound(&client, listen);
        let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
        let deadline = Instant::now() + Duration::from_secs(4);
        while !fs::read_to_string(f.file("peer.out"))
            .unwrap()
            .contains("request_pending")
        {
            assert!(
                Instant::now() < deadline,
                "peer did not hold authenticated setup"
            );
            sender.send_to(b"unavailable-setup", listen).unwrap();
            std::thread::sleep(Duration::from_millis(10));
        }
        stopped(&mut client, signal, listen, "setup");
        server.signal(signal).unwrap();
        assert!(server.finish(Duration::from_secs(5)).unwrap().success());
        let _port = UdpSocket::bind(proxy).unwrap();
    }
}
