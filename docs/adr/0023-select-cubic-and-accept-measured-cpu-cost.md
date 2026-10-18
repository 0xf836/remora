# ADR 0023: Select CUBIC and accept the measured output-order CPU cost

Status: accepted by the project owner; final qualification pending.
Date: 2026-10-16.

## Context and authorization

ADR 0022's corrected qlog executable is
`7f7a3c4de7341b8edf0860ff3c8457642ff423cf4658f9aa93076db678cfe3b1`.
Its complete 60-row comparison passes all CUBIC medium/capacity/resource checks.
The three small-packet repeats fail the prospectively frozen historical CPU
bounds. Small delivery is lossless. Adjacent old CUBIC also exceeds those CPU
bounds; the correction adds 0.04–4.98% process CPU, at most about 0.02 core per
role, relative to its adjacent control. The corrected resource workload is
lossless at 500.736Mbit/s/direction, with <=0.915203 core and <=12.161MiB RSS
per role. The complete comparison and original failed policy remain retained.

The owner explicitly accepts this measured CPU cost as a small engineering
trade-off and authorizes stopping CPU optimization, retaining CUBIC, a short
controlled-loss characterization and proceeding to final qualification.
This is an approved adoption decision for a measured candidate, not a claim
that the original no-regression comparison passed. The owner is responsible
for the trade-off; T078 must reconcile it against final qualification before
release. It is not authorization for arbitrary future CPU increases.

## Options and decision

Keep CUBIC as the default and keep ADR 0022's corrected bounded output order.
Accept the four small-workload historical process/owner CPU bound failures
for this exact candidate. Record them as an accepted measured trade-off in
the adoption policy, separate from unchanged raw comparison acceptance.
Stop further CPU optimization and BBR2 diagnosis in this Phase 8 increment.

BBR2 remains an existing optional configuration. Earlier controlled losses
demonstrate its potential for lossy paths, but the latest comparison retains
medium-packet queue pressure, paired delivery regressions and higher small
CPU usage. Switching defaults or qualifying BBR2 broadly would expand the
current work without being necessary to validate the selected CUBIC path.

Before full qualification, characterize three alternating control/drop-five
pairs on the exact current CUBIC executable. Inject five server-to-client
outer packets in each impaired run, preserve counter proof and all endpoint
results. Impaired-path results describe risk; the reference-path SC thresholds
are not reinterpreted as injected-loss acceptance criteria. This diagnostic
must complete validly and clean up its private rules/processes.

## Boundaries and consequences

No production code, dependency, wire/configuration contract, queue size,
residence deadline, security check or resource requirement changes. In
particular SC-003/004/005/007 stay unchanged; the 32-session resource budget
remains <=2 logical cores and <=128MiB RSS per role at >=500Mbit/s/direction.
No prior FAIL is rewritten and no old executable's qualification transfers.
The adoption exception is limited to the recorded small CPU metrics for the
exact hash above and is reconciled/expires at T078. A changed candidate needs
fresh evidence and its own regression/adoption assessment.

T073–T075 evidence is refreshed for exact inputs, then full T076 runs with
the owned generic/WireGuard VM fixture and a hard 90-minute total coordinator
budget including prerequisites and cleanup. T077 follows only after exact-input
T076 acceptance; T078 follows both. ARM64 remains unqualified and T079 excluded.
No independent reviewer, commit, push or publication is authorized here.
