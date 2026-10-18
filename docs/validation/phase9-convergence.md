# Phase 9 convergence — 2026-10-17

Scope: T080 and T081 only, native Linux GNU x86_64. Deferred ARM64/T072 and
future CUBIC/density experiments are outside this increment. This is an
implementation/affected-verification record, not a replacement full release
qualification.

## T080: failure evidence and bounded investigation

See the [investigation and artifact contract](phase8-implementation-review-fixes.md#t080-follow-up--2026-10-17).
The original isolated churn integrity failure remains unexplained. The successful
bounded reproduction campaign is evidence of those passing runs, not a causal
fix. New unexpected/integrity/cross-delivery failures retain private, bounded
packet metadata and sanitized JSON results outside credential fixture cleanup.
Raw payload, config, credential/key files and raw role logs are excluded. An
intentionally poisoned case lock no longer creates unrelated secondary failures.

Local evidence is under `benchmarks/results/phase9-convergence-20261017/`:

- `investigation-progress.json` / `investigation-summary.json`: failed initial
  fuzz startup, with three successful native runs; PATH prerequisite missing.
- `investigation-retry/investigation-summary.json`: corrected environment,
  144.2-second campaign under a 240-second deadline, three native suites,
  concurrent four-target 30-second ASan smoke and two Python regressions pass.
- Each campaign retains its driver, source hashes, commands and raw check logs.

For a failed ordinary Rust concurrency case, the panic prints
`target/test-failures/remora-test-…/`. Its mode-0700 directory contains only
mode-0600 `manifest.json`, `summary.json` and/or `failure.json` that existed when
cleanup started; each source JSON read is limited to 1 MiB. The original fixture
is removed. Treat artifacts as private operator evidence and review before
publication. The standalone Python runner keeps its own private output directory.

## T081: connection-local output backpressure

[ADR 0025](../adr/0025-connection-local-output-backpressure.md) describes the
alignment with reactor plan step 6. A full 32-record committed ciphertext FIFO
now refuses further application-to-native admission on that connection. The
application packet remains cancellable in its session queue with its original
generation, receive time and expiry. Expiry validation precedes the capacity
gate; a blocked enqueue does not advance the fairness cursor or request a busy
immediate continuation. Timer/control visits and independent connections remain
serviceable. Output draining, pacing, queue limits and wire/configuration remain
unchanged.

Both new native regressions fail on the preceding production implementation:
one receives `Ok(())` instead of Capacity, the other observes two unexpected
native enqueues behind full output. Both pass with the gate. The regressions
exercise spare native capacity, full output under EAGAIN/future pacing, unchanged
packet metadata, accepted-prefix drain, resumption of both flows, original native
expiry, cancellation with committed output retained, and another connection's
progress. Logs: `backpressure-before.log` and `backpressure-after.log`; the
separate initial compile failure is not a failing-before behavioral result.

## Affected verification and prospective performance comparison

Implementation verification is recorded in `verification-progress.json` and
`verification-summary.json`, with explicit command exit codes and source hashes.
The independent hold-response shutdown case and the independent both-role
protocol matrix have separate logs/results. Miri/syscall/dependency audits and
the full VM/reference release matrices are not re-executed for this increment.
No dependency, unsafe syscall implementation or WireGuard-specific code changes.

The focused comparison uses the existing owned-namespace/reference runner:
20 ms RTT, 1 Gbit/s, CUBIC, mmsg32, offloads off, matched socket defaults, disjoint
role/tool CPU placement and three-second warmup. Each of three repetitions has
direct UDP and adjacent control/candidate members; tunnel order alternates.
Workloads are simultaneous small (1 x 128 bytes, 64849 pps, 15 seconds), medium
(1 x 512, 91552 pps, 60 seconds), capacity (32 x 1200, 91136 pps, 60 seconds), and
resource (32 x 1200, 52160 pps, 60 seconds). Total input is per direction across
the declared sessions; the resource workload is 500.736 Mbit/s each way.

The prospective [policy](../../benchmarks/policies/phase9-backpressure.json)
binds exact binaries, production-input digest, driver/runner/profile/tool hashes,
and a 45-minute wall budget before collection. Every repeat independently checks
valid generator/clock/integrity/coverage evidence and >=100 RTT samples/session.
Candidate delivery must be >=99% of the adjacent control; added endpoint loss
versus control <=0.1 percentage point, added p95 RTT <=1 ms, per-role process and
owner CPU <=control x 1.05 + 0.01 core, and peak RSS <=max(control x 1.1,
control + 1 MiB). Existing applicable absolute SC bounds remain unchanged.
SC-004/005 apply to subcapacity, not an absolute <=0.1% capacity-loss guarantee.
The adopted qualified CUBIC executable is the reference; this does not rewrite
the historical failed CPU comparison or carry its exact-binary exception to a
new executable. No failed/invalid repeat is averaged away.

## Completed results

T080 and T081 are complete. Formatting, Clippy with warnings denied, locked
default/qlog release builds, **187 native tests**, **100 Python tests**, the
independent hold-response shutdown test and all **29 independent Go protocol
cases** pass. The two ordinary ignored tests retain their prerequisite markers;
both were also explicitly executed successfully in this increment. The protocol
manifest identifies the same new release executable used for measurement.

Candidate ASan smoke completes without crashes: MASQUE 3,448,123, policy
4,210,611, ancillary 21,852,010 and lifecycle 948 executions. Each campaign is
configured for 30 seconds and logs 31 elapsed seconds including its final
iteration. These are smoke campaigns, not an exhaustive state-space guarantee.

The comparison completes **36/36 members and 12/12 per-repeat checks** in
**1966 seconds (32m46s)** under the 2700-second budget, including namespace
cleanup. All generator, clock, integrity, diagnostic coverage and sample checks
pass. Policy SHA-256:
`fb74ab9e2ab628ead5d7d7e0d59f1628588c63dfc6e5b7da6241176910fd6dbb`.
No threshold or policy is changed after collection starts.

| Observed candidate metric | Result across the three repetitions |
|---|---|
| Delivered count / direct at the fixed capacity-load point | At least 99.9966% in each direction |
| Maximum subcapacity added endpoint loss vs direct | 0.0002913 percentage point |
| Maximum subcapacity p95 RTT difference vs direct | -0.047 ms; no increase observed in these pairs |
| Maximum added endpoint loss vs adjacent control | 0.0033833 percentage point |
| Maximum added p95 RTT vs adjacent control | 0.099 ms |
| Adjacent process CPU change per role | -1.73% to +2.82%; each run passes its ratio-plus-absolute-margin bound |
| Maximum candidate process / owner CPU | 0.916720 / 0.915385 core per role |
| Maximum candidate RSS | 13,201,408 bytes (12.59 MiB) per role |
| Resource delivery, every repetition | 500.736 Mbit/s each direction, zero measured endpoint loss |

These are paired observations at the stated workloads. In particular, the fixed
capacity-load comparison is not a fresh maximum-capacity search, and small CPU
differences are not a causal attribution to a single branch. CUBIC remains
selected; no BBR2 experiment, target change or retrospective CPU waiver is made.

The new qlog release executable SHA-256 is
`2e2b923c0ae7e56f842f215fbe44801facadf637249cbb7cc3abd719b3cca231`.
The previous full T076/T077 qualification continues to identify
`7f7a3c4de7341b8edf0860ff3c8457642ff423cf4658f9aa93076db678cfe3b1`.
The new executable has this increment's native, interoperability, fuzz and
focused regression evidence; it does not inherit the old full VM/reference
qualification. No VM/full release matrix was rerun and no new independent review
of Phase 9 is claimed.

`summary.json`, `comparison/evaluation.json`, `comparison/manifest.json` and the
immutable per-member directories retain the detailed identities/results. Source
inputs still match after collection. Failed initial setup attempts are retained
separately: missing fuzz PATH, an initial test compile error, and a comparison
startup helper's Path argument error before any measurement. They are not
counted as passing checks or discarded numerical failures. Existing Phase 8
records and raw measurements are unchanged.
