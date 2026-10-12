# Performance evidence

This report retains measured outcomes and limits. Raw manifests, samples, logs
and historical comparisons remain in ignored local `benchmarks/results/`
archives, not Git. See [artifact retention](validation/README.md).

## Phase 4 independent review corrections — 2026-10-12

The corrected executable SHA-256 is
`9a7504b55ed9ab90425e0f152da3c9d59851fc19d6dbe610e1ef8fd967e39d53`.
Local critical HTTP/3 stream STOP now fails its connection, and commitment/
active-capacity checks each use a four-Session cursor instead of full native
checks across all Sessions. A full quota making progress with candidates left
requests continuation; waiting receipts/ordinary healthy capacity do not.
ADR-0006/0008 clarify item quotas and remove the unsupported fixed native-call
count. These are correctness/service-bound corrections, not an optimization claim.

`phase4-review-qualified-candidate/comparison.json` passes 12/12 comparisons
against `phase4-reference-native` and its unchanged frozen policy. The profile
remains one stream, 128 bytes, 1000 pps, two-second warmup, three-second
measurement and two paired repetitions. Every active direction delivers
3000/3000 datagrams (1.024 Mbit/s), without measured loss/integrity/generator
errors. The comparison runs serially before the separate process laboratories.

| Corrected workload | Baseline p95 RTT, ms | Tunnel p95 RTT, ms |
|---|---:|---:|
| Forward | 0.817–0.818 | 0.673–0.721 |
| Reverse | 0.694–0.750 | 0.705–0.841 |
| Simultaneous | 0.660–0.761 | 0.472–0.661 |

Simultaneous client/server process CPU is 0.153–0.221 / 0.160–0.234 cores;
owner CPU is 0.150–0.217 / 0.156–0.231. Peak RSS is 8.13–8.25 / 8.11–8.20 MiB.
These small samples meet the same development gate; they establish neither a
significant speedup nor release capacity. The original duration/allocator
results below retain their original executable hashes. New regression/process/
independent-peer evidence and the retained incomplete baseline-tool attempt are
described in [concurrency validation](validation/concurrency.md).

## Phase 4 concurrency comparison before independent review corrections — 2026-10-12

Before production Phase 4 edits, `phase4-reference-native` recorded the
pre-change executable
`767d143ca6e32f62f28da0bee3d6cc8446e8ace4033c6a915e699f945bd86a99`.
The frozen policy binds reference manifest SHA-256
`f8d4190dd4c412a8c977df51b6beab5a4967e247a73163afabcd743fb17be5e8`
and summary SHA-256
`d255e9fb3b365b79acbd1c72f9ff5d210e80c50812dc555fd0c2aa81c08410a2`.
It copies the existing development policy without changing any numerical bound,
including the previously authorized provisional 0.5-core process/owner ceiling.
That CPU ceiling remains a development smoke gate, not calibrated regression
tolerance or a relaxed release criterion.

The final `phase4-candidate-complete` executable SHA-256 is
`f0d22b9400ce1666cff4cac86f611cbe579c4df5dbada79710f70aff814f4e3a`.
After production checks/builds it runs the same profile serially, before the
duration qualification laboratories, without overlapping builds/tests.
All 12 per-run comparisons pass. Each active direction delivers 3000/3000
packets (1.024 Mbit/s), with zero measured loss, integrity or generator errors.
The profile remains one session, 128 bytes, 1000 pps per active direction,
two-second warmup, three-second measurement and two paired repetitions.

| Final workload | Baseline p95 RTT, ms (range) | Tunnel p95 RTT, ms (range) |
|---|---|---|
| Forward | 0.707–0.750 | 0.722–0.829 |
| Reverse | 0.801–0.875 | 0.575–0.777 |
| Simultaneous | 0.762–0.763 | 0.519–0.783 |

Simultaneous tunnel client/server process CPU is 0.190–0.281 / 0.203–0.281
cores; owner CPU is 0.186–0.271 / 0.200–0.281. Peak RSS is 8.08–8.26 /
8.16–8.16 MiB. These small samples establish a passing unchanged development
gate, not a statistically significant speedup or sustainable rate.

