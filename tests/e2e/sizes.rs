use super::forwarding::Tunnel;
use std::time::{Duration, Instant};

#[test]
fn narrow_outer_path_reports_exact_local_rejection_in_both_directions() {
    let mut tunnel = Tunnel::start(false, |f| {
        for name in ["client.toml", "server.toml"] {
            let file = f.file(name);
            let text = std::fs::read_to_string(&file).unwrap();
            std::fs::write(
                file,
                text + "\n[transport]\nouter_udp_max_bytes=1200\napplication_max_bytes=1400\n",
            )
            .unwrap();
        }
    });
    let sender = tunnel.sender();
    let relay = tunnel.ready(&sender, b"narrow");
    let mut bytes = [0; 1500];
    sender.send_to(&[0x7a; 1312], tunnel.listen).unwrap();
    assert!(tunnel.target.recv_from(&mut bytes).is_err());
    tunnel.target.send_to(&[0x7b; 1312], relay).unwrap();
    assert!(sender.recv_from(&mut bytes).is_err());
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let both = [("client", "to_target"), ("server", "to_client")]
            .into_iter()
            .all(|(role, direction)| {
                super::diagnostics::snapshots(&tunnel, role)
                    .iter()
                    .any(|record| {
                        record["failures"].as_array().is_some_and(|failures| {
                            failures.iter().any(|f| {
                                f["class"] == "size"
                                    && f["code"] == 2
                                    && f["size"]["direction"] == direction
                                    && f["size"]["datagram_bytes"] == 1312
                                    && f["size"]["application_cap_bytes"] == 1400
                                    && f["size"]["outgoing_ceiling_bytes"]
                                        .as_u64()
                                        .is_some_and(|n| n < 1312)
                                    && f["connection"].is_string()
                                    && f["session"].is_string()
                            })
                        })
                    })
            });
        if both {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "missing correlated directional size details"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(tunnel.ready(&sender, b"still-usable"), relay);
    tunnel.stop();
}

#[test]
fn configured_size_boundary_bursts_and_rejection_preserve_other_flows() {
    for (ipv6, cap) in [(false, 1200), (true, 1200), (false, 1400), (true, 1400)] {
        let mut tunnel = Tunnel::start(ipv6, |f| {
            if cap == 1200 {
                for name in ["client.toml", "server.toml"] {
                    let file = f.file(name);
                    let text = std::fs::read_to_string(&file).unwrap();
                    std::fs::write(file, text + "\n[transport]\nouter_udp_max_bytes=1350\napplication_max_bytes=1200\n").unwrap();
                }
            }
        });
        let a = tunnel.sender();
        let b = tunnel.sender();
        let relay_a = tunnel.ready(&a, b"size-a");
        let relay_b = tunnel.ready(&b, b"size-b");
        assert_ne!(relay_a, relay_b);
        let mut bytes = [0; 65536];
        // A small setup exchange does not prove PMTU discovery has completed.
        tunnel.ready(&a, &vec![0x71; cap]);
        for size in [0, 1, 128, 512, 1200, 1312, 1392, 1400]
            .into_iter()
            .filter(|size| *size <= cap)
        {
            let payload = vec![size as u8; size];
            a.send_to(&payload, tunnel.listen).unwrap();
            let (n, from) = tunnel.target.recv_from(&mut bytes).unwrap();
            assert_eq!(from, relay_a);
            assert_eq!(&bytes[..n], payload);
            tunnel.target.send_to(&bytes[..n], from).unwrap();
            let (n, from) = a.recv_from(&mut bytes).unwrap();
            assert_eq!(from, tunnel.listen);
            assert_eq!(n, size);
            assert_eq!(&bytes[..n], payload);
        }
        a.send_to(&vec![0xda; cap + 1], tunnel.listen).unwrap();
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
