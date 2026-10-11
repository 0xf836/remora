# Independent review: Phase 1 and requirements readiness

**Date:** 2026-10-11  
**Reviewed state:** Working tree based on commit `855d5ba`, including the uncommitted Phase 1 changes.  
**Review mode:** Read-only. No project files, checklist markers, task states, or commits were changed.

**Conclusion:** No significant correctness or security defect was identified in the Phase 1 infrastructure. T001–T004 can remain complete. All 40 readiness criteria are satisfied as **requirements-quality criteria**, and the 16 existing specification-quality marks remain justified. Two low-severity documentation findings should be corrected to prevent stale or premature evidence claims.

These conclusions do not establish forwarding correctness, interoperability, performance, hosted CI success, or ARM64 support. The implementation currently consists of package/build/CI infrastructure and an executable that reports that tunnel commands are not implemented.

## Findings

### F1 — LOW: Historical repository-state statements are now stale

**Locations:**

- `specs/001-masque-udp-tunnel/plan.md:102`: says the repository has no Cargo implementation and leaves `tasks.md` to task generation.
- `specs/001-masque-udp-tunnel/checklists/readiness.md:90`: says `tasks.md` does not yet exist.
- `docs/adr/0004-verification-and-performance.md:7`: says the repository currently has no executable.

**Evidence:** `Cargo.toml`, `Cargo.lock`, `src/main.rs`, and `tasks.md` now exist, and T001–T004 are recorded as complete. `docs/build.md:78`–82 accurately describes the skeleton executable and zero-test verification scope.

**Impact:** Readers can misunderstand whether these statements describe the original planning snapshot or the current repository. This does not invalidate the selected architecture, checklist criteria, or Phase 1 completion.

**Suggested correction:** Label the statements as historical planning context or update the current-state descriptions. Preserve the distinction between an infrastructure skeleton and a working tunnel.

### F2 — LOW: The wire contract uses completed-test wording for planned GOAWAY verification

**Location:** `specs/001-masque-udp-tunnel/contracts/masque-wire.md:53`.

**Problem:** The sentence says the first transport/MVP tests “already verify” the initial GOAWAY behavior. The corresponding tasks remain unchecked, and the present test targets contain no behavioral tests.

**Evidence:** `tasks.md` leaves T018/T019 and the MVP acceptance tasks incomplete. All five current integration test entry points contain only documentation comments. `docs/build.md:78`–80 explicitly records zero tests.

**Impact:** Read alone, this sentence can be mistaken for existing verification evidence. The surrounding contract and task list make the intended future obligation clear, so this is a wording defect rather than a missing GOAWAY requirement.

**Suggested correction:** State that the first transport/MVP tests **must verify** this behavior before acceptance. Retain the later replacement-connection checks as separate recovery obligations.

## Phase 1 implementation assessment

| Task | Conclusion | Evidence and limits |
|---|---|---|
| T001 | **Complete; retain `[X]`** | `Cargo.toml:1`–39 defines one package, edition 2024, Rust 1.99, exact quiche/Tokio pins, justified dependencies, optional qlog disabled by default, and declared initial targets. `rust-toolchain.toml` pins 1.99.0 and required components. `src/lib.rs` rejects unsupported platform families. `Cargo.lock` resolves quiche 0.30.0, Tokio 1.53.2, and boring/boring-sys 5.2.0; registry packages have checksum entries. No reactor or forwarding behavior is claimed. |
| T002 | **Complete; retain `[X]`** | `tools/build/Dockerfile` pins the Rust base image by digest and added Debian packages through a fixed snapshot, installs the native compilation prerequisites, and records package inventory. `docs/build.md:9`–25 explains the Rust/native inputs and bundled BoringSSL source. Lines 33–62 give locked native/container procedures and explicitly avoid a bit-for-bit reproducibility claim. Recorded local checks are evidence supplied by implementation, not independently reproduced by this reviewer. |
| T003 | **Complete; retain `[X]`** | `.github/workflows/ci.yml:15`–37 pins checkout, uses read-only repository permissions, disables persisted checkout credentials, checks native x86_64, builds the pinned container, and runs locked build/format/Clippy/test commands. Five integration entry points exist. The workflow explicitly identifies separate qualification and empty-test limitations. Creating this workflow does not require a successful hosted run to satisfy this setup task; hosted execution remains unverified. |
| T004 | **Complete; retain `[X]`** | `docs/adr/0002-runtime-and-linux-io.md:34`–64 records exact upstream source blobs, supported patterns, missing metadata/truncation/error handling, and the narrow local-adapter decision. I independently retrieved the tagged manifest, mmsg implementation, and socket traits; the three hashes match the ADR. The source supports the stated reasons for rejecting unchanged adoption. This is source/API evidence, not a safety certification or performance result. |

