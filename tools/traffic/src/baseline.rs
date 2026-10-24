//! Test-only two-leg relay. Reuses reviewed Linux UDP I/O and configured batch
//! buffers. A fixed envelope carries only the baseline's session association.
use remora::{
    buffers::{BufferPool, Slab},
    config::{IoMode, IoSettings},
    udp::{SendPacket, Udp},
};
use std::{
    collections::{HashMap, VecDeque},
    io,
    net::SocketAddr,
    os::fd::AsRawFd,
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
struct Pending {
    bytes: Slab,
    length: usize,
    to: SocketAddr,
    expires: Instant,
}
struct Queues<'a> {
    pending: HashMap<i32, HashMap<SocketAddr, VecDeque<Pending>>>,
    retained: usize,
    application: &'a BufferPool,
    ciphertext: &'a BufferPool,
    batch_max: usize,
}
impl Queues<'_> {
    fn send(
        &mut self,
        socket: &mut Udp,
        bytes: &[u8],
        to: SocketAddr,
        outer: bool,
    ) -> io::Result<bool> {
        let q = self
            .pending
            .entry(socket.as_raw_fd())
            .or_default()
            .entry(to)
            .or_default();
        if q.is_empty() {
            match socket.send_batch(&[SendPacket {
                bytes,
                from: socket.local_addr(),
                to,
            }]) {
                Ok(1) => return Ok(true),
                Err(e)
                    if e.raw_os_error() == Some(libc_errno_enobufs())
                        || e.raw_os_error() == Some(90) =>
                {
                    return Ok(false);
                }
                Err(e)
                    if e.kind() == io::ErrorKind::WouldBlock
                        || e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
                _ => {}
            }
        }
        let limit = if outer { 64 } else { 32 };
        let byte_limit = if outer { 262144 } else { 65536 };
        if q.len() >= limit || q.iter().map(|p| p.length).sum::<usize>() + bytes.len() > byte_limit
        {
            return Ok(false);
        }
        let pool = if outer {
            self.ciphertext
        } else {
            self.application
        };
        let Ok(mut slab) = pool.acquire() else {
            return Ok(false);
        };
        if bytes.len() > slab.capacity() {
            return Ok(false);
        }
        slab.bytes_mut()[..bytes.len()].copy_from_slice(bytes);
        q.push_back(Pending {
            bytes: slab,
            length: bytes.len(),
            to,
            expires: Instant::now() + Duration::from_millis(if outer { 100 } else { 20 }),
        });
        self.retained += bytes.len();
        Ok(true)
    }
    fn flush(&mut self, socket: &mut Udp, dropped: &mut u64) -> io::Result<usize> {
        let Some(groups) = self.pending.get_mut(&socket.as_raw_fd()) else {
            return Ok(0);
        };
        let now = Instant::now();
        let mut progress = 0;
        for q in groups.values_mut() {
            while q.front().is_some_and(|p| p.expires <= now) {
                self.retained -= q.pop_front().unwrap().length;
                *dropped += 1;
            }
            if q.is_empty() {
                continue;
            }
            let packets: Vec<_> = q
                .iter()
                .take(self.batch_max)
                .map(|p| SendPacket {
                    bytes: &p.bytes.bytes()[..p.length],
                    from: socket.local_addr(),
                    to: p.to,
                })
                .collect();
            let accepted = match socket.send_batch(&packets) {
                Ok(n) => n,
                Err(e)
                    if e.kind() == io::ErrorKind::WouldBlock
                        || e.kind() == io::ErrorKind::Interrupted =>
                {
                    continue;
                }
                Err(e)
                    if e.raw_os_error() == Some(libc_errno_enobufs())
                        || e.raw_os_error() == Some(90) =>
                {
                    drop(packets);
                    self.retained -= q.pop_front().unwrap().length;
                    *dropped += 1;
                    continue;
                }
                Err(e) => return Err(e),
            };
            for _ in 0..accepted {
                self.retained -= q.pop_front().unwrap().length;
            }
            progress += accepted;
        }
        Ok(progress)
    }
}
// Linux-only test executable; no extra dependency or unsafe syscall outside the
// reviewed production adapter. Values are Linux ENOBUFS=105 and EMSGSIZE=90.
fn libc_errno_enobufs() -> i32 {
    105
}
fn send(
    socket: &mut Udp,
    bytes: &[u8],
    to: SocketAddr,
    queues: &mut Queues<'_>,
    outer: bool,
) -> io::Result<bool> {
    queues.send(socket, bytes, to, outer)
}
pub struct Buffers {
    pub application_pool_bytes: usize,
    pub application_max_bytes: usize,
    pub outer_udp_max_bytes: usize,
}
pub fn run(
    server: bool,
    listen: SocketAddr,
    peer: SocketAddr,
    mode: &str,
    batch_max: usize,
    buffers: Buffers,
    outer_socket_buffer_bytes: usize,
) -> io::Result<()> {
    if !(1..=64).contains(&batch_max)
        || !(40..=1400).contains(&buffers.application_max_bytes)
        || !(buffers.application_max_bytes + 8..=1452).contains(&buffers.outer_udp_max_bytes)
        || buffers.application_pool_bytes < buffers.application_max_bytes * batch_max
        || !(1..=i32::MAX as usize).contains(&outer_socket_buffer_bytes)
    {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let io = IoSettings {
        mode: if mode == "single" {
            IoMode::Single
        } else if mode == "mmsg" {
            IoMode::Mmsg
        } else {
            return Err(io::ErrorKind::InvalidInput.into());
        },
        batch_max,
        outer_socket_buffer_bytes,
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
    let application_pool = BufferPool::new(
        buffers.application_max_bytes,
        buffers.application_pool_bytes,
    )
    .map_err(|_| io::ErrorKind::OutOfMemory)?;
    let pool = BufferPool::new(buffers.outer_udp_max_bytes, 2 * 1024 * 1024)
        .map_err(|_| io::ErrorKind::OutOfMemory)?;
    let mut sources: HashMap<SocketAddr, u64> = HashMap::new();
    let mut ids: HashMap<u64, SocketAddr> = HashMap::new();
    let mut relays: HashMap<u64, (SocketAddr, Udp)> = HashMap::new();
    let mut next = 0u64;
    let mut sent = 0u64;
    let mut dropped = 0u64;
    let mut snapshot = Instant::now();
    let ciphertext_pool = BufferPool::new(buffers.outer_udp_max_bytes, 4 * 1024 * 1024)
        .map_err(|_| io::ErrorKind::OutOfMemory)?;
    let mut queues = Queues {
        pending: HashMap::new(),
        retained: 0,
        application: &application_pool,
        ciphertext: &ciphertext_pool,
        batch_max,
    };
    loop {
        let before = (sent, dropped);
        let mut progress = queues.flush(&mut outer, &mut dropped)?;
        if let Some(socket) = listener.as_mut() {
            progress += queues.flush(socket, &mut dropped)?;
        }
        for (_, socket) in relays.values_mut() {
            progress += queues.flush(socket, &mut dropped)?;
        }
        if let Some(listener) = listener.as_mut() {
            for (from, bytes) in receive(listener, &application_pool, batch_max)? {
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
                if send(&mut outer, &wrapped, peer, &mut queues, true)? {
                    sent += 1
                } else {
                    dropped += 1;
                }
            }
        }
        for (from, bytes) in receive(&mut outer, &pool, batch_max)? {
            if server {
                if bytes.len() < 8 || bytes.len() > buffers.application_max_bytes + 8 {
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
                if send(relay, &bytes[8..], peer, &mut queues, false)? {
                    sent += 1
                } else {
                    dropped += 1;
                }
            } else if from == peer {
                if bytes.len() < 8 || bytes.len() > buffers.application_max_bytes + 8 {
                    dropped += 1;
                    continue;
                }
                let id = u64::from_be_bytes(bytes[..8].try_into().unwrap());
                if let Some(to) = ids.get(&id) {
                    if send(
                        listener.as_mut().expect("client listener"),
                        &bytes[8..],
                        *to,
                        &mut queues,
                        false,
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
                for (_, bytes) in receive(relay, &application_pool, batch_max)? {
                    let mut wrapped = Vec::with_capacity(8 + bytes.len());
                    wrapped.extend_from_slice(&id.to_be_bytes());
                    wrapped.extend_from_slice(&bytes);
                    if send(&mut outer, &wrapped, *source, &mut queues, true)? {
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
            status.profile.io = outer.stats();
            for socket in listener
                .iter()
                .chain(relays.values().map(|(_, socket)| socket))
            {
                status.profile.io = status.profile.io.merge(socket.stats());
            }
            status.profile.payload_slab_allocations =
                application_pool.allocations() + pool.allocations() + ciphertext_pool.allocations();
            status.counters.relay_socket_sent = sent;
            status.counters.local_drops.queue_full.to_target = dropped;
            let _ = diagnostics.snapshot(Instant::now(), status);
            snapshot = Instant::now();
        }
        // Idle-only bounded polling; do not sleep after productive work.
        // Sustainable capacity is measured externally, never inferred here.
        if progress == 0 && before == (sent, dropped) {
            std::thread::sleep(Duration::from_micros(100));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_outer_buffer_requests_fail_before_binding() {
        for bytes in [0, i32::MAX as usize + 1, usize::MAX] {
            let error = run(
                false,
                "127.0.0.1:0".parse().unwrap(),
                "127.0.0.1:9".parse().unwrap(),
                "mmsg",
                32,
                Buffers {
                    application_pool_bytes: 8 * 1024 * 1024,
                    application_max_bytes: 1200,
                    outer_udp_max_bytes: 1350,
                },
                bytes,
            )
            .unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        }
    }
    #[test]
    fn native_queued_partial_prefix_never_replays_and_keeps_original_suffix() {
        let settings = IoSettings::default();
        let mut sender = Udp::bind("127.0.0.1:0".parse().unwrap(), &settings, false).unwrap();
        let mut receiver = Udp::bind("127.0.0.1:0".parse().unwrap(), &settings, false).unwrap();
        let application = BufferPool::new(1200, 1200 * 8).unwrap();
        let ciphertext = BufferPool::new(1350, 1350 * 8).unwrap();
        let mut queues = Queues {
            pending: HashMap::new(),
            retained: 0,
            application: &application,
            ciphertext: &ciphertext,
            batch_max: 32,
        };
        let to = receiver.local_addr();
        let mut group = VecDeque::new();
        for (bytes, destination) in [
            (b"".as_slice(), to),
            (b"bad".as_slice(), "255.255.255.255:45678".parse().unwrap()),
            (b"tail".as_slice(), to),
        ] {
            let mut slab = ciphertext.acquire().unwrap();
            slab.bytes_mut()[..bytes.len()].copy_from_slice(bytes);
            queues.retained += bytes.len();
            group.push_back(Pending {
                bytes: slab,
                length: bytes.len(),
                to: destination,
                expires: Instant::now() + Duration::from_secs(2),
            });
        }
        queues
            .pending
            .entry(sender.as_raw_fd())
            .or_default()
            .insert(to, group);
        let mut dropped = 0;
        assert_eq!(queues.flush(&mut sender, &mut dropped).unwrap(), 1);
        assert!(queues.flush(&mut sender, &mut dropped).is_err());
        let q = queues
            .pending
            .get_mut(&sender.as_raw_fd())
            .unwrap()
            .get_mut(&to)
            .unwrap();
        assert_eq!(q.len(), 2);
        assert_eq!(q[1].bytes.bytes()[..q[1].length], *b"tail");
        // The test driver rejects the failed destination, then resumes only
        // its original suffix; the already accepted empty message stays gone.
        queues.retained -= q.pop_front().unwrap().length;
        assert_eq!(queues.flush(&mut sender, &mut dropped).unwrap(), 1);
        assert_eq!(queues.retained, 0);
        assert_eq!(sender.stats().sent_datagrams, 2);
        let deadline = Instant::now() + Duration::from_secs(1);
        let mut seen = Vec::new();
        while seen.len() < 2 {
            seen.extend(
                receive(&mut receiver, &application, 4)
                    .unwrap()
                    .into_iter()
                    .map(|(_, b)| b),
            );
            assert!(Instant::now() < deadline);
        }
        assert_eq!(seen, vec![vec![], b"tail".to_vec()]);
        assert!(receive(&mut receiver, &application, 4).unwrap().is_empty());
    }
    #[test]
    fn expired_pending_slabs_are_reclaimed_without_submission() {
        let settings = IoSettings::default();
        let mut sender = Udp::bind("127.0.0.1:0".parse().unwrap(), &settings, false).unwrap();
        let application = BufferPool::new(1200, 1200).unwrap();
        let ciphertext = BufferPool::new(1350, 1350).unwrap();
        let to = "127.0.0.1:45678".parse().unwrap();
        let mut group = VecDeque::new();
        group.push_back(Pending {
            bytes: application.acquire().unwrap(),
            length: 0,
            to,
            expires: Instant::now(),
        });
        assert!(application.acquire().is_err());
        let mut queues = Queues {
            pending: HashMap::from([(sender.as_raw_fd(), HashMap::from([(to, group)]))]),
            retained: 0,
            application: &application,
            ciphertext: &ciphertext,
            batch_max: 32,
        };
        let mut dropped = 0;
        assert_eq!(queues.flush(&mut sender, &mut dropped).unwrap(), 0);
        assert_eq!(dropped, 1);
        assert_eq!(sender.stats().send_syscalls, 0);
        assert!(application.acquire().is_ok());
        assert_eq!(application.allocations(), 1);
    }
}
