# ADR 0024: Defer ARM64 outside the current release requirements

Date: 2026-10-17.
Status: accepted; explicitly requested by the project owner.

## Context

The original plan included Linux GNU x86_64 and ARM64. Native x86_64 functional,
security, interoperability, duration and bounded reference qualification is
complete. ARM64 has a prepared CI workflow/reference profile but no qualifying
native execution. The owner requested removing ARM64 from requirements for now.

## Decision

Linux 6.1+ GNU **x86_64 is the sole current required release platform**.
ARM64 native builds, tests, benchmarks and T072 are deferred outside the active
requirements, completion criteria and Phase 8 dependency graph. They are not
waived failures, passed checks, or missing work that blocks this scope.
T073–T079 remain complete for the recorded x86_64 profile.

Keep the prepared ARM64 reference profile and native workflow as experimental
future tooling. The workflow is manual-only (`workflow_dispatch`); no push/PR
trigger or required ARM64 job is introduced. Optional ARM64 experiments do not
imply support or reopen current acceptance.

Do not remove existing build-compatible source paths, architecture-aware test
validators or Cargo target hints merely to narrow requirements. They are not
support commitments. In particular, an architecture list in `--version` or
Cargo metadata does not mean that every listed architecture is qualified.
No runtime source, root lockfile, numerical threshold or measured artifact is
changed by this scope decision.

Reintroducing ARM64 requires an explicit future scope decision and native
build/syscall/FFI/runtime and applicable performance/interop evidence. A
cross-build or prepared workflow alone is insufficient for a support claim.
There is no scheduled ARM64 delivery commitment in the current feature.

## Consequences and verification

The active specification, plan, benchmark contract, task graph and operator
documents describe x86_64 requirements consistently. Retain the T072 identifier
as deferred text rather than checking an unexecuted task or reusing its number.
Historical ADRs, research, review checklists and measurements retain their dated
scope; this decision supersedes their original ARM64 commitment. Reviewer-owned
checklist markers are unchanged and do not constitute a fresh review.

All security, correctness and SC-001–SC-010 numerical requirements for x86_64
remain in force. The constitution requires evidence for claimed platforms and
is unchanged. This is a platform scope reduction authorized by the owner,
not a reinterpretation of a failed ARM64 measurement as success.

Verify active document links/scope, manual-only workflow syntax, current task
dependencies and unchanged production/binary hashes. Runtime/VM/performance
reruns are unnecessary for this requirements/CI-trigger change. Previously bound
reports and snapshots remain historical; new scope metadata must not replace
original run inputs or statuses.
