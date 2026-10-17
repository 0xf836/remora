# ADR 0017: Bound receive-density history to recent DATAGRAM packets

Status: implemented; combined correction passes focused comparison; full
qualification pending. History32-only failed comparison remains retained.
Date: 2026-10-16.

## Context and options

The Phase 8 loss research in `docs/validation/phase8-loss-research.md` reproduces
a permanent outer-read quota reduction after a coalesced short DATAGRAM burst.
The lifetime maximum can keep a large-packet connection reading only 4–10
outer packets per visit. A most-recent-only experiment improves large-packet
delivery but regresses small-packet loss. Increasing UDP buffers alone still
loses traffic. Neither experiment is accepted as the correction.

Options considered are a bounded recent maximum, asymmetric decay, the last
nonempty observation, and interleaving outer receive/native DATAGRAM service
under a common work budget. The user approved starting with bounded history,
retaining more extensive receive/service scheduling changes as a fallback.

## Candidate decision

Keep the maximum over the last 32 outer-packet observations that actually
increase the connection's native DATAGRAM receive queue. Store a fixed ring
of byte-sized observations, a cursor and a cached maximum in the exclusive
owner. Clamp each observation to 32: all densities at least 32 already select
the minimum one-packet read quota. No packet allocation or clock is needed.
Only expiration of a maximum can require a bounded 32-entry rescan.

Thirty-two observations are the first prospectively selected candidate, not
an asserted optimum. This retains evidence across a short sparse tail while
allowing a sustained change to one-DATAGRAM packets to recover after at most
32 nonempty observations. It shares the existing 32-item service scale without
changing any work quota. A different window needs fresh comparative evidence.

ACK-only packets, rejected input and receive-queue drains do not advance the
history. Repeated dense bursts inside the window retain a conservative quota.
State starts empty with an effective density of one, belongs to the connection
generation, and is discarded on connection retirement. Only admitted active
sessions contribute to the most conservative estimate on a shared socket.

The estimate remains a heuristic, not a bound on a future packet's frame count.
An unexpected burst can still fill the bounded native queue. Existing queue
capacities, overflow behavior, authentication, authorization, session generation,
expiry, pacing, partial-send, control/timer and cancellation rules remain intact.
No unsafe code, dependency, wire/config/CLI change, task or shared lock is added.

## Verification and evidence boundaries

Add native coalesced-short-to-large and large-to-short transitions, ACK-only
retention, repeating short bursts and shared-socket isolation tests, plus bounded
history/model properties. Demonstrate the transition failure on the archived
pre-change source before accepting the regression. Existing native readiness,
fairness, backpressure, lifetime, parser and lifecycle checks remain required.

Before candidate measurements, collect a fresh reference on the archived
Git-source binary, then freeze per-metric bounds from its extrema/spread and
declared measurement floors. Alternate control/candidate order with a matching
two-leg UDP baseline. Use three repetitions of small simultaneous (15 s), large
capacity simultaneous (60 s), and 32-session resource (60 s), with three-second
loaded warmup and tools-only collector affinity. Check both directions, loss,
delivery, RTT, per-role CPU/owner CPU/RSS, diagnostics and generator validity.
No average hides a failed repeat. Reject a failed comparison; retain all evidence.

The original prospective method used 15-second capacity runs. Its second
reference row on the old binary loses 33.28% and collects only 67 RTT samples
per session, below the unchanged 100 minimum. Preserve that row as insufficient
RTT evidence. Before any candidate measurement, method v2 increases only the
capacity interval to 60 seconds and requires three fresh capacity references.
All original small/resource reference rows remain in scope; no repetition is
selected away. No numeric threshold is relaxed.

This focused matrix is not full T076/T077 qualification. Production source
changes reopen source-bound verification; T076/T077/T078 remain pending until
their full required evidence exists. T079 remains excluded. No commit or push
is implied by the implementation authorization.
