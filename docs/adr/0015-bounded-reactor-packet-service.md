# ADR 0015: Readiness-driven reads and bounded fair packet rounds

Status: candidate implementation; focused A/B and final qualification pending.
Date: 2026-10-15.

## Evidence and prospective policy

Final r3 evidence shows single-session small-packet added loss and a resource
pair delivering 368/490 Mbit/s instead of 500/500. Server owner uses 0.938 core,
with 27.3M receive/error-queue visits and 870760 ToTarget expiries in that run.
Five new 15s samples for each exact workload reproduce loss on the unchanged
qlog binary. Original binary/source and counters are frozen under
`benchmarks/results/phase8-performance-fix-20261015`. Before production edits,
`benchmarks/policies/phase8-reactor-service.json` binds those five reference rows,
binary and per-metric extrema/spread/floors, with unchanged absolute SC targets.
Its SHA-256 is
`27c93dfb58f5a248521d7966a5f7a172db9a5b527d6fe17cafa6500a192065a3`.
This 15-second profile is diagnosis/comparison, not final resource qualification.

## Candidate decision

Each PollSocket carries one owner-local read-continuation bit, initially true.
Native EAGAIN clears it, except for a pending bounded error-queue suffix.
Successful/budget-limited reads retain it; the reactor readiness poll marks it
again. Idle sockets skip receive/error-drain/slab work. New sockets and local
quota continuations still get a direct probe before an edge is reported.
Read syscalls execute inside AsyncFd::try_io whenever cached readiness exists;
its generation handling clears only observed EAGAIN. A direct fallback never
clears a possibly racing edge. No fake EAGAIN clearing follows the read.
Write wakeups cover only the queue direction submitted to that socket.

Connection control/incoming-DATAGRAM, application enqueue and QUIC output
are separate owner visits. Incoming DATAGRAM and enqueue each have a 32-item
cap. One output visit divides its 32-item work allowance into at most 16
generations and 16 physical submissions, using one shared `OUTPUT_QUOTA`. This
replaces the former four-packet physical flush and keeps queued ciphertext close
to its native generation time. Setup/control/body quotas remain four. Enqueue
selects up to 32
ready sessions and repeats fair rounds: one packet per selected session before
another round, with a 32-packet total cap and immediate stop at native capacity.
This lets one active flow use the finite quota while preserving original
packet/expiry on native capacity. Active-session membership in the positional
owner visit ring stays stable across packet arrivals/drains; empty visits are
cheap because read syscalls are readiness-gated. The owner window stays at most eight
visits before timer/cancellation work. Thus packet-data work stays <=256 items
per window; control/library internal parsing is not a fixed instruction count.

Selection previews record a fixed array of at most 32 cursor steps. Only
consumed/dropped packet progress is committed; native Capacity restores the
previous cursor so the unserved producer gets the next free slot. The finite
generation horizon is preserved across partial rounds and newly arriving flows.
A real two-stream native test, with one free native slot and continuous backlog,
fails on the original production source and on the preceding candidate, then
passes after this fix. All original production-source hashes match the recorded
T076 prerequisite inputs; the isolated baseline build adds only the regression
and test-harness field adaptation. This is a reproduced fairness error, not a
claim that it alone explains every failed r3 pair.

Application-pool idle storage keeps one 256-packet owner window for reuse.
At most 32 excess cached slabs are retired per owner turn; live leases,
preallocated return slots, admission/queue limits and original expiry remain
intact. Startup's congestion-window-limited backlog had left about 1.25 MiB
cached for the connection lifetime even with only 0–3 packets queued. Bounded
retirement addresses that retained high-water cache without blocking the shared
listener behind one congested flow. It adds safe ownership-only maintenance,
not a new packet queue or allocator/OS tuning. Actual RSS benefit is measured
against the same frozen policy, not inferred from released logical bytes.

Outer UDP reads retain the configured batch ceiling, at most 32. A connection
records the high-water increase in its native receive-DATAGRAM queue caused by
one outer packet. Socket reads cap the batch at `max(1, 32 / density)` for the
highest observed density among connections with an admitted active session on
that socket. Unadmitted connections do not constrain other clients' read quota.
The observation is
owner-local, initialized to one and reset with the connection generation.
Queue draining and ACK-only packets do not erase it. Large-payload batches can
therefore coalesce ACK work; multiple small DATAGRAMs per QUIC packet reduce the
next read batch before the incoming-DATAGRAM service visit. The observation is
an adaptive service heuristic, not a bound on future hostile frame density.
The lifetime high-water rule described here is superseded by the bounded
recent-history candidate in ADR 0017 (2026-10-16); earlier measurements retain
their original implementation and provenance.
Native queues remain bounded and retain their documented overload/drop behavior;
a sudden unobserved density change may still drop packets. Shared server sockets
use the most conservative observation among their resident connections.

## Preserved unsuccessful experiments

