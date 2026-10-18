# Final independent review corrections — 2026-10-18

Scope: the two P2 findings from the independent audit of snapshot
`34390cb297c63a73830cbd26780f6011bf79fb8e`.
See [ADR 0027](../adr/0027-finite-controls-and-interrupted-syscalls.md).

## Corrections and regressions

Commitment controls retain their four-session quota and generation-fair cursor,
but complete a finite sweep regardless of how many inspected receipts are
ready. Native input, due transport timers and response setup rearm a completed
sweep. Held receipts alone do not rearm it. The regression creates 13 real H3
streams, loses the first 12 responses and ACKs the last. It runs H3/control,
output, enqueue and final continuation stages until idle, requiring activation
of the last stream before parking. It also delivers delayed/retransmitted data
and routes real ACKs through the production outer-packet boundary to prove that
new input rearms the sweep. It checks finite completion and single-counted
activation. The pre-fix regression fails with AwaitingCommitment instead of
Active; the corrected regression passes.

Receive/send/timerfd read share a retry policy limited to four attempts per
adapter visit. Exhaustion returns Interrupted. Success (including a partial
prefix) and non-EINTR errors return immediately. Regressions cover persistent
interruption with a guard against hanging CI, success on the final attempt,
accepted-prefix non-replay, per-visit budget renewal, EAGAIN and hard errors.
The persistent-interruption regression fails before the limit is added and
passes afterward. Native syscall arguments and unsafe blocks are unchanged.

## Verification

Raw logs and private generated inputs are retained
locally under `benchmarks/results/final-review-p2-20261018/`; generated inputs,
binaries and raw logs are not publication artifacts.

Formatting, warnings-denied all-target/all-feature Clippy, locked default/qlog
release builds and the pinned quiche input verifier pass. The native suite
passes **203 tests**; **100 Python tests** pass. The two ordinary ignored
independent-peer prerequisites are also executed explicitly: the **29-case
protocol matrix** and the held-response SIGTERM/SIGINT shutdown case pass.

The first independent-peer attempt used an old `/tmp` executable that lacked
both `hold-response` and `request_pending` markers and failed before holding
setup. That failure remains in `independent-peer.log`. A freshly built current
Go fixture using the pinned cached graph passes both explicit tests; its source
and binary identities are retained by the independent manifest. No production
change or relaxed assertion was made to obtain the pass.

Persistent EINTR injection in both `recvmsg` and `recvmmsg` now permits clean
exit code 0 on both SIGTERM and SIGINT. The four single/mmsg and signal cases
finish in 0.00721–0.00723 seconds after signaling. The shared retry-policy tests
cover send/timerfd policy; native send/timerfd fault injection is not claimed.
The C injection shim and its binary are retained with the private run artifacts.
Each terminal snapshot confirms 438,240–469,360 observed receive EINTR errors,
the same number of receive attempts, zero received datagrams and a count
divisible by four; the experiment did not silently run without injection.

Four 30-second ASan smoke campaigns pass (31 seconds each): MASQUE 3,640,220
executions, policy 4,650,674, ancillary 22,953,239 and lifecycle 995. Source/input
checks remain passing after every campaign. These are bounded smoke campaigns,
not exhaustive safety proof. No new Miri or advisory audit is claimed.

The [original prospective policy](../../benchmarks/policies/final-review-p2.json)
was frozen before implementation and candidate measurement. It keeps the
existing three alternating direct/control/candidate repetitions, small-packet
and 32-session resource workloads, and every per-repeat threshold from the
earlier endpoint comparison. The pre-change control and input hashes are
retained; the run manifest binds candidate inputs and checks for drift.

The first comparison preflight rejected a stale inherited reference-runner
hash before any measurement. The original policy and failed launch are retained.
The [metadata-corrected policy](../../benchmarks/policies/final-review-p2-v2.json)
binds the current runner, which matches the unchanged pre-fix HEAD, and a new
driver/output identity. All workloads and numerical bounds are identical to
the original pre-implementation policy.

The corrected comparison passes all **18 members / 6 per-repeat checks** in
**780.39 seconds**, within the 1800-second measurement budget. Every candidate
resource repeat delivers **500.736 Mbit/s per direction**. All six candidate
members have zero measured endpoint loss. Maximum candidate process CPU is
0.920168 core, owner CPU 0.918999 core, and per-role RSS 12,775,424 bytes
(12.184 MiB). The third small-packet direct baseline has 0.004832%/0.003290%
loss, inside its unchanged 0.1% validity limit; no uniform lossless-baseline
claim is made. Every delivery/loss/RTT/CPU/RSS comparison passes separately.
No failed member is averaged away or numerical threshold relaxed.

The final summary checks both executable identities, current production and
runner inputs against the run manifest, and all regression/native/independent/
ASan results. Sources remain unchanged throughout collection. The bound local
record is `benchmarks/results/final-review-p2-20261018/verification-summary.json`;
the complete comparison and driver are in its `comparison-v2/` subdirectory.

Pre-change qlog control SHA-256:
`5ea715ea21800945a0d4ca9a4880e3aa46a9cc212a2a62c9d6a24482acf85c3f`.
Candidate qlog SHA-256:
`4dc83ee4acbf3234a49fd43bbe2479a094f573be200dbbe3bf40ada7c4ad214d`.

No full release, VM/WireGuard, remote deployment, WAN or ARM64 qualification is
part of this correction. Prior evidence keeps its original source/executable
provenance. No commit or publication is performed by this work.
