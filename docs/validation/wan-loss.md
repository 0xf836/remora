# WAN datagram loss remediation

Status: Phase 15 laboratory increment complete, 2026-10-24. Independent
read-only final review: APPROVE. No live-WAN or release qualification is claimed.

## Current selected profile and completed evidence

The implementation and required laboratory measurements are complete, including
the paired 1800-second WG soak. Both intervals and their final hashes/gates have
been checked. Independent read-only final review found no confirmed P0–P3 defect in the
implementation or completed evidence, including the full paired soak.

Selected explicit WAN settings, on both client and server, are CUBIC with
8192 packets, an 8-MiB per-session byte ceiling and 194-ms application residence.
The global application pool remains 8 MiB. Production defaults remain CUBIC /
32 packets / 20 ms; this increment does not silently migrate existing settings.
Set these fields in the existing TOML tables rather than appending duplicate
tables:

```toml
[transport]
cc = "cubic"

[io]
outer_socket_buffer_bytes = 1048576
relay_socket_buffer_bytes = 131072

[limits]
session_queue_packets = 8192
session_queue_bytes = 8388608
application_residence_ms = 194
```

The tested size profile is matched 1452-byte outer UDP / 1400-byte Remora
application / WG MTU 1280. Buffer requests remain unchanged: larger outer-only
and relay-only buffers did not eliminate the observed capacity regression.
This is a configuration for the tested workload, not peer-cap negotiation or
a claim about a real WAN PMTU.

The retained 194-ms WG matrix contains 18 RTT/size/direction combinations,
three repeats each, with independently valid paired controls:

| Configured RTT | Cold 20-second payload loss | Original-sequence loss after 2 s |
| --- | --- | --- |
| 55 ms | 0.63289–0.7492% | 0% |
| 60 ms | 0.80833–0.9608% | 0% |
| 100 ms | 2.34556–2.7528% | 0% |

The 100-ms figures use the explicitly approved startup-v2 bounds, including the
20-MB startup payload-loss ceiling; they do not pass the original 1% cold gate.
Across these 54 normal runs, maximum measured added one-way delay was
196.523 ms. The 21 disturbance runs (seven cases, three repeats) passed their
unchanged bounds: exact five-packet drops, small reverse-path disturbance and
5/10-ms owner pauses. Maximum steady added loss was 0.036534 percentage points;
complete arrival-second recovery windows returned to >=190 Mbit/s within
0.999 seconds and held it for three full seconds. Both 20/50/100/200-Mbit/s
step series passed the final high-rate recovery and steady-delivery checks.
Maximum steady added RTT p95 across all 81 selected runs was 0.645 ms.

The same-binary controller comparison retained three BBR2 cold-loss failures.
CUBIC remains selected. The 190-ms profile's 1.0064% cold failure is retained;
the prospectively declared 194/195-ms refinement passed all three repeats for
each variant, and 194 ms provided the lower latency tail. Separate fresh
32-session direct/candidate capacity and resource comparisons for 194 ms passed
all six paired checks (12 measurements). They supplement the completed code
regression; unchanged HEAD/code pairs were not repeated.

Current build/check evidence is bound to binary
`d2dcfa79f206b8b830950b5dc9ad33ab3b982aa3b7ed9e8eb49749c718bc8019`:
pinned Rust 1.99.0, 238 native tests plus the separately executed held-response
shutdown case, 30 traffic tests, 144 Python tests and 29 independent Go cases.
Four seeded noninstrumented fuzz smokes passed; sanitizer/coverage qualification
is unavailable and is not inferred. The original unknown-1200-byte parallel
fixture failure remains unattributed. At laboratory acceptance, changes for this increment had not been installed
on the real client or VPS, committed, released or pushed.

The final paired soak used WG1280, 100-ms RTT, 1200-byte payloads and simultaneous
200-Mbit/s offered traffic in each direction, without a loaded warmup. Each arm
submitted exactly 37,499,400 datagrams per direction. The matched control received
every datagram and had no validity errors. The candidate passed every applicable
gate:

| Candidate 1800-second interval | Forward | Reverse |
| --- | --- | --- |
| Unique received datagrams | 37,487,992 | 37,489,273 |
| Delivered payload rate | 199.935957 Mbit/s | 199.942789 Mbit/s |
| Whole-interval loss including cold start | 0.030422% | 0.027006% |
| Loss in original sequence slots after 2 s | 0.000278% (104 datagrams) | 0.000091% (34 datagrams) |
| Maximum added one-way delay | 194.414 ms | 194.610 ms |

