# Phase 8 implementation review corrections — 2026-10-17

The requested independent read-only implementation audit found two P2 issues:
a result-validator defect and a lifecycle fuzz coverage gap. It found no
confirmed production forwarding defect or P0/P1 in the reviewed delta. That is
an audit result for the inspected scope, not a proof that all runtime paths
are defect-free. Both reported issues are corrected below.

## Bounded-search stopping policy

`tools/traffic/src/measurement.rs` now accepts a valid early ceiling at the
end of a checked search of fewer than six points. Starting at 125 Mbit/s,
`125 → 250 → 500 → 1000` with sustainable delivery is a valid four-point search.
The previous one-point exception could never match the required starting rate.

The regression in `tools/traffic/tests/results.rs` includes matched paired
counts/rates and passes for bounded-reference-v1 and bounded-reference-v2.
Negative cases retain rejection of early stops below the ceiling, a rejected
1000 Mbit/s point, and extra steps after a successful ceiling. The six-step
search path and numerical acceptance thresholds are unchanged.

The saved selected T077 suite is re-evaluated by the corrected tool and remains
`passed`, with empty `invalid` and `failed` arrays. Its original 18 searches all
have six points; no saved manifest, measurement or historical verdict is edited.

## Lifecycle transition coverage

`fuzz/support/lifecycle.rs` is shared by a native deterministic regression and
the lifecycle libFuzzer target. It calls actual owner transition methods, with
native QUIC/TLS peers exchanging packets in memory. Expectations derive from
input operations rather than from the production latch or generation predicate:

- Current packets must enqueue; stale session and stale connection packets
  reach `schedule_datagrams` and must increment only the stale-drop counter.
- Successful stale setup callbacks reach `setup_done` with an otherwise
  authorizing session. Relay/identity accounting and session state stay intact.
  A current-key duplicate callback in the active state also cannot change it.
- Closing a flow discards queued application/native work, preserves committed
  ciphertext, and makes subsequent successful callbacks harmless.
- Scripted EAGAIN preserves the ciphertext FIFO byte-for-byte. Partial physical
  acceptance removes exactly the accepted prefix without replay or reordering.
- Connection replacement cancels old sessions; old successful callbacks are
  delivered after the new connection exists.
- Permanent mapping rejection blocks real loopback input from new senders and
  prevents a reconnect attempt after connection removal. Expected rejection is
  independent of `MappingLatch::may_setup()`.

A fixed prelude exercises these transitions even on empty input; at most 32
additional operations fuzz their ordering and repetition. The older small
queue/retry properties remain, now with an independent rejection assertion.
This does not fuzz the complete async owner loop, resolver, GOAWAY, Linux
scheduling or a WAN. Existing deterministic/process/interop tests retain their
separate scope. The expanded lifecycle target is slower than the helper-only
predecessor; iteration counts are not comparable across these versions.

The separate unpublished fuzz package includes the current production sources.
Its build adapter adjusts module/doc paths and enables precisely the five
existing native-test send-hook sites, checking each replacement occurs once.
It contains no copied owner algorithm and fails to build if those seams drift.
All adapter/support/source files are bound by the campaign input hashes.
New direct test dependencies reuse the exact production lock versions; the only
fuzz-only packages remain libfuzzer-sys, arbitrary, jobserver and the fuzz package.
No root feature, production API or runtime dependency changes were needed.

## Verification and evidence boundaries

Evidence directory:
`benchmarks/results/phase8-implementation-review-fixes-20261017/`.
The final orchestrator's commands, durations and exit codes are recorded in
`verification-progress.json`; hashes and concise outcomes are in `summary.json`.

- Root locked all-target/all-feature native suite: **183 passed**, two existing
  prerequisite-dependent tests ignored. Those independent/native checks were
  not newly executed under their missing prerequisites in this correction.
- Separate traffic package: **24 passed**, including the ceiling regression.
- Root and fuzz-adapter Clippy passed with warnings denied. Root, traffic and
  fuzz formatting, shell syntax and whitespace checks passed.
- Four ASan campaigns, 30 seconds each, seed 73013, passed with unchanged inputs:
  MASQUE **3,397,510**, policy **4,771,186**, ancillary **21,616,372**, lifecycle
  **963** executions. These are bounded smoke campaigns; scheduled longer CI
  remains a configured workflow, not new execution evidence.
- The corrected validator reports the saved T077 suite as `passed`.
- Every selected production input still matches T076. Rebuilding qlog release
  yields the identical qualified executable SHA-256:
  `7f7a3c4de7341b8edf0860ff3c8457642ff423cf4658f9aa93076db678cfe3b1`.

One earlier concurrent native run failed churn with `unexpected application
payload`; its two subsequent failures were poisoned-lock consequences. The
failed log is retained as `native-tests-final.log`. A sequential e2e repeat
passed 22 tests, and the later full native suite after ASan finished also passed.
The cause of that isolated first failure is **not established or fixed** by
these two corrections. Do not describe retries as a causal fix; if it recurs,
retain the offending payload and sender/local socket identities before drawing
an attribution. Early exploratory fuzz attempts whose input hashes changed
were rejected by the hash guard and are not the passing evidence. The final
accepted campaign is exclusively `fuzz-complete/`.

T076/T077/T078 remain complete for the identical selected native x86_64
executable and its measured profile. No new VM or long performance run was
needed or claimed. Native ARM64 remains unqualified; T079 remains excluded.
This is the author's correction/verification record, not a second independent
review of the fixes. Nothing was committed, pushed, published or deployed.
