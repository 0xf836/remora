use remora::{
    buffers::BufferPool,
    config::{IoMode, IoSettings, Offload},
    diagnostics::OffloadReport,
    udp::{SendPacket, Udp, decode_control},
};
use std::{
    io,
    mem::size_of,
    time::{Duration, Instant},
};

fn udp_control(kind: i32, data: &[u8]) -> Vec<u8> {
    let header = size_of::<libc::cmsghdr>();
    let length = header + data.len();
    let mut bytes = vec![0; length.next_multiple_of(size_of::<usize>())];
    bytes[..size_of::<usize>()].copy_from_slice(&length.to_ne_bytes());
    bytes[size_of::<usize>()..size_of::<usize>() + 4]
        .copy_from_slice(&libc::IPPROTO_UDP.to_ne_bytes());
    bytes[size_of::<usize>() + 4..size_of::<usize>() + 8].copy_from_slice(&kind.to_ne_bytes());
    bytes[header..length].copy_from_slice(data);
    bytes
}

#[test]
fn unimplemented_gro_never_forwards_an_aggregate_or_unknown_segment_marker() {
    let local = "127.0.0.1:40000".parse().unwrap();
    for kind in [libc::UDP_GRO, 0x7fff] {
        for data in [vec![], vec![0], 1200u16.to_ne_bytes().to_vec(), vec![0; 4]] {
            let bytes = udp_control(kind, &data);
            assert!(decode_control(&bytes, local).2);
            for end in 1..bytes.len() {
                assert!(decode_control(&bytes[..end], local).2);
            }
        }
    }
    assert!(!decode_control(&[], local).2);
}

#[test]
fn native_off_and_deferred_auto_preserve_logical_empty_mixed_prefix_without_replay() {
    for offload in [Offload::Off, Offload::Auto] {
        let report = serde_json::to_value(OffloadReport::new(offload)).unwrap();
        assert_eq!(report["effective_on"], false);
        assert_eq!(
            report["reason"],
            if offload == Offload::Off {
                "configured_off"
            } else {
                "not_implemented"
            }
        );
        assert_eq!(
            report["requested"],
            if offload == Offload::Off {
                "off"
            } else {
                "auto"
            }
        );
        for mode in [IoMode::Single, IoMode::Mmsg] {
            let settings = IoSettings {
                mode,
                gso: offload,
                gro: offload,
                ..Default::default()
            };
            let mut receiver = Udp::bind("127.0.0.1:0".parse().unwrap(), &settings, true).unwrap();
            let mut sender = Udp::bind("127.0.0.1:0".parse().unwrap(), &settings, true).unwrap();
            sender.connect(receiver.local_addr()).unwrap();
            let too_large = vec![0; sender.payload_ceiling().unwrap().unwrap() + 1];
            let bytes = [b"".as_slice(), b"mixed", &[7; 1200], too_large.as_slice()];
            let packets: Vec<_> = bytes
                .iter()
                .map(|bytes| SendPacket {
                    bytes,
                    from: sender.local_addr(),
                    to: receiver.local_addr(),
                })
                .collect();
            let mut accepted = 0;
            while accepted < 3 {
                accepted += sender.send_batch(&packets[accepted..]).unwrap();
            }
            assert_eq!(accepted, 3);
            assert_eq!(
                sender.send_batch(&packets[3..]).unwrap_err().raw_os_error(),
                Some(libc::EMSGSIZE)
            );
            assert_eq!(sender.stats().sent_datagrams, 3);
            assert_eq!(
                sender.stats().send_syscalls,
                if mode == IoMode::Single { 3 } else { 1 }
            );
            let pool = BufferPool::new(1350, 1350 * 4).unwrap();
            let mut slabs = vec![
                pool.acquire().unwrap(),
                pool.acquire().unwrap(),
                pool.acquire().unwrap(),
            ];
            let mut seen = Vec::new();
            let deadline = Instant::now() + Duration::from_secs(1);
            while seen.len() < 3 {
                let mut meta = Vec::new();
                match receiver.recv_batch(&mut slabs, &mut meta) {
                    Ok(_) => {
                        for m in meta {
                            assert_eq!(m.fault, None);
                            seen.push(slabs[m.index].bytes()[..m.length].to_vec());
                        }
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline);
                    }
                    result => panic!("unexpected receive: {result:?}"),
                }
            }
            assert_eq!(
                seen,
                bytes[..3].iter().map(|b| b.to_vec()).collect::<Vec<_>>()
            );
            assert_eq!(
                receiver
                    .recv_batch(&mut slabs, &mut Vec::new())
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::WouldBlock
            );
            assert!(!sender.stats().overflowed);
        }
    }
}
