# ADR 0013: Bound the complete reference measurement cycle

Status: accepted prospectively by user instruction on 2026-10-14.

## Context

The original method requires 185 sequential pairs, each two 60s warmup/300s
measurement runs: 37 hours before calibration and overhead. No measured
variance or rare-failure evidence justifies that cost for every combination.
The user explicitly requested at most 60–90 minutes and accepted background
execution with a separate intermediate progress report. The long schema-2
r3 cycle was interrupted; its artifacts remain immutable historical evidence.

## Decision

Use schema 3 / bounded-reference-v1 with 18 baseline searches and 75 pairs.
Keep all 18 subcapacity combinations; pair capacity only for the six 1200-byte
SC-003 combinations. Repeat each three times, with seed-bound alternating
mode order and shuffled pair scheduling. Keep SC thresholds unchanged.

Calibration starts at 125 Mbit/s, doubles to a failing bracket or 1000 Mbit/s,
then bisects, at most six measured points. Use 2s warmup/8s measurement.
Select the highest tested valid <=0.1% loss point; do not infer exact capacity.
Pairs use 3s warmup/15s measurement; 32-session simultaneous resource pairs
measure 60s. Drain is 500ms. Preserve zero generator-error validity, matched
workload/topology/source/binary, per-run acceptance, spread and sample counts.
Churn remains 1000 cycles. Nominal interval cost is 4695 seconds; a 5400-second
wall budget with 30 seconds reserved for cleanup bounds the complete cycle.
Budget exhaustion stops owned workers and yields incomplete/invalid evidence.

## Consequences

These intervals provide less temporal coverage, shorter warmup and limited
repeat variance evidence. They are a deliberate budget/coverage tradeoff, not
proof of statistical equivalence to five-minute runs. Do not claim long-term
line-rate stability. Report baseline brackets and per-workload RTT probe counts.
Existing T076 functional duration evidence remains applicable to its recorded
profile, not transferred to high-rate throughput. A noisy/failing workload
needs a new declared follow-up within its own approved budget; no automatic
unbounded retry or retrospective threshold revision is allowed.

Schema-2 suite validation retains its original intervals and matrix. The
new validator requires explicit schema/method identity and rejects shortened,
missing, mixed or malformed coverage. ARM64 remains native-only and unqualified
here. Production code is unchanged. T079 remains outside this work.

Schema 3 requires >=100 same-sender RTT probes/session; schema 2 retains >=1000.
At 10Hz, 15-second measurements nominally give 150. The p95 tail is represented
by few observations and may be noisy; retain spread and fail any bad pair.