The source assessment is supported by Cloudflare's tagged [manifest](https://github.com/cloudflare/quiche/blob/0.30.0/datagram-socket/Cargo.toml), [mmsg implementation](https://github.com/cloudflare/quiche/blob/0.30.0/datagram-socket/src/mmsg.rs), and [socket traits](https://github.com/cloudflare/quiche/blob/0.30.0/datagram-socket/src/datagram.rs). The reviewed mmsg implementation uses batches of 16, supplies no per-message address/control storage, does not reject receive truncation through returned flags, and returns accumulated progress when a subsequent syscall fails. The selected adapter still needs T016/T017 verification before acceptance.

**Task-marker recommendation:** Do not uncheck any of T001–T004. Leave T005–T079 unchecked.

## Validation evidence and limitations

I inspected the actual package, source skeleton, lockfile, workflow, Dockerfile, ignore files, test entry points, Phase 1 diff, and the relevant specification/design artifacts. `git diff --check` passed. I parsed the Cargo manifest/lock information and independently checked the three upstream datagram-socket blobs recorded in ADR-0002.

I did **not** rebuild the project, rerun Cargo tests, rebuild the container, or execute hosted CI during this read-only review. Native/container build success is recorded in `docs/build.md:69`–82 and is accepted as implementation-supplied evidence with that limitation.

| Evidence category | Current status |
|---|---|
| Native x86_64 build, formatting, Clippy, test-target execution | Recorded as passed by implementation; not independently rerun |
| Pinned build image and unprivileged container checks | Recorded as passed by implementation; configuration reviewed |
| Behavioral automated tests | **Zero implemented tests** |
| Hosted GitHub CI | Not run/verified by this review |
| Linux UDP adapter behavior and unsafe-code safety | Not implemented or qualified |
| Authenticated end-to-end forwarding | Not implemented |
| Independent interoperability | Not performed |
| WireGuard application compatibility | Not performed |
| Native ARM64 qualification | Not performed |
| Performance/recovery/resource acceptance | Not measured or established |

There is no need to complete later networking phases to accept the setup increment. Its zero-test status must continue to be reported accurately.

## Requirements-readiness checklist decisions

The evidence abbreviations below identify repository files:

- **Spec:** `specs/001-masque-udp-tunnel/spec.md`
- **Plan:** `specs/001-masque-udp-tunnel/plan.md`
- **Model:** `specs/001-masque-udp-tunnel/data-model.md`
- **Amendments:** `specs/001-masque-udp-tunnel/proposed-amendments.md`
- **Wire / Config / Diagnostics / Benchmarks:** respective files in `specs/001-masque-udp-tunnel/contracts/`
- **Tasks:** `specs/001-masque-udp-tunnel/tasks.md`
- **Constitution:** `.specify/memory/constitution.md`
- **Template:** `.specify/templates/tasks-template.md`
- **Generation skill:** `.agents/skills/speckit-tasks/SKILL.md`

**Marker recommendation:** All CHK001–CHK040 may be marked `[x]`. Each decision concerns the completeness and quality of written requirements. It does not certify their implementation.

