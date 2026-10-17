# Phase 8 qualification and documentation status — 2026-10-17

**Current outcome: T076, T077 and T078 are complete for the selected native
x86_64 CUBIC candidate.** Full functional qualification and all 75 numerical
pairs pass on qlog executable
`7f7a3c4de7341b8edf0860ff3c8457642ff423cf4658f9aa93076db678cfe3b1`.
The final requirement/review reconciliation below records the scope and risks.
ARM64/T072 is deferred outside current requirements. Final instructions/changelog (T079)
are complete for this declared scope; no publication or deployment is claimed.
Historical failed runs and the owner-accepted small CPU trade-off are unchanged.

**Platform scope update — 2026-10-17:** The owner removed ARM64 from current
requirements. [ADR 0024](../adr/0024-defer-arm64-release-scope.md) makes Linux GNU
x86_64 the sole required target and T072 deferred outside acceptance/completion
gates. T073–T079 are complete and Phase 8 has no remaining active task in this
scope. ARM64 tooling is experimental/manual-only. Earlier ARM64-pending notes
below are historical; no missing native check is relabelled passed. Production
inputs and the qualified binary remain unchanged. Scope checks and pre-update
snapshots are retained in `benchmarks/results/phase8-x86-only-scope-20261017/`.

**T079 documentation completion — 2026-10-17:** The owner explicitly authorized
T079 after the earlier exclusion. README, build/operation/WireGuard/performance
guides, quickstart and Unreleased changelog now describe the selected native
x86_64 CUBIC build, pinned Git dependency, current defaults, compatibility,
restart/revocation boundary, reproduction commands and known limitations.
Static link/anchor, shell syntax and CLI/plan checks pass; no workload or VM
was started for documentation verification. All selected production inputs and
the qualified executable remain unchanged. Local checks and before/after
snapshots are retained in `benchmarks/results/phase8-t079-docs-20261017/`.

The pre-T079 report/tasks snapshots in that directory preserve hashes bound by
the preceding implementation-review correction record. Earlier T078 report
snapshots and raw qualification manifests remain unchanged. Statements below
that T079 is excluded or that final wording is pending are historical checkpoint
notes, superseded by this documentation record. T072 is now deferred outside active requirements by ADR 0024; the phase is
complete for the sole required native x86_64 platform, without an ARM64 support
claim.

**Post-qualification decision review — 2026-10-17:** An explicitly requested
independent read-only review assessed decisions using primary evidence without
ADRs or previous rationale. It supports the narrow native checkpoint and
output-order correction, and identifies further questions about lossy-path CC,
shared-socket density coupling and buffer/service-pause headroom. The ideas and
three short distinguishing experiments are retained as a
[future research backlog](phase8-decision-review-followup.md), not additional
work started by this report.

The review also finds stale requested-buffer numbers in the T077 manifest.
[The metadata erratum](phase8-benchmark-metadata-erratum.md) corrects this
transparently: all 300 baseline/tunnel role diagnostics match actual effective
buffers; the original manifest and measured source/results remain unchanged.
The corrected runner derives requests from shared Rust defaults. All 98 Python
tests pass, including the failing-before/passing-after manifest regression.
Only metadata/test/report files change after qualification; the selected
production executable and numerical acceptance are unchanged. The original
T078 report hash resolves to the archived pre-correction report snapshot.

**Implementation review corrections — 2026-10-17:** The independent audit
found no confirmed production forwarding defect/P0/P1 in the reviewed delta.
Its two P2 findings are corrected: valid early bounded-search ceiling acceptance
and production-transition lifecycle fuzz coverage with independent expectations.
See the [correction and verification record](phase8-implementation-review-fixes.md).
The final correction checks pass 183 native and 24 traffic tests, four fresh
ASan campaigns and the saved T077 validation. All selected production inputs
and the rebuilt qlog executable remain identical. The record preserves one
earlier isolated churn failure and its successful repeats without claiming
its cause is fixed. No new VM/performance qualification is claimed or needed
for an unchanged executable; T079 remains excluded.

**Selected candidate and current execution:** The owner accepts ADR 0022's
measured small CPU cost, keeps CUBIC and stops CPU/BBR2 investigation for this
increment. [ADR 0023](../adr/0023-select-cubic-and-accept-measured-cpu-cost.md)
and `benchmarks/policies/phase8-cubic-adoption.json` bind this adoption decision
to qlog executable
`7f7a3c4de7341b8edf0860ff3c8457642ff423cf4658f9aa93076db678cfe3b1`.
The historical failed comparison stays failed; SC-003/004/005/007 are unchanged.
The short exact-candidate control/drop-five characterization completes all six
valid runs: injected reverse losses are 2.198022%, 1.956192%, 2.514200%, with
counter proof of exactly five external drops per impaired run. CUBIC loss
amplification at high constant input remains an explicit limitation, rather
than a claim of qualification on lossy WAN paths.

Fresh input-bound T073–T076 execution passed under
`benchmarks/results/phase8-cubic-qualification-20261016/`, including
the owned VM generic/WireGuard fixture. All 23 functional stages passed;
total duration was 80m54s within the 90-minute budget, including prerequisites
and VM cleanup. T076 is complete for native x86_64. T077's full numerical
reference cycle completed successfully under
`benchmarks/results/phase8-cubic-reference-20261016/`; final T078 reconciliation
is recorded below. T079 remains excluded.

**Current increment:** [ADR 0022](../adr/0022-drain-committed-output-before-generation.md)
corrects generate-before-flush ordering within the bounded output visit. A native
regression fails with six ingress drops before the fix and passes with zero
after it. Locked default/qlog builds, formatting, Clippy, 182 ordinary Rust
tests, one independent-peer process test, 23 traffic tests and 95 Python tests
pass. The new qlog executable is
`7f7a3c4de7341b8edf0860ff3c8457642ff423cf4658f9aa93076db678cfe3b1`.
Its source-bound focused comparison completed all 60 rows in 54m44s under
`benchmarks/results/phase8-output-order-fix-20261016/comparison-progress.json`;
acceptance failed. CUBIC passes 9/12 frozen checks, failing the three small CPU
repeats; BBR2 passes 9/12 separate checks, failing two paired medium-loss repeats
and one full-lifetime socket-count guard. That excess socket is observed only
before measurement; raw rejection remains unchanged. All new resource repeats
deliver 500.736Mbit/s/direction without endpoint loss. See
[the completed methodology and evidence](phase8-output-order.md). All modes request
the same 1MiB outer buffers. CUBIC remains the default. Subsequent full T076
passes for this executable; the subsequent T077 matrix and T078 reconciliation
also complete for native x86_64. T079 remains excluded.

**Release acceptance is pending.** ADR 0021's new candidate changes the outer
UDP buffer request to 1MiB; locked builds, formatting, Clippy, 179 ordinary Rust
tests, the independent-peer process test, 95 Python tests and updated traffic
checks passed. The focused four-workload comparison with matched baseline,
preceding control and BBR2 completed under
`benchmarks/results/phase8-outer-buffer-20261016/resume1/`; per-row state is in
`comparison-r2/progress.json` in its parent directory. CUBIC passed 10/12 frozen
checks; historical capacity-loss/delivery and small-packet CPU bounds failed.
BBR2 passed its 12 general SC checks, a distinct acceptance scope; it consumes
10.1–12.5% more small-packet CPU than adjacent CUBIC and would fail historical
CUBIC CPU bounds. See [the completed comparison](phase8-outer-buffer-comparison.md).
This configuration change
reopens T076 for the new executable
`a766bdb78cb54910ee1a7246b0077936b23430a9cf27627f541558f825633add`.
T077/T078 remain pending. The preceding Git-fork candidate incorporates
the density window, downstream-first service order and 128KiB application
socket buffer default (ADRs 0017–0019). Its qlog executable SHA-256 is
`1229d87f5fe1fbfbf5c0ccf231d033af2e96f1645184d05cd7f826c94e78cdee`.
All nine focused candidate checks passed. Full T076 also passed on this exact
preceding candidate with fresh source-bound prerequisite evidence under
`benchmarks/results/phase8-final-qualification-20261016/`; see the current
acceptance record below. T077's
preceding full matrix remains unsuccessful and final T078 remains pending.
Before new T077 collection, ADR 0020 prospectively extends only the three
32-session simultaneous capacity pairs to 60 seconds per member. The nominal
plan is 4965 seconds under the existing 5400-second deadline; numerical gates
are unchanged. T072 lacks native
ARM64 execution; T079 is deliberately excluded. Historical records and
requirements-quality checklists are unchanged. No release, push, deployment or
independent reviewer audit is claimed for Remora.

