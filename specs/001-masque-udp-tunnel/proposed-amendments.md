# Accepted specification amendments: A1 and A2

**Scope update — 2026-10-17:** [ADR 0024](../../docs/adr/0024-defer-arm64-release-scope.md) supersedes the original ARM64 commitment below. Linux GNU x86_64 is the sole current required release target; ARM64 is deferred outside completion gates. The original research/decision text remains historical.

**Decision date**: 2026-10-11. **Status**: revised as instructed by the user and applied to `spec.md` and planning artifacts. The filename is retained so existing links remain valid. The former fixed 30-second socket quarantine and separate dual-architecture feasibility gate are superseded; they are not requirements.

## A1: Observable isolation and realistic UDP guarantees

Remora must isolate local senders, authenticated clients and active sessions, prevent cross-client data leakage through routing/state reuse/retained data, and discard identifiable stale session traffic. Responses are eligible only for their matching accepted association and authorized destination. Pending datagrams and delayed work from a closed connection/session cannot be reassigned or replayed into another association.

Opaque destination UDP replies do not contain tunnel-session identity. After an identical relay endpoint tuple is reused or the process restarts, a delayed authorized-destination reply can be indistinguishable from a new reply. An application restart reusing a still-active sender tuple may likewise lack an observable lifecycle boundary. These are documented limits of available information, not permission to merge active sessions, forward identifiable stale traffic, leak retained data across clients, or weaken credential/target checks.

The implementation strategy and its resource, lifecycle, availability and security trade-offs belong in architectural design. No particular socket quarantine, duration, retained-socket entity, persistent reservation store or extra configuration knob is mandated. The current design uses distinct active destination associations and generation-aware stream/state routing; any additional reuse mitigation must be justified by the actual threat model and evidence. Required tests cover independent clients/shared destinations, closure/recreation, stale callbacks/queued datagrams, wrong-source replies and the documented behavior/limitations of ambiguous external replies. A limitation must not be presented as a successfully verified provenance guarantee.

Applied to FR-010/FR-014, User Story 2 scenario 4, Edge Cases and Assumptions. Resource bounds and SC-007 reclamation criteria remain unchanged.

## A2: Early MVP and benchmarks within normal implementation

All existing numeric SC-001..SC-010 targets remain release acceptance criteria, not guaranteed outcomes or achieved results. Normal incremental implementation includes an early authenticated end-to-end MVP and a reproducible unencrypted-relay baseline harness on the same two-leg topology. Measure performance as soon as the basic tunnel works, record environment/settings/results and use those findings to guide subsequent engineering and optimization.

There is no separate feasibility prototype, no requirement for measurements before implementation, and no mandatory tokio-quiche comparison as a development checkpoint. Reuse/optimization alternatives are evaluated when API/resource contracts or observed bottlenecks justify them. Start development and early measurements on x86_64. ARM64 hardware, native validation and measurements do not block that work; native ARM64 validation and applicable acceptance evidence remain necessary before claiming ARM64 release support.

The early MVP retains authentication, server identity validation, default-deny target policy, datagram correctness, bounded resources and safe diagnostics. Incremental benchmarks may report unmet targets and incomplete coverage honestly. Release qualification still uses the full prescribed workloads, repetitions, security/conformance/interop tests and resource criteria. Target amendments remain explicit, reviewed and prospective; no failed result is relabelled as a pass.

Applied to Assumptions, the plan's gate/sequencing, research, state/interface contracts, quickstart and ADRs. No constitutional correctness, security, standards or measurement obligation is waived.

## Independent-review corrections R1–R6 (2026-10-11)

These corrections address the accepted read-only audit within existing scope; they are design decisions and future test obligations, not runtime passes.

