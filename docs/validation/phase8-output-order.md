# Phase 8 output-order correction — 2026-10-16

The native regression and increment checks pass. The focused comparison
completed all 60 rows in 54m44s but failed acceptance. Final
functional/performance qualification remains pending.

## Diagnosis and change

The earlier outer-buffer comparison remains unsuccessful. Six uninstrumented
diagnostic runs localize additional BBR2 medium-packet losses to application
queues and, in some repeats, application kernel buffers. They do not reproduce
the earlier 92-packet outer-socket overflow. Scheduler samples contain accumulated
runqueue wait over each approximately 50ms observation interval, rather than a
measured duration of a single pause.

Two separately instrumented BBR2 runs observe a full 32-packet application queue,
64-packet native queue and 24–31 ciphertext records with an immediately eligible
front. Bounded diagnostic output is retained under
`benchmarks/results/phase8-service-timing-probe-20261016/`; `analysis.json` contains
examples and the instrumented executable hash. This is causal investigation,
not a production performance comparison.

[ADR 0022](../adr/0022-drain-committed-output-before-generation.md) changes the
output visit to drain eligible committed packets, generate new packets into
the freed space, and submit using any remaining quota. The same visit retains
16 generations and 16 offered submission descriptors, all prior expiry/PMTU
checks and pacing/FIFO rules. No queue or residence limit is enlarged.

A regression using real native DATAGRAM queues and six real ingress UDP
messages fails against the previous order with six queue-full drops, then
passes after the fix with zero. Its committed ciphertext FIFO uses a scripted
physical-send boundary; it is not a real encrypted end-to-end delivery test.
Additional regressions cover partial sends, EAGAIN, EINTR, ENOBUFS, shared
descriptor budgets, immutable prefix progress, expiry and future pacing.
Existing process E2E and independent-peer tests separately pass.

## Exact increment checks

All check logs and source/binary snapshots are in
`benchmarks/results/phase8-output-order-fix-20261016/`:

- Locked offline default and qlog release builds, formatting and all-target,
  all-feature Clippy with warnings denied pass.
- 182 ordinary Rust tests pass; two opt-in tests are ignored in that invocation.
  The independent-peer process test is explicitly run separately and passes.
- 23 traffic-tool tests, traffic build/format/Clippy and 95 Python tests pass.
- `regression-before.json` records the observed failing native regression;
  `regression-after.json` binds the passing full Rust log and source hash.
- The new qlog binary SHA-256 is
  `7f7a3c4de7341b8edf0860ff3c8457642ff423cf4658f9aa93076db678cfe3b1`;
  control SHA-256 is
  `a766bdb78cb54910ee1a7246b0077936b23430a9cf27627f541558f825633add`.

## Prospective comparison

`benchmarks/policies/phase8-output-order.json` freezes the policy before
collection; `frozen-policy.json` preserves the launch copy. There are three
repeats of each workload with direct UDP, old/new CUBIC and old/new BBR2.
Tunnel order reverses on odd repeats; direct UDP comes first.

| Workload | Sessions | Payload bytes | Packets/s/direction | Measured seconds |
|---|---:|---:|---:|---:|
| Small | 1 | 128 | 64849 | 15 |
| Medium | 1 | 512 | 91552 | 60 |
| Capacity | 32 | 1200 | 91136 | 60 |
| Resource | 32 | 1200 | 52160 | 60 |

All modes explicitly request 1MiB outer and 128KiB application socket buffers.
Expected effective RX/TX limits are 2MiB and 256KiB respectively. Topology is
20ms RTT/1Gbit/s, mmsg batch 32, offloads disabled, three-second warmup. Client
CPUs are 0–3, server CPUs 4–7, tools/collector CPUs 8–23. No compilation runs
during collection. The expected duration is about 55 minutes; the driver has a
65-minute deadline and its coordinator a 66-minute deadline including teardown.

CUBIC retains every numerical bound from
`phase8-density-receive-buffer.json`, including historical small-packet CPU and
capacity-loss limits, plus matched sustainable-baseline/SC checks and the strict
adjacent small-packet two-message allowance. BBR2 has a distinct prospective
scope: general SC checks, per-role process/owner CPU no greater than adjacent
old BBR2 times 1.05 plus 0.01 cores, and medium delivery no lower than old BBR2
minus two messages per direction. Historical CUBIC CPU bounds are not BBR2
acceptance bounds. Reports must preserve this distinction.

The collector retains layer-separated kernel queues/drop counters; sample
boundaries are not exact per-message attribution. Baseline invalidity, generator
errors and insufficient RTT samples fail acceptance. All planned rows are
retained even if a numerical gate fails. `comparison/evaluation.json` contains
per-repeat failures; `comparison-progress.json` is coordinator status and
`comparison/progress.json` is row progress. Run
`python3 /tmp/remora-phase8-output-order-status.py` for a concise view.

