# Data model and lifecycle

Design-only, no database schema. Initial values and interface validation are in [CLI/config](contracts/cli-config.md). The accepted [A1 decision](proposed-amendments.md) specifies observable isolation and UDP provenance limitations; no particular late-reply retention mechanism is required.

## Entities and relationships

| Entity | Fields | Identity and relationships |
|--------|--------|----------------------------|
| ValidatedConfig | schema_version, role, socket addresses, TLS/trust paths, credential/policy records, resource/I/O/recovery/diagnostic budgets | Immutable after startup; restart replaces an entire policy generation |
| ClientCredential | operator identity, token SHA-256 digest, allowed target rules, per-identity session quota | Identity unique in config; token secret loaded only by client, digest only by server; identity not an unbounded metric label |
| TargetRule | canonical requested host, port, approved address/CIDR set | Exact host/port rule plus resolved-address permission; wildcard host/port permissions excluded initially |
| ListenerMapping | listener_id, bind, server dial endpoint, TLS server_name, proxy authority/template, credential, target host/port, permanent rejection latch/reason | One mapping/process initially; source tuple plus local destination tuple and listener_id identifies local flow; fixed-size client latch survives flow cleanup until controlled restart |
| ConnectionState | opaque local id, generation, owned quiche/H3 state, role, issued/retired CID map, negotiated settings, path state, timers, bounded queues | One owner; connection generation never reused; request stream IDs unique within its connection |
| SessionState | opaque id, generation, listener/source/local-destination tuples or server identity, connection generation, stream_id, target, lifecycle, idle/setup deadlines, counters, payload ceiling | Client map (listener_id, source IP/port, local destination IP/port) -> live generation; server map (connection generation, stream_id) -> accepted identity/target/socket |
| ResolvedTarget | requested canonical host, approved numeric SocketAddr, resolution/policy generation, destination PMTU | One pinned address/session; all DNS answers authorized before selection; no automatic target change |
| UdpRelay | owned nonblocking connected fd, local/remote numeric tuple, owner session generation, buffers/metadata | One live session; receives only configured peer; closure invalidates the association and releases resources within bounded cleanup |
| DatagramDescriptor | slab ownership, offset/length, direction, session+connection generation, received_at, expires_at | Opaque bytes; zero length valid; no per-packet secret/payload diagnostic fields |
| CiphertextDescriptor | owned slab, UDP length, SendInfo.from/to/at, connection generation, output sequence, generated_at, expires_at | Successful packet generation is transport commitment; immutable connection-owned output may mix streams/control, survives individual session close only on its original generation; FIFO and <=100 ms residence, never recreated or selectively rewritten |
| BudgetLedger | admitted connections/sessions/setup/relay fds, bounded cleanup, pooled capacities, queued counts/bytes, per-identity totals | Single-owner accounting; checked allocation/return, no saturating arithmetic hiding overshoot |
| TerminalSummary | monotonic terminal sequence, opaque connection/session generation ids, fixed failure class/code, monotonic time, repeat count | Fixed 256-entry ring within 64 KiB; independent of log bucket; completed output retires acknowledged entries, overflow is explicit coverage loss, not unbounded history |
| DiagnosticSnapshot | schema_version, process_epoch, monotonic sequence, role, aggregate counters, fixed histograms, bounded live state ids, terminal summaries, coverage-loss counters | No credentials, headers, payloads, user source tuples or target names; ordinary period at least one second with the bounded shutdown-only exception in the diagnostics contract; status queue reservation and writer acknowledgement preserve terminal correlation under ordinary event suppression |

## Connection state machine

`Starting -> Handshaking -> Negotiating -> Active` after verified TLS and required peer settings. Transient failure: `Active/Handshaking/Negotiating -> Recovering -> Handshaking` with a fresh generation on client. Mapping-wide identity/config failure: `-> Rejected`, waiting for operator action after startup; fatal initialization before any usable session may exit. A session-specific credential/policy/capability failure rejects only that request, leaving other accepted streams usable. Stop from any state: `-> Stopping -> Stopped`. The server closes/reclaims a failed connection rather than initiating outbound reconnects.

Connection Active is transport readiness, not proof of UDP endpoint liveness. A healthy connection may contain a rejected session. Validated QUIC path migration/rebinding and new-connection recovery must be distinguished in events; neither permits a changed target/policy binding.

## Session state machine

Client: `Starting -> AwaitingAcceptance -> Active`; server: `Starting -> Authenticating -> Authorizing -> Active`. Rejection or setup timeout discards cancellable pending packets and closes only the affected stream. A permanent authentication/policy/capability rejection sets a fixed-size admission latch on the client's immutable listener mapping (same credential/target/profile), retained until controlled restart. Flow-state cleanup, a new sender tuple or transient reconnect cannot clear it. Already accepted unrelated sessions continue; newly arriving denied packets drop/count without another setup. The server retains no unlimited rejected-client/source history: reject/close each request under its existing admission/work budgets, then keep only bounded diagnostic summaries. Ordinary expiry/reset: `Active -> Closing -> Closed`; release the relay association within bounded cleanup and reject late work using its original session generation. Client connection failure marks flows Recovering, discards all old generation work/output and drops unavailable traffic; recreation repeats TLS/settings/auth/policy. Incoming traffic during setup is counted and dropped rather than retained.

