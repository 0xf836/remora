# CLI and configuration contract

Version: 1. Startup file/schema validation is implemented in Phase 2; commands below remain the planned T024 interface. Requirements FR-001/006/007/009/011/012/017/020/024/025/028/029.

## Commands and process lifecycle

```text
remora client --config PATH
remora server --config PATH
remora check-config --config PATH
remora --version
remora --help
```

PATH is configuration, never a token. check-config validates schema, arithmetic, role fields, secret permissions/format, PEM/trust parsing and policy; it does not bind ports, contact peers or claim network readiness. Return 0 on success/graceful SIGINT/SIGTERM completion, 2 on invalid config, 1 on fatal runtime initialization/permanent rejection. Fatal mapping initialization/identity/config failure before usable sessions reports rejection then exits. After startup, mapping-wide permanent rejection stops new setup and awaits operator restart; session-specific auth/policy/capability rejection closes only its flow and leaves unrelated accepted sessions/process alive; transient outages keep its listener and recover automatically. Operational NDJSON goes to stdout, structured events to stderr. Version includes build commit, Rust/quiche version and supported target/profile. Unknown flags/fields/schema versions fail early. SIGTERM/SIGINT stops admission, cancels setup/recovery and releases all resources within five seconds.

## TOML schema_version=1

Exactly one `[client]` or `[server]` role section is required. Explicit config values override defaults; no secret-bearing environment or CLI override. Paths resolve relative to config file directory. Client tokens and server keys/credential files must be regular files owned by the executing uid (or root for a provisioned server) and deny group/world access; reject symlink substitution/unsafe ownership at secure open. Do not echo contents on errors. Server certificate chain and CA bundle are public, private key and token files are not.

Initial bounded parser ceilings: configuration <=256 KiB, credentials <=4 MiB,
each PEM input <=1 MiB, <=16 distinct server listeners, <=128 CIDRs per target
rule, and identity IDs of 1..64 ASCII alphanumeric/`_-.` bytes. Token input is
at most 65 bytes including optional terminal LF. Encrypted private keys are
unsupported in v1 and fail without prompting; the leaf certificate must match
the private key. These startup checks do not establish peer trust or DNS policy.
See [ADR-0005](../../../docs/adr/0005-foundation-primitives.md) for descriptor-open
guarantees, provisioning assumptions and the conservative FD calculation.

Client example:

```toml
schema_version = 1
[client]
listen = "127.0.0.1:51821"
allow_non_loopback = false
server_address = "127.0.0.1:4433"
server_name = "remora.test"
proxy_authority = "remora.test:4433"
uri_template = "https://remora.test:4433/.well-known/masque/udp/{target_host}/{target_port}/"
ca_file = "ca.pem"
token_file = "client.token"
target_host = "127.0.0.1"
target_port = 9000
```

Server example (the digest is supplied by provisioning, not a literal template accepted at runtime):

```toml
schema_version = 1
[server]
listen = ["127.0.0.1:4433", "[::1]:4433"]
proxy_authority = "remora.test:4433"
path_prefix = "/.well-known/masque/udp/"
certificate_file = "server.pem"
private_key_file = "server-key.pem"
credentials_file = "credentials.toml"
```

Credential file shape:

```toml
[[identities]]
id = "wireguard-example"
token_sha256 = "<64 lowercase hex characters: SHA-256 of canonical token text>"
[[identities.targets]]
host = "127.0.0.1"
port = 9000
addresses = ["127.0.0.1/32"]
```

No wildcard credentials/targets. A valid client token is 64 lowercase hex characters representing 32 random bytes; permit a single terminal LF when loading, remove it before hashing/sending. Server stores hash of this canonical ASCII string, not hash of decoded binary bytes. Require unique identity IDs/digests, at most 128 identities and 128 target rules/identity. No identity label derived from arbitrary request input. Hostname rule requires all returned addresses inside addresses; reject mixed results. Numeric targets must satisfy the same rule. Canonical DNS labels, IP forms, mapped addresses and ports are validated as described in the data model. Default deny all destinations, including private/loopback. A target is pinned for its stream lifetime; reauthorization/re-resolution occur on recreation.

