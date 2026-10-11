use crate::common::{FakeClock, IoStep, ScriptedIo};
use remora::{buffers::*, config::Limits};
use std::time::Duration;

fn packet(pool: &BufferPool, length: usize, direction: Direction) -> Datagram {
    let clock = FakeClock::new();
    Datagram::new(
        pool.acquire().unwrap(),
        0,
        length,
        direction,
        Generation(1),
        Generation(2),
        clock.now(),
        clock.now() + Duration::from_millis(20),
    )
    .unwrap()
}
#[test]
fn slab_capacity_stays_charged_until_trim_and_reuse_is_zeroed() {
    let pool = BufferPool::new(128, 256).unwrap();
    let mut slab = pool.acquire().unwrap();
    slab.bytes_mut().fill(0xab);
    let other = pool.acquire().unwrap();
    assert!(matches!(pool.acquire(), Err(BudgetError::Exhausted)));
    assert_eq!(
        pool.retained(),
        Usage {
            count: 2,
            bytes: 256
        }
    );
    drop(slab);
    assert_eq!(pool.retained().bytes, 256);
    let returned = pool.acquire().unwrap();
    assert!(returned.bytes().iter().all(|b| *b == 0));
    drop(returned);
    drop(other);
    pool.trim_idle();
    assert_eq!(pool.retained(), Usage::default());
}
#[test]
fn empty_datagram_offsets_deadlines_and_generation_metadata() {
    let pool = BufferPool::new(128, 256).unwrap();
    let clock = FakeClock::new();
    let empty = Datagram::new(
        pool.acquire().unwrap(),
        128,
        0,
        Direction::ToClient,
        Generation(3),
        Generation(4),
        clock.now(),
        clock.now(),
    )
    .unwrap();
    assert_eq!(empty.bytes(), b"");
    assert_eq!(empty.session(), Generation(3));
    assert_eq!(empty.connection(), Generation(4));
    assert_eq!(empty.capacity(), 128);
    drop(empty);
    assert!(
        Datagram::new(
            pool.acquire().unwrap(),
            usize::MAX,
            1,
            Direction::ToTarget,
            Generation(1),
            Generation(1),
            clock.now(),
            clock.now()
        )
        .is_err()
    );
    assert!(
        Datagram::new(
            pool.acquire().unwrap(),
            120,
            9,
            Direction::ToTarget,
            Generation(1),
            Generation(1),
            clock.now(),
            clock.now()
        )
        .is_err()
    );
    assert!(
        Datagram::new(
            pool.acquire().unwrap(),
            0,
            1,
            Direction::ToTarget,
            Generation(1),
            Generation(1),
            clock.now(),
            clock.now() - Duration::from_millis(1)
        )
        .is_err()
    );
}
#[test]
fn count_byte_and_checked_arithmetic_failures_leave_exact_accounting() {
    let budget = Budget::new(2, 128);
    let a = budget.reserve(1, 100).unwrap();
    assert!(matches!(budget.reserve(1, 29), Err(BudgetError::Exhausted)));
    assert!(matches!(budget.reserve(2, 0), Err(BudgetError::Exhausted)));
    assert!(matches!(
        budget.reserve(usize::MAX, 0),
        Err(BudgetError::Arithmetic)
    ));
    assert!(matches!(
        budget.reserve(0, usize::MAX),
        Err(BudgetError::Arithmetic)
    ));
    assert_eq!(
        budget.used(),
        Usage {
            count: 1,
            bytes: 100
        }
    );
    let b = budget.reserve(1, 28).unwrap();
    drop(a);
    assert_eq!(
        budget.used(),
        Usage {
            count: 1,
            bytes: 28
        }
    );
    drop(b);
    assert_eq!(budget.used(), Usage::default());
    assert_eq!(
        budget.peak(),
        Usage {
            count: 2,
            bytes: 128
        }
    );
}
#[test]
fn session_directions_share_bytes_but_have_separate_empty_packet_caps() {
    let pool = BufferPool::new(128, 1024).unwrap();
    let mut queue = SessionQueue::new(2, 128);
    queue.push(packet(&pool, 100, Direction::ToTarget)).unwrap();
    assert_eq!(
        queue.push(packet(&pool, 29, Direction::ToClient)),
        Err(BudgetError::Exhausted)
    );
    queue.push(packet(&pool, 28, Direction::ToClient)).unwrap();
    queue.push(packet(&pool, 0, Direction::ToTarget)).unwrap();
    assert_eq!(
        queue.push(packet(&pool, 0, Direction::ToTarget)),
        Err(BudgetError::Exhausted)
    );
    assert_eq!(
        queue.used(),
        Usage {
            count: 3,
            bytes: 128
        }
    );
    assert_eq!(queue.pop(Direction::ToTarget).unwrap().bytes().len(), 100);
    assert_eq!(
        queue.used(),
        Usage {
            count: 2,
            bytes: 28
        }
    );
    drop(queue);
    pool.trim_idle();
    assert_eq!(pool.retained().bytes, 0);
}
#[test]
fn native_queue_atomic_reservation_and_resource_permits() {
    let limits = Limits {
        quiche_dgram_packets: 1,
        connection_dgram_bytes: 128,
        ..Limits::default()
    };
    let native = ResourceBudgets::native_datagrams(&limits).unwrap();
    let first = native.reserve(Direction::ToTarget, 100).unwrap();
    assert!(native.reserve(Direction::ToClient, 29).is_err());
    assert_eq!(native.used_bytes(), 100);
    let second = native.reserve(Direction::ToClient, 28).unwrap();
    assert!(native.reserve(Direction::ToTarget, 0).is_err());
    drop(first);
    drop(second);
    assert_eq!(native.used_bytes(), 0);
    let budgets = ResourceBudgets::new(&limits, 1);
    let fd = budgets.fds.reserve(1, 0).unwrap();
    assert!(budgets.fds.reserve(1, 0).is_err());
    drop(fd);
    let setup: Vec<_> = (0..limits.max_pending_setup)
        .map(|_| budgets.setup.reserve(1, 0).unwrap())
        .collect();
    assert!(budgets.setup.reserve(1, 0).is_err());
    drop(setup);
    assert_eq!(budgets.setup.used().count, 0);
}
#[test]
fn deterministic_clock_scripted_partial_progress_and_errors() {
    let mut clock = FakeClock::new();
    let before = clock.now();
    clock.advance(Duration::from_millis(20));
    assert_eq!(
        clock.now().duration_since(before),
        Duration::from_millis(20)
    );
    let mut io = ScriptedIo::new([
        IoStep::Accepted(2),
        IoStep::WouldBlock,
        IoStep::Interrupted,
        IoStep::Accepted(1),
        IoStep::HardError,
    ]);
    assert_eq!(io.send(3).unwrap(), 2);
    assert_eq!(
        io.send(1).unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    assert_eq!(
        io.send(1).unwrap_err().kind(),
        std::io::ErrorKind::Interrupted
    );
    assert_eq!(io.send(1).unwrap(), 1);
    assert!(io.send(1).is_err());
    assert_eq!(io.accepted(), 3);
}

#[test]
fn ciphertext_preserves_original_metadata_and_retained_capacity_is_separate() {
    let pools = Pools::new(&Limits::default(), 1200, 1350, 65536).unwrap();
    let clock = FakeClock::new();
    let from = "127.0.0.1:4000".parse().unwrap();
    let to = "127.0.0.1:4001".parse().unwrap();
    let mut slab = pools.ciphertext.acquire().unwrap();
    slab.bytes_mut()[..3].copy_from_slice(b"abc");
    let output = Ciphertext::new(
        slab,
        3,
        from,
        to,
        clock.now() + Duration::from_millis(2),
        Generation(1),
        7,
        clock.now(),
        clock.now() + Duration::from_millis(100),
    )
    .unwrap();
    assert_eq!(output.bytes(), b"abc");
    assert_eq!(output.from(), from);
    assert_eq!(output.to(), to);
    assert_eq!(output.connection(), Generation(1));
    assert_eq!(output.sequence(), 7);
    assert_eq!(
        output.at().duration_since(output.generated_at()),
        Duration::from_millis(2)
    );
    assert_eq!(
        output.expires_at().duration_since(output.generated_at()),
        Duration::from_millis(100)
    );
    assert_eq!(pools.ciphertext.retained().bytes, 1350);
    assert!(pools.ciphertext.metadata_capacity_bytes() > 0);
    pools.ciphertext.trim_idle();
    assert_eq!(pools.ciphertext.retained().bytes, 1350);
    drop(output);
    pools.ciphertext.trim_idle();
    assert_eq!(pools.ciphertext.retained().bytes, 0);
    assert_eq!(pools.ciphertext.metadata_capacity_bytes(), 0);
    let mut generations = Generations::default();
    assert_eq!(generations.next_generation().unwrap(), Generation(1));
    assert_eq!(generations.next_generation().unwrap(), Generation(2));
}
proptest::proptest! {
    #[test]
    fn arbitrary_quota_requests_never_exceed_bounds(requests in proptest::collection::vec((0usize..8, 0usize..100), 0..100)) {
        let budget = Budget::new(32, 512); let mut reservations = Vec::new();
        for (count, bytes) in requests { if let Ok(r) = budget.reserve(count, bytes) { reservations.push(r); }
            proptest::prop_assert!(budget.used().count <= 32 && budget.used().bytes <= 512);
        }
        drop(reservations); proptest::prop_assert_eq!(budget.used(), Usage::default());
    }
}
