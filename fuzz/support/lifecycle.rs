//! Test-only owner driver. Inputs choose transitions; expectations are kept
//! independently of MappingLatch and production generation predicates.
use super::*;
use crate::buffers::{Ciphertext, Datagram, Generation};
#[path = "native_pair.rs"]
mod native_pair;
use native_pair::Pair;

fn install(r: &mut Runner<'_>, transport: Connection) {
    let generation = transport.generation.0;
    let permit = r.budgets.connections.reserve(1, 0).unwrap();
    let id = r.diagnostics.next_id().unwrap();
    r.connections.insert(
        generation,
        OwnedConnection {
            transport,
            socket: 0,
            _permit: permit,
            _socket_permit: None,
            id,
            initial_cid: vec![],
            cursors: [FairCursor::default(); 7],
            commitment_remaining: None,
            receive_density: ReceiveDensity::default(),
            enqueue_pending: false,
            output_pending: false,
        },
    );
}
fn committed(r: &Runner<'_>, connection: u64) -> Vec<Vec<u8>> {
    r.connections[&connection]
        .transport
        .output
        .iter()
        .map(|p| p.bytes().to_vec())
        .collect()
}
fn late(r: &mut Runner<'_>, id: u64, key: SetupKey, now: Instant) {
    let before = (
        r.sessions.len(),
        r.budgets.relay_fds.used().count,
        r.identities.iter().map(|b| b.used().count).sum::<usize>(),
    );
    let state = r.sessions.get(&id).map(|s| s.state);
    // Success is deliberate: an error completion could pass without testing
    // whether stale success can allocate a relay or change the current session.
    r.setup_done(
        id,
        key,
        Ok((
            0,
            policy::ResolvedTarget {
                address: "127.0.0.1:9999".parse().unwrap(),
                key,
            },
        )),
        now,
    )
    .unwrap();
    assert_eq!(
        before,
        (
            r.sessions.len(),
            r.budgets.relay_fds.used().count,
            r.identities.iter().map(|b| b.used().count).sum::<usize>()
        )
    );
    assert_eq!(state, r.sessions.get(&id).map(|s| s.state));
}

