# MVP quickstart and qualification guide

Phase 3 implements authenticated generic UDP forwarding, its Linux I/O and
protocol/security/lifecycle tests, separate test traffic tools and the first
paired x86_64 measurements. See [MVP evidence](../../docs/validation/mvp.md),
[performance](../../docs/performance.md) and [tasks](tasks.md) for actual
coverage and remaining release work. Planning readiness is not runtime acceptance.
These commands do not deploy a server or configure an existing VPN.

The [review correction record](../../docs/validation/review-remediation.md)
identifies the latest verified increment. Explicit wildcard listeners retain
each packet's local destination as the reply source; sender/local-destination
pairs consume separate bounded sessions. On shutdown, pending terminal status
is collected independently of periodic cadence within a shared one-second
deadline. An unfinished drain/writer returns a nonzero exit; a blocked sink
does not receive a delivery guarantee. See the [diagnostics contract](contracts/diagnostics.md).

## Prerequisites and checks

Start on Linux x86_64 (kernel 6.1+), Rust 1.99.0, C/C++, CMake and Clang/libclang
for bundled BoringSSL. Test fixtures require Bash, Python 3.11+ standard library,
OpenSSL and ordinary loopback UDP permission. The pinned image includes these
prerequisites; see [build instructions](../../docs/build.md). Native ARM64 remains
required before an ARM64 support claim and does not block x86_64 development.

```bash
cargo build --locked --release
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
cargo build --locked --release --manifest-path tools/traffic/Cargo.toml
cargo test --locked --manifest-path tools/traffic/Cargo.toml
```

`./scripts/validate/mvp.sh` runs the complete implemented-package checks,
including separate traffic formatting and Clippy. Use `CARGO_NET_OFFLINE=true`
only with a populated, checksum-verified cache. `--all-features` checks compile
support; it does not activate a runtime qlog writer.

## Private fixtures and actual forwarding

Choose a fresh directory; fixtures refuse to overwrite existing output.
Generation creates private credentials, local CA/certificate for remora.test,
exact target permission and three ephemeral loopback ports. No token enters
argv/logs. Ports are held during generation only; retry with fresh fixtures if
another process later occupies them. Certificates are short-lived test inputs.

```bash
./scripts/validate/fixtures.sh --output /tmp/remora-validation
./target/release/remora check-config --config /tmp/remora-validation/server.toml
./target/release/remora check-config --config /tmp/remora-validation/client.toml
./scripts/validate/e2e.sh --fixtures /tmp/remora-validation --scenario smoke
./scripts/validate/e2e.sh --fixtures /tmp/remora-validation --scenario shutdown
./scripts/validate/e2e.sh --fixtures /tmp/remora-validation --scenario security
```

`check-config` validates schema, private-file modes, trust and static rules
without binding, DNS or peer traffic. Smoke/shutdown own a responder and both
roles, verify exact 0/1/128/512/1200-byte datagrams from two distinct senders,
and clean their processes. Security runs the real-process monitored negatives
with its own private fixtures. Full concurrency/recovery scenarios explicitly
report later-phase unavailability rather than a pass.

Generate IPv6 fixtures with the same commands and a different fresh path:

```bash
./scripts/validate/fixtures.sh --output /tmp/remora-validation-v6 --family ipv6
./scripts/validate/e2e.sh --fixtures /tmp/remora-validation-v6 --scenario shutdown
```

For an existing UDP application, configure the client mapping's local listener,
proxy trust/authority and target host/port; configure the server's exact
credential/host/port/address rule as described in the
[CLI/config contract](contracts/cli-config.md). Start the two roles in separate
terminals with `remora server --config PATH` and `remora client --config PATH`.
Provision credentials privately; keep the client secret out of argv, URLs and
public examples. Private files are loaded once; updates/revocation require
candidate check-config and controlled stop/start, not hot reload.

## Supported MVP behavior

The client sends credentials only after CA/name verification and required H3
settings. The server authorizes every resolved address and pins one approved
address to a distinct connected relay. No application payload is forwarded
before valid client acceptance and the server's conservative ACK-backed
acceptance receipt. Initial application packets and packets during setup are
dropped/countable; readiness probes retry only in the test application.

Datagrams retain exact bytes/boundaries, including empty messages. Configured
application maximum defaults to 1200 bytes and is constrained by current QUIC
DATAGRAM capacity/path limits. Oversize/backpressure/expired traffic is counted
and dropped without truncation, private fragmentation or reliable fallback.
Capsules are parsed for conformance, never used for application forwarding.
Default application/native residence is 20 ms; generated ciphertext has a
100-ms original-generation FIFO deadline and obeys quiche pacing metadata.
An individual stream close purges cancellable data but cannot rewrite or recall
already generated mixed ciphertext or kernel/network traffic. Stop discards
unsent original-generation output; no new-generation replay occurs.

Exclusive state ownership, opaque generations and connected relay sockets
isolate live/internal associations. External UDP provides no generation tag
for an indistinguishable late reply after identical tuple reuse/restart.
No fixed quarantine promise is made. Initial GOAWAY tests preserve accepted
lower streams and refuse excluded/new requests; replacement/recovery is later
work. Permanent auth/policy/capability rejection retains a mapping latch until
controlled restart. See [wire contract](contracts/masque-wire.md) and
[ADR-0006](../../docs/adr/0006-mvp-acceptance-and-ownership.md).

