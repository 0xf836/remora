# Independent review: Phase 2 foundation and requirements readiness

**Date:** 2026-10-11
**Reviewed state:** Working tree based on commit `6cdee35`, including tracked changes and untracked Phase 2 source, tests, fixture tooling, ADR and validation documentation.
**Review mode:** Read-only. The reviewer did not modify project files, task states, checklist markers or commits.

**Conclusion:** Phase 2 is substantially implemented, and no significant security or correctness defect was identified in the reviewed configuration, buffer-accounting or diagnostic primitives. However, the foundation increment should not yet be recorded as fully accepted: the child-process guard can leave runner-owned descendants alive, and the implemented configuration parsers lack the fuzz/property verification required by Constitution V. Retain T005–T007 as complete; reopen T008 and T009. All 40 requirements-readiness criteria and all 16 specification-quality criteria remain satisfied, because the findings concern implementation and verification rather than defects in the written requirements.

The necessary corrections are small and fit the existing foundation work. They do not require a new feasibility prototype, another runtime, ARM64 qualification, performance measurements or an additional review process before initial x86_64 development.

## Findings

### F1 — MEDIUM: Successful completion of a process-group leader disables cleanup of its surviving descendants

**Locations:**

- `tests/common/mod.rs:81`–87: `kill_group()` returns immediately when `reaped` is true.
- `tests/common/mod.rs:94`–96: `finish()` sets `reaped = true` after observing and reaping the direct child.
- `tests/common/mod.rs:106`–111: `Drop` relies on `kill_group()` to clean up the process group.
- `specs/001-masque-udp-tunnel/tasks.md:39`: T008 requires bounded runner-owned cleanup.

**Problem:** Reaping the direct child does not establish that its process group has no surviving descendants. A runner command can start a child process and exit successfully. `finish()` then sets `reaped`, and subsequent cleanup skips the group entirely.

**Evidence:** Source tracing identified this execution path. At the reviewer's request, the main agent executed a standalone scratch probe outside the repository, using the actual `tests/common/mod.rs` implementation and the currently built dependency libraries. The guarded command was:

```text
sh -c 'sleep 30 & echo $! > "$1"; exit 0'
```

The probe called `finish()` and dropped the guard, then checked the recorded descendant PID. Its output was:

```text
parent_success=true descendant_alive_after_finish_and_drop=true
```

The main agent killed its own remaining descendant after the check. This reproduction is evidence supplied by the main agent; the reviewer inspected the scratch probe source but did not execute it independently.

**Impact:** Future scenario runners using this reusable guard can leave traffic generators, servers or other descendants running after successful leader exit. Those descendants can retain sockets and interfere with later tests. The same escape can occur after a failure if the leader has already exited before cleanup begins. This contradicts T008's cleanup guarantee.

This is a test-runner lifecycle defect, not evidence that the current Remora production binary leaks processes.

**Recommended correction:** Keep process-group cleanup independent of the direct child's completion status, while preserving protection against PID/process-group identifier reuse. Do not simply remove the `reaped` check and signal a potentially reused identifier after reaping the leader. Use a lifecycle that retains a trustworthy group identity through cleanup—for example, observe leader exit without reaping it, terminate remaining members of the owned group, then reap the leader. An equivalently safe, bounded arrangement is acceptable.

Add a regression covering a leader that spawns a descendant and exits successfully, plus the applicable timeout/error cleanup path. Verify that no owned descendant remains and that unrelated processes are untouched.

**Task effect:** Reopen T008. T009 should also remain incomplete until the regression and affected checks pass.

### F2 — MEDIUM: Configuration parsers have malformed examples but no required fuzz/property coverage

**Locations:**

- `.specify/memory/constitution.md:72`–75: parsers and untrusted-input boundaries must have malformed-input coverage and fuzz or property-based tests.
- `src/config.rs:213`–230: schema parsing and role validation.
- `src/config.rs:532`–605: credential, target-rule and CIDR parsing.
- `src/config.rs:618`–695: host canonicalization and authority parsing.
- `tests/security/config.rs:76`–424: example-based configuration and parser tests.
- `tests/io/buffers.rs:278`–286: the current property test exercises `Budget`, not a parser.
- `specs/001-masque-udp-tunnel/tasks.md:40`: T009 includes the foundation verification required before accepting the increment.

