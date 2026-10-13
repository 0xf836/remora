# Building Remora

Phase 3 provides the client/server/check-config executable, authenticated UDP
forwarding and separate test traffic/interoperability tools. See the
[MVP validation](validation/mvp.md) for verified scope and the
[quickstart](../specs/001-masque-udp-tunnel/quickstart.md) for a working walkthrough.
The earlier Phase 1/2 records below remain historical evidence.

## Inputs

- Rust 1.99.0, edition 2024, rustfmt and Clippy: `rust-toolchain.toml`.
- quiche 0.30.0, Tokio 1.53.2, and transitive Rust/native sources: `Cargo.lock`.
  quiche's default backend resolves to boring/boring-sys 5.2.0. BoringSSL is
  built from boring-sys's bundled source, not the host OpenSSL library.
- Linux GNU x86_64 or ARM64, kernel 6.1+ for runtime qualification. Start with
  native x86_64; ARM64 checks remain required before claiming ARM64 support.
- C/C++ compiler, CMake, Clang/libclang (bindgen), pkg-config, and a linker.
  The pinned image supplies them; no system package changes are needed on
  operator hosts. Native builds need equivalent prerequisites already installed.
- Foundation tests require Bash, Python 3 (standard library), OpenSSL CLI and
  permission to bind unprivileged loopback UDP sockets. The pinned image supplies
  these tools. OpenSSL is test-only certificate provisioning; production PEM
  validation uses the existing BoringSSL backend.

The [Dockerfile](../tools/build/Dockerfile) pins the official Rust image by
multi-platform digest and Debian package resolution to the 2026-10-01 snapshot.
The base image fixes the C/C++ toolchain; the snapshot fixes added CMake and
Clang packages. APT signature verification stays enabled. The image records
its exact package versions in `/usr/local/share/remora-build-packages.txt`.
Archive that inventory, the image ID, tool versions, source revision, lock hash,
and build commands with release evidence. An image tag alone is insufficient.

## Native checks

With rustup, the repository selects its pinned toolchain automatically. A
system toolchain must match Rust 1.99.0 and provide rustfmt/Clippy. From the root:

```bash
cargo build --locked --release
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
```

Default builds omit qlog. `--all-features` validates its optional build path;
activation additionally requires a fresh private `diagnostics.qlog_directory`.
Only reconstructed metadata is persisted. See [operations](operations.md#optional-filtered-qlog).
Commit lockfile changes deliberately; ordinary builds must not regenerate it.

## Pinned container checks

```bash
docker build --pull -f tools/build/Dockerfile -t remora-build .
docker run --rm --user "$(id -u):$(id -g)" \
  --env CARGO_HOME=/tmp/cargo \
  --mount "type=bind,source=$PWD,target=/work" remora-build \
  bash -euc '
    cargo build --locked --release
    cargo fmt --all -- --check
    cargo clippy --locked --all-targets --all-features -- -D warnings
    cargo test --locked --all-targets --all-features
  '
```

Docker/registry access is a build prerequisite, not a Remora runtime privilege.
Dependencies are downloaded with Cargo checksum verification; the container
does not modify host package configuration. The same procedure runs in
[native x86_64 CI](../.github/workflows/ci.yml). Pinned inputs make rebuilds
reviewable; bit-for-bit reproducibility requires separate verification.

Network namespaces, MTU/impairment, WireGuard, independent peers, benchmarks,
and native ARM64 validation are separate qualification jobs. See the
[quickstart](../specs/001-masque-udp-tunnel/quickstart.md) and
[tasks](../specs/001-masque-udp-tunnel/tasks.md) for prerequisites and sequencing.

The implemented fixture generator can be run independently:

```bash
./scripts/validate/fixtures.sh --output /tmp/remora-validation
```

The output must not already exist. Generated ports are recorded in `ports.json`;
they are distinct at generation but are not held after the script exits.
Protect the private output directory and remove only fixtures you own.
`remora check-config --config PATH` validates startup inputs without binding,
DNS or contacting peers. The test-only traffic package has its own lockfile;
build it separately with `cargo build --locked --release --manifest-path tools/traffic/Cargo.toml`.
`./scripts/validate/mvp.sh` includes its checks. The independent Go fixture uses
Go 1.27.2 and committed module sums; see `tools/interop/README.md`.

## Phase 1 verification

On 2026-10-11, native Linux GNU x86_64 and the pinned Docker image passed
the build, formatting, Clippy and test commands above. Container checks used
an unprivileged UID, a read-only populated Cargo registry cache, `--offline`
for build/Clippy/test, and a separate target directory. No dependency resolution
or lockfile changes occurred during these checks. The image supplied GCC
12.2.0, CMake 3.25.1 and Clang/libclang 14; Rust was 1.99.0.

All five integration entry points and library/binary test targets executed
with **zero tests**. This verifies setup and native dependency compilation,
not forwarding, security, interoperability, performance or ARM64 behavior.
The skeleton binary was checked to exit with status 1 and an explicit
not-implemented diagnostic. Hosted GitHub CI has not been run from this session.

## Phase 2 verification

On 2026-10-11, native x86_64 and the updated pinned build image passed the same
locked release, formatting, Clippy and all-feature test commands, with **33
foundation tests**. Python 3/OpenSSL and local unprivileged socket permission
are required by the generated fixtures. See the [full Phase 2 evidence](validation/phase2.md)
for inputs, scope, failure checks and remaining qualification. The CLI binary
still has no forwarding or check-config command implementation.

After the independent Phase 2 review, the child-cleanup fix and parser properties
passed the same native/container commands with **45 tests**. The
[resolution evidence](validation/phase2.md#review-finding-resolution) preserves
the original 33-test results and distinguishes the corrections from a repeated
independent audit.

## Phase 3 verification

On 2026-10-11 native x86_64 and the pinned x86_64 container passed the locked
MVP checks. The initial 87-test set was followed by a real encrypted HTTP
rejection/cleanup regression, bringing the production package to 90 tests with additional RESET and stateless Initial
admission test and one separately invoked external interoperability test;
the traffic package has two accounting tests. Verification and partial
qualification are recorded in [MVP evidence](validation/mvp.md), with
[independent peer results](validation/interop.md) and
[first measurements](performance.md). Native ARM64, hosted Actions and full
release matrices have not run.
