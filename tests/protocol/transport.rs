use crate::common::quic::Pair;
use remora::{
    masque,
    transport::{self, TransportError},
};
use std::time::{Duration, Instant};
#[test]
fn real_goaway_preserves_lower_stream_delivery_and_bounded_cid_updates() {
    let mut p = Pair::new(true);
    let (a, b) = p.requests();
    for stream in [a, b] {
        p.server
            .h3
            .as_mut()
            .unwrap()
            .send_response(
                &mut p.server.quic,
                stream,
                &masque::response_headers(200),
                false,
            )
            .unwrap();
    }
    for _ in 0..10 {
        p.pump();
        while p
            .client
            .h3
            .as_mut()
            .unwrap()
            .poll(&mut p.client.quic)
            .is_ok()
        {}
    }
    p.server
        .h3
        .as_mut()
        .unwrap()
        .send_goaway(&mut p.server.quic, b)
        .unwrap();
    let mut cutoff = None;
    for _ in 0..20 {
        p.pump();
        while let Ok((id, event)) = p.client.h3.as_mut().unwrap().poll(&mut p.client.quic) {
            if event == quiche::h3::Event::GoAway {
                cutoff = Some(id);
            }
        }
    }
    assert_eq!(cutoff, Some(b));
    p.client.goaway(b).unwrap();
    p.client.close_stream(b, 0x10b);
    assert!(!p.client.may_open());
    let now = Instant::now();
    p.client
        .enqueue(a, b"lower-stream-continues", now, now, &mut [0; 1350])
        .unwrap();
    p.pump();
    let mut bytes = [0; 1350];
    let n = p.server.quic.dgram_recv(&mut bytes).unwrap();
    let decoded = masque::decode_datagram(&bytes[..n]).unwrap().unwrap();
    assert_eq!(decoded.stream, a);
    assert_eq!(decoded.payload, b"lower-stream-continues");
    let mut issued = 0;
    for value in 10..20 {
        let cid = [value; 16];
        if p.server
            .quic
            .new_scid(
                &quiche::ConnectionId::from_ref(&cid),
                u128::from(value),
                false,
            )
            .is_ok()
        {
            issued += 1;
        }
    }
    assert!(issued <= 3);
    assert!(p.server.quic.source_ids().count() <= 4);
}

