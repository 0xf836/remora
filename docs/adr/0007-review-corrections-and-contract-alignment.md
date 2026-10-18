# ADR-0007: Review corrections and contract alignment

Date: 2026-10-11. Status: accepted for the Phase 3 correction increment.

## Context

The independent code/design/documentation/test review identified nine defects
or evidence gaps. Its scope excluded the archived performance and E2E outcomes.
Correct the existing MVP without adding runtime components, dependencies,
quarantine policy or a pre-implementation feasibility gate. The resolution
and verification are recorded in [review remediation](../validation/review-remediation.md).

## Decisions

Keep both quiche connection receive-window settings at the contracted **256 KiB**,
and stream windows at 16 KiB. The implementation's 1 MiB values had no documented
need or supporting measurement. Restore the existing bound rather than increase
memory/control exposure and change every budget artifact. QUIC DATAGRAM payloads
are not stream/connection flow-controlled; this window bounds reliable H3
control/Capsule input. Future changes require a measured reason and synchronized
contracts, budgets, tests and migration guidance. No configuration/schema change
is needed for this restoration. See [RFC 9221 §5.2](https://www.rfc-editor.org/rfc/rfc9221.html#section-5.2)
and the [resource contract](../../specs/001-masque-udp-tunnel/contracts/cli-config.md).

Drain quiche path events in bounded batches of 32 after input and on owner
timer turns, requesting another turn when necessary. Even disabled active
migration does not justify retaining an unbounded event history. quiche owns
path validation; no address history or diagnostic address export is added.
Source: [quiche 0.30.0 path-event API](https://github.com/cloudflare/quiche/blob/0.30.0/quiche/src/lib.rs).

Bind a client association to both its sender and its original local destination
tuple. Preserve packet-info source selection for immediate and queued replies.
One sender reaching two IPs on a wildcard listener now has two bounded sessions;
neither changes the other's reply address. This applies existing supported
wildcard configuration and does not relax the explicit exposure opt-in. External
UDP provenance limitations after identical tuple reuse remain unchanged.

Validate HTTP/3 fields before policy/activation: request-only `te: trailers`,
consistent single Host with nonempty authority, and legal field-value bytes.
Reject malformed messages with H3_MESSAGE_ERROR without forwarding/logging
offending fields. This intentionally stops accepting malformed peers; valid
independent peers retain the same profile. See [the wire contract](../../specs/001-masque-udp-tunnel/contracts/masque-wire.md),
[RFC 9114 §§4.2–4.3](https://www.rfc-editor.org/rfc/rfc9114.html#section-4.2)
and [RFC 9110 §5.5](https://www.rfc-editor.org/rfc/rfc9110.html#section-5.5).

Permit a shutdown-only status drain to bypass periodic cadence after packet
work and socket cleanup end. Keep one batch in flight, retain newer entries
across an older writer ACK, and share a one-second deadline with writer flush.
Incomplete collection returns a fixed nonzero runtime failure. A bounded wait
cannot guarantee delivery to a blocked sink; success must not hide that failure.
The ordinary packet path stays nonblocking. Schema, queue reservations and
overall five-second shutdown budget are unchanged. Synchronize the
[diagnostics contract](../../specs/001-masque-udp-tunnel/contracts/diagnostics.md)
and research rather than silently change the cadence promise.

Treat VM cleanup as ownership-verified recovery. Scan current and persistent
next-boot XML, including transient guests' current attachments, before mutation.
Persist volume creation intent before its side effect and generated identity
before upload. Ambiguous creation cannot establish ownership by name: retain
the manifest and fail cleanup explicitly. After operator verification/removal,
retry cleanup. libvirt generates volume keys; it does not accept a caller-chosen
ownership key. See [libvirt volume metadata](https://libvirt.org/formatstorage.html#storage-volume-general-metadata)
and [the recovery guide](../validation/local-kvm.md). The manifest lock does not
serialize other libvirt administrators, so concurrent foreign attachment edits
remain outside this local tool's guarantee.

Require exact benchmark workload/pair coverage and a complete finite policy
before comparison. Use synthetic negative fixtures rather than depending on
saved measurement artifacts. Freeze a fresh immediately pre-change reference
for this increment with the existing prospective numeric bounds unchanged;
record actual outcomes without relaxing those bounds.

After the final comparison failed only CPU bounds, the project owner authorized
a separate provisional development policy on 2026-10-11: average process and
owner CPU <=0.5 cores per role for this one-session low-rate loopback profile.
The short original policy did not establish a reliable regression/noise limit;
avoid making further profiling or optimization a prerequisite for continued
MVP implementation. Keep every other bound, all release criteria and the
original prospective policy/failure unchanged. Reusing saved measurements under
the later policy is explicitly a retrospective development exception, never
proof of no regression. See [performance evidence](../performance.md).

Exercise the production Runner's flush and timer paths through a test-only
syscall seam, real encrypted two-stream packets and withheld ACKs. Retain
immutable ciphertext/deadlines and accepted-prefix accounting through close,
partial sends, EAGAIN and bounded EINTR. Remove the imitation queue test.
Hard syscall errors remain visible to the owner; genuine connection failure
still affects that connection's flows. No production callback layer is added.

## Deferred work

T040 explicitly requires rotating body/Capsule service across all pending
streams and at least five continuously pending streams in its progress test.
The current first-four HashMap visit is not accepted as full fairness evidence.
Implement this with Phase 4 scheduling/admission; no Phase 4 task is completed
by this correction increment. Final-revision long-duration and platform/load
qualification remain T076 and the associated later tasks.