| Finding | Selected correction | Required evidence in normal implementation |
|---------|---------------------|--------------------------------------------|
| R1 — generated output lifecycle | Successful quiche packet generation is outer transmission commitment. Close discards cancellable application/native queues and prevents new commitment; immutable committed output may finish only on its original association within the existing 100 ms generation-based bound. Replacement/stop discards unsent old output with old transport state. Local close cannot recall committed or kernel/network in-flight packets. FR-010/014 explicitly distinguish these boundaries; no ciphertext rewriting, per-session cancellation fiction or shared-connection close merely for session cleanup. | Two streams, close before/after generation, EAGAIN/partial send, original deadline/order, unaffected-flow progress and no replacement replay |
| R2 — successful Capsule response | Final well-formed 2xx excluding 204/205/206, with prohibited Capsule message fields rejected before forwarding; proper Structured Field handling and normative stream errors | Positive independent-peer acceptance and negative status/header cases |
| R3 — Capsule validation before discard | Bounded Context ID parsing and checked Context 0 payload length precede discard of valid unsupported carrier; syntax/truncation and >65527-byte semantic errors abort appropriately | Byte-split/truncated prefix, 65527/65528 boundaries, valid skip followed by next capsule |
| R4 — idle timeout | Default 120000 ms of valid UDP activity, with request/socket closure together; probes do not refresh application idle. Shorter operator override is an explicit documented SHOULD deviation with sparse-traffic consequences. | Sparse traffic, idle expiry, stream/socket lifecycle and source-port/setup behavior |
| R5 — suppressed terminal diagnostics | Fixed 256-entry/64 KiB terminal ring plus bounded live failure status and status reservation, all inside existing 2 MiB diagnostic budget; completed output acknowledges records, overflow/failed output reports missing coverage | Correlate every SC-010 injection under ordinary suppression with zero status coverage loss; separate overflow/blocked-sink robustness |
| R6 — optional-tests workflow | Task template, speckit-tasks and speckit-implement instructions require constitution/contract behavioral tests; only TDD ordering depends on explicit request | Generation/execution consistency against each behavioral contract and actual test dependencies, without completing later subsystems ahead of MVP |

The R1 correction explicitly narrows the formerly overbroad cancellation wording: no promise of instantaneous remote silence for packets committed before local close. It preserves mandatory client/session ownership, no new commitment after closure, closed-flow receive discard, bounded output and the prohibition on reassignment/replay. This internal boundary is separate from A1’s external UDP provenance limitation.

The audit’s permanent-rejection ambiguity is resolved by one fixed-size client mapping admission latch until controlled restart, surviving flow cleanup/new sender tuples/transient recovery; the server retains no unlimited deny history. Single-owner capacity, shared native-queue fairness/control servicing and actual buffer/RSS/qlog overhead remain measurement risks. Profile these with the early x86_64 baseline; optional qlog follows the first working MVP. No extra feasibility or platform prerequisite is introduced.

## Cross-artifact analysis follow-up (2026-10-11)

Applied after user approval of the speckit-analyze findings; these are documentation/task corrections, not successful implementation checks.

| Finding | Applied correction | Evidence required during implementation |
|---------|--------------------|-----------------------------------------|
| C1 — prospective regression tolerances | First MVP establishes measured references; before later critical-path changes/candidate runs, freeze reference revision/hash, covered workload, metric bounds/units and noise/comparison method. Separate regression outcomes from unchanged SC release criteria. | Manifest/result validator plus affected paired comparisons in T034/T047/T050/T054/T055/T063/T071 |
| C2 — initial GOAWAY verification | T019 verifies the GOAWAY behavior already introduced in T018 and is required by T033; T060 retains later replacement/recovery checks. | Cutoff, refusal of new/excluded requests, accepted lower-ID continuation and no replay before first MVP acceptance |
| I1 — unconditional TDD in executor | Execution instructions now match R6: mandatory behavioral checks before increment acceptance; test-first only on explicit TDD request. | Actual implementation/test dependencies respected; no optional-tests waiver |
| I2 — phase barrier after MVP | T034 explicitly permits dependency-ready later stories while named external qualification T035–T037 remains unchecked; T038 publishes available evidence. Executor and template follow declared checkpoints/dependencies. | Missing external evidence recorded; no deferral of failing implemented-behavior checks and no unsupported full story/release claim |
| U1 — deferred offload auto | Accept off/auto; deferred auto uses effective off/not_implemented without a fake probe. Implemented auto probes and reports fixed capability/path state under bounded diagnostics. | T017/T052 distinguish configured-off, not-implemented and capability/eligibility paths without packet duplication or changed pacing |

## Gate reassessment

PASS for task generation and incremental implementation: A1/A2 and the R1–R6 contract corrections are integrated. This is not an assertion that every engineering risk is experimentally resolved; targeted checks above must accompany their implemented increments. Task generation and normal incremental implementation may proceed. Functional/security checks accompany each implemented increment; full platform and numerical qualification are release gates, not prerequisites for writing the MVP. This document is a decision record, not evidence of implemented or measured behavior.
