//! Owner component tests are compiled inside reactor so they exercise cleanup.
use super::*;
use crate::common::{Fixtures, quic::Pair};
use proptest::prelude::*;

pub(super) fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}
pub(super) fn install(r: &mut Runner<'_>, transport: Connection) {
    let permit = r.budgets.connections.reserve(1, 0).unwrap();
    let id = r.diagnostics.next_id().unwrap();
    r.connections.insert(
        2,
        OwnedConnection {
            transport,
            socket: 0,
            _permit: permit,
            _socket_permit: None,
            id,
            initial_cid: vec![],
            cursors: [FairCursor::default(); 7],
            receive_density: ReceiveDensity::default(),
            enqueue_pending: false,
            output_pending: false,
        },
    );
}

#[test]
fn full_ciphertext_backpressure_preserves_packets_and_resumes_after_prefix_drain() {
    for future in [false, true] {
        runtime().block_on(async {
            let f = Fixtures::new();
            let cfg = ValidatedConfig::load(&f.file("server.toml")).unwrap();
            let resolver = SystemResolver::new(1).unwrap();
            let (diag, _rx) =
                Diagnostics::new(Role::Server, Instant::now(), Duration::from_secs(1)).unwrap();
            let mut r = Runner::new(&cfg, &resolver, diag).unwrap();
            let mut blocked = Pair::new(true);
            let (a, b) = blocked.requests();
            install(&mut r, blocked.server);
            let held = r.connections.remove(&2).unwrap();
            let mut independent = Pair::new(true);
            let (other_stream, _) = independent.requests();
            independent.server.generation = crate::buffers::Generation(3);
            install(&mut r, independent.server);
            let other = r.connections.remove(&2).unwrap();
            r.connections.insert(3, other);
            r.connections.insert(2, held);
            let now = Instant::now();
            let mut ids = Vec::new();
            for stream in [a, b] {
                let id = r.create_session(2, stream, None, now).unwrap();
                r.sessions.get_mut(&id).unwrap().state = State::Active;
                for _ in 0..2 {
                    r.queue_outgoing(id, &[7; 128], now).unwrap();
                }
                ids.push(id);
            }
            for sequence in 0..32 {
                let mut slab = r.pools.ciphertext.acquire().unwrap();
                slab.bytes_mut()[0] = sequence as u8;
                r.connections
                    .get_mut(&2)
                    .unwrap()
                    .transport
                    .output
                    .push_back(
                        crate::buffers::Ciphertext::new(
                            slab,
                            1,
                            "127.0.0.1:40001".parse().unwrap(),
                            "127.0.0.1:40000".parse().unwrap(),
                            now + Duration::from_millis(if future { 10 } else { 0 }),
                            crate::buffers::Generation(2),
                            sequence,
                            now,
                            now + Duration::from_millis(100),
                        )
                        .unwrap(),
                    );
            }
            let original: Vec<_> = ids
                .iter()
                .map(|id| {
                    let p = r.sessions[id].queue.front(Direction::ToClient).unwrap();
                    (
                        p.session(),
                        p.connection(),
                        p.received_at(),
                        p.expires_at(),
                        p.bytes().to_vec(),
                    )
                })
                .collect();
            let cursor = r.connections[&2].cursors[3];
            assert_eq!(r.connections[&2].transport.quic.dgram_send_queue_len(), 0);
            let mut scratch = [0; 1350];
            assert_eq!(
                r.connections.get_mut(&2).unwrap().transport.enqueue(
                    a,
                    &[7; 128],
                    now,
                    now,
                    &mut scratch
                ),
                Err(TransportError::Capacity)
            );
            assert!(
                !r.schedule_datagrams(2, now).unwrap(),
                "blocked enqueue must not request a busy continuation"
            );
            assert_eq!(r.connections[&2].cursors[3].last, cursor.last);
            assert_eq!(r.connections[&2].cursors[3].ceiling, cursor.ceiling);
            assert!(!r.connections[&2].enqueue_pending);
            let offered = std::rc::Rc::new(std::cell::Cell::new(0));
            let observed = offered.clone();
            r.outer_send_hook = Some(Box::new(move |_| {
                observed.set(observed.get() + 1);
                Err(std::io::Error::from_raw_os_error(libc::EAGAIN))
            }));
            r.service_output(2, now).unwrap();
            assert_eq!(offered.get(), if future { 0 } else { 1 });
            assert_eq!(r.connections[&2].transport.output.len(), 32);
            assert_eq!(r.connections[&2].transport.quic.dgram_send_queue_len(), 0);
            for (&id, expected) in ids.iter().zip(&original) {
                let p = r.sessions[&id].queue.front(Direction::ToClient).unwrap();
                assert_eq!(
                    (
                        p.session(),
                        p.connection(),
                        p.received_at(),
                        p.expires_at(),
                        p.bytes().to_vec()
                    ),
                    *expected
                );
                assert_eq!(r.sessions[&id].queue.used().count, 2);
            }
            // The blocked connection does not stop another one's application work.
            let other = r.create_session(3, other_stream, None, now).unwrap();
            r.sessions.get_mut(&other).unwrap().state = State::Active;
            r.queue_outgoing(other, &[8; 128], now).unwrap();
            r.schedule_datagrams(3, now).unwrap();
            assert_eq!(r.connections[&3].transport.quic.dgram_send_queue_len(), 1);
            // Accept exactly one committed descriptor. Nothing is regenerated,
            // and application work can use the newly available output credit.
            let accepted = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
            let observed = accepted.clone();
            r.outer_send_hook = Some(Box::new(move |packets| {
                observed.borrow_mut().push(packets[0].bytes.to_vec());
                Ok(1)
            }));
            let due = now + Duration::from_millis(11);
            r.flush_limit(2, due, 1).unwrap();
            assert_eq!(*accepted.borrow(), vec![vec![0]]);
            assert_eq!(r.connections[&2].transport.output.len(), 31);
            assert_eq!(
                r.connections[&2].transport.output.front().unwrap().bytes(),
                &[1]
            );
            r.schedule_datagrams(2, due).unwrap();
            assert_eq!(r.connections[&2].transport.quic.dgram_send_queue_len(), 4);
            for id in ids {
                assert_eq!(r.sessions[&id].queue.used().count, 0);
            }
            assert!(r.connections[&2].transport.deadline(due).unwrap() <= original[0].3);
        });
    }
}

