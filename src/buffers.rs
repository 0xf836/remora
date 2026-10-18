//! Reactor-local ownership and bounded retained capacity. None of these types
//! are Send: a lease never transfers accounting to a different reactor.
use crate::config::Limits;
use serde::Serialize;
use std::{cell::RefCell, collections::VecDeque, net::SocketAddr, rc::Rc, time::Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BudgetError {
    Exhausted,
    Arithmetic,
    Invalid,
    Allocation,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    pub count: usize,
    pub bytes: usize,
}
struct Ledger {
    limit: Usage,
    used: Usage,
    peak: Usage,
}
/// Independent count and byte quotas. Reservations release exactly once on Drop.
#[derive(Clone)]
pub struct Budget(Rc<RefCell<Ledger>>);
impl Budget {
    pub fn new(count: usize, bytes: usize) -> Self {
        Self(Rc::new(RefCell::new(Ledger {
            limit: Usage { count, bytes },
            used: Usage::default(),
            peak: Usage::default(),
        })))
    }
    pub fn used(&self) -> Usage {
        self.0.borrow().used
    }
    pub fn peak(&self) -> Usage {
        self.0.borrow().peak
    }
    pub fn reserve(&self, count: usize, bytes: usize) -> Result<Reservation, BudgetError> {
        let mut l = self.0.borrow_mut();
        let next = Usage {
            count: l
                .used
                .count
                .checked_add(count)
                .ok_or(BudgetError::Arithmetic)?,
            bytes: l
                .used
                .bytes
                .checked_add(bytes)
                .ok_or(BudgetError::Arithmetic)?,
        };
        if next.count > l.limit.count || next.bytes > l.limit.bytes {
            return Err(BudgetError::Exhausted);
        }
        l.used = next;
        l.peak.count = l.peak.count.max(next.count);
        l.peak.bytes = l.peak.bytes.max(next.bytes);
        Ok(Reservation {
            budget: self.clone(),
            charge: Usage { count, bytes },
        })
    }
}
pub struct Reservation {
    budget: Budget,
    charge: Usage,
}
impl Drop for Reservation {
    fn drop(&mut self) {
        let mut l = self.budget.0.borrow_mut();
        // Reservations are not cloneable and their charge is private: these
        // subtractions cannot underflow through any public operation.
        l.used.count = l
            .used
            .count
            .checked_sub(self.charge.count)
            .expect("owned reservation count");
        l.used.bytes = l
            .used
            .bytes
            .checked_sub(self.charge.bytes)
            .expect("owned reservation bytes");
    }
}
struct StoredSlab {
    data: Box<[u8]>,
    _charge: Reservation,
}
struct PoolState {
    free: Vec<StoredSlab>,
    allocations: usize,
}
/// Fixed-size slabs: the exact boxed capacity stays charged while cached or lent.
/// Acquisition performs at most one bounded allocation; reuse does not allocate.
pub struct BufferPool {
    slab_size: usize,
    budget: Budget,
    state: Rc<RefCell<PoolState>>,
}
impl BufferPool {
    pub fn new(slab_size: usize, byte_limit: usize) -> Result<Self, BudgetError> {
        if slab_size == 0 || slab_size > byte_limit || byte_limit > isize::MAX as usize {
            return Err(BudgetError::Invalid);
        }
        Ok(Self {
            slab_size,
            budget: Budget::new(byte_limit / slab_size, byte_limit),
            state: Rc::new(RefCell::new(PoolState {
                free: Vec::new(),
                allocations: 0,
            })),
        })
    }
    pub fn acquire(&self) -> Result<Slab, BudgetError> {
        let cached = { self.state.borrow_mut().free.pop() };
        let stored = if let Some(stored) = cached {
            stored
        } else {
            let charge = self.budget.reserve(1, self.slab_size)?;
            // Reserve the eventual free-list slot now: returning a lease must
            // never allocate or fail in Drop, even when every slab was lent.
            let retained = self.budget.used().count;
            let mut state = self.state.borrow_mut();
            if state.free.capacity() < retained {
                let extra = retained
                    .checked_sub(state.free.len())
                    .ok_or(BudgetError::Arithmetic)?;
                state
                    .free
                    .try_reserve_exact(extra)
                    .map_err(|_| BudgetError::Allocation)?;
            }
            drop(state);
            let mut data = Vec::new();
            data.try_reserve_exact(self.slab_size)
                .map_err(|_| BudgetError::Allocation)?;
            data.resize(self.slab_size, 0);
            let mut state = self.state.borrow_mut();
            state.allocations = state
                .allocations
                .checked_add(1)
                .ok_or(BudgetError::Arithmetic)?;
            // Box<[u8]> has exactly this many addressable bytes. Do not cache a
            // Vec with an implementation-dependent excess capacity.
            StoredSlab {
                data: data.into_boxed_slice(),
                _charge: charge,
            }
        };
        Ok(Slab {
            stored: Some(stored),
            state: self.state.clone(),
        })
    }
    pub fn retained_peak(&self) -> Usage {
        self.budget.peak()
    }
    pub fn retained(&self) -> Usage {
        self.budget.used()
    }
    /// Successful new payload-slab allocations, excluding metadata/native heaps.
    pub fn allocations(&self) -> usize {
        self.state.borrow().allocations
    }
    /// Separately reported finite descriptor capacity, not UDP payload bytes.
    pub fn metadata_capacity_bytes(&self) -> usize {
        self.state
            .borrow()
            .free
            .capacity()
            .checked_mul(std::mem::size_of::<StoredSlab>())
            .expect("Vec allocation bound")
    }
    /// Retire a bounded suffix of idle payload storage while keeping a reuse
    /// reserve. Lent slabs and their preallocated return slots remain intact.
    pub fn trim_cached(&self, keep: usize, quota: usize) {
        let mut state = self.state.borrow_mut();
        for _ in 0..quota.min(state.free.len().saturating_sub(keep)) {
            state.free.pop();
        }
    }
    pub fn trim_idle(&self) {
        let mut state = self.state.borrow_mut();
        state.free.clear();
        if self.budget.used().count == 0 {
            state.free = Vec::new();
        }
        // Lent slabs still need their preallocated return slots.
    }
}
/// Unique non-cloneable lease; bytes are deliberately excluded from Debug.
pub struct Slab {
    stored: Option<StoredSlab>,
    state: Rc<RefCell<PoolState>>,
}
impl Slab {
    pub fn bytes(&self) -> &[u8] {
        &self.stored.as_ref().expect("live slab").data
    }
    pub fn bytes_mut(&mut self) -> &mut [u8] {
        &mut self.stored.as_mut().expect("live slab").data
    }
    pub fn capacity(&self) -> usize {
        self.bytes().len()
    }
}
impl Drop for Slab {
    fn drop(&mut self) {
        if let Some(mut stored) = self.stored.take() {
            // Never make a previous client's contents visible through a new lease.
            stored.data.fill(0);
            self.state.borrow_mut().free.push(stored);
        }
    }
}
/// Default global pool ceilings; slab sizes follow the validated configuration.
pub struct Pools {
    pub application: BufferPool,
    pub ciphertext: BufferPool,
    pub scratch: BufferPool,
}
impl Pools {
    pub fn new(
        limits: &Limits,
        application_size: usize,
        outer_size: usize,
        scratch_size: usize,
    ) -> Result<Self, BudgetError> {
        Ok(Self {
            application: BufferPool::new(application_size, limits.application_pool_bytes)?,
            ciphertext: BufferPool::new(outer_size, limits.ciphertext_pool_bytes)?,
            scratch: BufferPool::new(scratch_size, limits.scratch_pool_bytes)?,
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    ToTarget,
    ToClient,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Generation(pub u64);
/// Owner-only monotonically increasing generation source. Overflow is an error.
#[derive(Default)]
pub struct Generations(u64);
impl Generations {
    pub fn next_generation(&mut self) -> Result<Generation, BudgetError> {
        self.0 = self.0.checked_add(1).ok_or(BudgetError::Arithmetic)?;
        Ok(Generation(self.0))
    }
}
pub struct Datagram {
    slab: Slab,
    offset: usize,
    length: usize,
    direction: Direction,
    session: Generation,
    connection: Generation,
    received_at: Instant,
    expires_at: Instant,
}
impl Datagram {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        slab: Slab,
        offset: usize,
        length: usize,
        direction: Direction,
        session: Generation,
        connection: Generation,
        received_at: Instant,
        expires_at: Instant,
    ) -> Result<Self, BudgetError> {
        if offset.checked_add(length).ok_or(BudgetError::Arithmetic)? > slab.capacity()
            || expires_at < received_at
        {
            return Err(BudgetError::Invalid);
        }
        Ok(Self {
            slab,
            offset,
            length,
            direction,
            session,
            connection,
            received_at,
            expires_at,
        })
    }
    pub fn bytes(&self) -> &[u8] {
        &self.slab.bytes()[self.offset..self.offset + self.length]
    }
    pub fn capacity(&self) -> usize {
        self.slab.capacity()
    }
    pub fn offset(&self) -> usize {
        self.offset
    }
    pub fn direction(&self) -> Direction {
        self.direction
    }
    pub fn session(&self) -> Generation {
        self.session
    }
    pub fn connection(&self) -> Generation {
        self.connection
    }
    pub fn received_at(&self) -> Instant {
        self.received_at
    }
    pub fn expires_at(&self) -> Instant {
        self.expires_at
    }
}
/// Generated output is immutable and remains on its original connection.
pub struct Ciphertext {
    slab: Slab,
    length: usize,
    from: SocketAddr,
    to: SocketAddr,
    at: Instant,
    connection: Generation,
    sequence: u64,
    generated_at: Instant,
    expires_at: Instant,
}
impl Ciphertext {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        slab: Slab,
        length: usize,
        from: SocketAddr,
        to: SocketAddr,
        at: Instant,
        connection: Generation,
        sequence: u64,
        generated_at: Instant,
        expires_at: Instant,
    ) -> Result<Self, BudgetError> {
        if length > slab.capacity() || expires_at < generated_at {
            return Err(BudgetError::Invalid);
        }
        Ok(Self {
            slab,
            length,
            from,
            to,
            at,
            connection,
            sequence,
            generated_at,
            expires_at,
        })
    }
    pub fn bytes(&self) -> &[u8] {
        &self.slab.bytes()[..self.length]
    }
    pub fn from(&self) -> SocketAddr {
        self.from
    }
    pub fn to(&self) -> SocketAddr {
        self.to
    }
    pub fn at(&self) -> Instant {
        self.at
    }
    pub fn connection(&self) -> Generation {
        self.connection
    }
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub fn generated_at(&self) -> Instant {
        self.generated_at
    }
    pub fn expires_at(&self) -> Instant {
        self.expires_at
    }
}
/// One session's two directions share a byte cap and have separate packet caps.
pub struct SessionQueue {
    queues: [VecDeque<Datagram>; 2],
    packets: usize,
    byte_limit: usize,
    bytes: usize,
    peak: Usage,
}
impl SessionQueue {
    pub fn new(packets: usize, byte_limit: usize) -> Self {
        Self {
            queues: [VecDeque::new(), VecDeque::new()],
            packets,
            byte_limit,
            bytes: 0,
            peak: Usage::default(),
        }
    }
    pub fn push(&mut self, packet: Datagram) -> Result<(), BudgetError> {
        let index = direction_index(packet.direction);
        let bytes = self
            .bytes
            .checked_add(packet.length)
            .ok_or(BudgetError::Arithmetic)?;
        if self.queues[index].len() >= self.packets || bytes > self.byte_limit {
            return Err(BudgetError::Exhausted);
        }
        self.queues[index]
            .try_reserve(1)
            .map_err(|_| BudgetError::Allocation)?;
        self.queues[index].push_back(packet);
        self.bytes = bytes;
        self.peak.count = self.peak.count.max(self.used().count);
        self.peak.bytes = self.peak.bytes.max(bytes);
        Ok(())
    }
    pub fn peak(&self) -> Usage {
        self.peak
    }
    pub fn front(&self, direction: Direction) -> Option<&Datagram> {
        self.queues[direction_index(direction)].front()
    }
    pub fn pop(&mut self, direction: Direction) -> Option<Datagram> {
        let packet = self.queues[direction_index(direction)].pop_front()?;
        self.bytes = self.bytes.checked_sub(packet.length).expect("queued bytes");
        Some(packet)
    }
    pub fn used(&self) -> Usage {
        Usage {
            count: self.queues[0].len() + self.queues[1].len(),
            bytes: self.bytes,
        }
    }
    pub fn metadata_capacity_bytes(&self) -> usize {
        self.queues
            .iter()
            .map(|q| {
                q.capacity()
                    .checked_mul(std::mem::size_of::<Datagram>())
                    .expect("VecDeque allocation bound")
            })
            .try_fold(0usize, |total, bytes| total.checked_add(bytes))
            .expect("two bounded queues")
    }
}
fn direction_index(direction: Direction) -> usize {
    match direction {
        Direction::ToTarget => 0,
        Direction::ToClient => 1,
    }
}
/// Counts include pending/active/cleanup. Holding a permit defines its lifetime.
pub struct ResourceBudgets {
    pub connections: Budget,
    pub sessions: Budget,
    pub setup: Budget,
    pub relay_fds: Budget,
    pub fds: Budget,
}
impl ResourceBudgets {
    pub fn new(limits: &Limits, fd_limit: usize) -> Self {
        Self {
            connections: Budget::new(limits.max_connections, 0),
            sessions: Budget::new(limits.max_sessions, 0),
            setup: Budget::new(limits.max_pending_setup, 0),
            relay_fds: Budget::new(limits.max_relay_sockets, 0),
            fds: Budget::new(fd_limit, 0),
        }
    }
    /// Configured identities own independent pending+active permits. The finite
    /// vector is startup-only; network identity strings never allocate a ledger.
    pub fn identity_budgets(limits: &Limits, identities: usize) -> Vec<Budget> {
        (0..identities)
            .map(|_| Budget::new(limits.max_sessions_per_identity, 0))
            .collect()
    }
    /// A separate two-direction DATAGRAM budget for each admitted connection.
    pub fn native_datagrams(limits: &Limits) -> Result<NativeDatagrams, BudgetError> {
        Ok(NativeDatagrams {
            directions: [
                Budget::new(limits.quiche_dgram_packets, 0),
                Budget::new(limits.quiche_dgram_packets, 0),
            ],
            bytes: Budget::new(0, limits.connection_dgram_bytes),
        })
    }
}
pub struct NativeDatagrams {
    directions: [Budget; 2],
    bytes: Budget,
}
impl NativeDatagrams {
    /// Count and bytes debit atomically; either failure rolls back all reservations.
    pub fn reserve(
        &self,
        direction: Direction,
        capacity: usize,
    ) -> Result<(Reservation, Reservation), BudgetError> {
        let count = self.directions[direction_index(direction)].reserve(1, 0)?;
        let bytes = self.bytes.reserve(0, capacity)?;
        Ok((count, bytes))
    }
    pub fn used_bytes(&self) -> usize {
        self.bytes.used().bytes
    }
}
