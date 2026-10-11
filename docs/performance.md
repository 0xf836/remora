# Performance evidence

This report retains measured outcomes and limits. Raw manifests, samples, logs
and historical comparisons remain in ignored local `benchmarks/results/`
archives, not Git. See [artifact retention](validation/README.md).

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
