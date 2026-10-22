# Default 1452 / 1400 / WG1280: local evidence

Date: 2026-10-23. Decision: [ADR 0031](../adr/0031-default-size-profile.md).

Both Remora roles now default to outer UDP payload ceiling 1452 and application
payload cap 1400. The primary WireGuard example uses interface MTU 1280,
requiring 1312 application bytes. Existing explicit overrides still apply.
The [profile table](../wireguard.md#choosing-a-size-profile) includes the old
1350/1200/WG1120 profile for rollback and WG1360 for measured path tuning.

## Checks performed

- Used the existing `remora-phase2-build` container, image ID `6da6665e07ee`,
  pinned Rust 1.99.0, locked dependencies and offline Cargo cache.
- Format check and Clippy with all targets/features and warnings denied pass.
  The full `cargo test --locked --offline --all-targets --all-features` run
  passes. Two independent-Go tests remain explicitly ignored.
- Process E2E tests exercise the new defaults without transport overrides
  over outer IPv4 and IPv6 loopback. After full-size readiness, they verify
  byte-identical 1312/1392/1400-byte application round trips and 1401-byte
  rejection while other flows remain usable. Explicit legacy 1350/1200
  configurations retain their 1200/1201 boundary checks.
- Native PMTU revalidation/purge/recovery and bounded directional size
  diagnostic regressions pass as part of the full suite. Matched outer
  1200/application 1400 process tests verify exact local rejection details in
  both directions while smaller traffic survives.
- Added explicit default assertions for both roles and reran the configuration
  test after the full suite. Format was checked again with the final assertions.
- All 111 Python tests pass inside the same container, including validation
  tooling tests. Historical impaired-path and VM fixture generators now
  explicitly select 1350/1200, preserving their stated 1200/1201 expectations.
- Locked offline release builds without and with qlog pass. The final qlog
  executable SHA-256 is
  `3de7f60648bde369131aeb92d294faed574b1ed3cdb66ce6ca702c8a2da3db85`.
- `quiche_inputs.py` passes for the unchanged source pin
  `154ce1995f23efa434e72dac269f707145be2015`. Cargo manifests/lockfiles and
  dependency policy remain unchanged. No backend patch was added.

## Evidence limits and migration

Update both endpoints together and remove or replace old explicit overrides
where intended. The application cap is not negotiated: an old 1200-byte peer
can reject WG1280 data after a successful handshake. The pinned backend's
known probes-above-smaller-peer-cap defect remains unresolved. Matching caps
avoid this particular mismatch; they do not qualify every network path.

Startup/revalidation can temporarily reduce verified capacity below the
1312 bytes needed by WG1280. Fixed-size WG data then drops until capacity
recovers; Remora does not change interface MTU or replay dropped packets.

These are local functional checks. Outer IPv6 opaque UDP is not inner-IPv6
WireGuard qualification. No new VM/WireGuard duration run, WAN, ARM64,
independent-stack interoperability, fuzz/ASan or throughput/CPU comparison
was performed for the new default executable. The earlier independent review
covered the preceding opt-in scope; it does not constitute review of this
subsequent default transition. Historical performance and duration results
remain bound to their original binaries and configurations. No measured
performance optimum, improvement or regression qualification is claimed.
