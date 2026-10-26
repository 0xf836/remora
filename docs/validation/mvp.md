# Phase 3 MVP validation

Generated benchmark, WG and interoperability archives are retained locally,
not in Git. The report below preserves outcomes and coverage limitations; see
[artifact retention](README.md).

Latest increment: [independent review remediation](review-remediation.md).
Earlier counts/results below are retained for their recorded revisions.

Date: 2026-10-11. Working tree based on
`503a83c374ac8b885f4d5e16bb26dff7c26b5a9e`; the benchmark manifest records
source and binary hashes because these changes are not yet committed.
Scope: T010–T034 and publication of T038, with external qualifications recorded
separately. Requirements-quality checklists remain unchanged.

## Verification

`CARGO_NET_OFFLINE=true ./scripts/validate/mvp.sh` runs the locked release build,
formatting, Clippy with warnings denied, all-target/all-feature tests, and the
separate traffic package's build/format/Clippy/tests. Native Linux x86_64,
Rust 1.99.0 and quiche 0.30.0 are pinned; the root lockfile is unchanged.
Native and pinned-container commands both passed. The suite has 90 production-package tests (16 library, 4 process, 15 I/O,
6 lifecycle, 15 protocol, 34 security) and two traffic-accounting tests. Three Python tests check frozen comparison
identity, per-run failures and rejection of false-valid/non-finite evidence.
One independent fixture test is ignored by the ordinary suite and is explicitly
run with its pinned external peer; that executed test passes separately.
Properties use bounded input, exercise the production parsers, and retain
normal shrinking/regression behavior. Fixtures require Bash, Python 3,
OpenSSL and unprivileged IPv4/IPv6 loopback sockets.

The published runner was also executed with separately generated IPv4 and
IPv6 fixtures using `--scenario shutdown`. Each run delivered the ten exact
mixed/empty/boundary datagrams of two sessions and terminated both Remora roles
successfully within the runner's five-second wait. This checks the runner
interface as well as the Rust process suite; it is not 30-minute qualification.

Container reproduction uses the pinned image ID
`sha256:6da6665e07eebe6948170c91b16eba3594b0695e704265b0f038ffe5a9be41f7`,
unprivileged UID, read-only populated Cargo registry, offline mode and a separate
`target/container`. See [build instructions](../build.md). Native/container
outcomes and benchmark results must be read with the coverage below; hosted
GitHub Actions and native ARM64 have not run in this session.

## Checked behavior and explicit implementer review

| Boundary | Evidence and assessment |
|---|---|
| Authentication / policy | Canonical ASCII token hashing, constant-time digest comparison, exact rules, all-answer checks, mapped/literal/scoped-address handling, pinned target, resolver cancellation/timeout permits and stale completion tests. No DNS or relay allocation precedes authentication and the exact rule. |
| R2 request / acceptance | Encode/decode once, bounded pseudo/auth headers, forbidden fields, valid final 2xx, no activation on 1xx/closed stream, Structured Field compatibility, IPv6 paths and checked associations. Real-process malformed headers cannot emit target UDP. |
| R3 Capsules | Streaming retained prefix at most eight bytes, unknown-type skip without body allocation, missing Context ID / incomplete FIN errors, 65527/65528 semantic boundaries, no reliable forwarding. Real-process malformed/oversized Capsule cases verify stream codes and zero target traffic. |
| Transport admission | Verified CA/name before H3 request, required settings, Retry tamper/address/CID/lifetime checks, finite source limiter, native CID changes, absent capability cases and real GOAWAY cutoff/lower-stream continuation. No early-data API is enabled or ticket resumption installed. |
| R1 commitment | ACK-backed public buffer receipt, blocked/reset cancellation, native purge before generation, immutable ciphertext across stream close, original pacing/expiry, pre-generation reservation, accepted-prefix progress, stop-generation discard and conservative incoming/native expiry. |
| Owner lifecycle | Finite rotating visits, short-read continuation, late setup completion ignored, future cancellation on close, separate connected relays, permanent rejection latch and five-second bounded process shutdown. |
| Linux unsafe boundary | All unsafe operations are restricted to `src/udp.rs`, documented at the call site. C headers/iovecs/sockaddr/control storage live through each synchronous syscall, unaligned ancillary values are copied only after bounds checks, no C pointer is retained, receive headers reset on reuse, MSG_TRUNC/MSG_CTRUNC are rejected before forwarding. Real v4/v6 single/mmsg, partial sendmmsg acceptance, EAGAIN, wrong-peer filtering, pktinfo, EMSGSIZE and timerfd tests pass. |
| R5 diagnostics | Fixed ring/count/byte reservations, one fully prepared chunked status batch, final-chunk writer ACK, coalescing only equal classes, newer updates surviving old ACK, terminal cleanup retention, intentional overflow/broken-sink loss explicitly counted. Typed fixed output and sentinel tests cover all levels; ordinary log suppression cannot consume status reservation. |
| Traffic / baseline | Unique sequence, integrity, direction separation, duplicate/reorder/overflow accounting, sparse same-clock RTT, offered/enqueue/unscheduled counts, explicit drain interval and matched two-role topology. Empty/tiny packets use a separate byte/boundary exercise rather than invented sequence identity. |