| ID | Decision | Repository evidence and reviewer conclusion |
|---|---|---|
| CHK001 | **Satisfied** | Spec FR-002/005/029 at lines 221–232 and 325 onward; Wire lines 5–9 and 37–45. CONNECT-UDP/H3/QUIC scope and the unreliable-only profile are explicit; reliable capsules are validated but not forwarded. |
| CHK002 | **Satisfied** | Spec lines 233–247; Config lines 63 and 108; ADR-0003, “Context and threat model” and “Decision.” Verified server trust, per-request credentials, default-deny target policy, and no early relay access are specified. |
| CHK003 | **Satisfied** | Spec lines 263–278 and FR-028; Config lines 67–106; Model lines 35–47; Plan lines 141–147. Active/pending/cleanup accounting, queue/setup/idle limits, and rejection/drop outcomes are required. |
| CHK004 | **Satisfied** | Spec lines 298–308; Diagnostics lines 5–23 and 25–44. Fixed failure classes, opaque correlation, drop accounting, bounded terminal retention, and coverage-loss reporting are stated together. |
| CHK005 | **Satisfied** | Spec User Story 5 and lines 312–315; FR-029; Config lines 112–118; quickstart lines 90–96. Operator documentation includes secure provisioning, permissions, updates, limits, and independently managed WireGuard. |
| CHK006 | **Satisfied** | Spec lines 269–278; Plan lines 133–139; Model lines 29–45. Cancellable queues, generated ciphertext commitment, syscall submission, and remote delivery are distinct. Recall limitations are explicit. |
| CHK007 | **Satisfied** | Spec lines 301–304; Model lines 23–27; Wire line 29. Active transport/session acceptance is explicitly separated from destination responsiveness and successful delivery. |
| CHK008 | **Satisfied** | Config lines 84, 95–97 and 110; Model lines 29–33; Spec SC-006/007. Valid UDP inactivity, transport liveness/probes, and post-close reclamation have separate meanings. Shorter idle overrides are documented deviations. |
| CHK009 | **Satisfied** | Spec lines 291–294; Model line 31; Config line 116; Plan line 147. A fixed mapping latch survives flow cleanup/new sender tuples/transient recovery until controlled restart. |
| CHK010 | **Satisfied** | Diagnostics lines 27–44; Benchmarks lines 55–72. Queue acceptance, socket transmission, harness delivery, local/kernel drops, and outer QUIC estimates are distinguished. |
| CHK011 | **Satisfied** | Spec lines 248–259 and Assumptions; Amendments lines 5–13; Model lines 43–47; ADR-0001 isolation section. Enforceable ownership/isolation and indistinguishable UDP provenance limits coexist without prescribed quarantine. |
| CHK012 | **Satisfied** | Spec lines 266–278; Plan lines 133–145; Model lines 40–45; Amendments R1. Close purges cancellable work, preserves committed output only on its original association/deadline, and does not invent per-session ciphertext recall. |
| CHK013 | **Satisfied** | Wire lines 25–29; Amendments R2. Final 204/205/206 and forbidden Capsule fields are rejected before forwarding, with malformed-message handling specified. |
| CHK014 | **Satisfied** | Wire lines 37–45; Amendments R3. Context/length/truncation validation precedes unsupported-carrier discard; bounded skipping and subsequent parsing are required. |
| CHK015 | **Satisfied** | Spec SC-010 and diagnostic Assumptions; Diagnostics lines 17–23; Amendments R5. Ordinary suppression does not waive correlation; overflow/failed collection cannot count as success. |
| CHK016 | **Satisfied** | Spec SC-003/004/005; Benchmarks lines 51–70. Units, payload/session combinations, separate directions, simultaneous per-direction outcomes, and loss/RTT load are explicit. |
| CHK017 | **Satisfied** | Benchmarks lines 51–57; Spec reference Assumptions. Baseline uses the same two-leg role locations, route, bottleneck, configuration/diagnostic conditions, and verified generator headroom. |
| CHK018 | **Satisfied** | Spec benchmark Assumptions; Benchmarks lines 55–57 and 72–76. Five paired runs, warmup/interval, variability, invalid generators, per-run acceptance, and immutable failures are specified. |
| CHK019 | **Satisfied** | Spec SC-007; Benchmarks lines 67 and 72. Per-role peak RSS, averaged CPU, simultaneous load, churn deadline, retained capacity, and separately reported kernel memory are defined. |
| CHK020 | **Satisfied** | Spec SC-006/008 and recovery Assumptions; Plan line 147; Benchmarks lines 66–68. Failure onset/restored availability/stop request are timing origins, with explicit profiles and states. |
| CHK021 | **Satisfied** | Diagnostics lines 17–23; Benchmarks lines 70 and 78; Spec diagnostic Assumptions. Functioning collector, bounded injection workload, all class/session correlations, and zero coverage loss are distinct from overflow robustness. |
| CHK022 | **Satisfied** | Spec User Story 1 and lines 218–232, 295–297; Wire lines 31–35; quickstart lines 90–92. Generic UDP and WireGuard use identical opaque bidirectional semantics, empty payloads, boundaries, and supported address families. |
| CHK023 | **Satisfied** | Spec lines 233–247 and SC-002; ADR-0003 “Consequences and review obligations”; Benchmarks line 33. Credential/trust/policy/DNS/early-data negative scenarios and zero unauthorized traffic are required. |
| CHK024 | **Satisfied** | Spec lines 209–211 and 266–278; Plan lines 133–139; Benchmarks line 27. EAGAIN/partial progress/close/cancellation have distinct pre- and post-commitment obligations and multi-stream tests. |
| CHK025 | **Satisfied** | Spec User Story 4 and lines 285–294; Plan line 147; quickstart line 61. Blackhole/restart/address-change/longer-outage recovery retains the listener, reauthorizes, bounds retries, and drops unavailable traffic. |
| CHK026 | **Satisfied** | Spec FR-006/009/025 and Assumptions; Config lines 112–116; quickstart line 96. Old-process exit plus replacement readiness defines update completion; rollback may intentionally restore policy; committed traffic cannot be recalled. |
| CHK027 | **Satisfied** | Spec User Story 2 scenario 4, FR-010/014 and UDP Assumptions; Amendments A1/R1; Model lines 31–33. External endpoint-reuse ambiguity is separated from identifiable stale state and original-association output. |
| CHK028 | **Satisfied** | Wire lines 31–45 and 51; Amendments R3. Quarter-stream/context distinctions, unknown/truncated fields, 65527-byte semantics, incomplete FIN, and continuation after valid skip are explicit. |
| CHK029 | **Satisfied** | Spec lines 227–232 and 279–284; Wire lines 33–35; Spec payload Assumptions. Empty preservation, dynamic effective limits, counted best-effort oversize loss, RFC size violations, and no fragmentation/fallback are distinguished. |
| CHK030 | **Satisfied** | Spec lines 260–278, SC-009 and Edge Cases; Plan resource design; Benchmarks line 69. Bounds/isolation/admitted-flow progress apply under process-budget saturation, with external link exhaustion excluded from availability guarantees. |
| CHK031 | **Satisfied** | Spec numeric-target Assumptions; Amendments lines 15–23; Plan lines 27 and 47; Benchmarks lines 3–13. Numeric targets remain release criteria without a pre-implementation performance gate. |
| CHK032 | **Satisfied** | Benchmarks lines 5–13; Tasks T033/T034 and lines 76–80; Amendments A2. Early authenticated MVP and immediate reproducible measurements are normal increments and inform subsequent decisions. |
| CHK033 | **Satisfied** | Plan “Complexity Tracking”; ADR-0002 lines 22–30 and 66–72; Benchmarks lines 80–86. Single-owner capacity and optimizations are hypotheses; measurements preserve bounds/fairness/pacing/correctness. |
| CHK034 | **Satisfied** | Spec security requirements and trust-boundary Assumption; ADR-0003 threat model/decision; Constitution III/VII. Tunnel confidentiality is separated from ordinary destination UDP, and performance cannot waive security. |
| CHK035 | **Satisfied** | Diagnostics lines 46–54; Plan verification sequencing; ADR-0003 diagnostic corrections; Constitution VI. All-level redaction, bounded tracing/state/output, explicit coverage gaps, and post-MVP qlog sequencing are required. |
| CHK036 | **Satisfied** | Research lines 7–13, 109–125; Plan technical context; ADR-0002 lines 34–64; docs/build.md. Versions/sources/rationale and outstanding validation are documented without equating source inspection with runtime evidence. |
| CHK037 | **Satisfied** | Spec lines 316–319; Benchmarks lines 37–47; Tasks T035/T036. Both roles, independent wire implementations, profile limits, peer hashes, actual traffic evidence, and unavailable-peer limitations are explicit. |
| CHK038 | **Satisfied** | Spec platform Assumption; Plan line 23 and verification sequencing; Benchmarks lines 13 and 35; ADR-0004 decision. x86_64 begins first; native ARM64 evidence precedes support claims; cross-builds do not substitute. |
| CHK039 | **Satisfied** | Spec clarifications; Amendments lines 25–56; Plan lines 33–49 and traceability table; Research lines 123–127. A1/A2/R1–R6 are traceable, and design readiness is distinguished from pending experiments/release evidence. F1/F2 are localized documentation wording issues, not missing traceability or a release-evidence pass. |
| CHK040 | **Satisfied** | Constitution V and quality gates; Template test policy; Generation skill lines 142 and 202–210; Tasks lines 7 and 32–42. Behavioral tests are mandatory, TDD ordering optional, and foundation scope limited to actual MVP prerequisites. |

