//! Exercise the retry policy used by real receive/send/timerfd adapters.
use super::retry_interrupted;
use std::io;

#[test]
fn persistent_interruption_returns_control_after_four_attempts() {
    // The explicit guard makes the old unbounded loop fail without hanging CI.
    for _ in 0..2 {
        let mut attempts = 0;
        let result = retry_interrupted::<usize>(|| {
            attempts += 1;
            assert!(attempts <= 4, "interrupted syscall monopolizes the owner");
            Err(io::Error::from_raw_os_error(libc::EINTR))
        });
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Interrupted);
        assert_eq!(attempts, 4);
    }
}

#[test]
fn interruption_can_succeed_on_the_last_attempt_without_replaying_a_prefix() {
    for accepted in [0, 1, 3] {
        let mut attempts = 0;
        let result = retry_interrupted(|| {
            attempts += 1;
            match attempts {
                1..=3 => Err(io::Error::from_raw_os_error(libc::EINTR)),
                4 => Ok(accepted),
                _ => panic!("accepted prefix was replayed"),
            }
        });
        assert_eq!(result.unwrap(), accepted);
        assert_eq!(attempts, 4);
    }
}

#[test]
fn readiness_and_hard_errors_return_immediately_after_interruption() {
    for errno in [libc::EAGAIN, libc::ENOBUFS, libc::EMSGSIZE, libc::EBADF] {
        let mut attempts = 0;
        let result = retry_interrupted::<usize>(|| {
            attempts += 1;
            Err(io::Error::from_raw_os_error(if attempts == 1 {
                libc::EINTR
            } else {
                assert_eq!(attempts, 2, "non-interrupted error was retried");
                errno
            }))
        });
        assert_eq!(result.unwrap_err().raw_os_error(), Some(errno));
        assert_eq!(attempts, 2);
    }
}
