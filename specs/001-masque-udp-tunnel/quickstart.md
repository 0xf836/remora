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
effective off/not_implemented without a probe. Optional qlog now persists only reconstructed transport metadata; raw upstream
and keylog output are never enabled. SIGTERM/SIGINT release listeners and terminate owned packet work.

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

## Phase 5 native development and reference method

After locked release builds for the root and test-only traffic package:

```bash
./scripts/validate/profile.sh --output benchmarks/results/profile-NEW
./scripts/validate/network.sh --output benchmarks/results/network-NEW
./scripts/validate/bench.sh --profile benchmarks/reference-x86_64.toml \
  --plan-only --output benchmarks/results/reference-plan-NEW
./scripts/validate/bench.sh --profile benchmarks/reference-x86_64.toml \
  --development-smoke --smoke-selected \
  --output benchmarks/results/reference-smoke-NEW
```

Profiling runs sequential low-rate parameter cases and freezes its reference
before selecting an acceleration. The network runner uses only anonymous owned
user/network namespaces and recorded veth/netem/feature settings. Host-state
mutation is not part of setup. Smoke checks loaded warmup/1-or-32-source plumbing
and deliberately produces an invalid **release** criterion result because its
intervals/search/repetitions/churn are incomplete; it must still complete schema
and process/integrity checks without an execution failure.

The full method command below is the later T077 qualification job, not a quick
smoke: 18 baseline searches plus 185 full baseline/tunnel pairs and churn can
take at least 37 hours even before searches/churn. Owned namespace holders
remain alive until explicit runner teardown, without a one-hour expiry. It
requires native x86_64, ip/tc/nsenter/unshare, read-only
feature-query support, >=10 available logical cores for disjoint four-core role
placement plus tools, a built traffic executable and generator headroom. Failed
or invalid calibration cannot establish Remora capacity.

```bash
./scripts/validate/bench.sh --profile benchmarks/reference-x86_64.toml \
  --output benchmarks/results/reference-full-NEW
```

Keep each output path fresh and retain rejected/failed artifacts. No raw
credentials/keys belong in Git. [Performance](../../docs/performance.md),
[network characterization](../../docs/validation/network.md) and
[Phase 5 validation](../../docs/validation/phase5.md) distinguish native checks,
retrospective development acceptance and the unchanged full release criteria.

## Phase 6 recovery and signal validation

Build the locked production binary and pinned independent Go fixture first.
Use fresh output directories; the runners provision their own private fixtures.
Recovery requires Linux user/network namespaces, iproute2/netem and ethtool.
It changes only its owned veth paths and removes its owned processes/namespaces.
Shutdown uses ordinary loopback and includes the Go peer withholding acceptance.

```bash
./scripts/validate/e2e.sh --scenario recovery \
  --peer-binary /tmp/remora-interop --output benchmarks/results/recovery-NEW
./scripts/validate/e2e.sh --scenario shutdown \
  --peer-binary /tmp/remora-interop --output benchmarks/results/shutdown-NEW
```

Recovery covers ten-second blackhole/restart, address replacement, thirty-second
outage, revoked credential, denied target, changed certificate CA, missing peer
capability and nine-second application idle. Timing starts at the actual fault
and continuous restored availability; tagged data verifies no outage replay and
the listener inode remains identical. Fatal initial permanent rejection exits 1;
permanent rejection after an accepted Session retains the denied mapping until
restart. SIGINT/SIGTERM are checked during Starting, Active, Recovering and held
setup, with process/listener release within the shared five-second budget.
See [recovery evidence](../../docs/validation/recovery.md) and
[ADR-0010](../../docs/adr/0010-client-recovery-and-draining.md).

For a future critical-path change, collect and freeze a fresh pre-change
reference/policy before editing, then use the same retained traffic executable:

```bash
./scripts/validate/bench.sh --profile benchmarks/recovery-x86_64.toml \
  --traffic-binary benchmarks/results/phase6-reference/reference-traffic \
  --output benchmarks/results/recovery-candidate-NEW
python3 scripts/validate/compare.py \
  --reference benchmarks/results/phase6-prechange-valid \
  --candidate benchmarks/results/recovery-candidate-NEW \
  --policy benchmarks/results/phase6-reference/comparison-policy.json \
  --output benchmarks/results/recovery-candidate-NEW/comparison.json
```

