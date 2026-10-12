use super::*;
use crate::common::{FakeClock, Fixtures, IoStep, ScriptedIo, quic::Pair};
use std::{net::UdpSocket, os::fd::AsRawFd};

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[test]
fn minimal_jitter_has_floor_bounded_frequency_and_capped_long_outages() {
    let mut clock = FakeClock::new();
    let mut retry = RetrySchedule::new(100, 2000);
    for _ in 0..1000 {
        retry.schedule(clock.now(), 0);
        let deadline = retry.deadline().unwrap();
        assert_eq!(
            deadline.duration_since(clock.now()),
            Duration::from_millis(100)
        );
        clock.advance(Duration::from_millis(99));
        assert!(!retry.due(clock.now()));
        clock.advance(Duration::from_millis(1));
        assert!(retry.due(clock.now()));
        retry.started();
        assert!(!retry.due(clock.now()));
    }
    retry.reset();
    for i in 0..1000 {
        let ceiling = retry.jitter_ceiling_ms();
        assert_eq!(ceiling, (100u64 * (1u64 << i.min(31))).min(1900));
        retry.schedule(clock.now(), u64::MAX);
        let delay = retry.deadline().unwrap().duration_since(clock.now());
        assert_eq!(delay, Duration::from_millis(100 + ceiling));
        assert!(delay <= Duration::from_secs(2));
        clock.advance(delay);
        retry.started();
    }
    retry.reset();
    assert_eq!(retry.jitter_ceiling_ms(), 100);
    assert_eq!(retry.deadline(), None);
}

#[test]
fn local_pto_probes_duplicates_and_unauthenticated_input_cannot_extend_watchdog() {
    let mut p = Pair::new(true);
    p.requests();
    let mut clock = FakeClock::new();
    let mut bytes = [0; 1350];
    p.server.quic.send_ack_eliciting().unwrap();
    let (n, info) = p.server.quic.send(&mut bytes).unwrap();
    let original = bytes[..n].to_vec();
    let recv = quiche::RecvInfo {
        from: info.from,
        to: info.to,
    };
    p.client
        .receive(&mut bytes[..n], recv, clock.now())
        .unwrap();
    let began = clock.now();
    // Local probes/PTO generation are real QUIC output, accepted by scripted
    // IO, but intentionally never delivered to a peer during the blackhole.
    for _ in 0..4 {
        clock.advance(Duration::from_secs(2));
        assert_eq!(
            p.client.service_liveness(clock.now(), true).unwrap(),
            transport::LivenessAction::Probe
        );
        p.client.quic.on_timeout();
        p.client.generate(&p.pool, clock.now(), 4).unwrap();
        let mut io = ScriptedIo::new([IoStep::Accepted(p.client.output.len())]);
        io.send(p.client.output.len()).unwrap();
        p.client.output.clear();
        let mut duplicate = original.clone();
        let received = p.client.quic.stats().recv;
        let _ = p.client.receive(&mut duplicate, recv, clock.now());
        assert_eq!(p.client.quic.stats().recv, received);
        let _ = p.client.receive(&mut [0; 64], recv, clock.now());
    }
    assert_eq!(clock.now().duration_since(began), Duration::from_secs(8));
    clock.advance(Duration::from_secs(2));
    assert_eq!(
        p.client.service_liveness(clock.now(), true).unwrap(),
        transport::LivenessAction::Expired
    );
    assert_eq!(
        p.client.service_liveness(clock.now(), false).unwrap(),
        transport::LivenessAction::None
    );
}

