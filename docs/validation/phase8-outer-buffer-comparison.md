# Outer-buffer and BBR2 focused comparison — 2026-10-16

Status: complete, CUBIC candidate not accepted by frozen regression checks;
T076/T077/T078 pending on the new candidate, T079 excluded.

All 48 runs completed in 2089 seconds (34m49s) with unchanged recorded inputs,
runner sources and executable hashes. Candidate qlog executable:
`a766bdb78cb54910ee1a7246b0077936b23430a9cf27627f541558f825633add`.
Frozen policy SHA-256:
`6512b1a220fc5c51de39d587e830572bdcdb71dd3cf72fc7d3a0c2c22fe71795`.
The 1MiB requested outer setting, explicit legacy baseline override, application
buffers and one outer socket/role were verified through actual kernel limits.
Kernel ceilings and observed queue peaks are retained separately from RSS.

## Method and acceptance scope

Three repetitions of 1×128 simultaneous, 1×512 simultaneous, 32×1200 capacity
and 32×1200 resource, under the same 1Gbit/s/direction, 20ms RTT, mmsg32,
CUBIC/BBR2, GSO/GRO-off topology. Each repetition has four modes: matched
two-leg UDP baseline, old 256KiB-outer CUBIC control, new 1MiB-outer CUBIC
candidate, and new 1MiB-outer BBR2 diagnostic. There is no injected loss/pause.
Algorithms are sender-side quiche configuration, with both roles matched.

CUBIC candidate checks retain the preceding numeric regression bounds and
strict small-packet delivery preservation, plus general SC thresholds and a
prospective <=0.1% capacity-loss guard. BBR2 checks were explicitly separate:
same general SC thresholds and kernel limits, without historical CUBIC numeric
CPU/RSS/RTT regression bounds. Thus "BBR2 12/12" and "CUBIC 10/12" are not the
same acceptance test. No failing result is relabelled or waived here.

## Results

| Workload, three repeats | CUBIC | BBR2 |
|---|---|---|
| 128-byte simultaneous | Zero loss both directions; one historical CPU-bound failure | Zero loss both directions; CPU 10.1–12.5% above adjacent CUBIC |
| 512-byte simultaneous | Maximum loss 0.002112% | Maximum forward loss 0.086727%; reverse <=0.000510% |
| 32×1200 capacity | Maximum reverse loss 0.019732%; one historical loss/delivery-bound failure | Zero loss both directions, 874.9056Mbit/s/direction in every repeat |
| 32×1200 resource | Zero loss, 500.736Mbit/s/direction in every repeat | Zero loss, 500.736Mbit/s/direction in every repeat |

All matched UDP baselines remained within the sustainable <=0.1% loss ceiling.
No generator/integrity validity failure is reported. Maximum role CPU is
0.914536 core for CUBIC and 0.913962 for BBR2; maximum role RSS is 13209600
and 12984320 bytes respectively. Maximum p95 RTT is 20.939/21.000ms.

## Two retained CUBIC failures

Capacity repeat 0 reverse delivery is 874.73296Mbit/s, below the historical
874.87344 floor; loss is 0.019732%, above the 0.003676% ceiling, while staying
within general SC bounds. Missing datagrams are 377 forward/1079 reverse.
Measured sampled drops include server outer 92 and client application 4.
Bracketing server reverse queue counters add 640 full and 256 expired drops.
These counters localize pressure but do not exactly decompose endpoint loss.
The larger outer buffer therefore did not prevent every outer overflow under
the full-rate workload; the exact receive-service stall/burst trigger remains
unproven. BBR2's adjacent capacity run had no drops/loss.

Small-packet repeat 2 server average CPU is 0.408422 core, above 0.402575;
owner CPU also exceeds its retained bound. Adjacent old control is 0.406689,
also above the old absolute bound. Candidate is approximately 0.43% above that
adjacent process CPU, while both exceed the historical threshold. Host or
measurement variation may contribute; the existing frozen result stays failed.

## BBR2 tradeoff

All 12 general SC checks pass for BBR2. Its small-packet role CPU is
0.428877–0.449558 core, versus CUBIC 0.384615–0.408422. Reapplying historical
CUBIC bounds diagnostically rejects the CPU fields in all three BBR2 small
rows; this is not a silently accepted CPU regression.

The first BBR2 512-byte run misses 1191 forward datagrams (0.086727%): sampled
client application kernel drops add 15, and bracketing client queue-full
counters add 1176. It passes the 0.1-point target with less margin than CUBIC
in this workload. Its cause was not profiled in this comparison. CUBIC's
corresponding missing count is 29 forward and 25 reverse.

These observations support further BBR2 evaluation, not an unconditional
algorithm-default switch. Capture receive/scheduler timing for the remaining
outer overflow, and separately profile BBR2's 512-byte queue pressure before
choosing a prospective performance tradeoff/profile. Historical CPU tolerances
cannot be widened retroactively. No new full qualification has been started.

## Evidence

- Coordinator: `benchmarks/results/phase8-outer-buffer-20261016/resume1/`.
- Raw rows and frozen evaluation: `comparison-r2/` in its parent directory.
- Derived, non-acceptance analysis: `comparison-r2/analysis.json`.
- Initial CLI-allowlist failure: parent `comparison/`; zero completed rows.
  The CLI fix, positive legacy-override check and regression are retained.

The candidate source/default remains implemented but unqualified; the prior
T076 pass remains historical evidence for its exact old binary only. No
source, profile, threshold, or raw result was changed to obtain this report.