The [Phase 4 concurrency record](validation/concurrency.md) separately reports
32-session isolation, low-rate probes during configured budget saturation and
1000-cycle resource reclamation. Final 1800-second isolation has zero
cross-delivery/integrity errors; all 31 continuous flows receive 1800/1800
probes per direction while one association expires/recreates. Final 600-second
overload meets the unchanged per-flow criterion: worst timely forward progress
598/600 (99.6667%), with 600/600 reverse progress for every probe flow.
The final churn run returns session/setup/relay
gauges to zero in 0.4904 seconds; retained RSS growth is at most 0.1641 MiB
against the warmed baseline, below the unchanged 10 MiB criterion. Visible slab,
native/control descriptor capacities, diagnostic ceiling and kernel queue
allocations are reported separately from RSS.

Phase 4 resolves two correctness bottlenecks: the first-four body selection
could starve a fifth pending stream, and unobserved completed STOP_SENDING
streams exhausted native stream credit under churn. Bounded cyclic body/native
service and closed-control reclamation address those failures. Per-session
queues add bounded copies before the shared native queue; no unmeasured
optimization claim follows. Intermediate binaries, the original failed churn
and interrupted duration runs remain retained with their own manifests.

The intermediate `phase4-candidate-ack` comparison fails in five tunnel runs
(7/12 pass): ordinary writable-stream continuation kept the client owner near
one full core, exceeding the existing 0.5-core development ceiling. Only
observed STOP work and a finite closing-control cycle now request continuation;
a normal-writability regression protects that correction. The final candidate
passes the same reference/policy with no changed bound. Earlier completed
isolation/overload/churn runs identify intermediate binaries; they are not
final-revision qualification. FIN-before-ACK permit/metadata reclamation is
separately regression-tested and its retained-memory effect is investigated
in the concurrency report.
The separate instrumented 600-second overload records final warmed allocator
occupancy of 1698608–1761808 bytes, a constant 1994752-byte arena and zero
mapped allocator bytes. The uninstrumented final server RSS stays within
9.406–9.430 MiB from seconds 59–599. Probe overhead and allocator measurements
are explicitly separated from the passing benchmark comparison.
The full controlled-link capacity/resource matrix, diagnostics overhead,
offloads and ARM64 qualification remain later work.

## Review correction comparison — 2026-10-11

Before changing production code for the nine review findings, the
review reference
recorded the immediately pre-change binary
`2b4b88a15ef978bb7c4a8722b476f43bdb3aa428bd1c2c3db3529e9c5c84d3d1`.
The frozen review policy
binds this reference's manifest/summary hashes and inherits every numeric
workload bound from the prior comparison policy **without changes**. Its
metadata records that the tolerances come from the earlier prospective policy;
the fresh reference identifies this increment, rather than re-estimating noise
or loosening bounds. It was frozen before candidate implementation/measurement.

After native/container tests and independent interoperation finished, the
corrected candidate
ran the same declared short profile serially, without overlapping builds/tests.
Binary SHA-256:
`767d143ca6e32f62f28da0bee3d6cc8446e8ace4033c6a915e699f945bd86a99`.
The final comparison failed:
9 of 12 runs passed; both simultaneous tunnel runs exceeded client/server
process and owner CPU bounds, and the second simultaneous baseline exceeded
both owner CPU bounds. All 12 measurements retained valid low-rate coverage.
Each active direction delivered 3000/3000 packets, 1.024 Mbit/s, with zero
measured loss, integrity or generator errors. The complete artifacts identify
source/lock/binary hashes, counters, resource samples and topology.

| Simultaneous CPU metric | Final range, cores | Frozen maximum, cores |
|---|---|---|
| Tunnel client process / owner | 0.296–0.313 / 0.292–0.306 | 0.275 / 0.268 |
| Tunnel server process / owner | 0.289–0.303 / 0.282–0.292 | 0.254 / 0.247 |
| Baseline pair 1 client / server owner | 0.138514 / 0.138514 | 0.137526 / 0.137627 |

