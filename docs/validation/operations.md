# Phase 7 operations and diagnostics evidence

Date: 2026-10-13. Scope: T064–T070 of
[the task plan](../../specs/001-masque-udp-tunnel/tasks.md). **T071 was explicitly
excluded and remains unchecked.** This is functional/privacy/operator evidence,
not diagnostic overhead, native ARM64 or final release qualification.

## Inputs and attribution

Base Git revision: `34d4a8fa13aa67436e057773d1a3199b69aba233`, with the recorded
Phase 7 working-tree changes. The walkthrough records `dirty = true`, individual
source/lock hashes and exact executable hashes; the base commit alone does not
identify this candidate. Production source hashes were checked against the
retained manifest after execution. Documentation/task markers were finalized
later; no subsequent production behavior change is included in this report.

| Executable/input | SHA-256 |
|---|---|
| Default locked release build | `947cdfc9b98ae3b7ce1b7c8f8ddd3d15462783f4c013c429def018018f6080d8` |
| qlog-feature locked release / copied walkthrough install | `c30a4983d18def84f643eaafaa98079208a59fb12db4bc76e5911bee84a4742c` |
| Compiler-reported all-feature debug process-test executable | `9ee01f1e40f28d116b2b85f7a2237955a11e25c52a66bc6e7e33b684aa94eac3` |
| Cargo.lock | `73f95573f9fa739062f98e414be62f3e1cf06b80dec65d44b80cef587e251cc7` |
| Rebuilt pinned independent Go peer | `10e862de7d7f6c12a7db0486d935622494443f33f68c9ba859e41e56f2ee743e` |

Native host: Linux `7.2.9-arch1-1`, x86_64, effective UID 1000. Rust
1.99.0 / quiche 0.30.0, Python 3.14.8, OpenSSL 3.6.5. Go peer build reports
Go 1.27.2-X:nodwarf5, masque-go 0.6.0 and quic-go 0.63.0 with the existing module
locks/reference commit. These are actual local inputs, not a hosted CI result.

The final retained walkthrough is
`benchmarks/results/phase7-operations-20261013-r3/`. It contains `manifest.json`,
`summary.json`, copied user install, per-case sanitized stdout/stderr and
`checks-0.log` through `checks-4.log`. Generated artifacts remain ignored;
private temporary fixtures are cleaned, and no credentials/certificates are
copied into this public report. The separate interop run is
`benchmarks/results/phase7-interop-20261013/` with its own exact release/peer
hashes, source/lock hashes and 29-case summary.

## Commands and results

```bash
cargo build --locked --release
cargo build --locked --release --features qlog
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
python3 -m unittest discover -s tests -p 'test_*.py'
bash -n scripts/validate/operations.sh
python3 -m py_compile scripts/validate/operations.py
./scripts/validate/operations.sh --binary target/release/remora \
  --output benchmarks/results/phase7-operations-20261013-r3
```

Locked default and qlog release builds, rustfmt and Clippy with warnings denied
PASS. The final all-feature Rust run passed **165 tests**, with two external
checks ignored by the standard command. Counts: library 46, E2E 21, Linux
I/O/buffers 24, lifecycle 5, protocol 22 and security 47. Main has zero unit
tests, rather than fabricated coverage. All **42 Python tests** pass, including
four collector regressions. Shell/Python syntax and local documentation links
pass proportional static checks. The standard Rust command includes property,
parser, real syscall, resource, recovery, privacy and process tests; it does not
run a sustained fuzz campaign or substitute for final platform qualification.

Both ignored checks also passed explicitly against the rebuilt independent
peer (one Rust test each). The interop runner passed **29/29 native cases** in
both roles/families, including monitored unauthorized targets. Held authenticated
setup terminated on both SIGINT and SIGTERM, each with listener release; the
reported local stop observations were 5 ms, not a performance benchmark.

```bash
(cd tools/interop && go build -mod=readonly -o /tmp/remora-phase7-interop . && go vet ./...)
cargo test --locked --all-features --test protocol \
  --config 'env.REMORA_INTEROP_BIN="/tmp/remora-phase7-interop"' \
  --config 'env.REMORA_INTEROP_OUTPUT="/home/lesnick/projects/remora/benchmarks/results/phase7-interop-20261013"' \
  interop_cases::independent_go_roles_preserve_bytes_and_deny_monitored_negatives \
  -- --ignored --exact --nocapture
cargo test --locked --all-features --test e2e \
  --config 'env.REMORA_INTEROP_BIN="/tmp/remora-phase7-interop"' \
  shutdown::both_signals_cancel_awaiting_acceptance_setup_with_independent_peer \
  -- --ignored --exact --nocapture
```

## Clean install and collected failure matrix

The runner refuses root and an existing output path, copies the exact release
executable into `install/bin/remora`, provisions fresh owner-private fixtures,
validates both configs and starts ordinary-user loopback roles. The clean case
exchanged exact 0/1/128/512/1200-byte request/reply datagrams without application
changes. No firewall, routes, host DNS, WireGuard or supervisor deployment was
modified. Config validation occurs before binds/peer traffic; the separate
operations tests prove it can succeed with occupied role ports.

