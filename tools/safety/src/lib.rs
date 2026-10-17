//! Test-only source inclusion avoids building Linux/BoringSSL under Miri.
//! Only the configuration type is an adapter; ownership code is production source.
#![allow(dead_code)]
mod config {
    pub struct Limits {
        pub application_pool_bytes: usize,
        pub ciphertext_pool_bytes: usize,
        pub scratch_pool_bytes: usize,
        pub max_connections: usize,
        pub max_sessions: usize,
        pub max_pending_setup: usize,
        pub max_relay_sockets: usize,
        pub max_sessions_per_identity: usize,
        pub quiche_dgram_packets: usize,
        pub connection_dgram_bytes: usize,
    }
}
#[path = "../../../src/buffers.rs"]
mod buffers;
#[cfg(test)]
mod tests {
    use super::buffers::*;
    use std::time::{Duration, Instant};
    #[test]
    fn bounded_cache_retirement_keeps_live_borrow_and_preallocated_return_slots() {
        let pool = BufferPool::new(8, 48).unwrap();
        let mut slabs: Vec<_> = (0..6).map(|_| pool.acquire().unwrap()).collect();
        let mut held = slabs.pop().unwrap();
        held.bytes_mut().fill(7);
        drop(slabs);
        let bytes = held.bytes();
        pool.trim_cached(2, 1);
        assert_eq!(pool.retained().count, 5);
        pool.trim_cached(2, usize::MAX);
        assert_eq!(pool.retained().count, 3);
        assert_eq!(bytes, &[7; 8]);
        let before = pool.allocations();
        let a = pool.acquire().unwrap();
        let b = pool.acquire().unwrap();
        assert_eq!(pool.allocations(), before);
        pool.trim_cached(0, usize::MAX);
        assert_eq!(pool.retained().count, 3);
        drop((held, a, b));
        pool.trim_cached(0, 3);
        assert_eq!(pool.retained(), Usage::default());
    }
    #[test]
    fn cancellation_returns_slabs_and_reservations_exactly_once() {
        let pool = BufferPool::new(64, 256).unwrap();
        let budget = Budget::new(4, 256);
        let mut leases = Vec::new();
        let mut queue = SessionQueue::new(4, 256);
        let now = Instant::now();
        for i in 0..4 {
            leases.push(budget.reserve(1, 64).unwrap());
            let mut slab = pool.acquire().unwrap();
            slab.bytes_mut().fill(i);
            queue
                .push(
                    Datagram::new(
                        slab,
                        0,
                        64,
                        Direction::ToTarget,
                        Generation(1),
                        Generation(2),
                        now,
                        now + Duration::from_millis(20),
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        assert!(pool.acquire().is_err());
        let held = queue.pop(Direction::ToTarget).unwrap();
        drop(queue);
        drop(leases);
        assert_eq!(budget.used(), Usage::default());
        assert_eq!(held.bytes(), &[0; 64]);
        // Borrow remains valid after unrelated slabs return and are reused.
        let mut reused = pool.acquire().unwrap();
        reused.bytes_mut().fill(99);
        assert_eq!(held.bytes(), &[0; 64]);
        drop(reused);
        drop(held);
        pool.trim_idle();
        assert_eq!(pool.retained(), Usage::default());
    }
    #[test]
    fn invalid_ranges_failed_reservations_and_zero_length_release_ownership() {
        let pool = BufferPool::new(8, 16).unwrap();
        let now = Instant::now();
        assert!(
            Datagram::new(
                pool.acquire().unwrap(),
                usize::MAX,
                1,
                Direction::ToClient,
                Generation(1),
                Generation(1),
                now,
                now
            )
            .is_err()
        );
        let mut queue = SessionQueue::new(1, 0);
        for _ in 0..2 {
            let packet = Datagram::new(
                pool.acquire().unwrap(),
                8,
                0,
                Direction::ToClient,
                Generation(1),
                Generation(1),
                now,
                now,
            )
            .unwrap();
            let _ = queue.push(packet);
        }
        assert_eq!(queue.used(), Usage { count: 1, bytes: 0 });
        drop(queue);
        pool.trim_idle();
        assert_eq!(pool.retained().bytes, 0);
        let budget = Budget::new(1, 1);
        let reservation = budget.reserve(1, 1).unwrap();
        assert!(budget.reserve(usize::MAX, usize::MAX).is_err());
        assert_eq!(budget.used(), Usage { count: 1, bytes: 1 });
        drop(reservation);
        assert_eq!(budget.used(), Usage::default());
    }
}
