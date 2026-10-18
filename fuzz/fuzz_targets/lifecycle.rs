#![no_main]
use libfuzzer_sys::fuzz_target;
use remora::{
    buffers::*,
    policy::{ResolvedTarget, SetupKey},
    session::{MappingLatch, RetrySchedule},
};
use remora_fuzz::config::ValidatedConfig;
use std::{
    path::Path,
    sync::LazyLock,
    time::{Duration, Instant},
};
static CONFIG: LazyLock<(ValidatedConfig, ValidatedConfig)> = LazyLock::new(|| {
    let path = std::env::var("REMORA_FUZZ_CONFIG").expect("use scripts/validate/fuzz.sh");
    let server = Path::new(&path);
    (
        ValidatedConfig::load(&server.with_file_name("client.toml")).unwrap(),
        ValidatedConfig::load(server).unwrap(),
    )
});
fuzz_target!(|data: &[u8]| {
    remora_fuzz::reactor::lifecycle_harness::check(data, &CONFIG.0, &CONFIG.1);
    let mut rejected = false;
    let pool = BufferPool::new(64, 512).unwrap();
    let mut queue = SessionQueue::new(4, 256);
    let mut generations = Generations::default();
    let mut generation = generations.next_generation().unwrap();
    let mut latch = MappingLatch::default();
    let mut retry = RetrySchedule::new(100, 2000);
    let now = Instant::now();
    for (step, op) in data.iter().take(1024).enumerate() {
        let at = now + Duration::from_millis(step as u64);
        match op % 7 {
            0 => {
                if latch.may_setup()
                    && let Ok(mut slab) = pool.acquire()
                {
                    slab.bytes_mut().fill(*op);
                    let packet = Datagram::new(
                        slab,
                        0,
                        usize::from(*op) % 65,
                        Direction::ToTarget,
                        generation,
                        generation,
                        at,
                        at + Duration::from_millis(20),
                    )
                    .unwrap();
                    let _ = queue.push(packet);
                }
            }
            1 => {
                if let Some(packet) = queue.pop(Direction::ToTarget) {
                    assert_eq!(packet.session(), generation);
                    assert_eq!(packet.connection(), generation);
                }
            }
            2 => {
                queue = SessionQueue::new(4, 256);
                generation = generations.next_generation().unwrap();
            }
            3 => {
                rejected = true;
                latch.reject();
            }
            4 => {
                retry.schedule(at, u64::from(*op));
                assert!(retry.deadline().unwrap() >= at + Duration::from_millis(100));
            }
            5 => {
                let old = Generation(generation.0.saturating_sub(1));
                let resolved = ResolvedTarget {
                    key: SetupKey {
                        connection: old,
                        session: old,
                    },
                    address: "127.0.0.1:9999".parse().unwrap(),
                };
                assert!(
                    resolved
                        .for_current(SetupKey {
                            connection: generation,
                            session: generation
                        })
                        .is_err()
                );
            }
            _ => {
                retry.started();
                pool.trim_idle();
            }
        }
        assert_eq!(latch.may_setup(), !rejected);
        assert!(queue.used().count <= 4 && queue.used().bytes <= 256);
        assert!(pool.retained().bytes <= 512);
    }
    drop(queue);
    pool.trim_idle();
    assert_eq!(pool.retained().bytes, 0);
});
