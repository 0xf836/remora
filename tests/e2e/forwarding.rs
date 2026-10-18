use crate::common::{ChildGuard, Fixtures};
use std::{
    fs,
    net::{SocketAddr, UdpSocket},
    process::Command,
    time::{Duration, Instant},
};
pub struct Tunnel {
    pub fixtures: Fixtures,
    pub server: ChildGuard,
    pub client: ChildGuard,
    pub target: UdpSocket,
    pub listen: SocketAddr,
}
impl Tunnel {
    pub fn start(ipv6: bool, change: impl FnOnce(&Fixtures)) -> Self {
        let f = Fixtures::new();
        let ports: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(f.file("ports.json")).unwrap()).unwrap();
        let host = if ipv6 { "[::1]" } else { "127.0.0.1" };
        if ipv6 {
            for file in ["client.toml", "server.toml", "credentials.toml"] {
                f.rewrite(
                    file,
                    "127.0.0.1",
                    if file == "credentials.toml" {
                        "::1"
                    } else {
                        host
                    },
                );
            }
            f.rewrite("credentials.toml", "::1/32", "::1/128");
        }
        change(&f);
        let target = UdpSocket::bind(format!("{host}:{}", ports["target"])).unwrap();
        target
            .set_read_timeout(Some(Duration::from_millis(150)))
            .unwrap();
        let server = ChildGuard::spawn_logged(
            Command::new(env!("CARGO_BIN_EXE_remora"))
                .env("SSLKEYLOGFILE", f.file("forbidden.keylog"))
                .env("RUST_LOG", "quiche=trace,quiche::h3=trace,remora=trace")
                .arg("server")
                .arg("--config")
                .arg(f.file("server.toml")),
            &f.file("server.out"),
            &f.file("server.err"),
        )
        .unwrap();
        // check-config succeeds independently of whether the server bind has run.
        let status = Command::new(env!("CARGO_BIN_EXE_remora"))
            .arg("check-config")
            .arg("--config")
            .arg(f.file("client.toml"))
            .status()
            .unwrap();
        if !status.success() {
            server.signal(rustix::process::Signal::TERM).unwrap();
            panic!("fixture check-config");
        }
        let client = ChildGuard::spawn_logged(
            Command::new(env!("CARGO_BIN_EXE_remora"))
                .env("SSLKEYLOGFILE", f.file("forbidden.keylog"))
                .env("RUST_LOG", "quiche=trace,quiche::h3=trace,remora=trace")
                .arg("client")
                .arg("--config")
                .arg(f.file("client.toml")),
            &f.file("client.out"),
            &f.file("client.err"),
        )
        .unwrap();
        Self {
            fixtures: f,
            server,
            client,
            target,
            listen: format!("{host}:{}", ports["client"]).parse().unwrap(),
        }
    }
    pub fn sender(&self) -> UdpSocket {
        let s = UdpSocket::bind(if self.listen.is_ipv4() {
            "127.0.0.1:0"
        } else {
            "[::1]:0"
        })
        .unwrap();
        s.set_read_timeout(Some(Duration::from_millis(150)))
            .unwrap();
        s
    }
    pub fn ready(&self, sender: &UdpSocket, tag: &[u8]) -> SocketAddr {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut b = [0; 65536];
        loop {
            assert!(
                Instant::now() < deadline,
                "setup failed: {} {}",
                fs::read_to_string(self.fixtures.file("client.err")).unwrap(),
                format_args!(
                    "stderr={} server={} client={}",
                    fs::read_to_string(self.fixtures.file("server.err")).unwrap(),
                    fs::read_to_string(self.fixtures.file("server.out")).unwrap(),
                    fs::read_to_string(self.fixtures.file("client.out")).unwrap()
                )
            );
            sender.send_to(tag, self.listen).unwrap();
            if let Ok((n, peer)) = self.target.recv_from(&mut b) {
                assert_eq!(&b[..n], tag);
                self.target.send_to(&b[..n], peer).unwrap();
                if let Ok((n, _)) = sender.recv_from(&mut b) {
                    assert_eq!(&b[..n], tag);
                    return peer;
                }
            }
        }
    }
    pub fn stop(&mut self) {
        for p in [&self.client, &self.server] {
            p.signal(rustix::process::Signal::TERM).unwrap();
        }
        assert!(
            self.client
                .finish(Duration::from_secs(5))
                .unwrap()
                .success()
        );
        assert!(
            self.server
                .finish(Duration::from_secs(5))
                .unwrap()
                .success()
        );
    }
}
#[test]
fn ipv4_ipv6_empty_boundary_two_sender_isolation_and_shutdown() {
    for v6 in [false, true] {
        let mut t = Tunnel::start(v6, |_| {});
        let a = t.sender();
        let b = t.sender();
        let pa = t.ready(&a, b"sender-a");
        let pb = t.ready(&b, b"sender-b");
        assert_ne!(pa, pb, "distinct connected relays");
        for size in [0, 1, 128, 512, 1200] {
            for (sender, peer, tag) in [(&a, pa, 17u8), (&b, pb, 239u8)] {
                let payload = vec![tag; size];
                sender.send_to(&payload, t.listen).unwrap();
                let mut bytes = [0; 65536];
                let (n, p) = t.target.recv_from(&mut bytes).unwrap();
                assert_eq!(p, peer);
                assert_eq!(&bytes[..n], payload);
                let reply = vec![tag.wrapping_add(1); size];
                t.target.send_to(&reply, peer).unwrap();
                let (n, _) = sender.recv_from(&mut bytes).unwrap();
                assert_eq!(&bytes[..n], reply);
            }
        }
        a.send_to(&[0; 1201], t.listen).unwrap();
        let mut bytes = [0; 65536];
        assert!(t.target.recv_from(&mut bytes).is_err());
        // Snapshot includes setup loss, and never includes application bytes or tuples.
        std::thread::sleep(Duration::from_millis(1100));
        t.stop();
        let out = fs::read_to_string(t.fixtures.file("client.out")).unwrap();
        let snapshots: Vec<serde_json::Value> = out
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert!(snapshots.iter().any(|s| {
            s["counters"]["local_drops"]["not_active"]["to_target"]
                .as_u64()
                .unwrap()
                > 0
        }));
        assert!(!out.contains("sender-a"));
        assert!(!out.contains("127.0.0.1"));
        // All process-owned bound ports are released after graceful completion.
        let _released = UdpSocket::bind(t.listen).unwrap();
    }
}
#[test]
fn cli_and_role_errors_are_safe() {
    let f = Fixtures::new();
    let bin = env!("CARGO_BIN_EXE_remora");
    for arg in ["--help", "--version"] {
        assert!(Command::new(bin).arg(arg).status().unwrap().success());
    }
    assert_eq!(
        Command::new(bin)
            .arg("--secret-unknown")
            .output()
            .unwrap()
            .status
            .code(),
        Some(2)
    );
    assert_eq!(
        Command::new(bin)
            .args(["server", "--config"])
            .arg(f.file("client.toml"))
            .output()
            .unwrap()
            .status
            .code(),
        Some(2)
    );
}