## Optional sections and documented initial defaults

Initial tuning values are hypotheses for qualification, not proof of performance. Every numeric setting is range checked with checked derived-budget arithmetic; an unachievable FD/config budget fails before forwarding. Config can reduce ceilings; increasing defaults must still meet recorded bounds and pass validation.

| Section/key | Default | Constraint / behavior |
|-------------|---------|-----------------------|
| limits.max_connections | 64 | Active plus handshaking; no allocation beyond ceiling |
| limits.max_sessions | 128 | Active plus pending, all identities |
| limits.max_sessions_per_identity | 32 | <= global; admission rejects on exhaustion |
| limits.max_sessions_per_connection | 32 | <= global; separate HTTP streams |
| limits.max_pending_setup | 16 | Includes DNS and authentication/authorization setup |
| limits.max_relay_sockets | 256 | Active plus bounded cleanup relay sockets; admission rejects at ceiling |
| limits.session_queue_packets | 32 | Per direction/session, reject newest on full |
| limits.session_queue_bytes | 65536 | Both directions/session combined; checked before allocation |
| limits.application_pool_bytes | 8388608 | Global retained slabs/queue ownership, not only lengths |
| limits.ciphertext_pool_bytes | 4194304 | Includes all generated unsent QUIC packets |
| limits.scratch_pool_bytes | 2097152 | Shared syscall/GRO scratch, independent of session count |
| limits.quiche_dgram_packets | 64 | Per connection/direction, explicit enable_dgram counts |
| limits.connection_dgram_bytes | 262144 | Send+receive bytes cap including internal queued ownership |
| limits.idle_session_ms | 120000 | Default two-minute UDP association inactivity timeout; positive finite; close stream and socket together; shorter override is a documented RFC SHOULD deviation |
| limits.application_residence_ms | 20 | Includes native outgoing quiche queue; conservative purge-all at earliest queued expiry |
| limits.ciphertext_residence_ms | 100 | Stuck generated output fails its connection, not recreated |
| io.mode | mmsg | mmsg or single; batch_max default 32, range 1..64 |
| io.gso / io.gro | off / off | off or auto; outer sockets only; unimplemented auto uses ordinary I/O with reason not_implemented; implemented auto probes capability |
| io.outer_socket_buffer_bytes | 262144 | Requested outer kernel buffers; report effective values/clamps |
| io.relay_socket_buffer_bytes | 65536 | Requested relay kernel buffers; report effective values/clamps |
| transport.cc | cubic | cubic, bbr2_gcongestion, reno; no generic bbr alias exposed |
| transport.pacing | timerfd | Mandatory user-space deadlines; no off mode in v1 |
| transport.outer_udp_max_bytes | 1350 | >=1200; PMTU probes and peer/path limits still apply |
| transport.application_max_bytes | 1200 | Actual limit may be smaller; empty payload supported |
| transport.idle_timeout_ms | 5000 | Finite liveness profile; keepalive when active flows otherwise idle |
| recovery.keepalive_interval_ms | 2000 | Active-flow otherwise-idle QUIC probe, normally paced |
| recovery.peer_progress_timeout_ms | 10000 | Watchdog based on authenticated peer progress, separate from quiche idle rules |
| recovery.setup_timeout_ms | 5000 | Bounded handshake and request setup |
| recovery.retry_initial_ms / retry_max_ms | 100 / 2000 | 100 ms scheduling floor plus capped exponential jitter; <=10 attempts/s/process, one attempt in flight |
| diagnostics.level | info | error/warn/info/debug/trace; same redaction at every level |
| diagnostics.snapshot_interval_ms | 1000 | Minimum 1000; bounded stdout NDJSON snapshots |
| diagnostics.qlog_directory | absent | Disabled by default; filtered data only, never raw H3 |

