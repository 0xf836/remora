# ADR 0014: Explicit completion reserve for reference generators

Status: accepted after targeted native verification; numerical release acceptance pending.
Date: 2026-10-14.

## Context

The second bounded cycle passed 11 workloads/67 points before 32-session,
128-byte simultaneous calibration at 125 Mbit/s failed with nine missing
sender slots. All submitted packets arrived; CPU/clock/integrity were valid.
The one-session receive deadline regression did not qualify 32 independent
sender processes. Four exact native namespace diagnostic repeats reproduced
errors 0/4/36/0; measured final service-visit gaps reached 2070us, while missing
senders last submitted 598–1397us before the cutoff. The 2ms busy-spin guard
cannot guarantee scheduling of 33 traffic processes across 16 logical tool CPUs.

## Decision

Prospectively test bounded-reference-v2 / schema 4 with an explicit 20ms
completion reserve. Preserve declared interval/counts, but uniformly schedule
measurement slots within interval minus 20ms, leaving time before the cutoff.
Warmup scheduling stays unchanged. Quantize to the existing 100us groups;
never send missing slots after the cutoff. Check cutoff between reverse peers
as well as between slots. Missing/excess/enqueue/warmup slots still invalidate.

For measured slot index k, rate r and duration S, its offset after warmup is
floor(k * (S*1e9 - 20e6) / (r*S)), quantized down to 100us. The first index
outside r*S gets offset S*1e9, never another eligible slot. Use u128 for the
intermediate product within existing bounded CLI limits. Active instantaneous
rate increases by S/(S-.020): 0.2506% at 8s, 0.1335% at 15s, 0.0334% at 60s.
The average declared interval load and total count are unchanged. Both baseline
and tunnel use the identical waveform; manifests/raw endpoints bind guard=20.

The CLI default reserve is zero for existing functional commands. Schema-2/3
archived suite validators retain their original identities; schema 4 requires
bounded-reference-v2 and explicit guard 20. No old failure is relabelled.
The existing 90-minute wall budget and numerical SC thresholds stay unchanged.

## Verification and limits

Do not start another complete cycle before targeted 32-session simultaneous
namespace trials cover calibration (8s), paired measurement (15s) and resource
(60s) intervals. Keep failures and instrumentation. Twenty milliseconds is a
prospective margin against the measured ~2ms delays, not a hard scheduler
latency guarantee. Larger unexpected delay still invalidates evidence; no
automatic unbounded retry or silent margin growth is permitted. Short-interval
confidence limitations from ADR 0013 remain. Production code is unchanged.


## Verification result

All nine native namespace probes pass generator/integrity/clock/coverage
validity: four 32-session/128-byte simultaneous 8s trials, four 15s trials,
and one 32-session/1200-byte simultaneous 60s resource trial. No missing slots;
generator CPU 0.236–0.290 core. The resource baseline delivers 500.4288 Mbit/s
in each direction with zero measured loss. Last submissions retain approximately
20ms margin. These are generator/fixture qualification checks, not candidate
Remora numerical acceptance. A fresh full cycle may now use the frozen method.