The original qualification below kept production source and runtime behavior
unchanged. Subsequent optimization candidates modify owner-local readiness and
bounded connection packet-service visits and idle slab retention in
`src/reactor.rs`/`src/session.rs`/`src/buffers.rs`;
see [ADR 0015](../adr/0015-bounded-reactor-packet-service.md). The later root-cause
correction uses a pinned local quiche 0.30.0 patch; see
[ADR 0016](../adr/0016-quiche-minimum-payload.md). Root/traffic/fuzz manifests and
locks now resolve that reviewed source, with other registry versions/checksums
unchanged. Other changes are test/qualification tooling, CI, ARM64
benchmark-profile support, tests and these evidence reports. The traffic validator now
accepts the two declared architectures; native execution still rejects a host
mismatch and unknown architecture labels. Both architectures retain the exact
same reference thresholds. The previous x86-only planner/validator failure and
its failing-before/passing-after regressions are retained locally.

## Selected CUBIC x86_64 T076 acceptance — 2026-10-16

`benchmarks/results/phase8-cubic-qualification-20261016/summary.json` records
`T076-passed`; `functional/summary.json` records all 23 stages passed and no
missing prerequisites. Production, qualifying runner/profile inputs and fresh
T073–T075 evidence match the selected executable and independent peer. The run
started at 15:02:57 UTC and finished at 16:23:51 UTC: 80m54s total, including
3m11s prerequisites and 77m43s functional/VM cleanup. Both owned guests were
independently observed shut off after cleanup.

Evidence includes locked default/qlog builds, format/Clippy, 182 ordinary Rust
tests, 1164 pinned quiche tests, 95 Python tests, 23 traffic tests and the
explicitly run independent-peer process test. Four fresh 30-second ASan fuzz
targets, native safety/Miri and dependency/advisory checks also pass.
The qlog release binary SHA-256 is
`7f7a3c4de7341b8edf0860ff3c8457642ff423cf4658f9aa93076db678cfe3b1`;
peer SHA-256 is
`73da4dfab3f31ddf517770b43cfe40fca2a4709893e7951d5c982bcc5f38be93`.
Process suites retain their debug/process binary provenance separately.

The runtime matrix passes monitored security negatives, operator correlation,
independent interoperability, 30-minute isolation, 10-minute overload,
1000-cycle churn, network/recovery/shutdown and 30-minute VM generic/WireGuard
traffic. The two guests together receive 72000/72000 measured messages across
the two channels/directions, with no invalid/duplicate/send-error/skipped
messages. Guest B records 73 warmup send errors, separate from measurement.
Isolation/overload record zero cross-delivery, generator or integrity errors.
The intentionally saturated overload records 13 diagnostic coverage losses;
full overload correlation is not claimed. Ordinary operator correlation is
separately checked by the operations stage.

This acceptance does not transfer to a changed executable or native ARM64.
T077's full SC-003/004/005/007 matrix and T078 final reconciliation subsequently
complete below. ADR 0023's owner-accepted small CPU trade-off and current CUBIC
impaired-loss characterization remain explicit; no historical FAIL is rewritten.

## Preceding x86_64 T076 acceptance — 2026-10-16

`benchmarks/results/phase8-final-qualification-20261016/summary.json` records
`T076-passed`, and `functional/summary.json` records all 23 stages passed with
no missing prerequisites. Production inputs, prerequisite artifacts, the peer,
all 100 frozen runner/test/profile inputs and the qlog executable match.
The run started at 03:48:53 UTC and finished at 05:09:43 UTC: 80m50s total,
including 3m10s prerequisite preparation and 77m40s functional/owned-VM phase.
The latter remained below its independent 90-minute budget.

Evidence includes 179 ordinary Rust tests, one explicitly run independent-peer
process test, 1164 pinned quiche tests, 95 Python tests and 21 traffic-tool
tests; locked builds, formatting and Clippy; four fresh 30-second ASan fuzz
targets; native safety/Miri; and all 22 refreshed dependency checks.
The independent peer SHA-256 is
`73da4dfab3f31ddf517770b43cfe40fca2a4709893e7951d5c982bcc5f38be93`.
Process tests record their own executable hashes (including debug test builds);
the deployed VM duration workload records the qlog release hash above.

The full suite passed 30-minute isolation, 10-minute saturation probes,
1000-cycle churn, network/recovery/shutdown, operator/independent interoperability
and 30-minute VM generic/WireGuard traffic. The two guest result rows together
record 72000/72000 measured datagrams across generic and WireGuard channels:
zero invalid, duplicate, send-error or skipped datagrams. Guest B recorded 47
warmup send errors, retained separately from the measured interval. Both owned
guests were independently observed shut off after successful cleanup.

Isolation and overload recorded zero cross-delivery, generator or integrity
errors. The saturated overload intentionally recorded queue drops and 28 lost
diagnostic records; `diagnostic_correlation_claim=false` is retained. This is
not complete per-drop diagnostic correlation evidence. Normal operator
correlation/redaction is separately checked by the operations stage.

T076 is complete only for the native x86_64 scope. SC-003/004/005/007 full
reference performance acceptance still awaits T077, followed by T078 final
reconciliation/review. ARM64 remains unqualified, T079 remains excluded and
no release acceptance is claimed. Historical failures below remain unchanged.

## Selected CUBIC T077 execution — 2026-10-16

After current exact-input T076 acceptance, the full native x86_64 reference
cycle completes successfully under `benchmarks/results/phase8-cubic-reference-20261016/`.
Raw results are retained under
`benchmarks/results/release/phase8-x86-cubic-20261016/reference/benchmark/`.
The coordinator binds T076 summaries, source-bound prerequisites, production,
runner/profile inputs, the adoption decision and both executable hashes.
It verifies them before and after collection. Remora qlog SHA-256 is
`7f7a3c4de7341b8edf0860ff3c8457642ff423cf4658f9aa93076db678cfe3b1`;
traffic SHA-256 is
`d2693daff0709b6c05c78a1476a35aaa0633139220f82a763e96d180c35ec36b`.

The unchanged bounded-reference-v2 plan contains 18 bounded calibration
searches, 75 paired measurements and 1000-cycle churn. Nominal intervals total
4965s, with a 5400s outer budget including a 30s cleanup reserve. CPU placement
is client 0–3, server 4–7, tools/collector 8–23. SC-003/004/005/007 and baseline
sustainability/validity requirements are unchanged. No concurrent compilation
or VM duration workload is running. The earlier failed focused CPU comparison
and owner-approved adoption decision remain distinct from numerical acceptance.
Use `python3 /tmp/remora-phase8-cubic-reference-status.py` for progress.
The run starts at 16:36:58 UTC and finishes at 18:01:40 UTC, taking 84m42s.
`criteria.json` records `passed`, with empty `invalid` and `failed` lists. All
18 searches, 75 pairs (18 capacity, 54 subcapacity, three resource) and
1000-cycle churn complete. All 150 paired baseline/tunnel measurements meet
generator/integrity/clock/coverage validity; each has at least 149 RTT probes
per session. Every paired baseline remains <=0.090004% loss, below 0.1%.

| Criterion | Worst measured outcome | Required |
|---|---:|---:|
| SC-003 capacity delivered/direct ratio | 99.815985% | >=80% per active direction |
| SC-004 added subcapacity loss | 0.038594 percentage points | <=0.1 percentage points |
| SC-005 added subcapacity p95 RTT | 0.768ms | <=5ms |
| SC-007 resource delivery, all three repeats | 500.736Mbit/s in each direction, zero measured loss | >=500Mbit/s/direction |
| SC-007 maximum resource role CPU / RSS | 0.910608 core / 12.179688MiB | <=2 cores / <=128MiB |
| SC-007 churn retained growth / settling | <=233472 bytes / 0.119239s | <=10MiB within <=60s |

The corresponding maximum absolute subcapacity loss is 0.128597%; its matched
baseline and added-loss gate pass. Capacity at near-link load has up to
0.215847% absolute loss; its SC-003 criterion concerns delivered/direct ratio,
not zero loss. Only resource delivery is lossless in every repeat. No averaging
or target change hides a failed pair. `spread.json` retains three-repeat
variability separately for both modes. T077 is complete for native x86_64;
T078 reconciliation follows below, T079 remains excluded.

## Preceding T077 execution — 2026-10-16

After exact-input T076 acceptance, the full native x86_64 cycle was started
under `benchmarks/results/phase8-final-reference-20261016/`. Its coordinator
binds the T076 summaries/prerequisite manifest, production sources, runner
sources, profile and both executable hashes before collection and after the
cycle. Raw results are retained under
`benchmarks/results/release/phase8-x86-final-20261016/reference/benchmark/`.
The production qlog executable is unchanged; the rebuilt traffic tool with
the explicit interval-extension validator has SHA-256
`cb2f0d710ce8c6f705051bb958aa683b1499c4cdf6d9d8bfd4a09a17cb866f50`.
The plan contains 18 bounded calibration searches and 75 paired measurements,
then 1000-cycle churn. ADR 0020 and the 90-minute deadline apply.
The cycle completed all 75 pairs/150 runs and 1000-cycle churn in 84m42s,
but numerical qualification is **invalid with one additional threshold failure**.
`criteria.json` records:

