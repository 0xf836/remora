# MASQUE wire contract: initial unreliable profile

Version: 1, design date 2026-10-10. Applicable requirements FR-002/004/005/008/010/015/016/026. No Remora-private wire extension or application header is introduced.

## Standards and carrier

Implement CONNECT-UDP [RFC 9298](https://www.rfc-editor.org/rfc/rfc9298.html), HTTP Datagram/Capsule [RFC 9297](https://www.rfc-editor.org/rfc/rfc9297.html), unreliable QUIC DATAGRAM [RFC 9221](https://www.rfc-editor.org/rfc/rfc9221.html), HTTP/3 [RFC 9114](https://www.rfc-editor.org/rfc/rfc9114.html), QUIC transport/TLS/loss recovery [RFC 9000](https://www.rfc-editor.org/rfc/rfc9000.html), [RFC 9001](https://www.rfc-editor.org/rfc/rfc9001.html), [RFC 9002](https://www.rfc-editor.org/rfc/rfc9002.html) and HTTP/3 Extended CONNECT [RFC 9220](https://www.rfc-editor.org/rfc/rfc9220.html). URI expansion follows [RFC 6570](https://www.rfc-editor.org/rfc/rfc6570.html); authorization uses the [Bearer scheme](https://www.rfc-editor.org/rfc/rfc6750.html), without an OAuth issuance service.

TLS 1.3 and ALPN h3 are required. QUIC DATAGRAM and both peers' SETTINGS_H3_DATAGRAM=1 are required; server advertises SETTINGS_ENABLE_CONNECT_PROTOCOL=1. quiche transport enable_dgram and H3 server enable_extended_connect configure this profile; the stack derives its own H3 datagram setting. Disabled/missing capabilities fail setup distinctly, never fall back to reliable payloads. No 0-RTT application requests/data.

Closing a local HTTP/3 control or opened QPACK stream, including peer
STOP_SENDING, is a connection error H3_CLOSED_CRITICAL_STREAM (0x104),
not a request Session cancellation or an HTTP refusal status. Cancel only
the affected connection's associations, stop new enqueue and retain bounded
connection-close generation. Optional unopened QPACK streams and completed
GREASE streams are not critical-stream failures. Conformance tests must include
a peer that continues QUIC after STOP without independently polling/closing H3.
Sources: [RFC 9114 section 6.2.1](https://www.rfc-editor.org/rfc/rfc9114.html#section-6.2.1),
[RFC 9204 section 4.2](https://www.rfc-editor.org/rfc/rfc9204.html#section-4.2).

## Request and acceptance

Default configured URI template: `https://{proxy-authority}/.well-known/masque/udp/{target_host}/{target_port}/` (proxy authority is configuration, not a target substitution). Client may use an explicitly configured HTTPS URI template supporting target_host/target_port; validate RFC 9298 constraints: template level <=3, ASCII characters 0x21..0x7e, absolute nonempty scheme/authority/path with slash-prefixed path, variables confined to path/query, and no reserved (+), fragment (#), dot-label (.), slash-segment (/) or semicolon (;) expansion operators. Reject invalid templates at check-config before any request; server v1 supports its exact default path prefix and segment grammar. Variable values use percent encoding once; decode once, validate canonical host/decimal port and authorize. IPv6 host substitution is the literal address with colons percent encoded, without authority brackets; IPv6 proxy authority uses normal brackets. Reject query/fragment/unexpected extra path segments on the default route, userinfo, NUL/control bytes, ambiguous numeric addresses and double-decoding tricks.

| Header | Request contract |
|--------|------------------|
| :method | CONNECT |
| :protocol | connect-udp |
| :scheme | https |
| :authority | Configured proxy authority; not target host |
| :path | Expanded configured template; default path above |
| authorization | Exactly one Bearer token; bounded length and never logged |
| capsule-protocol | ?1 |

Keep the stream open. Server validates pseudo-header combinations/path, credentials, permission and the approved resolved address before allocating its destination socket and queuing 200 with capsule-protocol=?1, fin=false. Client forwards only after a valid final successful response on that stream plus required transport/settings. Server forwards only after its acceptance is committed. No destination traffic occurs on rejected/setup/early-data paths. Datagram arrival before known acceptance drops/counts; no setup buffering.

A valid successful Capsule response is a well-formed final 2xx response excluding 204, 205 and 206. Requests and successful responses using Capsule Protocol must not contain Content-Length (including zero), Content-Type or Transfer-Encoding. Validate these fields before granting forward eligibility; reject malformed messages with HTTP/3 stream error H3_MESSAGE_ERROR (0x010e), not as an ordinary successful status. Informational responses do not activate a session; a non-successful final response aborts the request, and a successful response whose stream is already closed does not create an active flow. Remora sends capsule-protocol=?1 in requests and successful responses; CONNECT-UDP itself defines Capsule use, so an absent field alone on an otherwise valid independent-peer response is not a new capability failure. Parse the field as a Structured Field Boolean and ignore unknown parameters; duplicate/non-Boolean values are treated as absent under RFC 9297. Never send this field on non-2xx rejection responses. Sources: [RFC 9297 §§3.2–3.4](https://www.rfc-editor.org/rfc/rfc9297.html#section-3.2), [RFC 9114 §4.1.2](https://www.rfc-editor.org/rfc/rfc9114.html#section-4.1.2).

Missing/invalid credentials: 401 plus `www-authenticate: Bearer`; authenticated denied target: 403; malformed HTTP request/path: 400; unmatched route: 404; unsupported method: 405. Capacity admission uses 503; DNS/connect-socket setup failure 502, setup timeout 504. Numeric setup status and privacy-safe reason class are diagnostic, never raw headers/DNS input. Standards-mandated H3/QPACK/QUIC errors remain the stack's protocol errors, not arbitrarily converted to HTTP status. A 200 proves authorization/setup, not destination reachability.

HTTP/3 field validation precedes request policy and response activation. `te`
is allowed only in requests with the case-insensitive value `trailers`; it is
forbidden in responses. If a request includes `host`, it must occur once and
match its nonempty `:authority`. Field values may contain visible bytes,
obs-text and interior SP/HTAB, but no other control bytes, DEL or leading/trailing
SP/HTAB. Malformed fields cause H3_MESSAGE_ERROR, with no relay eligibility and
no diagnostic export of the offending value. Sources: [RFC 9114 §4.2](https://www.rfc-editor.org/rfc/rfc9114.html#section-4.2),
[§4.3.1](https://www.rfc-editor.org/rfc/rfc9114.html#section-4.3.1),
[§10.3](https://www.rfc-editor.org/rfc/rfc9114.html#section-10.3)
and [RFC 9110 §5.5](https://www.rfc-editor.org/rfc/rfc9110.html#section-5.5).

## HTTP Datagram and context format

QUIC DATAGRAM payload is `varint(request_stream_id / 4) || varint(context_id = 0) || opaque_udp_payload`. Request stream identity is checked against its connection generation. Context zero represents UDP, including empty payloads; nonzero unknown contexts drop without creating state or registering extensions. Closed/unknown/not-yet-created flow datagrams are discarded. Unparseable or out-of-range quarter-stream association is connection H3_DATAGRAM_ERROR (0x33). Missing/truncated Context ID datagrams are discarded and counted as malformed under Remora receive policy; no application bytes are forwarded and no new context is inferred. This is not an invented RFC-mandated stream-abort rule.

No deliberate duplication/retransmission/coalescing or ordering promise. Packet delivery can be lost/reordered by the network. Current writable DATAGRAM capacity from quiche already excludes QUIC overhead; subtract only the encoded association and context lengths. Target-leg limits may be smaller. Supported delivered payloads are byte-identical. Unsupported path sizes drop/count; no private fragments, truncation or reliable fallback. Context-zero UDP payload above RFC's 65527-byte bound aborts its stream, rather than treating it as ordinary oversize loss. Application configured size bound is stricter (initial 1200 bytes).

## Capsule and stream lifecycle

Capsule stream parser is incremental type/length/value with checked QUIC varints, bounded retained header/body state and streaming skip. Unknown Capsule types are silently skipped without reserving their declared length. DATAGRAM Capsule type 0 has implicit request association and a context-prefixed payload; it is a valid standard carrier but is not forwarded in this initial unreliable-only application profile. Unsupported-carrier handling does not bypass required parsing or semantic errors:

1. Parse type/declared length, then the complete Context ID within that length, retaining at most its eight-byte varint prefix. Missing/truncated Context ID or a truncated capsule at FIN is a malformed/incomplete Capsule message: abort the request stream with H3_MESSAGE_ERROR (0x010e). An incomplete prefix while the stream remains open awaits bounded incremental input; it is not prematurely called malformed.
2. For Context 0 compute UDP payload length with checked subtraction of the encoded Context ID length. A payload above 65527 bytes aborts the request stream as required by RFC 9298 §5; Remora selects H3_DATAGRAM_ERROR (0x33) for this datagram semantic error, since that section does not prescribe a code. This check precedes the stricter configured application limit and unsupported-carrier discard, without reading the whole body. Unknown nonzero contexts are discarded without private extensions; their UDP-specific length semantics are not inferred.
3. Stream-skip the remainder of a syntactically valid, in-range Context 0 capsule and count unsupported_carrier once it is completely consumed; never forward its payload. Stay bounded, resume at the next capsule, and detect truncation at FIN even on the skip path. Unknown-context capsules count unknown_context instead.

Do not label valid capsules malformed or claim a private capability that forbids them. Tests distinguish mandatory syntax/semantic outcomes from supported unreliable application delivery. Sources: [RFC 9297 §§3.3/3.5](https://www.rfc-editor.org/rfc/rfc9297.html#section-3.3), [RFC 9298 §5](https://www.rfc-editor.org/rfc/rfc9298.html#section-5).

Incomplete final capsule at FIN, malformed varint or stream/protocol violation gets the standards-required stream/connection outcome, documented in conformance cases. FIN/reset shuts the corresponding flow, purges cancellable application/native DATAGRAM queues, and releases stream/socket resources. Already generated ciphertext is transport-committed to the original connection/stream, may complete only within its original bounded deadline, and is not selectively purged or reassigned; see [the commitment boundary](../plan.md#transmission-commitment-and-session-closure-r1). Connection failure/replacement or process stop discards unsent old-connection output. No new DATAGRAM is committed for a stream whose send side has closed; datagrams received after its receive side closes are dropped. Local close cannot recall committed or kernel/network in-flight traffic. GOAWAY begins connection draining: stop new requests on that connection, classify requests excluded by its indicated stream-ID cutoff, and create new requests on a replacement connection. Existing accepted lower-ID flows continue until their own stream/connection closes or bounded operator shutdown. Do not replay unavailable-period traffic while recreating a disallowed request. The accepted [A1](../proposed-amendments.md) requires client/session routing isolation and rejection of identifiable stale traffic without prescribing socket quarantine. QUIC/stream identifiers cannot establish provenance of opaque old target replies after an identical relay tuple is reused; the architecture and operator guide must document that limitation and the selected reuse trade-offs.

## Conformance and independent compatibility cases

Test peer settings ordering, absent DATAGRAM/Extended CONNECT, early datagrams, 1xx/final response handling, 204/205/206 rejection, forbidden Capsule message fields (including Content-Length: 0), Structured Field Boolean/absent-field compatibility, duplicate/forbidden pseudo-headers, percent encoding/IPv6 literals, context zero/unknown/truncated values, large/invalid associations, capsules split at every byte boundary, unknown capsules, DATAGRAM Capsule truncated Context ID/FIN, Context 0 lengths 65527/65528 with different varint widths, reliable-carrier discard followed by a correct capsule, empty and oversize payloads, FIN/reset, two-stream EAGAIN/close before and after transport commitment, accepted-flow continuation after GOAWAY with a new flow on a replacement connection, and late closed-stream datagrams. Capture actual allowed/forbidden destination traffic and payload equality; do not use connection establishment alone as proof.

The first transport/MVP tests must verify the initial GOAWAY cutoff, refusal of new/excluded requests and continued accepted lower-ID flows without reassignment or replay. Verified replacement-connection creation and new request placement are additional recovery tests in the later increment; they do not postpone the initial behavior's checks.

Pin quic-go/masque-go fixture from research for both client->independent proxy and independent client->Remora server. Bearer headers and local test CA provide compatible credentials/trust. The Go fixture performs policy/auth checks but implements no Remora-specific wire codec. The declared unreliable profile and limitations must accompany independent test results.

## Phase 5 dynamic payload limits

The configured application limit is an upper bound, not a promise on every
path. Before enqueue, query the native DATAGRAM capacity, apply the current
verified QUIC path limit, then subtract only the association and Context ID
prefixes from that native payload allowance. Quiche already accounts for QUIC
packet/frame overhead. Its public writable-length API can retain a larger
recovery send allowance while PMTU revalidation has reset the verified path
size; the adapter clamps that difference before admitting application data.
It does not subtract the IP/UDP/QUIC overhead twice. Recheck the same limit
before native packet generation and purge oversized queued native entries with
matching reservations, retaining eligible entries in order.

Connected destination sockets query the current kernel route MTU before actual
submission, including deferred sends; do not retain a sticky maximum that
prevents a later increase. EMSGSIZE drops the failed application packet and
preserves the association. The outer QUIC socket permits DF probes above its
cached path MTU (PROBE), while target sockets enforce DO. IPv6 additionally
uses DONTFRAG. Neither role introduces IP/private fragmentation or reliable
payload fallback. Reject an intact received outer UDP message above the
configured outer cap as well as truncated messages.

Delayed peer/ICMP errors on receive or send do not close a Session or an
unrelated connection sharing its outer socket. A failed submission discards
only its unaccepted packet; accepted prefixes are never replayed. An errno
alone does not identify the connection/path of an earlier packet. Apply outer
PMTU revalidation only to a checked error-queue destination matching an active
path; hard socket failures retain their explicit terminal handling.

Drain at most four error-queue entries per selected socket visit. Validate
ancillary lengths/alignment/family/origin and the original destination; accept
only valid local/ICMP packet-too-big metadata for PMTU handling. Ignore quoted
payloads and never retain/log them. Outer errors revalidate only matching
connection paths and have no fabricated Session attribution. Non-PMTU target
errors are numeric observations; they are not unconditional application-session
closure based on unauthenticated ICMP. See ADR-0009 for API and privacy details.

Reduced-MTU native testing reproduced legal request-before-SETTINGS ordering:
loss of the client's control-stream packet let its later request stream arrive
first. HTTP/3 streams have no cross-stream delivery order. The server now
retains at most 32 fields/16 KiB in a pending Session charged to the existing
global/per-connection Session and pending-setup budgets, with the original
five-second setup deadline. It starts no authentication/DNS/relay/response/data
forwarding until peer SETTINGS and required DATAGRAM capability are verified.
Repack native header capacities into exact buffers, include their fixed memory
in control metadata accounting, and service four deferred requests with a
separate rotating cursor per connection. Expiry, reset, duplicate headers,
close and unsupported settings release the reserved Session and its headers.
Missing settings is not a success or permission to bypass capability checks.
The MTU 1280 namespace reproduction passes after this change; its original
failures remain retained.