No forward eligibility before Active on the corresponding accepted request. Closed association state cannot forward, resurrect a stream or retain payloads for reassignment. Individual close purges uncommitted application/relay/native DATAGRAM queues, not already generated mixed ciphertext. Committed output may complete only on the original connection/stream within its original deadline; connection replacement or stop discards unsent output. No Quarantined state or fixed-duration socket reservation is required. Opaque external reply ambiguity after endpoint-tuple reuse/restart is distinct from both identifiable stale generation data and transport commitment; see the [plan's commitment boundary](plan.md#transmission-commitment-and-session-closure-r1).

## Validation and invariants

- Parse and normalize host/port once. DNS names are ASCII canonical form; non-ASCII/zone identifiers and ambiguous numeric forms are rejected in v1. IP literals use standard parsed addresses, including normalized mapped addresses. Port 1..65535; no multicast/broadcast/unspecified targets. Loopback/private/link-local require explicit numeric permissions; unscoped IPv6 link-local is rejected.
- A request must match the configured proxy authority and path template, pass unique required header validation, then credential and destination checks. Duplicate/oversized credential headers never become policy inputs.
- DNS lookup is globally bounded and timed out; permission applies to every answer. The chosen numeric result is pinned into the connected socket. Late/cancelled lookup completion carries its original generation and is ignored.
- (connection generation, request stream_id) defines tunnel association; quarter stream_id arithmetic is checked. Context 0 contains unchanged UDP payload; unknown contexts drop without private semantics.
- Datagram payload ceiling is min(current transport capacity minus HTTP/context prefixes, destination-leg limit when known, configured application limit). Check on enqueue and actual relay send, because path size can change.
- Closed sessions cannot receive, enqueue or commit new application traffic. Purge queued quiche datagrams for the closed stream using a validated association predicate before the next packet generation. Already committed output remains immutable on its original connection generation; local close cannot recall it or establish an instantaneous remote-delivery cutoff. Unknown/closed stream datagrams drop on receipt; stale events cannot create new sessions.
- Partial socket progress transfers ownership of accepted descriptors exactly once. Receive slabs are not recycled while protocol code or syscall headers borrow them. GSO group ownership persists until acceptance/failure is known; GRO split views never outlive the aggregate slab.
- Output ordering and accounting are per connection, not per session. EAGAIN/partial send plus session close cannot recreate a packet, reuse ciphertext in a new generation, silently filter mixed frames or force unrelated streams closed merely for session cleanup. A real connection output timeout fails that connection; discard its unsent descriptors with the old transport state.
- Every queue/pool/setup/FD/identity allocation is debited first and credited once. Ciphertext/control buffers and kernel socket memory are not hidden inside the application pool budget. Heap capacities retained after churn count toward memory verification.
- Distinct active destination associations and owner/generation checks prevent cross-client routing or retained-data reuse. Any additional reuse mitigation must have documented, finite resource/lifecycle costs; resource exhaustion cannot bypass isolation or authorization. Tests must distinguish enforceable stale-generation rejection from externally indistinguishable UDP provenance after tuple reuse.
- Metrics expose fixed reason/direction enums. Live-session status is bounded by admission; closed-session detail exists only in the finite terminal-summary ring until completed output or explicitly reported overflow, then aggregates into totals. No indefinite history or high-cardinality metric labels.

## Persistent state and updates

Only configuration, trust material, client secret files, server credential digests and optional bounded diagnostics persist; no persistent port-reservation store is mandated. Config/credential/policy updates require a controlled process restart; the revocation completion point is old-process exit plus new-process readiness. No migration/database schema is needed. Config schema_version=1 rejects unknown fields; future incompatible schemas require explicit migration guidance.

## Phase 5 request ordering and numeric observations

A server pending Session may retain request headers before its peer SETTINGS
are available: State::Starting, <=32 fields/16KiB, charged to the existing
global/per-connection Session and pending-setup permits. Preserve its original
setup start/deadline; no identity/DNS/relay/acceptance/forwarding work starts
until required peer capabilities are verified. Reset, duplicate headers,
expiry/close and connection capability failure cancel this bounded header
reservation. An independent seventh four-item cursor services ready deferred
requests; these fields are not diagnostics, metric labels or a new unbounded
connection queue. Count their exact retained field/metadata bytes in resource
snapshots. Existing commitment, generations and cleanup permits are unchanged.

PMTU observations retain only numeric errno/MTU and the current validated
original destination while handled; quotes are discarded. Target route ceilings
are re-queried at send, and current native/verified QUIC limits at enqueue and
generation. Outer pressure/PMTU/lost-packet estimates have no invented Session
application-loss attribution. Fixed lifetime counters/maxima cover controlled
I/O/copy/factory/slab boundaries; overflow invalidates measurements without
changing already accepted send prefixes. Retired relay totals fold into one
process accumulator rather than unbounded per-flow history.
