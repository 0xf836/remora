# Quickstart validation guide

Phase 2 adds the validated configuration library, finite resource/buffer ownership, safe diagnostic primitives and fixture generator to Phase 1 build infrastructure; see [build instructions](../../docs/build.md) and [foundation evidence](../../docs/validation/phase2.md). The binary still exits with an explicit not-implemented error: command wiring, traffic tools and forwarding runners below remain planned. Revised [A1/A2](proposed-amendments.md) are accepted and the design-readiness gate passes. Early x86_64 E2E MVP/baseline runs precede full native-platform and numerical qualification. No command applies changes to a production server or configures an existing VPN.

## Prerequisites

Linux 6.1+ x86_64 or ARM64; Rust 1.99.0 from the pinned toolchain; C/C++, CMake and Clang/libclang for the BoringSSL build; Bash, Python 3 standard library and OpenSSL for test fixtures. Interop needs pinned Go 1.27.2. Foundation tests use unprivileged loopback ports. Delay/loss/MTU/WireGuard tests require a separate runner with network-namespace capabilities; Remora itself runs as an ordinary user.

## Build and static/component checks

From the repository root (available starting with Phase 1; behavior coverage grows with implementation):

```bash
cargo build --locked --release
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
```

Start these checks natively on x86_64 for implemented functionality. Add ARM64 native checks during development and complete them before claiming ARM64 release support; ARM64 availability/results do not block initial x86_64 development. Cross compilation alone does not qualify the ARM64 native library/syscall path. Fuzz smoke and scheduled longer jobs follow the benchmark contract; record corpus/toolchain and treat crashes as failed gates.

## Generate isolated fixtures

Implemented `scripts/validate/fixtures.sh` creates a mode-0700 output directory, a local CA, server certificate with DNS SAN remora.test, mode-0600 key/token/credential files, and client/server TOML. It provisions a random token and canonical-text digest without putting either in argv/logs. Client, proxy and authorized target use distinct ephemeral loopback ports recorded in `ports.json`, with an exact per-identity target permission. The script refuses any existing output. Ports are held during generation only; future process runners must handle bind collisions with fresh fixtures.

```bash
./scripts/validate/fixtures.sh --output /tmp/remora-validation
```

The following `check-config` commands are planned for T024; current foundation
tests validate both generated configurations through the library loader:

```bash
./target/release/remora check-config --config /tmp/remora-validation/server.toml
./target/release/remora check-config --config /tmp/remora-validation/client.toml
```

Expected once command wiring exists: valid schema/trust/secret files accepted, no sockets/peer traffic during check-config. The implemented library already rejects bad permissions, unknown fields and invalid static policy shapes. Runtime authentication and DNS authorization remain Phase 3 work.

## Generic UDP flow

Run responder, server and client in separate terminals once implemented. The examples below use the contract's illustrative fixed ports; substitute the generated `ports.json` target port when using ephemeral fixtures. The test-only traffic tool interfaces remain planned:

```bash
cargo run --locked --release --manifest-path tools/traffic/Cargo.toml -- responder --listen 127.0.0.1:9000
./target/release/remora server --config /tmp/remora-validation/server.toml
./target/release/remora client --config /tmp/remora-validation/client.toml
```

Then run the automated unprivileged smoke runner:

```bash
./scripts/validate/e2e.sh --fixtures /tmp/remora-validation --scenario smoke
./scripts/validate/e2e.sh --fixtures /tmp/remora-validation --scenario concurrency
```

Expected: independent tagged payloads return unchanged with correct datagram boundaries, including empty datagrams; startup first-packet drop is documented and test traffic retries at its application layer after acceptance. 32 admitted sessions remain isolated; unauthorized identity/target emits zero monitored target traffic. Run negative trust/token/size/partial-I/O and sentinel tests, and retain machine-readable results.

At the first MVP checkpoint, include initial GOAWAY cutoff/new-request refusal/accepted-flow continuation tests in the transport suite. Concurrency, recovery and other dependency-ready development can continue after checked T034 while an independent peer or prepared WireGuard fixture remains unavailable. Record those qualification tasks as pending, publish only actual MVP evidence, and complete the required compatibility evidence before the associated full story/release claim. Unavailable external qualification does not excuse a failing check of implemented behavior.

```bash
./scripts/validate/e2e.sh --fixtures /tmp/remora-validation --scenario security
./scripts/validate/e2e.sh --fixtures /tmp/remora-validation --scenario recovery
./scripts/validate/e2e.sh --fixtures /tmp/remora-validation --scenario shutdown
```