All **8/8 release scenarios** and **5/5 additional check groups** PASS:

| Release scenario | Collected boundary / result |
|---|---|
| clean-install | Bidirectional exact datagrams, acceptance visible, both exits 0 |
| authentication | `authentication/401`, opaque Session/connection, zero target messages |
| authorization | `authorization/403`, opaque Session/connection, zero target messages |
| trust | `trust/0`, connection attribution before Session creation where appropriate, zero target messages |
| resource | `resource/503` on refused requests with bounded opaque correlation; existing admitted sender remains usable |
| size | `size/3` for known-association local receive truncation, zero oversized target messages |
| socket | Actual Linux connected UDP ECONNREFUSED, `socket/111` with affected Session/connection |
| connection | Observed native `peer_timeout/0`, correlated original accepted Session, before cleanup |

All collected release cases have `coverage_lost = 0`,
`status_write_failures = 0` and no counter overflow. Secret/payload sentinels
are absent from retained diagnostics. Initial permanent auth/policy/trust
client exit 1 is the expected refusal result; server exit 0 remains required.
The configured error verbosity and ten-second snapshot interval suppress
ordinary detail and exercise terminal collection before periodic cadence.

Additional groups recorded in `checks-*.log`: operations **4/4**, process
diagnostics **3/3**, status writer **4/4**, qlog **9/9**, deterministic owner
setup-fault matrix **1/1**. The separate all-class writer test covers every fixed
failure class after event-bucket exhaustion and cleanup. Deterministic DNS/setup
completion injections produce `resolution/502`, `setup_timeout/504`,
`resource/503`, `authentication/401` and `authorization/403` with exact affected
opaque IDs, terminal flags and zero coverage loss. This proves owner integration;
it is not a claim that a live external resolver outage was induced.

The silent bound target test observes accepted Sessions without a socket-failure
record; closing that target produces actual `socket/111`. UDP silence is never
inferred to be a socket error or failed authorization.

## Privacy, bounds and controlled update

Partial one-byte JSON-SEQ input, malformed/new schema, enormous/nested arbitrary
input, H3 Authorization/header/payload/CID/reason/custom sentinels, all five
runtime log levels and arbitrary upstream trace targets are checked. No raw
qlog temporary or final file is created: persistence gets reconstructed typed
records only. A supplied `SSLKEYLOGFILE` path remains absent. Eight filters
reserve at most 512 KiB scratch; a ninth is denied. Rate accounting remains
shared across churn. Lifetime retention holds at 16 MiB/connection and
128 MiB/process even with deliberately dropped writer records. The 768 ordinary
records can be saturated by qlog while reserved status is still accepted and
selected first. Disk filename conflict disables tracing, preserves the existing
file and leaves status ACK working.

`qlog-build-gate.json` records a separate private-fixture check: the default
release rejects configured qlog with exit 2 / fixed `qlog_directory (Range)`;
the qlog-feature release accepts it with exit 0. Both validations create zero
files in the directory. All directory/filesystem checks remain startup/writer
work, outside packet forwarding.

Status tests separately verify distinct pending classes, repeats, cleanup,
old ACK/new summary, one status batch, first-chunk failure with later ACK, exact
lost sequence ranges, event reservation, broken and deliberately blocked sinks.
The deliberately overflowing ring retains 256 entries and exposes loss; it
**does not pass complete-correlation SC-010 coverage**. Histogram overflow is
machine-visible and invalidates numeric measurement.

Controlled update tests atomically replace mode-0600 credential/policy files,
check the candidate, prove the old immutable policy still works before its exit,
stop old roles and start the new generation. Removed credential and target
permission are denied with zero monitored destination messages after completion.
Epochs differ, new acceptances remain zero, and pre-existing in-flight traffic
is separately drained/attributed. Only deliberate restoration of the selected
old policy and another restart re-enable the grant. Examples validate after
private fixture provisioning. Owned children/directories are cleaned.

## Corrections and evidence limits

A status ACK regression was found and fixed: a successful final chunk could
retire summaries despite a failed first chunk. The new regression fails against
an isolated copy with the previous ACK-first order and passes on the corrected
working code. Pending summaries now survive the failed batch and are re-emitted.

The existing idle-process test assumed both peers independently probe within
its short 2500-ms application idle window. A peer probe legitimately generates
an ACK that renews the other endpoint's otherwise-idle send deadline. The test
now requires a probe on the connection, expiry on both application Sessions and
one unchanged client connection attempt. No production recovery policy changed.
Initial failing broad/isolated observations are not final passes.

