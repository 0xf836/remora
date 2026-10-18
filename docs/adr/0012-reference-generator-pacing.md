# ADR 0012: Explicit micro-batch pacing for reference generators

Status: accepted for prospective measurements; numerical acceptance pending.
Date: 2026-10-14.

## Context

The original reference calibration failed generator validity with 48 omitted
slots. Removing idle sleeps when work was due and adding a finite 2ms end guard
improved targeted trials, but the next complete cycle still omitted two slots
at 512 bytes / reverse / 510 Mbit/s. Both invalid cycles are retained. An
additional eight direct 124511pps reverse trials reproduced a 259-slot omission
in one trial. Scheduler/timer variability prevents an individual final slot
with an approximately 8us deadline margin from being reliably serviced.

## Decision

Use an explicit 100us pacing quantum in both native reference profiles. The
scheduled offset is floor(floor(tick * 1e9 / rate) / 100000) * 100000 ns.
All slots in a quantum become eligible at its beginning. Each service visit
remains limited to 32 slots; overdue work is serviced immediately, and the
finite 2ms end guard remains. Every visit checks the measurement cutoff.
Whole-second warmup and measurement boundaries preserve exact declared counts;
no slot is sent after the cutoff to conceal an omission. Unscheduled, excess,
enqueue and warmup errors remain invalidating evidence.

The CLI default remains zero quantum for existing functional profiles. The
reference controller passes 100 explicitly and rejects missing or different
profile values. Raw generator JSON records the quantum, while manifests bind
profile, source and executable hashes. Baseline and tunnel use identical pacing.

## Consequences

This changes the input arrival pattern to bounded micro-batches and requires
fresh calibration and all fresh paired measurements. Earlier cycles cannot be
relabelled or combined with these results. This is test-tool behavior, not a
production optimization, and no SC threshold is relaxed. It reduces the final
slot deadline risk but cannot guarantee validity under arbitrary preemption;
any remaining omitted slot still stops qualification. Late catch-up, ignoring
errors, or altering packet accounting are rejected alternatives.


## Receive service deadline correction — 2026-10-14

The first bounded schema-3 cycle still rejected simultaneous/128-byte/218.75
Mbit/s calibration with 174 omitted sender slots; generator CPU was 0.245 core,
clock/integrity/coverage checks passed. A four-trial direct simultaneous repeat
reproduced 19 omitted sender slots once. The source allowed an entire 32-packet
receive visit without checking an outgoing slot that became eligible. The
correction checks the send deadline between receive packets and yields when
output is due; drain-only/finished generation still receives normally. Slot
quantum, counts, cutoff, per-visit ceiling and error gates stay unchanged.

Four matching direct candidate trials each submit 427246 messages per direction
with zero unscheduled/enqueue/warmup failures. This is targeted regression
support, not proof against arbitrary OS preemption or release performance.
New full cycles bind fresh source/executable identities; failures remain stored.