- `capacity:32:1200:simultaneous:0`: baseline forward loss 0.115706%, exceeding
  the 0.1% sustainable-baseline ceiling (6327/5468160 missing datagrams).
  The measured sampled baseline process UDP-drop deltas were 4921 at the
  client role and 2863 at the server role; this evidence does not establish
  an exact per-socket/directional attribution of all endpoint loss. RTT sample
  counts now pass (minimum 598/session for baseline and 595 for tunnel).
- `subcapacity:1:512:simultaneous:2`: tunnel reverse loss 1.974980%
  (27122/1373280), versus zero baseline loss; SC-004 fails. Forward loss is
  0.010340%. Periodic server snapshots bracketing the measurement record
  26923 additional reverse `queue_full` drops and 28 `queue_expired` drops;
  this localizes queue pressure but is not an exact endpoint-loss decomposition.
  Measured sampled kernel UDP-drop deltas are client 147/server 168;
  generator and responder have zero. Generator/integrity error counts are zero.

All three resource repetitions have no reported numerical failure, delivering
500.7352–500.736 Mbit/s per direction with average CPU 0.8909–0.9068 core per
role. Churn passed 1000 cycles, settled in 0.125s and retained at most 229376
bytes above warmed RSS. There is no churn exception: the traceback propagates
the numerical validator's rejection after the completed matrix/churn.
Source, runner, profile and both executables remained unchanged.
The coordinator summary, raw suite, criteria and `analysis.json` preserve
the failures. T076 stays passed; T077/T078 remain unchecked. Next work is
focused paired 512-byte simultaneous profiling and separate baseline-overflow
diagnosis before another full cycle. No full rerun was started automatically.

Subsequent focused diagnosis reproduced the amplification causally: dropping
exactly five outer UDP packets gives 2.78–2.87% reverse application loss in
three repeats, while controls have zero reverse loss. An identical 5ms client
receive pause gives 2.10–2.22% loss with the current outer buffer and about
0.020–0.021% with an explicit 1MiB requested outer buffer; the larger setting
eliminates outer kernel drops in all three diagnostic repeats. The initial
host-trigger cause is still unproven, and the baseline's full-cycle failure
remains retained. No production defaults changed. See the
[causal diagnosis and evidence limits](phase8-final-loss-diagnosis.md).

## Historical candidate13 vendored-binary functional acceptance — 2026-10-15

`benchmarks/results/phase8-packet-fix-qualification-resume1-20261015/summary.json`
reports `T076-passed`; its full `functional/summary.json` has all 23 stages
passed, no missing prerequisites, and `release_acceptance_claim=false`.
Start/end: 11:18:58–12:38:15 UTC (79m17s), within the 90-minute continuation
budget including VM cleanup. Both owned guests were stopped successfully.
Production inputs, prerequisite artifact hashes and the measured release binary
still match; full functional summary SHA-256:
`99ee25de2fbe0c3e4ae7cf627ace02d8633c5f0bbc7e19e5ebf0b18ebe522637`.
Release qlog binary:
`ff75ff9fd493235e16affca4b8827d6514ebb6617b6ae160381c2bbd3312bc76`.
Independent Go peer:
`73da4dfab3f31ddf517770b43cfe40fca2a4709893e7951d5c982bcc5f38be93`.

- Locked default/qlog builds, format, Clippy and vendor integrity pass.
- 173 ordinary Rust tests, one explicit external-process shutdown case,
  1164 patched-quiche library tests, 93 Python and 20 traffic Rust tests pass.
  The independent interop runner separately executes the monitored role matrix;
  ordinary ignored external tests are not counted as ordinary-suite passes.
- Independent interoperability, operator diagnostics, all declared network,
  recovery and shutdown scenarios pass.
- 1800-second generic isolation, 600-second overload and 1000-cycle churn pass.
  Saturation intentionally causes bounded drops; no zero-loss overload claim.
- The independent 1800-second VM WireGuard/generic test delivers 18000/18000
  datagrams for each of two channels in each direction: 72000/72000 total,
  zero measured duplicates, integrity, send or skipped errors. Startup warmup
  has separately retained send errors and is outside the measured interval.
- Exact-input fuzz and safety passes are retained from the preceding attempt;
  all 22 dependency checks were newly executed and pass. The earlier network
  fetch failure remains unchanged; see the current [dependency audit](dependencies.md).

This closes T076 for current native x86_64 functional qualification. The prior
three paired 60-second resource probes and five repetitions of two affected
workloads also pass, but do not substitute for the full T077 reference matrix.
T077/T078 remain pending and ARM64 remains unqualified; T079 is excluded.

## Historical completed evidence — original qualification

Host: native x86_64 Linux 7.2.9-arch1-1, Intel Core i7-13700K. Production compiler:
Rust 1.99.0; test-only fuzz/Miri: nightly-2026-10-12,
`rustc 1.101.0-nightly (080cc2714 2026-10-11)`.
Source revision is `ff5e629750fd73d967f8154c227e1bd9b777901e` with the qualification
changes in the working tree; manifests bind production source/build/lock hashes.

| Evidence | Result | Limits |
|---|---|---|
| T073 ASan fuzz | Four 30-second targets passed, seed 73013; final counts: MASQUE 3,155,780, policy 4,678,613, ancillary 22,468,679, lifecycle 651,388 | Smoke campaign only; scheduled 900-second/target CI configured, not executed here |
| T074 native ownership + Miri | 2/2 source-included ownership tests; full library properties/components and IO/lifecycle suites pass | Miri excludes real Linux syscalls and BoringSSL; see [safety audit](safety.md) |
| T075 dependencies | Four Rust locks audited, Go graph/checksums/symbol scan and applicability review pass; locked native default/qlog/traffic/peer builds pass | Timestamped advisories and x86_64 only; see [dependency audit](dependencies.md) |
| Final local release checks | Locked default/qlog release builds, formatting, Clippy -D warnings, 168 Rust tests and both externally enabled process tests pass | Rust process cases use recorded debug executable, not the release hash |
| Independent interoperability | 29/29 cases pass on the release executable | Both role directions, v4/v6, monitored negatives and wire refusals; not long-duration/performance qualification |
| Operator diagnostics | 8/8 release scenarios plus five native check groups pass, normal collection has zero coverage loss/write failures and zero secret/payload sentinels | Intentional overflow/blocked collection remains explicit failure/robustness evidence, not SC-010 success |
| Python / traffic tooling | Latest 93 Python tests; 20 traffic Rust tests, formatting, Clippy and locked release build pass | Includes ARM64, continuation, pacing and collector-affinity regressions; T076 ran on its recorded earlier traffic executable |
| Reference plans | Latest schema-4 bounded profile: 18 searches and 75 pairs per architecture | r3 measured cycle remains invalid/unsuccessful; no final SC-003/004/005/007 acceptance |

The qlog-feature release executable used for local operations and interop is
SHA-256 `b548cada020465f3399da6244e770b48dafd26d6ea8d432e0313f3152c6bef32`.
The independent peer is
`9aa7b286a139f16f3ecd15ce3620b5f39ef2946929e1271856db0d54077864a2`.
Traffic validation changed afterward; only its affected tests/build and Python
checks were repeated at that original qualification. Production packet code
changed in the subsequent investigation below; older evidence does not
qualify that candidate.

Local archives (ignored generated data, preserved on this machine):

- `benchmarks/results/phase8-fuzz-final-20261013`: final per-target logs/corpus/status and source/lock hashes; earlier smoke is also retained.
- `benchmarks/results/phase8-safety-20261013`: native and Miri logs.
- `benchmarks/results/phase8-dependencies-20261013` and `phase8-dependencies-applicability-20261013`: resolved graphs/advisory snapshots/native builds and explicit non-applicable findings.
- `benchmarks/results/phase8-release-checks-r3-20261013`: successful local check/interop/operator matrix, exact binaries and subrunner manifests.
- `benchmarks/results/phase8-arm-regressions-20261013`: failing-before ARM64 planner/result-validation evidence; final targeted suites pass.
- `benchmarks/results/phase8-release-final-preflight-20261013`: full qualification blocked solely by missing prepared private WireGuard fixture.
- `benchmarks/results/phase8-reference-plan-x86-20261013` and `phase8-reference-plan-arm-final-20261013`: unmeasured reference plans.

The original sandbox-denied UDP-bind run and native Python holder-readiness
race are preserved in `phase8-release-checks-20261013` and
`phase8-release-checks-native-20261013`. The latter read `/proc/cmdline` before
child exec completed; a three-second bounded readiness wait fixes the test.
The original generated proptest seed is preserved with the sandbox failure,
not committed as a production regression. Missing privileges are not passes.

## Final T078 requirement and review reconciliation — 2026-10-16

T078 is complete for the exact selected native x86_64 candidate. This is an
**author protocol/security/unsafe/performance review and evidence reconciliation**,
not an independent reviewer audit. The owner authorized Phase 8 except T079;
no reviewer subagent, commit, push, deployment or publication is performed.
Native ARM64 is unqualified and excluded from the accepted platform claim.