The manifest profile, platform, inherited affinity, locks and comparator
accounting match; the same-clock drift checks and integrity/generator/delivery
checks pass. Inspecting retained `/proc` sample deltas confirms the reported
CPU arithmetic. These short samples do not record per-role actual CPU placement
or frequency history, and the earlier candidate passed the same bounds. This
does not establish whether the excess is a code regression or environmental
variability.

On 2026-10-11 the project owner authorized a minimal development exception:
the [MVP development policy](../benchmarks/policies/mvp-development.json)
sets a provisional **0.5-core average ceiling per role** for both process and
owner CPU in all six workloads. This is a broad smoke limit for the short,
low-rate profile, not a calibrated regression tolerance. Delivery, loss, RTT,
RSS, evidence validity and all release criteria are unchanged. It replaces the
strict CPU policy for ongoing MVP development; additional profiling or load
runs are not prerequisites for proceeding.

Applying it to the already recorded final measurements gives
12/12 passing runs.
This separate assessment explicitly records a **retrospective development
exception**, not a prospective no-regression claim. No fresh traffic ran.
The original frozen policy and its 9/12 failed comparison remain intact.
This closes the CPU development gate; full release qualification remains in
its existing tasks.

| Corrected workload | Baseline p95 RTT, ms (range) | Tunnel p95 RTT, ms (range) |
|---|---|---|
| Forward | 0.726–0.833 | 0.738–0.786 |
| Reverse | 0.784–0.795 | 0.852–0.936 |
| Simultaneous | 0.765–0.770 | 0.615–0.809 |

For the next increment, record the pre-change reference and bind the versioned
bounds before implementing or measuring the candidate. Use fresh output paths;
binding copies the limits unchanged and records local reference hashes:

```bash
./scripts/validate/bench.sh --profile benchmarks/mvp-x86_64.toml \
  --output benchmarks/results/mvp-reference
python3 scripts/validate/freeze-policy.py \
  --reference benchmarks/results/mvp-reference \
  --policy benchmarks/policies/mvp-development.json \
  --output benchmarks/results/mvp-reference/comparison-policy.json
```

After implementation and checks/builds, measure and compare the candidate:

```bash
./scripts/validate/bench.sh --profile benchmarks/mvp-x86_64.toml \
  --output benchmarks/results/mvp-candidate
python3 scripts/validate/compare.py \
  --reference benchmarks/results/mvp-reference \
  --candidate benchmarks/results/mvp-candidate \
  --policy benchmarks/results/mvp-reference/comparison-policy.json \
  --output benchmarks/results/mvp-candidate/comparison.json
```

This is the existing one-session, 128-byte, 1000-pps loopback profile with
two-second warmup, three-second measurement, 200-ms drain and two paired
repetitions. Future runs use the declared provisional development bounds;
they establish neither a speedup nor sustainable capacity/full SC acceptance.
The older evidence below remains tied to its recorded revisions. See the
[remediation record](validation/review-remediation.md) for correctness and
remaining qualification.

The first review-fix comparison
is also preserved: its 12 runs passed before the final Host/authority validation
ordering correction. Its manifest identifies that intermediate binary; the
original 9/12 comparison identifies the subsequent binary and uses that same
prospective policy. Neither reference nor bounds changed between those candidate
measurements; the separate development policy was adopted afterwards.

## First x86_64 MVP measurement — 2026-10-11

The serial reference
records revision `503a83c374ac8b885f4d5e16bb26dff7c26b5a9e` plus dirty-tree source,
lock and executable hashes, native CPU/kernel/affinity, two-role topology,
configuration, offload states and diagnostic settings. The working executable
contains the Phase 3 increment. Build identity alone is not a source attestation.

Reproduce after [MVP checks](validation/mvp.md):

```bash
./scripts/validate/bench.sh --profile benchmarks/mvp-x86_64.toml \
  --output /tmp/remora-mvp-measurement
```

