# ADR 0022: Drain eligible committed output before native generation

Status: implemented; increment checks pass; focused comparison completed but failed.
Date: 2026-10-16.

## Context and evidence

ADR 0018 orders output service ahead of application enqueue and ingress. Within
that output visit, however, generation precedes physical submission. A full
32-record ciphertext FIFO therefore prevents generation from freeing native
DATAGRAM credit, even when its front is already eligible for physical output.
The subsequent flush frees ciphertext slots too late for that visit's generation.
Fresh application input can then encounter full native and application queues.

Two separately instrumented BBR2 1x512-byte, 91552pps/direction, 60-second
diagnostic runs observe queue-full events with 64 native packets, 32 application
packets, 24–31 committed ciphertexts and an immediately eligible FIFO front.
The diagnostic executable and metadata are retained under
`benchmarks/results/phase8-service-timing-probe-20261016/`; its instrumented
timings are not comparative acceptance evidence. Normal uninstrumented repeats
are retained under `phase8-scheduler-loss-20261016/`. Their accumulated scheduler
wait per sample does not identify the duration of an individual scheduling pause.

A native regression fills all three queues, injects six real UDP messages and
captures the physical-send boundary. Generate-before-flush drops all six;
drain-before-generate admits all six with zero queue-full drops. This proves
the stage-order defect, without proving that it explains the earlier rare
92-packet server outer-socket overflow or every measured loss.

## Options and decision

Keep the same single owner, queue sizes, expiries and visit ring. Before native
generation, submit the already committed eligible FIFO prefix. Then generate
up to 16 native packets into the available ciphertext slots. Submit generated
output only using the remainder of the same 16-descriptor submission budget.
Count descriptors offered, including unaccepted suffixes, rather than successful
sends; partial sends, EAGAIN, EINTR and other errors cannot enlarge the budget.
The visit can make two bounded physical submission calls with at most 16
descriptors in total. FIFO identity, accepted-prefix removal and pacing apply
to both calls.

Before the initial submission, run the existing native expiry/PMTU/setup/output
deadline preparation with a generation quota of zero. This prevents the new
early flush from submitting a record that the previous generation step would
have rejected or pruned. Ordinary generation still consumes the output
continuation bit, including when congestion prevents progress.

Retaining the old order leaves a reproduced defect. Raising queue sizes or
residence deadlines, adding per-packet tasks or changing congestion control
does not address this ordering error and is not selected here. CUBIC remains
the default; BBR2 is evaluated separately on both old and new executables.

## Consequences and verification

No wire format, configuration, authentication, authorization, resource limit,
UDP provenance rule, cancellation boundary or unsafe code changes. The new
helper is private to the reactor. Transport and socket failures retain their
previous diagnostic classes and codes. Existing close/control service remains
ahead of output. Independent security review was not requested or claimed.

Regression coverage includes the failing-before six-message case, shared
submission budgets across partial progress and socket pressure, immutable FIFO
prefix progress, future pacing and expired output. Existing syscall, lifecycle,
security and process suites must pass. Fresh comparisons retain the preceding
CUBIC numerical bounds, use the same 1MiB requested outer buffer on both
executables and collect paired BBR2 results with prospectively specified bounds.
No historical failed comparison is relabelled. Performance and final
T076/T077/T078 acceptance remain pending; T079 remains excluded.

Locked default/qlog builds, formatting, Clippy, 182 ordinary Rust tests, the
independent-peer process test, 23 traffic-tool tests and 95 Python tests pass.
The qlog release executable is
`7f7a3c4de7341b8edf0860ff3c8457642ff423cf4658f9aa93076db678cfe3b1`.
Source snapshots, failing-before/passing-after regression records and exact
check logs are retained under
`benchmarks/results/phase8-output-order-fix-20261016/`. This increment does not
reuse the older executable's long functional acceptance.

The focused cycle completes all 60 rows in 54m44s. CUBIC passes 9/12 frozen
checks; all three small-packet repeats fail historical process/owner CPU bounds
on both roles. Other CUBIC workload checks pass. Adjacent old CUBIC also exceeds
the historical small CPU bounds; the new version adds 0.04–4.98% process CPU
relative to that control. Neither historical failure nor incremental cost is
waived. Medium endpoint losses sum to 413 instead of 50720 across the three
matched old runs and both directions, but individual directions need not improve.

BBR2 passes 9/12 of its separate checks. Two medium repeats regress delivery
against old BBR2, and one resource repeat fails the full-lifetime socket-count
guard. Its only excess socket observation is the first sample, 4965ms before
measurement; the measured minute has the expected count. The socket identity
was not retained, so this is not proof of either a leak or a harmless cause.
All new resource measurements deliver 500.736Mbit/s/direction without measured
endpoint loss. One old BBR2 resource control has substantial loss and queue
expiry; it must not be treated as a healthy CPU comparator or evidence of a
universally resolved old failure. See the completed evidence report.
