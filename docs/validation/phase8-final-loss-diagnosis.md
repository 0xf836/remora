# Final T077 loss diagnosis — 2026-10-16

Status: causal diagnostic experiments complete; production unchanged;
T076 remains passed, T077/T078 pending, T079 excluded.

All experiments use the qualified qlog Remora executable
`1229d87f5fe1fbfbf5c0ccf231d033af2e96f1645184d05cd7f826c94e78cdee`
and traffic tool `cb2f0d710ce8c6f705051bb958aa683b1499c4cdf6d9d8bfd4a09a17cb866f50`.
Production inputs/executable hashes were checked again after all campaigns.
No production or traffic-tool source/default was changed. Configuration variants
below are diagnostic only. Three successful campaigns contain 27 runs and take
698 seconds in total; fresh private namespaces are cleaned at each completion.

## Original observations

The complete T077 cycle retains an invalid 32×1200 simultaneous capacity
baseline (forward loss 0.115706%, ceiling 0.1%) and a failed SC-004 row:
1×512 simultaneous repeat 2, 91552 datagrams/s/direction, reverse tunnel loss
1.974980% against zero baseline loss. All 75 pairs and churn completed.

In the failed 512-byte row, aggregate client kernel UDP-drop increments occur
at measured +7602ms (85) and +7653ms (62). The server increments are +7653ms
(81) and +7857ms (87). Server QUIC loss rises by five packets, then reverse
application queue-full/expiry counters rise by 26923/28 in the bracketing
snapshots. The QUIC cwnd summary is sampled every five seconds, so it cannot
resolve the immediate reduction from this original log alone.

The original resource samples did not separate outer/application sockets or
record scheduler runqueue delay. They establish kernel overflow and subsequent
queue pressure, but cannot retrospectively prove the precise socket split or
whether a scheduler pause, softirq delay or another burst caused the trigger.
Do not claim a new quiche defect from these observations.

## Unchanged-candidate repetitions

`benchmarks/results/phase8-final-loss-probe-r2-20261016/` contains six alternating
baseline/tunnel pairs of the exact 1×512 simultaneous workload (3s warmup,
15s measured), then three 60-second baseline repetitions of 32×1200 simultaneous
at 91136 datagrams/s/direction. Collector and roles use disjoint CPU masks.
The collector separates outer/application socket drops for both Remora and
baseline, with measured-interval deltas instead of startup cumulative totals.

All six 512-byte pairs stay inside the loss threshold: worst tunnel loss is
0.006481%, versus baseline worst 0.001019%. One tunnel repeat records two outer
server drops and 43/89 application client/server drops. Two other repeats have
small application queue-full deltas (21 and 54). All three minute-long capacity
baselines have zero endpoint loss and zero sampled kernel-drop deltas.
Generator/integrity checks pass. These repeats do not erase the full-cycle
failure or constitute a replacement qualification matrix.

The initial diagnostic directory `phase8-final-loss-probe-20261016` is retained
as a failed collector attempt: after its first baseline run, analysis requested
a layer field that the collector had only populated for `--config` processes.
The second driver adds classification of baseline command arguments. This was
a diagnostic harness error, not a Remora failure.

## Controlled loss establishes amplification

`benchmarks/results/phase8-final-five-packet-probe-20261016/` contains three
alternating control/drop-five pairs. At measured +6 seconds, an atomic nft
rule is installed only in the owned client namespace. It matches outer UDP
packets larger than 400 bytes from the server and drops exactly the first five;
the otherwise matching control rule drops zero. Retained nft counters confirm
exactly five drops in each injected run. Rule setup is also present in controls.

| Repeat | Reverse control loss | Reverse loss after five outer drops | Missing application datagrams |
|---|---:|---:|---:|
| 0 | 0% | 2.783336% | 38223 |
| 1 | 0% | 2.848654% | 39120 |
| 2 | 0% | 2.872976% | 39454 |

All injected runs have zero sampled kernel socket overflow. Application
queue-full and expiry counters rise after the injection. Thus a small outer
loss event is sufficient to create the large application loss pattern without
a sustained kernel overflow. The pinned quiche CUBIC implementation's
`congestion_event` reduces cwnd with `BETA_CUBIC = 0.7`; a constant-rate UDP
producer does not respond to that reduction. Bounded queues discard excess
input while transport capacity recovers. This mechanism is consistent with
the original sequence; it does not prove the original host pause's cause.

## Controlled receive pause and bounded-buffer candidate

`benchmarks/results/phase8-final-outer-burst-probe-20261016/` contains three
alternating pairs with identical workload and a requested 5ms SIGSTOP/SIGCONT
pause of the owned client at measured +6 seconds. Each run records the observed
signal interval. The only configuration difference is requested outer RX/TX
buffer size, set explicitly on both roles: 262144 versus 1048576 bytes.
Application buffers retain their 131072-byte request. Generator and responder
continue running during the client pause.

| Repeat | Outer drops with 256KiB request | Reverse loss | Outer drops with 1MiB request | Reverse loss |
|---|---:|---:|---:|---:|
| 0 | 180 | 2.221178% | 0 | 0.021336% |
| 1 | 172 | 2.138166% | 0 | 0.020899% |
| 2 | 164 | 2.095931% | 0 | 0.020462% |

Effective kernel RX and TX limits are each 524288 bytes for the old request
and 2097152 bytes for the larger request. Larger outer buffers prevent the
outer losses and large amplification in all three synthetic repeats. Small
application losses remain under the deliberate pause: the client application
socket records 261–282 drops in the larger-buffer cases. The result is not a
zero-loss guarantee under arbitrary pauses or real network loss.

Increasing this request adds 3MiB to the combined RX+TX kernel ceiling per
outer socket on this host. This is a ceiling, not eager allocation or process
RSS. A client may own multiple connection sockets, so any default change must
account for configured connection bounds and actual effective/clamped values.
The experiment does not yet establish that 1MiB is the smallest adequate
request, nor that the same setting fixes the full capacity-baseline failure.

## Next implementation/verification boundary

The supported candidate is more receive headroom on outer sockets, with matched
baseline configuration and explicit kernel-memory accounting. Before selecting
a production default, check the bounded memory tradeoff and short mixed-size,
capacity and resource comparisons. Extend baseline tooling to accept and
report the same setting if selected. Capture outer/application drop deltas and
scheduler runqueue-delay deltas in diagnostic runs to investigate the original
host-trigger mechanism. Increasing application residence to seconds would
change latency/memory semantics and is not supported by these experiments.

No thresholds were relaxed and no historical failure was relabelled. Full
T077 remains failed/invalid; final acceptance requires fresh matching evidence
after any selected runtime/configuration change. Aggregate machine-readable
evidence and hashes are in
`benchmarks/results/phase8-final-outer-burst-probe-20261016/analysis.json`.