This is the implementer's explicit protocol, security and unsafe-boundary
review, not an independent reviewer approval. The new
[ADR-0006](../adr/0006-mvp-acceptance-and-ownership.md) records the public-API
acceptance trade-off and ownership invariants. No reviewer was automatically
invoked. Existing independent reports are historical and unmodified.

Process negatives monitor an actual target socket: bad/revoked/missing tokens,
forbidden exact target/alias and denied localhost resolution, wrong name,
untrusted CA, expired certificate, malformed requests, missing DATAGRAM
capability, pre-request datagrams and Capsule errors yield zero unauthorized
UDP. Mixed/changed DNS answers, link-local rules, cancellation and stale
callbacks are deterministic component tests; controlled DNS replacement and
actual 0-RTT ticket replay by an independent peer remain release evidence.
The public-quiche injecting peer is same-stack contract evidence, not FR-026
interoperability. The absence of destination replies alone is not a socket error.

## Kernel-induced and later evidence

Ordinary CI executes real MSG_TRUNC and scripted production flag validation for
MSG_CTRUNC; arbitrary ancillary parsing and overflow wrap use bounded component
inputs. A deterministic kernel-induced EINTR, control truncation or RX overflow
requires a dedicated runner: arrange a blocked/interruptible syscall rather
than the nonblocking DONTWAIT path, reduce ancillary capacity in a test-only
adapter, or pause a small-buffer receiver during a bounded burst and inspect
SO_MEMINFO drop sampling (do not enable SO_RXQ_OVFL). Record which kernel path actually occurred; injected flags are not
kernel observations. Controlled PMTU/path changes require runner-owned namespaces,
MTU/qdisc capabilities and recorded topology (T048/T049/T056). No host network
configuration is changed by this increment.

QPACK/control sizes are explicitly limited, but exhaustive control-stream
adversarial load, native platform syscall/FFI campaigns, maintained fuzz/Miri,
full SC-002/010 correlation matrix, 32-session ten-minute isolation, recovery,
qlog, PMTU error-queue handling and full resource/capacity acceptance remain
later tasks. Native ARM64 evidence is required before an ARM64 support claim.
Early low-rate results do not meet or waive release acceptance criteria.

First measurements and frozen prospective comparison bounds are in
[performance](../performance.md) and the preserved result directory.
External peer and prepared WireGuard evidence are tracked in
[interop](interop.md) and [application compatibility](application-compatibility.md).

## Incremental correction after the first reference

