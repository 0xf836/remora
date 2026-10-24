# CLI and configuration contract

Version: 1. Commands and startup validation are implemented; schema_version=1 is the sole configuration format. Requirements FR-001/006/007/009/011/012/017/020/024/025/028/029.

## Commands and process lifecycle

```text
remora client --config PATH
remora server --config PATH
remora check-config --config PATH
remora --version
remora --help
remora client|server|check-config --help
```

PATH is configuration, never a token. check-config validates schema, arithmetic, role fields, secret permissions/format, PEM/trust parsing and policy; it does not bind ports, contact peers or claim network readiness. Return 0 on success/graceful SIGINT/SIGTERM completion, 2 on invalid config, 1 on fatal runtime initialization/permanent rejection. Fatal mapping initialization/identity/config failure before usable sessions reports rejection then exits. After startup, mapping-wide permanent rejection stops new setup and awaits operator restart; session-specific auth/policy/capability rejection closes only its flow and leaves unrelated accepted sessions/process alive; transient outages keep its listener and recover automatically. Operational NDJSON goes to stdout, structured events to stderr. Version includes build commit, Rust/quiche version and supported target/profile. Unknown flags/fields/schema versions fail early. SIGTERM/SIGINT stops admission, cancels setup/recovery and releases all resources within five seconds.

## TOML schema_version=1

The public [configuration reference](../../../docs/configuration.md) specifies
all fields, defaults and constraints. Minimal public examples
are [client](../../../examples/client.toml) and [server](../../../examples/server.toml).
Exactly one role section is required. `proxy` is an HTTPS origin: port 443
is optional, other nonzero ports are explicit, and IPv6 is bracketed. An empty
or root path is accepted; credentials, non-root paths, queries, fragments,
unbracketed IPv6 and non-HTTPS schemes are rejected with a safe field error.
Equivalent default-port and root-slash forms normalize to one authority with
443 omitted; nondefault ports remain explicit. Client `target` and numeric
`connect_address` require an explicit port in host:port / [IPv6]:port syntax.
TLS identity, canonical HTTP authority
and the standard HTTPS URI template derive from `proxy`. Optional
`connect_address` overrides only the numeric transport destination; an advanced
`uri_template` must still match the canonical proxy authority. Server `proxy`
is independent of its numeric listeners; `path_prefix` defaults to the standard
MASQUE route. Neither role has a default listener.

Proxy DNS uses the existing bounded resolver worker pool (at most
`max_pending_setup` jobs), a setup-timeout per lookup and at most 128 answers.
Normalize and deduplicate usable numeric addresses, alternate families, and
try one bounded handshake at a time with normal recovery backoff. Exhaustion
re-resolves; successful connection setup discards the remaining list so the
next recovery re-resolves. Cancel the owned lookup on rejection/stop. Actual
uncancellable libc workers keep their permits; no stale callback mutates owner
state. Proxy address selection never changes TLS reference identity. DNS SAN
and IP SAN verification precede H3/authentication. Target DNS retains its
separate authentication-before-resolution and all-answer authorization rules.
See [ADR 0026](../../../docs/adr/0026-endpoint-configuration.md).

Schema/type errors name only compile-time field/table names and expectations;
syntax errors include source line/column without input. Successful check-config
prints `configuration is valid (client)` or `(server)` and still performs no
DNS, bind or handshake. Runtime/exit semantics below remain in force.

Exactly one `[client]` or `[server]` role section is required. Explicit config values override defaults; no secret-bearing environment or CLI override. Paths resolve relative to config file directory. All inputs must be regular files; reject symlinks at every path component. Private-file metadata and ACL checks use the held descriptor. Do not echo contents on errors. Server certificate chain and CA bundle are public, private key and token files are not.

Without an extended Linux POSIX access ACL, client tokens must be owned by the
executing UID; provisioned server keys/credentials may also be root-owned.
Require zero group/other mode bits, preserving ordinary `0400`/`0600` handling.
An encoded base-only ACL follows the same ordinary ownership/mode rules.
For an extended access ACL, trust only root or the executing UID as inode owner, and
require zero effective permissions for every group, unrelated named UID and
`other`. Apply the mask to named users and groups, never owner/other. Named root
and the executing UID are trusted. A root-owned file delegated to a non-root
executing UID must grant that UID exactly effective read permission via a named
entry. This explicit exception applies equally to client tokens and both server
secrets; it does not allow plain group-readable `0440`/`0640` files.