The output must be fresh. Do not run builds/tests or other avoidable competing
work during a comparison. Roles and endpoints run on one Linux x86_64 host;
client application → client relay → server relay → target is the same two-leg
route for the unencrypted baseline and tunnel. They use the same Linux mmsg
adapter, batch maximum 32, outer/relay kernel buffer requests 262144/65536 bytes,
offloads off and default info diagnostics. Read-back buffer maxima are in each
role's status records. This is loopback without controlled bottleneck or RTT.
The baseline's eight-byte association and idle 100-us polling are explicit
test-tool differences; it is not a direct-endpoint or echo-only capacity baseline.

Coverage: one session, 128-byte payloads, 1000 datagrams/s per active direction,
two-second warmup, three-second measurement, 200-ms drain, two paired repetitions,
alternating baseline/tunnel order. Independent forward and reverse streams are
measured separately and together; RTT probes are sparse and distinct from the
numbered measurement packets. CPU sampling uses actual `/proc` intervals and
reports the owner thread separately. All 12 serial-reference runs are valid for
this declared low-rate coverage, with zero integrity, generator or delivery loss.

| Workload | Baseline delivered Mbit/s forward / reverse | Tunnel delivered Mbit/s forward / reverse | Baseline p95 RTT, ms (range) | Tunnel p95 RTT, ms (range) |
|---|---|---|---|---|
| Forward | 1.024 / inactive | 1.024 / inactive | 0.759–0.770 | 0.768–0.786 |
| Reverse | inactive / 1.024 | inactive / 1.024 | 0.794–0.820 | 0.683–0.868 |
| Simultaneous | 1.024 / 1.024 | 1.024 / 1.024 | 0.763–0.870 | 0.657–0.754 |

Each active direction delivered all 3000 offered unique packets per run.
Tunnel simultaneous average process CPU was 0.233–0.247 cores at the client
and 0.247 cores at the server; peak RSS was 8.06–8.12 MiB and 8.07–8.16 MiB
respectively. Baseline simultaneous CPU was 0.135–0.138 and 0.127–0.132 cores,
with 4.75–4.91 MiB peak RSS per role. Per-run owner CPU/headroom, counts,
RTT samples, status, kernel-buffer readbacks and resource traces are retained in
summary and raw artifacts.
The sparse ~30 RTT samples/run do not support a high-confidence latency claim.

The initial 12-run set is also preserved at the result root. It overlapped
pinned-container verification, so its
qualification note
excludes it as a CPU/RSS regression reference. No run was deleted or relabelled
as a successful full acceptance test.

## Prospective comparisons and next decisions

Before any subsequent critical-path change, the
frozen comparison policy
binds the serial manifest/summary hashes and workload. It uses each workload's
observed min/max spread, explicit units and prospective floors: two datagrams
per interval for delivery/loss, max(2×repeat spread, 0.25 ms) added RTT tolerance,
max(2×spread, two `/proc` CPU ticks over the measured interval) CPU tolerance,
and max(2×spread, 256 KiB) RSS tolerance. Every matched candidate run must pass
its own bound; missing/unmatched/invalid evidence cannot become a pass by
averaging. Two pairs provide provisional variability, not a confidence interval.
The full release comparison and capacity matrix are later tasks.

The HTTP-refusal cleanup correction was followed by the
rejection-fix-serial comparison.
The final increment, including the partial-Capsule RESET correction, passed
all 12 per-run comparisons
against that same frozen policy. No tolerance or reference was changed after
the candidate measurements. Each active direction again delivered 3000/3000
packets (1.024 Mbit/s), with zero integrity/generator errors or measured loss.

| Final workload | Baseline p95 RTT, ms (range) | Tunnel p95 RTT, ms (range) |
|---|---|---|
| Forward | 0.686–0.715 | 0.717–0.765 |
| Reverse | 0.778–0.822 | 0.625–0.784 |
| Simultaneous | 0.761–0.762 | 0.560–0.734 |

