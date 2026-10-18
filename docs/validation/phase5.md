# Phase 5 incremental validation

Date: 2026-10-12. Scope: T048–T057, native Linux x86_64 development.

After the explicitly requested independent review and corrections, the
implementation passes 131 root Rust tests (one separately qualified
independent-peer test is ignored by Cargo), eleven test-only traffic package tests,
36 Python tests, locked release builds, rustfmt and Clippy -D warnings for both
packages. All 29 pinned Go interoperability cases pass for the final executable.
The original wrong-name diagnostic failure is retained; the fix preserves exact
trust/0 instead of weakening the interop assertion.

Native syscall checks cover IPv4/IPv6 no-fragment policy, current route MTU,
empty/at/above limits, accepted prefixes, actual ICMP error queues without quote
retention and immutable-peer reuse after an observed refusal. Component checks
exercise verified-quiche PMTU reduction between enqueue/generation, selective
native reservation purge, blocked-cwnd expiry, future pacing, EMSGSIZE/ENOBUFS
failed-front removal/no replay, intact outer oversize versus truncation and role
counter direction. Process size checks cover both families, byte equality,
multiple sources, burst integrity and surviving-flow progress.

The nine native shaped scenarios and their exact artifact/binary scope are in
[network.md](network.md). Profiling, original strict CPU failures, the existing
ADR-0007 development exception and complete reference-methodology limitations
are in [performance.md](../performance.md). Development assessment is not a
strict no-regression pass. Full SC release/resource/duration/ARM64 qualification
remains pending its later tasks, including T077; no old result is transferred
to another executable hash.

## Explicit security assessment

This is the implementing agent's contract assessment. The project owner
explicitly requested an independent read-only Phase 5 review and then authorized
correction of its five findings. The regression results below validate those
corrections; a second independent audit of the corrected tree has not run.

TLS peer/name validation, Bearer authentication, canonical URI parsing,
default-deny all-answer target policy and numeric destination pinning remain in
place; no data reaches a destination before committed acceptance. New early
request headers are bounded by <=32 fields/16KiB and existing Session/setup
quotas/deadline. Tests prove no identity/DNS/relay work starts before capability
verification, and cancellation/duplicate/expiry/close discard the reservation.
Malformed or unsupported settings do not become permission to forward.

Error-queue input is untrusted: validate lengths/alignment/family/origin/type/
code/destination, ignore quotes, and retain no quoted payload or arbitrary
strings in diagnostics. ICMP observations do not change target authorization
or identify an application's lost ciphertext. GRO segment markers fail closed
without an implemented splitter. Native syscall unsafe operations remain in
the narrow UDP runtime boundary with local safety comments; the bounded,
read-only ethtool getter is test-runner inventory only. No dependency, raw qlog,
TLS key logging, shared quiche mutex or per-packet task is introduced.

Namespace shaping uses fresh owned user/network namespaces and retained child
PIDs; original namespace FDs, descriptor types and fresh-loopback inventory are
guards. A same-namespace invocation is tested to fail before capability calls.
No host network state, credentials in Git, private production endpoint or
operator/VPS deployment is touched. Test CA/keys/tokens are private temporary
fixtures; raw artifacts stay under ignored result directories. The separate
retained traffic executable binds regression comparisons across harness edits.

The final reference-tool native smoke completes five selected pairs plus two
short calibration cases using the final producer/validator, records per-workload
spread values/sample counts and owned UDP queue/drop charges, and deliberately
returns invalid release criteria for missing full intervals/matrix/churn.
`benchmarks/results/phase5-reference-final-tool/` retains that actual result;
a schema/command failure is not treated as an expected incomplete smoke result.

## Independent-review corrections

The review identified a P1 shared-outer-socket ICMP attribution defect, a P2
relay send-path inconsistency and three P2 reference-tool defects. All five
have focused corrections and regressions:

- Peer errors on outer send no longer stop the currently selected connection.
  The failed FIFO front is discarded without accepted-prefix replay or Session
  attribution; PMTU revalidation requires checked metadata for an active path.
  A native regression queues six ICMP refusals, drains four, then submits to a
  different live peer on the same socket with two active connections. Only fresh
  accepted data arrives, both connections/Sessions survive, and EBADF remains a
  hard failure. Scripted prefix tests also cover ECONNREFUSED/EHOSTUNREACH.
- Immediate and queued relay sends preserve the same immutable association on
  delayed peer errors. A native closed-target test then rebinds the same target,
  sees only fresh bytes from the original relay tuple, and records the exact
  unaccepted application drop. The old implementation closes the Session.
- Namespace holders run until owned teardown, without the old one-hour expiry.
  Exit detection and bounded owned cleanup are tested; a virtual clock jump and
  short native namespace run are lifecycle checks, not a 37-hour soak.
- SC-007 checks delivered >=500Mbit/s/direction on baseline and tunnel, a
  sustainable paired baseline and all 32 per-session counts. A synthetic full
  matrix with one delivered packet/direction no longer passes. Tests cover the
  exact threshold, one datagram below it, inactive Sessions and inconsistent
  sums; no numerical criterion is relaxed.
- Sampling skips verified traffic-worker completion only after measurement,
  including exit during /proc reads, with an explicit completion marker instead
  of zero samples. Early traffic exit, production-role exit and unexplained
  missing live-process evidence remain failures. Actual staggered owned workers
  reproduce the old FileNotFoundError and pass after correction.

The corrected production executable is SHA-256
`c90e055f2b2d995c5a35a58cb700887b2fbfb53f9ce4ab6aca4d02865ba02611`;
the corrected traffic tool is
`89ecbf2c4f626ab0bfe3b424dd1c5ce5e552858411833b0832dff851735bd0cb`.
All 29 independent Go cases pass in
`benchmarks/results/phase5-review-fixes-interop-native/`. The native selected
reference smoke in `benchmarks/results/phase5-review-fixes-reference-native/`
completes two short searches and five pairs: ten rows have zero generator,
integrity or diagnostic-coverage errors and matching per-session sums. Its
release criteria correctly remain invalid for incomplete intervals/matrix/churn.
The corrected MTU 1500->1240 characterization passes in
`benchmarks/results/phase5-review-fixes-mtu/`: control progresses, oversized
1201-byte payloads never reach the endpoint, and no fragmentation/capture-loss
is observed. Previous artifacts remain evidence only for their recorded hashes.
Full-duration/resource/native-ARM64 release qualification remains T077.
The clean low-rate frozen-reference comparison on this executable yields
26/30 strict checks passed (overall FAIL, owner CPU only), while all non-CPU
bounds pass. The separately labelled existing development-exception assessment
passes 30/30; original failures remain unchanged. See
[the comparison record](../performance.md#phase-5-independent-review-corrections-and-comparison).
