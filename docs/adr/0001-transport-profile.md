# ADR-0001: Public quiche API and an explicit unreliable MASQUE profile

Date: 2026-10-10. Revised: 2026-10-11. Status: selected design; revised A1/A2 accepted, ready for incremental implementation.

## Context

Remora requires Rust/quiche, independent standards compatibility, opaque UDP semantics and no reliable fallback. quiche provides QUIC/H3 but not a complete authenticated UDP relay. The exact profile and lifecycle must be reviewable.

## Options considered

1. Public quiche 0.30.0 with a narrow Remora MASQUE handler.
2. Private transport framing or a reliable stream payload carrier.
3. A broader generic proxy framework or alternative QUIC stack.

## Decision

Refinement: [ADR-0007](0007-review-corrections-and-contract-alignment.md) makes
the local sender/local-destination pair the client association key, preserving
wildcard reply source without changing this transport profile.

Choose option 1. Pin quiche =0.30.0, Rust 1.99.0 and the resolved dependency graph; use its Rust API, default BoringSSL and h3 ALPN. One request stream per local sender/local-destination pair, shared connection per listener mapping, dedicated connected target socket per accepted flow. Enable QUIC DATAGRAM and Extended CONNECT; await processed settings and verified handshake before credentials/payloads. Implement only RFC CONNECT-UDP URI/Context 0/HTTP Datagram association and bounded Capsule parsing. Payloads remain unreliable and opaque, including empty datagrams. Required unknown-capsule handling and wire errors are distinct from unsupported reliable payload-carrier discards.

Transport per-connection failures interrupt that connection's sessions, but session/policy/overload failures close only the affected stream. Generation IDs and closed-flow datagram discard prevent internal replay. Accepted A1 defines observable isolation and explicit UDP provenance limitations, not a fixed socket-retention mechanism or private wire extension.

## Isolation design and reuse trade-offs

Initial design: each accepted server flow owns a distinct connected destination socket and (connection generation, request stream ID) association. Each client flow owns a listener/source mapping plus its generation. Responses cannot be routed by destination address alone, share another active flow's association, or reuse another client's queued payload. Closing a flow invalidates mappings, purges pending application/native DATAGRAM data and rejects delayed completion work before resources can serve a new owner. Wrong-source responses and identifiable old tunnel datagrams are discarded. Resource ownership/accounting remains bounded.

The baseline does not require retained closed sockets, a fixed quarantine interval or persistent port reservations. Ordinary endpoint-tuple reuse cannot prove the provenance of an arbitrarily delayed opaque destination reply, including after restart. Operator documentation and tests must explicitly distinguish this unavoidable information limit from required cross-client routing/stale-state isolation; no zero-leak provenance result is claimed for indistinguishable external packets. A local application restart with the same still-active source tuple also has no guaranteed observable boundary.

Additional reuse mitigation can be selected or revised during architectural refinement if the threat model/evidence warrants it. Compare its isolation benefit, FD/port/memory cost, bounded cleanup and availability trade-offs; do not add a quarantine subsystem only to satisfy a planning ceremony. Any selected mechanism must preserve authentication, active-session isolation and rejection of known stale data, and its residual limits must remain visible.

## Transmission commitment and close (independent review R1)

Choose successful quiche packet generation as the outer transmission-commitment boundary, not native DATAGRAM enqueue or UDP syscall completion. A generated packet has already advanced transport state and can mix several streams with ACK/control frames. Public dgram_purge_outgoing removes still-queued native DATAGRAMs, not generated ciphertext. Selective decrypt/rewrite would require transport internals; closing the shared connection merely to cancel one session would violate FR-013. Dropping all queued outer packets as whole-packet loss is another possible policy, but it also discards unrelated DATAGRAMs/control and cannot recall packets already accepted by the kernel. The initial design instead preserves committed packets with the existing short output bound; it does not claim that QUIC cannot tolerate whole-packet loss. Neither selective rewriting nor forced shared-connection close is part of this design.

| Stage | Close/expiry behavior |
|-------|-----------------------|
| Application/relay queue or native outgoing DATAGRAM queue | Stop eligibility and purge that flow before further generation/submission |
| Successful quiche.send()/send_on_path() output, not yet accepted by outer UDP syscall | Immutable committed output stays on its original connection/stream; preserve generated order, original SendInfo and <=100 ms generation-based residence even if one flow closes |
| UDP syscall accepted message / kernel-network in-flight | Cannot be recalled; no target-delivery guarantee |
| Connection replacement/failure or process stop | Drop unsent old-connection output with its transport state; never copy/re-enqueue it into a replacement |

Local session closure therefore does not promise an instantaneous cutoff of remote delivery for previously committed packets. A still-open peer may receive them only on their original authorized association; a closed receive side discards them. The boundary does not permit new commitment after close, forwarding newly received closed-session traffic, or reassignment to another client. If the output deadline expires, fail the genuinely blocked connection; normal session close does not manufacture that failure. Connection-owned ciphertext has no invented per-session cancellation tag. The two-stream close/EAGAIN/partial-send/replacement test is required in the first forwarding increment. Details: [plan boundary](../../specs/001-masque-udp-tunnel/plan.md#transmission-commitment-and-session-closure-r1), [quiche outgoing purge API](https://docs.rs/quiche/0.30.0/quiche/struct.Connection.html#method.dgram_purge_outgoing).

## Protocol and idle corrections (R2–R4)

The [wire contract](../../specs/001-masque-udp-tunnel/contracts/masque-wire.md) explicitly rejects forbidden Capsule response statuses/fields before accepting a flow. Mandatory DATAGRAM Capsule Context ID/length checks precede valid-carrier discard, with bounded prefix parsing and distinct syntax/semantic outcomes. Preserve these cases in independent-peer tests. Default session inactivity is 120000 ms of valid UDP activity in either direction, with stream/socket closure together, following RFC 9298 §3.1; transport keepalives do not keep an idle application association alive. A shorter configured timeout is an explicit operator deviation with documented sparse-traffic consequences, not the default.

## Consequences

Remora owns a small audited framing/lifecycle layer and conformance tests. No claim that standard valid DATAGRAM capsules are malformed. No early data, private fragmentation, alternative protocols or reliable payload fallback. Configuration and compatibility changes require explicit version/migration documentation. Independent quic-go/masque-go verification covers both roles.

Evidence: [research](../../specs/001-masque-udp-tunnel/research.md), [wire contract](../../specs/001-masque-udp-tunnel/contracts/masque-wire.md), [RFC 9298](https://www.rfc-editor.org/rfc/rfc9298.html), [RFC 9297](https://www.rfc-editor.org/rfc/rfc9297.html), [quiche H3 API](https://docs.rs/quiche/0.30.0/quiche/h3/struct.Connection.html).

## Phase 4 controlled UDP reuse characterization

The native test closes an old UDP endpoint, binds its exact local tuple again,
connects to the same authorized target, and sends a delayed reply from that
target. Linux delivers the reply to the new socket: its opaque payload has no
process/session-generation field. A same-tuple application restart while its
Remora mapping remains active has the same information limit. Wrong-source
packets to the connected socket are filtered. This is a characterization of
external provenance, not a cross-client isolation pass or a quarantine promise.

Owner tests separately close a stream with queued native data, create another
association and inject the old stream's DATAGRAM and setup callback. No relay
or queue is reassigned; known stale data is discarded and permits stay exact.
The 32-flow runner validates distinct live associations, expiry/recreation and
churn; a tuple observed from a previously expired association is not classified
as simultaneously active merely because the test retained its historical value.