Final simultaneous tunnel CPU was 0.196–0.217 cores for the client and
0.203–0.220 for the server, with peak RSS 8.14–8.16 MiB and 8.06–8.16 MiB.
These short-run changes are within the declared bounds; they do not establish
a statistically significant speedup. The
final manifest, samples and summary
identify the exact source/binaries and contain the resource and diagnostic data.

The earlier rejection-fix attempt
is preserved as invalid: its connected client started before the server bound
its UDP socket and received a refusal. The runner now waits for the owned
server's actual socket inode/bind before starting the client. The corrected
and final measurement series ran sequentially after verification builds/tests,
without overlapping those workloads.

These results establish a reproducible working path, not a sustainable capacity
ceiling. At 1000 pps the process CPU remains substantial; syscall/wakeup,
short-batch, scheduling and diagnostic costs need profiling before deciding
whether to change batching, queue layout, pacing or buffer ownership. Owner
headroom is present at this load; no conclusion about 32-session fairness or a
single owner's high-load limit follows. Compare single/mmsg, batch sizes and
congestion controllers under the planned matched workloads before optimization.
GSO/GRO auto remains effective off/not_implemented, without a capability probe.

SC-003/004/005/007 retain their numeric release acceptance criteria. These short
one-session runs do not qualify the 1-Gbit/s/20-ms reference path, sustainable
baseline rate, <=0.1% loss search, five paired five-minute runs, 32 sessions,
500-Mbit/s resource load, 1000-cycle churn, diagnostics overhead, ARM64 or
network impairments. None of those missing results is a pass or a guaranteed
outcome. Native ARM64 does not block initial x86_64 development.

## Phase 5 pre-refinement profiling — 2026-10-12

Ten sequential parameter cases in `benchmarks/experiments.toml` each ran two paired repetitions in forward, reverse and simultaneous directions (120 valid-low-rate runs). Each case offers 5000 packets/s, 128 bytes, one-second setup/probe warmup, two-second measurement and 200ms drain on the two-leg same-host loopback topology. This warmup is not a sustained traffic warmup; full reference tooling below requires 60 seconds of actual load. All observed integrity/generator errors are zero; these runs do not establish sustainable capacity or release criteria.

| Variant | Simultaneous owner CPU, client/server range (cores) | RTT p95 range (ms) |
|---|---|---|
| baseline-mmsg32 | 0.576–0.627 / 0.576–0.597 | 0.444–0.515 |
| bbr2 | 0.528–0.567 / 0.523–0.567 | 0.607–0.673 |
| mmsg1 | 0.430–0.502 / 0.435–0.517 | 0.379–0.539 |
| mmsg16 | 0.481–0.523 / 0.491–0.518 | 0.438–0.510 |
| mmsg64 | 0.521–0.532 / 0.527–0.546 | 0.511–0.560 |
| mmsg8 | 0.370–0.551 / 0.390–0.551 | 0.368–0.552 |
| reno-diagnostic | 0.522–0.588 / 0.527–0.583 | 0.567–0.729 |
| single | 0.421–0.430 / 0.416–0.445 | 0.406–0.487 |
| smaller-pools | 0.540–0.553 / 0.548–0.550 | 0.469–0.775 |
| smaller-queues | 0.537–0.542 / 0.537–0.537 | 0.386–0.572 |

The working path is expensive at this load: 5.12 Mbit/s per direction already uses about 0.6 of its one packet owner. Batch/CC results overlap and short pairs cannot isolate a robust winner. No default switch, sharding, backend change or GSO/GRO acceleration is selected from this evidence. Preserve mmsg32/CUBIC/timerfd and investigate under a separately referenced high-packet-rate workload before optimizing. Smaller pools/queues do not demonstrate a reliable advantage. Plaintext baseline uses the same selected adapter mode/batch/payload pool, fixed 100us polling and no sustainable-capacity claim; its lack of application queue pressure here does not prove matched high-load queue behavior.

