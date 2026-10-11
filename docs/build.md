# Building Remora

Phase 2 supplies validated startup configuration, bounded buffer/resource
ownership, safe diagnostic primitives and isolated test fixtures. The binary
still exits with an error; command wiring and forwarding arrive in Phase 3.
Only implemented foundation behavior has test coverage.

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

Default builds omit qlog; `--all-features` validates compilation of optional
qlog support. No runtime qlog writer is implemented or activated by this flag.
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
`remora check-config` is still a planned Phase 3 command; foundation tests call
the typed library loader without binding or contacting peers.

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
