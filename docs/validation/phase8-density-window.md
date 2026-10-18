# Phase 8 bounded receive-density history

Date: 2026-10-16. Status: final combined correction passes focused comparison;
earlier incomplete/failed candidates remain retained.
Decision: [ADR 0017](../adr/0017-recent-datagram-density.md).

## Behavior and checks

Each connection keeps a fixed 32-observation history of outer packets that
actually add native DATAGRAMs. Its recent maximum selects the outer-read quota.
Zero observations, including ACK-only input and queue drains, do not age the
history. A sustained change to large, single-DATAGRAM packets restores the
32-packet read quota after at most 32 nonempty observations. Repeated dense
bursts inside the window continue to constrain reads. History and rescans are
bounded; no allocation, task, mutex, unsafe code or configuration is added.

Existing owner-work, native queue, pacing, expiry, partial-send, authentication
and authorization boundaries remain intact. Only admitted active flows affect
their shared socket's quota; each connection owns independent history.

The native transition regression is compiled against an archived pre-change
source copy and fails at the expected stale-quota assertion (4 instead of 32).
The corrected production source passes that regression, native coalescing and
shared-socket tests, a model-based property over arbitrary observations, and
dense-burst/control-only retention cases. All 177 ordinary Rust tests pass,
with one additional independent-peer process test passing. Locked default/qlog
release builds, formatting and all-target/all-feature Clippy with warnings
denied pass. The independent fixture uses masque-go v0.6.0 and quic-go v0.63.0;
the exact executable hash is retained in the aggregate report.

No dependency or protocol parser changed. These checks do not refresh full
source-bound release qualification, native ARM64 evidence or every prior fuzz/
Miri campaign. Final qualification remains pending for the candidate binary.

## Prospective comparison

Raw artifacts, original sources/executables, driver copies, logs, hardware,
source hashes and aggregate results are retained under
`benchmarks/results/phase8-density-window-20261016/`.

The unchanged pre-correction qlog executable is
`3912e2574812ae86af4d3da580511409ed972f1eeef8c3ef9e52c295a24cccba`.
The history32 qlog candidate is
`6ef42bfcdbd78394462ec1bfd716c8d197480972bf58626e9cc12c5107a6ca74`.
Both use the same pinned quiche Git revision and unchanged dependency locks.

The original method's 15-second capacity reference loses 33.28% in one repeat
and has only 67 RTT samples per session. All original rows remain retained.
Before any candidate measurement, method v2 increases only the capacity
duration to 60 seconds and requires three new references for that workload.
All three original small/resource references remain in scope. The three new
capacity rows meet the unchanged 100-sample minimum. A supplemental driver
method-hash label was corrected with the original manifest preserved; raw
measurements were not edited.

`benchmarks/policies/phase8-density-window.json` freezes the selected reference
hashes and per-metric extrema plus the larger of two spreads or the declared
measurement floor. Its SHA-256 is
`866727fb46b1161fab649a7fdc98868bb19ea5c7468e6aa028744edb16946a25`.
Floors are two messages per interval, 0.002 core, 256 KiB RSS and 0.25 ms RTT.
Every small-packet candidate must additionally deliver no more than two fewer
messages than its adjacent control in either direction. No numeric threshold
is relaxed after candidate measurements.

Three repetitions alternate control/candidate order, with a matching direct
two-leg UDP baseline. Small simultaneous runs measure 15 seconds; capacity and
resource simultaneous runs measure 60 seconds. Every member must pass generator,
clock, diagnostics and RTT-sample validity. Candidate checks cover both
directions, delivery/loss, RTT, CPU/owner CPU and RSS, unchanged applicable SC
targets and frozen regression bounds. Stop at the first failed pair and retain
the partial result. Socket counters include warmup/drain and must not be treated
as measured-interval endpoint loss.

This is a focused correction comparison, not the complete T077 matrix. T076,
T077 and T078 remain pending; T079 is excluded. No commit or push is implied.

## History32-only comparison outcome

The first small/capacity/resource candidate rows pass all frozen checks. The
second small candidate loses six forward messages of 972735 (0.0006168%), versus
zero in its adjacent control, so the comparison stops at 12 completed rows and
four evaluated candidates. This exceeds the unchanged two-message regression
floor. Do not average the failure away or mark the comparison accepted.

