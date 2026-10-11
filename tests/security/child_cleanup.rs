use crate::common::ChildGuard;
use std::{
    fs,
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant},
};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let mut random = [0; 16];
        getrandom::fill(&mut random).unwrap();
        let suffix: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
        let path = std::env::temp_dir().join(format!("remora-child-test-{suffix}"));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn wait_for_pid(path: &Path) -> u32 {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(text) = fs::read_to_string(path)
            && let Ok(pid) = text.trim().parse()
        {
            return pid;
        }
        assert!(Instant::now() < deadline, "owned child did not start");
        thread::sleep(Duration::from_millis(5));
    }
}

fn running(pid: u32) -> bool {
    // A killed orphan can remain a zombie until the container/host's init
    // reaps it. Zombies have exited and cannot retain sockets or execute.
    let Ok(stat) = fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return false;
    };
    !stat.rsplit_once(") ").unwrap().1.starts_with(['Z', 'X'])
}

fn assert_stopped(pid: u32) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while running(pid) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }
    let still_running = running(pid);
    if still_running {
        // Regression failures must not leave their own sleeping descendant.
        let _ = rustix::process::kill_process(
            rustix::process::Pid::from_raw(pid as i32).unwrap(),
            rustix::process::Signal::KILL,
        );
    }
    assert!(!still_running, "owned descendant survived guard cleanup");
}

fn spawn_descendant(path: &Path, tail: &str) -> ChildGuard {
    ChildGuard::spawn(
        Command::new("sh")
            .arg("-c")
            .arg(format!("sleep 30 & printf '%s\\n' \"$!\" > \"$1\"; {tail}"))
            .arg("remora-owned-fixture")
            .arg(path),
    )
    .unwrap()
}

#[test]
fn successful_and_failed_leaders_cleanup_descendants_before_reaping() {
    let scratch = Scratch::new();
    let unrelated = ChildGuard::spawn(Command::new("sleep").arg("30")).unwrap();
    for code in [0, 7] {
        let path = scratch.0.join(format!("descendant-{code}"));
        let mut guard = spawn_descendant(&path, &format!("exit {code}"));
        let pid = wait_for_pid(&path);
        let status = guard.finish(Duration::from_secs(5)).unwrap();
        assert_eq!(status.code(), Some(code));
        assert_stopped(pid);
        assert_eq!(guard.finish(Duration::ZERO).unwrap(), status);
        drop(guard);
        assert!(running(unrelated.id()), "unrelated group was signalled");
    }
}

#[test]
fn timeout_cleans_owned_group_and_drop_reaps_leader() {
    let scratch = Scratch::new();
    let mut guard = spawn_descendant(&scratch.0.join("descendant"), "wait");
    let leader = guard.id();
    let pid = wait_for_pid(&scratch.0.join("descendant"));
    assert_eq!(
        guard.finish(Duration::ZERO).unwrap_err().kind(),
        std::io::ErrorKind::TimedOut
    );
    drop(guard);
    assert_stopped(pid);
    assert!(!Path::new(&format!("/proc/{leader}")).exists());
}

#[test]
fn dropping_running_guard_cleans_descendant_and_reaps_leader() {
    let scratch = Scratch::new();
    let guard = spawn_descendant(&scratch.0.join("descendant"), "wait");
    let leader = guard.id();
    let pid = wait_for_pid(&scratch.0.join("descendant"));
    drop(guard);
    assert_stopped(pid);
    assert!(!Path::new(&format!("/proc/{leader}")).exists());
}

#[test]
fn invalid_deadline_error_still_cleans_owned_group_on_drop() {
    let scratch = Scratch::new();
    let mut guard = spawn_descendant(&scratch.0.join("descendant"), "wait");
    let leader = guard.id();
    let pid = wait_for_pid(&scratch.0.join("descendant"));
    assert_eq!(
        guard.finish(Duration::MAX).unwrap_err().kind(),
        std::io::ErrorKind::InvalidInput
    );
    drop(guard);
    assert_stopped(pid);
    assert!(!Path::new(&format!("/proc/{leader}")).exists());
}
