# ADR-0006: Public quiche acceptance receipts and MVP ownership

Status: Accepted for the Phase 3 increment, 2026-10-11. This is an implementer
source/API assessment; an independent audit is invoked only at the user's request.

## Decision

Keep the selected current-thread Tokio owner and safe-facing Linux UDP/timerfd
adapter. Setup resolution and diagnostic writing are bounded off-owner work;
QUIC, HTTP/3, live associations and packet generation have exactly one owner.
No dependency or second production executable is added. The separate traffic
package supplies test generation and the plaintext two-role comparison.

quiche 0.30.0 exposes `BufFactory`, `accept_with_retry`, HTTP/3 polling,
`dgram_max_writable_len`, `dgram_purge_outgoing` and `SendInfo`, but no public
per-stream notification that a successful response entered a generated packet.
A custom safe `BufFactory` attaches a reference-counted receipt to STREAM buffers
created synchronously by `send_response`. Its scope resets on every exit.
The server waits until all such references are released, while the request
stream is still open and writable, before enabling relay forwarding.

The pinned source's `stream/send_buf.rs::ack_and_drop` retains buffers through
retransmission and releases only contiguously acknowledged data. An ACK is
therefore a conservative proof of the earlier transport commitment. A reset,
STOP_SENDING, failed send or connection teardown invalidates acceptance;
buffer release alone never authorizes a closed stream. A received RESET aborts
partial Capsule input without pretending it ended with FIN; only actual FIN
checks message completeness. QPACK has zero dynamic
table capacity and zero blocked streams. Dedicated tests exercise ACK retention,
stream cancellation and STOP_SENDING using real encrypted quiche packets.

This intentionally delays server activation beyond minimum packet-generation
commitment. Datagrams arriving before activation are dropped and counted; there
is no application retry or reliable-carrier fallback. Setup/acceptance latency
and first-packet loss remain visible. Reassess this adapter on a quiche upgrade;
a future public commitment receipt can replace it with the same guarantees.

## Queue and scheduler rules

Application residence is 20 ms by default. Native outgoing DATAGRAM queues have
count/byte reservations and a conservative earliest expiry; expiration purges
the entire cancellable queue. The native incoming queue also inherits its oldest
receive epoch until emptied. A socket-backpressure queue retains that original
expiry, rather than starting a fresh residence interval. Wire validation still
precedes expired-payload discard.

Reserve a ciphertext slab before `quiche.send`. Successful generation commits
immutable bytes and original generation/from/to/at/deadline metadata to a FIFO.
Individual stream close purges its native datagrams but cannot rewrite or recall
mixed ciphertext. Exact accepted prefixes are removed once; EAGAIN retains the
suffix. The default 100 ms output deadline fails the connection when exhausted.
Stop discards unsent output of the original generation. No replay occurs on a
new connection. Final HTTP rejection headers/FIN retain their connection-owned
STREAM buffers while the rejected session and cancellable datagrams retire;
ordinary abort cleanup must not cancel that queued refusal.

Rotate finite socket/connection/session visits (at most 32 operations each,
256 packet/control operations before timer and cancellation checks). Setup
futures are polled in slices of at most 32 after cancellation checks, with
explicit continuation to register every waker. Native H3/datagram work and
successful relay reads request continuation even when a socket batch was short.
Readiness clears only after an actual EAGAIN. AsyncFd and a monotonic absolute
timerfd retain user-space pacing; no qdisc or privileged runtime is required.

## Trust, admission and diagnostics

The client verifies the configured CA and server name and required H3 settings
before sending Bearer credentials. Early data, session resumption, raw upstream
tracing and keylog are disabled. Exact host/port and all resolved addresses must
be authorized; one approved address is pinned to one session-specific connected
socket. Blocking resolver permits survive async cancellation until the actual
lookup finishes. Generation checks reject late completions.

A process-random HMAC Retry token binds source address/port, original DCID,
Retry CID and a monotonic ten-second lifetime before connection allocation.
A fixed 64-bucket source limiter bounds admission state; collisions can reject
legitimate clients. quiche owns anti-amplification and validated path state;
active migration is disabled. Existing native CIDs are routed without retaining
retired IDs beyond the temporary Retry alias.

Failures enter a bounded 256-entry ring before log attempts/cleanup. One status
batch is prepared fully before publication and acknowledged only after the last
chunk is written and flushed. Old ACKs cannot retire newer updates. Diagnostic
output contains fixed classes and opaque IDs, never arbitrary headers, tokens,
payloads, target names or user source tuples. GSO/GRO auto report effective off
and `not_implemented`; configured off reports `configured_off`, with no probe.

## Limits

Connected sockets and generations isolate live/internal associations. External
UDP cannot identify an indistinguishable delayed response after the same tuple
is reused or the process restarts. No fixed quarantine duration can guarantee
provenance; no such duration is promised. The ACK receipt also cannot recall
already generated/kernel-submitted packets.

The first benchmark is a low-rate, one-session, same-host experiment. It cannot
select an optimization or qualify sustainable capacity, SC thresholds, ARM64,
independent-stack compatibility, long-duration WireGuard, PMTU error queues,
recovery or optional qlog. Those remain explicit later tasks.

Sources: [quiche 0.30.0 public buffer API](https://github.com/cloudflare/quiche/blob/0.30.0/quiche/src/lib.rs),
[pinned send buffer](https://github.com/cloudflare/quiche/blob/0.30.0/quiche/src/stream/send_buf.rs),
[HTTP/3 API](https://github.com/cloudflare/quiche/blob/0.30.0/quiche/src/h3/mod.rs),
[RFC 9298](https://www.rfc-editor.org/rfc/rfc9298.html),
[RFC 9297](https://www.rfc-editor.org/rfc/rfc9297.html).
