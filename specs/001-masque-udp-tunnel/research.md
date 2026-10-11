# Research: Remora MASQUE UDP Tunnel

**Research date**: 2026-10-10. **Decision revision**: 2026-10-11. **Scope**: architecture and API research; no implementation, interoperability run, or performance result is claimed. Findings use official documentation and tagged source. Decisions are recorded in `docs/adr/0001-transport-profile.md` through `0004-verification-and-performance.md`.

## Decision 1: Rust and a pinned Cloudflare quiche transport

**Decision:** Rust 1.99.0, edition 2024; `quiche = "=0.30.0"`, default BoringSSL backend, optional `qlog` feature; commit Cargo.lock and rust-toolchain.toml in implementation. Initial targets are `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu`, Linux 6.1 or later, native qualification on both. The chosen Rust version is installed locally; quiche's declared minimum is 1.88. Pin native build/compiler/container inputs as well as Rust dependencies.

**Rationale:** The user chose Rust/quiche. Use its public Rust API rather than the C FFI or internal APIs. Lockfile resolution also fixes which supported `boring` major and TLS defaults are used. Native C/C++, CMake and the BoringSSL build prerequisites belong in build images, not runtime privileges. No cross-compilation-only performance claim.

**Alternatives considered:** Other QUIC libraries conflict with the requested stack. A floating latest dependency makes API, crypto and benchmark reproduction unreliable. Musl, macOS, Windows and additional architectures are outside initial support.