For each offload, accept `off` and `auto` even when its implementation is deferred. `off` uses ordinary selected single/mmsg I/O without an offload probe. Deferred `auto` also uses ordinary I/O, reports effective `off` with `not_implemented`, and must not claim a failed kernel/NIC probe. Once implemented, `auto` probes each applicable outer socket/path and permits effective `on` only when capability and eligibility checks pass; otherwise use the same baseline I/O. Export requested `off|auto`, effective `off|on` and a fixed reason `configured_off|not_implemented|capability_unavailable|path_ineligible|enabled`. Effective `on` describes offload availability on that socket/path; each individual packet/group still must satisfy metadata, segment-size and pacing constraints, with incompatible packets sent by baseline I/O. Bound and aggregate transitions; no per-packet event stream. An unsupported implementation/capability never silently alters payloads or pacing, and baseline fallback never resubmits an already accepted logical packet. Version/help and operator documentation distinguish implemented/qualified offloads from accepted settings with deferred implementation; no new mode or build framework is required.

Set both maximum receive and send outer UDP payload sizes explicitly. Reject larger complete outer messages after GRO splitting. Validate that twice the configured quiche queue count times the worst-case outer size fits connection_dgram_bytes; otherwise reduce counts or reject configuration. Transport control windows are explicitly capped: connection receive window 256 KiB, per-stream 16 KiB, H3 field section 16 KiB, QPACK dynamic capacity/blocked streams zero, at most eight peer unidirectional streams, stream setup limit tied to admitted requests. Set both initial and maximum flow windows, drain bodies/control incrementally and bound priority/settings metadata. A window controls flow bytes, not every native-library allocation; RSS and adversarial tests remain necessary.

Client listener defaults to loopback. An explicit non-loopback bind additionally requires allow_non_loopback=true. V4/v6 binds remain separate, IPV6_V6ONLY set. A remote numeric dial address and DNS certificate identity are different fields; no insecure/skip-verify/keylog option. URI host/authority must match the expected proxy identity/config; secrets are sent only after verified TLS handshake. Auth-disabled operation is unsupported.

The 120000 ms session inactivity default follows [RFC 9298 §3.1](https://www.rfc-editor.org/rfc/rfc9298.html#section-3.1). Activity is valid UDP traffic in either direction; QUIC keepalives alone do not refresh it. Expiry closes the request stream and its relay socket together. A shorter operator override is an intentional deviation from the RFC SHOULD and must document its deployment rationale: sparse traffic can trigger repeated setup, source-port changes and first-packet loss. No separate approval mechanism is introduced. This is distinct from the transport liveness timeout and SC-007 reclamation of already closed resources.

## Updates and failure contract

Replace credential/policy/trust files atomically with correct ownership/permissions, validate the candidate config, then stop old process and start replacement. No live reload promised. Removed identities/policies cannot create sessions in the new process; existing sessions end when the old process stops. Document old-process exit plus new readiness as revocation completion, and documented external UDP provenance limitations after tuple reuse/restart. No socket-quarantine duration is part of config schema_version=1. Rollback restores reviewed config and restarts; it may re-enable intentionally restored credentials, so operators must select the intended policy generation.

A client permanent auth/policy/capability rejection leaves a fixed-size admission latch on the immutable listener mapping until controlled restart, even after the individual flow is reclaimed. New sender tuples and transient connection recovery cannot bypass it; accepted unrelated sessions continue. Server requests are rejected/closed under existing admission limits, not retained in an unlimited deny cache.

No Remora command installs firewall rules, obtains certificates, edits WireGuard, sets routes, creates TUN, changes qdiscs/sysctls or manages deployment. The test harness may request namespace capabilities separately from the unprivileged runtime.