#[test]
fn connection_pressure_keeps_draining_flows_and_cancels_old_setup_callbacks() {
    use std::{cell::Cell, rc::Rc};
    runtime().block_on(async {
        let f = Fixtures::new();
        let file = f.file("client.toml");
        std::fs::write(
            &file,
            std::fs::read_to_string(&file).unwrap() + "\n[limits]\nmax_connections=1\n",
        )
        .unwrap();
        let cfg = ValidatedConfig::load(&file).unwrap();
        let resolver = SystemResolver::new(1).unwrap();
        let now = Instant::now();
        let (diag, _rx) = Diagnostics::new(Role::Client, now, Duration::from_secs(1)).unwrap();
        let mut r = Runner::new(&cfg, &resolver, diag).unwrap();
        let old = r.primary.unwrap();
        let mut p = Pair::new(true);
        let (a, b) = p.requests();
        r.connections.get_mut(&old).unwrap().transport = p.client;
        let local = r
            .listener
            .as_ref()
            .unwrap()
            .get_ref()
            .0
            .borrow()
            .local_addr();
        let sid = r
            .create_session(
                old,
                a,
                Some(("127.0.0.1:42001".parse().unwrap(), local)),
                now,
            )
            .unwrap();
        r.sessions.get_mut(&sid).unwrap().state = State::Active;
        let key = SetupKey {
            connection: crate::buffers::Generation(old),
            session: crate::buffers::Generation(sid),
        };
        struct Cancel(Rc<Cell<bool>>);
        impl Drop for Cancel {
            fn drop(&mut self) {
                self.0.set(true);
            }
        }
        let cancelled = Rc::new(Cell::new(false));
        let guard = Cancel(cancelled.clone());
        r.pending.push(Pending {
            session: sid,
            key,
            future: Box::pin(async move {
                let _guard = guard;
                std::future::pending().await
            }),
        });
        r.connections
            .get_mut(&old)
            .unwrap()
            .transport
            .goaway(b)
            .unwrap();
        r.primary = None;
        r.reconnect.schedule(now, 0);
        assert!(
            r.service_recovery(now + Duration::from_millis(100))
                .unwrap()
        );
        assert_eq!(r.primary, None);
        assert_eq!(r.connections.len(), 1);
        assert_eq!(r.budgets.connections.used().count, 1);
        assert_eq!(r.budgets.fds.used().count, 1);
        assert_eq!(r.sessions[&sid].state, State::Active);
        assert!(!cancelled.get());
        r.remove_connection(old, now).unwrap();
        assert!(cancelled.get());
        assert!(r.pending.is_empty());
        r.setup_done(
            sid,
            key,
            Ok((
                0,
                policy::ResolvedTarget {
                    address: "127.0.0.1:53".parse().unwrap(),
                    key,
                },
            )),
            now,
        )
        .unwrap();
        assert_eq!(r.budgets.relay_fds.used().count, 0);
        let deadline = r.reconnect.deadline().unwrap();
        r.service_recovery(deadline).unwrap();
        assert!(r.primary.is_some_and(|new| new > old));
        assert_eq!(r.connections.len(), 1);
        assert_eq!(r.sessions.len(), 0);
        r.stop_owned(deadline).unwrap();
        assert!(
            !r.service_recovery(deadline + Duration::from_secs(10))
                .unwrap()
        );
    });
}

#[test]
fn validated_native_rebinding_keeps_original_generation_and_does_not_schedule_recovery() {
    runtime().block_on(async {
        let mut p = Pair::new(true);
        p.requests();
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
        let changed = "127.0.0.1:40002".parse().unwrap();
        let server = "127.0.0.1:40001".parse().unwrap();
        p.client.quic.probe_path(changed, server).unwrap();
        for _ in 0..20 {
            p.pump();
        }
        assert_eq!(p.server.quic.is_path_validated(server, changed), Ok(true));
        p.client.quic.migrate_source(changed).unwrap();
        p.client.quic.send_ack_eliciting().unwrap();
        for _ in 0..10 {
            p.pump();
        }
        assert_eq!(p.client.generation.0, 1);
        assert_eq!(p.server.generation.0, 2);
        let f = Fixtures::new();
        let cfg = ValidatedConfig::load(&f.file("client.toml")).unwrap();
        let resolver = SystemResolver::new(1).unwrap();
        let now = Instant::now();
        let (diag, _rx) = Diagnostics::new(Role::Client, now, Duration::from_secs(1)).unwrap();
        let mut r = Runner::new(&cfg, &resolver, diag).unwrap();
        let id = r.primary.unwrap();
        r.connections.get_mut(&id).unwrap().transport = p.client;
        r.service_transport_timers(now).unwrap();
        assert!(!r.service_recovery(now + Duration::from_secs(1)).unwrap());
        assert_eq!(r.primary, Some(id));
        assert_eq!(r.snapshot.recovery.connection_attempts, 1);
        assert_eq!(r.reconnect.deadline(), None);
        assert_eq!(
            r.connections
                .get_mut(&id)
                .unwrap()
                .transport
                .quic
                .path_event_next(),
            None
        );
    });
}