**Problem:** The existing tests cover useful malformed schema, token, PEM, hostname, CIDR and numeric examples. They do not provide fuzz or property-based verification for the implemented parsers. The only current `proptest` checks quota accounting.

**Evidence:** Inspection of the registered tests and a repository search for `proptest`, `quickcheck` and `fuzz` found the quota property test and no parser property/fuzz test. The reported 33-test pass therefore does not establish this constitutional verification obligation.

**Impact:** The foundation acceptance claim omits a required verification layer for the newly implemented parsing boundaries. This is a confirmed coverage gap; it is not a claim that a particular parser input currently causes a panic, authorization bypass or resource escape.

The later T073 fuzz campaign targets networking/policy/ancillary/lifecycle boundaries. Its presence does not replace applicable checks for parsers already introduced and accepted in Phase 2.

**Recommended correction:** Add a small property suite using the existing `proptest` dependency. Exercise bounded arbitrary malformed configuration/credential inputs and meaningful host/CIDR properties, such as canonicalization idempotence, rejection of forbidden forms, address-family normalization and range containment. Assert panic-free bounded outcomes and fixed privacy-safe errors where applicable.

This does not require creating a new fuzz framework or performing the later networking fuzz campaign now. Keep inputs and case counts proportional to a normal foundation check.

**Task effect:** Reopen T009 until the parser properties and affected native x86_64 checks pass. This finding does not by itself require reopening T005: its implementation was reviewed separately, and no corresponding functional defect was established.

## Phase 2 implementation assessment

| Task | Decision | Evidence and limits |
|---|---|---|
| T005 | **Retain `[X]`** | `src/config.rs` implements schema/version and unknown-field rejection, exactly one role, immutable validated accessors, relative paths, exposure opt-in, descriptor-based no-follow traversal, opened-inode ownership/mode checks, bounded regular-file reads, canonical token handling, PEM/key matching and encrypted-key rejection, static rule normalization, checked arithmetic and FD validation. Runtime TLS trust, authentication, DNS authorization and CLI wiring remain explicitly deferred. F2 concerns the verification task rather than a demonstrated functional defect in T005. |
| T006 | **Retain `[X]`** | `src/buffers.rs` provides owner-local checked ledgers, non-cloneable reservations, full-capacity charging for lent/cached slabs, cleared reuse, preallocated return slots, immutable checked descriptors, separate directional packet caps with combined session bytes, and atomic native count/byte reservation rollback. Descriptor metadata is distinguished from payload capacity. Runtime admission/routing, native-library integration and commitment handling remain later work. |
| T007 | **Retain `[X]`** | `src/diagnostics.rs` provides typed privacy-safe events and snapshots, opaque random-epoch IDs, fixed failure/drop/offload classes, checked direction counters, an exact event bucket, finite byte/count queue reservations including writer-held records, stdout/status versus stderr/event routing, record-boundary status priority, and bounded unsuccessful flush handling. Full terminal retention/ACK, runtime metrics and qlog are explicitly deferred. No premature claim of SC-010 acceptance is made. |
| T008 | **Change to `[ ]`** | Fixture generation, fake clock and scripted I/O are present. Generated CA/SAN/token/digest/policy material, private modes, no-overwrite behavior and distinct ephemeral-port selection are supported by the source and recorded tests. However, F1 invalidates the reusable child-group cleanup guarantee. |
| T009 | **Change to `[ ]`** | The registered 33 tests provide substantial foundation coverage, and native/container passes are documented. Acceptance remains incomplete because of F2 and the missing regression for F1. Existing successful test results should be retained as historical evidence rather than discarded or relabelled as failures. |

**Exact task-marker recommendation:** Keep T001–T007 checked. Uncheck T008 and T009. Keep T010–T079 unchecked.

