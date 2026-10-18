# Building Remora

Remora has one production Rust package and one executable with client, server
and `check-config` commands. The selected native Linux GNU x86_64 CUBIC build
has [functional and performance qualification](validation/release.md). Native
Linux GNU x86_64 is the sole current required platform. [ARM64 is deferred](adr/0024-defer-arm64-release-scope.md)
outside completion/release gates. Its [manual-only workflow](../.github/workflows/arm64.yml)
and [reference profile](../benchmarks/reference-arm64.toml) remain experimental;
neither cross-compilation nor prepared tooling constitutes a support claim.
Existing Cargo target hints and the architecture list in `--version` describe
build compatibility, not additional qualified platforms.

## Pinned inputs and prerequisites

- Rust **1.99.0**, edition 2024, rustfmt and Clippy: `rust-toolchain.toml`.
- quiche **0.30.0** from the project fork, exact Git commit
  `154ce1995f23efa434e72dac269f707145be2015`; Cargo manifests and lockfiles pin it.
  The minimum-payload correction and upstream issue are recorded in
  [dependency provenance](../dependencies/README.md) and
  [ADR 0016](adr/0016-quiche-minimum-payload.md). Use the commit pin rather than
  a moving branch. No quiche source tree is vendored into the production package.
- Tokio **1.53.2**, boring/boring-sys **5.2.0**, other resolved versions/checksums:
  `Cargo.lock`. BoringSSL is built from boring-sys's bundled source; host OpenSSL
  is not the production TLS implementation. The optional upstream qlog subcrate
  remains at registry **0.18.1** and octets at **0.3.7**.
- Linux **6.1+**, GNU target, C/C++ compiler and linker, CMake, Clang/libclang
  for bindgen, and pkg-config. Native builds need these already installed.
- Tests/runners need Bash, Python **3.11+** standard library, OpenSSL CLI and
  ordinary loopback UDP bind permission. OpenSSL provisions test certificates.
  Namespace/network jobs additionally need util-linux, iproute2 and ethtool;
  [VM prerequisites](validation/local-kvm.md) and Go are separate.

Cargo needs registry/Git access on the first fetch, or a populated verified
cache. Keep the checked-in lockfiles; normal builds must not regenerate them.
Record source revision, tool versions, lock/source hashes and binary SHA-256
with results. A rebuild of another revision is not qualified by an old hash.

## Build and native checks

From the repository root, rustup selects the pinned toolchain:

```bash
cargo fetch --locked
python3 scripts/validate/quiche_inputs.py
cargo build --locked --release
./target/release/remora --version
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
```

`quiche_inputs.py` verifies the actual Cargo Git checkout, revision and source
bytes. Do not edit that cache. Use `CARGO_NET_OFFLINE=true` only after locked
inputs are present. Tests create isolated loopback fixtures; ordinary package
checks do not start VMs or change host networking. Some independent-peer tests
are ignored until their explicitly documented prerequisites are supplied.
A passing ordinary test suite does not imply those tests ran.

Default builds omit filtered qlog support. To include it:

```bash
cargo build --locked --release --features qlog
sha256sum target/release/remora
```

The selected qualification executable was built with this feature, with runtime
tracing inactive for the reference benchmark. Its SHA-256 is
`7f7a3c4de7341b8edf0860ff3c8457642ff423cf4658f9aa93076db678cfe3b1`.
This hash identifies the measured build, not a universal reproducible-build
promise. Runtime tracing additionally requires a fresh empty private directory;
see [operations](operations.md#optional-filtered-qlog).

The traffic tool is a separate unpublished test package with its own lockfile:

```bash
cargo build --locked --release --manifest-path tools/traffic/Cargo.toml
cargo fmt --manifest-path tools/traffic/Cargo.toml --all -- --check
cargo clippy --locked --manifest-path tools/traffic/Cargo.toml --all-targets -- -D warnings
cargo test --locked --manifest-path tools/traffic/Cargo.toml
```

For the independent peer, use Go **1.27.2** and committed module sums. The
fixture pins masque-go v0.6.0 at
`c1cf0e4dd6439aea94d5491b27439a4736f00246` and quic-go v0.63.0:

```bash
(cd tools/interop && go build -mod=readonly -o /tmp/remora-interop . && go vet ./...)
```

It is test-only; see [its interface](../tools/interop/README.md). The
[quickstart](../specs/001-masque-udp-tunnel/quickstart.md) supplies forwarding and
independent-peer commands. Full qualification also includes the pinned quiche
source tests, Python runner tests, operator/network/recovery suites and VM
duration work; see the [release runner contract](validation/release.md#runner-and-continuation-contract).

## Pinned container checks

The [Dockerfile](../tools/build/Dockerfile) pins the Rust multi-platform image
by digest and additional Debian packages to the 2026-10-01 snapshot, with APT
signature verification. The package inventory is stored inside the image at
`/usr/local/share/remora-build-packages.txt`. Archive it and the exact image ID
with a container build result; an image tag alone does not identify the build.

```bash
docker build --pull -f tools/build/Dockerfile -t remora-build .
docker run --rm --user "$(id -u):$(id -g)" \
  --env CARGO_HOME=/tmp/cargo \
  --mount "type=bind,source=$PWD,target=/work" remora-build \
  bash -euc '
    cargo fetch --locked
    python3 scripts/validate/quiche_inputs.py
    cargo build --locked --release
    cargo fmt --all -- --check
    cargo clippy --locked --all-targets --all-features -- -D warnings
    cargo test --locked --all-targets --all-features
  '
```

Docker/registry access is a build prerequisite, not a runtime privilege for
Remora. The container does not install packages on the host. Native x86_64
[CI configuration](../.github/workflows/ci.yml) uses the pinned image; hosted CI
execution is separate evidence. The selected Phase 8 results are native x86_64
results, not a new run of every historical container configuration.

## Test-only safety and fuzz tooling

The production compiler remains pinned above. ASan fuzzing and Miri use
**nightly-2026-10-12**; install that test toolchain with `rust-src,miri` and
cargo-fuzz **0.13.1** before invoking the wrappers. Each output path must be new:

```bash
./scripts/validate/fuzz.sh 30 benchmarks/results/fuzz-NEW
./scripts/validate/safety.sh benchmarks/results/safety-NEW
./scripts/validate/dependencies.sh --help
```

The [fuzz package](../fuzz/README.md) documents the source-included lifecycle
adapter and its scope. [Safety](validation/safety.md) and
[dependency audit](validation/dependencies.md) retain actual checks and warning
limits. Miri does not qualify Linux syscalls or BoringSSL, and short ASan runs
do not establish exhaustive coverage. Advisory results are dated evidence;
repeat the audit for a proposed new dependency/release input set.

## Local artifacts

Credentials and test CA/key material are private generated inputs. The fixture
generator refuses to overwrite an existing directory:

```bash
./scripts/validate/fixtures.sh --output /tmp/remora-validation-NEW
```

Ports are reserved only during generation; use fresh fixtures if later occupied.
Generated results live under ignored local directories and are absent from a
fresh clone. Keep failures and input hashes; obtain the matching evidence bundle
or create a new run rather than fabricating missing artifacts. See
[artifact retention](validation/README.md). Early setup/container milestones
remain in [Phase 2](validation/phase2.md) and [MVP](validation/mvp.md) reports.