#[test]
fn replacement_discards_old_generations_preserves_listener_and_has_one_attempt() {
    runtime().block_on(async {
        let f = Fixtures::new();
        let cfg = ValidatedConfig::load(&f.file("client.toml")).unwrap();
        let resolver = SystemResolver::new(1).unwrap();
        let mut clock = FakeClock::new();
        let (diag, _rx) =
            Diagnostics::new(Role::Client, clock.now(), Duration::from_secs(1)).unwrap();
        let mut r = Runner::new(&cfg, &resolver, diag).unwrap();
        let old = r.primary.unwrap();
        let fd = r.listener.as_ref().unwrap().get_ref().as_raw_fd();
        let address = r
            .listener
            .as_ref()
            .unwrap()
            .get_ref()
            .0
            .borrow()
            .local_addr();
        let mut p = Pair::new(true);
        let (stream, _) = p.requests();
        let sid = r
            .create_session(
                old,
                stream,
                Some(("127.0.0.1:42001".parse().unwrap(), address)),
                clock.now(),
            )
            .unwrap();
        r.sessions.get_mut(&sid).unwrap().state = State::Active;
        p.client
            .enqueue(
                stream,
                b"old-uncommitted",
                clock.now(),
                clock.now(),
                &mut [0; 1350],
            )
            .unwrap();
        p.client
            .generate(&r.pools.ciphertext, clock.now(), 4)
            .unwrap();
        assert!(!p.client.output.is_empty());
        p.client
            .enqueue(
                stream,
                b"old-pending",
                clock.now(),
                clock.now(),
                &mut [0; 1350],
            )
            .unwrap();
        r.connections.get_mut(&old).unwrap().transport = p.client;
        r.remove_connection(old, clock.now()).unwrap();
        assert!(r.connections.is_empty() && r.sessions.is_empty() && r.senders.is_empty());
        assert_eq!(r.budgets.sessions.used().count, 0);
        assert_eq!(r.budgets.connections.used().count, 0);
        assert_eq!(r.budgets.fds.used().count, 0);
        assert!(r.snapshot.recovery.discarded_ciphertext > 0);
        assert!(r.snapshot.counters.local_drops.recovering.to_target > 0);
        r.reconnect.schedule(clock.now(), 0);
        clock.advance(Duration::from_millis(99));
        assert!(!r.service_recovery(clock.now()).unwrap());
        clock.advance(Duration::from_millis(1));
        assert!(r.service_recovery(clock.now()).unwrap());
        let new = r.primary.unwrap();
        assert!(new > old);
        assert_eq!(r.connections.len(), 1);
        assert_eq!(r.budgets.fds.used().count, 1);
        assert!(
            !r.service_recovery(clock.now() + Duration::from_secs(10))
                .unwrap()
        );
        assert_eq!(r.snapshot.recovery.connection_attempts, 2);
        assert_eq!(r.listener.as_ref().unwrap().get_ref().as_raw_fd(), fd);
        assert_eq!(
            r.listener
                .as_ref()
                .unwrap()
                .get_ref()
                .0
                .borrow()
                .local_addr(),
            address
        );
        assert!(r.connections[&new].transport.output.is_empty());
        assert_eq!(r.connections[&new].transport.quic.dgram_send_queue_len(), 0);
        assert!(r.connections[&new].transport.quic.peer_cert().is_none());
        r.stop_owned(clock.now()).unwrap();
        assert_eq!(
            r.snapshot.recovery.client_state,
            Some(ClientState::Stopping)
        );
        assert_eq!(r.reconnect.deadline(), None);
        assert!(
            !r.service_recovery(clock.now() + Duration::from_secs(100))
                .unwrap()
        );
        assert!(
            r.connections
                .values()
                .all(|c| c.transport.output.is_empty())
        );
        assert!(!r.relay_input(sid, clock.now()).unwrap());
    });
}