Read `system.posix_acl_access` once through the held FD with a 64 KiB ceiling;
validate Linux xattr version 2, canonical tag-class ordering/unique qualifiers,
permissions, required base entries/mask, and consistency with inode mode.
`ENODATA` and `EOPNOTSUPP` select only the strict ordinary-file policy. Any other
read error, malformed ACL, oversize input or observed metadata change fails
closed with a fixed `Permissions` error. Numeric named UID/GID order is
unrestricted. Check uniqueness within each class using a fallibly allocated
copy of at most 8191 qualifier pairs (less than 64 KiB requested), sorted without
modifying ACL entries. Allocation failure rejects. No unbounded retry or path
reopening.
For ACL acceptance, obtain the kernel overflow UID from a bounded procfs read;
reject an inode owner equal to it, failed procfs inspection, and undefined or
unmapped named IDs. Compare kernel-translated ACL IDs and metadata in the
process's current UID namespace; do not interpret on-disk IDs independently.
This specifies Linux POSIX ACL behavior, not NFSv4 or arbitrary remote ACLs.
Path names and environment variables never establish trust. See
[ADR 0029](../../../docs/adr/0029-private-file-acls.md).

Initial bounded parser ceilings: configuration <=256 KiB, credentials <=4 MiB,
each PEM input <=1 MiB, <=16 distinct server listeners, <=128 CIDRs per target
rule, and identity IDs of 1..64 ASCII alphanumeric/`_-.` bytes. Token input is
at most 65 bytes including optional terminal LF. Encrypted private keys are
unsupported and fail without prompting; the leaf certificate must match
the private key. These startup checks do not establish peer trust or DNS policy.
See [ADR-0005](../../../docs/adr/0005-foundation-primitives.md) for descriptor-open
guarantees, provisioning assumptions and the conservative FD calculation.

Client and server examples are maintained in the public reference and
`examples/`; protocol-derived TLS/authority/template values are not separate
required configuration fields.

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
| limits.application_residence_ms | 20 | Includes native outgoing quiche queue; each datagram keeps its original deadline and only expired entries are purged |
| limits.ciphertext_residence_ms | 100 | Stuck generated output fails its connection, not recreated |
| io.mode | mmsg | mmsg or single; batch_max default 32, range 1..64 |
| io.gso / io.gro | off / off | off or auto; outer sockets only; unimplemented auto uses ordinary I/O with reason not_implemented; implemented auto probes capability |
| io.outer_socket_buffer_bytes | 1048576 | Requested outer RX/TX kernel buffers; report effective values/clamps; explicit 262144 retains the former default; include per-socket kernel ceilings/counts separately from process RSS |
| io.relay_socket_buffer_bytes | 131072 | Requested RX/TX buffers for client application listener and server relay sockets; report effective values/clamps; explicit 65536 retains the former default |
| transport.cc | cubic | cubic, bbr2_gcongestion, reno; no generic bbr alias exposed |
| transport.pacing | timerfd | Mandatory user-space deadlines; no off mode |
| transport.hystart | true | Classic CUBIC/Reno HyStart++ setting; experimental false profile requires separate qualification; BBR2 does not use this detector |
| transport.outer_udp_max_bytes | 1452 | >=1200; PMTU probes and peer/path limits still apply |
| transport.application_max_bytes | 1400 | Actual limit may be smaller; empty payload supported |
| transport.idle_timeout_ms | 5000 | Finite liveness profile; keepalive when active flows otherwise idle |
| recovery.keepalive_interval_ms | 2000 | Active-flow otherwise-idle QUIC probe, normally paced |
| recovery.peer_progress_timeout_ms | 10000 | Watchdog based on authenticated peer progress, separate from quiche idle rules |
| recovery.setup_timeout_ms | 5000 | Bounded handshake and request setup |
| recovery.retry_initial_ms / retry_max_ms | 100 / 2000 | Validate 100 <= initial <= max <=2000; scheduling floor plus uniform capped exponential jitter, total delay <=max; <=10 attempts/s/process, one handshake in flight |
| diagnostics.level | info | error/warn/info/debug/trace; same redaction at every level |
| diagnostics.format | text | text: human operator profile; json: legacy structured diagnostics |
| diagnostics.snapshots | false | JSON only: true enables periodic metrics; false retains mandatory failure status in JSON. Text has no status stream |
| diagnostics.snapshot_interval_ms | 1000 | Minimum 1000; JSON status cadence; operator lifecycle events do not wait for this interval |
| diagnostics.qlog_directory | absent | Disabled by default; qlog build feature plus fresh empty owner-0700 directory; filtered NDJSON only, never raw H3 |

Mandatory pacing includes deferred generation as well as committed output.
For the pinned BBR2/gcongestion backend, capture the current release decision
before generation and bound lookahead by the backend recommendation and half
the configured ciphertext residence. Retained native input keeps its original
application deadline; a connection-owned timer resumes generation. Classic
CUBIC retains SendInfo timing. Neither path extends a committed deadline to
accommodate batching; see ADR 0033.

