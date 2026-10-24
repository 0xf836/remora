# PR 9 native CI test failures

Status: local corrections and verification complete, 2026-10-25. Updated hosted
CI has not run; no hosted green, deployment or new WAN qualification is claimed.

## Observed failures

- [PR merge job](https://github.com/0xf836/remora/actions/runs/40122093633/job/120955673560?pr=9),
  merge `2ac3991e9e8378d29a14ed7129b67dd6bb8e16b4`: 84 library tests passed;
  `relay_peer_error_on_immediate_or_queued_send_keeps_same_association_without_replay`
  timed out at the one-second wait for native socket error readiness. The job
  did not reach E2E tests.
- [Branch push job](https://github.com/0xf836/remora/actions/runs/40122076837/job/120955621567?pr=9),
  `c2916f8d8e07e1ba70dc3579db14d3dc005385ff`: all 85 library tests passed;
  `configured_size_boundary_bursts_and_rejection_preserve_other_flows` received
  167 unexpected bytes instead of `size-b` in `Tunnel::ready`. The log lacks
  sender/socket attribution. It does not establish the packet's exact origin.

The merge differs from the branch only in release version/changelog/lockfile
metadata, not production or test code. Local checks use the branch and pinned
Rust 1.99.0. Both failing tests passed in an initial local attempt; those passes
alone do not explain either hosted failure.

## Corrections and evidence boundaries

The relay-error regression now injects one socket-local `ECONNREFUSED` at the
existing UDP syscall boundary, compiled only under `cfg(test)`. Initial and
subsequent successful delivery still use real UDP. Both immediate and queued
paths must retain the association, drop the exact failed packet, deliver the
fresh packet once and preserve counters. The test no longer depends on a
closed port staying unbound or on ICMP arriving within one second. Existing
native shared-socket error-queue coverage remains and passed.

Fixture provisioning formerly selected and released kernel ephemeral ports.
An intentionally stopped server can leave its client addressing that tuple;
another fixture can then reuse it as an application target. A deterministic
test reproduces delivery of a late packet to a newly bound alias. This
demonstrates an isolation defect, not proof that the hosted 167-byte packet
came from that particular mechanism.

Configured fixture ports now come from outside the kernel ephemeral range.
A private, locked registry leases each choice for its caller's lifetime,
including intentional server downtime and fixture cleanup. Process start time
and boot identity prevent stale PID reuse; dead owners are reclaimed. The
registry is scoped to UID and network namespace, so disposable network labs
do not share leases with the host. Temporary sockets check both loopback
families during provisioning. No sysctls, routing or operator interfaces change.
The registry coordinates fixture provisioning, not noncooperating programs or
every possible UDP tuple-reuse ambiguity.

Unexpected setup payloads still fail immediately. A bounded JSON record now
retains length/hash, expected length/hash, receiver/peer/target/listener and
owned process IDs. It contains no payloads, credentials or raw process logs.
Children stop before private fixture cleanup. An injected foreign-packet test
proves failure, metadata retention and secret exclusion after unwinding. The
native CI workflow uploads only allowlisted JSON failure records.

The fixture helper is included in future WAN source snapshots and the explicit
operations provenance list. Earlier Phase 15 measurements remain bound to their
original source and executable hashes; they are not rerun or requalified here.

## Local verification

- Pinned all-target/all-feature Rust suite, ordinary parallel execution:
  **239 passed, 3 prerequisite-dependent ignored**.
- Three additional bounded library/E2E pairs on CPUs 0 and 1, two test threads:
  each **85 library + 34 E2E passed**; 357 passed checks across these repeats.
- Final network-namespace registry adjustment: exact relay-error and configured
  size-boundary tests passed again; full Python suite **150 passed** (including
  six allocator regressions).
- Root rustfmt, warnings-denied Clippy, shell syntax, CI YAML/allowlisted artifact
  checks, whitespace and public-source secret scan passed.
- Fuzz adapter `cargo check --locked --offline --manifest-path fuzz/Cargo.toml
  --lib --bins` passed. This is not an instrumented fuzz/ASAN campaign.

Local logs and repeat summaries are under ignored
`benchmarks/results/ci-wan-loss-20261025/`. The exact origin of the historical
foreign payload remains unattributed. Updated push and PR-merge CI must still
run before this incident is described as hosted-CI verified.
