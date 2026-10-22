use proptest::prelude::*;
use remora::{config::IoSettings, transport::TransportError, udp::*};
use std::{io, mem::size_of, net::SocketAddr, time::Instant};

fn error_control(ipv6: bool, errno: u32, origin: u8, ty: u8, code: u8, mtu: u32) -> Vec<u8> {
    let header = size_of::<libc::cmsghdr>();
    let length = header + size_of::<libc::sock_extended_err>();
    let aligned = length.next_multiple_of(size_of::<usize>());
    let mut bytes = vec![0; aligned];
    bytes[..size_of::<usize>()].copy_from_slice(&length.to_ne_bytes());
    let level = if ipv6 {
        libc::IPPROTO_IPV6
    } else {
        libc::IPPROTO_IP
    };
    let kind = if ipv6 {
        libc::IPV6_RECVERR
    } else {
        libc::IP_RECVERR
    };
    bytes[size_of::<usize>()..size_of::<usize>() + 4].copy_from_slice(&level.to_ne_bytes());
    bytes[size_of::<usize>() + 4..size_of::<usize>() + 8].copy_from_slice(&kind.to_ne_bytes());
    bytes[header..header + 4].copy_from_slice(&errno.to_ne_bytes());
    bytes[header + 4..header + 8].copy_from_slice(&[origin, ty, code, 0]);
    bytes[header + 8..header + 12].copy_from_slice(&mtu.to_ne_bytes());
    bytes
}

#[test]
fn checked_error_metadata_family_origin_duplicate_and_truncation() {
    for ipv6 in [false, true] {
        let to: SocketAddr = if ipv6 { "[::1]:53" } else { "127.0.0.1:53" }
            .parse()
            .unwrap();
        let origin = if ipv6 {
            libc::SO_EE_ORIGIN_ICMP6
        } else {
            libc::SO_EE_ORIGIN_ICMP
        };
        let (ty, code) = if ipv6 { (2, 0) } else { (3, 4) };
        let bytes = error_control(ipv6, libc::EMSGSIZE as u32, origin, ty, code, 1400);
        assert_eq!(
            decode_error_control(&bytes, to).unwrap(),
            Some(SocketError {
                destination: to,
                errno: libc::EMSGSIZE as u32,
                mtu: Some(1400)
            })
        );
        for end in 1..size_of::<libc::cmsghdr>() + size_of::<libc::sock_extended_err>() {
            assert!(decode_error_control(&bytes[..end], to).is_err());
        }
        let duplicate = [bytes.as_slice(), bytes.as_slice()].concat();
        assert!(decode_error_control(&duplicate, to).is_err());
        assert_eq!(
            decode_error_control(&error_control(ipv6, 90, 255, ty, code, 1400), to).unwrap(),
            None
        );
        assert!(
            decode_error_control(
                &error_control(ipv6, libc::EMSGSIZE as u32, origin, ty, code, 1),
                to
            )
            .is_err()
        );
        assert!(
            decode_error_control(
                &error_control(ipv6, libc::EMSGSIZE as u32, origin, 99, 99, 1400),
                to
            )
            .is_err()
        );
        assert_eq!(
            udp_payload_limit(1400, ipv6),
            Some(if ipv6 { 1352 } else { 1372 })
        );
        assert!(udp_payload_limit(0, ipv6).is_none());
    }
}

