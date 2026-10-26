# ADR 0034: Sample UDP drops without ancillary overflow messages

Status: Implemented; local tests and prior VM verification recorded separately.
Deployment acceptance remains with the owner.

## Context

The investigated Linux failure occurred while copying a four-byte SO_RXQ_OVFL
control message from an skb on the WireGuard/loopback path. A kernel usercopy
failure can kill the reactor thread while the diagnostics writer survives. This
is outside Rust's recoverable syscall errors. The socket option was enabled
regardless of diagnostic output. This change avoids that kernel path; it does
not claim to explain every behavioral difference between release builds.

## Decision

Never enable SO_RXQ_OVFL, including in diagnostic mode. Read SK_MEMINFO_DROPS
using SO_MEMINFO instead. Linux exposes the same cumulative sk_drops value via
a socket query with kernel-local response storage, avoiding skb ancillary copying.
Keep the syscall and its unsafe boundary in the UDP module; add no dependency,
worker, subprocess or new configuration option.

Only JSON output needs this telemetry. JSON with snapshots disabled still needs
it for failure/health reports. Text output performs no queries. Periodic sweeps
are limited to 16 socket queries per reactor turn, at roughly one-second cadence,
with a final read before normal socket retirement. Each socket owns its baseline.
Failures retain that baseline and mark aggregate coverage incomplete; they never
interrupt forwarding. The first incomplete-coverage transition is observable even
when periodic snapshots are disabled, without recurring health-report spam.

The periodic sweep uses a BTreeSet of live logical socket keys, bounded by the
existing socket/session/connection limits. Insert/remove happens only during
socket lifecycle changes. The cursor resumes an ordered range rather than
rescanning a HashMap prefix. Server listener indices are immutable; client
outer keys use connection generations and relay keys use session generations,
so fd reuse and client index renumbering cannot redirect a queued query. Freeze
the final key for each sweep; newly appended keys wait for the next sweep. This
adds bounded registry storage and O(log N) lifecycle operations without changing
socket ownership. Closing a socket removes its key immediately. Final reads on
retirement/shutdown are outside the 16-query periodic quota and remain bounded
by configured resource limits.

Benchmark/reference summaries preserve kernel telemetry completeness separately
from diagnostic-record loss. Unknown/legacy telemetry cannot establish zero
kernel drops. Direct baseline measurements mark this metric not applicable.

## Consequences

This replaces packet-time observations with query-time observations and adds a
small bounded periodic syscall cost only for JSON. Socket churn can postpone a
sample; final reads and cumulative deltas preserve normal retirement accounting.
A failed final read, abrupt termination, or more than one counter wrap between
successful reads prevents exact accounting. The sticky incomplete flag records
known failures, not every possible blind spot. No end-to-end loss or session
attribution is inferred. See the diagnostics contract and
[verification handoff](../validation/socket-drop-telemetry.md).