The independent Go negative found discarded final rejection headers during
session cleanup. Corrected cleanup releases the association/resources and
cancellable datagrams while preserving the queued final HTTP refusal/FIN.
New tests verify a real encrypted HTTP 401 reaches the peer after session
removal and invalid/unsupported/undersized/unvalidated Initials cannot allocate
connections; a full-size unvalidated Initial produces stateless Retry.
Native and pinned-container checks pass with 90 ordinary tests; one external
interop test is explicitly exercised outside normal CI. The frozen first
reference is unchanged, and post-fix paired comparison artifacts preserve
invalid startup-race runs as well as the subsequent checked comparison.
The benchmark runner now waits for the owned server's UDP socket inode/bind
before starting a connected client, using no extra traffic or network edits.

## Local WireGuard fixture follow-up

The operator-authorized two-guest lab exposed a timer readiness bug after a
quiet interval: clearing cached timerfd readiness without polling again failed
to register the owner's next wakeup. The bounded repoll now preserves timer
progress; a regression test reproduced the failure before the fix. Real relay
input also now increments the missing `to_client` ingress counter, verified
by the relay test and the structured status from both guest application paths.
These correctness/diagnostic changes do not change congestion control or
product idle-timeout defaults.

After these corrections and the lab tooling, the native and pinned-container
MVP commands passed with **91 ordinary production-package tests**, two traffic
tests and 13 Python tests (three comparison, two WG traffic and eight lab
safety tests). The separate independent-peer test remains excluded from the
ordinary count. Earlier counts and benchmark/interoperability artifacts above
describe their recorded revisions; they are not replaced with invented evidence
for this final binary. The latest guest smoke hashes identify the tested binary.
See the [lab guide](local-kvm.md) and [application evidence](application-compatibility.md)
for successful create/migrate/restart/traffic/cleanup coverage. Both guest
traffic runs described in that follow-up were 10-second smokes. The subsequent
full T037 run passed 1800
measured seconds with 72,000/72,000 WG/generic datagrams, stable Remora units
and verified cleanup. The [application evidence](application-compatibility.md)
records its profile, warmup observations and remaining release limits.

The final lifecycle review also separates RESET from normal FIN for partial
Capsule input. A real peer RESET regression confirms cancellation records its
transport code and retires the flow without an invented malformed-FIN failure.
The new test child-signal helper refuses already reaped identifiers, preserving
the Phase 2 process-group safety invariant; the existing descendant regression
checks this explicitly. These final changes share the predeclared comparison
policy. Native/container verification and the explicitly invoked 29-case
independent peer matrix passed for the final increment. Its matching paired
performance comparison passed all 12 runs against the unchanged reference and
policy; see final comparison.

## Independent review correction increment

The nine code/design/test findings are addressed in the
[remediation record](review-remediation.md) and
[ADR-0007](../adr/0007-review-corrections-and-contract-alignment.md).
Native and pinned-container MVP checks passed with 100 ordinary root-package
tests, two traffic tests and 28 Python tests. New cases include bounded path
events, real H3 field semantics, wildcard source preservation on immediate
and queued replies, terminal collection at long snapshot cadence, actual owner
partial-send/EINTR and blocked-cwnd expiry, lab ownership recovery and exact
benchmark/policy coverage. The 29-case independent peer matrix and the separate
Rust interoperability test passed again for this increment.

The earlier T037 duration and other archived results remain tied to their
recorded inputs; final-revision VM duration/load/platform qualification is still
T076. The short prospective comparison is recorded in [performance](../performance.md).
The original comparison failed CPU bounds in three of twelve runs despite
complete delivery and passing correctness/interoperability checks. The project
owner subsequently authorized the provisional MVP development policy: 0.5 cores
per role for process and owner averages, with every non-CPU bound unchanged.
Reassessment of the retained final measurements passes 12/12 under that policy,
explicitly as a retrospective development exception, not no-regression proof.
The original policy/failure remain intact; no fresh load runs were required.
The CPU development gate is closed. No Phase 4+ or release acceptance criterion
is inferred from these corrections.
