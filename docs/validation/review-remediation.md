# Phase 3 independent review remediation

Raw run data referenced by this report is retained in ignored local archives;
see [artifact retention](README.md).

Date: 2026-10-11. Working tree based on
`503a83c374ac8b885f4d5e16bb26dff7c26b5a9e`; no commit is created by this work.
This is the implementer's response to the explicitly requested read-only
code/design/documentation/test audit. It does not alter the review's conclusions
or certify its excluded archived measurement results. A repeated independent
review has not been requested or performed.

## Findings and corrections

| Finding | Correction and regression evidence |
|---|---|
| 1. Undrained path events | Bounded path-event drain on receive and owner timer work. A real quiche test validates two paths, switches source 100 times and asserts the event queue is empty; it failed on retained PeerMigrated events before the fix. |
| 2. Wildcard reply source | Immutable local destination joins the client flow key and selects both immediate and queued reply source. Connected applications reach 127.0.0.2 and 127.0.0.3 through a wildcard listener; the process test timed out before the fix. A separate actual Runner/UDP test exercises queued reply delivery from both addresses. |
| 3. HTTP/3 semantics | Enforce request-only TE/trailers, Host/authority consistency and legal field-value bytes. Semantic checks precede configured-route refusal; missing/empty authority with Host, conflicting foreign authority and duplicate Host are covered. Positive/negative parser tests and real decoded H3 request/response exchanges cover the checks. The parser test accepted malformed input before the fix. |
| 4. Terminal status at shutdown | Cadence-independent bounded final drain waits for old ACKs and publishes newer terminal entries. A process with a ten-second snapshot interval collects a short-lived authentication failure; restoring the old shutdown block makes that test fail. A blocked sink produces DiagnosticCollectionIncomplete at a bounded deadline. |
| 5. Foreign live attachments | Inspect current/next-boot persistent XML and current transient XML before shutdown/deletion. Scripted libvirt tests cover live-only network/disk attachments, transient guests and the no-mutation failure boundary; they failed against the prior implementation. |
| 6. Unrecorded volume creation | Persist intent before create and ownership before upload. Create/key-query errors and KeyboardInterrupt retain recoverable intent; unresolved objects block cleanup and cannot produce a destroyed manifest. Upload-interruption and operator-resolution cases cover safe cleanup. These cases failed before the fix. |
| 7. Incomplete comparison coverage | Validate the exact pair/mode/direction set plus every policy workload, mandatory metric, unit and finite bound. Synthetic tests cover duplicate/missing workloads, invalid pair types/ranges and incomplete/nonfinite policies; malformed cases failed before the fix. |
| 8. Receive-window mismatch | Restore both initial/max connection windows to the contracted 256 KiB. A real-handshake transport-parameter regression observed 1 MiB before the fix and now asserts 256 KiB; stream windows remain 16 KiB. ADR-0007 explains the choice. |
| 9. Imitation partial-send test | Replace the queue imitation with an actual Runner flush test using real mixed ciphertext from two streams. Exercise pacing, stream close, exact partial prefix, EAGAIN, EINTR, hard error, immutable metadata and remaining-stream peer delivery. Removing the production EINTR branch makes it fail. Another owner-timer test expires native input with withheld ACKs/blocking cwnd; existing owner tests reject late setup callbacks. |

The controlled pre-fix/mutation checks restored source automatically and ran
only the affected regression. They are correctness evidence, not kernel-induced
EINTR or performance observations. Linux syscall tests still cover the real
adapter separately. The flush syscall seam exists only under `cfg(test)`.

## Contract and scope assessment

[ADR-0007](../adr/0007-review-corrections-and-contract-alignment.md) records the
window restoration, wildcard association change, HTTP/3 rejection semantics,
shutdown exception and operator recovery guarantees. Wire/config/diagnostic/
benchmark contracts, the data model, research and operator guide are synchronized.
No dependency, feature flag, public config field, secret-bearing diagnostic or
unbounded history/queue was introduced. Authentication and target authorization
still precede relay allocation. UDP provenance limits and numeric release
acceptance criteria remain intact.

T040 records body/Capsule rotation and bounded progress for at least five
continuously pending streams, together with timer/cancellation/control progress.
Phase 4 and full release qualifications remain unchecked. The earlier T037
1800-second evidence identifies its own binary; it cannot qualify this corrected
binary. T076 requires final-revision qualification. These fixes do not impose
a repeated VM run for every incremental change.

## Verification

`CARGO_NET_OFFLINE=true ./scripts/validate/mvp.sh` passed on native Linux
x86_64 and in the existing pinned container
`sha256:6da6665e07eebe6948170c91b16eba3594b0695e704265b0f038ffe5a9be41f7`.
The container ran offline as an unprivileged UID with a read-only populated
registry and separate `target/container`. Both checked locked release builds,
formatting, Clippy with warnings denied, **100 ordinary root-package tests**
(21 library, 6 process, 15 I/O, 5 lifecycle, 19 protocol, 34 security),
**2 traffic tests** and **28 Python tests** (5 comparison, 2 WG traffic,
21 libvirt safety). The formerly imitative lifecycle test is removed from that
count; production owner regressions are in the library count. Root and traffic
lockfiles were not changed.

The independent Go matrix passed
**29 cases** across both roles with the existing pinned peer. The Rust suite's
one ordinarily ignored interoperability test was explicitly invoked with
`REMORA_INTEROP_BIN=/tmp/remora-interop` and also passed. Peer/input hashes are
in the retained artifacts; private credentials remain outside Git.

Release binary SHA-256:
`767d143ca6e32f62f28da0bee3d6cc8446e8ace4033c6a915e699f945bd86a99`.
The fresh pre-change reference and prospective comparison policy are linked
from [performance](../performance.md); measured short comparison outcomes are
recorded there separately from this correctness audit response.

The short comparison
failed CPU bounds in both simultaneous tunnel runs and one baseline run:
9/12 runs passed. The first intermediate correction candidate passed, but its
pass cannot be assigned to the final binary. Both the prospective policy and
its failed outcome remain intact. The project owner later authorized a separate
MVP development policy with a 0.5-core ceiling per role for process and owner
averages; non-CPU bounds and release criteria are unchanged. Reassessment of
the same saved measurements passes 12/12 under this provisional policy, recorded
as a retrospective development exception rather than no-regression proof.
No new load runs or production optimizations were required; this CPU gate no
longer blocks development. Active directions delivered 3000/3000 packets per
run with no measured loss, integrity or generator errors. See
[the policy and separate assessment](../performance.md) for exact scope.

Native ARM64, hosted CI, final-duration VM qualification, full load/fairness,
kernel-induced interruption and exhaustive fuzz/control-stream campaigns are
outside these checks. No unavailable check is counted as passed.

## Results-audit negative-test corrections

The two medium findings from the independent results audit are addressed in
the test-only interoperability runner and Go fixture. Negative cases require
their exact failure class/code or HTTP status; denied-target fixtures authorize
a different port while requesting the actually monitored loopback endpoint in
both directions. The independent Go-client denial remains a CONNECT-UDP refusal
check, not a forbidden application-payload injection. Summaries now retain the
observed refusal and monitor endpoint. See [the evidence scope](interop.md#negative-assertions-after-the-results-audit).

No production code, archived result or checklist completion was changed.
Tests were not rerun at the project owner's request; existing matrix results
remain tied to their original inputs. T037's duration outcome remains intact.
Static checks passed: Python AST parsing, Go formatting, relative documentation
links and `git diff --check`. No build or runtime validation was performed.
