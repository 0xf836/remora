# Changelog

## Unreleased

### Added

- Endpoint configuration: `proxy = "https://host[:port]"`, client `target`,
  derived TLS/HTTP identity and standard MASQUE path, optional numeric
  `connect_address` and advanced template override.
- Bounded proxy DNS with IPv4/IPv6 fallback and fresh resolution on recovery,
  including verified IP SAN identities for numeric proxies.
- Subcommand help, visible successful config validation and privacy-safe
  field/type/range and TOML syntax diagnostics; public configuration/TLS guides,
  UDP/443 service example and practical WireGuard setup.

- One Linux Rust executable with `client`, `server` and `check-config` commands;
  private startup configuration, verified TLS server identity,
  per-request Bearer authentication and explicit all-answer target authorization.
- HTTP/3 CONNECT-UDP with QUIC DATAGRAM/Context ID 0, per-sender/local-destination
  sessions, connected destination sockets, bounded queues/resources and exact
  UDP datagram boundaries including empty messages.
- Bounded recovery with listener preservation and no outage replay, GOAWAY
  draining/replacement, permanent-rejection admission latch and graceful signals.
- Privacy-safe structured events/status, correlated failure summaries, explicit
  collection-gap reporting and optional filtered metadata tracing via `qlog`.
- Native/property/process/protocol/security/syscall tests, independent pinned
  masque-go/quic-go interoperability, ASan lifecycle/parser fuzzing, Miri checks,
  dependency/source audits and opt-in two-VM WireGuard validation.
- Bounded reference benchmark methodology, recorded native x86_64 functional
  and performance results, and verified operator/build/WireGuard instructions.

### Changed

- Require a matching DNS SAN for DNS proxy identities.

- Pin quiche 0.30.0 to fork revision
  `154ce1995f23efa434e72dac269f707145be2015`, containing the reviewed
  minimum-payload correction. Source integrity and lockfiles bind the build;
  dependency sources are fetched by Cargo rather than vendored into Remora.
- Service eligible committed output before generating more packets, preserve
  exact partial-send progress, bound owner work/fair continuation, adapt receive
  visits to recent DATAGRAM density, and trim idle buffer storage within bounds.
- Raise default outer socket buffer requests from 262144 to **1048576 bytes**
  and application/relay requests from 65536 to **131072 bytes** per RX/TX request.
  Application/native/ciphertext queue and residence limits remain bounded.
  Explicit config overrides remain in force; effective kernel values are reported.
- Keep **CUBIC** as the selected default, accepting its measured small CPU cost.
  Explicit `bbr2_gcongestion` and `reno` remain available with their own evidence
  requirements. No congestion-control bypass is introduced.

### Fixed

- Finish a bounded response-commitment sweep before parking, including ready
  sessions behind ACK-pending responses, without restarting idle sweeps.
- Limit interrupted UDP receive/send and timerfd reads to four attempts per
  adapter visit so persistent EINTR cannot starve owner cancellation checks.

- Report prohibited target addresses as `client.target` with a privacy-safe
  explanation of the address restrictions.

- Stop requesting immediate H3 owner turns solely for queued application data
  behind full ciphertext output; wait for real readiness/pacing/expiry while
  retaining staged enqueue/output progress and original packet deadlines.

- Preserve accepted-prefix/pacing/cancellation invariants under send pressure
  and prevent fresh producers from consuming service ahead of eligible output.
- Derive benchmark manifest socket-buffer requests from shared configuration
  defaults; preserve the original selected manifest with a transparent erratum.
- Accept valid early bounded-search completion at 1000 Mbit/s and add actual
  reactor lifecycle fuzz transitions with independent rejection/generation oracles.

### Compatibility and qualification

- Configuration stays **schema_version = 1**. Trust, credentials and target
  policies are immutable at startup: updates/revocation require controlled
  stop/start, completing at old-process exit plus replacement readiness. See
  [migration and rollback](docs/operations.md#controlled-update-revocation-and-rollback).
- The selected qlog-build CUBIC executable is qualified for the recorded native
  Linux GNU **x86_64** profile, the sole current required platform. ARM64/T072
  is [deferred outside requirements](docs/adr/0024-defer-arm64-release-scope.md);
  retained manual-only tooling/profile files are experimental, not support evidence. No release tag, publication
  or deployment is asserted by this Unreleased entry.
- GSO/GRO remain effective off, including deferred `auto`. High-rate active
  tracing, BBR2/full WAN performance and universal WireGuard MTUs are unqualified.
- Setup/outage traffic can be dropped; there is no reliable payload fallback,
  private fragmentation, VPN/TUN management or recall of generated/in-flight
  traffic. External UDP freshness cannot be established after identical tuple
  reuse. CUBIC loss amplification and limited reference tail/variance evidence
  remain documented limitations.
- [Qualification](docs/validation/release.md), [performance](docs/performance.md)
  and [review corrections](docs/validation/phase8-implementation-review-fixes.md)
  preserve failed runs, measured scopes and the isolated churn-test failure whose
  cause remains unestablished despite passing repeats.