#[test]
fn permanent_mapping_rejection_preserves_accepted_flows_and_blocks_new_senders() {
    runtime().block_on(async {
        let f = Fixtures::new();
        let cfg = ValidatedConfig::load(&f.file("client.toml")).unwrap();
        let resolver = SystemResolver::new(1).unwrap();
        let now = Instant::now();
        let (diag, _rx) = Diagnostics::new(Role::Client, now, Duration::from_secs(1)).unwrap();
        let mut r = Runner::new(&cfg, &resolver, diag).unwrap();
        let mut p = Pair::new(true);
        let (a, b) = p.requests();
        let id = r.primary.unwrap();
        r.connections.get_mut(&id).unwrap().transport = p.client;
        let local = r
            .listener
            .as_ref()
            .unwrap()
            .get_ref()
            .0
            .borrow()
            .local_addr();
        let accepted = r
            .create_session(
                id,
                a,
                Some(("127.0.0.1:42001".parse().unwrap(), local)),
                now,
            )
            .unwrap();
        let rejected = r
            .create_session(
                id,
                b,
                Some(("127.0.0.1:42002".parse().unwrap(), local)),
                now,
            )
            .unwrap();
        r.sessions.get_mut(&accepted).unwrap().state = State::Active;
        r.sessions.get_mut(&rejected).unwrap().state = State::AwaitingAcceptance;
        r.ever_active = true;
        r.headers(id, b, masque::response_headers(403), false, now)
            .unwrap();
        assert!(!r.sessions.contains_key(&rejected));
        assert_eq!(r.sessions[&accepted].state, State::Active);
        assert_eq!(
            r.snapshot.recovery.client_state,
            Some(ClientState::Rejected)
        );
        assert!(!r.latch.may_setup());
        let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
        sender.send_to(b"denied", local).unwrap();
        r.client_input(now).unwrap();
        assert_eq!(r.snapshot.counters.local_drops.rejected.to_target, 1);
        assert_eq!(r.sessions.len(), 1);
        r.close_session(accepted, 0x100);
        r.remove_connection(id, now).unwrap();
        assert_eq!(r.reconnect.deadline(), None);
        assert!(!r.service_recovery(now + Duration::from_secs(100)).unwrap());
        sender.send_to(b"still-denied", local).unwrap();
        r.client_input(now).unwrap();
        assert!(r.sessions.is_empty());
        assert_eq!(r.snapshot.counters.local_drops.rejected.to_target, 2);
    });
}

