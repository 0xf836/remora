# Phase 2 foundation validation

Date: 2026-10-11. Scope: T005–T009 only, working tree based on `6cdee35`.
The implementer originally accepted the foundation increment, not forwarding or release qualification.

**Independent review snapshot (2026-10-11, before corrections):** Foundation acceptance remained
incomplete; see the [full read-only report](phase2-review.md). T005–T007 remain
complete. T008 is reopened for surviving runner-owned child-process descendants
(F1); T009 is reopened for the cleanup regression and missing configuration-parser
property/fuzz coverage (F2). Recorded 33-test passes below remain valid historical
evidence. All 40 requirements-readiness and 16 specification-quality criteria
remain satisfied; they do not certify implementation acceptance.

**Current implementation assessment (2026-10-11, after corrections):** F1/F2
are corrected and verified as described in [review finding resolution](#review-finding-resolution).
T008/T009 are complete again; Phase 2 is accepted for its foundation scope.
This is the implementer's assessment after automated checks, not a second
independent reviewer approval. The original review report is preserved unchanged.

## Reproduction inputs and results

- Rust 1.99.0, quiche 0.30.0 and the unchanged resolved package versions in
  `Cargo.lock`; SHA-256 of the lockfile is
  `73f95573f9fa739062f98e414be62f3e1cf06b80dec65d44b80cef587e251cc7`.
  Direct boring/rustix/time dependencies reuse packages already in that graph.
- Native Linux GNU x86_64, kernel 7.2.9; pinned Rust 1.99.0.
- Pinned Rust/Debian build image from `tools/build/Dockerfile`, locally built
  image ID prefix `6da6665e07ee`. Container checks ran as an unprivileged UID,
  with a read-only populated Cargo registry and separate `target/container`.
  Python 3.11.2 and OpenSSL 3.0.22 were available in the image. No downloads,
  host package changes, namespace privileges or network configuration occurred.

| Check | Native x86_64 | Pinned x86_64 container |
|-------|---------------|------------------------|
| `cargo build --locked --offline --release` | PASS | PASS |
| `cargo fmt --all -- --check` | PASS | PASS |
| `cargo clippy --locked --offline --all-targets --all-features -- -D warnings` | PASS | PASS |
| `cargo test --locked --offline --all-targets --all-features` | PASS: 33 tests | PASS: 33 tests |
| Dockerfile build / Python and OpenSSL prerequisites | Not applicable | PASS |
| `bash -n scripts/validate/fixtures.sh` | PASS | Script executed by fixture tests |

Test distribution: 3 library unit tests, 8 I/O/buffer tests, 22 security/config/
diagnostic tests. The binary and protocol/lifecycle/E2E entry points still have
zero tests because those behaviors are unimplemented. The buffer property test
also exercises randomized sequences of quota requests.

The first restricted-sandbox test run could not bind the local UDP sockets
needed by fixture generation. The full native test command was rerun with local
socket permission and passed. This is a test prerequisite, not a skipped test.
An initial status-reservation test assumed 256 full snapshots fit in 256 KiB;
it was corrected to check whichever byte/count ceiling is reached first, with a
separate small-record test proving the exact 1024/256 record limits. No budget
was increased or bypassed.

Final backend-source review identified that returning zero from a password
callback can decrypt an empty-password key. The loader now explicitly rejects
any callback invocation, and the encrypted-key test covers both a nonempty and
empty passphrase. The final native/container checks below include this correction.

Container reproduction follows [build instructions](../build.md): set
`CARGO_HOME=/tmp/cargo`, mount the existing registry read-only, set a separate
target directory, and execute the four locked commands above. An empty cache
needs normal Cargo downloads; offline mode does not waive missing dependencies.
Hosted GitHub CI was updated but was not run from this session.

## Verified behavior

| Task | Implemented and checked |
|------|-------------------------|
| T005 | Exactly one role, schema version/unknown-field rejection, relative paths, non-loopback client opt-in, regular private files/modes, descriptor-based no-follow traversal, FIFO rejection without blocking, canonical token/LF handling, PEM parsing/key matching, encrypted-key rejection without prompting, static identity/rule constraints, host normalization, checked numeric/FD budgets, accepted off/auto settings |
| T006 | Separate bounded pools; full boxed capacity charged while lent/cached; cleared reuse; preallocated return slots; checked RAII debit/credit; empty/range/deadline/generation descriptors; immutable ciphertext metadata; combined session byte and directional packet limits; native count/byte rollback; FD/setup permits; explicit metadata capacity |
| T007 | Typed privacy-safe schema/UTC timestamps/process IDs, fixed failure/drop/offload classes, direction counts with explicit overflow errors, exact 20/s burst-40 bucket, stdout/status and stderr/event routing, count/byte reservations including in-flight output, status priority, snapshot minimum interval, broken/blocked writer accounting and bounded unsuccessful flush |
| T008 | Local CA and verified DNS SAN remora.test, matching server key, random token and canonical-ASCII SHA-256 digest, explicit target permission, three distinct unprivileged ports, mode-0700 directory/mode-0600 private material, existing-output refusal, fake clock/scripted partial/error I/O, runner-owned private roots and bounded child-process cleanup |
| T009 | Registered unit/component/property/security tests pass in both x86_64 environments; secret sentinels are excluded from typed output and safe error text at every log level |

Filesystem permission/symlink/FIFO cases exercise real Linux opens. Wrong-owner
and root-provisioning decisions are additionally unit-tested with supplied UID
metadata; no privileged chown or root runtime is required by the suite.

## Security review and limitations

The implementer reviewed file/crypto/error handling, ownership, generation
immutability, arithmetic, reservations and diagnostic exposure explicitly; the
assumptions and trade-offs are in [ADR-0005](../adr/0005-foundation-primitives.md).
No independent reviewer was invoked during the original implementation. The
subsequent user-requested [independent review](phase2-review.md) identified F1/F2
and reopened T008/T009. The main agent recorded the reviewer's decisions in the
requirements-quality checklists at the user's request; all 56 marks remain checked.

Provisioning directories and the executing user/root remain trusted. No-follow
opens prevent symlink substitution, not a trusted owner editing an open inode.
Selected fixture ports are not reserved after generation; runners must handle
later bind collisions. A kernel-stuck child cannot be guaranteed reaped within
a user-space timeout; the runner bounds its wait and reports timeout. A blocked
writer can retain its charged record/thread until process exit; it is not
restarted indefinitely, and incomplete flush is never reported successful.

The current diagnostic schemas/counters are primitives. Terminal correlation,
retention/ACK handling, full runtime counters and filtered qlog remain later
tasks. GSO/GRO settings are accepted but deferred: off reports configured_off,
auto reports effective off/not_implemented, without a capability probe.

CLI wiring/check-config, runtime authentication/DNS policy, socket syscalls,
QUIC/HTTP/3 forwarding, commitment/close/GOAWAY integration, independent-peer
interoperability, WireGuard, native ARM64, performance/RSS and full acceptance
matrices are **not verified** by this increment. T010 onward remains unchecked.

## Review finding resolution

### F1: Owned process-group cleanup

`tests/common/mod.rs` now uses `waitid` with `EXITED | NOHANG | NOWAIT`
to detect leader exit without reaping it, signals its owned group, then reaps
the direct child. The unreaped leader keeps its PID/PGID reserved through
cleanup; a cached completed status prevents subsequent signalling of released
identifiers. Signal errors other than a missing group are returned by `finish`.
Timeout and Drop still use bounded waits. The exclusive wait/group ownership
assumptions and unavoidable kernel/init limitations are in
[ADR-0005](../adr/0005-foundation-primitives.md).

Four registered regressions in `tests/security/child_cleanup.rs` cover:

- Successful and nonzero leader exit with a surviving descendant, repeated
  `finish`, and an unrelated process group remaining alive.
- Timeout followed by Drop cleanup and direct-child reap.
- Dropping a still-running leader with a descendant.
- Invalid-deadline error followed by Drop cleanup.

The successful-leader regression was compiled and run against a scratch copy
of the pre-fix guard. It failed with `owned descendant survived guard cleanup`.
The probe cleaned its own remaining child. The same registered regression
passes with the corrected guard. No repository rollback was used for the probe.
Tests allow exited orphan zombies awaiting host/init reap; they must not be
running or retain sockets. They do not claim control of a descendant that
deliberately escapes the runner-owned group.

### F2: Parser property coverage

`src/config/properties.rs` adds eight normal CI properties using the existing
`proptest` dependency, 128 cases per property and fixed seed `0x52454d4f524132`:
bounded raw/Unicode configuration and credential inputs; schema versions and
unknown secret fields; host/authority panic-free normalization and fixed errors;
DNS case/trailing-dot/port identity and forbidden forms; IPv4/mapped-IPv6 CIDR
equivalence and containment; IPv6 prefixes/family separation; and arbitrary
CIDR text inside an otherwise valid credential document. Invalid prefix ranges
and secret sentinels are included. Normal proptest shrinking/regression
persistence remains enabled. No test-only parser is substituted for the actual
startup parser, and no public parsing bypass or new dependency was added.

### Checks after corrections

The lockfile, toolchain and pinned image inputs above are unchanged. Native
Linux GNU x86_64 and the existing pinned x86_64 container both passed:

| Command | Native x86_64 | Pinned x86_64 container |
|---------|---------------|------------------------|
| `cargo build --locked --offline --release` | PASS | PASS |
| `cargo fmt --all -- --check` | PASS | PASS |
| `cargo clippy --locked --offline --all-targets --all-features -- -D warnings` | PASS | PASS |
| `cargo test --locked --offline --all-targets --all-features` | PASS: 45 tests | PASS: 45 tests |

Distribution: 11 library tests, 8 I/O tests and 26 security tests. This adds eight
parser properties and four child-cleanup regressions to the original 33 tests.
The restricted-sandbox full test attempt again failed because fixture UDP bind
was unavailable; the full command passed with local socket permission. Container
checks ran as the unprivileged UID with the same read-only registry mount and
separate target directory; no image rebuild was needed for this source/test fix.

The implementer explicitly reviewed the ownership/identifier-reuse and error
paths introduced by the cleanup change. No second independent audit, hosted CI,
ARM64, interoperability or performance result is claimed. Requirements-quality
checklist marks and the original read-only review conclusions remain unchanged.
