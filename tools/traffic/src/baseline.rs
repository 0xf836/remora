//! Test-only two-leg relay. Reuses reviewed Linux UDP I/O and configured batch
//! buffers. A fixed envelope carries only the baseline's session association.
use remora::{
    buffers::BufferPool,
    config::{IoMode, IoSettings},
    udp::{SendPacket, Udp},
};
use std::{
    collections::HashMap,
    io,
    net::SocketAddr,
    time::{Duration, Instant},
};
fn receive(
    socket: &mut Udp,
    pool: &BufferPool,
    max: usize,
) -> io::Result<Vec<(SocketAddr, Vec<u8>)>> {
    let mut slabs = Vec::new();
    for _ in 0..max {
        slabs.push(pool.acquire().map_err(|_| io::ErrorKind::OutOfMemory)?);
    }
    let mut meta = Vec::new();
    match socket.recv_batch(&mut slabs, &mut meta) {
        Ok(_) => Ok(meta
            .into_iter()
            .filter(|m| m.fault.is_none())
            .map(|m| (m.peer, slabs[m.index].bytes()[..m.length].to_vec()))
            .collect()),
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}
fn send(socket: &mut Udp, bytes: &[u8], to: SocketAddr) -> io::Result<bool> {
    let from = socket.local_addr();
    match socket.send_batch(&[SendPacket { bytes, from, to }]) {
        Ok(1) => Ok(true),
        Ok(_) => Ok(false),
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(false),
        Err(e) => Err(e),
    }
}
pub fn run(server: bool, listen: SocketAddr, peer: SocketAddr, mode: &str) -> io::Result<()> {
    let io = IoSettings {
        mode: if mode == "single" {
            IoMode::Single
        } else if mode == "mmsg" {
            IoMode::Mmsg
        } else {
            return Err(io::ErrorKind::InvalidInput.into());
        },
        ..IoSettings::default()
    };
    let (mut diagnostics, receiver) = remora::diagnostics::Diagnostics::new(
        if server {
            remora::diagnostics::Role::Server
        } else {
            remora::diagnostics::Role::Client
        },
        Instant::now(),
        Duration::from_secs(1),
    )
    .map_err(|_| io::ErrorKind::OutOfMemory)?;
    diagnostics.set_level(remora::config::Level::Info);
    let _writer = receiver.spawn(io::stdout(), io::stderr())?;
    let mut listener = if server {
        None
    } else {
        Some(Udp::bind(listen, &io, true)?)
    };
    let mut outer = Udp::bind(
        if server {
            listen
        } else {
            if peer.is_ipv4() {
                "0.0.0.0:0"
            } else {
                "[::]:0"
            }
            .parse()
            .unwrap()
        },
        &io,
        false,
    )?;
    if !server {
        outer.connect(peer)?;
    }
    let application_pool =
        BufferPool::new(1200, 8 * 1024 * 1024).map_err(|_| io::ErrorKind::OutOfMemory)?;
    let pool = BufferPool::new(1350, 2 * 1024 * 1024).map_err(|_| io::ErrorKind::OutOfMemory)?;
    let mut sources: HashMap<SocketAddr, u64> = HashMap::new();
    let mut ids: HashMap<u64, SocketAddr> = HashMap::new();
    let mut relays: HashMap<u64, (SocketAddr, Udp)> = HashMap::new();
    let mut next = 0u64;
    let mut sent = 0u64;
    let mut dropped = 0u64;
    let mut snapshot = Instant::now();
    loop {
        if let Some(listener) = listener.as_mut() {
            for (from, bytes) in receive(listener, &application_pool, 32)? {
                let id = if let Some(id) = sources.get(&from) {
                    *id
                } else {
                    if sources.len() >= 32 {
                        dropped += 1;
                        continue;
                    }
                    next += 1;
                    sources.insert(from, next);
                    ids.insert(next, from);
                    next
                };
                let mut wrapped = Vec::with_capacity(8 + bytes.len());
                wrapped.extend_from_slice(&id.to_be_bytes());
                wrapped.extend_from_slice(&bytes);
                if send(&mut outer, &wrapped, peer)? {
                    sent += 1
                } else {
                    dropped += 1;
                }
            }
        }
        for (from, bytes) in receive(&mut outer, &pool, 32)? {
            if server {
                if bytes.len() < 8 || bytes.len() > 1208 {
                    dropped += 1;
                    continue;
                }
                let id = u64::from_be_bytes(bytes[..8].try_into().unwrap());
                if !relays.contains_key(&id) {
                    if relays.len() >= 32 {
                        dropped += 1;
                        continue;
                    }
                    let mut relay = Udp::bind(
                        if peer.is_ipv4() {
                            "0.0.0.0:0"
                        } else {
                            "[::]:0"
                        }
                        .parse()
                        .unwrap(),
                        &io,
                        true,
                    )?;
                    relay.connect(peer)?;
                    relays.insert(id, (from, relay));
                }
                let (source, relay) = relays.get_mut(&id).unwrap();
                if *source != from {
                    dropped += 1;
                    continue;
                }
                if send(relay, &bytes[8..], peer)? {
                    sent += 1
                } else {
                    dropped += 1;
                }
            } else if from == peer {
                if bytes.len() < 8 || bytes.len() > 1208 {
                    dropped += 1;
                    continue;
                }
                let id = u64::from_be_bytes(bytes[..8].try_into().unwrap());
                if let Some(to) = ids.get(&id) {
                    if send(
                        listener.as_mut().expect("client listener"),
                        &bytes[8..],
                        *to,
                    )? {
                        sent += 1
                    } else {
                        dropped += 1;
                    }
                } else {
                    dropped += 1;
                }
            }
        }

        if server {
            for (id, (source, relay)) in &mut relays {
                for (_, bytes) in receive(relay, &application_pool, 28)? {
                    let mut wrapped = Vec::with_capacity(8 + bytes.len());
                    wrapped.extend_from_slice(&id.to_be_bytes());
                    wrapped.extend_from_slice(&bytes);
                    if send(&mut outer, &wrapped, *source)? {
                        sent += 1
                    } else {
                        dropped += 1;
                    }
                }
            }
        }
        if snapshot.elapsed() >= Duration::from_secs(1) {
            let mut status = remora::diagnostics::Snapshot {
                sessions_active: if server { relays.len() } else { sources.len() } as u64,
                connections_active: 1,
                ..Default::default()
            };
            status.socket_buffers.outer_receive_max_per_socket = outer.receive_buffer;
            status.socket_buffers.outer_send_max_per_socket = outer.send_buffer;
            for socket in listener
                .iter()
                .chain(relays.values().map(|(_, socket)| socket))
            {
                status.socket_buffers.application_receive_max_per_socket = status
                    .socket_buffers
                    .application_receive_max_per_socket
                    .max(socket.receive_buffer);
                status.socket_buffers.application_send_max_per_socket = status
                    .socket_buffers
                    .application_send_max_per_socket
                    .max(socket.send_buffer);
            }
            status.counters.relay_socket_sent = sent;
            status.counters.local_drops.queue_full.to_target = dropped;
            let _ = diagnostics.snapshot(Instant::now(), status);
            snapshot = Instant::now();
        }
        // Early low-rate baseline: bounded polling cost is recorded, never a claim
        // that this implementation establishes sustainable capacity.
        std::thread::sleep(Duration::from_micros(100));
    }
}