Forward missing slots occurred only in original seconds 0, 1 and 2 (9967,
1337 and 104). Reverse missing slots occurred in seconds 0 and 932 (10093
and 34). Thus the long interval is not described as lossless. Aggregate server
kernel RX overflow was 50 and client outer QUIC loss was 16; those counters do
not establish the exact provenance of each missing endpoint payload. Neither
role had a pressure-window discard, watchdog timeout or unexpected reconnect.
All loss over the full interval was 13,689,600 / 12,152,400 payload bytes, below
the unchanged 20-MB startup budget even when the late residual is included.
The dedicated short-run cold-start matrix above remains separate evidence.

Steady RTT p95 was 101.141 ms against control 101.726 ms; this passed the added
RTT limit and is not a claimed causal latency improvement. Candidate process
CPU averaged 0.3142 / 0.3212 cores and peak RSS was 17,764,352 / 17,412,096 bytes
(client/server). The per-role RSS medians were identical in the first and last
five-minute windows. Both arms have all 1800 original-sequence bins, 1801 arrival
bins, and whole-interval resource samples spanning about 1803 seconds; WG
handshakes and bidirectional counters were present. Counts reconcile with raw
endpoint summaries. The bounded 30-second timeline was not substituted for
these full-interval records. Frozen source, tool, policy and binary hashes
matched after completion. See `pressure-wan194-finish/soak100/`,
`soak-whole-interval-inspection.json` and `soak-residual-loss.json`.

Primary completed results are in `pressure-regression-cpu-tradeoff/`,
`profile194-regression/`, `pressure-wan194-campaign/` and the separately labelled
corrected assessments in `pressure-wan194-finish/retained-gates/` under the
ignored artifact root below. Original failures, policies and measurements remain.

The owner approved implementation and laboratory validation of the conversation
plan on 2026-10-23, with a transitional added one-way delay ceiling of 200 ms,
at most 1% endpoint loss over a complete cold 20-second run, and at most 0.1
percentage points added loss after its first two seconds. A different default
congestion controller is permitted only after comparison. Live installation is
excluded. An independent read-only reviewer is required before completion.

## Work and evidence gates

- [x] Selective outgoing native expiry and exact accounting regressions.
- [x] Progress-aware owner continuation under transport/native backpressure.
- [x] Fresh bounded QUIC diagnostics and reproducible WAN laboratory runner.
- [x] Same-binary CUBIC/BBR2 and queue/residence selection under frozen policy.
- [x] Applicable native/lifecycle/parser/property checks, independent interoperability and noninstrumented fuzz smokes (sanitizer qualification remains unavailable).
- [x] Final paired selected-profile soak, including whole-interval count/resource/hash checks.
- [x] Final independent soak evidence/documentation review: APPROVE; Phase 15 completion is recorded in `pressure-wan194-finish/laboratory-acceptance.json`.

Policy: `benchmarks/policies/wan-loss.json`. The baseline is the pre-change
checkout, built and retained with its hash under ignored `benchmarks/results`.
Each run records binary/source/config/runner hashes and its actual topology.
Instrumented evidence is distinct from production performance evidence.

## Earlier observations (not full acceptance)

Raw evidence is under ignored `benchmarks/results/wan-loss-20261023/`.
The pre-change binary is `control-remora`; `expiry-only-remora` additionally
preserves the candidate before the BBR pacing correction. Individual manifests
identify each executable, runner, traffic tool, source variants and CPU placement.
Later measurements do not retroactively qualify earlier executables or invalid
runs.

- Selective-expiry and native-full continuation regressions have assertion
  failures before the correction and passing targeted checks afterward. Expiry
  during generation survives an error/stop and is consumed once.
- The complete native suite passed serialized before the pacing correction.
  A parallel run reported an unknown 1200-byte UDP payload in an E2E fixture;
  the cause remains unattributed. Serialization is not a fix for that failure.
- `screen-profiles`: at 55 ms RTT, a single 20-second forward run of the
  expiry-corrected executable lost 3.442% with original CUBIC settings, 1.483%
  with 512 packets/100 ms, and 1.078% with 2048 packets/200 ms. The BBR2 values
  were 2.782%, 2.664% and 2.311%. These used 100-us generator pacing.