The source-bound machine record is
`benchmarks/results/phase8-cubic-reconciliation-20261016/summary.json`.
It binds current production inputs, the selected executable, T076/prerequisites,
T077 suite/criteria/spread/churn, operator/independent-peer/VM evidence and the
owner's ADR 0023 decision. An offline reconciliation script independently checks
all 75 pair thresholds and provenance against existing artifacts; it does not
rerun or relabel measurements. T076/T077 original summaries retain their
stage-specific `release_acceptance_claim: false`: neither individual runner
publishes or certifies an unrestricted release.

### Functional requirements

All FR-001–029 are accounted for below. “Verified” means the exercised
native x86_64 scope and documented protocol profile, not a proof for every
possible network/input sequence. Evidence paths are relative to
`benchmarks/results/phase8-cubic-qualification-20261016/` unless noted.

| Requirements | Reconciled implementation/evidence | Outcome / limits |
|---|---|---|
| FR-001, FR-003, FR-004 | Native datagram/syscall/size tests; process forwarding; `functional/interop/summary.json`; `functional/wireguard/summary.json` | Both roles, opaque empty/supported-size datagrams, generic/WireGuard verified; VM duration is 30 minutes, 72000/72000 measured messages |
| FR-002, FR-005 | QUIC/H3/settings/Capsule/datagram conformance and fuzz; 1164 pinned quiche tests; 29 independent IPv4/IPv6 cases in both roles | Required CONNECT-UDP/datagram profile verified; unavailable datagram capabilities rejected; no reliable payload fallback |
| FR-006, FR-007, FR-008, FR-009 | Native credential/TLS/policy tests; monitored independent refusals/early data/mixed DNS; `functional/operations/summary.json` | Authenticate before target setup, all-answer authorization and identity validation checked; controlled restart/update remains the revocation boundary |
| FR-010, FR-011, FR-013, FR-014 | Native generations/commitment/close/partial-send/reuse/fairness tests; `functional/isolation`, `overload`, `churn` | 32 sessions isolated; cancellation and original-association ciphertext bounded; indistinguishable external tuple-reuse replies retain the documented provenance limit |
| FR-012, FR-028 | Buffer/ledger properties, source-included Miri; saturation/churn; full T077 resource rows | Finite queues/native/FD/session budgets verified; idle slab retirement bounded; 1000-cycle retention and >=500Mbit/s resource criteria pass |
| FR-015, FR-016 | Native send/error-queue/PMTU/truncation tests; `functional/network`; current loss characterization and T077 | Best-effort drops remain observable; no private fragmentation/reliable fallback; selected GSO/GRO remains off; no zero-loss promise under overload |
| FR-017, FR-018, FR-019 | Recovery/watchdog/GOAWAY/permanent-latch components; nine `functional/recovery` scenarios | Listener retained, renewed trust/auth/policy, stale-work discard and bounded recovery checked; permanent refusals remain denied |
| FR-020, FR-024 | Config/private-file/exposure tests; IPv4/IPv6 process cases; `functional/shutdown/shutdown-summary.json` | Loopback defaults, explicit exposure, unprivileged ports, fail-before-forward configuration and <=5s stop/cancellation verified |
| FR-021, FR-022, FR-023 | Fixed telemetry/privacy/qlog/status tests; all 13 `functional/operations` results | Normal injected failures correlated and sentinels absent; bounded suppression/overflow is exposed, not complete correlation under deliberate overflow |
| FR-025 | Existing build/operations/trust/revocation/limit/troubleshooting/WireGuard guides; fresh clean-install walkthrough; owned VM fixture execution | Existing procedure exercised without secret argv/URLs; final publication/default/performance wording and changelog refresh are T079, explicitly excluded |
| FR-026 | Current native properties/components/syscalls/process/security/fuzz/Miri, independent client/server and VM suites | Exercised native x86_64 verification complete; ARM64, all-path proof and independent code audit are not claimed |
| FR-027 | Full T077 18 searches, 75 paired runs and `spread.json` in the selected reference archive | Each of three repetitions checked separately; direct/tunnel, forward/reverse/simultaneous, CPU/RSS/kernel queues and clock/generator validity retained |
| FR-029 | One production Rust package/executable; traffic, Go peer, fuzz and Miri are test-only; fixture is external | UDP-only scope preserved; no production VPN/TUN/routing/WireGuard management, GUI or transport fallback added |

### Success criteria

| Criterion | Final evidence / assessment |
|---|---|
| SC-001 | T076 generic/WireGuard 1800s and full functional matrix pass on selected binary |
| SC-002 | Current credential/trust/policy/early-data negatives, destination monitors and diagnostics sentinel checks pass; exact HTTP/protocol failure evidence retained |
| SC-003 | All 18 capacity pairs pass, minimum directional delivered/direct ratio 99.815985% against >=80% |
| SC-004 | All 54 subcapacity pairs pass, maximum directional added loss 0.038594 percentage points against <=0.1 |
| SC-005 | Same subcapacity pairs pass, maximum added p95 RTT 0.768ms against <=5ms; no one-way latency claim |
| SC-006 | Full T076 recovery scenarios satisfy detection/restored-path criteria; revoked/trust/policy/capability failures remain denied |
| SC-007 | Three 32-session resource pairs each deliver 500.736Mbit/s/direction with zero measured loss, <=0.910608 process core and <=12.179688MiB RSS; 1000-cycle growth <=233472 bytes settles in 0.119239s |
| SC-008 | Both roles' starting/active/recovering shutdown and listener release tests pass within 5s |
| SC-009 | Full 30-minute isolation and 10-minute admitted-probe saturation pass, with zero cross-delivery/integrity/generator errors and recorded ceilings |
| SC-010 | Current normal operator failure/redaction cases pass with privacy-safe correlation; blocked/incomplete/overflow collection remains explicit failure/limited-coverage evidence, never a complete-correlation success |

### Protocol review

Read-only review rechecked `src/transport.rs`, `src/masque.rs` contracts/tests
and affected reactor sequencing. Client certificate/server-name validation and
completed non-early QUIC negotiation precede credential/application setup.
Required H3/HTTP/QUIC datagram settings and extended CONNECT are checked.
Request/response/Capsule/Context-ID validation, forbidden final responses and
canonical target handling retain their existing negative tests and independent
peer evidence. The pinned quiche PING/minimum-payload correction remains at
Git commit `154ce1995f23efa434e72dac269f707145be2015`; its source identity and
1164-test run match. No newer-upstream or universal conformance claim is made.

The current output-order change does not reinterpret datagrams or wire fields.
Ready committed ciphertext is submitted only after native expiry/PMTU/setup
preparation; generated packets and both submission opportunities share the
existing 16-generation/16-offered-descriptor visit limits. FIFO identity,
pacing, partial accepted-prefix progress, hard-error classification and
original-generation association are retained. Regression tests reproduce the
old six-message loss and cover partial sends/pressure, expiry and future pacing.
No protocol-blocking finding is identified in these reviewed boundaries.

### Security and resource review

Read-only review rechecked `policy::authenticate`, all-answer numeric-address
authorization, bounded DNS workers, generation-key validation, server request
admission and session retirement in the reactor. Credential comparison checks
all configured identities; authorization precedes destination socket use;
authenticated identity/pending quotas bound setup. Retired sessions discard
cancellable application/native records and delayed setup, while already
committed ciphertext remains on its immutable original association. Output
service stays after HTTP/3 control/close handling and before producer reads.
No secret-bearing diagnostic field, new verification bypass, shared quiche
mutex, per-packet task or unbounded queue is introduced.

ADR 0021's 1MiB requested outer/128KiB application buffers are a declared kernel
memory trade-off, independent of RSS. Effective RX/TX maxima are 2MiB/256KiB
each on this host. Resource runs observe two total client UDP sockets and 33
server sockets: one outer per role, one client listener and 32 server relay
sockets. Combined configured socket ceilings are 4.5MiB client and 20MiB server;
these are not actual committed/RSS allocation. Measured aggregate UDP RX+TX
queue peaks are <=174336/60480 bytes. Larger configured numbers of sockets
can multiply those ceilings; global/FD/session limits still apply.

All 22 current dependency checks pass, collected 2026-10-16T15:06:08Z. RustSec
snapshot `479c888ce4401313bca61ba4aba2d0f6244c4a09` has 1329 advisories, updated
2026-10-16T08:29:44+02:00; all five checked Rust graphs report zero known
vulnerabilities. The separate upstream test lock retains the yanked
`yoke-derive` warning; it is not in the production/traffic graphs and has no
reported security advisory. Its yank reason has not been assessed. Ten Go
module findings are `not-imported` on the exact test-only peer graph. These are
timestamped applicability results, not proof of no unknown vulnerabilities.
No security-blocking finding is identified within the reviewed/current scope.

### Unsafe and ownership review