Prototype 1 combined eight-item enqueue/receive/generate work. It reduced CPU
but still failed small-packet added-loss checks. Increasing separate visits to
32 with a fixed three-packet outer read cap (prototype 2) gave zero loss in the
pilot, but resource RSS exceeded the frozen client and server regression bounds.
Prototype 3 reduced generation to eight. Client RSS still failed, and a separate
smaps diagnostic run exposed substantial forward loss. These are failed attempts,
not successful qualification. Their binaries, sources, raw rows and evaluations
are retained without changing the original policy.

Prototype 4 introduced density-based reads but kept packet-dependent client
session membership in that ring. Resource reverse delivery fell to 443.21 Mbit/s,
with 11.49% loss and 133183 cumulative reverse expiries. Its failed evaluation is
retained. Removing an empty session from a positional ring changes subsequent
indices; repeated packet arrival/drain can skip work. The next candidate restores
the original stable active-session membership and retains syscall readiness gates.
Prototype 5 passed pilot loss/delivery but failed the client RSS bound by 163840
bytes. Prototype 6 used generation eight on both roles: RSS passed, but reverse
delivery fell to 377.65 Mbit/s, with insufficient RTT samples. Prototype 7 used
server32/client8 and passed the pilot loss/delivery/RSS/CPU/RTT bounds. The next
candidate also prevents unadmitted connections from reducing the shared-socket
read quota; it requires fresh measurements. All earlier failures remain retained.

Prototype 8 passed measured loss/delivery but again failed the client RSS bound,
at 12021760 bytes. The comparison stopped at its first failed check. One observed
outer QUIC loss correlates with the native packet-history growth; an allocation-site
proof is not claimed. The next candidate adds the bounded idle-cache retirement
above, with native and three source-included Miri ownership cases. This remains
an experiment until all repeated frozen-policy checks pass.

Prototype 9 passed the first two small-packet checks and first resource check,
then failed a resource repeat with forward loss and client RSS growth. Returning
both roles to generation 32 (prototype 10) passed two completed 60s resource
candidate runs with zero loss; the diagnostic series was stopped to fix the
reproduced cursor error. Those partial runs are not a completed qualification.

## Prospective author review of the RSS trade-off

The first policy remains immutable and its failures remain failures. As allowed
by the benchmark contract's prospective trade-off rule, a new comparison policy
`benchmarks/policies/phase8-reactor-service-tradeoff-v2.json` permits only
resource RSS to rise by at most 1 MiB per role above each original reference
maximum: client 11599872 -> 12648448 bytes, server 12333056 -> 13381632 bytes.
Both roles retain native QUIC packet history. All other reference
bindings, metrics, repetition/ordering and absolute SC criteria stay unchanged.
It applies only to a fresh five-repeat comparison, not to any prior measurement.
SHA-256: `bd6d188b394abe6f43dff3b3f83fa38a34f3d2eb293afa25a069d29f903289d9`.
The earlier client-only policy draft is retained at
`phase8-reactor-service-tradeoff.json` (SHA-256
`b6a2e9f335b46cf31ceb907fdcfcc70840cbb4c2a14e2b7a97336c1edd39c8a4`);
no measured comparison was executed under it. The symmetric v2 review is frozen
before the first new regression comparison.

This is an intentional, reviewed memory cost for balanced bounded packet service,
not a dismissal of increased RSS as noise. Observed native anonymous mapping
steps were about 528 KiB; retain the original 256 KiB sampling/allocator floor
plus code/allocator headroom, rounded to a declared 1 MiB maximum increase.
Allocation-site attribution is still unproven, so acceptance checks actual
process RSS. The increment is under 0.8% of SC-007's unchanged 128 MiB budget.
An eight-packet client generation limit was rejected after real forward loss;
an eight-packet server limit was rejected after reverse loss. Shared-listener
backpressure was rejected because it could couple independent healthy flows.

Author security/ownership review: no authentication, target authorization,
generation, expiry, queue capacity, native partial-send or close rule changes.
Unadmitted connections do not affect read quota; live leases survive cache
retirement; each cursor step and maintenance suffix is bounded. No unsafe code,
dependency, worker/mutex or wire/config/CLI change is added. Native tests include
before-admission/after-close negatives, actual coalescing, real readiness,
native backpressure and reserved-buffer lifetime cases; three source-included
Miri cases pass. Independent review was not requested. Full functional/security/
interop/SC-009 and reference qualification remain mandatory before release.

At equal useful traffic, prototype 3's client sent 1,717,918 outer packets versus
1,291,958 for the original binary. Fixed small outer batches allow more frequent
ACK generation. Smaps showed additional anonymous resident memory while the
application pools/heap stayed close. The pinned quiche recovery implementation
retains a growing packet-history deque; increased packet/ACK work is a plausible
contributor, not a measured allocation-site attribution. The adaptive read quota
must be evaluated against the same frozen RSS/CPU/loss bounds; do not waive the
failed pilot bounds retrospectively.

## Invariants and qualification

Do not extend queue/ciphertext deadlines, resource bounds, settings/auth/policy,
stream/session generation, packet boundaries, partial-send or close semantics.
No new tasks, mutexes, unsafe code, dependency, wire/config/CLI change is added.
Native tests cover initial read, quota continuation, empty-read suppression,
real rearming, single-flow quota use, native coalescing/density observations,
five-flow fairness, close/expiry and blocked-window behavior.