/// Drive a bounded sequence against actual owner methods and native QUIC.
/// Configs must be isolated campaign fixtures; no external peer is contacted.
/// QUIC/TLS exchanges run in memory. Only admission checks use loopback UDP.
pub fn check(data: &[u8], client: &ValidatedConfig, server: &ValidatedConfig) {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let resolver = SystemResolver::new(1).unwrap();
            let (diag, _rx) =
                Diagnostics::new(Role::Server, Instant::now(), Duration::from_secs(1)).unwrap();
            let mut r = Runner::new(server, &resolver, diag).unwrap();
            let mut pair = Pair::new(client, server);
            let mut streams = pair.requests().into_iter();
            install(&mut r, pair.server);
            let mut connection = 2;
            let mut current = None;
            let mut history = Vec::new();
            // The prelude guarantees every campaign exercises each transition;
            // mutations then explore ordering, repetition and payload variants.
            for op in [0, 1, 9, 17, 2, 12, 3, 0, 5, 0, 2]
                .iter()
                .chain(data.iter().take(32))
            {
                let now = Instant::now();
                match op % 8 {
                    0 if current.is_none() => {
                        let Some(stream) = streams.next() else {
                            continue;
                        };
                        let id = r.create_session(connection, stream, None, now).unwrap();
                        current = Some(id);
                        history.push((
                            id,
                            SetupKey {
                                connection: Generation(connection),
                                session: Generation(id),
                            },
                        ));
                        r.sessions.get_mut(&id).unwrap().state = State::Active;
                    }
                    1 => {
                        if let Some(id) = current {
                            let mismatch = (op / 8) % 3;
                            let mut slab = r.pools.application.acquire().unwrap();
                            slab.bytes_mut()[..8].fill(*op);
                            let packet = Datagram::new(
                                slab,
                                0,
                                8,
                                Direction::ToClient,
                                Generation(if mismatch == 1 { 0 } else { id }),
                                Generation(if mismatch == 2 { 0 } else { connection }),
                                now,
                                now + Duration::from_secs(1),
                            )
                            .unwrap();
                            r.sessions.get_mut(&id).unwrap().queue.push(packet).unwrap();
                            let accepted = r.snapshot.counters.quic_dgram_enqueued;
                            let stale = r.snapshot.counters.local_drops.stale_generation.to_client;
                            r.schedule_datagrams(connection, now).unwrap();
                            assert_eq!(r.sessions[&id].queue.used().count, 0);
                            assert_eq!(
                                r.snapshot.counters.quic_dgram_enqueued,
                                accepted + u64::from(mismatch == 0)
                            );
                            assert_eq!(
                                r.snapshot.counters.local_drops.stale_generation.to_client,
                                stale + u64::from(mismatch != 0)
                            );
                        }
                    }
                    2 => {
                        if let Some(id) = current {
                            let s = r.sessions.get_mut(&id).unwrap();
                            // Ensure stale generation checks run with an otherwise
                            // eligible authorizing session, not an early state rejection.
                            s.state = State::Authorizing;
                            for key in [
                                SetupKey {
                                    connection: Generation(0),
                                    session: Generation(id),
                                },
                                SetupKey {
                                    connection: Generation(connection),
                                    session: Generation(0),
                                },
                            ] {
                                late(&mut r, id, key, now);
                            }
                            r.sessions.get_mut(&id).unwrap().state = State::Active;
                            late(
                                &mut r,
                                id,
                                SetupKey {
                                    connection: Generation(connection),
                                    session: Generation(id),
                                },
                                now,
                            );
                        }
                    }
                    3 => {
                        if let Some(id) = current.take() {
                            let output = committed(&r, connection);
                            r.queue_outgoing(id, b"cancel-before-commit", now).unwrap();
                            r.close_session(id, 0x100);
                            assert!(!r.sessions.contains_key(&id));
                            assert_eq!(
                                r.connections[&connection]
                                    .transport
                                    .quic
                                    .dgram_send_queue_len(),
                                0
                            );
                            assert_eq!(
                                committed(&r, connection),
                                output,
                                "close must preserve committed bytes"
                            );
                            let (_, key) = *history.last().unwrap();
                            late(&mut r, id, key, now);
                        }
                    }
                    4 => {
                        // Real native generation is supplemented with synthetic
                        // committed records when there was no application traffic.
                        r.connections
                            .get_mut(&connection)
                            .unwrap()
                            .transport
                            .generate(&r.pools.ciphertext, now, 4)
                            .unwrap();
                        if r.connections[&connection].transport.output.len() < 2 {
                            for sequence in
                                r.connections[&connection].transport.output.len() as u64..2
                            {
                                let mut slab = r.pools.ciphertext.acquire().unwrap();
                                slab.bytes_mut()[0] = sequence as u8;
                                let packet = Ciphertext::new(
                                    slab,
                                    1,
                                    "127.0.0.1:40001".parse().unwrap(),
                                    "127.0.0.1:40000".parse().unwrap(),
                                    now,
                                    Generation(connection),
                                    sequence,
                                    now,
                                    now + Duration::from_secs(10),
                                )
                                .unwrap();
                                r.connections
                                    .get_mut(&connection)
                                    .unwrap()
                                    .transport
                                    .output
                                    .push_back(packet);
                            }
                        }
                        let before = committed(&r, connection);
                        r.outer_send_hook =
                            Some(Box::new(|_| Err(io::ErrorKind::WouldBlock.into())));
                        r.flush_limit(connection, now + Duration::from_secs(1), 16)
                            .unwrap();
                        assert_eq!(
                            committed(&r, connection),
                            before,
                            "EAGAIN consumed committed output"
                        );
                        let accepted = usize::from(*op / 8) % (before.len().min(16) + 1);
                        let expected = before[..accepted].to_vec();
                        r.outer_send_hook = Some(Box::new(move |packets| {
                            assert_eq!(
                                packets
                                    .iter()
                                    .take(accepted)
                                    .map(|p| p.bytes.to_vec())
                                    .collect::<Vec<_>>(),
                                expected
                            );
                            Ok(accepted)
                        }));
                        r.flush_limit(connection, now + Duration::from_secs(1), 16)
                            .unwrap();
                        assert_eq!(committed(&r, connection), before[accepted..]);
                    }
                    5 => {
                        r.remove_connection(connection, now).unwrap();
                        assert!(r.sessions.is_empty());
                        current = None;
                        connection += 1;
                        let mut replacement = Pair::new(client, server);
                        streams = replacement.requests().into_iter();
                        replacement.server.generation = Generation(connection);
                        install(&mut r, replacement.server);
                        // Deliver old successful callbacks after replacement exists.
                        for &(id, key) in &history {
                            late(&mut r, id, key, now);
                        }
                    }
                    _ => {}
                }
                assert!(r.sessions.len() <= 1);
                assert!(r.budgets.setup.used().count <= 1);
            }
            drop(r);
            rejection(client, &resolver, data);
        });
}

fn rejection(client: &ValidatedConfig, resolver: &SystemResolver, data: &[u8]) {
    let (diag, _rx) =
        Diagnostics::new(Role::Client, Instant::now(), Duration::from_secs(1)).unwrap();
    let mut r = Runner::new(client, resolver, diag).unwrap();
    let now = Instant::now();
    r.permanent_client_failure(now, Failure::Authentication, 401)
        .unwrap();
    let rejected = true; // Oracle is independent of production MappingLatch.
    assert_eq!(r.latch.may_setup(), !rejected);
    let connection = r.primary.unwrap();
    r.remove_connection(connection, now).unwrap();
    let attempts = r.snapshot.recovery.connection_attempts;
    r.start_client_connection(now).unwrap();
    assert_eq!(r.snapshot.recovery.connection_attempts, attempts);
    assert!(r.connections.is_empty());
    let local = r
        .listener
        .as_ref()
        .unwrap()
        .get_ref()
        .0
        .borrow()
        .local_addr();
    for op in [0].iter().chain(data.iter().take(4)) {
        let sender = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let before = r.snapshot.counters.local_drops.rejected.to_target;
        sender.send_to(&[*op], local).unwrap();
        r.client_input(Instant::now()).unwrap();
        assert_eq!(
            r.snapshot.counters.local_drops.rejected.to_target,
            before + 1
        );
        assert!(r.sessions.is_empty());
        assert!(r.senders.is_empty());
        assert_eq!(r.budgets.setup.used().count, 0);
        assert_eq!(r.latch.may_setup(), !rejected);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn owner_transition_regressions() {
        let fixtures = crate::common::Fixtures::new();
        let client = crate::config::ValidatedConfig::load(&fixtures.file("client.toml")).unwrap();
        let server = crate::config::ValidatedConfig::load(&fixtures.file("server.toml")).unwrap();
        for data in [
            vec![],
            (0..=255).collect(),
            vec![0, 1, 12, 3, 0, 9, 2, 5, 0, 17, 2, 3],
        ] {
            super::check(&data, &client, &server);
        }
    }
}
