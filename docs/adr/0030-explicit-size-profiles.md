# ADR 0030: Explicit size profiles without extending the QUIC fork

Status: accepted after independent design review, 2026-10-22. The default-size
decision below is superseded by [ADR0031](0031-default-size-profile.md),
2026-10-23; the diagnostic semantics and unchanged dependency remain applicable.

## Decision

Keep the production defaults `outer_udp_max_bytes = 1350` and
`application_max_bytes = 1200`. Keep the documented WG1120 inner-IPv4 starting
example and its historical validation boundaries. Offer an explicit
1452/1400/WG1280 operator-selected profile; WG1360 is a tuning candidate for a
verified path. Neither is a universal path guarantee or a measured optimum.

Both Remora endpoints must select the same outer and application caps. Operators
own both WireGuard MTUs and validate full-size traffic in both directions.
Application caps are not negotiated by CONNECT-UDP. Preserve the current
quiche Git revision and the existing minimum-payload correction: no new fork
patch, public API, dependency, private fragmentation or reliable fallback.

## Context and alternatives

WG1280 needs 1312 bytes of opaque application payload. The pinned quiche with
Remora CID16 reserves 42–49 further bytes for QUIC/DATAGRAM/MASQUE. Thus the old
outer1350 ceiling cannot support it, even with a raised application cap.
At verified outer1452, 1400 application bytes fit all Remora association-ID
widths. WG1360 produces 1392 bytes, leaving less network-path headroom.

The first review reproduced a backend limitation: a 1452 sender against a peer
advertising 1350 generated PMTU probes above 1350 before converging. This is also
possible with old 1350 against 1200; increasing the default would make it common
during partial upgrades. Merely clipping the output buffer can stall discovery.
The public live-connection API does not expose the necessary PMTU search-bound
update. Fixing this in the temporary fork would expand dependency maintenance
beyond the owner's intended upstream-return boundary, and is excluded here.

A global 1452/1400 default transition was therefore rejected for this increment.
An intermediate application cap 1300 under outer 1350 would not solve WG1280 and
has no measured basis. A private fragmentation protocol would add wire,
reassembly and resource-policy complexity. Retaining defaults and exposing
tested, explicit profiles gives operators a useful choice with smaller impact.
The known peer-probe issue remains unresolved and must not be described as safe
or fixed. Matching peer caps avoids that specific conflict only.

## Diagnostics, validation and consequences

Reuse bounded, rate-limited numeric diagnostics. Size failures can retain
direction, rejected size, configured cap and the actual per-stream outgoing
ceiling with opaque connection/session IDs. Repeated observations replace the
size details in the existing failure-ring entry. Periodic QUIC status reports
a conservative local-send application lower bound, sampled across connections
and allowing the longest association ID. This does not include remote
application policy or target-leg route capacity and is not WG readiness.

Startup and PMTU revalidation can temporarily reduce capacity below 1312;
unavailable-period traffic remains dropped without replay. A path that stays
too small cannot support the chosen fixed WG MTU. Remora remains generic and
does not inspect WireGuard or control its interface.

Tests preserve the default 1200/1201 boundary and add matched 1452/1400 endpoints,
1312/1392/1400 delivery and 1401 rejection on outer IPv4/IPv6 loopback, native
revalidation/purge/recovery and bounded diagnostic metadata. These tests do not
establish inner-IPv6 WireGuard, WAN reliability, capacity or a speedup. Historical
WG1120 duration and performance results remain bound to their original binary.
