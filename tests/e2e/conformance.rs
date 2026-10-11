//! A public-API quiche peer injects wire negatives into the real server process.
//! This is contract testing, not independent-stack interoperability evidence.
use crate::common::{ChildGuard, Fixtures};
use remora::{
    config::ValidatedConfig,
    masque,
    transport::{self, TrackedFactory},
};
use std::{
    collections::VecDeque,
    fs,
    net::UdpSocket,
    process::Command,
    time::{Duration, Instant},
};
#[test]
fn malformed_requests_missing_settings_early_datagrams_and_capsules_do_not_forward() {
    for case in 0..6 {
        let f = Fixtures::new();
        let ports: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(f.file("ports.json")).unwrap()).unwrap();
        let target = UdpSocket::bind(format!("127.0.0.1:{}", ports["target"])).unwrap();
        target.set_nonblocking(true).unwrap();
        let remote = format!("127.0.0.1:{}", ports["proxy"]).parse().unwrap();
        let mut server = ChildGuard::spawn_logged(
            Command::new(env!("CARGO_BIN_EXE_remora"))
                .arg("server")
                .arg("--config")
                .arg(f.file("server.toml")),
            &f.file("server.out"),
            &f.file("server.err"),
        )
        .unwrap();
        let settings = ValidatedConfig::load(&f.file("client.toml")).unwrap();
        let mut cfg = transport::config(&settings).unwrap();
        if case == 1 {
            cfg.enable_dgram(false, 0, 0);
        }
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        socket.set_nonblocking(true).unwrap();
        let cid = transport::random_cid().unwrap();
        let mut q = quiche::connect_with_buffer_factory::<TrackedFactory>(
            Some("remora.test"),
            &quiche::ConnectionId::from_ref(&cid),
            socket.local_addr().unwrap(),
            remote,
            &mut cfg,
        )
        .unwrap();
        let mut h3 = None;
        let mut output = VecDeque::new();
        let mut injected = false;
        let mut reset = None;
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut buffer = [0; 65536];
        while Instant::now() < deadline {
            for _ in 0..32 {
                match socket.recv_from(&mut buffer) {
                    Ok((n, from)) => {
                        let _ = q.recv(
                            &mut buffer[..n],
                            quiche::RecvInfo {
                                from,
                                to: socket.local_addr().unwrap(),
                            },
                        );
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) => panic!("peer receive {e:?}"),
                }
            }
            if q.is_established() && h3.is_none() {
                h3 = Some(
                    quiche::h3::Connection::with_transport(
                        &mut q,
                        &transport::h3_config(false).unwrap(),
                    )
                    .unwrap(),
                );
            }
            if let Some(h3) = h3.as_mut() {
                if case == 1 {
                    injected = true;
                }
                while let Ok((_, event)) = h3.poll(&mut q) {
                    if let quiche::h3::Event::Reset(code) = event {
                        reset = Some(code);
                    }
                }
                if !injected && h3.peer_settings_raw().is_some() {
                    let token = fs::read_to_string(f.file("client.token")).unwrap();
                    let authority = format!("remora.test:{}", ports["proxy"]);
                    let path = format!("/.well-known/masque/udp/127.0.0.1/{}/", ports["target"]);
                    let mut headers = masque::request_headers(
                        &authority,
                        &path,
                        token.trim().as_bytes().try_into().unwrap(),
                    );
                    if case == 0 {
                        headers.push(quiche::h3::Header::new(b"content-length", b"0"));
                    }
                    if case == 5 {
                        use quiche::h3::NameValue;
                        headers.retain(|h| h.name() != b"authorization");
                    }
                    if case == 2 {
                        q.dgram_send(&[0, 0, 0x55]).unwrap();
                    } else {
                        let stream = h3.send_request(&mut q, &headers, false).unwrap();
                        if [3, 4].contains(&case) {
                            let mut body = vec![0];
                            if case == 3 {
                                body.push(0);
                            } else {
                                let mut encoded = [0; 8];
                                let n = masque::put_varint(65529, &mut encoded).unwrap();
                                body.extend_from_slice(&encoded[..n]);
                                body.push(0);
                            }
                            h3.send_body(&mut q, stream, &body, false).unwrap();
                        }
                    }
                    injected = true;
                }
            }
            if q.timeout_instant().is_some_and(|at| at <= Instant::now()) {
                q.on_timeout();
            }
            for _ in 0..32 {
                let mut packet = [0; 1350];
                match q.send(&mut packet) {
                    Ok((n, info)) => output.push_back((packet[..n].to_vec(), info)),
                    Err(quiche::Error::Done) => break,
                    Err(_) => break,
                }
            }
            while output
                .front()
                .is_some_and(|(_, info)| info.at <= Instant::now())
            {
                let (bytes, info) = output.pop_front().unwrap();
                socket.send_to(&bytes, info.to).unwrap();
            }
            assert!(
                target.recv_from(&mut buffer).is_err(),
                "negative case {case} emitted target UDP"
            );
            if reset.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(
            injected,
            "peer failed to reach injection point case={case} stderr={} stdout={}",
            fs::read_to_string(f.file("server.err")).unwrap(),
            fs::read_to_string(f.file("server.out")).unwrap()
        );
        if [0, 3].contains(&case) {
            assert_eq!(reset, Some(masque::H3_MESSAGE_ERROR));
        }
        if case == 4 {
            assert_eq!(reset, Some(masque::H3_DATAGRAM_ERROR));
        }
        server.signal(rustix::process::Signal::TERM).unwrap();
        assert!(server.finish(Duration::from_secs(5)).unwrap().success());
        let token = fs::read_to_string(f.file("client.token")).unwrap();
        for name in ["server.out", "server.err"] {
            assert!(
                !fs::read_to_string(f.file(name))
                    .unwrap()
                    .contains(token.trim())
            );
        }
    }
}