Client queue-full snapshots increase by exactly six during the measured
interval, with no kernel overflow or outer QUIC loss. This is distinct from the
large-packet receive-quota issue. ADR 0018 records the next coordinated-service
candidate and its native regression. The history32-only executable, sources,
policy, raw rows and failed aggregate remain retained under the original paths.

## Final combined correction

ADR 0018 orders outer/control processing, native output, application enqueue,
then producer reads. Finite per-connection enqueue/output continuations carry
new staged work across eight-visit ring boundaries. The native full-queue
regression drops six new real UDP messages with the old order and delivers all
102 exact payloads FIFO with the correction. An initial reordered E2E attempt
exposes missing continuation, which is corrected before performance measurement.
The final source passes the 32-session isolation/overload/shutdown suites.

With the original application buffers, two complete comparison rounds pass,
but the third small row loses 13 messages (0.0013364%). All 13 are client
application-socket kernel drops, with no measured software queue-full increase
or outer QUIC loss. That second comparison stops at 21 rows/seven candidates
and remains failed. A subsequent same-binary six-row buffer diagnostic shows
seven server application-socket drops in one old-setting repeat and zero loss/
socket drops in all three 128 KiB requested-buffer repeats.

ADR 0019 selects 128 KiB requested application RX/TX buffers as an explicit
kernel-memory trade-off after the reactor fixes. On this host, effective RX/TX
is 256 KiB each. The configured aggregate application-socket limit is 512 KiB
for one client listener and 16 MiB for 32 server relays, increases of 256 KiB
and 8 MiB respectively. These limits are separate from RSS and are not eager
allocations. Explicit `io.relay_socket_buffer_bytes = 65536` retains the former
setting. No packet deadline, admission ceiling or user-space queue is enlarged.

The final qlog executable SHA-256 is
`1229d87f5fe1fbfbf5c0ccf231d033af2e96f1645184d05cd7f826c94e78cdee`.
Policy SHA-256 is
`e16362935def466725f5862fe23a9803a92b36abc9201ecadea0c696a1709dd6`.
Every existing delivery/loss/RTT/CPU/RSS bound is byte-equivalent to the previous
policy. Additional guards verify effective socket limits and bounded application
socket counts. The direct UDP baseline is rebuilt with matching buffers; tool
generator/responder sources and locks remain unchanged and package checks pass.

All 27 rows and nine candidate checks pass in 1374 seconds (22m54s). The table
reports the worst individual candidate measurements across three repeats,
with directions shown forward/reverse:

| Workload | Interval | Minimum delivered Mbit/s | Maximum loss % | Maximum p95 RTT ms |
|---|---|---|---|---|
| 1 × 128 simultaneous | 15 s | 66.405376 / 66.405376 | 0 / 0 | 20.261 |
| 32 × 1200 capacity simultaneous | 60 s | 874.90416 / 874.89536 | 0.000165 / 0.001171 | 20.898 |
| 32 × 1200 resource simultaneous | 60 s | 500.736 / 500.736 | 0 / 0 | 20.690 |

Worst per-role average CPU is 0.918404 core and peak RSS is 12726272 bytes
(12.137 MiB). Configured kernel budgets and observed queue peaks are recorded
separately. Source/input stability checks pass, all owned namespaces are closed,
and no validation job remains active.

Validation on the final source passes locked default/qlog release builds,
formatting, all-target/all-feature Clippy with warnings denied, 179 ordinary
Rust tests, one independent-peer process test, and 20 traffic tests with its
separate locked build/Clippy. Native/property cases cover workload transitions,
control-only retention, dense bursts, shared-socket admission and finite staged
continuation. This is native x86_64 veth/netem evidence, not physical NIC, VM,
ARM64, 30-minute isolation/WireGuard or full reference qualification.

The aggregate is
`benchmarks/results/phase8-density-window-20261016/drain-order/receiver-buffer/summary.json`.
Earlier failures and their original policies/executables remain unchanged.
T076/T077/T078 remain unchecked for final qualification; T079 is excluded.