**Foundation checkpoint:** Not yet fully accepted. Correct F1 and complete the narrowly scoped verification in F2, then rerun the affected checks and re-evaluate T008/T009. The design-readiness gate remains passed. Neither later interoperability evidence nor ARM64/performance qualification is a prerequisite for these corrections or for initial x86_64 development.

## Architecture, security and documentation assessment

The reviewed foundation remains aligned with the original minimal Rust/quiche architecture:

- One production package and executable are preserved. Configuration, buffer ownership and diagnostic responsibilities are separated into concrete modules.
- Buffer and resource ownership remains local through `Rc`/`RefCell`; no shared quiche mutex, per-packet task or additional runtime was introduced.
- The direct `boring`, `rustix` and `time` dependencies expose capabilities already present in the resolved graph. The lockfile changes add dependency edges without changing package versions. ADR-0005 explains their purpose and the relevant alternatives.
- Secure file traversal checks each component against a held directory descriptor. Private-file checks concern the opened inode; errors retain fixed field/reason values. The trusted provisioning-directory/owner assumption and the limitation concerning edits to an already-open inode are explicit.
- PEM parsing and certificate/key matching are accurately separated from runtime TLS authentication. The callback-invocation check correctly addresses the documented empty-password encrypted-key case.
- Retained slab capacity is accounted independently of current datagram length. Session queue bytes, descriptor metadata, native DATAGRAM quotas, kernel memory and later RSS evidence are distinguished.
- Diagnostic APIs accept typed fixed fields rather than arbitrary configuration/header/payload/peer-error strings. Queue charges remain held through a blocked write. The blocked-thread lifetime and incomplete-flush limitations are explicit.
- No QUIC/HTTP/3 forwarding, unsafe packet syscall implementation, congestion/pacing behavior, session routing, independent interoperability or performance result is presented as implemented by this foundation.

I checked the relevant locked BoringSSL wrapper and rustix source for PEM parsing, callback semantics, public-key comparison and descriptor opening. Those APIs support the described startup implementation. This source inspection is not runtime TLS or syscall qualification.

ADR-0005 provides a reasonable foundation decision record. Its parser ceilings, FD reserve and diagnostic storage allocations are explicit initial bounds, not throughput or RSS measurements. When runtime integration introduces native transport state, terminal snapshots and optional filters, their actual retained capacities must continue to fit the existing contracts; constants alone must not be presented as measured memory evidence.

The previous Phase 1 findings are addressed in the current changes: the original repository-state statements have historical/current implementation context, and the GOAWAY wording now specifies required future verification instead of saying it already ran.

No additional significant architecture or security finding was identified within the reviewed Phase 2 scope. The two findings above should be resolved without expanding the product or adding speculative infrastructure.

## Validation evidence and limitations

I inspected the tracked diff, untracked Phase 2 files, source execution paths, test registrations and relevant constitution/specification/plan/research/model/contracts/ADRs/tasks/checklists. I ran read-only `git diff --check`, which passed, and independently checked the lockfile hash:

```text
73f95573f9fa739062f98e414be62f3e1cf06b80dec65d44b80cef587e251cc7
```

I did **not** rebuild the project, rerun the Cargo suite, rebuild the container, execute hosted CI or perform networking/performance qualification during this read-only review.

| Evidence category | Status |
|---|---|
| Native x86_64 locked release build, formatting, Clippy and 33 tests | Recorded as passed by implementation; not independently rerun |
| Pinned x86_64 container and 33 tests | Recorded as passed by implementation; configuration reviewed |
| Fixture script syntax and generated material | Source/tests reviewed; previous execution recorded by implementation |
| F1 child-descendant reproduction | Main agent executed reviewer-requested standalone probe; reviewer inspected source and reported output |
| Parser fuzz/property coverage | **Missing for the implemented configuration parsers** |
| Hosted GitHub CI | Not run/verified by this review |
| Runtime authentication/DNS policy and CLI wiring | Not implemented in this increment |
| QUIC/HTTP/3/UDP forwarding and commitment lifecycle | Not implemented or qualified |
| Independent interoperability and WireGuard | Not performed |
| Native ARM64 qualification | Not performed |
| Performance, recovery and RSS acceptance | Not measured or established |