#[test]
fn settings_required_tls_and_goaway_cutoff() {
    let mut p = Pair::new(true);
    p.negotiate().unwrap();
    assert!(!p.client.quic.is_in_early_data());
    assert_eq!(p.client.quic.application_proto(), b"h3");
    assert!(p.client.quic.peer_cert().is_some());
    let (a, b) = p.requests();
    assert_ne!(a, b);
    p.client.goaway(b).unwrap();
    assert!(!p.client.may_open());
    assert!(p.client.allowed_stream(a));
    assert!(!p.client.allowed_stream(b));
    assert_eq!(p.client.goaway(b + 4), Err(TransportError::Protocol));
    assert_eq!(p.client.goaway(3), Err(TransportError::Protocol));
    assert_eq!(
        Pair::new(false).negotiate(),
        Err(TransportError::Capability)
    );
}
#[test]
fn retry_is_bound_to_address_cids_key_and_monotonic_expiry() {
    let now = Instant::now();
    let retry = transport::Retry::new(now).unwrap();
    let address = "127.0.0.1:43210".parse().unwrap();
    let original = [1; 16];
    let cid = [2; 16];
    let token = retry.mint(address, &original, &cid, now).unwrap();
    assert_eq!(retry.verify(address, &cid, &token, now).unwrap(), original);
    assert!(
        retry
            .verify("127.0.0.1:43211".parse().unwrap(), &cid, &token, now)
            .is_err()
    );
    assert!(retry.verify(address, &[3; 16], &token, now).is_err());
    assert!(
        retry
            .verify(address, &cid, &token, now + Duration::from_secs(11))
            .is_err()
    );
    let mut changed = token;
    changed[10] ^= 1;
    assert!(retry.verify(address, &cid, &changed, now).is_err());
}
#[test]
fn successful_response_receipt_is_retained_until_ack_and_close_cancels() {
    let mut p = Pair::new(true);
    let (a, _) = p.requests();
    let (result, receipt) = transport::track_response(|| {
        p.server.h3.as_mut().unwrap().send_response(
            &mut p.server.quic,
            a,
            &masque::response_headers(200),
            false,
        )
    });
    result.unwrap();
    assert!(receipt.strong_count() > 0);
    for _ in 0..100 {
        p.pump();
        while p
            .client
            .h3
            .as_mut()
            .unwrap()
            .poll(&mut p.client.quic)
            .is_ok()
        {}
        if receipt.strong_count() == 0 {
            return;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("acceptance receipt not released by ACK");
}
#[test]
fn secrets_wait_for_verified_handshake_and_extended_connect_setting() {
    let mut p = Pair::new(true);
    assert!(!p.client.negotiate().unwrap());
    assert!(p.client.h3.is_none());
    for _ in 0..100 {
        p.pump();
        if p.client.quic.is_established() && p.server.quic.is_established() {
            break;
        }
    }
    p.server.h3 = Some(
        quiche::h3::Connection::with_transport(
            &mut p.server.quic,
            &transport::h3_config(false).unwrap(),
        )
        .unwrap(),
    );
    assert_eq!(p.negotiate(), Err(TransportError::Capability));
}
#[test]
fn stop_sending_receipt_release_cannot_authorize_more_datagrams() {
    let mut p = Pair::new(true);
    let (a, _) = p.requests();
    let (result, receipt) = transport::track_response(|| {
        p.server.h3.as_mut().unwrap().send_response(
            &mut p.server.quic,
            a,
            &masque::response_headers(200),
            false,
        )
    });
    result.unwrap();
    assert!(receipt.strong_count() > 0);
    p.client
        .quic
        .stream_shutdown(a, quiche::Shutdown::Read, 0x10c)
        .unwrap();
    for _ in 0..10 {
        p.pump();
    }
    assert!(p.server.quic.stream_capacity(a).is_err());
    let now = Instant::now();
    assert_eq!(
        p.server
            .enqueue(a, b"forbidden-after-stop", now, now, &mut [0; 1350]),
        Err(TransportError::Stopped)
    );
}

#[test]
fn advertised_connection_receive_window_matches_256kib_contract() {
    let mut p = Pair::new(true);
    p.negotiate().unwrap();
    for q in [&p.client.quic, &p.server.quic] {
        let params = q.peer_transport_params().unwrap();
        assert_eq!(params.initial_max_data, 256 * 1024);
        assert_eq!(params.initial_max_stream_data_uni, 16384);
    }
}

#[test]
fn repeated_validated_path_switches_do_not_retain_events() {
    let mut p = Pair::new(true);
    p.negotiate().unwrap();
    for (q, value) in [(&mut p.client.quic, 31), (&mut p.server.quic, 32)] {
        q.new_scid(
            &quiche::ConnectionId::from_ref(&[value; 16]),
            u128::from(value),
            false,
        )
        .unwrap();
    }
    for _ in 0..10 {
        p.pump();
    }
    let first = "127.0.0.1:40000".parse().unwrap();
    let second = "127.0.0.1:40002".parse().unwrap();
    let server = "127.0.0.1:40001".parse().unwrap();
    p.client.quic.probe_path(second, server).unwrap();
    for _ in 0..20 {
        p.pump();
    }
    assert_eq!(p.server.quic.is_path_validated(server, second), Ok(true));
    while p.server.quic.path_event_next().is_some() {}
    for n in 0..100 {
        p.client
            .quic
            .migrate_source(if n % 2 == 0 { second } else { first })
            .unwrap();
        p.client.quic.dgram_send(&[n]).unwrap();
        for _ in 0..3 {
            p.pump();
        }
        let mut bytes = [0; 1350];
        assert_eq!(p.server.quic.dgram_recv(&mut bytes), Ok(1));
        assert_eq!(bytes[0], n);
        assert_eq!(
            p.server.quic.path_event_next(),
            None,
            "owner must consume migration events"
        );
    }
}

#[test]
fn real_h3_exchange_preserves_fields_for_request_and_response_semantic_validation() {
    use quiche::h3::{Event, Header};
    for (name, value) in [
        (b"te".as_slice(), b"gzip".as_slice()),
        (b"host", b"other.test"),
        (b"x-test", b"a\x01b"),
    ] {
        let mut p = Pair::new(true);
        p.negotiate().unwrap();
        let mut headers = masque::request_headers(
            "remora.test",
            "/.well-known/masque/udp/127.0.0.1/53/",
            &[b'a'; 64],
        );
        headers.push(Header::new(name, value));
        let stream = p
            .client
            .h3
            .as_mut()
            .unwrap()
            .send_request(&mut p.client.quic, &headers, false)
            .unwrap();
        let mut observed = false;
        for _ in 0..20 {
            p.pump();
            while let Ok((id, event)) = p.server.h3.as_mut().unwrap().poll(&mut p.server.quic) {
                if let Event::Headers { list, .. } = event {
                    assert_eq!(id, stream);
                    assert_eq!(
                        masque::request(&list, "remora.test", "/.well-known/masque/udp/").err(),
                        Some(masque::WireError::Message)
                    );
                    observed = true;
                }
            }
        }
        assert!(observed, "missing decoded request fields");
    }
    let mut p = Pair::new(true);
    let (stream, _) = p.requests();
    let mut headers = masque::response_headers(200);
    headers.push(Header::new(b"te", b"trailers"));
    p.server
        .h3
        .as_mut()
        .unwrap()
        .send_response(&mut p.server.quic, stream, &headers, false)
        .unwrap();
    let mut observed = false;
    for _ in 0..20 {
        p.pump();
        while let Ok((id, event)) = p.client.h3.as_mut().unwrap().poll(&mut p.client.quic) {
            if let Event::Headers { list, .. } = event {
                assert_eq!(id, stream);
                assert_eq!(
                    masque::response(&list, false),
                    Err(masque::WireError::Message)
                );
                observed = true;
            }
        }
    }
    assert!(observed, "missing decoded response fields");
}
