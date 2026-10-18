# Performance evidence

## Current selected native x86_64 profile — 2026-10-17

The selected CUBIC executable passes the bounded reference criteria for the
recorded native Linux GNU x86_64 profile. T076 functional qualification and T078
requirement/review reconciliation also pass within their declared scope. See
[the qualification report](validation/release.md) and
[post-review corrections](validation/phase8-implementation-review-fixes.md).
This is local evidence for an exact build/workload, not a published release,
WAN guarantee or native ARM64 qualification. Linux GNU x86_64 is the only
currently required platform; [ARM64 is deferred](adr/0024-defer-arm64-release-scope.md)
outside the completion criteria, not an unfinished qualification gate.

Measured qlog-build executable SHA-256:
`7f7a3c4de7341b8edf0860ff3c8457642ff423cf4658f9aa93076db678cfe3b1`.
The original traffic executable SHA-256 is
`d2693daff0709b6c05c78a1476a35aaa0633139220f82a763e96d180c35ec36b`.
The subsequent validator-only correction revalidates the saved suite as passed;
its new tool build is not relabelled as the original measurement executable.

**T082 working-source follow-up — 2026-10-17:** The current source corrects an
extra H3 continuation that kept the owner runnable behind full ciphertext
output. Its qlog executable is
`a4b10cf925fea1739595c6b30d2d46a4535ebf9dce0d5398431cca9c77da557f`.
[The affected-check/focused-comparison record](validation/phase10-t082.md)
passes 36 members/12 per-repeat checks against the preceding Phase 9 build
under a newly frozen policy, with resource delivery 500.73152–500.736 Mbit/s
per direction. These results are separate from the selected full qualification
below and do not transfer that qualification to the changed executable.

Raw artifacts remain local under
`benchmarks/results/release/phase8-x86-cubic-20261016/reference/benchmark/`;
the input-bound coordinator is
`benchmarks/results/phase8-cubic-reference-20261016/`. A fresh clone contains
tracked reports/profiles, not those ignored raw bundles. See
[artifact retention](validation/README.md).

| Metric, worst applicable selected run | Measured result | Acceptance boundary |
| --- | --- | --- |
| Delivered/direct payload ratio, 1200-byte capacity cases | **99.815985% minimum** | >=80%, separately for each direction/pair |
| Added subcapacity loss | **0.038594 percentage points maximum** | <=0.1 pp per active direction/pair |
| Added end-to-end p95 RTT | **0.768 ms maximum** | <=5 ms |
| 32-session resource delivery, each of three repeats | **500.736 Mbit/s in each direction**, zero measured endpoint loss | >=500 Mbit/s/direction |
| Resource role CPU | **0.910608 core maximum** | <=2 average cores per role |
| Resource role RSS | **12771328 bytes (12.18 MiB) maximum** | <=128 MiB |
| 1000-cycle retained RSS growth / reclamation | **<=228 KiB / 0.119 s** | <=10 MiB within <=60 s |

These are paired delivery ratios at a tested sustainable offered rate, not
99.8% of universal physical link capacity. Calibration chooses a tested point;
it does not interpolate an exact maximum. Maximum absolute tunnel loss was
0.215847% in capacity and 0.128597% in subcapacity cases. Capacity acceptance
uses delivered/direct ratio; SC-004 uses **added** loss relative to an
independently valid matching baseline. Absolute tunnel loss is not hidden by
those definitions, and no failed pair/direction is averaged away.

## Workload and measurement method

The [current profile](../benchmarks/reference-x86_64.toml) and
[benchmark contract](../specs/001-masque-udp-tunnel/contracts/benchmarks.md)
define schema 4, `bounded-reference-v2`. The run uses an Intel Core i7-13700K,
Linux 7.2.9-arch1-1, owned network namespaces/veth and a two-leg route shaped to
1000 Mbit/s per direction with 20 ms reference RTT. Client CPUs are 0–3, server
4–7 and tools/collector 8–23. These are disjoint logical CPU masks; the QUIC/H3
owner itself remains sequential. No competing build/test was scheduled during
the numerical run.

- **18 baseline searches:** 1/32 sessions × 128/512/1200-byte payloads × forward,
  reverse and simultaneous traffic. Start at 125 Mbit/s, double to a rejected
  point or 1000 Mbit/s, then bisect; at most six points, with early stopping at
  a successful 1000. Every point has 2 s loaded warmup, 8 s measurement and
  500 ms drain. Invalid generator/integrity evidence invalidates calibration.
- **75 baseline/tunnel pairs, three repeats:** 54 subcapacity pairs at half the
  selected rate, 18 capacity pairs at 1200 bytes, and three resource pairs.
  All pairs use 3 s loaded warmup and 500 ms drain. Ordinary measurement is
  15 s; the three 32-session simultaneous capacity pairs and all resource
  pairs use 60 s per member. Pair order/seed and raw directional counts remain
  recorded; each paired baseline independently confirms <=0.1% loss.
- **Validity:** matching plaintext relay on the same two-leg path, bounded
  sequence tracking, integrity/duplicate/scheduling checks, generator CPU below
  0.9 core, >=100 RTT samples per session, >=10 resource samples/s, owner CPU,
  kernel queue charges and process RSS recorded separately.
- **Arrival waveform:** deadlines grouped at 100 microseconds and exact counts
  generated over the declared interval minus a 20 ms completion reserve.
  Collection/resources cover the full interval. Both pair members match; the
  reserve slightly increases active intensity and does not permit missing or
  late catch-up slots.
