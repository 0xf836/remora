# Remora Constitution

## Core Principles

### I. Standards Compliance and Interoperability

Remora MUST implement networking protocols according to their applicable published standards.
Each protocol feature MUST identify the specification and version it implements, relevant
normative requirements, and any supported extensions or known limitations. Protocol behavior
MUST be verified with conformance cases and interoperability tests against an independent
implementation where one is available. When no independent implementation is available, the
limitation and alternative verification evidence MUST be documented.

Extensions and intentional deviations MUST be explicitly documented and tested, with their
compatibility implications stated. Remora MUST NOT silently substitute incompatible behavior
or claim compliance beyond what its implementation and verification support. Standards-based
behavior makes interoperability predictable and exposes assumptions to review.

### II. Correctness Before Performance

Implementations MUST define and preserve their protocol, state-machine, ownership, and
concurrency invariants. Externally supplied data MUST be validated before it affects trusted
state. Malformed input, cancellation, timeouts, partial progress, and resource exhaustion MUST
have explicit, tested outcomes appropriate to the affected feature.

Recoverable failures MUST be represented by errors rather than uncontrolled panics. Changes
MUST preserve required ordering, lifecycle, and error-handling behavior. An optimization MUST
NOT weaken correctness guarantees, even when doing so improves a benchmark. Correct behavior
is a prerequisite for useful networking performance.

### III. Security by Design

Network input and peer behavior MUST be treated as untrusted. Features that introduce or change
trust boundaries MUST document their threat model, security assumptions, and mitigations.
Implementations MUST enforce bounded resource use, validate lengths and identifiers, and fail
closed when a required security check cannot be completed.

Cryptographic operations MUST use established, maintained libraries and protocol-approved
constructions. Dependencies MUST be reviewed for necessity, maintenance, and known security
issues. An applicable known vulnerability MUST be remediated or mitigated before release; the
mitigation and remaining exposure MUST be recorded for review.

Secrets, credentials, and sensitive traffic contents MUST NOT appear in routine diagnostics.
Security-relevant changes MUST receive explicit security review and negative tests. Performance
goals MUST NOT justify bypassing authentication, authorization, validation, or confidentiality
requirements that apply to a feature.

### IV. Idiomatic Rust, Simplicity, and Maintainability

Rust code MUST use ownership, borrowing, types, and explicit error handling to express invariants.
Public APIs MUST document their contracts, errors, and lifecycle expectations. Modules and
abstractions MUST have a concrete responsibility justified by current requirements; speculative
frameworks and unnecessary indirection MUST NOT be introduced.

Safe Rust is the default. Any `unsafe` code MUST have a documented necessity, local safety
invariants, and targeted verification; a claimed speed benefit alone is insufficient evidence.
Dependency additions MUST be justified against a simpler implementation or existing capability.

Architectural decisions affecting protocol behavior, trust boundaries, concurrency, public APIs,
dependencies, or significant performance tradeoffs MUST be recorded in an architectural decision
record (ADR). Each ADR MUST state the context, options considered, decision, and consequences.
Simple, explicit designs reduce the cost of auditing and future changes.

### V. Comprehensive Automated Verification

Every behavioral change MUST include automated tests covering its contract and relevant failure
modes. Bug fixes MUST include a regression test that demonstrates the defect before the fix
where reproducible; any inability to reproduce it automatically MUST be documented with
alternative evidence. Tests MUST assert externally meaningful behavior and invariants, not
merely mirror implementation details.

The test suite MUST include unit tests, protocol and component integration tests, and end-to-end
tests for supported networking paths. Parsers and untrusted-input boundaries MUST have malformed
input coverage and fuzz or property-based tests. Concurrent and asynchronous behavior MUST be
tested for applicable cancellation, shutdown, ordering, and race scenarios.

Tests MUST be reproducible, and flaky failures MUST be investigated rather than ignored.
Automated checks MUST run in continuous integration. Static analysis and unit tests MUST NOT be
presented as proof of live interoperability, deployment readiness, or end-to-end performance.

### VI. Actionable Observability

Networking behavior MUST be diagnosable through structured logs and relevant metrics covering
connection lifecycle, protocol errors, resource pressure, and performance bottlenecks. Diagnostic
events MUST provide enough context to correlate an operation without exposing secrets or
unnecessarily identifying users.