#[test]
fn connected_kernel_ceiling_keeps_empty_exact_limit_and_accepted_prefix() {
    for family in ["127.0.0.1:0", "[::1]:0"] {
        let mut receiver =
            Udp::bind(family.parse().unwrap(), &IoSettings::default(), true).unwrap();
        let mut sender = Udp::bind(family.parse().unwrap(), &IoSettings::default(), true).unwrap();
        sender.connect(receiver.local_addr()).unwrap();
        let ceiling = sender.payload_ceiling().unwrap().unwrap();
        assert!(ceiling >= 1200);
        let full = vec![0x57; ceiling];
        let too_big = vec![0x58; ceiling + 1];
        let from = sender.local_addr();
        let to = receiver.local_addr();
        assert_eq!(
            sender
                .send_batch(&[
                    SendPacket {
                        bytes: &[],
                        from,
                        to
                    },
                    SendPacket {
                        bytes: &too_big,
                        from,
                        to
                    }
                ])
                .unwrap(),
            1
        );
        assert_eq!(
            sender
                .send_batch(&[SendPacket {
                    bytes: &too_big,
                    from,
                    to
                }])
                .unwrap_err()
                .raw_os_error(),
            Some(libc::EMSGSIZE)
        );
        let pool = remora::buffers::BufferPool::new(65527, 65527 * 2).unwrap();
        let mut buffers = vec![pool.acquire().unwrap()];
        let mut metadata = Vec::new();
        assert_eq!(receiver.recv_batch(&mut buffers, &mut metadata).unwrap(), 1);
        assert_eq!(metadata[0].length, 0);
        assert_eq!(
            sender
                .send_batch(&[SendPacket {
                    bytes: &full,
                    from,
                    to
                }])
                .unwrap(),
            1
        );
        assert_eq!(receiver.recv_batch(&mut buffers, &mut metadata).unwrap(), 1);
        assert_eq!(metadata[0].fault, None);
        assert_eq!(&buffers[0].bytes()[..metadata[0].length], full);
        assert_eq!(
            receiver
                .recv_batch(&mut buffers, &mut metadata)
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        assert!(!sender.drain_errors().unwrap().pending);
    }
}

#[test]
fn native_path_reduction_purges_only_oversized_uncommitted_messages() {
    let mut pair = crate::common::quic::Pair::new(true);
    let (stream, _) = pair.requests();
    for _ in 0..10 {
        pair.pump();
    }
    let old = pair.client.payload_ceiling(stream);
    assert!(old >= 1200);
    let now = Instant::now();
    let mut scratch = [0; 1350];
    pair.client
        .enqueue(stream, &[42; 1200], now, now, &mut scratch)
        .unwrap();
    pair.client
        .enqueue(stream, &[], now, now, &mut scratch)
        .unwrap();
    // Invalidate the larger verified PMTU through the real public backend API.
    pair.client.quic.revalidate_pmtu();
    assert!(pair.client.payload_ceiling(stream) < 1200);
    assert_eq!(
        pair.client
            .enqueue(stream, &[42; 1200], now, now, &mut scratch),
        Err(TransportError::Pmtu)
    );
    pair.client.generate(&pair.pool, now, 4).unwrap();
    assert_eq!(pair.client.take_pmtu_drops(), 1);
    assert_eq!(pair.client.take_pmtu_drops(), 0);
    assert!(pair.client.resource_usage().0 <= 2);
    assert!(!pair.client.is_stopped());
}

#[test]
fn selected_large_profile_revalidates_without_changing_the_backend() {
    let mut pair = crate::common::quic::Pair::with_config(true, |f| {
        for name in ["client.toml", "server.toml"] {
            let file = f.file(name);
            let text = std::fs::read_to_string(&file).unwrap();
            std::fs::write(
                file,
                text + "\n[transport]\nouter_udp_max_bytes=1452\napplication_max_bytes=1400\n",
            )
            .unwrap();
        }
    });
    let (stream, _) = pair.requests();
    let wait_large = |p: &mut crate::common::quic::Pair| {
        let deadline = Instant::now() + std::time::Duration::from_secs(2);
        while p.client.payload_ceiling(stream) < 1400 || p.server.payload_ceiling(stream) < 1400 {
            assert!(
                Instant::now() < deadline,
                "discovery did not reach selected profile"
            );
            p.pump();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(p.client.quic.pmtu(), Some(1452));
        assert_eq!(p.server.quic.pmtu(), Some(1452));
        assert_eq!(
            p.client
                .operational_stats()
                .application_payload_lower_bound_min,
            1400
        );
    };
    wait_large(&mut pair);
    let mut scratch = [0; 1452];
    let now = Instant::now();
    pair.client
        .enqueue(stream, &[42; 1312], now, now, &mut scratch)
        .unwrap();
    pair.client
        .enqueue(stream, &[], now, now, &mut scratch)
        .unwrap();
    pair.client.quic.revalidate_pmtu();
    assert!(pair.client.payload_ceiling(stream) < 1312);
    assert!(
        pair.client
            .operational_stats()
            .application_payload_lower_bound_min
            < 1312
    );
    assert_eq!(
        pair.client
            .enqueue(stream, &[43; 1312], now, now, &mut scratch),
        Err(TransportError::Pmtu)
    );
    pair.client.generate(&pair.pool, now, 4).unwrap();
    assert_eq!(pair.client.take_pmtu_drops(), 1);
    assert!(!pair.client.is_stopped());
    // The committed output must reach its original peer; do not silently
    // bypass it while waiting for discovery to recover.
    while let Some(packet) = pair.client.output.pop_front() {
        let mut bytes = packet.bytes().to_vec();
        let from = packet.from();
        let to = packet.to();
        pair.server
            .receive(&mut bytes, quiche::RecvInfo { from, to }, now)
            .unwrap();
    }
    wait_large(&mut pair);
    assert_eq!(
        pair.client.enqueue(
            stream,
            &[44; 1401],
            Instant::now(),
            Instant::now(),
            &mut scratch
        ),
        Err(TransportError::Oversized)
    );
}

proptest! {
    #[test]
    fn arbitrary_error_ancillary_is_bounded_and_panic_free(bytes in prop::collection::vec(any::<u8>(),0..1024), ipv6 in any::<bool>()) {
        let to: SocketAddr = if ipv6 { "[::1]:53" } else { "127.0.0.1:53" }.parse().unwrap();
        if let Ok(Some(error)) = decode_error_control(&bytes,to) {
            prop_assert_eq!(error.destination,to);
            if let Some(mtu) = error.mtu { prop_assert!(udp_payload_limit(mtu,ipv6).is_some()); }
        }
    }
}

#[test]
fn actual_socket_fragment_policy_distinguishes_outer_probes_and_target_packets() {
    use std::os::fd::AsRawFd;
    fn option(socket: &Udp, level: i32, name: i32) -> i32 {
        let mut value = 0i32;
        let mut length = size_of::<i32>() as libc::socklen_t;
        // SAFETY: live owned descriptor, initialized aligned integer and length;
        // the synchronous getter can write only their exact native capacities.
        let result = unsafe {
            libc::getsockopt(
                socket.as_raw_fd(),
                level,
                name,
                (&mut value as *mut i32).cast(),
                &mut length,
            )
        };
        assert_eq!(result, 0);
        assert_eq!(length as usize, size_of::<i32>());
        value
    }
    for (family, level, discover) in [
        ("127.0.0.1:0", libc::IPPROTO_IP, libc::IP_MTU_DISCOVER),
        ("[::1]:0", libc::IPPROTO_IPV6, libc::IPV6_MTU_DISCOVER),
    ] {
        for target in [false, true] {
            let socket =
                Udp::bind(family.parse().unwrap(), &IoSettings::default(), target).unwrap();
            assert_eq!(
                option(&socket, level, discover),
                if target {
                    libc::IP_PMTUDISC_DO
                } else {
                    libc::IP_PMTUDISC_PROBE
                }
            );
            if level == libc::IPPROTO_IPV6 {
                assert_eq!(option(&socket, level, libc::IPV6_DONTFRAG), 1);
            }
        }
    }
}

#[test]
fn native_icmp_observation_discards_quotes_and_keeps_immutable_peer_usable() {
    use std::net::UdpSocket;
    let selected = UdpSocket::bind("127.0.0.1:0").unwrap();
    let to = selected.local_addr().unwrap();
    drop(selected);
    let mut sender =
        Udp::bind("127.0.0.1:0".parse().unwrap(), &IoSettings::default(), true).unwrap();
    sender.connect(to).unwrap();
    assert_eq!(
        sender
            .send_batch(&[SendPacket {
                bytes: b"ignored-quote-sentinel",
                from: sender.local_addr(),
                to
            }])
            .unwrap(),
        1
    );
    let deadline = Instant::now() + std::time::Duration::from_secs(1);
    loop {
        let errors = sender.drain_errors().unwrap();
        let observed: Vec<_> = errors.errors.into_iter().flatten().collect();
        if !observed.is_empty() {
            assert_eq!(observed.len(), 1);
            assert_eq!(
                observed[0],
                SocketError {
                    destination: to,
                    errno: libc::ECONNREFUSED as u32,
                    mtu: None
                }
            );
            break;
        }
        assert!(Instant::now() < deadline);
    }
    assert!(is_peer_error(&io::Error::from_raw_os_error(
        libc::ECONNREFUSED
    )));
    assert!(is_peer_error(&io::Error::from_raw_os_error(libc::EMSGSIZE)));
    assert!(!is_peer_error(&io::Error::from_raw_os_error(libc::EBADF)));
    let receiver = UdpSocket::bind(to).unwrap();
    receiver
        .set_read_timeout(Some(std::time::Duration::from_secs(1)))
        .unwrap();
    assert_eq!(
        sender
            .send_batch(&[SendPacket {
                bytes: b"fresh",
                from: sender.local_addr(),
                to
            }])
            .unwrap(),
        1
    );
    let mut bytes = [0; 64];
    let (n, from) = receiver.recv_from(&mut bytes).unwrap();
    assert_eq!(&bytes[..n], b"fresh");
    assert_eq!(from, sender.local_addr());
    assert!(
        sender
            .drain_errors()
            .unwrap()
            .errors
            .into_iter()
            .all(|e| e.is_none())
    );
}