- **Duration:** nominal intervals total 82m45s. The runner enforces a 5400 s
  (90-minute) wall budget with a 30 s cleanup reserve. The selected complete
  run took **84m42s**, including 1000-cycle churn. This replaces the historical
  schema-2 multi-hour method; the old failures/results remain unchanged.

CUBIC, mmsg/batch ceiling 32, GSO/GRO off and info diagnostics with runtime
qlog inactive are the measured settings. Current buffer requests are 1 MiB
outer and 128 KiB application/relay for each receive/send buffer; effective
Linux reference values were 2 MiB and 256 KiB respectively. All 300 role records
agree. The original manifest retained stale requested values; the
[transparent metadata erratum](validation/phase8-benchmark-metadata-erratum.md)
preserves it and supplies the correction. The current runner derives requests
from the bound Rust defaults; kernel memory is additional to process RSS.

T076 is a separate functional job: its **80m54s** selected execution includes
30-minute generic/isolation and independently prepared VM WireGuard duration,
10-minute overload, network/recovery/shutdown, security/operator/interop checks
and prerequisites/cleanup. It is not part of the 90-minute T077 numerical
budget. The VM workload is low-rate duration evidence, not WireGuard capacity.

## Reproduce the current numerical profile

Build the pinned root and traffic packages as described in [build](build.md).
Use a native x86_64 host with at least ten available logical CPUs, disjoint
four-CPU role masks plus generator/endpoint cores, working Linux user/network
namespaces, `unshare`, `nsenter`, `taskset`, `ip`, `tc` and read-only `ethtool`
queries. Local policy must permit the owned namespace setup. No host network
or sysctl mutation is part of the runner. Record power/governor/affinity and
kernel feature inventory rather than silently changing the machine.

Inspect the plan first; plan-only does not collect acceptance evidence:

```bash
cargo build --locked --release --features qlog
cargo build --locked --release --manifest-path tools/traffic/Cargo.toml
./scripts/validate/bench.sh --profile benchmarks/reference-x86_64.toml \
  --plan-only --output benchmarks/results/reference-plan-NEW
```

For a short plumbing check, use a separate fresh path. Smoke is intentionally
**invalid for full release criteria** because its matrix/intervals are incomplete:

```bash
./scripts/validate/bench.sh --profile benchmarks/reference-x86_64.toml \
  --development-smoke --smoke-selected \
  --output benchmarks/results/reference-smoke-NEW
```

The actual complete numerical job is:

```bash
./scripts/validate/bench.sh --profile benchmarks/reference-x86_64.toml \
  --output benchmarks/results/reference-full-NEW
```

Run sequentially, without competing measurements, builds or tests. Keep output
paths fresh and retain failed/invalid/incomplete artifacts. The driver must
have access to the whole intended role/tool CPU set; on the recorded host this
was CPUs 0–23, with collector work confined to 8–23. Do not pin the whole driver
to the collector mask and thereby exclude role CPUs. Preserve inputs while it
runs. Inspect `suite.json`, `criteria.json`, pair/search/churn artifacts and
process exit status; a generated manifest or finished process is not itself a
pass. Exact selected reproduction also requires the original binaries/profiles;
a new build produces evidence for its own recorded hash.