This comparison cannot establish final T076/T077/T078 acceptance, native ARM64
support, or that every earlier rare loss cause has been removed. CUBIC remains
the default; T079 is excluded. No independent reviewer audit, commit or push
is claimed.

## Completed comparison

The coordinator records `comparison-completed-with-failures`, with all 60 rows
retained. The driver exits 1 only after evaluating all rows:
`RuntimeError: frozen comparison criteria failed; all rows retained`. No process
crash or measurement interruption is reported. Production inputs and the frozen
policy still match; all 60 measurements pass generator/integrity/calibration
validity checks. This does not mean their performance acceptance passes.
`analysis.json` is a derivative analysis; original `comparison/evaluation.json`
and the policy remain unchanged.

| Corrected mode | Small | Medium | Capacity | Resource | Overall |
|---|---|---|---|---|---|
| CUBIC | 0/3, historical CPU | 3/3 | 3/3 | 3/3 | 9/12 |
| BBR2 | 3/3 | 1/3, paired loss | 3/3 | 2/3, socket count | 9/12 |

These modes have different acceptance scopes as frozen above; equal totals do
not imply equivalent results.

### CUBIC delivery and CPU

Medium workload missing endpoint messages, out of 5493120 offered per direction
per repeat:

| Repeat | Old forward / reverse | Corrected forward / reverse |
|---|---:|---:|
| 0 | 15324 / 18095 | 90 / 78 |
| 1 | 17243 / 25 | 77 / 161 |
| 2 | 33 / 0 | 0 / 7 |

The aggregate is 50720 old versus 413 corrected missing messages across both
directions and all repeats. This is observed improvement in these runs, not
proof that every direction improves or every rare trigger is removed. Corrected
medium loss is at most 0.002931%. Enclosing diagnostic snapshots report zero
client queue-full/expired increments for all corrected medium repeats, versus
client queue-full increments 12205/13484/0 and expiry 1406/2339/0 for old CUBIC.
Corrected server queue-full increments are 15/37/0. Application-kernel drop
deltas plus those queue increments numerically account for the corrected
medium endpoint losses; snapshots surround measurement by a few milliseconds
before and approximately 0.5s after, so exact message attribution is not claimed.

Corrected capacity is at least 874.87536Mbit/s/direction, with maximum loss
0.0034564%. Corrected resource delivers 500.736Mbit/s/direction in all three
repeats without measured endpoint loss; per-role CPU is at most 0.915203 core
and resource RSS at most 12.161MiB. These checks pass.

Small delivery is lossless, but process CPU is 0.400858–0.437800 client and
0.412254–0.467165 server, against historical maxima 0.392008/0.402575. Owner CPU
also fails its historical bounds. Adjacent old CUBIC exceeds these historical
CPU bounds too. Corrected versus adjacent old process CPU increases by
1.57–4.98% client and 0.04–4.08% server. Thus neither attributing the whole
historical excess to this patch nor declaring no regression is supported.

### BBR2 residual loss and resource observation

Corrected medium forward losses are 804/1015/455, versus 3775/1754/200 for old
BBR2. Reverse losses are 79/230/69, versus 113/68/0. Repeats 1 and 2 exceed
the prospectively frozen two-message paired allowance. Maximum corrected
medium loss is 0.018478%, within general SC loss limits but insufficient for
the separate no-regression gate. Enclosing snapshots show client queue-full
increments 606/990/455; client application-kernel drop deltas are 198/25/0.
Reverse losses equal the server application-kernel drop deltas. This preserves
evidence that the ordering fix does not eliminate all BBR2 queue pressure.
Corrected BBR2 uses 8.17–16.74% more small-packet process CPU than adjacent
corrected CUBIC. CUBIC remains the default.

All corrected BBR2 resource repeats also deliver 500.736Mbit/s/direction without
endpoint loss. Repeat 2 fails the full-lifetime socket-count guard because the
first client sample counts two outer sockets, 4965ms before measured start.
Every subsequent sample and the entire measured minute count one. The snapshot
does not retain socket identities, so its origin remains unresolved; neither a
persistent leak nor a harmless sampling explanation is established. The raw
failed gate is preserved.

The adjacent old BBR2 resource repeat 2 has a separate substantial failure:
416549/188139 missing messages (13.31%/6.01% loss), delivering only
434.08816/470.63376Mbit/s. Enclosing snapshots show 317248 client and 186112
server queue-expiry increments, no queue-full increments or sampled kernel
drops. These do not account for every missing message. Its CPU average is not
a healthy equal-delivery control, and this single contrast does not establish
a universal fix for such stalls. All other old resource repeats are lossless.

The owner subsequently accepts the small CPU cost and selects CUBIC;
[ADR 0023](../adr/0023-select-cubic-and-accept-measured-cpu-cost.md) records the
exact candidate, authorization, scope and T078 reconciliation. CPU optimization
and BBR2 investigation stop for this increment. The raw focused acceptance
remains failed. A short exact-candidate controlled-loss characterization
precedes refreshed full qualification. No new long test was launched during
the earlier result inspection.