Log levels MUST distinguish routine activity from actionable failures. High-frequency events
MUST use suitable levels, sampling, aggregation, or rate limits to bound diagnostic overhead.
Metrics MUST avoid unbounded label cardinality. Tests or measurements MUST verify these bounds
when a change affects a high-volume path. Observability MUST preserve protocol behavior and
provide evidence for debugging and performance work.

### VII. Measurable Performance

Performance requirements MUST define the workload, relevant metrics, and acceptance criteria.
Optimization work MUST begin with profiling or measurements that identify a bottleneck.
Performance claims MUST include a reproducible baseline and comparison, recording hardware,
software versions, configuration, workload, and measurement method.

Measurements MUST account for variability and relevant tradeoffs such as throughput, latency,
CPU use, memory use, and behavior under load or loss. A faster isolated microbenchmark MUST NOT
be treated as proof of an end-to-end improvement. Changes to critical paths MUST be checked
against applicable performance baselines, with regression tolerances established for the
workload rather than invented after observing results.

Premature optimization MUST NOT introduce complexity without evidence of a current need.
Correctness and security checks MUST continue to pass for every optimization; neither guarantee
may be traded for speed.

## Engineering Constraints

Remora is a security-focused, high-performance, open-source networking project implemented in
Rust. Specifications, architectural decisions, development instructions, and verification
methods MUST remain accessible in the repository so contributors can reproduce and review work.

- Supported platforms, toolchains, protocol versions, and compatibility commitments MUST be
  explicitly documented as they are introduced or changed.
- Configuration defaults MUST preserve applicable security guarantees. Resource limits and
  operational failure behavior MUST be documented for network-facing functionality.
- Public API, configuration, and wire-compatibility changes MUST identify their impact and
  provide migration guidance when existing users are affected.
- Benchmarks and diagnostic tooling MUST state what they measure and their limitations.
  Experimental behavior MUST be explicitly identified and MUST NOT silently change supported
  protocol or security guarantees.

## Development Workflow and Quality Gates

Before implementation, feature specifications and plans MUST identify applicable standards,
invariants, security risks, verification requirements, and performance criteria. The level of
detail MUST match the scope and risk of the change; small changes need no unnecessary ceremony.

Each pull request MUST provide its purpose, behavior change, relevant documentation or ADRs,
and verification evidence. Review MUST assess compliance with every applicable principle,
including security and compatibility implications. Exceptions and unresolved concerns MUST be
explicit; missing evidence MUST NOT be described as a successful check.

Before merge, affected Rust code MUST pass formatting checks, Clippy with warnings treated as
errors, and the applicable automated test suite. Targeted lint allowances MUST document their
reason. Protocol, security, unsafe-code, or performance-sensitive changes MUST also complete
their applicable conformance, negative, fuzz/property, safety, or benchmark checks. Any checks
that cannot run in continuous integration MUST have a documented execution procedure and
recorded results before a release that depends on them.

Reviewers MUST reject changes that compromise correctness or security for performance. Releases
MUST document known limitations and material compatibility changes, and MUST have evidence for
the supported behavior they claim.

## Governance

This constitution is the authoritative source of Remora's engineering principles. Project
guidance, specifications, plans, and reviews MUST comply with it. Maintainers MUST resolve
conflicts in favor of the constitution and record the resolution.

Amendments MUST be proposed as a reviewed repository change with a rationale, affected
principles, compatibility impact, and migration or adoption steps where needed. A project
maintainer MUST approve the amendment before merge. The amended constitution MUST update its
version and last-amended date while preserving the original ratification date.

Constitution versions follow semantic versioning: MAJOR for incompatible principle or governance
removals or redefinitions, MINOR for new principles or materially expanded obligations, and PATCH
for clarifications that do not change obligations. The initial ratified version is 1.0.0.

Every feature plan and pull-request review MUST check applicable constitutional requirements.
Temporary exceptions to procedural requirements MUST document the reason, scope, responsible
maintainer, compensating evidence, and expiration or remediation condition, and MUST receive
maintainer approval. Exceptions MUST NOT waive protocol correctness or security guarantees.
Maintainers MUST recheck compliance when preparing releases and revisiting architectural decisions.

**Version**: 1.0.0 | **Ratified**: 2026-10-10 | **Last Amended**: 2026-10-10
