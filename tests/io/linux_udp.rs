use remora::{
    buffers::BufferPool,
    config::{IoMode, IoSettings},
    udp::*,
};
use std::{
    io,
    net::{IpAddr, SocketAddr},
    thread,
    time::{Duration, Instant},
};
fn receive(socket: &mut Udp, pool: &BufferPool) -> (Vec<u8>, Received) {
    let mut slabs = vec![pool.acquire().unwrap()];
    let mut packets = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match socket.recv_batch(&mut slabs, &mut packets) {
            Ok(n) if n > 0 => {
                let meta = packets[0];
                assert_eq!(meta.fault, None);
                return (slabs[0].bytes()[..meta.length].to_vec(), meta);
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
            result => panic!("unexpected receive: {result:?}"),
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
}
#[test]
fn sendmmsg_reports_only_the_prefix_before_a_hard_destination_error() {
    let settings = IoSettings::default();
    let mut sender = Udp::bind("127.0.0.1:0".parse().unwrap(), &settings, false).unwrap();
    let mut receiver = Udp::bind("127.0.0.1:0".parse().unwrap(), &settings, false).unwrap();
    let from = sender.local_addr();
    let packets = [
        SendPacket {
            bytes: b"first",
            from,
            to: receiver.local_addr(),
        },
        SendPacket {
            bytes: b"bad",
            from,
            to: "255.255.255.255:45678".parse().unwrap(),
        },
    ];
    assert_eq!(sender.send_batch(&packets).unwrap(), 1);
    assert!(sender.send_batch(&packets[1..]).is_err());
    let pool = BufferPool::new(1350, 2700).unwrap();
    assert_eq!(receive(&mut receiver, &pool).0, b"first");
    let mut slabs = vec![pool.acquire().unwrap()];
    let mut meta = Vec::new();
    assert_eq!(
        receiver
            .recv_batch(&mut slabs, &mut meta)
            .unwrap_err()
            .kind(),
        io::ErrorKind::WouldBlock
    );
}
#[test]
fn real_batches_preserve_empty_datagrams_metadata_reuse_and_families() {
    for mode in [IoMode::Single, IoMode::Mmsg] {
        for family in ["127.0.0.1:0", "[::1]:0"] {
            let settings = IoSettings {
                mode,
                ..Default::default()
            };
            let mut sender = Udp::bind(family.parse().unwrap(), &settings, false).unwrap();
            let mut receiver = Udp::bind(family.parse().unwrap(), &settings, false).unwrap();
            let pool = BufferPool::new(2048, 8192).unwrap();
            for data in [b"".as_slice(), b"boundary-sentinel", &[7; 1200], b""] {
                assert_eq!(
                    sender
                        .send_batch(&[SendPacket {
                            bytes: data,
                            from: sender.local_addr(),
                            to: receiver.local_addr()
                        }])
                        .unwrap(),
                    1
                );
                let (received, meta) = receive(&mut receiver, &pool);
                assert_eq!(received, data);
                assert_eq!(meta.peer, sender.local_addr());
                assert_eq!(meta.local.ip(), receiver.local_addr().ip());
            }
            let mut slabs = vec![pool.acquire().unwrap()];
            assert_eq!(
                receiver
                    .recv_batch(&mut slabs, &mut Vec::new())
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::WouldBlock
            );
            assert!(receiver.receive_buffer >= settings.outer_socket_buffer_bytes);
            assert!(receiver.send_buffer >= settings.outer_socket_buffer_bytes);
        }
    }
}
#[test]
fn wildcard_pktinfo_and_connected_peer_filtering() {
    let settings = IoSettings::default();
    let mut target = Udp::bind("0.0.0.0:0".parse().unwrap(), &settings, true).unwrap();
    let mut sender = Udp::bind("127.0.0.1:0".parse().unwrap(), &settings, true).unwrap();
    let mut wrong = Udp::bind("127.0.0.1:0".parse().unwrap(), &settings, true).unwrap();
    let to = SocketAddr::new("127.0.0.1".parse().unwrap(), target.local_addr().port());
    sender
        .send_batch(&[SendPacket {
            bytes: b"local",
            from: sender.local_addr(),
            to,
        }])
        .unwrap();
    let pool = BufferPool::new(128, 512).unwrap();
    let (_, meta) = receive(&mut target, &pool);
    assert_eq!(meta.local.ip(), "127.0.0.1".parse::<IpAddr>().unwrap());
    target.connect(sender.local_addr()).unwrap();
    wrong
        .send_batch(&[SendPacket {
            bytes: b"wrong",
            from: wrong.local_addr(),
            to,
        }])
        .unwrap();
    sender
        .send_batch(&[SendPacket {
            bytes: b"right",
            from: sender.local_addr(),
            to,
        }])
        .unwrap();
    assert_eq!(receive(&mut target, &pool).0, b"right");
}
#[test]
fn truncation_rejects_payload_and_emsgsize_never_reports_acceptance() {
    let settings = IoSettings::default();
    let mut sender = Udp::bind("127.0.0.1:0".parse().unwrap(), &settings, false).unwrap();
    let mut receiver = Udp::bind("127.0.0.1:0".parse().unwrap(), &settings, false).unwrap();
    sender
        .send_batch(&[SendPacket {
            bytes: &[9; 100],
            from: sender.local_addr(),
            to: receiver.local_addr(),
        }])
        .unwrap();
    let pool = BufferPool::new(8, 8).unwrap();
    let mut slabs = vec![pool.acquire().unwrap()];
    let mut packets = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match receiver.recv_batch(&mut slabs, &mut packets) {
            Ok(1) => break,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
            r => panic!("{r:?}"),
        }
        assert!(Instant::now() < deadline);
    }
    assert_eq!(packets[0].fault, Some(ReceiveFault::Truncated));
    let data = vec![0; 65536];
    let error = sender
        .send_batch(&[SendPacket {
            bytes: &data,
            from: sender.local_addr(),
            to: receiver.local_addr(),
        }])
        .unwrap_err();
    assert_eq!(error.raw_os_error(), Some(libc::EMSGSIZE));
}
#[test]
fn monotonic_timer_rearm_disarm_and_overflow_wrap() {
    let timer = MonotonicTimer::new().unwrap();
    assert_eq!(
        timer.consume().unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    timer
        .arm(Some(Instant::now() + Duration::from_secs(5)))
        .unwrap();
    timer
        .arm(Some(Instant::now() + Duration::from_millis(2)))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        match timer.consume() {
            Ok(n) => {
                assert_eq!(n, 1);
                break;
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
            r => panic!("{r:?}"),
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    timer.arm(None).unwrap();
    assert_eq!(
        timer.consume().unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    assert_eq!(overflow_delta(u32::MAX - 2, 1), 4);
}
proptest::proptest! {#[test]fn malformed_ancillary_remains_bounded(bytes in proptest::collection::vec(proptest::prelude::any::<u8>(),0..256)){let _=decode_control(&bytes,"0.0.0.0:1234".parse().unwrap());}}

#[test]
fn truncation_flags_are_rejected_before_metadata() {
    use remora::udp::receive_fault;
    for flags in [
        libc::MSG_TRUNC,
        libc::MSG_CTRUNC,
        libc::MSG_TRUNC | libc::MSG_CTRUNC,
    ] {
        assert_eq!(
            receive_fault(flags, false, true),
            Some(ReceiveFault::Truncated)
        );
    }
    assert_eq!(receive_fault(0, true, true), Some(ReceiveFault::Ancillary));
    assert_eq!(receive_fault(0, false, false), Some(ReceiveFault::Address));
    assert_eq!(receive_fault(0, false, true), None);
}