Controlled lifetime server counters for the first simultaneous mmsg32 run: `{"application_queue_age_max_us": 577, "ciphertext_age_max_us": 118, "factory": {"datagram_allocations": 9980, "datagram_copy_bytes": 1297400, "encoded_payload_bytes": 1277440, "overflowed": false, "stream_allocations": 13, "stream_copy_bytes": 120}, "io": {"error_queue_syscalls": 90019, "mtu_queries": 9979, "overflowed": false, "receive_syscalls": 90019, "received_datagrams": 28376, "send_syscalls": 28964, "sent_datagrams": 28968}, "owner_visits": 135031, "pacing_lateness_max_us": 105, "payload_slab_allocations": 68, "queue_copy_bytes": 1277440, "received_datagram_copy_bytes": 1297270, "send_burst_datagrams_max": 4}`. They include setup/drain, not only the measurement window. They count actual adapter calls and controlled copy/allocation boundaries, not all crypto/allocator activity. Maxima are lifetime maxima, not p95 histograms. Owner context-switch deltas and actual sampled CPU/headroom are retained in each summary/samples file.

CPU-clock user IP sampling produced 37 server samples in the selected two-second mmsg32 run, zero lost samples. The largest observed symbol is payload Slab drop (10.81%); recv_batch and Tokio driver/wake work also appear. This sparse sample is an investigation lead, not an optimization justification. No raw stack/callgraph/payload capture is used. Kernel syscall tracepoints remain unavailable because tracefs access is denied even outside the sandbox; actual adapter counters provide syscall counts without claiming syscall latency. The independent 1ms-sleep coarse timer baseline retains 400 actual lateness samples; actual timerfd path lateness/bursts are recorded above, without substituting a coarse production pacing backend.

The first profiler wrapper used overlapping stderr filenames; CPU perf data, process samples and final cumulative snapshots remained intact, but those logs cannot establish diagnostic coverage. Fixed future profiler log names are independent. A wrong concurrency CLI option halted the runner after completed experiments; the corrected resume validated and retained all 120 completed measurements, then performed the missing stages. The original invocation failure is not a passed stage.

A separate 60-second, 32-session isolation/control run passes integrity and surviving-flow progress, with zero cross-delivery and zero reported diagnostic coverage loss. Session 0 is deliberately closed/recreated: its 98.25% forward/96.49% reverse one-second ratios are excluded by that scenario’s lifecycle rule and do not meet SC-009 if naively treated as continuously admitted. The other 31 flows have 100% one-second replies. This is a short control/fairness experiment, not 30-minute or saturation qualification.

The prospective reference was collected **before T051**: revision `0301c9239e0bfcf2cf0238da69393dfd3bb60262` plus the recorded dirty-source hashes, executable `b45fe7e5fd68d95f225503be30169834312be3bd98bfb29e1ca3800904abf174`, manifest `b935bfa11571818cf1f0d4646621af828fdcb789084d3318a6effc7b9fc0a81d`, summary `f661d57dd399178f03651514c4846bb141bc7dcdeba7d05f59675e628c3d5a97`. It covers five pairs, 128-byte/1000pps, separate/simultaneous directions, two-second setup warmup and three-second measurement, 200ms drain. All 30 runs are valid. The immutable bound policy is `benchmarks/results/phase5-profile/comparison-policy.json`; the versioned numeric template is `benchmarks/policies/phase5-x86_64.json`.

Every matched candidate run must satisfy the reference extrema plus two observed repeat spreads or predeclared measurement floors: two delivered messages/three seconds, 0.25ms RTT, 0.02 CPU cores and 256KiB RSS. Units, direction/role coverage and interpretation are explicit in the template; no averaging failures. This is a limited noise observation, not a confidence interval or permission to change SC targets. New workloads need independent prospective references. The exact generator/plaintext-relay executable is retained as `benchmarks/results/phase5-profile/reference-traffic` for comparisons after test-harness changes.

Raw artifacts: `benchmarks/results/phase5-profile/`. Native x86_64 evidence only; ARM64 and final-revision complete numerical qualification remain pending T077.

## Phase 5 implementation outcome and reference tooling