#[test]
fn permanent_rejection_cancels_queued_and_awaiting_setup_but_keeps_accepted_traffic() {
    runtime().block_on(async {
        for code in [401, 403, 501] {
            let f = Fixtures::new();
            let cfg = ValidatedConfig::load(&f.file("client.toml")).unwrap();
            let resolver = SystemResolver::new(1).unwrap();
            let now = Instant::now();
            let (diag, _rx) = Diagnostics::new(Role::Client, now, Duration::from_secs(1)).unwrap();
            let mut r = Runner::new(&cfg, &resolver, diag).unwrap();
            let mut p = Pair::new(true);
            let (a, b) = p.requests();
            let token = std::fs::read_to_string(p.fixture.file("client.token")).unwrap();
            let headers = masque::request_headers(
                "remora.test",
                "/.well-known/masque/udp/127.0.0.1/53/",
                token.trim().as_bytes().try_into().unwrap(),
            );
            let c = p
                .client
                .h3
                .as_mut()
                .unwrap()
                .send_request(&mut p.client.quic, &headers, false)
                .unwrap();
            for _ in 0..20 {
                p.pump();
                while p
                    .server
                    .h3
                    .as_mut()
                    .unwrap()
                    .poll(&mut p.server.quic)
                    .is_ok()
                {}
            }
            let connection = r.primary.unwrap();
            r.connections.get_mut(&connection).unwrap().transport = p.client;
            let local = r
                .listener
                .as_ref()
                .unwrap()
                .get_ref()
                .0
                .borrow()
                .local_addr();
            let senders: Vec<_> = (0..4)
                .map(|_| UdpSocket::bind("127.0.0.1:0").unwrap())
                .collect();
            let streams = [a, b, c, u64::MAX];
            let ids: Vec<_> = streams
                .iter()
                .zip(&senders)
                .map(|(&stream, sender)| {
                    r.create_session(
                        connection,
                        stream,
                        Some((sender.local_addr().unwrap(), local)),
                        now,
                    )
                    .unwrap()
                })
                .collect();
            for id in &ids[..3] {
                r.sessions.get_mut(id).unwrap().state = State::AwaitingAcceptance;
            }
            r.headers(connection, a, masque::response_headers(200), true, now)
                .unwrap();
            assert!(r.ever_active);
            assert_eq!(r.budgets.setup.used().count, 3);
            // A valid response for C is already queued at the peer, but withheld
            // until B's permanent refusal latches the mapping.
            p.server
                .h3
                .as_mut()
                .unwrap()
                .send_response(&mut p.server.quic, c, &masque::response_headers(200), false)
                .unwrap();
            r.reconnect.schedule(now, 0);
            r.setup_retry.schedule(now, 0);
            r.headers(connection, b, masque::response_headers(code), false, now)
                .unwrap();
            assert_eq!(
                r.sessions.len(),
                1,
                "permanent refusal left pending setup alive"
            );
            assert_eq!(r.sessions[&ids[0]].state, State::Active);
            assert_eq!(r.budgets.setup.used().count, 0);
            assert_eq!(r.senders.len(), 1);
            assert_eq!(r.reconnect.deadline(), None);
            assert_eq!(r.setup_retry.deadline(), None);
            r.headers(connection, c, masque::response_headers(200), true, now)
                .unwrap();
            for (i, sender) in senders.iter().enumerate() {
                sender
                    .send_to(if i == 0 { b"accepted" } else { b"denied" }, local)
                    .unwrap();
            }
            r.client_input(now).unwrap();
            assert_eq!(r.snapshot.counters.local_drops.rejected.to_target, 3);
            assert_eq!(r.sessions[&ids[0]].queue.used().count, 1);
            r.schedule_datagrams(connection, now).unwrap();
            // Exercise real encrypted QUIC/H3 after cancellation, including
            // the withheld response. The peer must see no new CONNECT and only
            // A's datagram; no pending association can become Active or forward.
            for _ in 0..20 {
                for direction in 0..2 {
                    let client = &mut r.connections.get_mut(&connection).unwrap().transport;
                    let (from, to) = if direction == 0 {
                        (client, &mut p.server)
                    } else {
                        (&mut p.server, client)
                    };
                    let mut bytes = [0; 1350];
                    for _ in 0..32 {
                        match from.quic.send(&mut bytes) {
                            Ok((n, info)) => {
                                let result = to.receive(
                                    &mut bytes[..n],
                                    quiche::RecvInfo {
                                        from: info.from,
                                        to: info.to,
                                    },
                                    Instant::now(),
                                );
                                assert!(
                                    matches!(result, Ok(_) | Err(quiche::Error::Done)),
                                    "{result:?}"
                                );
                            }
                            Err(quiche::Error::Done) => break,
                            Err(error) => panic!("{error:?}"),
                        }
                    }
                }
                r.h3_work(connection, now).unwrap();
                while let Ok((stream, event)) =
                    p.server.h3.as_mut().unwrap().poll(&mut p.server.quic)
                {
                    assert!(
                        !matches!(event, quiche::h3::Event::Headers { .. }),
                        "new CONNECT after permanent rejection on {stream}"
                    );
                }
            }
            assert_eq!(r.sessions.len(), 1);
            assert_eq!(
                r.snapshot.recovery.client_state,
                Some(ClientState::Rejected)
            );
            let mut bytes = [0; 1350];
            let n = p.server.quic.dgram_recv(&mut bytes).unwrap();
            let datagram = masque::decode_datagram(&bytes[..n]).unwrap().unwrap();
            assert_eq!(datagram.stream, a);
            assert_eq!(datagram.payload, b"accepted");
            assert!(matches!(
                p.server.quic.dgram_recv(&mut bytes),
                Err(quiche::Error::Done)
            ));
            // Fault-inject stale owner setup work to exercise the two guards
            // independently of eager cancellation. It must neither open a
            // request nor accept a late successful response after the latch.
            let stale = r
                .create_session(
                    connection,
                    u64::MAX,
                    Some((senders[3].local_addr().unwrap(), local)),
                    now,
                )
                .unwrap();
            r.h3_work(connection, now).unwrap();
            assert_eq!(r.sessions[&stale].state, State::Starting);
            r.sessions.get_mut(&stale).unwrap().stream = c;
            r.sessions.get_mut(&stale).unwrap().state = State::AwaitingAcceptance;
            r.headers(connection, c, masque::response_headers(200), true, now)
                .unwrap();
            assert!(!r.sessions.contains_key(&stale));
            assert_eq!(r.sessions.len(), 1);
            assert_eq!(r.budgets.setup.used().count, 0);
        }
    });
}

