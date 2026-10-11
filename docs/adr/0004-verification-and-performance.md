# ADR-0004: Native independent qualification and frozen benchmark criteria

Date: 2026-10-10. Revised: 2026-10-11. Status: design selected; revised A2 accepted, early measurements are normal implementation work.

## Context

A performant authenticated tunnel must demonstrate correctness, both-role independent compatibility, security and measured directional throughput/loss/latency/resources. The initial planning snapshot had no executable or recorded reference hardware/results; the current skeleton and foundation still provide no working tunnel or benchmark evidence. Architectural estimates are not evidence that release targets have been met; the specification permits implementation to start and requires early reproducible measurements as soon as the basic tunnel works.

## Options considered

Self-interoperation only versus independent peer; cross-build only versus native target execution; isolated microbenchmarks versus matched end-to-end relay baseline; echo-coupled throughput versus independent senders; silently adjusting targets versus early incremental measurements and unchanged release qualification.

## Decision

Start with native Linux x86_64 development and checks for implemented behavior. Add ARM64 native build/runtime verification during development and complete it before claiming ARM64 release support; it does not block x86_64 work. Preserve unit/property/fuzz/component/syscall/process tests, dedicated privileged network jobs and explicit security review as applicable behavior and release scope mature. Use pinned independent quic-go/masque-go fixture in both roles, with CA validation/Bearer/allowlist plumbing but independent protocol encoding. Treat a source/API pin as preparation, never a successful interop run.

Measure the specified two-leg unencrypted UDP relay baseline and tunneled traffic under identical recorded conditions. Independent directional generators and sparse RTT probes prevent coupled echo limits. Five paired runs with fixed warmup/interval, each threshold assessed per run; separate outer estimates/local/kernel/tool loss from uniquely delivered application loss. Record platform/kernel/NIC/offloads/toolchains/locks/settings/seeds and all invalid/failed results. Report kernel socket memory separately from RSS.

Accepted A2: implement the authenticated end-to-end MVP and reproducible same-topology baseline/load harness as normal early increments. Measure immediately when the basic tunnel works on x86_64, declare incomplete coverage/unmet targets honestly and use results to guide subsequent engineering. No separate prototype, pre-implementation measurements or dual-architecture development checkpoint. Preserve all SC thresholds as release acceptance criteria; any target amendment is prospective and explicit. Compare batching/timerfd/offloads/CC or runtime reuse when contracts/profiles justify it; add sharding or other complexity only for measured need.

After first measurements and before critical-path changes/candidate runs, freeze a pre-change Remora reference and workload-specific regression bounds/comparison method justified by reference variability. Keep this revision comparison distinct from the matching unencrypted-relay release baseline. Missing evidence is inconclusive/invalid, failed comparisons remain recorded and revised policies apply only prospectively; regression allowances cannot waive frozen SC criteria. The first MVP needs neither a prior Remora reference nor fabricated numeric tolerances.

The checked T034 milestone allows dependency-ready product work to continue while explicitly named T035–T037 external compatibility qualification remains pending. Leave such tasks unchecked, record unavailable prerequisites and retain their full story/release obligations; a failing check for implemented behavior cannot use this deferral. Phase headings and local Spec Kit execution instructions must follow the declared milestone/dependency graph. Early instructions describe available evidence without waiting for external qualification.

## Verification corrections (R1–R6)

Independent-review follow-up is part of normal increments: R1 two-stream close/commitment/EAGAIN/replacement tests in the first forwarding path; R2/R3 successful-response and Capsule parsing negatives in the protocol path; R4 sparse-traffic idle/stream/socket lifecycle tests; R5 terminal correlation under suppression plus explicit overflow/failed-sink robustness. These are required behavior checks, not successful tests claimed by this ADR.

R6: the task template, generation and execution instructions require constitutional/contract tests for behavioral changes even when a feature does not repeat them. Explicit TDD chooses test-first ordering; tests themselves are mandatory. Initial GOAWAY cutoff/new-request refusal/accepted-stream continuation checks accompany the first transport/MVP, with replacement recovery checked later. Keep foundational work to the first usable tunnel's real dependencies, with optional qlog/offloads, complete later matrices and ARM64 qualification sequenced after the early x86_64 MVP/baseline. No unnecessary framework or process checkpoint is added.

## Consequences

Dedicated reference runners and independent fixture maintenance cost effort but support credible claims. No promised performance on every architecture/network without evidence. Accepted A1 tests observable active-client isolation and rejection of retained/identifiable stale data while explicitly characterizing impossible external provenance claims after identical UDP tuple reuse. No fixed quarantine test or approval remains outstanding. No deployment follows from planning or static checks. Source checks performed now are not runtime qualification.

Evidence: [benchmark/test contract](../../specs/001-masque-udp-tunnel/contracts/benchmarks.md), [accepted amendment record](../../specs/001-masque-udp-tunnel/proposed-amendments.md), [independent peer client](https://quic-go.net/docs/connect-udp/client/), [independent peer proxy](https://quic-go.net/docs/connect-udp/proxy/).