Those saved reference paths describe this increment's local artifacts and must
exist before reuse. A new clone must create its own reference, not fabricate the
missing files. This short low-rate loaded comparison measures enabled liveness
checks; idle probe behavior has separate functional evidence. Neither result
qualifies idle CPU cost, sustained capacity, final release resources or ARM64.

## Phase 7 operator walkthrough (T064–T070)

See [operations](../../docs/operations.md) for secure provisioning, candidate
validation, controlled update/revocation and deliberate rollback. See
[troubleshooting](../../docs/troubleshooting.md) for status boundaries, fixed
classes and collection gaps. The examples contain no secret material.

```bash
cargo build --locked --release --features qlog
./scripts/validate/operations.sh --binary target/release/remora \
  --output /tmp/remora-phase7-operations-new
```

Run as an ordinary user with a fresh output path. The walkthrough installs the
exact executable into its own user directory, establishes byte-identical UDP
request/reply traffic and collects auth/policy/trust/resource/size/socket/connection
faults with zero status coverage loss. It also runs deterministic DNS/setup
completion, controlled policy-generation/rollback and status/qlog pressure tests.
Private certificates/tokens and child cleanup remain runner-owned; retained
records contain only sanitized evidence. The manifest distinguishes the copied
release executable from the compiler-reported all-feature debug process-test
binary and hashes the exact source/locks. [Recorded evidence](../../docs/validation/operations.md)
identifies actual commands/results and limitations.

Optional runtime tracing requires `--features qlog` and an empty mode-0700
role-owned directory. It uses a typed JSON-SEQ filter before any persistence,
shares the ordinary output budget and cannot consume reserved status capacity.
It is disabled by default. No raw H3/header/frame/payload/keylog data is written.
Native ARM64, final-revision release/performance qualification and T071
**diagnostic-mode overhead measurements** remain outstanding. T071 was explicitly
excluded from this implementation invocation.


## T071 diagnostics-mode measurements

The original measurement-only invocation kept production Rust unchanged. The
[performance record](../../docs/performance.md#phase-7-t071-diagnostics-measurement-method--2026-10-13)
contains immutable inputs, frozen policy and exact reproduction commands through
`bench.sh --profile benchmarks/diagnostics.toml --diagnostics-stage
reference|freeze|candidate`. Pass explicit preserved Remora/traffic binaries;
use fresh output directories. Reference must precede policy freeze, which must
precede candidate collection. Do not mutate reference/bounds after measuring.

Four modes compare ordinary detail off (mandatory status remains), default,
verbose and active filtered-qlog, at 100pps per active direction, one Session,
128-byte payloads and five five-second runs in each direction. Separate unread
stdout cases validate actual writer blocking and continued traffic with explicit
incomplete collection. Normal mode gaps or stopped qlog invalidate coverage;
blocked collection is never SC-010 success. 70/70 same-binary repeatability checks pass;
T071 was pending at this same-binary checkpoint; the later cross-revision cycle below supplies scoped acceptance. These preserved
results do not qualify the executable after subsequent review corrections;
high-rate capacity/trace overhead, ARM64 and final release remain separate.


## T071 cross-revision acceptance cycle

Use the [cross-revision reproduction procedure](../../docs/performance.md#reproduce-the-cross-revision-cycle).
`diagnostic_inputs.py --prepare-baseline` snapshots a precise pre-Phase-7 revision
and the proposed sources into fresh local artifacts. Collect the baseline with
`bench.sh --diagnostics-kind regression --diagnostics-stage reference`, then
freeze the pinned plan before candidate reconstruction/build. Use fresh paths
and explicit build-input manifests for both executables. Each role requires one
of four distinct physical cores and `taskset` (util-linux); live worker masks are
checked. CPU power settings are recorded without changing the host.

Reference repeats interleave modes/directions over five rounds. Candidate pairs
alternate baseline/candidate order. A baseline outside its frozen variability
range invalidates acceptance; retain its failure and limits. The original
same-binary results and first baseline-drift failure remain historical evidence.
The corrected final cycle passes all 70 paired checks; see the [final report](../../docs/performance.md#final-t071-cross-revision-results).
