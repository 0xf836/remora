# ADR 0031: Default to 1452 / 1400 with a WG 1280 starting example

Status: selected by the owner, 2026-10-23. Supersedes the default-size choice
in [ADR 0030](0030-explicit-size-profiles.md).

## Decision

Change default `transport.outer_udp_max_bytes` from 1350 to 1452 and
`transport.application_max_bytes` from 1200 to 1400. Change the primary WireGuard
example from MTU 1120 to 1280. Existing explicit values remain authoritative;
WireGuard interfaces are still configured independently by the operator.

Preserve the existing quiche Git pin, its sole previously accepted correction,
wire format, PMTU discovery, no-fragment/no-replay behavior and resource budgets.
The owner explicitly chose the larger profile without extending the temporary
QUIC fork. A verified outer 1452 permits application 1400 for Remora association
IDs; WG 1280 requires 1312 application bytes. None is a path or speed guarantee.

## Compatibility and migration

Absent transport overrides now select the new sizes. Update both endpoints
together. Replace explicit 1350/1200 values where the new profile is intended;
otherwise they continue to select the old profile. A peer's application cap
is not negotiated. A new client can therefore send 1312 which an old server
with application cap 1200 rejects even after a successful handshake.

The pinned backend's probes can also exceed a smaller peer's advertised outer
cap before converging. That pre-existing defect remains unresolved; coordinated
endpoint settings avoid this particular mismatch. The default transition does
not establish general compatibility with every smaller independent peer.

Startup and revalidation may still temporarily reject larger application
messages while verified PMTU is 1200. A permanently smaller path cannot carry
the chosen fixed WG MTU. Remora neither buffers/replays such data nor adjusts
WireGuard. Retain 1350/1200/WG1120 as an explicit inner-IPv4 rollback profile.

## Verification and evidence

Test the new 1400/1401 default boundary without transport overrides, opaque
1312/1392/1400 round trips over outer IPv4/IPv6, the explicit old profile and
existing revalidation/diagnostic regressions. Check default values and resource
budget validation. Preserve historical network and VM fixture profiles with
explicit 1350/1200 overrides, so their 1200/1201 expectations and published
metadata remain truthful; they are not qualification of the new defaults.

Historical benchmark/WireGuard results retain their original binaries and
settings. New default checks must be recorded separately; outer IPv6 UDP is not
inner-IPv6 WireGuard qualification, and increasing a ceiling is not a measured
performance improvement.

Final local results and limitations: [default-size validation](../validation/default-size-profile.md).