The recorded 33-test passes remain valid evidence for those tests. They do not negate the uncovered cleanup defect or satisfy absent parser properties.

`docs/validation/phase2.md:4`–5 and the Phase 2 completion statement in `tasks.md:44` should receive a current review annotation reflecting this assessment. Preserve the prior commands, results and limitations; distinguish the implementation's original acceptance claim from the subsequent independent review. This is evidence maintenance, not a reason to change the specification.

## Requirements-readiness checklist decisions

**Exact recommendation:** Retain `[x]` for every item CHK001–CHK040. Uncheck none.

These decisions certify the written requirements and their design traceability. They do not certify implemented runtime behavior, Phase 2 completion or achieved acceptance targets. F1/F2 do not remove or contradict the requirements they fail to satisfy.

Evidence abbreviations:

- **Spec:** `specs/001-masque-udp-tunnel/spec.md`
- **Plan:** `specs/001-masque-udp-tunnel/plan.md`
- **Model:** `specs/001-masque-udp-tunnel/data-model.md`
- **Amendments:** `specs/001-masque-udp-tunnel/proposed-amendments.md`
- **Wire / Config / Diagnostics / Benchmarks:** respective files under `specs/001-masque-udp-tunnel/contracts/`
- **Tasks:** `specs/001-masque-udp-tunnel/tasks.md`
- **Constitution:** `.specify/memory/constitution.md`