Stdout contains bounded JSON status, stderr fixed diagnostic events. Never infer
a destination error solely from UDP silence. Terminal failures survive ordinary
log suppression until writer acknowledgement; explicit coverage-loss counters
invalidate complete-correlation claims after deliberate overflow/broken sinks.
GSO/GRO off/auto are accepted: off reports configured_off, deferred auto reports
effective off/not_implemented without a probe. No raw upstream/qlog/keylog output
is enabled. SIGTERM/SIGINT release listeners and terminate owned packet work.

## Independent peer and performance reproduction

Build the separate pinned Go 1.27.2 fixture; normal CA/name validation remains
active. Module sums pin masque-go v0.6.0 at the requested commit and quic-go
v0.63.0. No system DNS or /etc/hosts edit is needed for the numeric proxy dial.

```bash
(cd tools/interop && go build -mod=readonly -o /tmp/remora-interop . && go vet ./...)
./scripts/validate/interop.sh --fixtures /tmp/remora-validation \
  --peer-ref c1cf0e4dd6439aea94d5491b27439a4736f00246 \
  --direction both --peer-binary /tmp/remora-interop --output /tmp/remora-interop-results
./scripts/validate/bench.sh --profile benchmarks/mvp-x86_64.toml \
  --output /tmp/remora-mvp-benchmark
```

Both output paths must be fresh. The interop runner validates supplied fixtures
and provisions isolated per-case fixtures; see [actual independent evidence](../../docs/validation/interop.md)
for passed cases and pending peer-profile cases. Same-stack contract tests are
recorded separately. Preserve failed runs, manifests and logs.

The first benchmark is one 128-byte session at 1000 pps per active direction,
two-second warmup, three-second measurement and two pairs on the same-host
matching two-role path. It measures independent separate/simultaneous directions,
unique delivery/loss, sparse RTT, per-role CPU/RSS and owner headroom.
It makes no sustainable capacity or full SC claim. The versioned
[MVP development bounds](../../benchmarks/policies/mvp-development.json)
are bound to a pre-change reference before later critical-path changes using
the commands in [performance](../../docs/performance.md). Matched per-run bounds
must pass and missing/invalid comparisons remain visible. Do not run competing builds/tests
during comparisons. Full 1/32-session, five-pair/five-minute, size/impairment,
resource/churn and native ARM64 matrices remain later tasks; their planned
reference manifests and namespace runners must not be treated as existing tools.

## Remaining qualification and cleanup

The [local two-VM operator tool](../../docs/validation/local-kvm.md) prepares a
dedicated fixture. T037's 30-minute WG/generic qualification passed on the
recorded x86_64 profile with exact bidirectional delivery and verified cleanup;
final-revision release qualification remains separate. Remora does not change
keys/routes/firewalls or TUN state. Follow [WireGuard interface/MTU guidance](../../docs/wireguard.md)
and [application compatibility evidence](../../docs/validation/application-compatibility.md).
Installed WireGuard tools or a successful handshake alone do not qualify SC-001.
VM qualification is explicitly opt-in through `libvirt-lab.sh test`; ordinary
build/unit/CI checks do not start guests. The tool reuses a verified shared
image cache and cleans up its disposable guests/network/storage on exit.

Requirements-quality checklists remain reviewer-owned and read-only during
implementation. Completed tasks reflect checked increments; partial or missing
release evidence stays explicit. T034 permits dependency-ready development
without waiting for the named external qualifications. This does not waive a
failing security/correctness test or change numeric release targets.

Stop your manually started roles with SIGTERM/SIGINT. Delete only private
fixtures/results that you created and no longer need. Runners clean only their
own processes/directories and never modify operator networking.

## Concurrent-session qualification (Phase 4)

The unprivileged owned-loopback runner provisions fresh private fixtures itself;
`--fixtures` is not needed for these scenarios. Build the production release
first. Use fresh ignored output paths and preserve failures:

```bash
./scripts/validate/e2e.sh --scenario concurrency --suite isolation \
  --output benchmarks/results/concurrency-isolation
./scripts/validate/e2e.sh --scenario concurrency --suite overload \
  --output benchmarks/results/concurrency-overload
./scripts/validate/e2e.sh --scenario concurrency --suite churn \
  --output benchmarks/results/concurrency-churn
```

Defaults are 1800 seconds of isolation, 600 seconds of saturation and 1000 churn
cycles. `--seconds`/`--cycles` select shorter development coverage, never full
qualification. Four client processes, two credentials, two destinations and
32 source sockets exercise shared/distinct routing and independent reverse
messages. Saturation adds a separate admitted load flow so opaque probes do not
compete with excess traffic inside their own application association. It also
attempts newest-request admission and malformed outer traffic. The path is
responsive loopback with low wire load; no reference throughput claim follows.

Test-only idle overrides are 2000 ms for flow expiry/recreation and 500 ms for
churn. These intentionally deviate from RFC 9298's SHOULD solely to accelerate
lifecycle tests. Production remains 120000 ms; shorter operator values require
a deployment rationale and can cause repeated setup/source-port changes/first
packet loss. See [concurrency evidence](../../docs/validation/concurrency.md)
and [ADR-0008](../../docs/adr/0008-concurrent-admission-and-fair-service.md).