No acceleration/default switch is selected. Functional profiling exposed two
correctness failures: synchronous ENOBUFS from owned netem pressure closed a
connection, and a request arriving before delayed client SETTINGS closed the
server connection. Their original failures are preserved. The corrected owner
discards only the unaccepted pressured FIFO front, preserves control recovery
and admitted flows, and defers <=32 headers/16KiB under existing Session/setup
budgets and the original five-second deadline until required capabilities are
verified. Tests cover prefix preservation/no replay, cancellation, admission
limits, no premature DNS/relay and unchanged setup expiry. Unknown/malformed UDP
segment ancillary data is rejected while GRO is unimplemented. Auto reports
effective off/not_implemented with no production capability probe.

Batch 1/8/16/32/64 experiments are requested adapter ceilings; production owner
receive is capped at 32 and per-connection outer submission at four selected
packets. Requested batch 64 does not imply 64-packet outer syscalls. The perf
attach window starts after role readiness and includes setup and the first
measurement portion; its sparse IP samples are not a phase-aligned full-run
profile. Initial plaintext profile snapshots accidentally discarded the return
value of IoStats.merge and therefore count outer I/O only; production Remora
counters were merged correctly. The final test-only baseline fixes that merge.
Do not reinterpret archived partial baseline counters as all-socket syscall
counts. CPU/RSS and end-to-end measurements are independent of this issue.

`benchmarks/reference-x86_64.toml` and the schema-2 runner implement 18
sustainable-rate searches, 185 paired repetitions covering capacity/subcapacity
and resources, 1/32 sessions, 128/512/1200-byte payloads and every directional
case. Mode order uses reproducible SplitMix64 decisions tied to the workload
and seed; pair scheduling is shuffled with the recorded seed. Calibrate in 1%
bottleneck steps, retain rejected/invalid rates, then require each baseline to
confirm sustainability during the full 60s loaded warmup/300s measurement.
Include 50% subcapacity loss/RTT, 500Mbit/s each-direction resource load and
1000-cycle warmed-RSS churn. Native generator sequence tracking uses bounded
bitmaps instead of the old million-entry HashSet limit; warmup has a separate
sequence namespace and is not counted as measurement traffic.

The plaintext two-leg harness uses the same adapter/batch/socket buffers,
source/endpoint mapping, 32-flow limit, 32-packet/65536-byte application queues,
64-packet/262144-byte outer queues and 20/100ms residence bounds. Pending bytes
lease the same 8MiB application/4MiB wire pools with 2MiB receive scratch; no
independent uncharged retained packet Vec queue. It sleeps only when idle;
accepted prefixes and expired leases are native-tested. It is test-only and
adds no second production package or executable.

Each row is checked for generator submission/scheduling/headroom, corruption,
clock, samples, collector gaps and source/executable identity before thresholds.
Every applicable direction/pair independently enforces unchanged SC-003/004/005
and SC-007 limits. Strict Rust tests and the separate frozen-policy Python
comparator reject malformed/missing/duplicate workloads, mismatched units,
boolean schema versions, missing references, bad seeds and invalid evidence.
The runner records actual core placement, hardware/kernel/compiler/build/locks,
qdisc/netdev features, requested/effective buffers and kernel UDP queue/drop
charges separately from RSS; queue charges exclude total socket struct overhead.

The initial development matrix executed all 18 short calibration cases and
37 low-rate pairs, then caught a producer/validator owner-CPU schema mismatch.
That original failure remains preserved. The corrected selected native check
executes one-session forward and 32-session simultaneous capacity/subcapacity
plus a resource-shaped smoke, validates its actual schema, and returns
**invalid for release** for deliberately short intervals/incomplete searches,
pairs/churn. No smoke is relabelled full numerical qualification. Complete
full-duration/final-revision execution remains T077.

## Phase 5 T055 comparison and development exception