- `screen-maximum`: at 55 ms RTT and 1-ms generator pacing, CUBIC with 8192
  packets/200 ms lost 0.598%, BBR2 lost 1.801%, and the direct UDP control lost
  zero. Both tunnel runs had zero loss in original sequence slots after 2 s.
  The maximum one-way values (227.898/247.596 ms) alone do not prove the added
  delay gate: the old control recorded only its maximum, not its minimum.
- `drop5-maximum-cubic`: three 55-ms repeats confirmed exactly five outer data
  drops each. Cold loss was 0.7503%, 0.7925%, 0.7548%; steady loss was
  0.001333% in every repeat. All complete arrival-second recovery windows
  satisfy >=190 Mbit/s within 2 s for 3 s. Paired added-delay/RTT evidence is
  absent, so these are individual passed gates, not full acceptance.
- `cold100-maximum`: all three CUBIC repeats failed cold loss (3.089%, 2.844%,
  2.909%) while steady original-sequence loss was zero. All three old BBR2
  integration repeats failed (99.99784% total loss, repeated transport 1001).
  The three direct UDP controls lost zero. Defaults remain unchanged.
- The BBR2 pacing regression demonstrates over-generation before the
  correction and passes afterward. The comparative namespace run is separate
  evidence and must be checked before making a runtime claim.
- `wg-smoke` is invalid: a generator bound to loopback could not send to the
  inner WG route. `wg-smoke-source` has traffic but includes WG's retry/setup
  delay, not a ready-tunnel comparison. `wg-ready55` candidate passed integrity
  and delivered 198.741 Mbit/s with 0.628% cold loss and zero steady loss;
  steady RTT p95 was 55.727 ms. Its plaintext control is invalid at the old
  1200-byte cap. None establishes full WG acceptance.

`scripts/validate/wan_loss_report.py` evaluates individual gates and reports
missing evidence. Recovery uses full arrival seconds starting after the actual
fault, never a fractional earlier second. Added delay subtracts the paired
baseline minimum from the candidate maximum. Steady RTT uses only probes sent
after the first two seconds. Whole-run RTT p95 and differences between two
maxima cannot stand in for those gates. Individual runner/report outputs intentionally keep
`acceptance_complete=false`: a screen alone cannot qualify the full increment.
After the complete matrix, regressions, soak and review, Phase 15 laboratory
completion is recorded separately in
`pressure-wan194-finish/laboratory-acceptance.json`; it does not qualify a release
or a live WAN deployment.

## Earlier automated-check checkpoint and environment

This historical checkpoint precedes the final pinned pressure-retry checks.
For current totals and remaining work, use the opening section.

After the BBR pacing correction, the full all-target/all-feature Rust suite
passed serialized: **235 passed, 3 prerequisite-dependent ignored**. The
independent local Go matrix passed **29 cases**, using the separately retained
Go1.27.2/masque-go0.6.0/quic-go0.63.0 peer. Root Clippy passed with warnings
denied. The traffic package subsequently passed **30 tests** and warnings-denied
Clippy, and the full Python suite passed **137 tests** with the required CPU
affinity available. These results precede the experimental HyStart configuration
and startup-exit telemetry; the latter has a passing targeted configuration test,
but at that checkpoint the full Rust suite still required a rerun after the
final implementation. That rerun subsequently passed as recorded above.

This machine has system Rust/Cargo **1.99.1**, while `rust-toolchain.toml` pins
**1.99.0**. The official download failed with TLS unexpected EOF. The pinned
compiler was subsequently located in an existing local build image and exported
to `/tmp/remora-rust-1.99.0`; the system installation was unchanged. A pinned
release build and targeted configuration test passed. Those earlier full-suite results
remain system-toolchain evidence; the later pinned rerun is recorded separately. The sanitizer nightly and
`cargo-fuzz` remain unavailable. Four ten-second noninstrumented fuzz-adapter
smokes passed; these do not establish sanitizer or coverage qualification.

The 190-ms `bounded` queue candidate was added prospectively in
`benchmarks/policies/wan-loss-candidates-v2.json`: it reserves 10 ms of the
unchanged end-to-end 200-ms ceiling for local service and bounded pacing
lookahead. Every numerical acceptance threshold in `wan-loss.json` remains
unchanged. A failed original 200-ms candidate remains failed or incomplete.

