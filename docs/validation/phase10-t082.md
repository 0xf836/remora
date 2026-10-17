# T082 owner continuation — 2026-10-17

Scope: T082 only, native Linux GNU x86_64. This increment corrects the
remaining H3 continuation source behind full ciphertext output. It does not
change queue sizes, residence deadlines, protocol, trust, dependencies or the
Linux syscall implementation. [ADR 0025](../adr/0025-connection-local-output-backpressure.md)
records the scheduling decision.

## Behavioral regression

`full_ciphertext_owner_parks_until_readiness_pacing_or_original_expiry` runs
all stages of a complete owner window and then the production physical-readiness
poll. Before the fix it fails at `blocked application work must not keep the
owner running`. Afterward its four EAGAIN/future-pacing and expiry/resumption
combinations pass, as do the two existing T081 backpressure regressions.
No I/O-driver tick is inserted between clearing cached EAGAIN writability and
the final poll; a subsequent real writable notification remains eligible.
The existing native timer test separately exercises actual timer wakeup.

The first sandbox attempt fails at the local UDP fixture prerequisite and
is not counted as a failing-before behavioral result. `before-native.log`
contains the behavioral failure; `after-native.log` contains the three passing
targeted regressions. Local evidence is retained under
`benchmarks/results/phase10-t082-20261017/`.

## Affected checks and prospective comparison

Formatting, Clippy with warnings denied, locked default/qlog release builds,
188 ordinary native tests, 100 Python tests, the independent hold-response
shutdown test and all 29 independent both-role protocol cases pass. The two
ordinary ignored prerequisite markers remain; both tests were explicitly run.
All four 30-second ASan smoke campaigns pass. These are affected checks, not
an exhaustive fuzz guarantee or a new full release matrix.

The candidate qlog executable SHA-256 is
`a4b10cf925fea1739595c6b30d2d46a4535ebf9dce0d5398431cca9c77da557f`.
The [prospective policy](../../benchmarks/policies/phase10-owner-continuation.json)
SHA-256 is
`c8816cbfae04a8c7584f24ab9c457d95843df55f6b9fb7c4e33ff28d5debeaf1`.
It binds production, driver/runner/profile/tool and both executable identities,
three alternating repetitions of the four Phase 9 simultaneous workloads and
a 2700-second budget. Per-repeat candidate bounds remain delivery >=99% of
control, added endpoint loss <=0.1 percentage point, added p95 RTT <=1ms,
process/owner CPU <=control x 1.05 + 0.01 core and RSS <=max(control x 1.1,
control + 1 MiB), with existing applicable absolute SC thresholds unchanged.
The direct member must independently meet its <=0.1% endpoint-loss bound;
all generator, clock, integrity, diagnostic coverage and sample checks apply.

The paired comparison completes successfully: 36/36 members and 12/12
per-repeat checks pass in 1966.45 seconds (32m46s), including namespace cleanup,
under the 2700-second policy budget. T082 is complete. The control is the exact preceding Phase 9 executable
`2e2b923c0ae7e56f842f215fbe44801facadf637249cbb7cc3abd719b3cca231`.
The focused comparison reuses the Phase 9 workloads and numerical bounds in a
new policy frozen before candidate measurements; it does not reuse its result.
No full VM/reference release qualification or independent reviewer audit is
claimed for this increment.


## Completed measurements and evidence boundary

| Candidate observation across three repetitions | Result |
| --- | --- |
| Capacity delivered/direct ratio, each direction | >=99.9985% at this fixed offered load |
| Maximum subcapacity added endpoint loss vs direct | 0.0014564 percentage point |
| Maximum subcapacity p95 RTT difference vs direct | -0.050 ms; no increase in these pairs |
| Maximum added endpoint loss vs adjacent control | 0.0022574 percentage point |
| Maximum added p95 RTT vs adjacent control | 0.037 ms |
| Adjacent process CPU difference per role | -4.63% to +1.39%; every ratio-plus-margin bound passes |
| Maximum process / owner CPU per role | 0.913897 / 0.912563 core |
| Maximum candidate RSS per role | 13,131,776 bytes (12.52 MiB) |
| Resource delivery, separately by direction/repeat | 500.73152–500.736 Mbit/s |

One resource repeat loses 28 of 3,129,600 forward messages (0.0008947%);
its reverse direction and both directions of the other resource repeats have
zero measured endpoint loss. Every measured resource direction still exceeds
500 Mbit/s. No failed repetition is averaged away, and no criterion or policy
changes after collection starts. These fixed-load observations do not establish
a fresh maximum-capacity search or causally attribute small CPU differences to
one branch. Source, runner and policy hashes remain unchanged after collection.

ASan smoke executions are MASQUE 3,031,780, policy 4,362,879, ancillary
21,592,528 and lifecycle 842. Each reports 31 seconds including its final
iteration for a configured 30-second campaign; no crash is observed.

`summary.json`, `verification-summary.json`, `comparison/evaluation.json`,
`comparison/manifest.json`, immutable member artifacts and the archived driver
retain detailed provenance and commands. The original sandbox fixture failure
and the failing-before behavioral result remain separate from passing checks.
`Runner::poll_io()` preserves the preceding physical readiness code verbatim
apart from indentation; it introduces no new readiness policy.

The new executable has this increment's affected native, independent-peer,
fuzz and focused comparison evidence. The original full T076/T077 qualification
continues to identify `7f7a3c4de7341b8edf0860ff3c8457642ff423cf4658f9aa93076db678cfe3b1`.
It is not transferred to this changed executable. No VM/full release matrix,
new Miri/syscall/dependency audit, ARM64 qualification, independent reviewer
audit, commit or publication is claimed by this increment.
