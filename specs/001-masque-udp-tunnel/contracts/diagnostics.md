# Diagnostics contract

Version: 1. Requirements FR-021/022/023/027/028. Records contain observable operation, not application data.

## Streams and schema

stderr: NDJSON structured tracing events. stdout: NDJSON aggregate/status snapshots at most once/second during packet processing; the bounded shutdown-only terminal drain described below may bypass periodic cadence. Both use schema_version=1, UTC timestamp, monotonic sequence, role, process_epoch and a fixed event name. Privacy-safe opaque connection/session ids are randomly scoped to process lifetime; no tokens/digests, arbitrary header values, payload bytes, local source tuples or target names. Human errors retain safe reason/field names rather than serialized config/peer text. Any exported peer error text is replaced by numeric code and fixed reason enum.

Example snapshot shape (numeric values illustrative, not measured):

```json
{"schema_version":1,"event":"snapshot","role":"client","process_epoch":"opaque","sequence":1,"state":"active","sessions_active":1,"connections_active":1,"counters":{"app_ingress_datagrams":0,"app_ingress_bytes":0,"quic_dgram_enqueued":0,"relay_socket_sent":0,"local_drops":{}},"diagnostics_dropped":0}
```

Snapshots carry fixed reason/direction counters, bounded histograms, bounded live lifecycle state summaries and the terminal summaries defined below. Closed sessions aggregate into process totals after their bounded terminal summary is recorded; no unlimited per-session history or high-cardinality counter labels. Detailed summaries may be split into bounded <=64 KiB records with consistent snapshot sequence and <=admission-limit live entries. Opaque ids in bounded status records are correlation fields, never metric labels.

### Terminal failure correlation and collection limits (R5)

Record each terminal setup/session/connection failure in a fixed 256-entry, 64 KiB in-memory ring before cleanup and before attempting an ordinary log event. Each entry is at most 256 bytes of fixed-format state: terminal sequence, opaque connection/session generation ids when available, fixed failure class/numeric code, monotonic time and bounded repeat count. Nonterminal observable failures remain in bounded live status with summaries for each fixed failure class until a completed snapshot carries them; coalesce repeats but never overwrite a different pending class. Include those class summaries in the terminal record if the flow closes first. Connection-wide failures identify their connection and affected bounded live session ids, or emit one bounded terminal entry per affected session; all entries consume the same ring. No packet payload, source tuple, target name or credential is stored. No per-packet terminal history is generated. Repeated same-class events for one live association aggregate, while distinct terminal generations remain distinguishable.

Ordinary event suppression does not suppress these status records. The existing 1 MiB/1024-record writer budget reserves 256 KiB/256 records for status output; events/qlog use at most the remaining 768 KiB/768 records. Prioritize pending status batches over ordinary events/qlog at record boundaries. Do not keep more than one status batch in flight; each batch is <=256 KiB with <=64 KiB chunks. The writer reports successful batch completion to the owner by a bounded acknowledgement; retire only entries covered by that completed batch. New or updated entries must not be cleared by an older acknowledgement. Failed/dropped writes leave entries pending for a later snapshot. Packet processing never waits for the sink. Preserve total queue accounting across both reservations.

If the ring fills before completed output, evict the oldest pending entry with a monotonic coverage-loss count and lost-sequence range; expose status write failures, ring overflow and ordinary event loss separately. Their summaries remain in cumulative counters, but missing session correlation is not reconstructed or claimed. A blocked/broken sink or terminal churn above retention capacity cannot guarantee delivery of every session's diagnostics. SC-010 is verified with a functioning collector and injected failures within the documented status capacity; ordinary event-bucket exhaustion is explicitly included. Every injected failure must be correlated and coverage-loss counts must remain zero. An overflow/failed collection is a failed or invalid SC-010 run, never a pass. Separate robustness tests intentionally exceed the ring or block the sink and verify bounded memory, explicit coverage gaps and uninterrupted packet processing; they do not establish complete diagnostic coverage.

### Shutdown collection

After packet work and session/socket cleanup end, drain pending terminal status
within one shared one-second collection deadline, inside the overall shutdown
budget. This shutdown-only operation may bypass the ordinary snapshot interval;
one batch in flight, reserved capacity and ACK coverage rules still apply.
Wait for an older batch's ACK before publishing updated/new terminal entries.
Do not clear pending entries merely because they were queued. An incomplete
terminal drain or unfinished writer produces the fixed
`DiagnosticCollectionIncomplete` runtime failure and a nonzero exit, including
when ordinary logging is suppressed. A functioning collector within the
declared capacity must receive short-lived failures at shutdown even with a
long configured snapshot interval. Blocked/broken sinks remain bounded and
cannot guarantee correlation delivery.

## Required measurements and meanings