The test generator now supports explicit equal 20/50/100/200-Mbit/s phases and
one bounded 1800-second interval. Step tests preserve total slot counts and exact
phase boundaries; final high-rate delivery/recovery cannot be inferred from
low-rate phases. At that checkpoint this was tooling capability; the final paired soak status is
recorded in the opening section.

## Cold-start budget at 100 ms RTT

The CUBIC snapshot approximately 8 ms before the loaded start in
`cold100-maximum/0-candidate-cubic-maximum` records a 14,520-byte congestion
window and about 100.5 ms RTT. The 200-Mbit/s offered payload is 25 MB/s and
its 100-ms bandwidth-delay product is 2.5 MB.

An intentionally optimistic ordinary slow-start model gives the sender the
entire current window immediately at each RTT boundary, doubles it every RTT,
ignores every protocol byte and service cost, and permits a full 200 ms of
queueing. Immediately before 800 ms, its first eight windows total
`14520 * (1 + 2 + ... + 128) = 3,702,600` bytes. The source has offered 20 MB;
at most its last 5 MB fit the 200-ms age bound. The excess is 11,297,400 bytes,
or about **2.26% of a 20-second 500-MB run**. Actual old-integration CUBIC
measurements lose 2.84–3.09% during startup and zero afterward.

This is an explanatory model under the stated initial-window/RTT/ordinary
slow-start assumptions, not a universal impossibility theorem or an acceptance
result. It identifies a conflict to resolve before calling the full plan done:
the current transport's cold start at 100 ms does not meet 1% while also keeping
the 200-ms added-delay bound. Merely enlarging an age-bounded queue cannot
remove this startup deficit. Do not hide a loaded warmup, enlarge the initial
window beyond protocol policy, extend the ciphertext lifetime or weaken the
recorded gates to make the comparison pass.

## Earlier owner-approved startup revision and repeated gates

On 2026-10-24 the owner chose to retain the <=200-ms added-delay ceiling and
separately normalize cold startup at 100 ms RTT. Before the new 100-ms runs,
`wan-loss-startup-v2.json` fixed a finite 20-MB startup-loss budget (4% of the
20-second/200-Mbit/s payload), >=190 Mbit/s within two seconds held for three
seconds, and all unchanged steady/latency/resource limits. The 55/60-ms cold
limit remains 1%. Original 100-ms runs without this policy in their manifest
remain failed under the original criteria.

These individually checked scenarios use the `bounded` profile and CUBIC.
Each has three repeats with a valid matched plaintext control. Loss ranges
include every active direction; added delay is candidate maximum minus paired
control minimum. All listed individual gates passed at that earlier checkpoint;
the full phase was still pending. Current completion is recorded at the top.

| Evidence directory | Workload | Cold loss | Maximum steady loss | Maximum added one-way delay |
| --- | --- | --- | --- | --- |
| `wg-bounded55` | WG1280, 55 ms, 1200 bytes, forward | 0.666–0.687% | 0% | 190.303 ms |
| `wg-bounded60-reverse1000` | WG1280, 60 ms, 1000 bytes, reverse | 0.914–0.963% | 0% | 190.334 ms |
| `wg-bounded100-simultaneous` | WG1280, 100 ms, 1200 bytes, both directions | 2.347–2.631% | 0% | 190.674 ms |
| `steps55-bounded` | Generic UDP, 55 ms, 20/50/100/200-Mbit/s phases | 0% | 0% | 140.389 ms |
| `pause10-wg55-simultaneous` | WG1280, 55 ms, both directions; 10-ms owned-process pause | 0.670–0.689% | 0.0304% | 190.618 ms |

All these steady added RTT p95 values are below 5 ms. BBR2 with the corrected
pacing and 190-ms profile failed the unchanged 55-ms cold gate in every repeat
(about 1.12%); no controller change is justified. The 10-ms pause's first repeat
has 96 client kernel RX overflows matching 96 additional lost datagrams, with
zero outer QUIC loss reported. This is counted loss, not erased from the result.

