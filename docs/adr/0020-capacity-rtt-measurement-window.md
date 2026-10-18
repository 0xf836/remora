# ADR 0020: Extend the 32-session simultaneous capacity interval

Status: accepted prospectively; full numerical qualification pending.
Date: 2026-10-16.

## Context

The preceding full reference cycle and the first focused density comparison
retained invalid 32-session simultaneous capacity rows: the 15-second window
did not collect the required 100 RTT probes per session under overload.
The focused method then used 60 seconds for this workload, preserving the
sample requirement and every numerical acceptance threshold. The combined
density/order/buffer candidate passed three such focused capacity repetitions.
That focused evidence does not replace the complete T077 matrix.

## Decision

Before collecting new full reference evidence, set the explicit schema-4
profile/suite extension `capacity_32_simultaneous_seconds = 60`. Apply it to
both baseline and tunnel in all three capacity repetitions with 32 sessions
and simultaneous traffic. All other capacity/subcapacity intervals remain
15 seconds; resource intervals remain 60 seconds. Keep bounded-reference-v2,
the 20ms completion reserve, calibration policy, workloads, ordering seed,
three repetitions, SC thresholds and minimum probe counts unchanged.

The Python runner derives the common pair duration and serialized interval
from the same function. The Rust validator requires the explicit extension,
exactly 60 seconds for those rows, and the original intervals elsewhere.
Absent extension retains the strict historical 15-second interpretation;
schema 2/3 cannot declare the extension. Old failed/invalid results retain
their original identity and outcome.

The nominal interval plan increases from 4695 to 4965 seconds (82m45s): three
pairs times two members times 45 extra seconds = 270 seconds. The 5400-second
wall deadline and 30-second cleanup reserve remain enforced. This leaves
405 seconds for parsing, setup, teardown and churn outside nominal intervals.
The old complete cycle took about 80 minutes, making this extension plausible
within 90 minutes; actual exhaustion still produces incomplete/invalid evidence.
Extending all capacity rows would exceed the budget and is not selected.

## Verification and consequences

Python tests check exactly three extended pairs, matching architecture plans,
historical intervals, invalid extension values and the interval budget.
Rust regression tests reject undeclared/shortened/mixed-method intervals and
insufficient RTT probes, and preserve failure for subthreshold delivery.
The traffic generator, relay and responder source behavior is unchanged;
the traffic executable must nevertheless be rebuilt and its new hash retained.
The Remora production sources and measured candidate executable are unchanged.

This interval can expose sustained overload that a shorter run misses. It
cannot guarantee that every session supplies enough probes; missing samples,
generator failures and numerical threshold failures remain real blockers.
ARM64 receives the same written method but remains unqualified without native
execution. T077 must run after exact-input T076; T078 and T079 are separate.