The response/Capsule distinctions in CHK013/014/028 are consistent with the normative requirements in [RFC 9297 §§3.2–3.5](https://www.rfc-editor.org/rfc/rfc9297.html#section-3.2), including forbidden fields/statuses, malformed/incomplete-message handling, unknown Capsule skipping, and stream lifecycle rules.

**Readiness totals:** 40 satisfied; 0 not satisfied; 0 not assessed.

## Existing specification-quality checklist decisions

The criteria below follow the order in `checklists/requirements.md`. Retain all 16 existing `[x]` marks.

| # | Criterion | Decision and evidence |
|---|---|---|
| 1 | No implementation details: languages/frameworks/APIs | **Satisfied.** Spec requires the user-selected Linux/MASQUE/H3/QUIC product constraints but leaves implementation language/library/runtime/config/authentication mechanisms to planning; see Spec Assumptions. |
| 2 | Focused on user value and business needs | **Satisfied.** Five operator/application stories describe secure forwarding, isolation, throughput, recovery, and diagnosis. |
| 3 | Written for non-technical stakeholders | **Satisfied in project context.** The intended audience is Linux operators; stories explain observable outcomes. Required standards vocabulary is appropriate to this networking product and does not select code APIs. |
| 4 | All mandatory sections completed | **Satisfied.** User scenarios, requirements, success criteria, and assumptions are present and substantive. |
| 5 | No unresolved clarification markers | **Satisfied.** A1/A2 decisions are integrated, with no unresolved clarification placeholder in the specification. |
| 6 | Requirements testable and unambiguous | **Satisfied.** FR-001–029 define observable positive/negative behavior; commitment, provenance, recovery, and diagnostic limits are resolved through referenced contracts. |
| 7 | Success criteria measurable | **Satisfied.** SC-001–010 and reference Assumptions define outcomes, units, workloads, thresholds, durations, and timing origins. |
| 8 | Success criteria technology-agnostic | **Satisfied.** Criteria measure application delivery, isolation, security, timing, resources, and operation; no library/runtime API determines acceptance. |
| 9 | All acceptance scenarios defined | **Satisfied.** Each of the five stories has given/when/then scenarios and an independent-test description. |
| 10 | Edge cases identified | **Satisfied.** Spec lines 186–212 cover stale/reused associations, malformed input, size/path limits, silence, outage, pressure, partial progress, and shutdown. |
| 11 | Scope clearly bounded | **Satisfied.** FR-029 and Assumptions exclude CONNECT-IP/TUN/VPN management, GUI, alternative transports, and unrelated services. |
| 12 | Dependencies and assumptions identified | **Satisfied.** Trust provisioning, destination access, network paths, reference hosts, independent peers, and platform qualification are explicit. |
| 13 | Functional requirements have clear acceptance criteria | **Satisfied.** Stories, SC criteria, and contract matrices connect positive/negative behavior to FR-001–029. |
| 14 | User scenarios cover primary flows | **Satisfied.** Forwarding, concurrency, performance, recovery, and operation/revocation are covered. |
| 15 | Feature meets measurable outcomes defined in Success Criteria | **Satisfied only under the file's declared requirements-quality semantics.** The written feature defines outcomes and verification; this mark does not assert that an implementation has achieved them. Actual SC acceptance remains pending. |
| 16 | No implementation details leak into specification | **Satisfied.** Implementation structure and library/API selections reside in plan/research/ADRs/contracts, not in the specification's product acceptance criteria. |

**Specification-quality totals:** 16 satisfied; 0 not satisfied; 0 not assessed.

## Remaining risks and open evidence

The one-owner reactor's throughput/headroom, fairness of shared native DATAGRAM queues, actual heap/BoringSSL costs, recovery timing, syscall safety, and optional diagnostic overhead remain unmeasured. The artifacts correctly identify these as questions for subsequent implementation and qualification. They are not confirmed defects and do not justify introducing a separate feasibility prototype or ARM64 development gate.

The current version pins and existing advisory discussion do not establish a current complete vulnerability assessment of the resolved Rust/native graph. The documented dependency-security and release-review obligations remain applicable.

Phase 1 can remain accepted for its limited infrastructure scope. Recording the satisfied requirements checklists should allow subsequent phase execution without suggesting that the tunnel or release criteria have passed.