`src/udp.rs` still has audited SHA-256
`57e039ff069a57a7b37bc845aba0335fe29482c1c3b5cbb5a3c0a4dec3c4f3b6`.
The local synchronous syscall/FFI safety invariants and descriptor/ancillary
validation in [the safety audit](safety.md) remain applicable. Scheduling,
session and buffer changes use safe Rust. Read-only review checks the new
bounded generation/flush path and unique slab/prefix ownership against that
boundary. Fresh native partial-send/PMTU/error-queue/commitment tests and
source-included buffer ownership Miri pass. Miri does not qualify BoringSSL,
Linux FFI, all event sequences or ARM64. No new unsafe operation or unresolved
ownership finding is identified in the inspected change.

### Performance review and accepted trade-off

The full methodology is bounded-reference-v2/schema 4, not a smoke or the
historical v1 cycle. Both modes share the declared 20ms RTT/1Gbit/s topology,
requested buffers/offload settings, clocks and offered intervals. Client cores
0–3, server 4–7 and tools/collector 8–23 remain disjoint; collector affinity is
verified each sample. All 75 pairs and all directional/repetition thresholds
pass without averaging away failures. Paired baselines each remain <=0.090004%
loss, below 0.1%; at least 149 RTT samples/session are recorded. Three-repeat
spread, manifests, executable/lock/source hashes and raw rows remain available.

Resource owner-thread CPU peaks at 0.909607 core: about 9% average headroom on
one packet owner. Across all tunnel runs it peaks at 0.938086 core. A two-core
process budget does not give that owner two cores; these averages do not prove
instantaneous burst headroom or faster-link capacity. All-run RSS peaks at
12.652344MiB. Capacity maximum absolute loss is 0.215847%, and subcapacity
absolute loss reaches 0.128597% when its paired baseline also loses traffic;
SC-003 and added-loss SC-004 pass. Only the three resource repeats are uniformly
lossless. This is not blanket zero-loss qualification.

ADR 0023's owner-accepted small CPU cost is reconciled for this exact candidate.
The earlier frozen focused comparison remains failed, with its reference and
metrics intact. No final SC threshold changes or future-source waiver follows.
The exception's T078 review obligation is now discharged; changed source needs
its own applicable evidence/adoption assessment.

### Residual risks and explicit exclusions

- The current CUBIC five-packet-loss experiment retains 1.956–2.514% reverse
  application loss at high constant ingress. No reliable delivery, broad lossy
  WAN/NIC/RTT fairness or maximum throughput promise follows from this reference.
- Indistinguishable opaque external UDP replies after tuple reuse/restart have
  the documented provenance limitation; identifiable stale data and active
  cross-session mixing are separately rejected/tested.
- Historical low-rate filtered-qlog overhead evidence remains bound to its
  recorded earlier executable/profile. Current T076 qlog correctness/privacy
  tests pass; performance with active filtered tracing and enabled GSO/GRO is
  not qualified by this current reference cycle.
- Deliberate overload loses 13 diagnostic records; incomplete correlation is
  exposed. VM warmup has 73 send errors; measured delivery is 72000/72000.
- The earlier isolated outer-socket overflow and BBR2 startup socket observation
  are not retroactively explained. They are retained history; BBR2 default/full
  qualification is not selected. Known tests do not prove rare-loss absence.
- Native ARM64/T072 remains unavailable and unclaimed. T079's final public
  instructions/default/performance/changelog refresh is excluded by the owner;
  no unrestricted release readiness, publication, deployment or independent
  audit is asserted. Existing clean-install/operator/WireGuard procedures were
  exercised; their final release wording remains a separate task.

All required T078 reviews and requirement mappings are recorded for the selected
native scope, with no unresolved in-scope blocker identified. Historical
failures, upstream-test warnings, unqualified profiles and the agreed CPU cost
are retained rather than converted into unconditional acceptance claims.

## Runner and continuation contract

Use `scripts/validate/release.sh --plan-only --output /absolute/fresh-output
--peer-binary /absolute/pinned-peer` to inspect the full command plan. Arguments
can be placed on one shell line. `--scope checks` runs the local static/native,
operator, independent-peer and externally enabled process tests and reports
`checks-passed`, which never means full T076/release acceptance.

Full execution requires `--wireguard-fixture /absolute/private/fixture.json`
and `--prerequisites /absolute/prerequisite-summary.json`. The prerequisite JSON
must have `status: passed`, tasks T073/T074/T075 all `passed`, `production_inputs`
equal to the runner's `inputs()` mapping, and nonempty `artifacts` mapping
absolute retained report paths to verified SHA-256 values. The local
`benchmarks/results/phase8-prerequisites-20261013/summary.json` is such a record.
It binds available evidence; it does not qualify an absent platform.

Full mode runs unchanged 1800-second generic/isolation, 600-second overload,
1000-cycle churn, network/recovery/shutdown and independently prepared
1800-second WireGuard scenarios. It consumes existing peer fixtures; it does
not provision WireGuard or deploy Remora. Native network runners may require
privileges in the harness. Every stage stops on failure and records the exact
command/exit/log plus unchanged production input hashes. A missing external
prerequisite produces `blocked`, never a successful placeholder.

After T076 passes, run the matching reference profile with
`scripts/validate/bench.sh --profile benchmarks/reference-x86_64.toml --output
benchmarks/results/release/FRESH_RUN` on native x86_64, or the ARM64 profile on
qualified native aarch64. Do not use development smoke/plan-only as T077.
Complete T078 reconciliation/reviews only after the required runs; keep T079
outside this invocation. Follow [artifact retention](README.md) and exclude
private fixtures before sharing archives.

## VM continuation — in progress

The user authorized continuation with VM on 2026-10-13. All old Remora lab
manifests were destroyed; read-only libvirt inventory showed only the unrelated
Kali guest. A new owned `remora-phase8` lab was created from the verified cached
image on 192.168.252.0/24, with private state at
`/tmp/remora-phase8-vm-lab-20261013`. Both guests completed SSH/cloud-init/WG
configuration and the installed binary hash matches the current release.
The private fixture is mode 0600. The 10-second WireGuard/generic bidirectional
smoke passed in `benchmarks/results/phase8-vm-smoke-20261013`; its duration is
explicitly unqualified. Full T076 is running in
`benchmarks/results/phase8-full-20261013`. The earlier missing-fixture records
above remain historical. Tasks are not marked complete from a running test.

### Author protocol/security review during VM continuation

Read-only source review rechecked the current production trust and wire
boundaries against the contracts: client CA/server-name verification before
credential submission; no early-data relay eligibility; strict CONNECT request
fields and canonical Bearer syntax; final response exclusions/forbidden capsule
fields; all-answer numeric-address authorization before relay allocation;
bounded Retry authentication, address/DCID association and expiry; session
close cancellation versus immutable original-generation ciphertext; fixed
privacy-safe errors and qlog metadata reconstruction. Current conformance,
security negatives, independent-peer cases and lifecycle regressions passed in
the final-check portion of the running T076 cycle. No new blocking finding was
identified in these inspected boundaries. This is an author review, not an
independent audit. The previously recorded unsafe audit remains applicable
because production source is unchanged.

Final performance review remains pending the T077 measurements. Reference
plans retain native-host architecture checks, generator validity/headroom,
per-role resource thresholds and per-run directional throughput/loss/RTT gates.
The VM smoke/duration workload cannot prove SC-003/004/005/007. No critical-path
optimization, target relaxation or release acceptance is authorized by this
partial review. Final T078 closure still requires complete applicable evidence.

### T076 resumed qualification — 2026-10-14, running

The first full run passed 1801 seconds of 32-session isolation with zero
cross-delivery/integrity/generator errors, 600 seconds of overload with every
admitted probe replying within one second in both directions, and 1000 churn
cycles settling in 0.736 seconds with 0.16–0.22 MiB RSS growth per role. The
ordinary network dispatch then failed with a Python UnboundLocalError because
a recovery import shadowed its global characterize function. This failed run
is retained. An alias fixes dispatch; three regression cases cover ordinary,
recovery and benchmark modes, with failing-before evidence retained.

The continuation runner reuses only an exact successful command prefix after
checking unchanged production hashes, native architecture, release/peer hashes
and retained logs. It records previous summary/log hashes and references; it
does not overwrite or relabel failures. Source, executable and shortened-duration
mismatch regression tests pass. All 76 Python tests pass after the corrections.
The resumed run is `benchmarks/results/phase8-full-r2-20261013`: all nine network
scenarios, all nine recovery scenarios and shutdown matrix pass. The 1800-second
VM WireGuard/generic qualification is running. No T076 completion is recorded
before its final result. T077 and final T078 remain pending.

## T076 x86_64 final functional acceptance — 2026-10-14

T076 is complete for the native x86_64 scope. The final VM WG/generic test
passed the full 1800-second measurement: each of four channel/direction pairs
sent/received exactly 18000 messages, 72000/72000 in total; zero invalid,
duplicate, send-error or skipped-interval counts. Handshakes remain fresh,
WG counters grow both ways, Remora process identities stay unchanged and
runner-owned traffic/Remora units are stopped afterward.