Recovery scenario covers a ten-second blackhole or server restart, preserved listener, no replay, repeated authentication and the exact timing origins in SC-006. Permanent rejection remains denied after source-tuple churn, flow cleanup and transient reconnect; only a controlled client restart replaces the mapping admission latch. Shutdown releases resources <=5 seconds. The lifecycle component tests must include two streams, EAGAIN/partial send and close on both sides of the packet-generation commitment boundary. Uncommitted data is discarded; already generated output can finish only on its original association within its unchanged 100 ms residence, cannot be recalled by local close, and is discarded with old connection state on replacement/stop. No output is replayed into a new generation. Test the two-minute idle default with sparse traffic and prove that transport keepalives do not refresh application inactivity. A1 tests require rejection of identifiable closed-session datagrams, queued responses and late setup work, with zero cross-client delivery for active associations and retained data during closure/recreation. Characterize and document the chosen reuse strategy and the ambiguity of externally indistinguishable replies after identical UDP tuple reuse/restart; do not claim a fixed quarantine guarantee.

## Independent interop

```bash
./scripts/validate/interop.sh --fixtures /tmp/remora-validation --peer-ref c1cf0e4dd6439aea94d5491b27439a4736f00246 --direction both
```

Build the pinned Go fixture with committed module sums, verify certificate trust, and test Remora-client->masque-go-proxy and masque-go-client->Remora-server. Expected: both-role byte equality and supported-profile negative cases, including forbidden final Capsule statuses/fields and mandatory Context ID/oversized Capsule checks before carrier discard; preserve peer versions/settings/results. API inspection in research is not a completed test. No InsecureSkipVerify or raw authentication dump. Configure the pinned Transport.DialAddr callback to dial the numeric loopback proxy endpoint while TLSClientConfig retains ServerName=remora.test and the generated RootCAs. This avoids system DNS or /etc/hosts changes and preserves normal certificate verification.

## Reference benchmarks and network cases

A planned benchmark runner consumes a predeclared manifest and writes immutable results. It validates hardware/topology/generator headroom before claiming a valid run. Follow [benchmark contract](contracts/benchmarks.md); do not substitute a shorter direct path, report only simultaneous sum, or use echo traffic as true bidirectional capacity.

```bash
./scripts/validate/bench.sh --manifest benchmarks/reference-x86_64.toml --output /tmp/remora-bench-x86_64
```

Reference manifests/scripts do not exist yet. Build the baseline/load harness with the first working E2E MVP, record CPU/kernel/NIC/path/dependency/settings, and start reproducible direct-versus-tunneled measurements on x86_64 immediately. Early measurements may have declared partial workload coverage and unmet targets; they guide continued engineering and are not full release acceptance results. For release qualification use prospectively recorded reference settings, >=five pairs, one/32 sessions, 128/512/1200 bytes and separate/simultaneous directions; each applicable SC threshold must pass per run. Add ARM64 native evidence before claiming its release support, without making it a prerequisite for x86_64 work. No separate prototype or current throughput/loss/latency/RSS result is asserted.

After initial measurements and before critical-path changes, record the pre-change Remora revision/reference-manifest, metric bounds with units, variability and comparison method under the benchmark contract's prospective regression policy. Run affected paired comparisons before accepting the changed increment; missing/unmatched evidence is inconclusive/invalid, and failed comparisons remain visible. Initial baseline runs need no regression tolerance and make no regression claim. Requested/effective GSO/GRO modes and fixed fallback reasons are part of run records; deferred `auto` means ordinary I/O with `not_implemented`, not a claimed kernel capability failure.

Later, as the relevant functionality and runners become available, run full reference/network cases and native ARM64 qualification. These commands are release-validation work, not prerequisites for the initial x86_64 MVP:

```bash
./scripts/validate/network.sh --fixtures /tmp/remora-validation --scenario impairment
./scripts/validate/bench.sh --manifest benchmarks/reference-arm64.toml --output /tmp/remora-bench-arm64
```

## Independently managed WireGuard validation

The operator first prepares existing WireGuard peers, keys, routes, firewall and a permitted target UDP port outside Remora. Configure the client target to the server's permitted WireGuard endpoint and use Remora's local listener as the local WireGuard peer endpoint. Document an application MTU compatible with the measured effective payload ceiling; do not assume 1200-byte application payload fits every outer path. Maintain ordinary bidirectional traffic for 30 minutes and verify handshake/data with independent WireGuard counters alongside Remora events. Remora neither edits peer configuration nor manages routes/TUN. Generic UDP and WireGuard evidence are both required by SC-001.

## Revocation, diagnostics and cleanup

Use [config/update contract](contracts/cli-config.md) for private file replacement, candidate validation and controlled restart. Verify old process has exited and replacement is ready, then prove removed credentials/policies cannot create new authorized sessions or cause new forbidden relay emission. Attribute already committed/kernel/network in-flight pre-update traffic separately; a restart cannot recall it. Collect stdout snapshots and stderr events using [diagnostics contract](contracts/diagnostics.md); distinguish queue/kernel drops from outer QUIC loss and destination silence. SC-010 collection includes short-lived terminal failures after ordinary event suppression; require every injected class/session and zero status coverage loss. Deliberate blocked-sink/terminal-overflow tests verify bounded behavior and explicit missing coverage, not a diagnostic-acceptance pass. Optional filtered qlog is later work and must not delay the first E2E MVP. SIGTERM/SIGINT test processes; runner-owned cleanup must leave no listener/child/namespace and must never delete operator files. Retain failed manifests/results for review.