| Signal | Meaning and boundary |
|--------|----------------------|
| Starting/Active/Recovering/Rejected/Stopped | Request/session lifecycle; Active does not prove target response |
| app_ingress/app_egress datagrams and bytes | Accepted full application UDP messages at role boundary, direction explicit |
| quic_dgram_enqueued | Accepted by quiche send queue; not socket sent or peer delivered |
| outer_socket_sent/received | Successfully submitted/received UDP packets and bytes; GSO counts logical segments separately from syscall count |
| relay_socket_sent | UDP syscall accepted message; not target delivery proof |
| local_drops{reason,direction} | Observable rejection at this process, each packet counted once at terminal drop boundary |
| kernel_rx_overflow | SO_RXQ_OVFL cumulative deltas with wrap handling; per socket, no fabricated session attribution |
| quic_packets_lost/retransmitted/PTO | Outer QUIC transport observations/estimates, never equated with inner UDP loss |
| rtt/rttvar/cwnd/path_payload_limit | Current path stats sampled <=once/5 seconds; peer path ids bounded |
| queue packets/bytes/age and pool capacities | Retained memory/accounting, high-water marks, pressure and expiration |
| syscall/batch/offload counters | Partial progress, EAGAIN, logical segments and per-offload requested off/auto, effective off/on, fixed reason configured_off/not_implemented/capability_unavailable/path_ineligible/enabled; socket/path availability is separate from per-packet eligibility |
| pacing_lateness / emitted burst size | Fixed histogram; no per-packet logs |
| recovery attempts / duration / failure class | Transient versus permanent, authorization rechecks and generation changes |
| FD/setup/session/connection gauges | Active, pending, bounded cleanup and admission rejection |

Fixed local-drop reasons: not_active, recovering, rejected, oversized, malformed, unknown_flow, unknown_context, unsupported_carrier, queue_full, queue_expired, stale_generation, socket_truncated, ancillary_invalid, socket_error, pmtu. No quarantine-specific state/counter is required; any chosen additional isolation mechanism must use bounded privacy-safe diagnostics. Reason enums are versioned; arbitrary error strings cannot become metric labels. Global drops are not added to every per-session total. End-to-end delivered loss is produced only by the benchmark sequence/integrity harness.

## Bounded overhead and qlog privacy

Per-process ordinary event token bucket: 20 events/second with burst 40, aggregate repeated failures. Lifecycle detail logs share this bucket; terminal/live failure summaries use the separate bounded status path above. Bounded nonblocking diagnostic queue: 1 MiB and 1024 records including the status reservation; slow sinks drop/count diagnostic records and report lost coverage distinctly. No blocking disk/stdout operations in packet loop. Snapshot interval >=1 second. On shutdown, bounded flush timeout consumes the same five-second global shutdown budget; unflushed summaries are not claimed delivered.

Optional qlog is disabled by default and follows the first E2E MVP and safe counters/status; its filter must not delay initial forwarding. Enabling it creates a bounded per-connection incremental JSON-SEQ filter that parses upstream records in memory, permits only an explicit transport/recovery event/field schema, and reconstructs permitted records before sending to the writer. Drop every HTTP/3 event, headers, raw payload/frame bytes, custom metadata, close reason text and unknown events/fields. Invalid/oversize record or unrecognized schema disables that connection's qlog fail closed; never fall through to raw output. Filter scratch <=64 KiB/connection, at most eight traced connections; reserve against a separate 512 KiB global filter pool before activation. Total diagnostics memory is capped at 2 MiB: 1 MiB output queue plus 512 KiB filter scratch plus 512 KiB status/record/format/snapshot storage (64 KiB terminal ring, 448 KiB all remaining live-status, formatting, snapshot and parser capacities). Queue/status ids and acknowledgement bookkeeping also fit those reservations. Reject tracing activation when reservation fails. QUIC packet numbers, lengths, timing, numeric recovery metrics and safe frame type/length metadata may be retained; data contents and identifiers that expose users are excluded.

Qlog rate <=1000 permitted events/second/process and <=1 MiB/second; retained files <=16 MiB/connection, <=128 MiB/process-directory and max eight simultaneously traced connections, then tracing stops with a bounded event. Disk capacity and writer queues are independent, finite budgets. Never call quiche keylog APIs or forward SSLKEYLOGFILE. Arbitrary quiche `log` targets are not automatically forwarded at trace verbosity; audit and allowlist safe metadata formats at the pinned version.