The accepted continuation summary is
`benchmarks/results/phase8-full-r2-20261013/summary.json`; it binds the exact
passed prefix in the retained original failed run and all final successful
stages. Release/peer hashes are unchanged from the earlier final local checks.
The initially failed network dispatch and its correction remain visible.
Normal operations retain zero status/terminal coverage loss; intentional
overload coverage gaps are separately reported without a correlation claim.

This closes applicable functional SC-001/002/006/008/009/010 assertions for the
recorded profiles and source. Numeric SC-003/004/005/007 reference acceptance,
final T078 release reconciliation and native ARM64 evidence remain pending.
The functional KVM lab does not establish reference capacity or native ARM64.

## T077 first attempt and generator correction — 2026-10-14

The first full reference attempt is retained at
`benchmarks/results/release/phase8-x86-20261014`; it stopped as **invalid**
before any candidate pairs. Baseline 1-session/128-byte/forward calibration
passed 10/20/30 Mbit/s. At 40 Mbit/s all 1171812 offered/submitted messages were
delivered, with no enqueue/integrity errors, generator CPU 0.049 core, negligible
clock drift and zero diagnostic loss; 48 intended slots remained unscheduled,
so the strict generator-validity gate correctly rejected the run. No capacity
or release-performance acceptance is inferred from those rows.

Eight direct 3-second/39062pps trials reproduced the omission in four trials
(3, 1, 1 and 1 slots), with complete delivery of submitted messages. The
original executable/source and the proposal are retained in
`benchmarks/results/phase8-generator-proposal-20261014`. The test-only generator
unconditionally slept after each bounded visit even when eligible backlog
remained or the measurement cutoff was close. The correction skips that idle
sleep for overdue slots and uses a finite <=2ms no-sleep end guard (48 slots at
39062pps represent an observed 1.229ms omission). Slot eligibility, cutoff,
32-slot visit quantum, drain and all unscheduled/excess/enqueue counters are
unchanged. No late slots are relabelled, and no validity gate or SC threshold
is relaxed. Three deterministic wait-policy regressions fail before/pass after;
all 15 traffic tests, formatting, Clippy and locked release build pass.
Production source and T076 functional qualification are unchanged.

The eight matching candidate direct trials all submit the exact declared
117186 slots with zero unscheduled/enqueue/warmup failures. This is targeted
generator regression evidence, not release performance. A new unchanged-profile
full T077 cycle is running in
`benchmarks/results/release/phase8-x86-r2-20261014`; the original invalid cycle
remains unchanged. The VM lab is shut off with its disks/private state retained.


## T077 second invalid cycle and prospective micro-batches — 2026-10-14

The second cycle stopped invalid after 139 calibration rows, before paired
measurements, at 1-session / 512-byte / reverse / 510 Mbit/s: 3735328 submitted
of 3735330 intended slots, two unscheduled, zero enqueue/warmup errors,
0.201 core generator CPU and 0.007951% delivered loss. Its retained directory
is `benchmarks/results/release/phase8-x86-r2-20261014`. Eight additional direct
reverse trials at 124511pps reproduced 259 omitted slots in one trial;
the other seven submitted all 373533 slots. These are generator diagnostics,
not sustainable-capacity acceptance.

ADR 0012 records a prospective 100us micro-batch method. Both reference profiles
explicitly declare it, every endpoint records it, and the controller checks
that raw records match the frozen profile. Functional defaults retain zero
quantum. Each visit now also checks the absolute cutoff before another slot.
Original executable/source and proposal are retained under
`benchmarks/results/phase8-generator-microbatch-20261014`; invalid cycles remain
unchanged. T077/T078 remain unchecked pending fresh complete measurements.

Eight matching direct reverse trials with 100us pacing each submit exactly
373533/373533 messages, with zero unscheduled/enqueue/warmup failures.
All 16 traffic Rust tests, Clippy, locked release build and nine reference
controller tests pass. A fresh full cycle is started at
`benchmarks/results/release/phase8-x86-r3-20261014`; no qualification claim is
made before its final evaluation. Native ARM64 remains unavailable.


## User-approved bounded T077 revision — 2026-10-14

The user rejected the 37-hour paired duration and approved a prospective full
measurement budget of 60–90 minutes. The long r3 cycle was interrupted with
its results retained and an explicit interrupted record; namespace final-state
cleanup evidence exists. ADR 0013 updates spec/contract/profiles to schema 3 /
bounded-reference-v1: 18 bounded calibration searches, 75 pairs, three
repetitions, 3s loaded warmup/15s measurement (60s resource), 500ms drain,
1000 churn cycles and a 90-minute wall budget with cleanup reserve. Numerical
SC thresholds are unchanged. Historical schema-2 validation remains strict.

Nominal maximum intervals total 4695s (78.25 minutes), excluding churn and
process/parsing overhead. Budget exhaustion is incomplete/invalid. Shorter
warmup, intervals, coarse calibration brackets and >=100 RTT probes/session
reduce temporal/statistical confidence; this is the explicitly accepted cost
tradeoff, not equivalence to old long measurements or a line-rate soak claim.
T076 production/VM qualification remains on its recorded source and profile.
T077/T078 stay unchecked until new valid results and final review exist.


Bounded-method validation: all 82 Python tests and all 17 traffic Rust tests
pass; traffic formatting/Clippy/locked release build and diff checks pass.
The final selected native namespace smoke at
`benchmarks/results/phase8-bounded-smoke-r3-20261014` executes two calibration
workloads and four pairs through both baseline/tunnel modes. Its expected
incomplete result is `invalid`, with runner exit 0; it is not T077 acceptance.
Two earlier smoke failures are retained: create-only live progress write,
and /proc/net teardown before a traffic child became waitable. Mutable progress
now updates atomically, separately from immutable artifacts; only expected
post-measurement traffic exit gets a bounded 50ms wait for observed completion.
Missing live evidence still fails; dedicated regressions pass. Churn-budget
regression verifies cleanup of the owned process group on exhaustion.

A fresh full schema-3 cycle is started in
`benchmarks/results/release/phase8-x86-bounded-20261014`, with log
`/tmp/remora-phase8-bounded-20261014.log`. Live `progress.json` records stage,
workload and completed pairs; terminal `criteria.json` and outer `failure.json`
are authoritative results. T077/T078 remain pending while it runs.


## First bounded cycle invalid and receive-deadline correction — 2026-10-14

`phase8-x86-bounded-20261014` stopped after 16 calibration points, two complete
workloads and no paired runs. At simultaneous/128-byte/218.75 Mbit/s the sender
submitted 1708810 of 1708984 intended slots (174 unscheduled); the responder
submitted all 1708984. Generator CPU 0.245 core, no enqueue/warmup/integrity/
coverage/overflow errors and negligible clock drift. The invalid gate correctly
rejected the row; baseline loss at this overloaded point was separately about
5.8%. This is not an evaluated candidate SC failure or a release pass.

Four matching direct repeats reproduced 19 omitted sender slots once.
The generator receive visit did not yield to an eligible outgoing slot until
its 32-packet allowance ended. Checking the send deadline between receives
corrects that service policy; drain remains available. All four candidate
repeats submit exactly 427246/427246 per direction, with zero generator errors.
Original executable/source/proposal remain in
`benchmarks/results/phase8-generator-receive-deadline-20261014`.
All 18 traffic Rust tests, 83 Python tests, formatting/Clippy/locked release build
and diff checks pass. No production source or numerical/validity threshold
changed. Remaining OS scheduling risks are not dismissed by four trials.

Calibration failure output now names the failed fields and summary file;
terminal live progress is updated to invalid instead of remaining running.
A new full cycle starts at
`benchmarks/results/release/phase8-x86-bounded-r2-20261014`, with the unchanged
90-minute budget and log `/tmp/remora-phase8-bounded-r2-20261014.log`.
T077/T078 remain unchecked pending final valid measurement/reconciliation.

The terminal-progress/failure-detail dispatch regression also passes; the full
Python suite is now 84 tests. New-cycle progress is observed live, with strict
invalid-gate behavior retained and no acceptance from in-progress calibration.


## Second bounded cycle and measured 32-session deadline diagnosis

`phase8-x86-bounded-r2-20261014` stopped invalid after 11 complete workloads,
67 points and no candidate pairs. At 32-session/128-byte/simultaneous/125Mbit/s,
sender 7 omitted one slot and sender 25 omitted eight. Submitted=unique in both
directions; generator CPU 0.240 core; clock/integrity/coverage checks pass.
The exact diagnostic namespace repeats retained under
`phase8-generator-32session-diagnosis-20261014/native-probes` produce generator
errors 0/4/36/0. Final service-visit gaps reach 2070us; missing senders last
submitted 598–1397us before cutoff. This reveals insufficient deadline margin
under 33 traffic processes and 16 logical tool CPUs; one-session checks were
insufficient to accept another full cycle.

