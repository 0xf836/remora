# Agent Guide

## Project references

- Follow the [constitution](.specify/memory/constitution.md), the authority for engineering and review requirements.
- Consult the [specification](specs/001-masque-udp-tunnel/spec.md), [implementation plan](specs/001-masque-udp-tunnel/plan.md), and [tasks](specs/001-masque-udp-tunnel/tasks.md) for scope, module boundaries, dependencies, and acceptance criteria.
- Use the [contracts](specs/001-masque-udp-tunnel/contracts/) for wire, configuration, diagnostics, and benchmark behavior; use [ADRs](docs/adr/) for architectural rationale.

## Architecture and changes

- Keep one production Rust package and one client/server executable; keep CLI wiring separate from protocol and policy logic. Traffic and interoperability tools are test-only builds.
- Preserve exclusive reactor ownership of QUIC/HTTP/3 and session state. Avoid per-packet tasks, shared quiche mutexes, and unbounded queues; keep Linux syscall/FFI code behind the reviewed UDP module.
- Preserve session generations, datagram boundaries, pacing, partial-send progress, and close/cancellation invariants. Follow the contracts for commitment and UDP provenance limitations.
- Use safe Rust by default. Document the necessity and local safety invariants of every unsafe operation; justify dependencies and abstractions against existing capabilities.
- Treat network input as untrusted. Keep authentication, target authorization, resource bounds, and diagnostic privacy intact in every change.
- Keep changes focused. Record significant protocol, trust, ownership, dependency, API, or performance decisions in an ADR; update affected contracts and migration guidance together.

## Build and verification

Use the pinned toolchain and lockfiles. From the repository root, once the Rust package is initialized:

```bash
cargo build --locked --release
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
```

See the [quickstart](specs/001-masque-udp-tunnel/quickstart.md) for prerequisites and scenario runners. Check that referenced manifests and scripts exist before running them; planned paths are not completed tooling. Validate separate tool packages when affected.

- Add contract and failure-mode tests for behavioral changes, and regression tests for reproducible bugs. Test-first ordering is required only when explicitly requested. Documentation-only changes need proportional static checks.
- Run applicable protocol, security, parser/property/fuzz, lifecycle, syscall, and interoperability checks for the changed behavior. Report unavailable checks and prerequisites explicitly; keep incomplete tasks unchecked.
- Profile before optimizing. Use the [benchmark contract](specs/001-masque-udp-tunnel/contracts/benchmarks.md) for reproducible comparisons and establish regression tolerances before candidate measurements.
- Distinguish static checks, independent interoperability, native platform validation, and performance evidence. Claim only what actually ran and passed.

## Git and review

- Inspect status and diff before editing or committing; preserve unrelated changes. Stage explicit paths and exclude secrets, generated credentials, build output, and unrelated files.
- Follow the active feature's task dependencies and checkpoints; mark tasks complete only after their required verification.
- Use focused Conventional Commits when commits are requested. Respect configured Spec Kit hooks; do not enable them implicitly or push/publish without authorization.
- Include the purpose, behavior change, compatibility impact, and verification evidence in pull requests. Security-sensitive changes require explicit security review.
- Invoke the independent `reviewer` subagent only when explicitly requested; keep its audit read-only.
- Preserve any Spec Kit managed block when maintaining this file.