The endpoint timeline is a numeric <=3000-bin prefix at 10 ms, separately from
whole-run one-second accounting and once-per-second QUIC state. It covers
offered/unique arrival counts and same-sender probe peaks, not raw payloads,
keys or high-rate qlog. Its short-run totals reconcile with endpoint counters;
its bounded prefix cannot establish completeness of a 30-minute soak.

Four 10-second libFuzzer smoke runs (`masque`, `policy`, `ancillary`, `lifecycle`)
completed after compiling the adapters on system Rust. They have **no sanitizer
or coverage instrumentation** and do not satisfy the pinned ASan campaign gate.
The independent held-response SIGINT/SIGTERM test also passed explicitly.

The requested read-only reviewer found and rechecked fixes for baseline
validity, mandatory per-session/two-role resource evidence, and freezing of
re-executed shell fixtures. Current runners validate input and binary hashes
before/after each arm and before final output. Source snapshots use a public
whitelist capped at 16 MiB and exclude generated keys/private fixtures.

The prospectively frozen `wan-loss-regression.json` separately compares code
under identical original limits (retaining historical paired CPU/RSS tolerances)
and the larger selected profile under unchanged absolute SC-003/004/005/007
plus paired delivery/loss. Its explicit three-second loaded warmup is the
existing-performance method and cannot establish cold WAN acceptance.

## Pinned checks for bounded-pressure candidate

Candidate `d2dcfa79f206b8b830950b5dc9ad33ab3b982aa3b7ed9e8eb49749c718bc8019`
was built with pinned Rust 1.99.0. Full all-target/all-feature native tests passed
**238 tests, 3 prerequisite-dependent ignored**; the held-response independent
SIGINT/SIGTERM case separately passed (5-ms stop and released listener for each
signal). Root rustfmt and warnings-denied Clippy passed. The separately pinned
traffic package passed 30 tests, Python passed 138, and the independent Go matrix
passed all 29 cases. These are functional/static results, not performance gates.

The full suite first exposed a faulty BBR regression fixture: `max_pacing_rate`
10000 is converted by pinned gcongestion with `from_mbits_per_second`, so it did
not represent the intended slow byte/s pacer. Ten of 30 bounded repeats failed
when default congestion credit, rather than pacing, blocked generation. A lower
cap also constrains CWND by measured RTT in that backend. The corrected fixture
therefore has controlled one-way delay, adequate initial credit verified after
setup, the proper cap units, physical pre-send pacing and bounded waits. Thirty
of 30 repeats then passed, followed by the full native suite. Production BBR
settings/code were not changed for this fixture failure. Original logs remain.

Four ten-second, seeded, noninstrumented fuzz smokes passed: masque 35,314,554,
policy 31,455,499, ancillary 51,027,423, lifecycle 1,278 executions. The initial
policy/lifecycle smoke lacked the mandatory fixture environment and did not
start; those artifacts remain separately. Sanitizer/coverage qualification is
still unavailable and is not inferred from these smoke counts.

## Capacity startup investigation, 2026-10-24

The selected 8192-packet/190-ms application profile failed the focused 32-session
capacity comparison. It is not promoted. A same-executable HyStart on/off screen
retained the failed result and stopped prospectively: on lost 0.167/0.108%, off
lost 0.959/2.061%. This does not support disabling HyStart as the remedy.

A subsequent diagnostic comparison kept the executable, HyStart enabled,
application profile, 20-ms RTT, 1-Gbit/s rate and 32-session workload fixed, and
changed only both private netem queue limits. Order was 2048, 8192, 8192, 2048.
All four measurements had zero generator/integrity/coverage errors, generator
CPU below 0.46 core and clock drift below 0.002 ms.

| Netem limit | Repeat | Forward/reverse loss | RTT p95 | Client/server send pressure |
| --- | --- | --- | --- | --- |
| 2048 | 0 | 0.0695% / 0.0167% | 21.166 ms | 98 / 16 |
| 8192 | 0 | 0% / 0% | 21.052 ms | 0 / 0 |
| 8192 | 1 | 0% / 0% | 21.050 ms | 0 / 0 |
| 2048 | 1 | 2.8735% / 4.8766% | 136.320 ms | 130 / 39 |

Artifacts are in `benchmarks/results/wan-loss-20261023/qdisc-comparison/`.
This supports sensitivity to early local output pressure and substantial run
variability. It is not an acceptance comparison: increasing the netem limit
also permits more network buffering. The driver retained its initial topology
snapshot and successful shaping commands, but not per-arm qdisc drop-counter
snapshots. Do not attribute every packet to a specific kernel queue from these
data, or use the larger limit to replace the original regression topology.

