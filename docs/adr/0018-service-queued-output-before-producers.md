# ADR 0018: Service queued output before refilling application queues

Status: implemented; native/E2E checks and final combined focused comparison pass.
The no-buffer-change comparison remains unsuccessful.
Date: 2026-10-16.

## Evidence and options

ADR 0017's bounded density history passes its native contract tests and the
first focused small/capacity/resource comparison. The next small-packet repeat
loses six measured messages while its adjacent control loses none, exceeding
the prospectively frozen two-message floor. That comparison remains failed.
All six losses appear as client session-queue-full drops during the measured
interval. There is no kernel overflow or outer QUIC loss, and client native
enqueues equal server UDP egress cumulatively. This localizes the remaining
loss stage; it does not establish the exact native-credit state at that instant.

The existing stage order reads client application input before servicing the
connection's native output and queued application packets. A controlled native
regression fills the 64-slot native queue and 32-slot session queue, supplies
six new real UDP messages, and gives output an opportunity to progress. The old
production visit order drops all six before servicing output in the same round.
The proposed order delivers all 102 exact payloads FIFO without loss/duplicates.

Increasing queues/deadlines or accepting the failed comparison is not selected.
Global producer backpressure could couple independent flows. The user approved
coordinating receive/queue service as the next approach if bounded history alone
does not satisfy comparison. First try reordering the existing bounded stages;
more extensive interleaved read/drain machinery is not required by this test.

## Decision and invariants

Build the same stable owner visit ring, ordered as follows:

1. Receive outer packets, including transport credit.
2. Process HTTP/3, session control and close/cancellation.
3. Generate/submit already queued native output.
4. Enqueue already admitted application packets into available native capacity.
5. Read client listener and server relay producers.

Existing quotas remain: eight visits between timer/cancellation checks,
32-item data stages and the shared 16-generation/16-submission output stage.
No extra service call is inserted when a producer queue is full. Ring membership
remains independent of packet arrival/drain; per-connection/session fairness,
readiness continuation, native partial progress and physical pacing are unchanged.
New application input may wait until the next ring cycle before native generation.
No queue size, resource reservation or packet expiry is extended.

Author security/ownership assessment: HTTP/3 close/authentication handling stays
before native generation; all enqueue/admission and generation checks remain in
their original methods. No unadmitted flow contributes to receive density.
There is no global blocked-flow gate, unsafe code, new task/lock/dependency,
wire/config/CLI change, or per-packet scheduling task. Cancellation still has a
fixed visit bound. Existing security, lifecycle, pacing, syscall, fairness and
shutdown tests must pass on the final candidate. Independent review was not
requested and is not claimed.

The first reordered E2E attempt exposes an additional continuation requirement:
application queues can become empty after enqueue, while their newly queued
native data still needs an output visit in the next ring cycle. Per-connection
enqueue/output continuation bits retain this work across eight-visit window
boundaries. Queue insertion requests enqueue service; native enqueue requests
output service. Each corresponding attempt consumes its bit, even when output
is congestion-blocked, so it does not create an unbounded empty-round loop.
Quota-limited remaining application work requests another enqueue visit.
Existing credit/readiness/expiry wakeups remain responsible for blocked work.
The regression also asserts finite continuation and no empty-round request after
all 102 payloads have been generated. The failed isolation attempt is retained.

## Comparison

Preserve the failed history32-only comparison and both native regression logs.
Use fresh matched comparisons for the combined candidate. The new prospective
policy retains every numeric bound and reference hash from the previous policy;
only candidate scope/provenance changes. Continue to reject any failed repeat,
including the two-message small-packet gate. Full T076/T077/T078 qualification
remains pending; T079 remains excluded.

The completed no-buffer-change comparison passes six candidate checks, then
stops on the third small row: 13 forward messages lost, matching 13 client
application-socket kernel drops. No measured software queue-full increase or
outer QUIC loss explains that row. ADR 0019 adds the prospectively reviewed
kernel-buffer headroom. Its final combined candidate passes all nine focused
checks without changing the preceding numeric regression bounds.