ADR 0014 declares a prospective 20ms completion reserve with uniformly compressed
measurement generation, exact original counts/cutoff and unchanged gates.
Schema 4 / bounded-reference-v2 binds this waveform in profile/raw endpoints;
old schema suites remain historical. No complete cycle is started while
32-session calibration/measurement/resource diagnostic checks are in progress.
T077/T078 remain pending. The previous promise of background r2 completion was
an in-progress statement, not acceptance. Original failed artifacts remain.


## Completion-reserve verification and next full cycle

All nine explicit-reserve native probes under
`phase8-generator-32session-diagnosis-20261014/guard-probes` pass generator/
integrity/clock/coverage checks. Four use 8s calibration, four 15s measurement,
and one 60s resource interval, all 32-session simultaneous workloads. No omitted
slots; generator CPU 0.236–0.290 core. Resource baseline delivers exactly
500.4288 Mbit/s both ways with zero measured loss. This is targeted fixture
proof, not Remora candidate SC acceptance. All 20 traffic tests/85 Python tests,
formatting/Clippy/locked release build and diff checks pass; schemas 2/3 remain
strict historical validation. Raw tail instrumentation and all failures remain.

A fresh frozen schema-4 cycle starts in
`benchmarks/results/release/phase8-x86-bounded-r3-20261014`, log
`/tmp/remora-phase8-bounded-r3-20261014.log`, with the same 90-minute wall budget.
No historical failed cycle is relabelled or reused. T077/T078 stay unchecked.


## Full measured matrix, corrected churn and unsuccessful qualification

The schema-4 r3 cycle completes all 108 calibration points and 75 pairs in
about 4744 seconds (79.1 minutes), then fails the churn tail. Fresh setup
namespace lo is DOWN, while churn needs owned loopback. A separate lo-DOWN
reproducer fails immediately with zero traffic; lo-UP passes all 1000 cycles.
Network now enables only proven-private setup lo after inherited namespace
identity and fresh-link checks. Integrated Network/run_churn verification in
`phase8-churn-loopback-integrated-20261014` passes 1000 cycles, 32832 datagrams
per direction, no generator/cross/integrity errors, released listeners,
0.2284s reclamation and per-role RSS growth <=217088 bytes. Host lo is untouched.

Frozen source/binary hashes matched before these fixes. All original measured
files remain unchanged. Derived evaluation artifacts in
`phase8-r3-measurement-analysis-20261014` bind original manifest/pair/raw-log
hashes and separate corrected churn hashes. This is a post-failure continuation,
not an uninterrupted successful original cycle or a renewed wall-duration claim.

A second accounting defect incorrectly added ordinary `diagnostics_dropped`
to status `coverage_lost`. The diagnostics contract explicitly separates
ordinary token-bucket suppression from reserved status/ring/write failure.
Coverage now sums actual coverage_lost and status_write_failures only. The
62 affected original aggregates are preserved; derived corrected fields are
computed from immutable raw final snapshots with recorded provenance. Actual
status loss/write failures remain invalidating; no SC threshold changed.

**Qualification remains invalid/unsuccessful.** Derived strict Rust evaluation
reports 30 insufficient tunnel RTT-sample runs plus one unsustainable baseline,
and 20 failed numerical checks in 16 paired workloads: 18 SC-004 added-loss
checks and two SC-007 delivered-load directions. No generator errors remain
in the 150 measured baseline/tunnel runs. No missing RTT samples or bad baseline
are waived. Invalid paired runs do not yield performance acceptance.

For 1-session/128-byte forward subcapacity, baseline loss is 0–0.0774%,
while tunnel loss is 0.3735–3.6185%, beyond the 0.1 percentage-point added-loss
bound. Resource pair 2 baseline delivers 500.678/500.736 Mbit/s; tunnel delivers
368.330/490.186 Mbit/s, below 500 Mbit/s both ways. The other two resource pairs
pass delivered load; they cannot average away the failed pair. Resource pair 2
server owner is 0.938 core and records 27.3M receive/error-queue syscall visits;
this suggests a service-cost bottleneck worth targeted profiling, not a proven
root cause or justification for speculative optimization.

All 87 Python tests and diff checks pass after the runner fixes; production
source/binary remain unchanged. T077/T078 stay unchecked. No complete rerun
is active: next work is targeted profiling/reproduction of actual loss/resource
failures and handling insufficient evidence under the unchanged criteria.
T079 remains excluded, native ARM64 unqualified.

## Targeted packet-service investigation — 2026-10-15 (in progress)

Original production binary/source and five matching old-binary repetitions per
workload are retained in `benchmarks/results/phase8-performance-fix-20261015`.
The frozen prospective policy is
`benchmarks/policies/phase8-reactor-service.json`, SHA-256
`27c93dfb58f5a248521d7966a5f7a172db9a5b527d6fe17cafa6500a192065a3`.
Each focused run uses the same two-leg native 1 Gbit/s / 20ms topology, affinity,
3s loaded warmup, 15s measurement, exact workload and generator/coverage gates:
1x128 forward at 76293 pps, and 32x1200 simultaneous at 52160 pps per direction.
The short resource profile does not replace final 60s SC-007 qualification.

Candidates replace unconditional empty receive/error-queue probes with
owner-local readiness and bounded continuation, use fair multi-round enqueue,
and split incoming/application/output work into finite visits. An initial
fixed small outer-read batch reduced small-packet loss but increased packet/ACK
work and RSS. A packet-dependent active-session visit list destabilized the
positional cursor under reverse traffic; it was rejected and stable membership
restored. Density-based reads retain broad batches for large DATAGRAMs while
reducing batches after observing coalesced small DATAGRAMs. This is an adaptive
service heuristic with bounded overload drops, not a zero-loss guarantee for
arbitrary hostile frame-density changes. See [ADR 0015](../adr/0015-bounded-reactor-packet-service.md).

All unsuccessful prototypes remain failed under the original policy:
prototype 1 failed small-packet loss; prototypes 2/3 failed RSS; prototype 3's
separate smaps diagnostic also exposed substantial loss; prototype 4 failed
reverse resource delivery; prototype 5 passed loss/delivery but exceeded the
client RSS regression bound by 163840 bytes. None is retroactively accepted,
and smaps-instrumented runs are diagnostic evidence only.

Prototype 5 native checks: 170 Rust tests passed with two external-peer
cases ignored for separate explicit invocation; formatting, Clippy and locked
qlog release build pass. Tests exercise actual readiness/rearming, a complete
single-flow packet quota and native coalesced-versus-large DATAGRAM observations.
These results do not transfer full functional/release acceptance. T076 is
reopened for changed production source; T077/T078 remain pending. T079 is excluded.


Author protocol/security/ownership review for the packet-service candidate:
no wire/parser, authentication, target policy, generation, commitment,
expiry, partial-send or close rule is relaxed. Unadmitted connections cannot
reduce a shared-socket read quota; the native coalescing regression checks
before admission, after active admission and after closure. Density influences
only bounded scheduling after admission, not authorization or queue capacity.
Excess-cache retirement touches only idle owned slabs and retains all lent
storage/return slots; native and source-included Miri regressions cover this.
No unsafe operation, dependency or external API/configuration change is added.
A cache helper is added to the existing Rust buffer API for native ownership
verification. Independent review was not requested; full interoperability and
functional qualification on the final candidate remain required.

Current candidate12 additionally fixes the reproduced native-backpressure cursor
bug (regression fails on the original production source) and the four-packet
physical flush bottleneck (native FIFO regression fails before the correction).
Generation/submission each use 16 items per output visit; timers/cancellation,
native queues, expiry, pacing and immutable partial-send rules are preserved.
Native Rust checks: 173 pass, two externally enabled cases reserved for the
full runner; Clippy and locked qlog build pass. Three source-included Miri
ownership cases pass. No quiche patch, CC/default-buffer change or new unsafe
operation/dependency is included. The candidate release SHA-256 is
`f974465bd01313885c92e9ca644a06be1bd159b6d634ba44939d08c72026932a`.

`comparison-output12` passes all ten short candidate checks under the frozen
reviewed RSS policy, with zero candidate loss. Its original strict RSS checks
remain failed. `resource12` then fails one of three 60s repeats (reverse
458.398 Mbit/s / 8.45514% loss). This prevents claiming stable qualification.
Numeric quiche-only diagnostic builds showed early outer congestion losses,
with application expiry amplifying the reduced CUBIC window; their precise
origin remains unproven. Larger socket/native queues or disabling HyStart were
not complete fixes and were not adopted.

The resource collector also had unrestricted CPUs 0–23, overlapping role masks
0–3 and 4–7. The runner now isolates it on tools CPUs 8–23, verifies placement at
every sample and restores caller affinity on every exit. The 89-test Python
suite covers real affinity restoration and invalid-placement rejection. On
this host tools and roles do not share physical siblings. Old measurements
retain their original collector method; fresh matched reference/policy and
comparison are required for regression claims.

`resource-pinned12` passes all three paired 60s absolute resource probes on that
same binary: forward loss 0–0.000607%, reverse zero, >=500.73296 Mbit/s both
ways, p95 RTT <=20.588ms, CPU <=0.919 core and RSS <=12865536 bytes per role.
Generators, integrity, clock and coverage are valid. This is encouraging target
evidence, not proof of sole causation or complete T077 acceptance. T076 remains
reopened and T077/T078 unchecked pending fresh matched comparison and final
source-bound qualification. T079 stays excluded; ARM64 remains unqualified.

