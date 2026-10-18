# Phase 8 loss investigation after the Git-source migration

Research performed 2026-10-15, recorded 2026-10-16. No production correction is
accepted by this report. T076/T077/T078 remain open; T079 is excluded.

## Scope and preserved inputs

The production executable remains
`3912e2574812ae86af4d3da580511409ed972f1eeef8c3ef9e52c295a24cccba`, using
quiche fork revision `154ce1995f23efa434e72dac269f707145be2015`. Every production
Rust source hash still matches the Git migration report. Diagnostic copies of
Remora source and executables are retained under ignored benchmark results;
the Cargo dependency checkout was not edited.

Four completed native x86_64 diagnostic series took 848 seconds in total. Each
used the existing isolated veth/netem two-leg 1 Gbit/s, 20 ms path, tools-only
collector affinity and bounded workload durations. These are not VM or physical
NIC performance claims. All 27 completed rows had zero generator/integrity
errors and no calibration-validity rejection. The resource A/B rows below are
30 seconds, not full 60-second resource qualification.

The aggregate report, source/binary/driver hashes and individual observations
are in `benchmarks/results/phase8-git-loss-research-20261015/summary.json`.
Original failures remain retained. Kernel socket counters include warmup and
drain; the delivered-loss percentages use only the declared measurement interval.

## Reproduced symptoms and rejected buffer-only remedy

`phase8-git-loss-20261015-r2` repeats the three previously problematic workloads
twice on the production executable. Capacity runs show server outer-socket
drops of 136 and 427; forward measured losses are 0.004974% and 2.33409%.
Both 60-second resource rows still deliver at least 500 Mbit/s per direction.
Thus this short series reproduces outer drops, not the complete earlier
resource acceptance failure.

`phase8-git-buffers-20261015` alternates three 30-second capacity repetitions
with default and 1 MiB requested outer socket buffers. Effective receive buffers
are respectively 524288 and 2097152 bytes. Default forward losses are 0.00903%,
59.18302% and 54.35444%; enlarged-buffer losses are 0.07436%, 0.13134% and
2.60109%. Both configurations still lose packets on the server outer socket.
Larger buffers alone are therefore not an accepted remedy. No default changed.

In sampled drop windows of the 59% run, the server owner accumulated about
50–52 ms CPU time per 56–58 ms sample, with sub-millisecond scheduler wait.
This does not resolve sub-sample timing, but does not support a long scheduler
wait as the sole explanation of those windows.

## Persistent receive-density estimate

`Runner::outer_packet` retains the lifetime maximum increase in quiche's receive
DATAGRAM queue after one outer packet. `outer_receive_quota` divides 32 by that
maximum. Short 128-byte RTT probes can coalesce even during a workload whose
main payload is 1200 bytes. A brief dense packet consequently lowers future
outer reads for the entire connection generation.

`phase8-git-service-profile-20261015` uses an otherwise matching source copy
with one numeric service-state record per snapshot. Measured density values
3–5 leave read quotas at 10, 8 or 6 on the large-packet workload. All three
server user-space perf captures succeeded; client captures were unavailable
because perf could not reserve its mmap pages. No host perf setting changed.
The first server profile attributes approximately 9.8% of user-space samples
to slab return/drop, 5.6% to relay input, 5.4% to readiness polling and 4.6% to
UDP receive-batch work. These are sampled exclusive symbols, not full-stack
or kernel-time attribution and not proof that any one function causes loss.

A native QUIC regression, `outer_read_quota_recovers_after_probe_burst`, sends
eight short DATAGRAMs, drains them, then actually delivers a 1200-byte DATAGRAM.
The high-water implementation fails the final quota assertion: actual 4,
expected 32. A source copy using the most recent nonempty observation passes.
The source copies, test text and before/after logs are retained in
`phase8-git-service-probe-20261015`. Initial missing-fixture and sandbox attempts
are separately retained and are not the reproduced failure.

## Matched exploratory A/B and remaining work

`phase8-git-density-ab-20261015` compares two equally instrumented binaries,
alternating order between two repeats. The sole behavioral change is replacing
the lifetime maximum by the most recent nonempty density observation. Perf is
not active during this comparison. Forward loss results are:

| Workload | High-water repeat 1 / 2 | Recent-density repeat 1 / 2 |
|---|---|---|
| 32 × 1200 capacity, 15 s | 0.669768% / 0.278485% | 0% / 0.000658% |
| 32 × 1200 resource, 30 s | 0.003387% / 0% | 0% / 0% |
| 1 × 128 simultaneous, 15 s | 0% / 0% | 0.001953% / 0.007299% |

Capacity p95 RTT changes from 28.864/21.060 ms to 20.742/20.768 ms. Recent-density
resource rows deliver 500.736 Mbit/s in both directions. However, small-packet
loss also increases in the reverse direction (up to 0.002673%, versus zero).
The short comparison supports stale density as a performance contributor;
it does not establish that this explains every prior failure or that the
simple replacement is an acceptable final fix. Remaining small-packet loss
has not been attributed to a specific queue or transport mechanism.

Next implementation work should let the estimate recover after workload
changes while retaining protection against dense small-DATAGRAM bursts, with
the native transition regression plus ACK-only, continuous-small-packet and
shared-listener cases. A bounded recent-history estimate is a candidate to
evaluate, not an adopted design. Preserve existing per-turn work, native queue,
fairness, cancellation and expiry bounds. Then perform fresh matched regression
measurements against a prospectively frozen policy before final qualification.
No acceptance threshold, production buffer default or quiche revision changed.