#[test]
fn full_ciphertext_backpressure_does_not_prevent_expiry_or_control_cancellation() {
    runtime().block_on(async {
        let f = Fixtures::new();
        let cfg = ValidatedConfig::load(&f.file("server.toml")).unwrap();
        let resolver = SystemResolver::new(1).unwrap();
        let (diag, _rx) =
            Diagnostics::new(Role::Server, Instant::now(), Duration::from_secs(1)).unwrap();
        let mut r = Runner::new(&cfg, &resolver, diag).unwrap();
        let mut peer = Pair::new(true);
        let (a, b) = peer.requests();
        install(&mut r, peer.server);
        let now = Instant::now();
        let mut ids = Vec::new();
        for stream in [a, b] {
            let id = r.create_session(2, stream, None, now).unwrap();
            r.sessions.get_mut(&id).unwrap().state = State::Active;
            r.queue_outgoing(id, &[7; 128], now).unwrap();
            ids.push(id);
        }
        for sequence in 0..32 {
            let mut slab = r.pools.ciphertext.acquire().unwrap();
            slab.bytes_mut()[0] = sequence as u8;
            r.connections
                .get_mut(&2)
                .unwrap()
                .transport
                .output
                .push_back(
                    crate::buffers::Ciphertext::new(
                        slab,
                        1,
                        "127.0.0.1:40001".parse().unwrap(),
                        "127.0.0.1:40000".parse().unwrap(),
                        now + Duration::from_millis(50),
                        crate::buffers::Generation(2),
                        sequence,
                        now,
                        now + Duration::from_millis(100),
                    )
                    .unwrap(),
                );
        }
        assert!(!r.schedule_datagrams(2, now).unwrap());
        assert_eq!(r.connections[&2].transport.quic.dgram_send_queue_len(), 0);
        r.close_session(ids[1], 0x100);
        assert!(!r.sessions.contains_key(&ids[1]));
        r.service_transport_timers(now + Duration::from_millis(21))
            .unwrap();
        r.schedule_datagrams(2, now + Duration::from_millis(21))
            .unwrap();
        assert_eq!(r.sessions[&ids[0]].queue.used().count, 0);
        assert_eq!(r.snapshot.counters.local_drops.queue_expired.to_client, 1);
        assert_eq!(r.connections[&2].transport.quic.dgram_send_queue_len(), 0);
        assert_eq!(
            r.connections[&2].transport.output.len(),
            32,
            "session cancellation cannot rewrite committed output"
        );
    });
}
#[test]
fn pending_identity_quota_duplicate_completion_cancellation_and_joint_idle_close() {
    runtime().block_on(async {
        let f = Fixtures::new();
        let path = f.file("server.toml");
        std::fs::write(
            &path,
            std::fs::read_to_string(&path).unwrap() + "\n[limits]\nmax_sessions_per_identity = 1\n",
        )
        .unwrap();
        let cfg = ValidatedConfig::load(&f.file("server.toml")).unwrap();
        let resolver = SystemResolver::new(1).unwrap();
        let (diag, _rx) =
            Diagnostics::new(Role::Server, Instant::now(), Duration::from_secs(1)).unwrap();
        let mut r = Runner::new(&cfg, &resolver, diag).unwrap();
        let mut pair = Pair::new(true);
        let (a, b) = pair.requests();
        install(&mut r, pair.server);
        let now = Instant::now();
        let sa = r.create_session(2, a, None, now).unwrap();
        let sb = r.create_session(2, b, None, now).unwrap();
        r.bind_identity(sa, 0).unwrap();
        assert_eq!(r.identities[0].used().count, 1, "pending DNS counts");
        assert!(r.bind_identity(sb, 0).is_err());
        assert!(
            r.create_session(2, a, None, now).is_err(),
            "duplicate stream"
        );
        let key = SetupKey {
            connection: crate::buffers::Generation(2),
            session: crate::buffers::Generation(sa),
        };
        let target = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let address = target.local_addr().unwrap();
        let result = Ok((0, policy::ResolvedTarget { address, key }));
        r.setup_done(sa, key, result, now).unwrap();
        let relay = r.sessions[&sa]
            .relay
            .as_ref()
            .unwrap()
            .get_ref()
            .0
            .borrow()
            .local_addr();
        r.setup_done(sa, key, result, now).unwrap();
        assert_eq!(
            r.budgets.relay_fds.used().count,
            1,
            "duplicate callback cannot replace socket"
        );
        r.sessions.get_mut(&sa).unwrap().state = State::Active;
        r.close_session(sb, 0x100);
        r.expire_sessions(now + Duration::from_secs(119));
        assert!(r.sessions.contains_key(&sa));
        r.sessions
            .get_mut(&sa)
            .unwrap()
            .udp_activity(now + Duration::from_secs(119));
        r.expire_sessions(now + Duration::from_secs(238));
        assert!(
            r.sessions.contains_key(&sa),
            "sparse valid UDP preserves association"
        );
        // A transport probe does not call udp_activity.
        r.service_transport_timers(now + Duration::from_secs(239))
            .unwrap();
        r.expire_sessions(now + Duration::from_secs(239));
        assert!(r.sessions.is_empty());
        assert_eq!(r.budgets.sessions.used().count, 0);
        assert_eq!(r.budgets.setup.used().count, 0);
        assert_eq!(r.budgets.relay_fds.used().count, 0);
        assert_eq!(r.budgets.fds.used().count, 0);
        assert_eq!(r.identities[0].used().count, 0);
        let _rebound = std::net::UdpSocket::bind(relay).unwrap();
        r.setup_done(sa, key, result, now).unwrap();
        assert!(r.sessions.is_empty());
    });
}
#[test]
fn closed_control_keeps_identity_and_global_credit_until_native_collection() {
    runtime().block_on(async {
        let f = Fixtures::new();
        let path = f.file("server.toml");
        std::fs::write(
            &path,
            std::fs::read_to_string(&path).unwrap() + "\n[limits]\nmax_sessions_per_identity = 1\n",
        )
        .unwrap();
        let cfg = ValidatedConfig::load(&path).unwrap();
        let resolver = SystemResolver::new(1).unwrap();
        let (diag, _rx) =
            Diagnostics::new(Role::Server, Instant::now(), Duration::from_secs(1)).unwrap();
        let mut r = Runner::new(&cfg, &resolver, diag).unwrap();
        let mut pair = Pair::new(true);
        let (a, b) = pair.requests();
        install(&mut r, pair.server);
        let sa = r.create_session(2, a, None, Instant::now()).unwrap();
        r.bind_identity(sa, 0).unwrap();
        r.close_session(sa, 0x100);
        r.close_session(sa, 0x100);
        assert_eq!(r.budgets.sessions.used().count, 1);
        assert_eq!(r.identities[0].used().count, 1);
        assert_eq!(r.connections[&2].transport.cleanup_count(), 1);
        assert_eq!(r.budgets.setup.used().count, 0);
        let sb = r.create_session(2, b, None, Instant::now()).unwrap();
        assert!(r.bind_identity(sb, 0).is_err(), "cleanup is still charged");
        pair.server = r.connections.remove(&2).unwrap().transport;
        pair.client.close_stream(a, 0x100);
        let deadline = Instant::now() + Duration::from_secs(2);
        while pair.server.cleanup_count() != 0 {
            assert!(
                Instant::now() < deadline,
                "native cleanup did not return credit"
            );
            pair.pump();
            pair.client.reap_stopped_streams();
            pair.server.reap_stopped_streams();
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(r.identities[0].used().count, 0);
        assert_eq!(
            r.budgets.sessions.used().count,
            1,
            "only the pending second flow remains"
        );
        r.close_session(sb, 0x100);
        assert_eq!(r.budgets.sessions.used().count, 0);
    });
}
#[test]
fn five_continuously_pending_bodies_and_datagram_queues_make_bounded_progress() {
    runtime().block_on(async {
        let f = Fixtures::new();
        let cfg = ValidatedConfig::load(&f.file("server.toml")).unwrap();
        let resolver = SystemResolver::new(1).unwrap();
        let (diag, _rx) =
            Diagnostics::new(Role::Server, Instant::now(), Duration::from_secs(1)).unwrap();
        let mut r = Runner::new(&cfg, &resolver, diag).unwrap();
        let mut pair = Pair::new(true);
        pair.negotiate().unwrap();
        let token = std::fs::read_to_string(pair.fixture.file("client.token")).unwrap();
        let h = masque::request_headers(
            "remora.test",
            "/.well-known/masque/udp/127.0.0.1/53/",
            token.trim().as_bytes().try_into().unwrap(),
        );
        let mut streams = Vec::new();
        for _ in 0..5 {
            streams.push(
                pair.client
                    .h3
                    .as_mut()
                    .unwrap()
                    .send_request(&mut pair.client.quic, &h, false)
                    .unwrap(),
            );
        }
        for _ in 0..30 {
            pair.pump();
            while pair
                .server
                .h3
                .as_mut()
                .unwrap()
                .poll(&mut pair.server.quic)
                .is_ok()
            {}
        }
        // Unknown capsule has a long valid body: no payload forwarding or allocation.
        let mut body = vec![0; 4096];
        body[0] = 0x21;
        body[1] = 0x80;
        body[2] = 0x01;
        body[3] = 0x00;
        body[4] = 0x00;
        for stream in &streams {
            let mut offset = 0;
            let deadline = Instant::now() + Duration::from_secs(2);
            while offset < body.len() {
                assert!(Instant::now() < deadline);
                match pair.client.h3.as_mut().unwrap().send_body(
                    &mut pair.client.quic,
                    *stream,
                    &body[offset..],
                    false,
                ) {
                    Ok(n) => offset += n,
                    Err(quiche::h3::Error::Done) => {}
                    Err(e) => panic!("body {e:?}"),
                }
                pair.pump();
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        for _ in 0..30 {
            pair.pump();
            while pair
                .server
                .h3
                .as_mut()
                .unwrap()
                .poll(&mut pair.server.quic)
                .is_ok()
            {}
        }
        install(&mut r, pair.server);
        let now = Instant::now();
        let mut ids = Vec::new();
        for stream in streams {
            let id = r.create_session(2, stream, None, now).unwrap();
            let s = r.sessions.get_mut(&id).unwrap();
            s.body_pending = true;
            s.state = State::Active;
            s._setup = None;
            ids.push(id);
            for _ in 0..4 {
                r.queue_outgoing(id, b"bounded", now).unwrap();
            }
        }
        for _ in 0..2 {
            r.h3_work(2, now).unwrap();
            r.schedule_datagrams(2, now).unwrap();
            r.service_transport_timers(now).unwrap();
        }
        for id in &ids {
            let stream = r.sessions[id].stream;
            let mut unread = [0; 8192];
            let (remaining, _) = r
                .connections
                .get_mut(&2)
                .unwrap()
                .transport
                .quic
                .stream_recv(stream, &mut unread)
                .unwrap();
            assert!(
                remaining < 4096,
                "all five continuously pending bodies must be consumed within two owner visits"
            );
        }
        for id in &ids {
            assert!(
                r.sessions[id].queue.used().count < 4,
                "every native producer gets a visit"
            );
            assert_eq!(r.sessions[id].capsules.retained_bytes(), 0);
        }
        // With the first four still pending, two visits also consume the fifth.
        // Exhaust buffered bodies then observe Done on every stream within a bounded cycle.
        for _ in 0..12 {
            r.h3_work(2, now).unwrap();
        }
        assert!(ids.iter().all(|id| !r.sessions[id].body_pending));
        r.close_session(ids[0], 0x100);
        assert_eq!(r.sessions.len(), 4);
        assert!(!r.connections[&2].transport.is_stopped());
        assert!(
            ids[1..]
                .iter()
                .all(|id| r.sessions[id].state == State::Active)
        );
        // A peer STOP is delivered to the owner before H3 metadata reclamation;
        // it cancels only the affected admitted association.
        let stopped = r.sessions[&ids[1]].stream;
        pair.client
            .quic
            .stream_shutdown(stopped, quiche::Shutdown::Read, 0x10c)
            .unwrap();
        let mut bytes = [0; 1350];
        while let Ok((n, info)) = pair.client.quic.send(&mut bytes) {
            r.connections
                .get_mut(&2)
                .unwrap()
                .transport
                .receive(
                    &mut bytes[..n],
                    quiche::RecvInfo {
                        from: info.from,
                        to: info.to,
                    },
                    Instant::now(),
                )
                .unwrap();
        }
        for _ in 0..8 {
            r.reclaim_control(2);
        }
        assert!(!r.sessions.contains_key(&ids[1]));
        assert!(
            ids[2..]
                .iter()
                .all(|id| r.sessions[id].state == State::Active)
        );
        assert!(!r.connections[&2].transport.is_stopped());
    });
}
proptest! {
    #[test]
    fn cyclic_selection_covers_every_live_generation_after_churn(ids in prop::collection::btree_set(1u64..10000, 1..129), cursor in any::<u64>()) {
        let ids: Vec<_> = ids.into_iter().collect();
        let mut cursor = FairCursor {last: Some(cursor), ceiling: ids.last().copied()};
        let mut visited = std::collections::BTreeSet::new();
        for _ in 0..ids.len().div_ceil(4) { visited.extend(round_robin(ids.clone(), &mut cursor, 4)); }
        prop_assert_eq!(visited, ids.into_iter().collect());
    }
}

#[test]
fn commitment_and_active_stop_checks_rotate_with_four_session_quotas() {
    runtime().block_on(async {
        let f = Fixtures::new();
        let cfg = ValidatedConfig::load(&f.file("server.toml")).unwrap();
        let resolver = SystemResolver::new(1).unwrap();
        let (diag, _rx) =
            Diagnostics::new(Role::Server, Instant::now(), Duration::from_secs(1)).unwrap();
        let mut r = Runner::new(&cfg, &resolver, diag).unwrap();
        let mut pair = Pair::new(true);
        let (a, b) = pair.requests();
        let headers = masque::request_headers(
            "remora.test",
            "/.well-known/masque/udp/127.0.0.1/53/",
            std::fs::read_to_string(pair.fixture.file("client.token"))
                .unwrap()
                .trim()
                .as_bytes()
                .try_into()
                .unwrap(),
        );
        let mut streams = vec![a, b];
        for _ in 2..9 {
            streams.push(
                pair.client
                    .h3
                    .as_mut()
                    .unwrap()
                    .send_request(&mut pair.client.quic, &headers, false)
                    .unwrap(),
            );
        }
        for _ in 0..20 {
            pair.pump();
            while pair
                .server
                .h3
                .as_mut()
                .unwrap()
                .poll(&mut pair.server.quic)
                .is_ok()
            {}
        }
        install(&mut r, pair.server);
        let now = Instant::now();
        for stream in &streams {
            let id = r.create_session(2, *stream, None, now).unwrap();
            let s = r.sessions.get_mut(&id).unwrap();
            s.state = State::AwaitingCommitment;
            // No tracked response buffer remains, as after ACK. The real native
            // stream must still be open before an association may become active.
            s.receipt = Some(std::sync::Weak::new());
        }
        for expected in [4, 8, 9] {
            assert_eq!(r.service_session_controls(2).unwrap(), expected < 9);
            assert_eq!(
                r.sessions
                    .values()
                    .filter(|s| s.state == State::Active)
                    .count(),
                expected
            );
            r.service_transport_timers(now).unwrap();
        }
        assert!(
            !r.service_session_controls(2).unwrap(),
            "ordinary live capacity is not continuation"
        );
        // Deliver peer STOPs, without polling H3 to consume the reset events.
        // The active-capacity class must retire no more than four per visit.
        for stream in streams {
            pair.client
                .quic
                .stream_shutdown(stream, quiche::Shutdown::Read, 0x10c)
                .unwrap();
        }
        let mut bytes = [0; 1350];
        while let Ok((n, info)) = pair.client.quic.send(&mut bytes) {
            r.connections
                .get_mut(&2)
                .unwrap()
                .transport
                .receive(
                    &mut bytes[..n],
                    quiche::RecvInfo {
                        from: info.from,
                        to: info.to,
                    },
                    now,
                )
                .unwrap();
        }
        for expected in [5, 1, 0] {
            assert_eq!(r.service_session_controls(2).unwrap(), expected > 0);
            assert_eq!(r.sessions.len(), expected);
            r.service_transport_timers(now).unwrap();
        }
        assert!(!r.connections[&2].transport.is_stopped());
    });
}

#[test]
fn critical_stop_cancels_only_its_connection_and_preserves_close_generation() {
    runtime().block_on(async {
        let f = Fixtures::new();
        let cfg = ValidatedConfig::load(&f.file("server.toml")).unwrap();
        let resolver = SystemResolver::new(1).unwrap();
        let (diag, _rx) =
            Diagnostics::new(Role::Server, Instant::now(), Duration::from_secs(1)).unwrap();
        let mut r = Runner::new(&cfg, &resolver, diag).unwrap();
        let mut pair = Pair::new(true);
        let (a, b) = pair.requests();
        install(&mut r, pair.server);
        let now = Instant::now();
        for stream in [a, b] {
            let id = r.create_session(2, stream, None, now).unwrap();
            r.sessions.get_mut(&id).unwrap().state = State::Active;
        }
        let mut other = Pair::new(true);
        let (other_stream, _) = other.requests();
        let permit = r.budgets.connections.reserve(1, 0).unwrap();
        let id = r.diagnostics.next_id().unwrap();
        r.connections.insert(
            99,
            OwnedConnection {
                transport: other.server,
                socket: 0,
                _permit: permit,
                _socket_permit: None,
                id,
                initial_cid: vec![],
                cursors: [FairCursor::default(); 7],
                receive_density: ReceiveDensity::default(),
                enqueue_pending: false,
                output_pending: false,
            },
        );
        let unaffected = r.create_session(99, other_stream, None, now).unwrap();
        r.sessions.get_mut(&unaffected).unwrap().state = State::Active;
        r.sessions.get_mut(&unaffected).unwrap()._setup = None;
        pair.client
            .quic
            .stream_shutdown(3, quiche::Shutdown::Read, 0x10c)
            .unwrap();
        let mut bytes = [0; 1350];
        while let Ok((n, info)) = pair.client.quic.send(&mut bytes) {
            r.connections
                .get_mut(&2)
                .unwrap()
                .transport
                .receive(
                    &mut bytes[..n],
                    quiche::RecvInfo {
                        from: info.from,
                        to: info.to,
                    },
                    now,
                )
                .unwrap();
        }
        r.reclaim_control(2);
        assert_eq!(r.sessions.len(), 1);
        assert!(r.sessions.contains_key(&unaffected));
        assert!(!r.connections[&99].transport.is_stopped());
        assert_eq!(r.sessions[&unaffected].state, State::Active);
        assert!(r.connections[&2].transport.critical_failed());
        assert_eq!(r.h3_work(2, now), Ok(false));
        let c = &mut r.connections.get_mut(&2).unwrap().transport;
        assert_eq!(c.quic.local_error().unwrap().error_code, 0x104);
        assert!(c.generate(&r.pools.ciphertext, now, 4).unwrap() > 0);
        // Another admitted connection continues actual tagged UDP delivery.
        let now = Instant::now();
        r.queue_outgoing(unaffected, b"unaffected", now).unwrap();
        r.schedule_datagrams(99, now).unwrap();
        let other_transport = &mut r.connections.get_mut(&99).unwrap().transport;
        assert!(
            other_transport
                .generate(&r.pools.ciphertext, now, 4)
                .unwrap()
                > 0
        );
        for packet in &other_transport.output {
            let mut data = packet.bytes().to_vec();
            other
                .client
                .receive(
                    &mut data,
                    quiche::RecvInfo {
                        from: packet.from(),
                        to: packet.to(),
                    },
                    now,
                )
                .unwrap();
        }
        let n = other.client.quic.dgram_recv(&mut bytes).unwrap();
        let datagram = masque::decode_datagram(&bytes[..n]).unwrap().unwrap();
        assert_eq!(datagram.stream, other_stream);
        assert_eq!(datagram.payload, b"unaffected");
    });
}
#[test]
fn continuous_new_generations_cannot_extend_a_round_forever_or_duplicate_a_visit() {
    let mut cursor = FairCursor::default();
    let mut last = [0usize; 5];
    for tick in 1..=64 {
        // Five persistent producers and four continually replaced new ones.
        let mut ids: Vec<_> = (1u64..=5).collect();
        ids.extend((10 + tick as u64 * 4)..(14 + tick as u64 * 4));
        let selected = round_robin(ids, &mut cursor, 4);
        let unique: std::collections::BTreeSet<_> = selected.iter().copied().collect();
        assert_eq!(unique.len(), selected.len());
        for id in selected.into_iter().filter(|id| *id <= 5) {
            last[id as usize - 1] = tick;
        }
        if tick >= 4 {
            assert!(
                last.iter().all(|previous| tick - previous < 4),
                "old producers must progress during continuous churn"
            );
        }
    }
    let mut cursor = FairCursor {
        last: Some(1),
        ceiling: Some(2),
    };
    let selected = round_robin(vec![1, 2, 3, 4], &mut cursor, 4);
    assert_eq!(
        selected
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        4
    );
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    #[test]
    fn owner_admission_and_credit_match_live_pending_active_state(events in prop::collection::vec((0u8..3, 0u8..8),1..100)) {
        runtime().block_on(async {
            let f=Fixtures::new();
            let path=f.file("client.toml");
            std::fs::write(&path,std::fs::read_to_string(&path).unwrap()+"\n[limits]\nmax_sessions = 4\nmax_sessions_per_identity = 4\nmax_sessions_per_connection = 4\nmax_pending_setup = 2\n").unwrap();
            let cfg=ValidatedConfig::load(&path).unwrap();
            let resolver=SystemResolver::new(2).unwrap();
            let (diag,_rx)=Diagnostics::new(Role::Client,Instant::now(),Duration::from_secs(1)).unwrap();
            let mut r=Runner::new(&cfg,&resolver,diag).unwrap();
            let connection=*r.connections.keys().next().unwrap();
            for (op,index) in events {
                let sender=SocketAddr::from(([127,0,0,1],10000+u16::from(index)));
                let local="127.0.0.1:50000".parse().unwrap();
                let id=r.senders.get(&(sender,local)).copied();
                match op {
                    0=> { let _=r.create_session(connection,u64::MAX,Some((sender,local)),Instant::now()); },
                    1=> if let Some(id)=id { let s=r.sessions.get_mut(&id).unwrap(); s.state=State::Active;s._setup=None; },
                    _=> if let Some(id)=id { r.close_session(id,0x100);r.close_session(id,0x100); },
                }
                assert_eq!(r.senders.len(),r.sessions.len());
                assert_eq!(r.budgets.sessions.used().count,r.sessions.len());
                assert_eq!(r.budgets.setup.used().count,r.sessions.values().filter(|s|s._setup.is_some()).count());
                assert!(r.sessions.len()<=4); assert!(r.budgets.setup.used().count<=2);
            }
            for id in r.sessions.keys().copied().collect::<Vec<_>>() { r.close_session(id,0x100); }
            assert_eq!(r.budgets.sessions.used().count,0);assert_eq!(r.budgets.setup.used().count,0);
        });
    }
}
#[test]
fn rejected_mapping_survives_sender_churn_and_fresh_connection_generation() {
    runtime().block_on(async {
        let f = Fixtures::new();
        let cfg = ValidatedConfig::load(&f.file("client.toml")).unwrap();
        let resolver = SystemResolver::new(1).unwrap();
        let (diag, _rx) =
            Diagnostics::new(Role::Client, Instant::now(), Duration::from_secs(1)).unwrap();
        let mut r = Runner::new(&cfg, &resolver, diag).unwrap();
        let mut pair = Pair::new(true);
        let (a, b) = pair.requests();
        let mut old = r.connections.drain().next().unwrap().1;
        old.transport = pair.client;
        let generation = old.transport.generation.0;
        r.connections.insert(generation, old);
        let now = Instant::now();
        let kept = r.create_session(generation, a, None, now).unwrap();
        let denied = r.create_session(generation, b, None, now).unwrap();
        r.sessions.get_mut(&kept).unwrap().state = State::Active;
        r.ever_active = true;
        r.headers(generation, b, masque::response_headers(403), false, now)
            .unwrap();
        assert!(!r.latch.may_setup());
        assert!(!r.sessions.contains_key(&denied));
        assert_eq!(r.sessions[&kept].state, State::Active);
        let listen = r
            .listener
            .as_ref()
            .unwrap()
            .get_ref()
            .0
            .borrow()
            .local_addr();
        for _ in 0..64 {
            let sender = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
            sender
                .send_to(b"new-source-cannot-bypass-latch", listen)
                .unwrap();
            r.client_input(now).unwrap();
        }
        assert_eq!(r.sessions.len(), 1);
        r.queue_outgoing(kept, b"accepted-unrelated", now).unwrap();
        r.schedule_datagrams(generation, now).unwrap();
        assert_eq!(
            r.connections[&generation]
                .transport
                .quic
                .dgram_send_queue_len(),
            1
        );
        r.close_session(kept, 0x100);
        r.connections.clear();
        let replacement = Pair::new(true);
        let permit = r.budgets.connections.reserve(1, 0).unwrap();
        let id = r.diagnostics.next_id().unwrap();
        let mut transport = replacement.client;
        transport.generation = crate::buffers::Generation(999);
        r.connections.insert(
            999,
            OwnedConnection {
                transport,
                socket: 0,
                _permit: permit,
                _socket_permit: None,
                id,
                initial_cid: vec![],
                cursors: [FairCursor::default(); 7],
                receive_density: ReceiveDensity::default(),
                enqueue_pending: false,
                output_pending: false,
            },
        );
        let sender = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        sender.send_to(b"after-reconnect", listen).unwrap();
        r.client_input(now).unwrap();
        assert!(r.sessions.is_empty());
        assert!(!r.latch.may_setup());
        assert_eq!(r.budgets.setup.used().count, 0);
    });
}

#[test]
fn native_backpressure_commits_only_actual_fair_enqueue_progress() {
    runtime().block_on(async {
        let f = Fixtures::new();
        let cfg = ValidatedConfig::load(&f.file("server.toml")).unwrap();
        let resolver = SystemResolver::new(1).unwrap();
        let (diag, _rx) =
            Diagnostics::new(Role::Server, Instant::now(), Duration::from_secs(1)).unwrap();
        let mut r = Runner::new(&cfg, &resolver, diag).unwrap();
        let mut pair = Pair::new(true);
        let (a, b) = pair.requests();
        install(&mut r, pair.server);
        let now = Instant::now();
        let mut ids = Vec::new();
        for stream in [a, b] {
            let id = r.create_session(2, stream, None, now).unwrap();
            r.sessions.get_mut(&id).unwrap().state = State::Active;
            for _ in 0..4 {
                r.queue_outgoing(id, &[7; 1200], now).unwrap();
            }
            ids.push(id);
        }
        let mut scratch = [0; 1350];
        for _ in 0..cfg.limits().quiche_dgram_packets - 1 {
            r.connections
                .get_mut(&2)
                .unwrap()
                .transport
                .enqueue(a, &[1; 1200], now, now, &mut scratch)
                .unwrap();
        }
        assert!(r.schedule_datagrams(2, now).unwrap());
        assert_eq!(r.sessions[&ids[0]].queue.used().count, 3);
        assert_eq!(r.sessions[&ids[1]].queue.used().count, 4);
        let committed = r.connections[&2].cursors[3];
        for _ in 0..8 {
            assert!(r.schedule_datagrams(2, now).unwrap());
        }
        assert_eq!(r.connections[&2].cursors[3].last, committed.last);
        assert_eq!(r.connections[&2].cursors[3].ceiling, committed.ceiling);
        assert_eq!(r.sessions[&ids[0]].queue.used().count, 3);
        assert_eq!(r.sessions[&ids[1]].queue.used().count, 4);
        // One 1200-byte DATAGRAM occupies one actual QUIC packet. Free one
        // native slot, leaving both producers continuously backlogged.
        let transport = &mut r.connections.get_mut(&2).unwrap().transport;
        assert_eq!(
            transport.generate(&pair.pool, Instant::now(), 1).unwrap(),
            1
        );
        assert_eq!(
            transport.quic.dgram_send_queue_len(),
            cfg.limits().quiche_dgram_packets - 1
        );
        assert!(r.schedule_datagrams(2, Instant::now()).unwrap());
        assert_eq!(r.sessions[&ids[0]].queue.used().count, 3);
        assert_eq!(
            r.sessions[&ids[1]].queue.used().count,
            3,
            "the next native slot must serve the previously unserved flow"
        );
    });
}

// Shared with the separate ASan/libFuzzer package; production has no new API.
#[path = "../../fuzz/support/lifecycle.rs"]
mod lifecycle_harness;
