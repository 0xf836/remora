use super::forwarding::Tunnel;
use std::time::{Duration, Instant};

#[test]
fn configured_size_boundary_bursts_and_rejection_preserve_other_flows() {
    for ipv6 in [false, true] {
        let mut tunnel = Tunnel::start(ipv6, |_| {});
        let a = tunnel.sender();
        let b = tunnel.sender();
        let relay_a = tunnel.ready(&a, b"size-a");
        let relay_b = tunnel.ready(&b, b"size-b");
        assert_ne!(relay_a, relay_b);
        let mut bytes = [0; 65536];
        for size in [0, 1, 128, 512, 1200] {
            let payload = vec![size as u8; size];
            a.send_to(&payload, tunnel.listen).unwrap();
            let (n, from) = tunnel.target.recv_from(&mut bytes).unwrap();
            assert_eq!(from, relay_a);
            assert_eq!(&bytes[..n], payload);
            tunnel.target.send_to(&bytes[..n], from).unwrap();
            assert_eq!(a.recv_from(&mut bytes).unwrap().0, size);
        }
        a.send_to(&[0xda; 1201], tunnel.listen).unwrap();
        assert!(
            tunnel.target.recv_from(&mut bytes).is_err(),
            "oversized data cannot be truncated, fragmented privately or sent reliably"
        );
        // Opaque tagged bursts are checked for integrity/routing, not guaranteed
        // delivery under unreliable queue pressure. Follow with a usable peer.
        for sequence in 0u8..64 {
            let mut payload = [0x68; 128];
            payload[0] = sequence;
            a.send_to(&payload, tunnel.listen).unwrap();
        }
        tunnel
            .target
            .set_read_timeout(Some(Duration::from_millis(20)))
            .unwrap();
        let deadline = Instant::now() + Duration::from_millis(300);
        let mut seen = std::collections::BTreeSet::new();
        while Instant::now() < deadline {
            if let Ok((n, from)) = tunnel.target.recv_from(&mut bytes) {
                assert_eq!(from, relay_a);
                assert_eq!(n, 128);
                assert_eq!(&bytes[1..n], &[0x68; 127]);
                assert!(seen.insert(bytes[0]), "duplicate replay");
            }
        }
        assert!(!seen.is_empty());
        tunnel
            .target
            .set_read_timeout(Some(Duration::from_millis(150)))
            .unwrap();
        assert_eq!(tunnel.ready(&b, b"still-usable"), relay_b);
        assert_eq!(tunnel.ready(&a, b"after-rejection"), relay_a);
        // Counters obey the ordinary one-second publication cadence; a run
        // ending earlier need not have a counter snapshot at all.
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let stdout =
                std::fs::read_to_string(tunnel.fixtures.file("client.out")).unwrap_or_default();
            let observed = stdout
                .lines()
                .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
                .any(|s| {
                    ["oversized", "socket_truncated"].iter().any(|reason| {
                        s["counters"]["local_drops"][reason]["to_target"]
                            .as_u64()
                            .unwrap_or(0)
                            > 0
                    })
                });
            if observed {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "missing observed oversize/truncation counter"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        tunnel.stop();
    }
}