The first walkthrough (`phase7-operations-20261013`) retained a trust-fixture
configuration failure: changing only server_name left authority/template
inconsistent and failed validation before TLS. The corrected fixture changes
those expected client identity fields consistently and requires actual trust/0.
The second run (`...-r2`) passed all release cases but hit a stale debug test
binary after the deliberate ACK regression used a shared Cargo target. The
working diagnostic source and build metadata were forced to recompile, all
native tests passed again, and final `...-r3` passed completely. These failed
runs remain preserved. Use a separate target directory for future scratch
mutation checks; source isolation alone does not isolate Cargo outputs.

T071 was not implemented/run: no diagnostics off/default/verbose/qlog A/B
measurement, matching pre-change overhead reference or prospective overhead
policy is claimed. Packet responsiveness assertions and finite-state tests are
functional checks, not throughput/CPU/RSS/latency bounds. Freeze the required
reference/policy before that later measurement. Native ARM64, hosted CI,
final-revision WireGuard/network/full performance matrices and Phase 8 release
qualification remain separate. No independent read-only review or explicit
merge/release security review is claimed. See
[ADR-0011](../adr/0011-bounded-operator-telemetry-and-filtered-qlog.md),
[operations](../operations.md) and [troubleshooting](../troubleshooting.md).


## Later T071 follow-up

The prior exclusion/limitations above describe the T064–T070 invocation.
A later user request collected T071's scoped native loopback repeatability/blocked
collector matrix without production Rust changes. See the [performance
record](../performance.md#phase-7-t071-diagnostics-measurement-method--2026-10-13)
for immutable inputs, prospective per-run policy, 70/70 results and coverage
limits. This does not extend the original operations run into a full release
or pre-T064 performance qualification.


The independent Phase 7 audit found missing reverse size/resource correlation
and an incorrect lost-sequence envelope after live-summary coalescing. The
review corrections add symmetric summaries and regression coverage. Original
saved operations/benchmark evidence applies to its recorded executable hashes.
At the review-correction checkpoint T071 remained unchecked: same-binary repeatability does not satisfy the matching
pre-Phase-7 regression reference or freeze-before-critical-path-edits requirement.


## Review corrections — 2026-10-13

The reverse relay receive path now records `size/3` for native truncation and
`malformed/3` for invalid ancillary/address metadata, with the known opaque
Session/connection. Received QUIC DATAGRAM application-cap drops record `size/1`;
egress EAGAIN followed by slab/queue rejection records `resource/1` or
`resource/2`. Delivery, drop, queue residence and cancellation behavior is
preserved; the shared post-send outcome handler never retries the payload.
The status ring retains admission order during live coalescing and reports the
minimum/maximum sequence envelope across every eviction, including session
finalization that assigns a newer sequence in place.

Three automated regressions cover native oversized target replies (including
continued bidirectional service), received DATAGRAM size and deterministic
EAGAIN/slab/queue pressure with exact opaque correlation, and coalescing plus
overflow both before and after session finalization. The lost-range and reverse
reply tests were run against their original branches and failed on the reported
`lost_last=2` and missing `size/3`, then passed with the fixes restored.

Verification on the corrected tree: locked default and qlog release builds,
formatting, all-target/all-feature Clippy with warnings denied, and the full
all-target/all-feature Rust suite passed (168 tests; two external-peer tests
remain ignored in this invocation). All 49 Python tests passed. The local UDP
process/syscall tests required execution outside the sandbox; the first reactor
test attempt was blocked at fixture bind and was repeated successfully.
Qlog release SHA-256:
`cd73f59238cc48709c93afa35f34cf9d1fd67d31aba27f86ef1b2c1a93addde3`.
All-feature debug process executable SHA-256:
`9a953be81a666f872998acf880e3df62effb83132241567899180a8a6ef1f521`.

The retained T071 comparator still reports 70/70 per-run passes with the original
policy SHA-256 `146a29bdc8eefc5861147de405d351a9843b424bb1df539538951b9a73d8d99e`.
New comparison/manifest output explicitly includes
`measurement_kind=same-binary-repeatability` and `regression_acceptance=false`.
Saved original artifacts and numeric limits were not rewritten. No new
performance, independent interoperability, standalone operations-runner,
high-rate stopped-qlog CPU, ARM64 or release qualification is claimed for the
corrected executable at that checkpoint. T071 was pending; requirements were not relaxed.


## T071 final cross-revision follow-up — 2026-10-13

The later T071 acceptance cycle now passes all 70 adjacent Phase-6/Phase-7 pairs
under a policy frozen before candidate reconstruction/build/collection, using
50 pre-change reference runs and matched native workload/CPU/compiler/features.
See the [final performance report](../performance.md#final-t071-cross-revision-results)
for exact binary/source/policy/artifact hashes, metrics, noise and claim limits.
Failed baseline-drift and invalid startup-affinity attempts remain preserved;
all original policy files are unchanged. The sampler now waits for taskset's
assigned CPU mask before sampling. All 64 Python tests pass. No production Rust
changed in this measurement work. This completes the declared T071 workload;
the original missed freeze before initial authorship is not rewritten, and
high-rate/stopped-filter CPU, ARM64 and Phase 8 release remain separate.