Frozen old/new binaries use adjacent alternating comparisons plus a contemporary
two-leg UDP relay. Reject generator/clock/status/sample failures and any per-run
metric/SC failure; never average a bad repeat away. Review delivery, RTT, CPU,
RSS, owner headroom and syscall costs together. Accept only measured benefits
within frozen tolerances. Final T076/T077 on the changed production source remain
necessary; original functional evidence does not qualify a candidate binary.


## Physical-output correction and current candidate

The earlier candidate increased application/native generation work while the
physical `flush` still selected only four ciphertext packets. A native real-UDP
regression with 20 ready immutable ciphertext packets fails before the change
(actual first submission four) and passes after it (first submission 16, four
retained, exact FIFO bytes and no extra packet). Existing pacing/partial-progress
and mixed-ciphertext tests remain required. Generation and submission now share
one 16+16 output visit, preserving the 32-item ceiling. The current source has
173 passing native Rust tests with two external-peer cases separately invoked,
Clippy/locked qlog build passes and three source-included Miri ownership passes.

Additional diagnostic builds/configurations were not adopted: disabling HyStart
also failed resource delivery; increasing socket or native-queue budgets alone
was not a complete fix. Temporary source-copied quiche counters isolated actual
congestion events from HyStart exits without persisting payload/headers/secrets.
One failed tuned diagnostic had three congestion events accounting for 108 lost
bytes, zero HyStart exits and zero kernel/qdisc drops. Small outer losses can
leave CUBIC below the reference path's BDP and amplify application expiry; exact
origin of each lost packet is not attributed. No quiche patch, CC change, new
configuration default or dependency is included in production.

For the current fresh comparison, the same reviewed numeric bounds are bound
by `benchmarks/policies/phase8-reactor-service-reviewed.json`, SHA-256
`29a72c36307240083d9d2d8e99dbb69a6f1bfae998d0dc2d02520925b7dc4334`.
All workload metric bounds are byte-equivalent to the preceding v2 policy;
only prospective comparison scope/provenance changes. The failed v2 comparison
on the earlier source remains failed. Original strict-RSS evaluation remains
separate; final T076/T077 and every unchanged absolute SC target still apply.

## Collector placement correction

`comparison-output12` completed five adjacent old/new/direct repetitions per
workload with zero candidate loss in all ten short runs. The original strict
RSS policy still rejects its resource RSS; the separately frozen reviewed
policy passes all ten. A subsequent three-pair 60s `resource12` series nevertheless
fails one reverse repeat: 458.398 Mbit/s, 8.45514% loss. The other two repeats
pass; this is not stable resource qualification. The first outer loss occurs
during startup, with the low congestion window recovering over about 20s while
application queues expire. This traces the amplification mechanism, not the
origin of the original outer loss.

The collector had inherited CPUs 0–23 while polling 34 traffic/role processes
at 50ms intervals; role CPUs were 0–3 and 4–7. Unlike traffic children, it had no
placement constraint and could compete with either packet owner. Each native
run now pins its collector to tools CPUs 8–23, records requested/effective
placement, verifies it every sample and restores the caller on every exit.
The host's sibling topology confirms tools share no physical core with either
role. Native affinity success/failure restoration and pre-mutation rejection
regressions pass in the 89-test Python suite. Earlier evidence keeps its
original unpinned collector method; new comparative claims need a fresh matched
reference and prospectively frozen policy.

`resource-pinned12` completes three paired 60s absolute-criteria probes on the
same candidate12 binary. Each direction delivers >=500.73296 Mbit/s; forward
loss 0/0.000128/0.000607%, reverse zero. Per-role CPU <=0.919 core, peak RSS
<=12865536 bytes and p95 RTT <=20.588ms. All generator/integrity/clock/coverage
checks pass. This improves evidence but does not prove collector contention was
the sole cause; it does not replace the complete reference matrix or justify
accepting the preceding failed repeat.

The fresh tools-only `comparison-pinned12` fails at its fourth candidate check
(second resource repeat): reverse 278.44928 Mbit/s, 44.391999% loss, p95
39.091ms and insufficient RTT samples. Its earlier passes and the separate
three 60s passes do not qualify candidate12. The series stops immediately and
T076 is not launched. The fresh policy is retained at
`phase8-reactor-service-pinned.json`, SHA-256
`7eb49746d766aab4fced0a76d3b52f730c93d3758512734a662c77c0e1500357`.

A temporary source-copied quiche diagnostic on the same candidate reproduces
44.69% reverse loss. The three initial congestion losses are application-epoch
PING-only packets of 36 bytes. Each is physically submitted within 14us, and
the peer receives corresponding 36-byte packets with a known CID, but none is
accepted as a fresh QUIC packet. There are zero outer-kernel/qdisc drops. This
narrows the cause to malformed generated small packets rather than wire loss.
The upstream minimum-padding branch uses remaining congestion credit after
PING; if fewer than three bytes remain, it silently skips padding. Header
protection uses bytes beyond the transmitted encrypted suffix. A deterministic
native dependency regression is being prepared on checksum-verified official
quiche 0.30.0, before any production dependency/source change is adopted.