| ID | Decision | Evidence and conclusion |
|---|---|---|
| CHK001 | **Retain `[x]`** | Spec FR-002/005/029 and Wire define the standards, unreliable-only profile and excluded transports/features. |
| CHK002 | **Retain `[x]`** | Spec FR-006–009, Plan and ADR-0003 require verified trust, per-session authentication, default-deny authorization and no early relay access. |
| CHK003 | **Retain `[x]`** | Spec FR-012/014/028, Config and Model define active/pending/cleanup admission, queue and reclamation limits with explicit exhaustion outcomes. |
| CHK004 | **Retain `[x]`** | Spec FR-021–023 and Diagnostics specify failure classes, privacy-safe correlation, local drops and bounded post-session summaries. |
| CHK005 | **Retain `[x]`** | Spec FR-025/029, User Story 5 and Tasks require secure provisioning, operation, updates, limits and independently managed WireGuard instructions. |
| CHK006 | **Retain `[x]`** | Spec FR-010/014, Amendments R1, Plan and ADR-0001 distinguish cancellable queues, transport commitment and kernel/network traffic. |
| CHK007 | **Retain `[x]`** | Spec FR-022, Model and Wire explicitly separate acceptance/Active from destination responsiveness and delivery. |
| CHK008 | **Retain `[x]`** | Config, Model, Amendments R4 and Spec SC-006/007 distinguish application inactivity, transport liveness and closed-resource reclamation. |
| CHK009 | **Retain `[x]`** | Spec FR-019, Model and Config specify permanent mapping rejection, its restart requirement and separation from transient recovery. |
| CHK010 | **Retain `[x]`** | Spec FR-021/022/027, Diagnostics and Benchmarks distinguish offered/accepted/sent/delivered traffic and local/kernel/outer loss. |
| CHK011 | **Retain `[x]`** | Revised A1 is consistent across Spec, Amendments, Plan and ADR-0001; no fixed quarantine is prescribed. |
| CHK012 | **Retain `[x]`** | R1 closure, original-association completion, fixed output deadlines and unaffected-flow obligations agree across Spec, Model and Plan. |
| CHK013 | **Retain `[x]`** | Wire and Amendments R2 specify forbidden 204/205/206 responses and prohibited Capsule message fields before acceptance. |
| CHK014 | **Retain `[x]`** | Wire and Amendments R3 preserve mandatory structure/context/length validation before unsupported-carrier discard. |
| CHK015 | **Retain `[x]`** | Diagnostics R5 reconciles suppression, cleanup, bounded retention, ACK and explicit coverage gaps with SC-010. |
| CHK016 | **Retain `[x]`** | Spec SC-003–005 and Benchmarks define units, payload sizes, session counts and separate/simultaneous directional comparisons. |
| CHK017 | **Retain `[x]`** | Spec Assumptions and Benchmarks require a matching two-leg relay topology, bottleneck, diagnostics and generator headroom. |
| CHK018 | **Retain `[x]`** | Spec and Benchmarks require paired runs, warm-up/measurement intervals, variability, invalid-run reporting and per-run acceptance. |
| CHK019 | **Retain `[x]`** | SC-007 and Benchmarks define per-role RSS/CPU, directional load, churn timing and separate kernel-memory reporting. |
| CHK020 | **Retain `[x]`** | SC-006/008, Spec Assumptions and Plan identify timing origins, restored availability and relevant lifecycle states. |
| CHK021 | **Retain `[x]`** | SC-010 assumptions and Diagnostics require functioning collection, bounded failure workload and zero required correlation loss. |
| CHK022 | **Retain `[x]`** | User Story 1, FR-001/003/004/020 and SC-001 require generic UDP/WireGuard, bidirectionality, opaque boundaries and empty payloads. |
| CHK023 | **Retain `[x]`** | FR-006–009, SC-002 and ADR-0003 include revoked/invalid credentials, trust, forbidden/mixed/changed resolution and early data. |
| CHK024 | **Retain `[x]`** | FR-013/014/024, Plan and R1 distinguish partial progress, blockage, cancellation and pre/post-commitment close. |
| CHK025 | **Retain `[x]`** | User Story 4, FR-017–019 and SC-006 define blackhole/restart/address-change recovery, retained listeners, reauthorization and no replay. |
| CHK026 | **Retain `[x]`** | Config and Spec define old-process exit plus replacement readiness, affected sessions, deliberate rollback policy and committed-traffic limits. |
| CHK027 | **Retain `[x]`** | A1/R1, Spec Assumptions and ADR-0001 distinguish endpoint reuse ambiguity, stale generation rejection and committed output. |
| CHK028 | **Retain `[x]`** | Wire/R3 explicitly distinguish unknown/truncated contexts and Capsules, size semantics, final truncation and continuation after valid skip. |
| CHK029 | **Retain `[x]`** | FR-004/005/015/016 and Wire distinguish preserved empty/supported datagrams, best-effort oversize drops and prohibited fragmentation/fallback. |
| CHK030 | **Retain `[x]`** | FR-011–013/028, SC-009 and Edge Cases define saturation progress/isolation without promising availability under external link exhaustion. |
| CHK031 | **Retain `[x]`** | A2, Spec Assumptions and Plan consistently preserve numeric targets as release criteria rather than development prerequisites. |
| CHK032 | **Retain `[x]`** | A2, Benchmarks and Tasks require an early authenticated E2E MVP and immediate reproducible measurements in normal implementation. |
| CHK033 | **Retain `[x]`** | Plan, research and ADR-0002 treat initial tuning/optional optimizations as hypotheses and preserve correctness, fairness and pacing. |
| CHK034 | **Retain `[x]`** | FR-006–009, Spec Assumptions and ADR-0003 describe tunnel/destination trust boundaries and prohibit security-for-performance tradeoffs. |
| CHK035 | **Retain `[x]`** | FR-021–023, Diagnostics and Plan bound every diagnostic mode, privacy, retention/output and optional qlog sequencing. |
| CHK036 | **Retain `[x]`** | Research, Plan, locked dependencies and ADR-0005 document versioned API choices and distinguish source evidence from runtime qualification. |
| CHK037 | **Retain `[x]`** | FR-026, Benchmarks and Tasks require both-role independent interoperability and explicit unavailable-peer limitations. |
| CHK038 | **Retain `[x]`** | A2, Spec, Plan and ADR-0004 permit x86_64 development while retaining native ARM64 evidence before a support claim. |
| CHK039 | **Retain `[x]`** | Amendments, Spec clarifications, contracts and task traceability preserve A1/A2/R1–R6 and separate design readiness from runtime evidence. |
| CHK040 | **Retain `[x]`** | Constitution, R6 and Tasks consistently require behavioral verification with optional TDD ordering and a limited foundation. F2 is a failure to complete that written obligation, not an inconsistency in it. |

