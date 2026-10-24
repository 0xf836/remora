//! Runner-owned fixtures only; no operator networking or existing files touched.
#![allow(dead_code)] // Shared support is used incrementally by separate test bins.
use std::{
    collections::VecDeque,
    fs, io,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

pub struct FakeClock {
    now: Instant,
}
impl FakeClock {
    pub fn new() -> Self {
        Self {
            now: Instant::now(),
        }
    }
    pub fn now(&self) -> Instant {
        self.now
    }
    pub fn advance(&mut self, duration: Duration) {
        self.now = self.now.checked_add(duration).expect("test clock range");
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IoStep {
    Accepted(usize),
    WouldBlock,
    Interrupted,
    HardError,
}
pub struct ScriptedIo {
    steps: VecDeque<IoStep>,
    accepted: usize,
}
impl ScriptedIo {
    pub fn new(steps: impl IntoIterator<Item = IoStep>) -> Self {
        Self {
            steps: steps.into_iter().collect(),
            accepted: 0,
        }
    }
    /// Model an accepted prefix; the caller retains/retries the unchanged suffix.
    pub fn send(&mut self, available: usize) -> io::Result<usize> {
        match self.steps.pop_front().expect("script has an outcome") {
            IoStep::Accepted(n) if n <= available => {
                self.accepted += n;
                Ok(n)
            }
            IoStep::Accepted(_) => Err(io::ErrorKind::InvalidData.into()),
            IoStep::WouldBlock => Err(io::ErrorKind::WouldBlock.into()),
            IoStep::Interrupted => Err(io::ErrorKind::Interrupted.into()),
            IoStep::HardError => Err(io::ErrorKind::Other.into()),
        }
    }
    pub fn accepted(&self) -> usize {
        self.accepted
    }
}
pub struct ChildGuard {
    child: Child,
    reaped: bool,
    status: Option<ExitStatus>,
}
impl ChildGuard {
    pub fn spawn(command: &mut Command) -> io::Result<Self> {
        use std::os::unix::process::CommandExt;
        let child = command
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        Ok(Self {
            child,
            reaped: false,
            status: None,
        })
    }
    pub fn spawn_logged(command: &mut Command, stdout: &Path, stderr: &Path) -> io::Result<Self> {
        Self::spawn_with_io(
            command,
            fs::File::create(stdout)?.into(),
            fs::File::create(stderr)?.into(),
        )
    }
    pub fn spawn_with_io(command: &mut Command, stdout: Stdio, stderr: Stdio) -> io::Result<Self> {
        use std::os::unix::process::CommandExt;
        let child = command
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr)
            .spawn()?;
        Ok(Self {
            child,
            reaped: false,
            status: None,
        })
    }
    pub fn signal(&self, signal: rustix::process::Signal) -> io::Result<()> {
        if self.reaped {
            return Err(io::ErrorKind::NotFound.into());
        }
        let pid = rustix::process::Pid::from_raw(self.child.id() as i32)
            .ok_or(io::ErrorKind::InvalidInput)?;
        rustix::process::kill_process_group(pid, signal).map_err(Into::into)
    }
    pub fn id(&self) -> u32 {
        self.child.id()
    }
    fn kill_group(&mut self) -> io::Result<()> {
        if self.reaped {
            return Ok(());
        }
        let pid = rustix::process::Pid::from_raw(self.child.id() as i32)
            .ok_or(io::ErrorKind::InvalidInput)?;
        match rustix::process::kill_process_group(pid, rustix::process::Signal::KILL) {
            Ok(()) | Err(rustix::io::Errno::SRCH) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
    pub fn finish(&mut self, timeout: Duration) -> io::Result<ExitStatus> {
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or(io::ErrorKind::InvalidInput)?;
        if let Some(status) = self.status {
            return Ok(status);
        }
        let pid = rustix::process::Pid::from_raw(self.child.id() as i32)
            .ok_or(io::ErrorKind::InvalidInput)?;
        loop {
            // Exclusive wait ownership: never expose Child or use a concurrent
            // wait(-1) reaper. NOWAIT keeps the exited leader waitable, pinning
            // its PID/PGID until the owned group has been signalled. Reaping
            // first would either leak descendants or risk signalling a reused ID.
            use rustix::process::{WaitId, WaitIdOptions, waitid};
            match waitid(
                WaitId::Pid(pid),
                WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
            ) {
                Ok(Some(_)) => {
                    self.kill_group()?;
                    if let Some(status) = self.child.try_wait()? {
                        self.reaped = true;
                        self.status = Some(status);
                        return Ok(status);
                    }
                    return Err(io::ErrorKind::Other.into());
                }
                Ok(None) | Err(rustix::io::Errno::INTR) => {}
                Err(error) => {
                    // An external reaper violates exclusive ownership. Do not
                    // subsequently signal an identifier it may have released.
                    if error == rustix::io::Errno::CHILD {
                        self.reaped = true;
                    }
                    return Err(error.into());
                }
            }
            if Instant::now() >= deadline {
                self.kill_group()?;
                return Err(io::ErrorKind::TimedOut.into());
            }
            thread::sleep(Duration::from_millis(5));
        }
    }
}
impl Drop for ChildGuard {
    fn drop(&mut self) {
        // Bound cleanup even for a broken child. A process stuck in kernel I/O
        // cannot be promised reaped by a user-space deadline.
        let _ = self.kill_group();
        let _ = self.finish(Duration::from_secs(1));
    }
}
pub struct Fixtures {
    path: PathBuf,
    owned_root: PathBuf,
}
impl Fixtures {
    /// Remove only the generated machine profile when a case tests another one.
    pub fn without_diagnostics(&self, file: &str) -> String {
        fs::read_to_string(self.file(file))
            .unwrap()
            .replace("[diagnostics]\nformat = \"json\"\nsnapshots = true\n", "")
    }
    pub fn new() -> Self {
        let mut random = [0; 16];
        getrandom::fill(&mut random).unwrap();
        let suffix: String = random.iter().map(|b| format!("{b:02x}")).collect();
        use std::os::unix::fs::DirBuilderExt;
        let owned_root = std::env::temp_dir().join(format!("remora-test-{suffix}"));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&owned_root)
            .unwrap();
        let fixtures = Self {
            path: owned_root.join("fixtures"),
            owned_root,
        };
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/validate/fixtures.sh");
        let mut child = ChildGuard::spawn(
            Command::new("bash")
                .arg(script)
                .arg("--output")
                .arg(&fixtures.path),
        )
        .unwrap();
        let status = child
            .finish(Duration::from_secs(40))
            .expect("fixture generator deadline");
        assert!(
            status.success(),
            "fixture generator requires Python 3, OpenSSL and local UDP bind permission"
        );
        fixtures
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn file(&self, file: &str) -> PathBuf {
        self.path.join(file)
    }
    pub fn rewrite(&self, file: &str, from: &str, to: &str) {
        let path = self.file(file);
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains(from), "fixture rewrite must match");
        fs::write(path, text.replace(from, to)).unwrap();
    }
    /// Retain only the concurrency runner's bounded, non-secret JSON evidence.
    /// Configs, raw logs and all credential/trust fixtures are always deleted.
    #[allow(dead_code)] // This shared support is also compiled by other suites.
    pub fn retain_concurrency_failure(&self) -> io::Result<PathBuf> {
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-failures");
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&parent)?;
        let retained = parent.join(self.owned_root.file_name().unwrap());
        fs::DirBuilder::new().mode(0o700).create(&retained)?;
        for (relative, name) in [
            ("concurrency-result/manifest.json", "manifest.json"),
            ("concurrency-result/summary.json", "summary.json"),
            ("concurrency-result/failure.json", "failure.json"),
            ("forwarding-failure.json", "forwarding-failure.json"),
        ] {
            let source = self.file(relative);
            let mut file = match fs::File::open(source) {
                Ok(f) => f,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e),
            };
            // Bound reads too, including a file that grows after metadata().
            use std::io::Read;
            let mut bytes = Vec::new();
            (&mut file).take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
            if bytes.len() > 1024 * 1024 {
                return Err(io::Error::other("failure artifact exceeds limit"));
            }
            let value: serde_json::Value = serde_json::from_slice(&bytes)?;
            let target = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(retained.join(name))?;
            serde_json::to_writer_pretty(target, &value)?;
        }
        Ok(retained)
    }
}
impl Drop for Fixtures {
    fn drop(&mut self) {
        if std::thread::panicking()
            && (self.file("concurrency-result").is_dir()
                || self.file("forwarding-failure.json").is_file())
        {
            match self.retain_concurrency_failure() {
                Ok(path) => eprintln!("concurrency failure artifacts: {}", path.display()),
                Err(_) => eprintln!("could not retain bounded concurrency failure artifacts"),
            }
        }
        fs::remove_dir_all(&self.owned_root).expect("owned fixture cleanup");
    }
}

pub mod quic;