The original pre-T051 reference and strict policy remain immutable. The first
candidate passes 24/30 strict comparisons; the final trust/0-corrected executable
`bd9f20254da298ac43e7a46c8fdfccf58a4d59837b0f2d3fa3175f47694120f5` (before independent-review corrections) passes 25/30. All runs retain valid low-rate traffic/clock/integrity
coverage, and every delivery/loss/RTT/RSS bound passes. Failures are exclusively
CPU: final baseline simultaneous server owner/process in four pairs and one
reverse tunnel pair's owner CPU. The unchanged retained baseline executable
also exceeds its own reference bounds; this is evidence of reference/environment
variability, not proof that a Remora code change caused all increases. No bound
is changed and no failure averaged away. A strict no-regression pass is **not**
asserted.

The existing owner-authorized ADR-0007 development exception applies to this
same one-session 128-byte/1000pps loopback profile: process/owner <=0.5 cores per
role. The separate `benchmarks/policies/phase5-development.json` assessment
applies that previously authorized ceiling, retaining **every other original
Phase 5 bound**, and passes 30/30 saved final runs. It is explicitly retrospective
development acceptance, not a prospective regression/noise proof, and changes
no SC release criterion. This permits the checked incremental Phase 5 method/
correctness work under the existing exception without selecting a speculative
speed change. Retain the strict failures and collect a stable, separately
referenced high-rate profile before any subsequent optimization.

Artifacts: `benchmarks/results/refinement-x86_64/` and its `final-binary/`.
Next release work: native complete five-pair reference matrix and 500Mbit/s/
direction resources/churn on the final candidate (T077), sustained high-rate
generator headroom, packet-owner capacity, complete diagnostic qualification
and each claimed architecture. Current data does not establish SC-003/004/005/
007 or ARM64 support.

## Phase 5 independent-review corrections and comparison

The owner-requested review found inconsistent delayed-ICMP send handling and
three complete-method defects: one-hour namespace holders, offered-only SC-007
acceptance and sampling exited workers. Corrected behavior and focused/native
evidence are recorded in [phase5.md](validation/phase5.md). No throughput
optimization or offload was selected. Required schema-2 per-session delivery
counts are emitted by the producer and checked against aggregate counts.
SC-007 requires actual >=500Mbit/s payload delivery in both directions on the
matched sustainable baseline and tunnel, with all 32 Sessions carrying traffic.
The producer provides <=0.1%-loss offered-load headroom without relaxing that
delivered threshold. Namespace holders live until explicit teardown; expected
post-measurement worker exit is recorded without fabricated zero samples.

After correctness/static checks and independent Go interoperability, the
corrected executable
`c90e055f2b2d995c5a35a58cb700887b2fbfb53f9ce4ab6aca4d02865ba02611`
was measured with the original retained plaintext/generator fixture, matching
one-session/128-byte/1000pps settings, five pairs and all three directions. The
first series overlapped a short validator test and is retained but excluded
from performance evidence (`phase5-review-fixes-regression/evidence-note.json`).
The separate clean series runs without competing builds/tests and retains all
30 valid rows in `benchmarks/results/phase5-review-fixes-regression-clean/`.

The unchanged pre-T051 policy yields **26/30 strict checks passed; overall FAIL**.
All delivery/loss/RTT/RSS bounds pass. Failed owner-CPU checks are:

| Run | Observed owner cores | Frozen maximum |
| --- | --- | --- |
| Baseline simultaneous server, pair 0 | 0.114943 | 0.111797 |
| Baseline simultaneous server, pair 1 | 0.123330 | 0.111797 |
| Baseline simultaneous server, pair 4 | 0.116558 | 0.111797 |
| Tunnel reverse client, pair 2 | 0.138795 | 0.129372 |
| Tunnel reverse server, pair 2 | 0.199729 | 0.194058 |

The last two metrics belong to one failed run. No original bound, reference or
failure is changed; this is not strict no-regression proof or evidence of the
cause of reference/environment variability. A separately labelled saved-data
assessment using the existing ADR-0007/Phase-5 development policy passes 30/30
at the already authorized 0.5-core ceilings with every non-CPU bound unchanged.
It remains retrospective development acceptance, not SC/release qualification.
Full native-duration/resource/churn/platform acceptance remains T077.
