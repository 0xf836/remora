# ADR 0025: Stop native application enqueue behind a full ciphertext FIFO

Status: implemented; affected checks and frozen CUBIC comparison pass.
Date: 2026-10-17.

## Context

The reactor design in `specs/001-masque-udp-tunnel/plan.md`, turn step 6,
requires a full ciphertext queue to stop both packet generation and application
enqueueing. The existing generation loop enforces the 32-record output bound,
but application enqueue can still consume spare native DATAGRAM credit behind
that full FIFO. T081 closes this partial implementation of the existing plan.
It does not attribute earlier measured losses or the isolated churn integrity
failure to this behavior.

## Options and decision

Keeping the bounded native queue writable would preserve a finite memory bound,
but would not satisfy the planned backpressure boundary. Global producer
backpressure would couple independent connections. Increasing queues or expiry
deadlines would change the resource/lifetime contract without addressing the
missing gate.

Reject application-to-native admission with the existing retryable Capacity
outcome while the connection's ciphertext FIFO has 32 records. Validate expiry
before returning capacity so an expired application descriptor is still
discarded and counted. The owner retains a current blocked descriptor in its
session queue, preserving its generation, received time and expiry; an
unsuccessful admission does not advance the fairness cursor. Full-output
capacity does not request an immediate enqueue continuation: output readiness,
pacing, transport progress and original application deadlines wake the owner.
The gate is per connection; control/timer visits and other connections continue.
Existing draining of eligible committed output before generation is unchanged.

## Consequences and verification

No wire/configuration/dependency/unsafe-code or queue-size change is introduced.
No secret, authentication, target authorization or UDP provenance rule changes.
Application bytes behind a blocked output queue remain cancellable until actual
packet generation; their deadlines cannot be refreshed by a failed enqueue.
Existing connection-level output timeout and shutdown rules still apply.

Native component regressions must cover spare native capacity behind a full
ciphertext FIFO, future pacing and EAGAIN, exact accepted-prefix progress,
resumption/fairness, expiry and cancellation, and an independent connection.
Applicable lifecycle/security/process checks and a prospective frozen CUBIC
comparison are required. The previously selected T076/T077 executable and its
evidence remain separate from this changed executable. Focused regression
results do not constitute a new full release qualification.

The [Phase 9 record](../validation/phase9-convergence.md) retains the two
failing-before/passing-after regressions, 187 passing native tests, both
independent-peer checks, 100 Python tests and four ASan smoke campaigns. The
36-member focused comparison completes all 12 per-repeat checks in 32m46s under
its 45-minute budget. No retrospective tolerance change or CPU exception is
needed. The new qlog executable is
`2e2b923c0ae7e56f842f215fbe44801facadf637249cbb7cc3abd719b3cca231`;
the earlier full release evidence remains bound to the previous executable.
