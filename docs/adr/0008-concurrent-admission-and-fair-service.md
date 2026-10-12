# ADR-0008: Concurrent admission and fair service

Status: accepted for Phase 4 implementation; qualification is recorded in
[concurrency validation](../validation/concurrency.md).

## Context and options

The MVP bounds global sessions/setup/relay sockets and connections, but checks
an identity's quota only after DNS. Pending requests can occupy setup work
without that identity's debit. Packet input also enters the shared native
DATAGRAM queue immediately. A busy producer can fill it before other sessions
are visited. Selecting the first four HashMap bodies can starve a fifth body.

Consider immediate native enqueue with only socket rotation, a byte deficit
scheduler, and finite per-session queues with datagram round-robin. Choose the
last option: UDP datagram units preserve boundaries, including empty payloads,
and need no new runtime, task, dependency or packet priority interpretation.
No speedup or full capacity claim motivates this correctness change.

## Decision

Authenticate a request against immutable configured identities, debit a fixed
identity ledger before DNS and retain its noncloneable permit through native
control cleanup. Pending, active and cleanup associations count together. Global/setup/connection
limits apply before work allocation; relay and FD permits precede socket creation.
Exhaustion rejects the new request with 503 and preserves admitted streams.
Cancellation releases asynchronous work and permits together. Duplicate/stale
setup completions cannot replace a socket or resurrect an association.

Validated outgoing UDP uses existing per-session bounded queues, rejecting the
newest arrival on count/byte/pool exhaustion. Separate connection-owned cursors
rotate request opening, response retry, body/Capsule reads and native enqueue.
Each native scheduling visit attempts one original datagram from each of at most
four sessions. Expiry and generation checks precede enqueue; native-capacity
failure retains the original packet and its deadline. These cursors keep a
finite maximum generation for the current round: continuous new arrivals cannot
extend it indefinitely ahead of old producers. They survive removal, skip
duplicate selections across a wrap and do not depend on HashMap iteration.
Each connection visit selects up to four items in each of ten classes: setup,
H3 events, bodies, commitment checks, active-stream capacity checks, incoming
DATAGRAMs, outgoing queue attempts, generated packets, output submissions and
closing/writable IDs (two reserved closing IDs and two other writable IDs).
Commitment and active-capacity checks have independent generation-horizon
cursors, just like the body class; nine real streams exercise four-item quotas
and eventual progress for both classes. Three additional reserved critical-stream
checks run independently of request streams. Timer/cancellation checks occur
between at most eight owner visits. These are selected-item bounds, not 32 native
API calls or a 256-instruction/time guarantee: metadata enumeration remains
bounded by configured counts, and one H3/quiche call can perform internal work.
When an entire commitment/capacity quota makes progress and candidates remain,
it requests continuation so ready work need not wait for the next periodic wake.
Waiting receipts and ordinary healthy capacity alone do not request continuation;
the nine-stream regression checks both quota progress and eventual idle.
An ACK-pending response or ordinary live write capacity alone does not create
an owner busy-spin continuation. A finite closing-control cycle and observed
STOP work can request continuation. The normal-writability component regression
protects this distinction; a preserved candidate measurement exposing one-core
client spin is not an accepted comparison.

Valid UDP in either direction refreshes the application clock, including a
valid message dropped under pressure; invalid/oversize traffic and transport
keepalives do not. Default inactivity is 120000 ms. Expiry closes stream/socket
and cancels uncommitted queues jointly. Relay/payload resources drop synchronously
in the owner; bounded native control cleanup retains only IDs, deadlines and
admission permits, with no payload or UDP quarantine. An unadmitted request's diagnostic
failure is scoped to that request/connection aggregate, without falsely tagging
all admitted streams as failed. A genuine shared connection fault still affects
its own bounded flow set.

## Consequences and compatibility

Configuration schema, defaults, wire format, credentials and commitment boundary
remain compatible. Resource snapshots add fixed numeric `resources` fields to
the existing NDJSON schema: admission/current peaks, queued lengths, retained
slab capacities, descriptor capacity and native queued bytes. They contain no
source tuples, identity names, credentials or payloads. Native/control descriptor
capacity is distinct from all quiche/TLS heap memory; RSS and kernel allocations
are measured separately. Diagnostic retention keeps its existing 2 MiB ceiling.

