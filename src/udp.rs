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
pub struct SendPacket<'a> {
    pub bytes: &'a [u8],
    pub from: SocketAddr,
    pub to: SocketAddr,
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
    overflow_pending: u64,
    memory: Box<BatchMemory>,
    pub receive_buffer: usize,
    pub send_buffer: usize,
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
                libc::IP_PMTUDISC_DO,
            )?;
        } else {
            set_int(fd, libc::IPPROTO_IPV6, libc::IPV6_RECVPKTINFO, 1)?;
            set_int(
                fd,
                libc::IPPROTO_IPV6,
                libc::IPV6_MTU_DISCOVER,
                libc::IP_PMTUDISC_DO,
            )?;
        }
        set_int(fd, libc::SOL_SOCKET, libc::SO_RXQ_OVFL, 1)?;
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
            overflow_pending: 0,
            memory: Box::new(BatchMemory::new()),
            receive_buffer,
            send_buffer,
        })
    }
    pub fn take_overflow(&mut self) -> u64 {
        std::mem::take(&mut self.overflow_pending)
    }
    pub fn connect(&mut self, peer: SocketAddr) -> io::Result<()> {
        self.socket.connect(peer)?;
        self.local = self.socket.local_addr()?;
        Ok(())
    }
    pub fn local_addr(&self) -> SocketAddr {
        self.local
    }
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
        let n = loop {
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
                break result as usize;
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        };
        for i in 0..n {
            let h = &self.memory.headers[i];
            let peer = address(&self.memory.addresses[i], h.msg_hdr.msg_namelen);
            let (local, overflow, ancillary) = parse_control(
                &self.memory.control[i],
                h.msg_hdr.msg_controllen,
                self.local,
            );
            let fault = receive_fault(h.msg_hdr.msg_flags, ancillary, peer.is_some());
            if fault.is_none()
                && let Some(value) = overflow
            {
                self.overflow_pending = self
                    .overflow_pending
                    .checked_add(u64::from(overflow_delta(self.overflow_last, value)))
                    .ok_or(io::ErrorKind::Other)?;
                self.overflow_last = value;
            }
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
    pub fn send_batch(&mut self, packets: &[SendPacket<'_>]) -> io::Result<usize> {
        let count = packets.len().min(self.max);
        if count == 0 {
            return Ok(0);
        }
        for (i, packet) in packets.iter().take(count).enumerate() {
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
        loop {
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
                return Ok(result as usize);
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
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
    pub fn consume(&self) -> io::Result<u64> {
        let mut count = 0u64;
        loop {
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
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
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