Full functional qualification has its own peer/VM/prerequisite inputs and
[runner contract](validation/release.md#runner-and-continuation-contract).
Planning it is read-only:

```bash
./scripts/validate/release.sh --plan-only \
  --peer-binary /absolute/pinned/remora-interop \
  --output /absolute/fresh-functional-output
```

For execution, supply the prepared private WireGuard fixture and input-bound
T073/T074/T075 prerequisite record described there. `--scope checks` is a useful
subset, but its `checks-passed` result is not full T076, T077 or release acceptance.
Do not reuse a historical prerequisite JSON with different source/binary inputs.

## Known trade-offs and unqualified conditions

CUBIC remains the default and the measured small CPU increase is accepted.
The downstream-first output correction addresses observed eligible ciphertext
waiting behind fresh producers; it is not a reason to bypass congestion control
or enlarge every queue. BBR2 investigation is paused for this increment, not
rejected as a useful direction for lossy paths.

On the selected CUBIC executable, dropping exactly five reverse outer packets
produced **1.956192–2.514200%** reverse application loss in three controlled runs,
with zero measured endpoint kernel-overflow deltas. The clean reference pass
does not qualify that loss amplification away. Earlier BBR2 observations are
not a same-build proof of general superiority. See the
[comparison](validation/phase8-cc-comparison.md),
[loss diagnosis](validation/phase8-final-loss-diagnosis.md) and
[future experiments](validation/phase8-decision-review-followup.md).

Other limits remain explicit:

- Three repeats and 15/60-second windows provide limited variance/tail evidence;
  sparse p95 RTT is not a long-run p99 or maximum-delay guarantee.
- Peak measured owner CPU across workloads is about 0.938 core. A two-core role
  budget does not make the sequential owner two cores fast. Buffer/service-pause
  headroom and heterogeneous-client density coupling remain future experiments.
- Kernel buffering can delay UDP before the application's 20 ms residence clock;
  unchanged userspace queue limits do not bound full end-to-end queueing delay.
- Active high-rate filtered-qlog cost, enabled GSO/GRO, other CC choices, WAN
  paths, high-rate WireGuard and native ARM64 are not qualified by this matrix.
  Earlier T071 diagnostics measurements below are limited to their own low-rate
  build/profile; filtered tracing correctness/privacy has separate native checks.
- Overload intentionally lost 13 diagnostic records and exposed incomplete
  correlation. VM warmup had 73 send errors; measured VM traffic was 72000/72000.
  One post-review concurrent churn test failed with unexpected payload; sequential
  e2e and final full native repeats passed. Its cause remains unestablished in
  the [correction record](validation/phase8-implementation-review-fixes.md).

## Historical development records

The records below preserve previous binaries, policies, methods and failures.
Their statements about pending tasks, older defaults, four-packet quotas,
schema-2 37-hour runs and saved-path commands describe those historical
checkpoints. They are not the current reproduction instructions or evidence for
a different executable. Current commands and accepted scope are above.


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

## Phase 6 T063 prospective loaded-path comparison — 2026-10-12

Recovery adds paced idle probes, authenticated-peer-progress checks, fresh
client generations/sockets and GOAWAY draining placement. Before production
edits, collected five paired repetitions in every direction with
`benchmarks/recovery-x86_64.toml`: one Session, 128-byte payload, 1000pps,
two-second warmup, three-second measurement, mmsg/32, CUBIC, offloads off.
The initial reverse collection was invalid due to a missing reverse-rate
argument in the runner's bounded collector; that failed series is retained.
Corrected the runner and recollected all 30 valid pre-change rows before edits.

Reference executable:
`43f0f30b1ce3f467df4bda5da69c6ebd6ba44967ca13a635583b5a44425aaec7`.
Pre-review recovery candidate:
`fe36ae3958b53bbd875ad052fb15e7f9c59b35cecd0b20f6775257a6b12118ef`.
Both series use the same retained traffic/baseline executable:
`89ecbf2c4f626ab0bfe3b424dd1c5ce5e552858411833b0832dff851735bd0cb`.
The candidate series ran after all builds/tests/native matrices finished.

The unbound tracked template is `benchmarks/policies/phase6-x86_64.json`.
It was frozen before production changes from reference extrema plus two
observed five-repeat spreads or declared CPU/RTT/RSS/delivery floors; it
compares every run without averaging failures. Template SHA-256:
`31319e104214ba20f08f158797ab9050c1b24a3022f89c0c0ecb207bd6e74038`.
The reference-bound local `phase6-reference/comparison-policy.json` SHA-256 is
`d0a7e2cf634fd88175d6f8aac4de989f4e8110c4ec1bbddd485a705fa3c0f65d`.
Reference manifest/summary hashes remain bound; no bound was adjusted after
measurement. Unlike earlier increments, no separate development exception
was needed for this comparison.

**Result: 30/30 strict workload checks passed; overall PASS.** All candidate
rows have valid clock/byte-integrity/delivery coverage, zero loss, corruption
and measurement errors, and 1.024Mbit/s delivery per active direction. CPU,
owner CPU, RSS, RTT and delivery/loss all satisfy their original workload
bounds. Tunnel ranges below are five separate runs, not confidence intervals.
CPU cells show process / owner cores.

| Tunnel workload/role | Reference CPU ranges | Candidate CPU ranges | Candidate maximum RSS MiB | Candidate maximum RTT p95 ms |
| --- | --- | --- | ---: | ---: |
| forward client | 0.129–0.160 / 0.129–0.153 | 0.179–0.199 / 0.175–0.193 | 8.34 | 0.919 |
| forward server | 0.092–0.112 / 0.092–0.109 | 0.127–0.141 / 0.120–0.138 | 8.27 | 0.919 |
| reverse client | 0.088–0.129 / 0.085–0.122 | 0.117–0.141 / 0.117–0.135 | 8.35 | 1.089 |
| reverse server | 0.126–0.183 / 0.122–0.176 | 0.182–0.210 / 0.172–0.203 | 8.28 | 1.089 |
| simultaneous client | 0.156–0.234 / 0.153–0.228 | 0.223–0.313 / 0.220–0.310 | 8.25 | 0.874 |
| simultaneous server | 0.166–0.234 / 0.156–0.234 | 0.213–0.317 / 0.210–0.310 | 8.30 | 0.874 |

Artifacts: `benchmarks/results/phase6-prechange-valid/`,
`phase6-reference/`, `phase6-candidate/` (including strict `comparison.json`).
Invalid original collector data remains `phase6-prechange/`. Commands and
functional recovery/shutdown evidence are in
[the recovery report](validation/recovery.md) and
[quickstart](../specs/001-masque-udp-tunnel/quickstart.md#phase-6-recovery-and-signal-validation).

This is a short, low-rate loopback comparison with enabled liveness code under
application load. It does not estimate actual idle PING CPU cost: the separate
native idle scenario proves four real probes over nine seconds without
replacement. Native recovery timing is separate again. No conclusion about
sustained capacity, high-rate regression, WAN/VPS, SC-003/004/005/007, final
release resource/duration acceptance or ARM64 follows; T076/T077 remain pending.

## Phase 6 independent-review correction comparison — 2026-10-13

The P1 fix cancels all not-yet-accepted client Sessions on permanent mapping
rejection and guards queued CONNECT and delayed-success activation. Mapping
refusal remains correlated before cleanup. The added owner regression checks
401/403/501, accepted-flow delivery, pending cleanup and both independent guards.
The complete correctness/native/interop/shutdown checks ran before measurement.
Details are in [recovery.md](validation/recovery.md#independent-review-correction--2026-10-13).

Corrected release SHA-256:
`dee40610068f3f20ccdb8799230bfef8a74022d5c6e02fb5ab09e97f7ca49602`.
The clean new candidate uses the identical pre-Phase-6 reference, retained
traffic executable, profile and bound policy described above. No original
policy/reference was modified or rebound; all previous results are retained
with their own executable scope.

**Corrected candidate: 30/30 strict checks passed; overall PASS.** All rows
are valid low-rate measurements with zero loss/integrity/measurement errors;
each active direction delivered 3000/3000 messages at 1.024Mbit/s. CPU, owner
CPU, RSS, RTT and delivery/loss satisfy unchanged frozen workload bounds.
Five-run tunnel ranges (process / owner cores) are:

| Corrected tunnel workload/role | CPU ranges | Maximum RSS MiB | Maximum RTT p95 ms |
| --- | --- | ---: | ---: |
| forward client | 0.162–0.183 / 0.155–0.178 | 8.32 | 0.890 |
| forward server | 0.114–0.130 / 0.107–0.125 | 8.32 | 0.890 |
| reverse client | 0.092–0.122 / 0.088–0.119 | 8.35 | 0.887 |
| reverse server | 0.139–0.180 / 0.133–0.176 | 8.27 | 0.887 |
| simultaneous client | 0.210–0.278 / 0.210–0.271 | 8.33 | 0.878 |
| simultaneous server | 0.217–0.275 / 0.210–0.271 | 8.29 | 0.878 |

Artifacts: `benchmarks/results/phase6-review-fixes-candidate/` including
`comparison.json`; source/reference/policy hashes are retained in the manifest.
This is the same short loaded-path scope, with functional idle-probe timing
separate. It does not extend the evidence to high-rate/idle CPU cost, WAN/VPS,
ARM64 or final release duration/resource requirements.


## Phase 7 T071 diagnostics measurement method — 2026-10-13

**Historical checkpoint: T071 regression acceptance was pending.** The saved same-binary runs
measure repeatability of the already instrumented Phase 7 implementation;
they do not compare the instrumentation against a pre-Phase-7 implementation.
The policy was frozen before candidate measurements, after the original
critical-path edits, so it does not satisfy the original prospective requirement.
A new matching implementation comparison with limits fixed before measurement
is required; the missed historical freeze remains an explicit acceptance gap.
No requirement exception has been approved.

The original measurement-only invocation added test-only harness support;
its production Rust sources were unchanged from the T064–T070 candidate. Before harness edits, copied the
qlog-feature release and traffic executables into
`benchmarks/results/t071-frozen-inputs/`, recording source and lock hashes.
The immutable Remora SHA-256 is
`c30a4983d18def84f643eaafaa98079208a59fb12db4bc76e5911bee84a4742c`;
traffic SHA-256 is
`e973d280db788bd948670d6e9d83e2db28e1f495f19cd87029bdaf99b621adad`.
This is the **pre-T071 Phase 7 reference**, not a retrospective pre-T064
instrumentation regression claim. No production critical-path change occurred
in that invocation. Subsequent review corrections change production code;
these saved results do not qualify the corrected executable. Earlier T064–T070
reports retain their historical scope.

`benchmarks/diagnostics.toml` declares one Session, 128-byte UDP payload,
100pps/active direction, forward/reverse/simultaneous traffic, two-second setup
warmup, five-second measurement, 200ms drain and five repetitions per workload.
Four diagnostic modes yield 60 normal runs; default/qlog blocked collectors
add ten simultaneous robustness runs. Reference and candidate mode orders
are reversed. All child roles inherit the same four allowed CPU-affinity cores.
Each run retains native kernel/CPU/affinity/locks/offload/socket-buffer/clock
metadata, unique delivery/loss/integrity, RTT samples, process/owner CPU,
headroom, RSS samples, JSON status/events, qlog sizes, exit and stop observations.
Generator saturation, corruption, bad clock evidence or incomplete matrices
invalidate the comparison. Runs are never averaged to hide a failure.

| Mode | Actual settings / boundary |
| --- | --- |
| off | `level=error`; ordinary detail suppressed, mandatory counters/status/ring remain active |
| default | `level=info`, 1000ms snapshots, no qlog |
| verbose | `level=trace`, same fixed schema/rate bucket and snapshot cadence; arbitrary upstream targets remain excluded |
| filtered-qlog | `level=info`, 1000ms snapshots and fresh private tracing directory |

This safe off mode does not remove observability obligations or represent a
build with all instrumentation removed. 100pps keeps qlog active below its
shared 1000-event/s ceiling. A normal qlog run with a stopped/failed filter is
invalid; it cannot be reported as the cost of active tracing. Trace file/scratch
caps are checked separately. Trace output is reconstructed metadata, never raw
H3/header/payload/keylog data.

After collecting the complete reference, froze
`benchmarks/policies/phase7-diagnostics.json`, SHA-256
`146a29bdc8eefc5861147de405d351a9843b424bb1df539538951b9a73d8d99e`,
before any candidate measurement. Every mode/direction/blocked state has its
own explicit-unit bounds from reference extrema plus twice the five-repeat
spread or declared measurement floors: two CPU clock ticks over the observed
five-second interval (denominator 4.9 seconds), 256KiB RSS, 0.25ms RTT and two
messages for delivery/loss. Tolerances describe reference variability and the
observed mode trade-off, not universal percentages or confidence intervals.
This versioned policy is bound to exact reference manifest/summary hashes,
unchanged executable hashes, profile, affinity, production sources and child
platform/lock/clock metadata. Missing or reinterpreted units/limits are rejected;
a candidate predating policy freeze or a policy modified during collection is
invalid. Original reference/bounds are not relaxed after seeing a candidate.

Blocked stdout uses an unread 4096-byte pipe. Kernel `pipe_write` observations
must prove the diagnostic writer actually blocked. Forwarding is measured by
the independent traffic tool, not an absent status ACK. SIGTERM collection
must finish within five seconds with the expected nonzero/incomplete result.
Partial/absent status is explicitly incomplete collection; qlog may exhaust
its ordinary reservation and stop. Such robustness measurements **do not pass
SC-010 complete correlation**. Normal measurements require a functioning
collector, zero coverage/write/overflow gaps and live filtered qlog; the
separate T065/T070 fault matrix establishes observed failure correlation.

```bash
./scripts/validate/bench.sh --profile benchmarks/diagnostics.toml \
  --diagnostics-stage reference \
  --remora-binary benchmarks/results/t071-frozen-inputs/remora \
  --traffic-binary benchmarks/results/t071-frozen-inputs/traffic \
  --output benchmarks/results/t071-reference
./scripts/validate/bench.sh --profile benchmarks/diagnostics.toml \
  --diagnostics-stage freeze --reference benchmarks/results/t071-reference \
  --output benchmarks/results/t071-comparison-policy.json
./scripts/validate/bench.sh --profile benchmarks/diagnostics.toml \
  --diagnostics-stage candidate --reference benchmarks/results/t071-reference \
  --policy benchmarks/policies/phase7-diagnostics.json \
  --remora-binary benchmarks/results/t071-frozen-inputs/remora \
  --traffic-binary benchmarks/results/t071-frozen-inputs/traffic \
  --output benchmarks/results/t071-candidate
```

The tracked policy is the unchanged versioned copy of the frozen local policy.
Outputs must be fresh; failed/invalid artifacts remain retained. New production
binaries/workloads need newly justified reference/policy coverage. This short,
low-rate native loopback matrix does not establish high-rate tracing cost,
sustainable capacity, WAN behavior, release SC-003/004/005/007 or native ARM64
support. Mandatory status costs remain present in every mode.


### T071 measured results

**70/70 candidate workload checks PASS** against the original frozen policy.
Reference also contains 70 valid runs. Production source/executable hashes,
profile, affinity, platform/locks/clock metadata match; policy was frozen before
candidate start and its SHA-256 remains unchanged. All active directions
received 500/500 measured data messages per run: 0.1024Mbit/s per active
direction and zero delivered loss, corruption or generator errors. Five short
runs describe spread, not a statistical confidence interval.

Simultaneous candidate ranges below show five separate runs. CPU is process /
packet-owner cores; peak RSS is MiB. Separate forward/reverse results remain
in the immutable summary; their largest RTT p95 was 0.929ms (qlog reverse).

| Mode / role | CPU process / owner range | Peak RSS MiB | Mode maximum RTT p95 ms |
| --- | --- | ---: | ---: |
| off / client | 0.026–0.032 / 0.022–0.026 | 8.88 | 0.622 |
| off / server | 0.024–0.028 / 0.020–0.026 | 8.79 | 0.622 |
| default / client | 0.024–0.028 / 0.018–0.026 | 8.84 | 0.531 |
| default / server | 0.020–0.028 / 0.016–0.024 | 8.73 | 0.531 |
| verbose / client | 0.024–0.028 / 0.020–0.024 | 8.82 | 0.627 |
| verbose / server | 0.022–0.032 / 0.020–0.026 | 8.80 | 0.627 |
| filtered-qlog / client | 0.032–0.045 / 0.024–0.037 | 8.89 | 0.817 |
| filtered-qlog / server | 0.030–0.045 / 0.024–0.039 | 8.87 | 0.817 |

The descriptive mean process-CPU differences from matched candidate off runs
across all directions were default −0.0014/−0.0007 cores (client/server),
verbose −0.0007/+0.0008 and filtered-qlog +0.0105/+0.0117. Normal verbosity
ranges overlap; these tiny negative observations do not imply an optimization.
Qlog's measured additional processing is visible at this offered load; it is
not an extrapolation to high-rate throughput. Acceptance uses each run's bounds,
not these means. Packet-owner headroom is retained per sample/run and has no
capacity interpretation beyond the measured offered workload.

All 60 normal candidate runs have zero ordinary diagnostic drops, zero status
write failures, zero coverage loss and no counter overflow. Every normal qlog
filter stayed active, with zero schema/rate/queue/output stops. Per-role retained
qlog files were at most 677518 bytes. The largest sampled scratch/active-filter gauges were 65536 bytes / one filter
per role; final scratch is zero after connection cleanup. These are sampled
gauges. The unchanged filter's
64KiB/connection and 512KiB/global reservations remain qualified by T067;
live file sizes and process RSS are measured here. Queue/slab/histogram/pacing
and failure totals remain in each run's final status, with their documented
sampling and ownership meanings.

All ten candidate blocked-sink runs exchanged their full simultaneous traffic
with zero delivered loss. Both writers were observed in kernel pipe-write
blocking; both roles returned the expected exit 1 for incomplete diagnostic
collection. Stop intervals were **1.006–1.009 seconds**, inside the shared
five-second limit. Maximum observed role RSS was **9.05MiB**. Forwarding and
owner scheduling stayed responsive at the specified load. Status/event loss
after the unread prefix is unobservable and collection is explicitly incomplete;
it is not recorded as zero loss or SC-010 success. Qlog can stop on ordinary
queue saturation while the status reservation remains isolated. These are
bounded-sink robustness results, not complete failure-correlation coverage.

Retained artifacts: `benchmarks/results/t071-reference/`, `t071-candidate/`,
`t071-frozen-inputs/` and `t071-comparison-policy.json`. Each mode directory has
its source/platform manifest, per-run summaries/counters, RTT histogram and
process samples, sanitized status/events and optional filtered trace files.
Candidate `comparison.json` checks every run separately. Reference manifest /
summary SHA-256:
`4db328beb0112b6fa0055ce7c26f9cc26535264b455d391b4ce5b17471e664bf` /
`5264836648d58b7ac691e4f22dd72245482e55c8e4d70b07e08d3f32de1210e9`.
Candidate manifest / summary:
`83e275135e974a026901900e19d6d581f0f62a4648fa857e35480b7398990206` /
`a427f4893346b6c2832f0af279cd84ce57b5d9122a48fe1f71811f52054afae2`.
No failing metric was removed, averaged away or reassessed with relaxed bounds.

Validation of the changed Python harness and comparator: **49 Python tests
PASS**, including seven T071 matrix/metadata/unit/collector rejection tests;
shell/Python syntax and diff checks pass. Production source hashes match the
frozen Phase 7 inputs, so the prior exact-binary Rust/native/privacy/interop
verification is preserved; no changed Rust behavior or fresh Rust-suite pass
is claimed by this measurement-only increment. The updated CI runs the new
comparator tests. No sustained fuzz, native ARM64 or release claim follows.


## Phase 7 T071 cross-revision acceptance cycle — 2026-10-13

The original same-binary comparison above remains repeatability evidence. A new
acceptance cycle uses the exact completed Phase 6 revision
`34d4a8fa13aa67436e057773d1a3199b69aba233`, reconstructed from `git archive`,
as its pre-change Remora reference. Baseline and candidate use the same pinned
compiler, Cargo.lock and `qlog` feature. Phase 6 has no runtime tracing filter;
its default mode is the explicit reference for the new filtered-qlog mode.
Ordinary off means error threshold with mandatory diagnostics retained in both.

Before collecting new reference data, recorded
`benchmarks/policies/phase7-regression-pinned-plan.json`: CPU cost budgets of 0.02 core
per role for mandatory modes and 0.05 core for filtered qlog, additional sampled
RSS budgets of 2 MiB and 3 MiB respectively, and 0.5 ms added p95 RTT. Every active
flow must deliver all offered messages with zero loss/integrity/generator errors.
These absolute budgets apply only to the one-Session, 128-byte, 100pps workload.
They reserve most of the single owner core, cover the fixed diagnostics memory
plus a qlog allocation margin, and keep allowed added RTT below the separate
release threshold. They are not universal percentage tolerances.

The reference matrix has 50 runs: three pre-change modes × three directions ×
five repetitions, plus five unread-default-collector runs. Numeric bounds are
frozen from these measurements using twice each reference spread or fixed
resolution floors (two CPU clock ticks over a 4.9-second denominator, 256 KiB
sampled RSS and 0.25 ms RTT). No delivery variability allowance applies. The
candidate source tree is then reconstructed from the frozen baseline archive
and exact proposed Rust source overlay, and compiled in a separate target tree.
The proposal is known existing code; this replay creates a new prospective
acceptance cycle, not evidence that its original authorship followed a freeze.
The original missed freeze remains recorded.

The candidate matrix contains 70 adjacent pairs (140 runs): four modes × three
directions × five repetitions, plus default/qlog unread-collector cases × five.
The first executable alternates by pair index. Each pair re-measures the exact
baseline binary, using default for the qlog counterpart. Every baseline must
remain inside the frozen noise envelope; every candidate must pass the frozen
absolute limit and the paired increase limit. A noisy or failed baseline is an
invalid/failed comparison, never hidden by averaging. Reference, policy, proposed
sources, compiler/features, executable hashes, generator, child measurements,
profile, affinity, platform/locks/socket/offload/clock and pair order are checked.
Both separate native source trees and the exact binaries remain preserved.

Blocked collection requires `pipe_write` observed for each writer, the exact
`DiagnosticCollectionIncomplete` runtime failure and stop within five seconds,
while the independent traffic tool proves full delivery. It never passes SC-010.
Normal collection requires zero event/status/coverage loss and numeric overflow;
qlog must stay active within its filter/output/file reservations. High-rate
stopped-filter CPU behavior, capacity, WAN, ARM64 and release qualification remain
outside this deliberately limited active-qlog profile.

The first cross-revision attempt used a shared four-CPU mask and grouped
reference repeats. Its policy (`phase7-diagnostics-regression.json`, SHA-256
`91f9d6cdc87ceed825187a28e250ac2c34db8cf0dbdd4631c4574f408e4a3cec`)
was frozen before candidate reconstruction. After 30 completed pairs, 44 metric
checks failed, including Phase 6 baseline CPU/RTT outside its reference envelope.
This was stopped and retained as **failed/inconclusive regression evidence**,
not an accepted instrumentation result. It did not justify relaxing bounds.
Artifacts: `benchmarks/results/t071-phase6-reference-r2/` and
`benchmarks/results/t071-phase7-paired-candidate/failure.json`.

The next cycle keeps the same CPU/RSS/RTT budgets and zero-loss requirement,
using a separate pinned plan/policy. `taskset` assigns client/server/generator/
receiver to four distinct physical cores (0/2/4/6 on this host); all worker-thread
masks are checked throughout each run. The controller shares that four-core
mask. Reference repeats interleave modes/directions/blocked cases over five
rounds, capturing temporal variability rather than one initial contiguous block.
CPU governor/driver/min/max settings are recorded but not changed. This is
process affinity, not exclusive CPU isolation or fixed frequency. Shared-host
noise can still invalidate a comparison and is not attributed to instrumentation.

The first pinned attempt (`t071-phase7-pinned-candidate/`) stopped after 31
complete pairs when the next run reported a thread-affinity validation error.
Its policy (`phase7-diagnostics-regression-pinned.json`, SHA-256
`503327a8b3a0bb0e4c058f89214655b380e9a4284472cb9139f6551bb2d99635`)
and invalid run remain preserved. A startup race was reproduced independently:
10/10 immediately sampled `taskset` children still had the inherited parent mask
before `taskset` installed the assigned mask. The sampler lacked an assignment
readiness gate. The corrected `Owned` constructor waits at most three seconds
for the assigned mask before sampling; worker threads inherit it across exec.
Tests cover inherited-to-assigned transition and bounded cleanup on failure.
This correction adds no production Rust changes and no metric allowance.

A fresh 50-run reference was collected with the corrected, unchanged harness,
then `phase7-diagnostics-regression-final.json` was frozen before a new candidate
source reconstruction/build. SHA-256:
`4ea5872a3dddad6e62d7c1dfde65eb0ab753d587d49afdbd432b0865a0089cc6`.
Final-cycle artifacts are `benchmarks/results/t071-regression-final-inputs/`,
`t071-phase6-final-reference/` and `t071-phase7-final-candidate/`.

The final cycle completed with **70/70 paired regression checks PASS**. All earlier evidence continues
to apply only to its recorded executable identity. Historical reproduction
commands in the original same-binary section describe its original profile and
harness; the current `diagnostics.toml` selects this pinned/interleaved workload.


### Reproduce the cross-revision cycle

Use fresh output/source/target paths. Both builds require the pinned toolchain
and cached lockfile dependencies (`--offline` fails clearly if they are absent).
The test-only traffic executable is built once and shared unchanged. Preparation
snapshots the current proposed production sources and the exact Phase 6 archive;
subsequent source changes require a new proposal/reference/policy/candidate cycle.

```bash
cargo build --locked --release --manifest-path tools/traffic/Cargo.toml
python3 scripts/validate/diagnostic_inputs.py --prepare-baseline \
  --revision 34d4a8fa13aa67436e057773d1a3199b69aba233 \
  --inputs benchmarks/results/t071-new-inputs \
  --source /tmp/remora-t071-new-phase6 --target /tmp/remora-t071-new-phase6-target \
  --traffic-binary tools/traffic/target/release/remora-traffic
./scripts/validate/bench.sh --profile benchmarks/diagnostics.toml \
  --diagnostics-kind regression --diagnostics-stage reference \
  --baseline-binary benchmarks/results/t071-new-inputs/phase6-remora \
  --baseline-input-manifest benchmarks/results/t071-new-inputs/phase6-input.json \
  --traffic-binary benchmarks/results/t071-new-inputs/traffic \
  --regression-plan benchmarks/policies/phase7-regression-pinned-plan.json \
  --proposal benchmarks/results/t071-new-inputs/proposal.json \
  --output benchmarks/results/t071-new-reference
./scripts/validate/bench.sh --profile benchmarks/diagnostics.toml \
  --diagnostics-kind regression --diagnostics-stage freeze \
  --reference benchmarks/results/t071-new-reference \
  --regression-plan benchmarks/policies/phase7-regression-pinned-plan.json \
  --proposal benchmarks/results/t071-new-inputs/proposal.json \
  --output benchmarks/results/t071-new-policy.json
python3 scripts/validate/diagnostic_inputs.py \
  --inputs benchmarks/results/t071-new-inputs --policy benchmarks/results/t071-new-policy.json \
  --source /tmp/remora-t071-new-phase7 --target /tmp/remora-t071-new-phase7-target
./scripts/validate/bench.sh --profile benchmarks/diagnostics.toml \
  --diagnostics-kind regression --diagnostics-stage candidate \
  --reference benchmarks/results/t071-new-reference --policy benchmarks/results/t071-new-policy.json \
  --baseline-binary benchmarks/results/t071-new-inputs/phase6-remora \
  --baseline-input-manifest benchmarks/results/t071-new-inputs/phase6-input.json \
  --remora-binary benchmarks/results/t071-new-inputs/phase7-remora \
  --remora-input-manifest benchmarks/results/t071-new-inputs/phase7-input.json \
  --traffic-binary benchmarks/results/t071-new-inputs/traffic \
  --regression-plan benchmarks/policies/phase7-regression-pinned-plan.json \
  --proposal benchmarks/results/t071-new-inputs/proposal.json \
  --output benchmarks/results/t071-new-candidate
```

The tracked policy is evidence for its recorded inputs, not a reusable allowance
for arbitrary future sources or workloads. Retain failures and frozen limits;
correct a failed increment or define a new prospectively reviewed trade-off
before a new comparison. Do not overwrite the recorded reference or policy.


### Final T071 cross-revision results

**PASS: all 70 candidate runs and all 70 adjacent baseline counterparts satisfy
the unchanged frozen policy.** The separate pre-change reference has 50 valid
runs. Comparator revalidation also passes: exact source/executable/feature/
compiler identities, child summaries, profile, platform/clock/locks/affinity,
measurement order, prospective construction/build timestamps and policy seal.
No run is averaged into acceptance. The following averages are descriptive only.

| Mode | Mean process CPU increase, client / server (cores) | Mean owner CPU increase, client / server (cores) | Candidate sampled RSS max (MiB, either role) | Largest paired added p95 RTT (ms) |
| --- | --- | --- | --- | --- |
| default | -0.00080 / -0.00053 | -0.00027 / -0.00094 | 8.840 | 0.687 |
| filtered-qlog | +0.01071 / +0.01043 | +0.00827 / +0.00880 | 8.930 | 0.673 |
| off | +0.00111 / +0.00031 | +0.00043 / +0.00043 | 8.836 | 0.562 |
| verbose | -0.00071 / -0.00031 | -0.00030 / -0.00044 | 8.871 | 0.830 |

Small off/default/verbose CPU deltas overlap clock-tick/reference noise; negative
means do not establish an optimization. Active filtered qlog adds about 0.0107
client / 0.0104 server process cores on this workload. Its largest observed
process/owner increases are 0.01811 / 0.01408 cores across either role. The
largest sampled RSS increase is 388 KiB across all normal modes. Candidate
normal RSS max is 8.930 MiB and normal p95 RTT max is 1.305 ms.

The 0.5 ms intended RTT increment is **added to the frozen reference noise
allowance**, not a standalone observed-delta ceiling. Per-group RTT noise is
0.444–1.866 ms, reflecting variability on this shared, dynamically clocked host.
The largest paired increase is 0.830 ms and passes its specific frozen bound.
These runs establish the declared budget check, not a tightly resolved causal
latency cost or a confidence interval. No tolerance was enlarged after the
final candidate started.

Every active direction delivers **500/500** offered data messages per run
(0.1024 Mbit/s per direction). Candidate data total: 50,000 unique messages,
zero delivery loss, corruption or generator errors. Normal runs have zero
ordinary diagnostic drops, status write failures, coverage loss and numeric
overflow. All 15 normal filtered-qlog runs retain active filters with no stop or
output failure. The largest retained trace file is 677,075 bytes per role;
sampled active scratch is 65,536 bytes (one filter), final scratch zero.
File/filter caps are verified for this one-filter workload; multi-filter/churn
reservation pressure remains separate component-test evidence.

All ten candidate unread-collector cases retain full bidirectional delivery,
observe `pipe_write` for both writers, and exit 1 with the exact
`DiagnosticCollectionIncomplete` failure. Stop time is 1.006–1.008 seconds;
blocked sampled RSS max is 9.008 MiB. Missing status/correlation after the unread
prefix stays unobservable and incomplete. `sc010_claim=false` remains explicit.

Baseline revision: `34d4a8fa13aa67436e057773d1a3199b69aba233`.
Both release builds use the pinned Rust 1.99.0/Cargo.lock and `qlog` feature,
with source-archive build metadata recorded separately from the source identity.
The final candidate contains the previously verified review corrections; no
production Rust changes were made during this T071 measurement work.

| Immutable evidence | SHA-256 |
| --- | --- |
| Phase 6 release executable | `78e7351678f7d402b5782b30acc93f2a0d4eb771f1692ede51e75e0a19879244` |
| Corrected Phase 7 release executable | `4b68d6b5843965f60a071692dc26bbc690f327769df3d1070f08ccaedaeacd07` |
| Shared traffic executable | `e973d280db788bd948670d6e9d83e2db28e1f495f19cd87029bdaf99b621adad` |
| candidate_manifest | `19f45a78b6e1e53c0561466f01b67443a8775d8919878ea7523556a2d9e4fbb8` |
| candidate_summary | `b7638e161052e03932a99eea7e22d429cac448671c2bf839a9efb6dabf74831a` |
| comparison | `89e3b5f2671a127c0374c06db482f78c954b2d6b6a0f76a5908046e7425f6876` |
| paired_baseline | `3a370e08d984b10a723612b51114e8b3bc04dc8c5089edaf1510e2265305a861` |
| policy | `4ea5872a3dddad6e62d7c1dfde65eb0ab753d587d49afdbd432b0865a0089cc6` |
| ref_manifest | `da2930a7f5c51688b789adc12b73ec24254901ae7f88c7a5a51db7f05a122e9f` |
| ref_summary | `4839678acc37f7540a58f2632a07584ba3747f78d6b1d8cde71473aaf627d43b` |

The policy freeze precedes materialization, build and collection respectively:
`1791856343.2578025 < 1791856349.2625508 <= 1791856353.078742 < 1791856523.008746`
(UTC epoch seconds). The reference is the actual pre-Phase-7 implementation;
the candidate is a fresh reconstruction of a known existing proposal. This
closes the current scoped T071 checkpoint. It does not rewrite the original
missed freeze before the initial T064–T070 authorship; the comparison output
preserves `original_freeze_sequence_repaired=false`.

Native x86_64 Linux 7.2.9-arch1-1, Intel i7-13700K; four distinct physical cores
0/2/4/6, one per role. The controller shares that mask. `intel_pstate/powersave`
settings remain unchanged; frequency is not fixed and cores are not exclusively
isolated. The result is limited to one Session / 128 bytes / 100pps / five
five-second repetitions in each direction, active qlog below its rate ceiling.
It does not establish high-rate tracing/stopped-filter CPU, sustained capacity,
WAN, ARM64, release SC-003/004/005/007 or final Phase 8 qualification.

Verification: 64 Python tests pass, including cross-revision/provenance/plan/
child-summary/pair-order/single-run failure guards, exact blocked-failure checks,
fresh source/build preparation and taskset startup readiness. Both exact
release executables were built locked/offline with matched features. Syntax,
local documentation targets and diff checks pass. The earlier full Rust suite
verified the unchanged proposed production sources; no new full Rust, interop
or standalone operations qualification is claimed by this measurement increment.
All failed/invalid/preparation artifacts remain preserved; no commit/publication.