Sources: [tagged quiche manifest](https://raw.githubusercontent.com/cloudflare/quiche/0.30.0/quiche/Cargo.toml), [build instructions](https://raw.githubusercontent.com/cloudflare/quiche/0.30.0/README.md).

## Decision 2: Explicit HTTP/3 CONNECT-UDP profile

**Decision:** Advertise ALPN h3, enable QUIC DATAGRAM with finite receive/send queue counts, and enable server Extended CONNECT. Wait for verified handshake and processed peer SETTINGS before opening/forwarding a flow. Remora implements CONNECT-UDP URI validation, Context ID 0, and Capsule parsing around quiche's public H3 API. Application payloads use unreliable QUIC DATAGRAM only.

**Rationale:** quiche is a QUIC/H3 stack, not a complete MASQUE proxy. Its H3 configuration derives SETTINGS_H3_DATAGRAM from transport DATAGRAM enablement; do not invent an `enable_h3_datagram` API or inject the reserved setting through additional settings. H3 poll must consume settings/events before querying negotiated capabilities. Keep request/response streams open with fin=false; drain body data on Data events, subject to bounded work and explicit rescheduling.

**Alternatives considered:** Private framing, datagrams on reliable streams, CONNECT-IP and HTTP/2 fallback violate scope. A valid DATAGRAM capsule is not a malformed packet: first validate required structure/Context ID and the RFC Context 0 length bound, then discard/count the valid unsupported carrier without forwarding. Malformed Capsule syntax and oversized Context 0 retain their normative stream outcomes. Validate successful Capsule responses, including forbidden statuses/message fields, before acceptance; details and sources are in the wire contract. Unknown Capsule types are skipped incrementally. Required framing/control handling remains compliant.

Sources: [tagged H3 implementation](https://raw.githubusercontent.com/cloudflare/quiche/0.30.0/quiche/src/h3/mod.rs), [H3 connection API](https://docs.rs/quiche/0.30.0/quiche/h3/struct.Connection.html). Normative details and separate RFC links are in [the wire contract](contracts/masque-wire.md).

## Decision 3: One owner and one initial runtime backend

**Decision:** Tokio 1.53.2 current-thread runtime, one reactor owning every quiche/H3 connection and session map. Linux sockets use AsyncFd around nonblocking socket2 sockets. A small adapter contains reviewed libc calls for ancillary/mmsg/timerfd operations; protocol, policy and session logic stay safe Rust. No lock around a shared quiche connection, no task or channel per packet. Setup resolution runs through a bounded auxiliary pool; results carry generations and are revalidated by the reactor.

**Rationale:** quiche supplies no socket/event loop. Tokio provides Unix readiness, shutdown signals and coarse housekeeping without implementing a second general scheduler. Explicit scheduling bounds receive work, H3 events, datagrams, connections and timers. Bounded readiness tasks may notify the owner by socket ID once per ready/batch transition; they never mutate quiche or carry an unbounded packet stream. Start with one connection-owner thread; the benchmark is the gate for adding parallel owners.

**Alternatives considered:** Plain mio is viable but requires more bespoke lifecycle/timer/setup coordination without measured benefit. Tokio multithread work stealing does not parallelize a single connection and encourages synchronization. io_uring multishot/provided buffers require additional cancellation/completion/pool invariants; compare later only if syscall/readiness profiles justify it. Official tokio-quiche 0.20.0 supports CONNECT-UDP and a custom ApplicationOverQuic handler. Its ready-made H3 flow channel/resource behavior and worker pacing-offload assumptions require adaptation to this contract. Evaluate supported reuse during normal implementation when API contracts or measured costs justify the comparison; ADR-0002 distinguishes published package source from compatibility-era Git tag evidence. Assess its datagram-socket adapter before writing bespoke unsafe ancillary code. No custom-reactor speed advantage is asserted.

Sources: [Tokio builder](https://docs.rs/tokio/1.53.2/tokio/runtime/struct.Builder.html), [AsyncFd semantics](https://docs.rs/tokio/1.53.2/tokio/io/unix/struct.AsyncFd.html), [mio Poll](https://docs.rs/mio/latest/mio/struct.Poll.html), [liburing UDP example](https://github.com/axboe/liburing/blob/master/examples/io_uring-udp.c).

## Decision 4: Batched nonblocking UDP with exact partial-progress accounting

**Decision:** Baseline recvmmsg/sendmmsg, initial maximum 32 messages per call. Receive immediately available packets; never wait to fill a batch. Fair round-robin/deficit service visits at most 32 datagrams/socket/session and 256 total work units before checking due timers, cancellation and yielding. Retain readiness on budget exhaustion; clear it only after actual EAGAIN. Normal recvmsg/sendmsg mode remains available for A/B comparisons.

**Rationale:** Batch syscalls amortize overhead while preserving individual UDP boundaries. sendmmsg accepts a prefix: remove exactly that prefix, retry only its suffix. A returned zero payload length is an empty datagram, not EOF. Reset mutable syscall header lengths/flags before each reuse. MSG_TRUNC, unsafe MSG_CTRUNC and malformed ancillary input produce counted rejection. Wildcard listeners require pktinfo: RecvInfo.to is the received local address, not the wildcard bind; outgoing source selection preserves SendInfo.from. Separate v4/v6 sockets and explicit IPV6_V6ONLY avoid mapped-address policy ambiguity.

**Alternatives considered:** One recv_from/send_to per packet remains the simple diagnostic baseline. Waiting for a full batch adds latency and conflicts with best-effort UDP forwarding. Ignoring partial sends duplicates accepted packets.

Sources: [recvmmsg](https://man7.org/linux/man-pages/man2/recvmmsg.2.html), [sendmmsg](https://man7.org/linux/man-pages/man2/sendmmsg.2.html), [recvmsg](https://man7.org/linux/man-pages/man2/recvmsg.2.html), [pktinfo](https://man7.org/linux/man-pages/man2/IP_PKTINFO.2const.html), [RecvInfo](https://docs.rs/quiche/0.30.0/quiche/struct.RecvInfo.html).

## Decision 5: Pacing is part of correctness

**Decision:** Enable quiche pacing; honor every SendInfo.at. One CLOCK_MONOTONIC timerfd integrated through AsyncFd wakes at the earliest transport, pacing, expiry or shutdown deadline. Reserve output storage before calling send; retain generated ciphertext and source/destination/deadline across EAGAIN. Batch only due packets, without intentional accumulation delay. Record lateness and burst distributions.

**Rationale:** Generating a QUIC packet advances transport state; reconstructing it after a failed socket send is not a safe retry. Tokio Sleep has millisecond granularity; timerfd provides a more appropriate deadline mechanism for high packet rates. Map Instant durations to separately sampled CLOCK_MONOTONIC with checked arithmetic; never reinterpret Instant representation. Timer precision does not guarantee scheduler precision.

**Alternatives considered:** Busy spinning wastes CPU and is off by default. SO_TXTIME/SCM_TXTIME is deferred: it needs a compatible operator-controlled qdisc, correct clocks and error-queue accounting. Disabling pacing to obtain larger batches is rejected. A bounded ciphertext queue preserves retryable packets on their original connection generation in generated order; if residence exceeds 100 ms, fail/recover the affected connection rather than selectively rewriting or discarding mixed generated output. Successful quiche packet generation is the commitment boundary, distinct from enqueue and kernel send: ordinary session close cancels uncommitted queues but may not recall generated packets. Connection replacement/stop discards unsent output with the old transport state, never replaying it. See R1 in the plan and ADR-0001.

Sources: [SendInfo](https://docs.rs/quiche/0.30.0/quiche/struct.SendInfo.html), [timerfd](https://man7.org/linux/man-pages/man2/timerfd_create.2.html), [Tokio Sleep](https://docs.rs/tokio/1.53.2/tokio/time/fn.sleep.html), [ETF/SO_TXTIME](https://man7.org/linux/man-pages/man8/tc-etf.8.html).

## Decision 6: Optional outer GSO/GRO, independently bounded

**Decision:** GSO/GRO default off until correctness and target-native A/B measurements qualify them. Config accepts off/auto even if an offload's implementation is deferred: deferred auto uses ordinary I/O with effective off/reason not_implemented and no fabricated capability probe. Once implemented, auto probes each socket/path and falls back when capability/eligibility checks fail. Report requested/effective state and the fixed reasons in the config/diagnostics contracts. Optimize outer QUIC sockets initially; application/destination sockets retain ordinary datagram I/O. GSO batches same path/metadata and eligible pacing class, uniform segment length with optional shorter last segment, at most 64 segments. GRO uses a shared bounded 64 KiB scratch pool, validates UDP_GRO metadata and splits before quiche receives packets.

**Rationale:** Kernel/version/architecture alone cannot prove capability. Empty or mixed-size packets, incompatible metadata, future deadlines, probe packets and unknown ancillary data use baseline I/O. Capability errors disable the optimization without losing/resending accepted logical packets; EMSGSIZE updates size constraints. Optimizations must never alter packet boundaries or create private fragmentation.

**Alternatives considered:** Always enabling offloads creates correctness/kernel/NIC assumptions. A custom io_uring/zero-copy pipeline adds complexity without evidence. Destination-leg GSO/GRO is unnecessary initially and can be reconsidered after profiling.

Source: [Linux UDP options](https://man7.org/linux/man-pages/man7/udp.7.html).

## Decision 7: Explicit congestion control and two-leg size handling

**Decision:** CUBIC default, optional explicit `bbr2_gcongestion`, Reno for diagnostic comparisons. Keep outer congestion control and pacing enabled. Enable quiche PMTU discovery up to a configured outer ceiling (initial 1350-byte UDP payload); enforce no introduced IP fragmentation on outer and destination sockets using IPv4/IPv6 PMTU socket policy. Initial outer size is conservative; re-query datagram capacity on path events and before enqueueing. Collect destination PMTU/error-queue updates separately. Outer ECN remains Not-ECT.

**Rationale:** 0.30.0 exposes Reno/CUBIC/Bbr2Gcongestion; historical BBR variants must not be promised. A 1200-byte application payload is larger than a 1200-byte outer UDP payload after encryption/framing. quiche's datagram capacity already excludes its QUIC overhead: subtract only HTTP association/context varints. A relay socket's MTU can further reduce the payload ceiling; EMSGSIZE drops/counts the original datagram. RecvInfo has no ECN API, so reading TOS alone is not QUIC ECN validation.

**Alternatives considered:** Blindly selecting BBR because WireGuard is involved is unsupported by measurements. Disabling outer CC, IP fragmentation and reliable oversized-payload fallback are rejected. PMTU probes that intentionally exceed the known size use no-fragment probe semantics and preserve quiche's own failure detection; do not treat every probe EMSGSIZE as an application drop.

Sources: [CC implementation](https://raw.githubusercontent.com/cloudflare/quiche/0.30.0/quiche/src/recovery/mod.rs), [transport config](https://docs.rs/quiche/0.30.0/quiche/struct.Config.html), [IPv4 PMTU](https://man7.org/linux/man-pages/man2/IP_MTU_DISCOVER.2const.html), [IPv6 PMTU](https://man7.org/linux/man-pages/man7/ipv6.7.html).

## Decision 8: Bounded buffers and session isolation

**Decision:** One HTTP request stream per local sender/local-destination pair; share one authenticated QUIC connection per listener mapping. Preserve that local destination as the reply source for wildcard listeners, including queued replies. Use a dedicated connected destination UDP socket per accepted server session. Tag internal work with connection/session generations; stream IDs are never reused within a connection. Reusable pool accounting covers retained capacity, not only current lengths. Newest-arrival drop policy on application-queue exhaustion; 20 ms application residence limit, immediate outage discard, explicit global/identity/session admission. See [configuration limits](contracts/cli-config.md) and [state model](data-model.md).

**Rationale:** Per-session scheduling before a small shared quiche queue protects fairness. dgram_send_buf/recv_buf ownership may avoid one copy, but benchmark rather than claim zero-copy: encryption and syscall staging still touch data. Internal quiche count limits require separate byte/capacity accounting. A failed session cannot close unrelated streams; a genuinely failed shared connection necessarily interrupts its own streams but not other connections.

**Alternatives considered:** Huge connection queues hide overload, increase latency and violate resource goals. One connection per sender multiplies handshake/state overhead. Per-packet Arc/channel/heap allocation is avoided unless an API requires ownership transfer. Arbitrary delayed old destination responses after identical UDP tuple reuse cannot be distinguished: revised accepted [A1](proposed-amendments.md) requires observable client/session isolation, stale-work rejection and explicit documentation of the selected reuse strategy and its unavoidable limitations. It does not prescribe a quarantine mechanism or duration. A dedicated active connected socket isolates live associations and wrong-source replies, not arbitrary post-reuse packet provenance. Any additional retention/reservation strategy must justify its security/resource/availability trade-offs; no new subsystem is required merely to complete planning.

Session inactivity defaults to two minutes of valid UDP activity, with stream/socket closure together, following [RFC 9298 §3.1](https://www.rfc-editor.org/rfc/rfc9298.html#section-3.1); this is independent of transport probes and closed-resource reclamation. Permanent client rejection uses one fixed-size mapping admission latch surviving flow cleanup until controlled restart; the server keeps no unlimited deny history.

Source: [DATAGRAM queue implementation](https://raw.githubusercontent.com/cloudflare/quiche/0.30.0/quiche/src/dgram.rs).

## Decision 9: Trust, bearer credentials and target policy

**Decision:** Explicit CA bundle plus configured DNS server_name distinct from dial address; `verify_peer(true)` on the client. No insecure flag, early data, cached 0-RTT credentials, or TLS key logging. Per-request Authorization Bearer token, provisioned 32-byte random secret encoded as 64 lowercase hex characters in a private file; server stores SHA-256 of canonical token text and uses constant-time digest comparison. Authentication precedes DNS/relay allocation; allowlists contain requested host/port and permitted normalized resolved addresses/CIDRs. Bind a session to one approved numeric address, without re-resolving while active.

**Rationale:** quiche certificate verification defaults false; SNI/trust must be set intentionally. Bearer headers are compatible with independent peers and do not require an OAuth service. Policy canonicalizes numeric literals, mapped addresses, DNS trailing dots/case and percent encoding before authorization. For a hostname, all returned addresses must be allowed; mixed allowed/forbidden answers are rejected. Reconnect rechecks every condition. Restart-only revocation completes when the old process exits and the replacement is ready.

**Alternatives considered:** Tokenized URI paths, command-line secrets, unauthenticated relays and verification bypass are rejected. mTLS adds identity/certificate lifecycle requirements not needed for the initial provisioning model. Bearer theft still grants the credential's target scope; files/transport/diagnostics are therefore explicit trust boundaries. No certificate revocation service is promised; private CA removal requires config update/restart and closes sessions.

Source: [Bearer transport](https://www.rfc-editor.org/rfc/rfc6750.html). Full threat assumptions and negative cases are in [the security ADR](../../docs/adr/0003-security-and-observability.md).

## Decision 10: Structured diagnostics with a privacy boundary

**Decision:** tracing/tracing-subscriber JSON events on stderr; versioned bounded aggregate counter/status snapshots on stdout at most once/second during packet processing. After packet work and socket cleanup stop, a shutdown-only terminal drain may bypass cadence while retaining one batch in flight and writer acknowledgement. It shares a one-second deadline with final writer flush inside the overall shutdown budget; incomplete collection returns a nonzero result. Bounded nonblocking writer; slow diagnostics cause counted diagnostic drops, never packet-loop blocking. Safe structured QUIC stats every five seconds, bounded live correlation IDs, no payload/header Debug dumps. Optional qlog uses a fail-closed bounded JSON-SEQ filter before any persistence: whitelist event types and numeric fields, drop all HTTP/3 events, raw/frame bytes, header values, close-reason text and unknown records. Disable qlog on parser/schema/size failure. Key logging has no supported path. A bounded terminal-summary ring and status reservation preserve failure/session correlation independently of ordinary event suppression, within the existing 2 MiB diagnostic budget. Slow sinks/overflow expose explicit coverage loss; SC-010 requires actual collected correlation with zero such loss. Optional qlog follows the first usable MVP, rather than blocking it.

**Rationale:** Tagged H3 qlog code includes header values, including authorization. A lower qlog verbosity is not sufficient redaction. Filtered packet/recovery metadata remains useful alongside stats without exposing contents. Stats estimates concern outer QUIC, not application endpoint delivery. SO_RXQ_OVFL counters are separate observed kernel drops, without invented per-session attribution.

**Alternatives considered:** Raw qlog, arbitrary trace forwarding and unbounded Prometheus session labels conflict with privacy/resource requirements. An admin HTTP server is unnecessary; NDJSON can be consumed externally. Qlog is opt-in, disabled by default, with bounded files/event rates and sentinel tests.

Sources: [tagged qlog source](https://raw.githubusercontent.com/cloudflare/quiche/0.30.0/quiche/src/lib.rs), [tracing formatting](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/index.html), [socket overflow counters](https://man7.org/linux/man-pages/man7/socket.7.html).

## Decision 11: Independent verification and reproducible measurement

**Decision:** Test-only quic-go/masque-go fixture in both directions, commit `c1cf0e4dd6439aea94d5491b27439a4736f00246` (2026-09-22), verified through GitHub API contents at that ref. go.mod requires Go 1.26.0 and quic-go 0.63.0; select Go 1.27.2 for the fixture and commit go.mod/go.sum. Request.Header and ProxyConnectedSocket exist at that commit. The fixture performs normal TLS trust, credential/policy checks, and leaves wire encoding to masque-go. Native x86_64/ARM64 checks, fuzz/property/component/process/network tests and paired end-to-end benchmarks form release qualification. Initial development and measurements begin on x86_64; ARM64 readiness is not a prerequisite for that work.

**Rationale:** Cloudflare quiche self-interoperation is not independent proof. Both official masque-go role APIs accommodate the chosen standard Bearer header. A source/API pin is verified here; its compiled fixture compatibility and interoperation are not yet tested. The performance harness separates independent directional senders from sparse RTT echo probes and compares an unencrypted relay on the same two-leg route. All SC thresholds are preserved.

**Alternatives considered:** Google QUICHE MASQUE tools are useful optional third-party evidence but their CLI traffic/credential ergonomics are weaker for required generic UDP tests. Published transport performance is not Remora feasibility evidence. Revised accepted [A2](proposed-amendments.md) puts an authenticated E2E MVP and reproducible baseline harness into normal incremental implementation. Measure as soon as the basic tunnel works and use the results for engineering decisions. No separate feasibility prototype, pre-implementation measurements or dual-architecture start gate is required; full numeric/platform release evidence remains necessary.

Sources: [client](https://quic-go.net/docs/connect-udp/client/), [proxy](https://quic-go.net/docs/connect-udp/proxy/), [pinned module API](https://api.github.com/repos/quic-go/masque-go/contents/go.mod?ref=c1cf0e4dd6439aea94d5491b27439a4736f00246), [Go release manifest](https://go.dev/dl/?mode=json), [fuzz guidance](https://rust-fuzz.github.io/book/cargo-fuzz.html). The complete methodology is in [benchmark contract](contracts/benchmarks.md).

## Dependency/security review

quiche's July 2026 resource-exhaustion advisories identify 0.29.3 as the first patched version for [path events](https://github.com/cloudflare/quiche/security/advisories/GHSA-4q5x-gp38-rfp4) and [H3/QPACK](https://github.com/cloudflare/quiche/security/advisories/GHSA-4fgf-9xrr-88gf). The June [CID FFI advisory](https://github.com/cloudflare/quiche/security/advisories/GHSA-mh64-ph39-mrc9) is patched in 0.29.2 and concerns FFI APIs, which Remora does not enable. Selected 0.30.0 includes these fixes; this is not a claim of absence of all vulnerabilities. Drain path events and enforce H3 header/stream/queue ceilings regardless. Implementation/release CI must audit the actual Rust and Go lock graphs against current advisories.

Production dependencies have concrete purposes: quiche transport/TLS; Tokio readiness/signals/setup; socket2/libc Linux I/O; serde/TOML configuration and JSON records; tracing/subscriber/appender diagnostics; sha2/hmac/subtle/getrandom token/Retry primitives. Avoid a second TLS stack, generic proxy framework, external metrics server, io_uring backend or persistent database. Exact nontransport patch versions are resolved into the implementation lockfile and audited before use.

## Research closure

The baseline is selected and independent-audit corrections R1–R6 are recorded in the contracts/ADRs and [decision record](proposed-amendments.md). These are design resolutions, not successful runtime tests. Single-owner throughput, shared DATAGRAM-queue fairness/control servicing, exact heap/RSS and qlog overhead remain implementation measurement questions. User-directed revised [A1/A2](proposed-amendments.md) are accepted and integrated into spec.md and the design. The specification/design readiness gate passes; task generation and incremental x86_64 implementation may proceed. Existing release targets remain unchanged, ARM64 qualification precedes its release-support claim, and no executable measurement or interoperability pass is inferred.

Cross-artifact follow-up specifies prospective regression policies after first measurements/before candidate changes, initial GOAWAY verification in the first transport increment, optional TDD ordering in generation/execution, the declared T034 transition with external qualification left pending, and deferred-auto offload status. These are workflow/contract clarifications within the selected architecture, not fresh API research, numerical tolerance selections or runtime evidence.