The original failing capacity snapshots show effective outer RX/TX buffers of
2 MiB and application RX/TX buffers of 256 KiB on both endpoints (requested
1 MiB and 128 KiB respectively); system maxima were 64 MiB. No clamp was found.
This verifies effective sizes, not adequacy under bursts. The subsequent
`socket-buffer-comparison/` retained nine valid measurements on the original
topology, with three rotated repeats each of default buffers, outer request
increased to 4 MiB only, and relay request increased to 1 MiB only. Every arm
verified effective sizes and pre/post qdisc settings/counters. Default loss
ranged 0.424–3.092%; outer-only 0.005–2.231%; relay-only 0.019–2.773% across
directions/repeats. Every variant retained synchronous output pressure and
qdisc drops. Neither larger buffer eliminated the regression; defaults remain
unchanged. The full rows and qdisc deltas are in `interpretation.json`.

The next prospectively bounded experiment proposed retaining an unaccepted outer packet
after ENOBUFS for 1-ms timer retries within a maximum 5-ms window keyed by
ciphertext generation/sequence. Original expiry, immutable bytes, FIFO prefix
progress and connection-local fairness remained required. That proposal was
frozen under `wan-loss-pressure-retry.json` before implementation and the
comparison below. The proposal itself was not proof of a fix or of the
200-ms end-to-end delay ceiling.

`pressure-comparison/` subsequently completed three alternating matched pairs
on the original topology, same pinned compiler, unchanged buffers and workload.
All measurements were valid. Candidate endpoint loss, QUIC loss and pressure
window discards were zero in all three repeats; RTT p95 was 21.046–21.071 ms.
Pre-retry control loss ranged 0.017–1.947% across directions/repeats, with RTT
p95 25.209–92.051 ms. Candidate still observed 2–28 failed submissions per role:
those failures no longer became immediate local packet discards. This supports
the local-pressure mechanism, not release/WAN acceptance.

The diagnostic driver's compact array defaulted the missing new window-discard
counter to zero for old control. That zero does not describe old immediate
discards: each old failed submission discarded its front. `interpretation.json`
retains explicit per-role semantics and pre/post qdisc deltas from raw artifacts.
The original frozen performance regression was the next required gate at
that checkpoint; its later result and the accepted CPU tradeoff follow below.

`pressure-focused-regression/` retained a failure on repeat 2 of the small-packet
workload after the first six workload checks passed. No traffic/clock/generator
validity error occurred; candidate original-limit process/owner CPU exceeded the
paired 5% plus 0.01-core allowance on both roles. Process CPU was 0.459/0.487
cores against control 0.409/0.425. Delivery and latency passed. This remains a
failed regression, not a variance waiver; profiling must precede further changes.

The owner then explicitly accepted the observed approximately 0.05–0.06-core
CPU increase and requested returning to delivery/latency work. Relative paired
CPU became informational only for this Phase 15 increment under the separate
`wan-loss-regression-v2.json`; absolute CPU <=2 cores, RSS <=128 MiB, paired RSS,
delivery, loss and latency bounds remain unchanged. The original strict-CPU
failure remains failed. The partial profiler was stopped and preserved.

`pressure-regression-cpu-tradeoff/` verified unchanged dataplane/tool/reference
inputs and retained the original 28 raw measurements, clearly reassessed
retrospectively under that user-approved CPU tradeoff. It completed only the
eight missing measurements prospectively. All nine workload/repeat checks and
36 measurements now satisfy the nonwaived gates. This does not qualify the
original strict CPU tolerance or the whole release reference matrix.

Before the next WAN freeze, resource validation now requires two complete finite
role records in both runner and report. Candidate CPU is bounded by the existing
SC-007 two-core limit, explicitly recorded in manifest/report; baseline CPU is
not silently given a new absolute bound. Missing/partial recovery records can no
longer omit the unexpected-recovery gate. Original policy JSON is unchanged.

The earlier live investigation is retained locally in
`/tmp/remora-live-diagnosis-20261023.md`. Its last extra capture filled a small
server `/run` tmpfs and is invalid for comparative conclusions. Future captures
must preflight storage, enforce size/time limits and validate complete output.