Additional bounded application queuing/copying changes the datapath. Compare
against the prospectively frozen MVP reference before accepting this increment;
full optimization and reference-capacity qualification remain Phase 5/release
work. Socket errors, closing streams and mapping denial do not relax trust,
authorization, generation validation or unrelated-flow service.

A shorter inactivity setting remains an operator RFC SHOULD deviation. Test
profiles use 2000 ms for closure/recreation and 500 ms for churn so they exercise
closed-resource reclamation without waiting two minutes per batch. Operators
must document their own rationale; sparse applications can lose the first setup
packet and change source port. These are test settings, not changed defaults.

## Closed native and HTTP stream reclamation

The 1000-cycle process test found peer stream-credit exhaustion after roughly
80 closures per connection while application sessions/relay FDs were already
released. quiche's STOP_SENDING notification can leave a completed stream in
its writable set until stream_capacity/send observes it. H3 poll processes
reads, and a removed application Session never queried that write side.

The adapter visits two FIFO-rotated closing IDs and two other writable IDs per
finite cycle, observes stream_capacity and returns control credit without admitting/forwarding any
closed association. StreamLimit while opening a new request is transient bounded
setup, not permanent mapping denial. Before local write reset, H3's public empty
FIN API marks the local side finished; the reset discards that uncommitted FIN
before any packet generation. For an already stopped write side, public H3 body/
response APIs remove completed metadata through their transport-error paths;
these calls cannot enqueue response/application bytes on a stopped stream.
This avoids reliance on private quiche fields, patches or an unlimited closed-ID
history. Closed IDs are bounded by the configured connection stream limit;
global and identity session permits remain charged until a visited native
stream is collected. `stream_closed()` signals FIN on both sides and does not
prove ACK/collection: only absence reported by `stream_capacity()` permits
removing the closing ID after the final H3 error-path visit. The encrypted
FIN-before-ACK regression fails under the earlier predicate and passes with
the corrected condition. A live stream's STOP notification first goes back to
the owner for scoped Session cancellation; H3 metadata cleanup cannot silently
remove an active association's state. Duplicate request headers also retire
their own Session. A peer omitting required control completion cannot retain
this state indefinitely: a 60-second deadline fails the genuinely stalled
connection and releases its permits. The deadline participates in timer arming.
Ordinary stream closure continues to preserve unrelated flows and previously
committed immutable connection output.

## Critical local HTTP/3 streams

Independent review exposed a previously existing local STOP_SENDING gap: H3
poll processes peer readable controls, while the owner matched writable STOPs
only to request Sessions. Stopping a local control/QPACK stream could therefore
leave the connection forwarding without its critical stream. The adapter now
remembers at most three existing, unfinished local uni streams immediately
after pinned quiche 0.30.0 constructs H3. It uses public QUIC capacity/closed
state and the pinned constructor's synchronous control/QPACK/GREASE ordering;
Remora opens no other local uni streams. GREASE queues FIN and is excluded;
optional QPACK streams that were not opened are not classified as failures.
Changing the backend/constructor ordering requires revisiting this classification.

Every control visit checks these IDs before the ordinary writable rotation.
STOP or native absence causes application connection error
`H3_CLOSED_CRITICAL_STREAM` (0x104), cancels only that connection's Sessions,
purges cancellable native DATAGRAMs and disallows new setup/enqueue/H3 work.
As for other connection failures, unsent old ciphertext is discarded; this
differs from a scoped Session close's mixed-output commitment rule. Close-packet
generation remains enabled under the existing output budget and deadline.
Normal request STOP remains a scoped Session failure. See
[RFC 9114 section 6.2.1](https://www.rfc-editor.org/rfc/rfc9114.html#section-6.2.1)
and [RFC 9204 section 4.2](https://www.rfc-editor.org/rfc/rfc9204.html#section-4.2).
The hostile-peer regression stops each of three streams in either role without
polling the peer's H3; it requires the local error, blocked DATAGRAM enqueue and
the peer receiving the generated connection close. Owner integration separately
checks cancellation scope and close generation. Wire format/configuration do
not change; peers that stop critical streams now receive the required error.

A 128-stream component regression passes with cleanup and fails without it in a
separate scratch copy. The five-body bounded-progress regression similarly fails
when the old first-four HashMap selection is restored. Process churn then runs
1000 cycles and measures both visible capacities and RSS. See validation for
exact binaries, failures retained and the final cleanup result.