For each offload, accept `off` and `auto` even when its implementation is deferred. `off` uses ordinary selected single/mmsg I/O without an offload probe. Deferred `auto` also uses ordinary I/O, reports effective `off` with `not_implemented`, and must not claim a failed kernel/NIC probe. Once implemented, `auto` probes each applicable outer socket/path and permits effective `on` only when capability and eligibility checks pass; otherwise use the same baseline I/O. Export requested `off|auto`, effective `off|on` and a fixed reason `configured_off|not_implemented|capability_unavailable|path_ineligible|enabled`. Effective `on` describes offload availability on that socket/path; each individual packet/group still must satisfy metadata, segment-size and pacing constraints, with incompatible packets sent by baseline I/O. Bound and aggregate transitions; no per-packet event stream. An unsupported implementation/capability never silently alters payloads or pacing, and baseline fallback never resubmits an already accepted logical packet. Version/help and operator documentation distinguish implemented/qualified offloads from accepted settings with deferred implementation; no new mode or build framework is required.

Set both maximum receive and send outer UDP payload sizes explicitly. Reject larger complete outer messages after GRO splitting. Validate that twice the configured quiche queue count times the worst-case outer size fits connection_dgram_bytes; otherwise reduce counts or reject configuration. Transport control windows are explicitly capped: connection receive window 256 KiB, per-stream 16 KiB, H3 field section 16 KiB, QPACK dynamic capacity/blocked streams zero, at most eight peer unidirectional streams, stream setup limit tied to admitted requests. Set both initial and maximum flow windows, drain bodies/control incrementally and bound priority/settings metadata. A window controls flow bytes, not every native-library allocation; RSS and adversarial tests remain necessary.

Client listen is required; the example binds loopback. An explicit non-loopback bind additionally requires allow_non_loopback=true. V4/v6 binds remain separate, IPV6_V6ONLY set. A wildcard listener preserves the local destination IP/port from packet info as its reply source, including replies queued after EAGAIN. Associations include both sender and local destination tuples: one sender addressing two local listener IPs uses separate bounded sessions, and neither can rewrite the other's reply source. The optional numeric dial override does not change proxy certificate identity; no insecure/skip-verify/keylog option. URI host/authority must match the expected proxy identity/config; secrets are sent only after verified TLS handshake. Auth-disabled operation is unsupported.

The 120000 ms session inactivity default follows [RFC 9298 §3.1](https://www.rfc-editor.org/rfc/rfc9298.html#section-3.1). Activity is valid UDP traffic in either direction; QUIC keepalives alone do not refresh it. Expiry closes the request stream and its relay socket together. A shorter operator override is an intentional deviation from the RFC SHOULD and must document its deployment rationale: sparse traffic can trigger repeated setup, source-port changes and first-packet loss. No separate approval mechanism is introduced. This is distinct from the transport liveness timeout and SC-007 reclamation of already closed resources.

## Updates and failure contract

Replace credential/policy/trust files atomically with correct ownership/permissions, validate the candidate config, then stop old process and start replacement. No live reload promised. Removed identities/policies cannot create sessions in the new process; existing sessions end when the old process stops. Document old-process exit plus new readiness as revocation completion, and documented external UDP provenance limitations after tuple reuse/restart. No socket-quarantine duration is part of config schema_version=1. Rollback restores reviewed config and restarts; it may re-enable intentionally restored credentials, so operators must select the intended policy generation.

A client permanent auth/policy/capability rejection leaves a fixed-size admission latch on the immutable listener mapping until controlled restart, even after the individual flow is reclaimed. Cancel every not-yet-accepted client Session, including queued Starting and AwaitingAcceptance, releasing its setup work and sender association. Neither queued CONNECT submission nor a delayed successful response can activate a Session after the latch. New sender tuples and transient connection recovery cannot bypass it; already accepted unrelated sessions continue. Server requests are rejected/closed under existing admission limits, not retained in an unlimited deny cache.

No Remora command installs firewall rules, obtains certificates, edits WireGuard, sets routes, creates TUN, changes qdiscs/sysctls or manages deployment. The test harness may request namespace capabilities separately from the unprivileged runtime.

### Phase 7 qlog activation

A configured qlog directory now requires the optional `qlog` build feature.
Startup/check-config refuse a missing, nonempty, symlinked, public or foreign-owned
directory. Validation performs no file creation/bind/peer traffic. The operator
creates a fresh empty mode-0700 directory per role/start and owns parent paths.
The writer uses its held directory FD and mode-0600 create-new files; no existing
output is deleted/overwritten. Sanitized schema-1 Remora NDJSON is not an upstream
qlog replay file. This activates the previously deferred option without changing
wire/config schema version; see [operations](../../../docs/operations.md#optional-filtered-qlog).