Tests must exercise event suppression plus short-lived terminal failures, cleanup before snapshot, old acknowledgement versus new summary, status reservation under event/qlog saturation, terminal ring overflow, failed/blocked writers, partial JSON-SEQ writes, malicious huge records, filtered H3 authorization values, payload sentinels, every log level, file caps, churn/cardinality bounds and diagnostic-overhead A/B runs. Verify SC-010 correlation with zero coverage loss separately from deliberate-overflow robustness. Raw upstream qlog must never touch the filesystem even temporarily. Source review: [tagged H3 qlog records](https://raw.githubusercontent.com/cloudflare/quiche/0.30.0/quiche/src/h3/mod.rs).

### Phase 4 resource snapshot increment

`resources` adds fixed numeric admission gauges and peaks (`connections`,
`sessions`, `setup`, `relay_fds`, `fd_reservations`), application queue packets/
bytes, retained application/ciphertext/scratch slab bytes, pool/queue descriptor
capacity, native send/receive queued bytes and connection output/native descriptor
capacity. Fields aggregate only bounded live state; no identity/source/target
labels are added. `sessions` counts admission permits for pending, active and
bounded closed-control cleanup; `sessions_active` retains its existing Active-only
meaning. Closing cleanup has no relay/payload ownership and releases its permits
on native collection or the connection's bounded terminal failure. Descriptor
capacity includes the closing-ID ledger but does not account for every native transport
or TLS/control allocation. Churn evidence reports process RSS, the existing
2 MiB diagnostic retention ceiling, effective kernel buffer capacities and
owned-socket kernel queue allocations separately. Full operational telemetry
remains T064, rather than inferred from these scoped gauges.

## Incremental Phase 5 numeric profiling fields

Schema 1 snapshots add a fixed `profile` object, with cumulative counters and
lifetime maxima, never endpoint labels or payload samples. Adapter counters
are actual receive/send/error-queue syscalls, MTU queries and logical UDP
messages accepted/received (including empty messages); count accepted prefixes
once even if a later operation fails. Merge retired relay totals into one
process accumulator. Arithmetic overflow marks the sample invalid and must not
turn a successful syscall into an apparent failed send/replay.

`owner_visits` counts scheduler selections, not OS wakeups. CPU/headroom and
owner voluntary/involuntary context switches are sampled separately by the
harness. Allocation counters cover payload slabs and custom quiche datagram/
stream factories; copy-byte counters cover explicit queue/receive/factory
boundaries. They exclude cryptographic and other allocator/metadata activity.
Queue/ciphertext age and pacing lateness are maxima in microseconds; burst is
the largest actually accepted outer-send prefix in logical UDP messages.
These bounded scalar measurements do not imply full allocator tracing or
latency histograms, and a lifetime maximum is not a measurement-window p95.

`outer_pmtu_errors` counts synchronous/validated error-queue observations, not
unique lost packets. `destination_pmtu_updates` counts validated route-update
observations. `outer_quic_lost_packets` is the delta of quiche's monotonic
connection lost-packet estimate, including non-application/probe packets.
Application `local_drops.pmtu`, `kernel_rx_overflow` and sequence-based delivered
loss remain separate. Error-queue quotes never enter diagnostics.

The native burst characterization exposed synchronous ENOBUFS (errno 105)
from runner-owned netem queue pressure. Preserve the association: drop only the
unaccepted outer FIFO front, count `outer_send_pressure`, and allow QUIC control
recovery. It is not a PMTU observation or fabricated Session application loss.
Relay ENOBUFS drops the exact application packet with `socket_error` without
closing its Session. EAGAIN continues to retain its original suffix/deadline;
hard errors retain numeric errno in the connection failure record. Adapter
`send_enobufs`/`send_emsgsize` count actual failed native submissions, separately
from PMTU prechecks. Scripted accepted-prefix tests cover both failure classes;
original native failures remain retained before the fix.

Asynchronous receive/send peer/ICMP errno observations can race a bounded
error-queue drain; preserve the original association and continue another owner
turn rather than treating unauthenticated ICMP as connection/Session closure.
`outer_async_errors` counts such outer observations, including failed sends on
a shared socket without attributing failure to its current connection. Relay
failed sends additionally count their unaccepted application packet under
`local_drops.socket_error`; target observations use fixed socket failure/numeric
errno records. These are observations, not unique ICMP/lost-packet estimates.
EMSGSIZE errno alone cannot identify the affected QUIC path; revalidate only
with checked error-queue destination metadata matching an active path.
Bounded interrupted receive retries
consume nothing and continue without cancellation starvation. Complete outer
oversize/truncation/ancillary drops use the role's receive direction.

The established trust failure code remains `trust/0` (including TLS certificate
name failure). Additional native transport diagnostics use fixed categories
0..22 (Done, BufferTooShort, UnknownVersion, InvalidFrame, InvalidPacket,
InvalidState, InvalidStreamState, InvalidTransportParam, CryptoFail, TlsFail,
FlowControl, StreamLimit, StreamStopped, StreamReset, FinalSize,
CongestionControl, IdLimit, OutOfIdentifiers, KeyUpdate, CryptoBufferExceeded,
InvalidAckRange, OptimisticAckDetected, InvalidDcidInitialization); associated
peer stream/error values are omitted. H3 failures use standard wire categories,
with local transport errors at 0x10000 + native category, StreamBlocked at
0x20000 and local BufferTooShort at 0x20001. Local expiry/capacity/protocol
packet-generation failures are 1001/1002/1003. Close observations retain only
the numeric native local/peer close code, never its reason text. These additions
do not replace the established authentication/authorization/capability/trust
refusal contracts or expose network-controlled strings.
