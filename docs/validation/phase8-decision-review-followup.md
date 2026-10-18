# Phase 8 independent decision review: future investigations

Recorded 2026-10-17 after an explicitly requested independent read-only review.
The reviewer received a fresh context with the problem, implemented changes and
observed results. ADRs, previous reasoning, planning/task narratives and derived
interpretive reports were excluded from the review. The reviewer inspected
normative requirements, source mechanisms and primary measurement artifacts.
No code changes or runtime experiments were performed during that review.

## Assessment retained

The reviewer accepts the selected native x86_64 reference checkpoint and sees
no evidence requiring rollback of output-order correction, bounded service
quotas/continuation, slab trimming or the current socket buffers. Accepting the
measured small CPU increase is reasonable. The successful qualification does
not establish a complete causal explanation of every loss episode, general WAN
robustness or the best congestion controller for all UDP paths.

The items below are a research backlog, not new Phase 8 tasks or authorization
to start experiments. T079 remains excluded. The current CUBIC selection and
owner-approved CPU cost are unchanged. Before any experiment, freeze its
comparison/validity criteria and record exact source/binary/workload identities.
Keep instrumented observations distinct from production performance acceptance.

Dependency follow-up: keep the quiche fork pinned and source-verified, limit
its delta, and retain a path back to upstream when an applicable fix is
available. The reviewer found no basis for replacing the QUIC stack. This note
does not start upstream monitoring, another fork change or dependency migration.

## 1. Congestion controller and high-rate loss amplification

**Observed:** The current CUBIC control/drop-five experiment loses 26864–34527
reverse application messages after exactly five lost outer packets, with zero
measured outer/application kernel-overflow deltas in the impaired runs. Earlier
BBR2 results lose about 9–10 messages in that scenario, but use another binary.
New BBR2 medium loss reaches about 0.01848%; paired no-regression failures do
not by themselves mean a violation of the product's added-loss requirement.

**Open question:** Does transport credit contraction explain the application
loss, or does local generation/drain scheduling contribute materially? Which
controller gives the preferred delivery/freshness trade-off on target networks?
Congestion control is mandatory; circumventing it is not a proposed remedy.
CUBIC remains the default and BBR2 remains an option worth revisiting if lossy
paths are a product priority. Pausing BBR2 work is a scope decision, not proof
that it has no useful direction.

**Small distinguishing experiment:** Use the same current source, buffers and
offered load for short CUBIC/BBR2 control/drop-five cases. Around injection,
collect a bounded aligned timeline of cwnd/bytes-in-flight or available credit,
native/application occupancy, queue-full/expiry, generation and eligible output
submission, linked to application sequence gaps. This separates credit shortage
from owner service delay. A full qualification repeat is unnecessary for this
question.

Primary evidence: `benchmarks/results/phase8-cubic-output-order-loss-20261016/`
and `benchmarks/results/phase8-final-cc-comparison-20261016/` raw summaries and
injection counters; retain their different binary identities.

## 2. Cross-client coupling through shared-socket receive density

**Observed mechanism:** `ReceiveDensity` retains a maximum over the last 32
nonempty observations; empty observations do not age it. `outer_receive_quota`
uses the maximum density across active connections on the shared socket. One
accepted dense connection can therefore lower the common receive quota and
retain its influence while it subsequently sends infrequently.

**Unproven risk:** This may affect another client's loss/latency/service cost.
No demonstrated fairness or isolation failure follows from code inspection
alone. A smaller receive quota can also improve timely downstream service.
The one-connection homogeneous reference matrix does not distinguish this
cross-client case, and ordinary overload/malformed-input tests are different.

**Small distinguishing experiment:** Place two independent accepted QUIC clients
on one server socket. Keep client A's workload fixed. Let client B establish
an actually observed high DATAGRAM density with short payloads, then send rarely
while remaining active. Compare A's receive quota, service gaps/visits, loss and
latency before/after B's burst and against an A-only control. Check actual density
rather than assuming a particular packing ratio.

**Alternative to evaluate only if needed:** Interleave outer receive and native
queue service according to actual queue occupancy with a shared finite work
budget. Compare that to predictive density, while preserving control/close order
and original-generation commitment. This is not a request for an immediate
reactor rewrite.

Relevant mechanism: `src/reactor.rs` (`ReceiveDensity`, `outer_receive_quota`).

## 3. Buffer headroom, service pauses and latency freshness

**Observed:** Resource owner CPU is about 0.89–0.91 core and peaks around 0.938
over all reference workloads. A two-core process budget does not double the
capacity of one sequential packet owner. Larger socket buffers provide space
for temporary backlog, but kernel waiting time precedes the userspace receive
timestamp used for application/native residence accounting.

**Competing explanations:** Corrected service order plus buffers may tolerate
brief scheduling pauses; alternatively, greater kernel backlog may defer
overflow and move cost into a latency tail near the owner's service limit.
Average CPU, aggregate loss and sparse p95 probes do not fully distinguish them.
The declared 100us generator batching and 20ms completion reserve are a specific
arrival pattern, not a universal model of application traffic.

**Small distinguishing experiment:** At one relevant working point, compare old
and new buffer requests with identical short owner pauses or matched ingress
bursts, preserving average offered load. Record service gaps, kernel backlog/drop
deltas, application sequence gaps and latency tails. Vary generator phase between
cases. Determine whether added buffering predictably moves the overflow threshold
by its backlog allowance or whether generation/drain dynamics change without
an imposed pause. Account for kernel waiting separately from userspace residence.

Do not increase FIFO sizes or deadlines blindly. UDP receive suppression moves
backlog/drops into the kernel; it does not provide backpressure to a remote UDP
sender. Any future drop policy should explicitly weigh freshness against delivery.
Connection-owner sharding is an option only after an actual owner bottleneck is
established; extra per-packet tasks/shared quiche locks are not the default answer.

## Metadata finding resolved separately

The reviewer found stale requested-buffer values in the T077 manifest. All 300
baseline/tunnel role diagnostics agree on actual effective values, so unequal
buffers were not found. The runner correction and additive historical erratum
are recorded in [the metadata correction](phase8-benchmark-metadata-erratum.md).
The original manifest, numerical results and historical source identities remain
unchanged. This finding does not require another full measurement campaign.

## Scope of conclusions

Retain the distinction between delivery/direct ratio at a selected sustainable
offered rate and an unconstrained maximum-capacity claim. Retain reference-path,
single-owner headroom, active diagnostic/offload and native ARM64 limits. The
reviewer did not independently execute or inspect every functional test; stage
results were used as bounded evidence. No new broad robustness or independent
full-code-audit claim follows from this decision review.