#[test]
fn wire_goaway_keeps_lower_flow_and_places_new_request_on_fresh_verified_connection() {
    runtime().block_on(async {
        let f = Fixtures::new();
        let cfg = ValidatedConfig::load(&f.file("client.toml")).unwrap();
        let server_cfg = ValidatedConfig::load(&f.file("server.toml")).unwrap();
        let resolver = SystemResolver::new(1).unwrap();
        let now = Instant::now();
        let (diag, _rx) = Diagnostics::new(Role::Client, now, Duration::from_secs(1)).unwrap();
        let mut r = Runner::new(&cfg, &resolver, diag).unwrap();
        let mut p = Pair::new(true);
        let (a, b) = p.requests();
        let old = r.primary.unwrap();
        r.connections.get_mut(&old).unwrap().transport = p.client;
        let local = r
            .listener
            .as_ref()
            .unwrap()
            .get_ref()
            .0
            .borrow()
            .local_addr();
        let lower = r
            .create_session(
                old,
                a,
                Some(("127.0.0.1:42001".parse().unwrap(), local)),
                now,
            )
            .unwrap();
        let excluded = r
            .create_session(
                old,
                b,
                Some(("127.0.0.1:42002".parse().unwrap(), local)),
                now,
            )
            .unwrap();
        r.sessions.get_mut(&lower).unwrap().state = State::Active;
        r.sessions.get_mut(&excluded).unwrap().state = State::AwaitingAcceptance;
        r.ever_active = true;
        p.server
            .h3
            .as_mut()
            .unwrap()
            .send_goaway(&mut p.server.quic, b)
            .unwrap();
        let mut bytes = [0; 1350];
        while let Ok((n, info)) = p.server.quic.send(&mut bytes) {
            r.connections
                .get_mut(&old)
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
        r.h3_work(old, now).unwrap();
        assert_eq!(r.connections[&old].transport.draining, Some(b));
        assert_eq!(r.sessions[&lower].connection.0, old);
        assert_eq!(r.sessions[&lower].state, State::Active);
        assert!(!r.sessions.contains_key(&excluded));
        assert_eq!(r.primary, None);
        r.reconnect.schedule(now, 0);
        r.service_recovery(now + Duration::from_millis(100))
            .unwrap();
        let fresh = r.primary.unwrap();
        assert_ne!(fresh, old);
        assert_eq!(r.connections.len(), 2);
        assert_eq!(r.budgets.connections.used().count, 2);
        assert_eq!(r.budgets.fds.used().count, 2);
        let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
        sender.send_to(b"must-not-be-replayed", local).unwrap();
        r.client_input(Instant::now()).unwrap();
        let fresh_session = *r
            .senders
            .get(&(sender.local_addr().unwrap(), local))
            .unwrap();
        assert_eq!(r.sessions[&fresh_session].connection.0, fresh);
        assert_eq!(r.sessions[&fresh_session].state, State::Starting);
        assert!(!r.connections[&fresh].transport.is_ready());
        r.h3_work(fresh, Instant::now()).unwrap();
        assert_eq!(r.sessions[&fresh_session].stream, u64::MAX);
        r.setup_retry.schedule(Instant::now(), 100);
        let RoleConfig::Client(client) = cfg.role() else {
            unreachable!()
        };
        let (n, info) = r
            .connections
            .get_mut(&fresh)
            .unwrap()
            .transport
            .quic
            .send(&mut bytes)
            .unwrap();
        let header = quiche::Header::from_slice(&mut bytes[..n], 16).unwrap();
        let mut qcfg = transport::config(&server_cfg).unwrap();
        let mut q = quiche::accept_with_buf_factory::<TrackedFactory>(
            &quiche::ConnectionId::from_ref(&[9; 16]),
            Some(&header.dcid),
            info.to,
            info.from,
            &mut qcfg,
        )
        .unwrap();
        q.recv(
            &mut bytes[..n],
            quiche::RecvInfo {
                from: info.from,
                to: info.to,
            },
        )
        .unwrap();
        let mut peer = Connection::new(
            q,
            crate::buffers::Generation(999),
            true,
            &server_cfg,
            Instant::now(),
        );
        let mut renewed = false;
        let mut pending_headers = Vec::new();
        for _ in 0..200 {
            for direction in 0..2 {
                let (from, to) = if direction == 0 {
                    (
                        &mut r.connections.get_mut(&fresh).unwrap().transport,
                        &mut peer,
                    )
                } else {
                    (
                        &mut peer,
                        &mut r.connections.get_mut(&fresh).unwrap().transport,
                    )
                };
                while let Ok((n, info)) = from.quic.send(&mut bytes) {
                    let result = to.receive(
                        &mut bytes[..n],
                        quiche::RecvInfo {
                            from: info.from,
                            to: info.to,
                        },
                        Instant::now(),
                    );
                    assert!(matches!(result, Ok(_) | Err(quiche::Error::Done)), "replacement receive: {result:?}");
                }
            }
            peer.negotiate().unwrap();
            if let Some(h3) = &mut peer.h3 {
                while let Ok((stream, event)) = h3.poll(&mut peer.quic) {
                    if let quiche::h3::Event::Headers { list, .. } = event {
                        pending_headers.push((stream, list));
                    }
                }
            }
            if peer.negotiate().unwrap() {
                for (stream, list) in pending_headers.drain(..) {
                        let RoleConfig::Server(server) = server_cfg.role() else {
                            unreachable!()
                        };
                        let request =
                            masque::request(&list, &server.proxy_authority, &server.path_prefix)
                                .unwrap();
                        assert_eq!(
                            policy::authenticate(&server.identities, &request.token),
                            Ok(0)
                        );
                        assert_eq!(request.host, client.target_host);
                        assert_eq!(request.port, client.target_port);
                        peer.h3
                            .as_mut()
                            .unwrap()
                            .send_response(
                                &mut peer.quic,
                                stream,
                                &masque::response_headers(200),
                                false,
                            )
                            .unwrap();
                        renewed = true;
                }
            }
            r.h3_work(fresh, Instant::now()).unwrap();
            if r.sessions[&fresh_session].state == State::Active {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(renewed, "client established={} ready={} h3={} server established={} ready={} h3={} session={:?} client_error={:?} server_error={:?}", r.connections[&fresh].transport.quic.is_established(), r.connections[&fresh].transport.is_ready(),r.connections[&fresh].transport.h3.is_some(),peer.quic.is_established(),peer.is_ready(),peer.h3.is_some(),r.sessions[&fresh_session].state,r.connections[&fresh].transport.quic.local_error().map(|e|e.error_code),peer.quic.local_error().map(|e|e.error_code));
        assert_eq!(r.snapshot.recovery.client_state, Some(ClientState::Active));
        assert!(r.connections[&fresh].transport.quic.peer_cert().is_some());
        assert_eq!(r.sessions[&fresh_session].state, State::Active);
        assert_eq!(r.setup_retry.deadline(), None);
        assert_eq!(r.sessions[&lower].connection.0, old);
        assert_eq!(
            r.connections[&fresh].transport.quic.dgram_send_queue_len(),
            0
        );
        assert_eq!(r.snapshot.counters.quic_dgram_enqueued, 0);
        assert_eq!(r.sessions[&fresh_session].queue.used().count, 0);
    });
}
