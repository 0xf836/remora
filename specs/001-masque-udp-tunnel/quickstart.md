# Quickstart and qualification guide

Remora provides client/server/check-config commands for an opaque UDP mapping
over HTTP/3 CONNECT-UDP and QUIC DATAGRAM. The selected native Linux GNU x86_64
CUBIC profile has completed T076 functional, T077 numerical and T078 review
qualification. [Current results](../../docs/performance.md) identify the exact
build/workload and limitations. Linux GNU x86_64 is the sole required platform;
[ARM64/T072 is deferred](../../docs/adr/0024-defer-arm64-release-scope.md) outside
current completion requirements.
The operator owns application, WireGuard, routes, firewall and trust provisioning.

For a deployment quickstart and all configuration settings, see the public
[README](../../README.md#usage), [configuration](../../docs/configuration.md)
and [TLS](../../docs/tls.md) guides. Generated validation fixtures below use the same application configuration
format and unprivileged local ports.

## Prerequisites and checks

Use Rust 1.99.0 with the lockfiles, Linux 6.1+ GNU, native C/C++/CMake/Clang tools,
Python 3.11+, Bash, OpenSSL CLI and permission to bind ordinary loopback UDP.
The [build guide](../../docs/build.md) includes pinned inputs, container checks,
the exact quiche fork and separate safety/fuzz tooling. Commands here run from
the repository root. Select new output paths for each run.

```bash
cargo fetch --locked
python3 scripts/validate/quiche_inputs.py
cargo build --locked --release --features qlog
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
cargo build --locked --release --manifest-path tools/traffic/Cargo.toml
cargo test --locked --manifest-path tools/traffic/Cargo.toml
```

The selected reference used a qlog-enabled build with runtime tracing inactive.
Default builds may omit the feature. `--all-features` compiles support without
activating a writer. Ordinary package tests do not start VMs; tests needing the
independent peer remain explicitly ignored unless enabled. All results belong
to their recorded source/binary inputs.

## Private fixtures and actual forwarding

Fixtures create mode-0700 directories with private token/trust/certificate
files, exact target permission and three distinct ephemeral loopback ports.
The directory must not exist. Ports are held during generation only; retry
with a new fixture if another process occupies them. Test certificates are
short-lived and are not deployment trust material.

```bash
./scripts/validate/fixtures.sh --output /tmp/remora-validation-NEW
./target/release/remora check-config --config /tmp/remora-validation-NEW/server.toml
./target/release/remora check-config --config /tmp/remora-validation-NEW/client.toml
./scripts/validate/e2e.sh --fixtures /tmp/remora-validation-NEW --scenario smoke
./scripts/validate/e2e.sh --fixtures /tmp/remora-validation-NEW --scenario security
```

Smoke owns its responder/roles, exchanges byte-identical 0/1/128/512/1200-byte
messages from distinct sources and cleans its processes. Security uses owned
private fixtures and monitored negative cases; neither result is a throughput
or 30-minute duration claim. To generate IPv6 fixtures:

```bash
./scripts/validate/fixtures.sh --output /tmp/remora-validation-v6-NEW --family ipv6
```

For an existing application, adapt the [client](../../examples/client.toml) and
[server](../../examples/server.toml) templates. Set the client listener, numeric
public `proxy = "https://host[:port]"`, CA trust, optional numeric `connect_address` and exact
target host/port. Provision the client token and server canonical-text digest
and target address ranges privately using [operations](../../docs/operations.md#install-and-provision).
Start each role on its own host/terminal after candidate validation:

```bash
remora check-config --config /absolute/private/server.toml
remora server --config /absolute/private/server.toml >server.status.ndjson 2>server.events.ndjson
# On the client host/terminal:
remora check-config --config /absolute/private/client.toml
remora client --config /absolute/private/client.toml >client.status.ndjson 2>client.events.ndjson
```

`check-config` performs no bind, DNS or network readiness check. A bound listener,
verified transport/settings, accepted CONNECT-UDP and observed application reply
are different milestones. Readiness probes must be safe for the application
to retry: setup and outage packets can be dropped and Remora does not replay.
Default listeners are loopback; exposing one requires explicit config and your
network access policy. Secrets never belong in argv, URLs or diagnostic output.

## Wire, lifecycle and operational guarantees

CA/name verification precedes credential transmission. Authentication and
all-answer target authorization precede relay allocation. A Session pins one
approved numeric target and a connected socket; accepted request streams
multiplex on a client mapping's transport connection. Live sender/local-destination
pairs stay distinct. No application forwarding precedes acceptance; the server
uses a conservative ACK-backed response commitment boundary.

UDP datagram bytes/boundaries, including empty messages, are preserved within
the current capacity. The default application limit is 1400 bytes and outer
cap 1452; actual path/native framing limits can be smaller. There is no private
fragmentation or reliable payload fallback. Capsules are parsed for control
conformance, not used as a reliable application carrier. Queue pressure,
oversize, expiry and unavailable-state drops have fixed directional counters.

Valid UDP activity retains a Session for the default 120000 ms; transport probes
do not extend application inactivity. Application/native queue residence is
20 ms and generated ciphertext 100 ms. Close purges cancellable work but cannot
recall generated mixed ciphertext or kernel/network traffic. Replacement/stop
discards old unsent output; identifiable stale work is never reassigned or replayed.
External UDP cannot distinguish a delayed reply after identical tuple reuse or
restart. No absolute freshness or fixed quarantine duration is promised.

Current defaults remain CUBIC, timerfd pacing, mmsg and GSO/GRO off. Deferred
`auto` reports effective off/`not_implemented`. Socket buffer requests are 1 MiB
outer and 128 KiB application/relay per RX/TX request; inspect actual effective
kernel values in status. See [limits and migration](../../docs/operations.md#limits-payload-size-and-lifecycle),
[controlled update/revocation](../../docs/operations.md#controlled-update-revocation-and-rollback)
and the [wire contract](contracts/masque-wire.md).

Config, trust and policy are loaded once. Revocation completes at old-process
exit plus replacement readiness; it requires controlled stop/start. A terminal
mapping rejection blocks new setup until restart while unrelated accepted flows
can continue. SIGINT/SIGTERM stop owned packet work and release listeners within
the tested five-second budget. Diagnostics distinguish observed failures from
UDP silence and expose incomplete collection; ordinary log suppression does
not suppress mandatory status. [Troubleshooting](../../docs/troubleshooting.md)
explains classes, codes and correlation limits.

## Independent compatibility and operator walkthrough

Use Go 1.27.2 with committed module sums. The fixture pins masque-go v0.6.0
at the ref below and quic-go v0.63.0; normal CA/name validation stays enabled:

```bash
(cd tools/interop && go build -mod=readonly -o /tmp/remora-interop . && go vet ./...)
./scripts/validate/interop.sh --fixtures /tmp/remora-validation-NEW \
  --peer-ref c1cf0e4dd6439aea94d5491b27439a4736f00246 \
  --direction both --peer-binary /tmp/remora-interop \
  --output /tmp/remora-interop-NEW
./scripts/validate/operations.sh --binary target/release/remora \
  --output /tmp/remora-operations-NEW
```

The runner creates isolated per-case fixtures and checks both role directions,
IPv4/IPv6 payload boundaries and exact monitored refusal classes/statuses.
[Historical independent cases](../../docs/validation/interop.md) and
[selected T076 evidence](../../docs/validation/release.md) identify their inputs.
The ordinary-user operator walkthrough covers installation, real traffic,
correlated faults, controlled policy generations/rollback and diagnostic pressure.
It does not deploy a service or edit host networking.

Filtered tracing additionally needs a fresh empty mode-0700 role-owned directory
in `diagnostics.qlog_directory`; it persists reconstructed metadata, not raw
qlog/H3/payload/keylog data. See [qlog operation](../../docs/operations.md#optional-filtered-qlog).
Correctness/privacy tests and earlier low-rate T071 overhead measurements do not
qualify current high-rate active tracing overhead.

## Concurrent-session qualification

These unprivileged loopback jobs provision their own fixtures and must run
sequentially with fresh outputs:

```bash
./scripts/validate/e2e.sh --scenario concurrency --suite isolation \
  --output benchmarks/results/concurrency-isolation-NEW
./scripts/validate/e2e.sh --scenario concurrency --suite overload \
  --output benchmarks/results/concurrency-overload-NEW
./scripts/validate/e2e.sh --scenario concurrency --suite churn \
  --output benchmarks/results/concurrency-churn-NEW
```

Defaults are 1800 s isolation, 600 s overload and 1000 churn cycles. Shorter
`--seconds`/`--cycles` are development checks. Four clients/two credentials/two
targets exercise 32 sources plus independent reverse traffic. Overload protects
admitted low-rate probes while saturating configured budgets; it is not a link
capacity or Internet attack guarantee. Test-only idle settings are 2000 ms
for isolation/overload and 500 ms for churn, not production defaults. Deliberate
collector overflow cannot establish complete diagnostic correlation.

## Phase 6 recovery and signal validation

Build the independent peer first. Recovery/network jobs need owned Linux
user/network namespaces, util-linux, iproute2/netem and ethtool; ordinary runtime
roles remain unprivileged. The tools remove their own paths/processes only:

```bash
./scripts/validate/network.sh --scenario all --output benchmarks/results/network-NEW
./scripts/validate/e2e.sh --scenario recovery \
  --peer-binary /tmp/remora-interop --output benchmarks/results/recovery-NEW
./scripts/validate/e2e.sh --scenario shutdown \
  --peer-binary /tmp/remora-interop --output benchmarks/results/shutdown-NEW
```

Cases include blackhole/server restart, address replacement, extended outage,
revoked credentials/target/trust/capability and held setup. They verify timing
from actual faults/restored availability, listener preservation and no outage
replay. See [recovery](../../docs/validation/recovery.md) for exact evidence.

## WireGuard and performance qualification

The [WireGuard guide](../../docs/wireguard.md) describes independent peers,
MTU, private fixture and opt-in two-VM command. Only a passing 1800-second run
qualifies duration; a handshake or ten-second smoke does not. The selected
T076 run has 72000/72000 measured generic/WG messages with warmup separate.
Remora does not provision keys, routes, TUN or firewalls.

For numerical reproduction, follow [performance](../../docs/performance.md#reproduce-the-current-numerical-profile).
The current bounded-reference-v2 cycle has 18 searches, 75 pairs and 1000 churn
cycles under a 90-minute deadline. It is separate from long functional/VM
qualification. The prior schema-2 37-hour method is historical. A plan is read-only:

```bash
./scripts/validate/bench.sh --profile benchmarks/reference-x86_64.toml \
  --plan-only --output benchmarks/results/reference-plan-NEW
./scripts/validate/release.sh --plan-only --peer-binary /tmp/remora-interop \
  --output /tmp/remora-functional-plan-NEW
```

Full functional execution requires the prepared WG fixture and input-bound
T073/T074/T075 reports from the [release runner contract](../../docs/validation/release.md#runner-and-continuation-contract).
Do not reuse old prerequisite hashes for a new source set. The full reference
must run without competing builds/tests; its driver needs the complete intended
role/tool affinity. Short smoke deliberately has incomplete release coverage.
CUBIC loss amplification, tail/headroom limits, native ARM64, active high-rate
qlog and enabled offloads remain as documented limitations rather than implied
passes from the clean x86_64 reference.

## Cleanup and evidence

Use SIGINT/SIGTERM for manually started roles. Runners clean owned children;
VM test cleanup is ownership-checked, with explicit recovery instructions if
interrupted cleanup cannot finish. Delete only private fixtures/results you own
and no longer need; retain failed runs and raw manifests/counters/hashes needed
for review. Raw local bundles are absent from a fresh checkout. Do not publish
keys, tokens or private fixture files.

Requirements checklists stay reviewer-owned/read-only. Task completion in
[tasks](tasks.md) describes the implemented/verified scope; experimental manual-only ARM64 tooling
is outside this scope. Local qualification does not imply a hosted CI run,
deployment or release tag.
