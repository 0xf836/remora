# Changelog

## [0.3.2](https://github.com/0xf836/remora/compare/v0.3.1...v0.3.2) (2026-10-26)


### Bug Fixes

* **udp:** avoid SO_RXQ_OVFL kernel crashes ([fa7ea9a](https://github.com/0xf836/remora/commit/fa7ea9ad843db8628326530364f9b901b34c0e7b))
* **udp:** avoid SO_RXQ_OVFL kernel crashes ([dada085](https://github.com/0xf836/remora/commit/dada08513dc5836f72a511197dd52b574f5f9b54))

## [0.3.1](https://github.com/0xf836/remora/compare/v0.3.0...v0.3.1) (2026-10-24)


### Bug Fixes

* **test:** isolate UDP fixtures and stabilize native CI ([7e201dd](https://github.com/0xf836/remora/commit/7e201dd4f9b5a147a63c85043664b211e397181a))
* **transport:** prevent WAN datagram loss amplification ([b412d82](https://github.com/0xf836/remora/commit/b412d82c5ce058790a3ed628c4684605ed3e0a11))
* **transport:** prevent WAN datagram loss amplification ([c2916f8](https://github.com/0xf836/remora/commit/c2916f8d8e07e1ba70dc3579db14d3dc005385ff))

## [0.3.0](https://github.com/0xf836/remora/compare/v0.2.0...v0.3.0) (2026-10-23)


### Features

* **logging:** add quiet, actionable operator logs ([a5bdff4](https://github.com/0xf836/remora/commit/a5bdff43371c04a6bca40fdc12eb4e7d4511313e))
* **logging:** add quiet, actionable operator logs ([5836c4d](https://github.com/0xf836/remora/commit/5836c4d3bc7f1173816ae71682fe0cb8f9550eae))
* **mtu:** default to 1452/1400 with a WG1280 example ([a115de3](https://github.com/0xf836/remora/commit/a115de3959b7b4de183d7d0869fa8d117162aaf5))
* **mtu:** default to 1452/1400 with a WG1280 example ([6321dea](https://github.com/0xf836/remora/commit/6321dea3d18ed3c88032c180c9a3d6bb78c73228))


### Bug Fixes

* **fuzz:** resolve operator diagnostics module in source adapter ([49d980b](https://github.com/0xf836/remora/commit/49d980b294dab04913130d139e07ee49e349b00f))

## [0.2.0](https://github.com/0xf836/remora/compare/v0.1.0...v0.2.0) (2026-10-21)


### Features

* **specify:** add bug triage extension ([c335271](https://github.com/0xf836/remora/commit/c335271429ebcad3dbca2eeff77db9f3611cb44e))
* **specify:** add bug triage extension ([2e2f436](https://github.com/0xf836/remora/commit/2e2f436a913b69e548e31f41bf498bf77c036d36))


### Bug Fixes

* **config:** accept safe systemd credential ACLs ([0b8c55a](https://github.com/0xf836/remora/commit/0b8c55a04679c76354254dbbd0b0cab32a02179c))

## 0.1.0 (2026-10-18)


### Features

* add authenticated MASQUE CONNECT-UDP tunnel ([99817de](https://github.com/0xf836/remora/commit/99817de565636e40ca0833b2fa7ca575cf6f92d3))
* **config:** simplify HTTPS proxy setup and add DNS recovery ([0b4c62c](https://github.com/0xf836/remora/commit/0b4c62c9c8d2510cce6aa522533355ae4a0d5df5))
* **core:** implement Phase 2 foundation primitives ([503a83c](https://github.com/0xf836/remora/commit/503a83c374ac8b885f4d5e16bb26dff7c26b5a9e))
* **datapath:** complete Phase 5 PMTU and benchmark validation ([bb4db2a](https://github.com/0xf836/remora/commit/bb4db2ac96e42b8955fda502a84f44086f3fab52))
* **operations:** complete Phase 7 diagnostics and validation ([ff5e629](https://github.com/0xf836/remora/commit/ff5e629750fd73d967f8154c227e1bd9b777901e))
* **recovery:** complete Phase 6 recovery and bounded shutdown ([34d4a8f](https://github.com/0xf836/remora/commit/34d4a8fa13aa67436e057773d1a3199b69aba233))
* **sessions:** complete concurrent isolation and bounded fair service ([0301c92](https://github.com/0xf836/remora/commit/0301c9239e0bfcf2cf0238da69393dfd3bb60262))
* **tunnel:** implement Phase 3 CONNECT-UDP MVP ([00d1dbe](https://github.com/0xf836/remora/commit/00d1dbe06fd0c67eb9053101a20f1e3eed158509))
* **validation:** complete Phase 8 x86_64 qualification ([66fa997](https://github.com/0xf836/remora/commit/66fa997a7060494cb4aa07dd95a8a466ac6be297))


### Bug Fixes

* **ci:** create missing fuzz output parents ([5b242b5](https://github.com/0xf836/remora/commit/5b242b5fda5db7fe0eff44c66af5c74ee8201480))
* **reactor:** avoid idle owner turns under output backpressure ([59263c4](https://github.com/0xf836/remora/commit/59263c40c71992d12bc591873efac99335f50293))
* **reactor:** enforce output backpressure and preserve test evidence ([e89d83a](https://github.com/0xf836/remora/commit/e89d83adfae73ae9ba18631948c5393fba9b99fb))
* **reactor:** finish control sweeps and bound interrupted syscalls ([38b5025](https://github.com/0xf836/remora/commit/38b5025525ee2d4ddf22c614839992f6371efad9))
* **validation:** support source archives on Python 3.11 ([6a927d5](https://github.com/0xf836/remora/commit/6a927d5b78f9abf2efb01849cf84768a37427d46))

## [Initial development notes](docs/releasing.md#first-release)

These notes describe development before the first release. Release Please adds
versioned entries above this section; the notes and their evidence boundaries
remain available as historical context.

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
  or deployment is asserted by these development notes.
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