## Existing specification-quality checklist decisions

**Exact recommendation:** Retain all 16 `[x]` marks in `checklists/requirements.md`. Uncheck none.

| Criterion | Decision | Conclusion |
|---|---|---|
| No implementation details (languages, frameworks, APIs) | **Retain `[x]`** | Spec defines product constraints and observable behavior; Rust/quiche/runtime/config implementation choices remain in planning artifacts. |
| Focused on user value and business needs | **Retain `[x]`** | Stories describe secure forwarding, isolation, performance, recovery and operation. |
| Written for non-technical stakeholders | **Retain `[x]`** | User stories and acceptance outcomes remain understandable; protocol constraints are necessary product requirements. |
| All mandatory sections completed | **Retain `[x]`** | Clarifications, scenarios, requirements, entities, success criteria and assumptions are present. |
| No unresolved clarification markers remain | **Retain `[x]`** | Accepted decisions replace the previous ambiguities; no unresolved clarification marker was identified. |
| Requirements are testable and unambiguous | **Retain `[x]`** | Requirements identify observable outcomes, boundaries and documented limitations. |
| Success criteria are measurable | **Retain `[x]`** | SC-001–010 and associated methodology define workloads, thresholds, timing and collected evidence. |
| Success criteria are technology-agnostic (no implementation details) | **Retain `[x]`** | Acceptance concerns user-visible behavior and resource/performance outcomes. |
| All acceptance scenarios are defined | **Retain `[x]`** | The five stories define success, denial, isolation, overload, recovery and operation scenarios. |
| Edge cases are identified | **Retain `[x]`** | Spec covers empty/oversize/malformed traffic, reuse, wrong-source replies, setup failure, churn, output blockage and shutdown. |
| Scope is clearly bounded | **Retain `[x]`** | FR-029 and transport/carrier constraints explicitly exclude unrelated proxy/VPN/application management. |
| Dependencies and assumptions identified | **Retain `[x]`** | Trust provisioning, external connectivity, peers, reference topology, platform evidence and UDP provenance limits are explicit. |
| All functional requirements have clear acceptance criteria | **Retain `[x]`** | Stories, SC outcomes and contract/task traceability provide verifiable behavior for FR-001–029. |
| User scenarios cover primary flows | **Retain `[x]`** | Secure forwarding, concurrent isolation, overload, recovery and operator lifecycle are covered. |
| Feature meets measurable outcomes defined in Success Criteria | **Retain `[x]`** | Under the checklist's documented requirements-quality semantics, the specification defines measurable intended outcomes. This mark does not claim they have been achieved. |
| No implementation details leak into specification | **Retain `[x]`** | Detailed API, ownership, build and configuration decisions remain in research, Plan, contracts and ADRs. |

## Recording recommendations for the main agent

1. Preserve this report, including the findings, conclusions, evidence attribution and limitations.
2. Reconfirm all 40 readiness and 16 specification-quality marks with a reference to this review. Keep their existing requirements-quality semantics.
3. Reopen T008 and T009 and record F1/F2 as the remaining foundation work. Retain T005–T007 and the previously justified Phase 1 marks.
4. Annotate the existing Phase 2 acceptance/completion claims with this subsequent review outcome while preserving the recorded 33-test passes.
5. Do not turn these checklist updates into a claim of runtime acceptance, independent interoperability, performance or ARM64 support.

**Final assessment:** Written requirements remain ready for incremental implementation. Phase 2 acceptance remains incomplete until the owned-descendant cleanup defect and the scoped parser-verification gap are resolved.