## Malformed small QUIC packet root cause — current candidate13

The subsequent matched `comparison-pinned12` fails its second resource candidate
repeat: reverse 278.44928 Mbit/s, 44.392% loss, p95 39.091ms and insufficient
RTT samples. It stops at the first failed check. Collector placement therefore
is not a sufficient correction. The prepared full VM qualification is not
launched on candidate12.

A temporary diagnostic build identifies three early 36-byte PING-only QUIC
packets. They are submitted within 14us and received for a known connection,
but never become freshly accepted QUIC packets. Official quiche 0.30.0 can
emit PING after its congestion-credit clamp leaves only 1–3 payload bytes;
the later minimum-padding insertion then silently fails. For short packet
numbers the header-protection sample includes bytes outside the transmitted
suffix, so the peer drops the packet and the sender reduces its CUBIC window.
This accounts for the previously observed control-loss/expiry amplification.

The checksum-verified local patch defers new small control output until its
minimum padded payload fits. ACK-only output and upstream PTO exemptions remain
intact. Only dependency `src/lib.rs` and `src/tests.rs` change; no diagnostic
patch, CC/HyStart/default queue change or new unsafe operation is imported.
[ADR0016](../adr/0016-quiche-minimum-payload.md) and `vendor/README.md` record
provenance, reviewed behavior and removal conditions. The exact official archive
SHA and all original/reviewed file hashes are retained and checked. Release,
benchmark, fuzz and dependency evidence now bind those local inputs.

Native checks on the imported source: all 24 new dependency cases fail before
and pass after; full quiche library suite 1164 passed, Remora 173 passed with two
external-peer cases reserved for separate explicit invocation, Python 92 passed,
Clippy -D warnings, formatting and locked qlog release build pass. Current binary
SHA-256: `ff75ff9fd493235e16affca4b8827d6514ebb6617b6ae160381c2bbd3312bc76`.
Resource/paired comparison and fresh source-bound fuzz/advisory/interop/VM
qualification remain pending. Earlier candidate12 or original T076 passes
cannot qualify this executable. T073/T075/T076/T077/T078 remain unchecked;
T074's source-included buffer/syscall safety evidence remains scoped to those
unchanged safety inputs. T079 is excluded, ARM64 unqualified, no release claimed.

Candidate13's `resource-patched13` completes three paired 60s native probes:
all generator/integrity/clock/coverage/sample gates and unchanged
SC004/SC005/SC007 assertions pass. Forward delivery is
500.72352/500.736/500.736 Mbit/s, reverse 500.736 each; forward loss
0.002492%/0/0, reverse zero. p95 RTT <=20.589ms, per-role CPU <=0.916 core and
RSS <=12824576 bytes. These are targeted resource checks, not the full matrix.

A guarded background continuation is launched at
`benchmarks/results/phase8-padding-followup-20261015/progress.json`:
fresh matching old reference, prospective policy freeze, five paired
old/new/direct repeats per affected workload, then only after every check passes
new source-bound T073/T074/T075 and full VM-enabled T076. Comparative stage
budget20min; qualification stage budget90min. It stops on the first failure,
preserves artifacts and stops owned VMs after the functional stage. Its child
qualification progress is in
`benchmarks/results/phase8-packet-fix-qualification-20261015/progress.json` once
that stage starts. Full numerical T077 is not automatically chained, and no
pending task is marked passed merely because this continuation was launched.


## Candidate13 full T077 matrix — running

After the successful source-bound T076 continuation, the unchanged native
x86_64 full reference profile starts at 2026-10-15T12:55:33 UTC in
`benchmarks/results/release/phase8-x86-patched13-20261015`. Its detailed
`reference/benchmark/progress.json` records calibration and paired workloads;
`benchmarks/results/phase8-patched13-reference-20261015/progress.json` records
the input-bound guard and final status. The guard binds the passed qualification
summary and exact production/runner/profile/binary hashes, retains the plan and
requires no changes during measurement. This is a fresh full 18-search/75-pair
cycle with unchanged thresholds and a 90-minute limit, not a relabelled targeted
comparison. T077 and final T078 remain pending until their evidence exists.


## Candidate13 full T077 outcome — unsuccessful

The full cycle completes in 80m12s: 108 calibration points, all 75 pairs and
valid 1000-cycle churn are retained. The final validator rejects the result
with one invalid pair and two numerical failures; the last `stage: churn`
progress pointer does not mean churn failed. Churn passed and the exception
was raised by final numerical validation.

| Pair index | Workload / repetition (zero-based) | Recorded problem |
|---|---|---|
| 42 | Capacity, 32 sessions, 1200 bytes, simultaneous, repeat 0 | RTT sample counts 74–103; 30 sessions below 100. Forward delivery 594.5024 versus baseline 874.9056 Mbit/s, measured loss 32.049538%. The validator skips numerical gates for this invalid pair, so the large delivery deficit must also remain visible. |
| 67 | Subcapacity, one session, 128 bytes, simultaneous, repeat 0 | Forward loss 0.604584% versus baseline 0.008430%, added 0.596154 percentage points, exceeding SC-004's 0.1-point bound. |
| 74 | Resource, 32 sessions, 1200 bytes, simultaneous, repeat 1 | Forward delivery 485.01696 Mbit/s versus required 500, with 3.139187% measured loss. Reverse delivers 500.736 Mbit/s without measured loss. |

All recorded paired-run generator error counts are zero. Other repetitions of
these workloads can pass; the failures therefore cannot be averaged away.
The two failing 32-session runs show owned UDP socket drop increments and local
queue expiry/full counters, with corresponding outer QUIC loss declarations.
The one-session small-packet run has local queue drops without observed socket
RX overflow. Final diagnostic counts include warmup/drain; their scope differs
from measured traffic loss. The origin of intermittent receive/service deficits
requires targeted investigation; no recurrence of the malformed-PING bug has
been demonstrated. Source, numerical thresholds and failed artifacts are not
changed or relabelled.

Raw criteria remain in
`benchmarks/results/release/phase8-x86-patched13-20261015/reference/benchmark/criteria.json`.
The hash-bound, read-only three-pair analysis is in
`benchmarks/results/phase8-patched13-analysis-20261015/summary.json`.
T076 remains passed for its recorded binary; T077/T078 remain pending.

The separately queued Git-fork verification completes afterward: all 1164
library tests pass on the complete committed fork src tree using the published
0.30.0 manifest/lock, and the remote branch SHA matches
`154ce1995f23efa434e72dac269f707145be2015`. See
`benchmarks/results/phase8-quiche-fork-20261015/summary.json`. This did not overlap
T077 measurement and did not change Remora's dependency source. Git-source
integration remains pending; T079 is excluded.


## Pinned Git-fork migration — completed

The user requested the dependency-source migration before further loss
investigation. Root/traffic/fuzz manifests and locks now pin
`https://github.com/0xf836/quiche` at
`154ce1995f23efa434e72dac269f707145be2015`. All other resolved package entries
are unchanged; the lock comparison is retained with the migration evidence.
No application Rust source, queue/scheduling/CC policy or numerical threshold
changed during this migration. The vendored source/old verifier are archived at
`/tmp/remora-quiche-vendor-archive-20261015`, outside the project. The previous
measured production/traffic executables and locks are retained under
`benchmarks/results/phase8-quiche-git-migration-20261015/before`.

The new provenance policy in `dependencies/quiche.json` binds the Git commit,
upstream base/archive, source SHA-256 values and reviewed symlinks. Release,
fuzz, dependency and benchmark runners now bind actual Git-cache inputs rather
than the removed local tree. Native CI fetches locked dependencies, verifies
the pinned source, and tests it through an ephemeral published-crate harness.
No dependency Rust sources or deployment credentials are added to the project.

`benchmarks/results/phase8-quiche-git-migration-20261015/checks/summary.json`
reports all 16 stages `checks-passed`: locked default/qlog builds, format,
Clippy, 173 ordinary Rust tests and one explicit external shutdown test,
1164 quiche library tests, 94 Python tests, 20 traffic tests/build/lint,
operator diagnostics and independent interoperability. All 22 dependency checks
pass, as do four new 30-second ASan fuzz targets with unchanged inputs and
per-target Git-provenance verification. Existing pure-Rust Miri safety inputs
are unchanged; Miri was not rerun for this distribution-only change.

New qlog release SHA-256:
`3912e2574812ae86af4d3da580511409ed972f1eeef8c3ef9e52c295a24cccba`.
The migration is verified for this build/check scope; it does not qualify
long-duration functional acceptance, performance or ARM64 for the new binary.
T076 is reopened for final source-bound full qualification after the remaining
performance corrections; T077/T078 remain pending. No new long test or
benchmark was launched and no loss fix is claimed by changing source location.
