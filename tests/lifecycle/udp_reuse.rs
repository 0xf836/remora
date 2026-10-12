use super::session_tests::{install, runtime};
use super::*;
use crate::common::{Fixtures, quic::Pair};
#[test]
fn old_callback_closed_stream_and_queued_packet_cannot_be_reassigned() {
    runtime().block_on(async {
        let f = Fixtures::new();
        let cfg = ValidatedConfig::load(&f.file("server.toml")).unwrap();
        let resolver = SystemResolver::new(1).unwrap();
        let (diag, _rx) =
            Diagnostics::new(Role::Server, Instant::now(), Duration::from_secs(1)).unwrap();
        let mut r = Runner::new(&cfg, &resolver, diag).unwrap();
        let mut p = Pair::new(true);
        let (a, b) = p.requests();
        install(&mut r, p.server);
        let now = Instant::now();
        let sa = r.create_session(2, a, None, now).unwrap();
        r.sessions.get_mut(&sa).unwrap().state = State::Active;
        r.queue_outgoing(sa, b"old-generation", now).unwrap();
        r.schedule_datagrams(2, now).unwrap();
        assert_eq!(r.connections[&2].transport.quic.dgram_send_queue_len(), 1);
        let key = SetupKey {
            connection: crate::buffers::Generation(2),
            session: crate::buffers::Generation(sa),
        };
        r.close_session(sa, 0x100);
        let sb = r.create_session(2, b, None, now).unwrap();
        assert_ne!(sa, sb);
        r.setup_done(sa, key, Err(PolicyError::Denied), now)
            .unwrap();
        let mut bytes = [0; 64];
        let n = masque::encode_datagram(a, b"late", &mut bytes).unwrap();
        r.datagram(2, &bytes[..n], now + Duration::from_millis(20), now)
            .unwrap();
        assert!(r.sessions[&sb].relay.is_none());
        assert_eq!(r.sessions[&sb].queue.used().count, 0);
        assert_eq!(r.connections[&2].transport.quic.dgram_send_queue_len(), 0);
        assert_eq!(r.budgets.relay_fds.used().count, 0);
        assert_eq!(r.snapshot.counters.local_drops.unknown_flow.to_target, 1);
    });
}
#[test]
fn authorized_late_reply_after_exact_tuple_reuse_is_indistinguishable() {
    // A controlled reused tuple (including a process/application restart) has
    // no UDP generation field. This characterizes the information limit.
    let target = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let old = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let tuple = old.local_addr().unwrap();
    drop(old);
    let new = std::net::UdpSocket::bind(tuple).unwrap();
    new.connect(target.local_addr().unwrap()).unwrap();
    new.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
    target
        .send_to(b"indistinguishable-old-authorized-reply", tuple)
        .unwrap();
    let mut bytes = [0; 128];
    let n = new.recv(&mut bytes).unwrap();
    assert_eq!(&bytes[..n], b"indistinguishable-old-authorized-reply");
    let wrong = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    wrong.send_to(b"wrong-source", tuple).unwrap();
    target.send_to(b"current", tuple).unwrap();
    let n = new.recv(&mut bytes).unwrap();
    assert_eq!(&bytes[..n], b"current");
}