#[test]
fn wildcard_listener_replies_from_each_original_destination() {
    let mut t = Tunnel::start(false, |f| {
        let text = fs::read_to_string(f.file("client.toml")).unwrap();
        let text = text
            .replacen("listen = \"127.0.0.1:", "listen = \"0.0.0.0:", 1)
            .replace("allow_non_loopback = false", "allow_non_loopback = true");
        fs::write(f.file("client.toml"), text).unwrap();
    });
    // Two local destinations from the same sender tuple need independent,
    // stable associations, not a mutable per-sender return-source address.
    let sender = t.sender();
    for ip in ["127.0.0.2", "127.0.0.3"] {
        t.listen.set_ip(ip.parse().unwrap());
        sender.connect(t.listen).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let peer = loop {
            assert!(
                Instant::now() < deadline,
                "no reply from the original local destination {ip}"
            );
            if sender.send(ip.as_bytes()).is_err() {
                std::thread::sleep(Duration::from_millis(10));
                continue;
            }
            let mut b = [0; 128];
            if let Ok((n, peer)) = t.target.recv_from(&mut b) {
                assert_eq!(&b[..n], ip.as_bytes());
                t.target.send_to(&b[..n], peer).unwrap();
                if let Ok(n) = sender.recv(&mut b) {
                    assert_eq!(&b[..n], ip.as_bytes());
                    break peer;
                }
            }
        };
        for _ in 0..3 {
            sender.send(ip.as_bytes()).unwrap();
            let mut b = [0; 128];
            let (n, source) = t.target.recv_from(&mut b).unwrap();
            assert_eq!(source, peer);
            t.target.send_to(&b[..n], source).unwrap();
            let n = sender.recv(&mut b).unwrap();
            assert_eq!(&b[..n], ip.as_bytes());
        }
    }
    t.stop();
}
