//! Linux UDP and CLOCK_MONOTONIC boundary. All local syscall/FFI lives here.
//! Batches never wait to fill; callers own the accepted prefix and retain suffix.
use crate::{
    buffers::Slab,
    config::{IoMode, IoSettings},
};
use socket2::{Domain, Protocol, SockAddr, Socket, Type};
use std::{
    io,
    mem::{size_of, zeroed},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket as StdUdp},
    os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
    time::{Duration, Instant},
};
const BATCH: usize = 64;
const CONTROL_WORDS: usize = 16;
const CONTROL_BYTES: usize = CONTROL_WORDS * size_of::<usize>();
const SYSCALL_ATTEMPTS: usize = 4;

/// Retry only calls which consumed nothing. The owner must regain control to
/// poll cancellation and other sockets, even under persistent interruption.
fn retry_interrupted<T>(mut call: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    let mut remaining = SYSCALL_ATTEMPTS;
    loop {
        remaining -= 1;
        match call() {
            Err(error) if error.kind() == io::ErrorKind::Interrupted && remaining > 0 => {}
            result => return result,
        }
    }
}
#[derive(Default, Clone, Copy, serde::Serialize)]
pub struct IoStats {
    pub receive_syscalls: u64,
    pub send_syscalls: u64,
    pub error_queue_syscalls: u64,
    pub mtu_queries: u64,
    pub rx_drop_queries: u64,
    pub received_datagrams: u64,
    pub sent_datagrams: u64,
    pub send_enobufs: u64,
    pub send_emsgsize: u64,
    pub receive_eagain: u64,
    pub send_eagain: u64,
    pub receive_eintr: u64,
    pub send_eintr: u64,
    pub partial_send_batches: u64,
    pub overflowed: bool,
}
impl IoStats {
    pub fn merge(mut self, other: Self) -> Self {
        macro_rules! add {
            ($field:ident) => {
                if let Some(next) = self.$field.checked_add(other.$field) {
                    self.$field = next;
                } else {
                    self.overflowed = true;
                }
            };
        }
        add!(receive_syscalls);
        add!(send_syscalls);
        add!(error_queue_syscalls);
        add!(mtu_queries);
        add!(rx_drop_queries);
        add!(received_datagrams);
        add!(sent_datagrams);
        add!(send_enobufs);
        add!(send_emsgsize);
        add!(receive_eagain);
        add!(send_eagain);
        add!(receive_eintr);
        add!(send_eintr);
        add!(partial_send_batches);
        self.overflowed |= other.overflowed;
        self
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiveFault {
    Truncated,
    Ancillary,
    Address,
}
#[derive(Clone, Copy, Debug)]
pub struct Received {
    pub index: usize,
    pub length: usize,
    pub peer: SocketAddr,
    pub local: SocketAddr,
    pub overflow: Option<u32>,
    pub fault: Option<ReceiveFault>,
}
/// Linux UDP may expose validated/queued ICMP errors on a later normal
/// operation. These observations do not prove that an application Session died.
pub fn is_peer_error(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(
            libc::ECONNREFUSED
                | libc::EHOSTUNREACH
                | libc::ENETUNREACH
                | libc::EMSGSIZE
                | libc::EPROTO
                | libc::EACCES
                | libc::EHOSTDOWN
                | libc::ENETDOWN
        )
    )
}
pub struct SendPacket<'a> {
    pub bytes: &'a [u8],
    pub from: SocketAddr,
    pub to: SocketAddr,
}
/// Numeric error metadata only; quoted original payloads never leave this module.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SocketError {
    pub destination: SocketAddr,
    pub errno: u32,
    pub mtu: Option<usize>,
}
#[derive(Default)]
pub struct ErrorBatch {
    pub errors: [Option<SocketError>; 4],
    pub malformed: usize,
    pub pending: bool,
}
struct BatchMemory {
    headers: [libc::mmsghdr; BATCH],
    iov: [libc::iovec; BATCH],
    addresses: [libc::sockaddr_storage; BATCH],
    control: [[usize; CONTROL_WORDS]; BATCH],
}
impl BatchMemory {
    fn new() -> Self {
        // SAFETY: These C syscall descriptors contain only integers, byte
        // storage and raw pointers, all permitting zero/null. No Rust reference
        // or enum is formed. Headers are fully initialized before a syscall.
        unsafe { zeroed() }
    }
}
pub struct Udp {
    socket: StdUdp,
    local: SocketAddr,
    mode: IoMode,
    max: usize,
    overflow_last: u32,
    memory: Box<BatchMemory>,
    peer: Option<SocketAddr>,
    outer: bool,
    stats: std::cell::Cell<IoStats>,
    pub receive_buffer: usize,
    pub send_buffer: usize,
    #[cfg(test)]
    next_send_error: Option<i32>,
    #[cfg(test)]
    next_rx_drops: Option<io::Result<u32>>,
}
fn set_int(fd: RawFd, level: i32, option: i32, value: i32) -> io::Result<()> {
    // SAFETY: fd is an owned live socket; value is a live aligned C int,
    // readable for its exact size. setsockopt retains no pointer after return.
    let result = unsafe {
        libc::setsockopt(
            fd,
            level,
            option,
            (&value as *const i32).cast(),
            size_of::<i32>() as libc::socklen_t,
        )
    };
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
impl AsRawFd for Udp {
    fn as_raw_fd(&self) -> RawFd {
        self.socket.as_raw_fd()
    }
}
impl Udp {
    pub fn bind(address: SocketAddr, settings: &IoSettings, relay: bool) -> io::Result<Self> {
        if !(1..=BATCH).contains(&settings.batch_max) {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let socket = Socket::new(
            if address.is_ipv4() {
                Domain::IPV4
            } else {
                Domain::IPV6
            },
            Type::DGRAM,
            Some(Protocol::UDP),
        )?;
        socket.set_nonblocking(true)?;
        if address.is_ipv6() {
            socket.set_only_v6(true)?;
        }
        let bytes = if relay {
            settings.relay_socket_buffer_bytes
        } else {
            settings.outer_socket_buffer_bytes
        };
        socket.set_recv_buffer_size(bytes)?;
        socket.set_send_buffer_size(bytes)?;
        let fd = socket.as_raw_fd();
        if address.is_ipv4() {
            set_int(fd, libc::IPPROTO_IP, libc::IP_PKTINFO, 1)?;
            set_int(
                fd,
                libc::IPPROTO_IP,
                libc::IP_MTU_DISCOVER,
                if relay {
                    libc::IP_PMTUDISC_DO
                } else {
                    libc::IP_PMTUDISC_PROBE
                },
            )?;
            if !relay {
                set_int(fd, libc::IPPROTO_IP, libc::IP_RECVERR, 1)?;
            }
        } else {
            set_int(fd, libc::IPPROTO_IPV6, libc::IPV6_RECVPKTINFO, 1)?;
            set_int(fd, libc::IPPROTO_IPV6, libc::IPV6_DONTFRAG, 1)?;
            set_int(
                fd,
                libc::IPPROTO_IPV6,
                libc::IPV6_MTU_DISCOVER,
                if relay {
                    libc::IP_PMTUDISC_DO
                } else {
                    libc::IP_PMTUDISC_PROBE
                },
            )?;
            if !relay {
                set_int(fd, libc::IPPROTO_IPV6, libc::IPV6_RECVERR, 1)?;
            }
        }
        socket.bind(&address.into())?;
        let receive_buffer = socket.recv_buffer_size()?;
        let send_buffer = socket.send_buffer_size()?;
        let socket: StdUdp = socket.into();
        let local = socket.local_addr()?;
        Ok(Self {
            socket,
            local,
            mode: settings.mode,
            max: settings.batch_max,
            overflow_last: 0,
            memory: Box::new(BatchMemory::new()),
            peer: None,
            outer: !relay,
            stats: std::cell::Cell::new(IoStats::default()),
            receive_buffer,
            send_buffer,
            #[cfg(test)]
            next_send_error: None,
            #[cfg(test)]
            next_rx_drops: None,
        })
    }
    /// Read the socket lifetime drop counter without requesting skb ancillary data.
    /// Failure leaves the baseline intact; a later successful read catches up.
    pub fn sample_rx_drops(&mut self) -> io::Result<u64> {
        self.count(|s| &mut s.rx_drop_queries, 1);
        let current = self.read_rx_drops()?;
        let delta = overflow_delta(self.overflow_last, current);
        self.overflow_last = current;
        Ok(u64::from(delta))
    }
    fn read_rx_drops(&mut self) -> io::Result<u32> {
        #[cfg(test)]
        if let Some(result) = self.next_rx_drops.take() {
            return result;
        }
        // Request the stable prefix through SK_MEMINFO_DROPS. Future kernel
        // fields are irrelevant; a shorter response must not look like zero loss.
        let mut values = [0u32; libc::SK_MEMINFO_DROPS as usize + 1];
        let mut length = size_of_val(&values) as libc::socklen_t;
        // SAFETY: the owned socket is live; values and length are initialized,
        // aligned writable storage of the advertised sizes. No pointer escapes.
        let result = unsafe {
            libc::getsockopt(
                self.socket.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_MEMINFO,
                values.as_mut_ptr().cast(),
                &mut length,
            )
        };
        if result == -1 {
            return Err(io::Error::last_os_error());
        }
        decode_rx_drops(&values, length)
    }
    #[cfg(test)]
    pub(crate) fn inject_rx_drops(&mut self, result: io::Result<u32>) {
        self.next_rx_drops = Some(result);
    }
    pub fn stats(&self) -> IoStats {
        self.stats.get()
    }
    fn count(&self, select: fn(&mut IoStats) -> &mut u64, amount: usize) {
        let mut stats = self.stats.get();
        let value = select(&mut stats);
        if let Some(next) = value.checked_add(amount as u64) {
            *value = next;
        } else {
            stats.overflowed = true;
        }
        self.stats.set(stats);
    }
    pub fn connect(&mut self, peer: SocketAddr) -> io::Result<()> {
        self.socket.connect(peer)?;
        self.local = self.socket.local_addr()?;
        self.peer = Some(peer);
        set_int(
            self.as_raw_fd(),
            if peer.is_ipv4() {
                libc::IPPROTO_IP
            } else {
                libc::IPPROTO_IPV6
            },
            if peer.is_ipv4() {
                libc::IP_RECVERR
            } else {
                libc::IPV6_RECVERR
            },
            1,
        )?;
        Ok(())
    }
    /// Connected destination ceiling from the current kernel route. A duplicate
    /// application cache must not permanently pin an old, smaller route MTU.
    pub fn payload_ceiling(&self) -> io::Result<Option<usize>> {
        // QUIC's DPLPMTUD deliberately probes beyond a cached route estimate.
        // PROBE still sets DF; the relay's DO policy never introduces fragments.
        if self.outer {
            return Ok(None);
        }
        let Some(peer) = self.peer else {
            return Ok(None);
        };
        let mut mtu: i32 = 0;
        self.count(|s| &mut s.mtu_queries, 1);
        let mut len = size_of::<i32>() as libc::socklen_t;
        // SAFETY: Owned live socket, aligned initialized int and length storage;
        // getsockopt writes at most the exact supplied size and retains no pointer.
        let result = unsafe {
            libc::getsockopt(
                self.as_raw_fd(),
                if peer.is_ipv4() {
                    libc::IPPROTO_IP
                } else {
                    libc::IPPROTO_IPV6
                },
                if peer.is_ipv4() {
                    libc::IP_MTU
                } else {
                    libc::IPV6_MTU
                },
                (&mut mtu as *mut i32).cast(),
                &mut len,
            )
        };
        if result < 0 {
            return Err(io::Error::last_os_error());
        }
        if len as usize != size_of::<i32>() || mtu <= 0 {
            return Err(io::ErrorKind::InvalidData.into());
        }
        Ok(Some(
            udp_payload_limit(mtu as usize, peer.is_ipv6()).ok_or(io::ErrorKind::InvalidData)?,
        ))
    }
    /// Finite error-queue drain. MSG_TRUNC refers to intentionally ignored quoted
    /// payload; MSG_CTRUNC invalidates metadata. Unknown origins cannot change MTU.
    pub fn drain_errors(&mut self) -> io::Result<ErrorBatch> {
        let mut output = ErrorBatch::default();
        for slot in &mut output.errors {
            let mut quoted = [0u8; 1];
            let mut control = [0usize; CONTROL_WORDS];
            // SAFETY: Integer/pointer-only C descriptors permit zero initialization.
            let mut destination: libc::sockaddr_storage = unsafe { zeroed() };
            let mut iov = libc::iovec {
                iov_base: quoted.as_mut_ptr().cast(),
                iov_len: quoted.len(),
            };
            // SAFETY: Zeroed msghdr fields are populated with live bounded storage
            // before the synchronous call. No pointer is retained by the kernel.
            let mut header: libc::msghdr = unsafe { zeroed() };
            header.msg_name = (&mut destination as *mut libc::sockaddr_storage).cast();
            header.msg_namelen = size_of::<libc::sockaddr_storage>() as u32;
            header.msg_iov = &mut iov;
            header.msg_iovlen = 1;
            header.msg_control = control.as_mut_ptr().cast();
            header.msg_controllen = CONTROL_BYTES;
            // SAFETY: All writable name/iovec/control spans above are initialized,
            // stable, disjoint and sized for recvmsg. Payload is never inspected.
            self.count(|s| &mut s.error_queue_syscalls, 1);
            let n = unsafe {
                libc::recvmsg(
                    self.as_raw_fd(),
                    &mut header,
                    libc::MSG_ERRQUEUE | libc::MSG_DONTWAIT,
                )
            };
            if n < 0 {
                let e = io::Error::last_os_error();
                if e.kind() == io::ErrorKind::WouldBlock {
                    return Ok(output);
                }
                if e.kind() == io::ErrorKind::Interrupted {
                    output.pending = true;
                    return Ok(output);
                }
                return Err(e);
            }
            let Some(to) = address(&destination, header.msg_namelen) else {
                output.malformed += 1;
                continue;
            };
            if header.msg_flags & libc::MSG_CTRUNC != 0
                || header.msg_controllen > CONTROL_BYTES
                || self.peer.is_some_and(|peer| peer != to)
            {
                output.malformed += 1;
                continue;
            }
            // SAFETY: Initialized usize storage has CONTROL_BYTES readable bytes;
            // untrusted kernel length is checked before forming this byte view.
            let bytes = unsafe {
                std::slice::from_raw_parts(control.as_ptr().cast::<u8>(), header.msg_controllen)
            };
            match decode_error_control(bytes, to) {
                Ok(Some(error)) => {
                    *slot = Some(error);
                }
                Ok(None) => {}
                Err(_) => output.malformed += 1,
            }
        }
        output.pending = true;
        Ok(output)
    }
    pub fn local_addr(&self) -> SocketAddr {
        self.local
    }
    /// Receive one nonblocking batch. After four interrupted attempts return
    /// Interrupted with no consumed datagrams, so the owner can service cancel.
    pub fn recv_batch(
        &mut self,
        buffers: &mut [Slab],
        output: &mut Vec<Received>,
    ) -> io::Result<usize> {
        let count = buffers.len().min(self.max);
        if count == 0 {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        output.clear();
        output
            .try_reserve(count)
            .map_err(|_| io::ErrorKind::OutOfMemory)?;
        for (i, slab) in buffers.iter_mut().take(count).enumerate() {
            let memory = &mut self.memory;
            memory.iov[i] = libc::iovec {
                iov_base: slab.bytes_mut().as_mut_ptr().cast(),
                iov_len: slab.capacity(),
            };
            memory.control[i].fill(0);
            // SAFETY: C descriptor zero initialization has no invalid bit patterns.
            memory.headers[i] = unsafe { zeroed() };
            let header = &mut memory.headers[i].msg_hdr;
            header.msg_name = (&mut memory.addresses[i] as *mut libc::sockaddr_storage).cast();
            header.msg_namelen = size_of::<libc::sockaddr_storage>() as u32;
            header.msg_iov = &mut memory.iov[i];
            header.msg_iovlen = 1;
            header.msg_control = memory.control[i].as_mut_ptr().cast();
            header.msg_controllen = CONTROL_BYTES;
        }
        let n = retry_interrupted(|| {
            self.count(|s| &mut s.receive_syscalls, 1);
            // SAFETY: memory is boxed and is neither moved nor resized while the
            // kernel uses its header/iovec/address/control pointers. Slabs are
            // exclusive mutable leases with live initialized capacities; all
            // pointers remain valid for this synchronous call. count <=64.
            let result = unsafe {
                match self.mode {
                    IoMode::Mmsg => libc::recvmmsg(
                        self.as_raw_fd(),
                        self.memory.headers.as_mut_ptr(),
                        count as u32,
                        libc::MSG_DONTWAIT,
                        std::ptr::null_mut(),
                    ),
                    IoMode::Single => {
                        let n = libc::recvmsg(
                            self.as_raw_fd(),
                            &mut self.memory.headers[0].msg_hdr,
                            libc::MSG_DONTWAIT,
                        );
                        if n >= 0 {
                            self.memory.headers[0].msg_len = n as u32;
                            1
                        } else {
                            -1
                        }
                    }
                }
            };
            if result >= 0 {
                return Ok(result as usize);
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                if error.kind() == io::ErrorKind::WouldBlock {
                    self.count(|s| &mut s.receive_eagain, 1);
                }
                return Err(error);
            }
            self.count(|s| &mut s.receive_eintr, 1);
            Err(error)
        })?;
        self.count(|s| &mut s.received_datagrams, n);
        for i in 0..n {
            let h = &self.memory.headers[i];
            let peer = address(&self.memory.addresses[i], h.msg_hdr.msg_namelen);
            let (local, overflow, ancillary) = parse_control(
                &self.memory.control[i],
                h.msg_hdr.msg_controllen,
                self.local,
            );
            let fault = receive_fault(h.msg_hdr.msg_flags, ancillary, peer.is_some());
            output.push(Received {
                index: i,
                length: h.msg_len as usize,
                peer: peer.unwrap_or(self.local),
                local,
                overflow,
                fault,
            });
        }
        Ok(n)
    }
    /// Return the accepted prefix immediately. Four interrupted attempts return
    /// Interrupted without accepting any prefix; the caller retains the suffix.
    pub fn send_batch(&mut self, packets: &[SendPacket<'_>]) -> io::Result<usize> {
        let count = packets.len().min(self.max);
        if count == 0 {
            return Ok(0);
        }
        // Preserve accepted-prefix semantics: trim the syscall to the valid
        // prefix, then the next caller visit observes the oversized suffix.
        let ceiling = self.payload_ceiling()?;
        let count = packets[..count]
            .iter()
            .position(|p| ceiling.is_some_and(|max| p.bytes.len() > max))
            .unwrap_or(count);
        if count == 0 {
            return Err(io::Error::from_raw_os_error(libc::EMSGSIZE));
        }
        for (i, packet) in packets.iter().take(count).enumerate() {
            if !self.outer && self.peer.is_some_and(|peer| peer != packet.to) {
                return Err(io::ErrorKind::InvalidInput.into());
            }
            if packet.to.is_ipv4() != self.local.is_ipv4()
                || packet.from.is_ipv4() != self.local.is_ipv4()
                || packet.from.port() != self.local.port()
            {
                return Err(io::ErrorKind::InvalidInput.into());
            }
            let dest = SockAddr::from(packet.to);
            // SAFETY: sockaddr_storage is integer/byte storage; dest is initialized
            // SockAddr with len <=storage. Exact bytes are copied without aliasing.
            unsafe {
                std::ptr::copy_nonoverlapping(
                    dest.as_ptr().cast::<u8>(),
                    (&mut self.memory.addresses[i] as *mut libc::sockaddr_storage).cast::<u8>(),
                    dest.len() as usize,
                );
            }
            self.memory.iov[i] = libc::iovec {
                iov_base: packet.bytes.as_ptr().cast_mut().cast(),
                iov_len: packet.bytes.len(),
            };
            self.memory.control[i].fill(0);
            // SAFETY: All fields admit zero/null; populated pointers below are
            // borrowed exclusively until the synchronous send call returns.
            self.memory.headers[i] = unsafe { zeroed() };
            let header = &mut self.memory.headers[i].msg_hdr;
            header.msg_name = (&mut self.memory.addresses[i] as *mut libc::sockaddr_storage).cast();
            header.msg_namelen = dest.len();
            header.msg_iov = &mut self.memory.iov[i];
            header.msg_iovlen = 1;
            if !packet.from.ip().is_unspecified() {
                let len = write_pktinfo(&mut self.memory.control[i], packet.from.ip())?;
                header.msg_control = self.memory.control[i].as_mut_ptr().cast();
                header.msg_controllen = len;
            }
        }
        retry_interrupted(|| {
            self.count(|s| &mut s.send_syscalls, 1);
            // Socket-local, one-shot syscall failure for owner regressions.
            // No packet or prefix is submitted; absent from production builds.
            #[cfg(test)]
            if let Some(errno) = self.next_send_error.take() {
                return Err(io::Error::from_raw_os_error(errno));
            }
            // SAFETY: Every header points into stable owned memory or a live
            // immutable packet slice. sendmsg never mutates payload bytes and
            // retains no userspace pointers. The accepted prefix is returned
            // once; no retry of already accepted messages occurs here.
            let result = unsafe {
                match self.mode {
                    IoMode::Mmsg => libc::sendmmsg(
                        self.as_raw_fd(),
                        self.memory.headers.as_mut_ptr(),
                        count as u32,
                        libc::MSG_DONTWAIT,
                    ),
                    IoMode::Single => {
                        let n = libc::sendmsg(
                            self.as_raw_fd(),
                            &self.memory.headers[0].msg_hdr,
                            libc::MSG_DONTWAIT,
                        );
                        if n >= 0 { 1 } else { -1 }
                    }
                }
            };
            if result >= 0 {
                self.count(|s| &mut s.sent_datagrams, result as usize);
                if result as usize != count {
                    self.count(|s| &mut s.partial_send_batches, 1);
                }
                return Ok(result as usize);
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                if error.kind() == io::ErrorKind::WouldBlock {
                    self.count(|s| &mut s.send_eagain, 1);
                }
                if error.raw_os_error() == Some(libc::ENOBUFS) {
                    self.count(|s| &mut s.send_enobufs, 1);
                }
                if error.raw_os_error() == Some(libc::EMSGSIZE) {
                    self.count(|s| &mut s.send_emsgsize, 1);
                }
                return Err(error);
            }
            self.count(|s| &mut s.send_eintr, 1);
            Err(error)
        })
    }
    #[cfg(test)]
    pub(crate) fn fail_next_send(&mut self, errno: i32) {
        assert!(self.next_send_error.replace(errno).is_none());
    }
}
fn address(storage: &libc::sockaddr_storage, len: libc::socklen_t) -> Option<SocketAddr> {
    let required = match i32::from(storage.ss_family) {
        libc::AF_INET => size_of::<libc::sockaddr_in>(),
        libc::AF_INET6 => size_of::<libc::sockaddr_in6>(),
        _ => return None,
    };
    if (len as usize) < required || len as usize > size_of::<libc::sockaddr_storage>() {
        return None;
    }
    // SAFETY: Family and exact minimum/maximum sockaddr lengths checked above.
    // Both repr(C) socket-address types contain only valid integer/byte fields;
    // unaligned loads do not form incorrectly aligned references.
    Some(unsafe {
        match i32::from(storage.ss_family) {
            libc::AF_INET => {
                let v = std::ptr::read_unaligned(
                    (storage as *const libc::sockaddr_storage).cast::<libc::sockaddr_in>(),
                );
                SocketAddr::new(
                    Ipv4Addr::from(v.sin_addr.s_addr.to_ne_bytes()).into(),
                    u16::from_be(v.sin_port),
                )
            }
            _ => {
                let v = std::ptr::read_unaligned(
                    (storage as *const libc::sockaddr_storage).cast::<libc::sockaddr_in6>(),
                );
                SocketAddr::V6(std::net::SocketAddrV6::new(
                    Ipv6Addr::from(v.sin6_addr.s6_addr),
                    u16::from_be(v.sin6_port),
                    u32::from_be(v.sin6_flowinfo),
                    v.sin6_scope_id,
                ))
            }
        }
    })
}
fn aligned(n: usize) -> Option<usize> {
    n.checked_add(size_of::<usize>() - 1)
        .map(|v| v & !(size_of::<usize>() - 1))
}
/// IP/UDP headers for this adapter's no-extension-header socket profile.
pub fn udp_payload_limit(mtu: usize, ipv6: bool) -> Option<usize> {
    if mtu < if ipv6 { 1280 } else { 68 } {
        return None;
    }
    Some(
        mtu.checked_sub(if ipv6 { 48 } else { 28 })?
            .min(if ipv6 { 65527 } else { 65507 }),
    )
}
/// Length-checked extended-error metadata. ICMP is untrusted and can only inform
/// path sizing, never target identity/authorization or quoted-payload replay.
pub fn decode_error_control(
    bytes: &[u8],
    destination: SocketAddr,
) -> io::Result<Option<SocketError>> {
    let mut offset = 0;
    let mut found = None;
    while offset < bytes.len() {
        let h: libc::cmsghdr = read_value(&bytes[offset..]).ok_or(io::ErrorKind::InvalidData)?;
        let start = aligned(size_of::<libc::cmsghdr>()).ok_or(io::ErrorKind::InvalidData)?;
        let end = offset
            .checked_add(h.cmsg_len)
            .filter(|end| h.cmsg_len >= start && *end <= bytes.len())
            .ok_or(io::ErrorKind::InvalidData)?;
        let expected = if destination.is_ipv4() {
            (libc::IPPROTO_IP, libc::IP_RECVERR)
        } else {
            (libc::IPPROTO_IPV6, libc::IPV6_RECVERR)
        };
        if (h.cmsg_level, h.cmsg_type) == expected {
            if found.is_some() {
                return Err(io::ErrorKind::InvalidData.into());
            }
            let e: libc::sock_extended_err =
                read_value(&bytes[offset + start..end]).ok_or(io::ErrorKind::InvalidData)?;
            let local = e.ee_origin == libc::SO_EE_ORIGIN_LOCAL;
            let icmp = if destination.is_ipv4() {
                e.ee_origin == libc::SO_EE_ORIGIN_ICMP
            } else {
                e.ee_origin == libc::SO_EE_ORIGIN_ICMP6
            };
            if local || icmp {
                let too_big = e.ee_errno == libc::EMSGSIZE as u32
                    && (local
                        || if destination.is_ipv4() {
                            e.ee_type == 3 && e.ee_code == 4
                        } else {
                            e.ee_type == 2 && e.ee_code == 0
                        });
                let mtu = (too_big && e.ee_info != 0).then_some(e.ee_info as usize);
                if mtu.is_some_and(|mtu| udp_payload_limit(mtu, destination.is_ipv6()).is_none()) {
                    return Err(io::ErrorKind::InvalidData.into());
                }
                if e.ee_errno == libc::EMSGSIZE as u32 && !too_big {
                    return Err(io::ErrorKind::InvalidData.into());
                }
                found = Some(SocketError {
                    destination,
                    errno: e.ee_errno,
                    mtu,
                });
            }
        }
        let next = aligned(h.cmsg_len)
            .and_then(|n| offset.checked_add(n))
            .ok_or(io::ErrorKind::InvalidData)?;
        if next > bytes.len() {
            if end == bytes.len() {
                break;
            }
            return Err(io::ErrorKind::InvalidData.into());
        }
        offset = next;
    }
    Ok(found)
}
fn read_value<T: Copy>(bytes: &[u8]) -> Option<T> {
    if bytes.len() < size_of::<T>() {
        return None;
    }
    // SAFETY: Called only for repr(C) libc structs/u32 whose fields are integers
    // and every bit pattern is valid. Bounds checked; unaligned read is explicit.
    Some(unsafe { std::ptr::read_unaligned(bytes.as_ptr().cast::<T>()) })
}
/// Length-checked ancillary decoder; malformed metadata is rejected as a whole.
/// Public byte entrypoint permits property testing without manufacturing pointers.
pub fn decode_control(bytes: &[u8], fallback: SocketAddr) -> (SocketAddr, Option<u32>, bool) {
    let mut offset = 0;
    let mut local = fallback;
    let mut overflow = None;
    let mut pktinfo = false;
    while offset < bytes.len() {
        if bytes.len() - offset < size_of::<libc::cmsghdr>() {
            return (fallback, None, true);
        }
        let h: libc::cmsghdr = read_value(&bytes[offset..]).expect("checked header size");
        let start = aligned(size_of::<libc::cmsghdr>()).unwrap();
        let end = match offset.checked_add(h.cmsg_len) {
            Some(end) if h.cmsg_len >= start && end <= bytes.len() => end,
            _ => return (fallback, None, true),
        };
        let data = &bytes[offset + start..end];
        // Baseline sockets never enable UDP_GRO. Any UDP-level ancillary
        // metadata (known or future/unknown segment framing) cannot safely be
        // presented as one ordinary datagram without an implemented splitter.
        if h.cmsg_level == libc::IPPROTO_UDP {
            return (fallback, None, true);
        }
        if h.cmsg_level == libc::IPPROTO_IP && h.cmsg_type == libc::IP_PKTINFO {
            let Some(v) = read_value::<libc::in_pktinfo>(data) else {
                return (fallback, None, true);
            };
            if pktinfo || !fallback.is_ipv4() {
                return (fallback, None, true);
            }
            pktinfo = true;
            local.set_ip(IpAddr::V4(Ipv4Addr::from(v.ipi_addr.s_addr.to_ne_bytes())));
        } else if h.cmsg_level == libc::IPPROTO_IPV6 && h.cmsg_type == libc::IPV6_PKTINFO {
            let Some(v) = read_value::<libc::in6_pktinfo>(data) else {
                return (fallback, None, true);
            };
            if pktinfo || !fallback.is_ipv6() {
                return (fallback, None, true);
            }
            pktinfo = true;
            local = SocketAddr::V6(std::net::SocketAddrV6::new(
                Ipv6Addr::from(v.ipi6_addr.s6_addr),
                fallback.port(),
                0,
                if Ipv6Addr::from(v.ipi6_addr.s6_addr).is_unicast_link_local() {
                    v.ipi6_ifindex
                } else {
                    0
                },
            ));
        } else if h.cmsg_level == libc::SOL_SOCKET && h.cmsg_type == libc::SO_RXQ_OVFL {
            let Some(value) = read_value::<u32>(data) else {
                return (fallback, None, true);
            };
            if overflow.replace(value).is_some() {
                return (fallback, None, true);
            }
        }
        let Some(next) = aligned(h.cmsg_len).and_then(|len| offset.checked_add(len)) else {
            return (fallback, None, true);
        };
        if next > bytes.len() {
            if end == bytes.len() {
                break;
            }
            return (fallback, None, true);
        }
        offset = next;
    }
    if !pktinfo && fallback.ip().is_unspecified() {
        return (fallback, None, true);
    }
    (local, overflow, false)
}
fn parse_control(
    words: &[usize; CONTROL_WORDS],
    len: usize,
    fallback: SocketAddr,
) -> (SocketAddr, Option<u32>, bool) {
    if len > CONTROL_BYTES {
        return (fallback, None, true);
    }
    // SAFETY: Initialized integer storage has exactly CONTROL_BYTES readable
    // bytes; len is kernel-reported but bounded above before constructing view.
    let bytes = unsafe { std::slice::from_raw_parts(words.as_ptr().cast::<u8>(), len) };
    decode_control(bytes, fallback)
}
fn write_pktinfo(words: &mut [usize; CONTROL_WORDS], ip: IpAddr) -> io::Result<usize> {
    let header_size = aligned(size_of::<libc::cmsghdr>()).ok_or(io::ErrorKind::InvalidInput)?;
    let (level, kind, len) = match ip {
        IpAddr::V4(_) => (
            libc::IPPROTO_IP,
            libc::IP_PKTINFO,
            size_of::<libc::in_pktinfo>(),
        ),
        IpAddr::V6(_) => (
            libc::IPPROTO_IPV6,
            libc::IPV6_PKTINFO,
            size_of::<libc::in6_pktinfo>(),
        ),
    };
    let total = header_size + aligned(len).ok_or(io::ErrorKind::InvalidInput)?;
    if total > CONTROL_BYTES {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let h = libc::cmsghdr {
        cmsg_len: header_size + len,
        cmsg_level: level,
        cmsg_type: kind,
    };
    // SAFETY: words provides initialized, usize-aligned capacity >=total;
    // writes are aligned or explicitly unaligned and repr(C) integer structs.
    // Padding is zeroed by caller. No pointer survives the send syscall.
    unsafe {
        std::ptr::write(words.as_mut_ptr().cast::<libc::cmsghdr>(), h);
        let data = words.as_mut_ptr().cast::<u8>().add(header_size);
        match ip {
            IpAddr::V4(ip) => std::ptr::write_unaligned(
                data.cast::<libc::in_pktinfo>(),
                libc::in_pktinfo {
                    ipi_ifindex: 0,
                    ipi_spec_dst: libc::in_addr {
                        s_addr: u32::from_ne_bytes(ip.octets()),
                    },
                    ipi_addr: libc::in_addr { s_addr: 0 },
                },
            ),
            IpAddr::V6(ip) => std::ptr::write_unaligned(
                data.cast::<libc::in6_pktinfo>(),
                libc::in6_pktinfo {
                    ipi6_addr: libc::in6_addr {
                        s6_addr: ip.octets(),
                    },
                    ipi6_ifindex: 0,
                },
            ),
        }
    }
    Ok(total)
}
pub fn overflow_delta(previous: u32, current: u32) -> u32 {
    current.wrapping_sub(previous)
}

pub struct MonotonicTimer {
    fd: OwnedFd,
}
impl AsRawFd for MonotonicTimer {
    fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}
impl MonotonicTimer {
    pub fn new() -> io::Result<Self> {
        // SAFETY: constant supported clock/flags; no pointers. A successful
        // returned FD is uniquely transferred to OwnedFd exactly once.
        let fd = unsafe {
            libc::timerfd_create(
                libc::CLOCK_MONOTONIC,
                libc::TFD_NONBLOCK | libc::TFD_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: fd is newly created and has no other Rust owner.
        Ok(Self {
            fd: unsafe { OwnedFd::from_raw_fd(fd) },
        })
    }
    pub fn arm(&self, deadline: Option<Instant>) -> io::Result<()> {
        let mut value = libc::itimerspec {
            it_interval: libc::timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
            it_value: libc::timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
        };
        if let Some(deadline) = deadline {
            let mut now = libc::timespec {
                tv_sec: 0,
                tv_nsec: 0,
            };
            // SAFETY: now is writable aligned timespec; clock_gettime fills it.
            if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut now) } < 0 {
                return Err(io::Error::last_os_error());
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            let base = Duration::new(
                u64::try_from(now.tv_sec).map_err(|_| io::ErrorKind::InvalidData)?,
                u32::try_from(now.tv_nsec).map_err(|_| io::ErrorKind::InvalidData)?,
            );
            let at = base
                .checked_add(remaining.max(Duration::from_nanos(1)))
                .ok_or(io::ErrorKind::InvalidInput)?;
            value.it_value.tv_sec = at
                .as_secs()
                .try_into()
                .map_err(|_| io::ErrorKind::InvalidInput)?;
            value.it_value.tv_nsec = i64::from(at.subsec_nanos());
        }
        // SAFETY: value is initialized live C struct; timerfd_settime copies it
        // synchronously. Absolute clock is CLOCK_MONOTONIC, never wall time.
        if unsafe {
            libc::timerfd_settime(
                self.as_raw_fd(),
                libc::TFD_TIMER_ABSTIME,
                &value,
                std::ptr::null_mut(),
            )
        } < 0
        {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
    /// Consume an expiration, returning Interrupted after four failed attempts
    /// without clearing the caller's readiness or waiting for another tick.
    pub fn consume(&self) -> io::Result<u64> {
        let mut count = 0u64;
        retry_interrupted(|| {
            // SAFETY: writable aligned u64 of exact eight-byte timerfd ABI length.
            let result = unsafe {
                libc::read(
                    self.as_raw_fd(),
                    (&mut count as *mut u64).cast(),
                    size_of::<u64>(),
                )
            };
            if result == 8 {
                return Ok(count);
            }
            if result >= 0 {
                return Err(io::ErrorKind::InvalidData.into());
            }
            let error = io::Error::last_os_error();
            Err(error)
        })
    }
}

/// Reject both payload and ancillary truncation before interpreting a message.
pub fn receive_fault(
    flags: i32,
    ancillary_invalid: bool,
    address_valid: bool,
) -> Option<ReceiveFault> {
    if flags & (libc::MSG_TRUNC | libc::MSG_CTRUNC) != 0 {
        Some(ReceiveFault::Truncated)
    } else if ancillary_invalid {
        Some(ReceiveFault::Ancillary)
    } else if !address_valid {
        Some(ReceiveFault::Address)
    } else {
        None
    }
}

#[cfg(test)]
#[path = "../tests/io/interrupted.rs"]
mod interrupted_tests;

fn decode_rx_drops(
    values: &[u32; libc::SK_MEMINFO_DROPS as usize + 1],
    length: libc::socklen_t,
) -> io::Result<u32> {
    if (length as usize) < size_of_val(values) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "short SO_MEMINFO response",
        ));
    }
    Ok(values[libc::SK_MEMINFO_DROPS as usize])
}

#[cfg(test)]
mod rx_drop_tests {
    use super::*;

    #[test]
    fn rx_drop_counter_wrap_failure_and_short_response() {
        let mut socket =
            Udp::bind("127.0.0.1:0".parse().unwrap(), &IoSettings::default(), true).unwrap();
        socket.inject_rx_drops(Ok(u32::MAX - 2));
        assert_eq!(socket.sample_rx_drops().unwrap(), u64::from(u32::MAX - 2));
        socket.inject_rx_drops(Err(io::Error::from_raw_os_error(libc::ENOPROTOOPT)));
        assert!(socket.sample_rx_drops().is_err());
        socket.inject_rx_drops(Ok(3));
        assert_eq!(socket.sample_rx_drops().unwrap(), 6);
        socket.inject_rx_drops(Ok(3));
        assert_eq!(socket.sample_rx_drops().unwrap(), 0);
        let values = [0; libc::SK_MEMINFO_DROPS as usize + 1];
        assert!(decode_rx_drops(&values, (size_of_val(&values) - 4) as _).is_err());
        assert_eq!(
            decode_rx_drops(&values, size_of_val(&values) as _).unwrap(),
            0
        );
    }

    #[test]
    fn rx_drop_native_pressure_without_ancillary_option() {
        for mode in [IoMode::Single, IoMode::Mmsg] {
            for relay in [false, true] {
                let settings = IoSettings {
                    mode,
                    outer_socket_buffer_bytes: 4096,
                    relay_socket_buffer_bytes: 4096,
                    ..IoSettings::default()
                };
                let mut socket =
                    Udp::bind("127.0.0.1:0".parse().unwrap(), &settings, relay).unwrap();
                let mut enabled: libc::c_int = -1;
                let mut length = size_of_val(&enabled) as libc::socklen_t;
                // SAFETY: live owned fd and aligned initialized integer storage;
                // getsockopt writes at most the advertised size and retains nothing.
                assert_eq!(
                    unsafe {
                        libc::getsockopt(
                            socket.as_raw_fd(),
                            libc::SOL_SOCKET,
                            libc::SO_RXQ_OVFL,
                            (&mut enabled as *mut libc::c_int).cast(),
                            &mut length,
                        )
                    },
                    0
                );
                assert_eq!(enabled, 0);
                assert_eq!(socket.sample_rx_drops().unwrap(), 0);
                let sender = StdUdp::bind("127.0.0.1:0").unwrap();
                for _ in 0..256 {
                    sender.send_to(&[42; 512], socket.local_addr()).unwrap();
                }
                // No receive is needed to observe drops, including an idle full socket.
                assert!(socket.sample_rx_drops().unwrap() > 0);
                assert_eq!(socket.sample_rx_drops().unwrap(), 0);
                let pool = crate::buffers::BufferPool::new(1024, 1024).unwrap();
                let mut slabs = vec![pool.acquire().unwrap()];
                let mut received = Vec::new();
                assert_eq!(socket.recv_batch(&mut slabs, &mut received).unwrap(), 1);
                assert_eq!(received[0].overflow, None);
                assert_eq!(received[0].fault, None);
                assert_eq!(&slabs[0].bytes()[..512], &[42; 512]);
            }
        }
    }
}
