# Building Remora

The Phase 1 package is build infrastructure. Its binary exits with an error;
client/server commands and forwarding arrive in later increments. Empty domain
test entry points establish registration and do not provide behavior coverage.

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
