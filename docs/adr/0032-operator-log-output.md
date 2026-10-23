# ADR 0032: Operator logs and an explicit legacy diagnostic profile

Status: owner-approved plan, implemented 2026-10-23. Replaces the initial
text-key/value proposal discussed in this session.

## Context and options

A mandatory full JSON snapshot every second overwhelms a systemd operator.
Changing braces into key/value fields does not explain the failure. Per-request
WARNs on a public server also create noise. Conversely, measurement tools and
SC-010 capture need the existing full schema and correlated status independent
of ordinary event filtering.

Considered: a general incident/aggregation engine; category cooldowns; simultaneous
journal/file sinks with internal rotation; a new logging framework; two explicit
profiles reusing existing bounded output. Choose the last, minimal option.

## Decision

- Defaults: info/text/snapshots=false. Human operational messages go to stderr;
  client/server stdout is empty. No failure ring is collected in operator mode:
  the journal is not a per-request audit and cannot establish SC-010 completeness.
- Explicit json/snapshots=true selects the existing schema-1 stderr events and
  stdout status, including full counters, ring/coverage, reservations, ACK/retry
  and final collection. JSON plus snapshots=false still carries mandatory pending
  failures. Text plus snapshots=true is rejected with an actionable field error.
- Profiles are alternatives. No own file backend, rotation, per-peer suppression
  map, incident engine, reminder timer or deferred replay of suppressed logs.
  An explicitly managed external collector owns JSON retention. Journald captures
  both streams, so explicitly selecting JSON under the unit is intentionally noisy.
- Reuse Event/Failure and the existing bounded queue/writer. Human formatting uses
  typed static messages plus numeric metadata, <=1024 bytes per record. Preserve
  syscall errno at listener bind and render safe operation/reason/action hints.
  Never render arbitrary peer text, targets, headers or payloads at any level.
- Normal INFO: startup/listener ready, first transport ready, first tunnel accepted,
  accepted tunnel recovery and shutdown. A handshake does not prove CONNECT
  acceptance; acceptance does not prove a UDP destination response. Retry
  transitions and packet/resource/request failures use DEBUG. Terminal initial
  refusal explains process exit; later permanent refusal explicitly says new
  setup is blocked while accepted flows may survive.
- A mapping-wide outage-announced flag prevents repeated WARNs across retries;
  acceptance clears it. Graceful GOAWAY replacement does not set it. A separate first-transport flag avoids repeat readiness
  notices. The first observed size failure per process is promoted to WARN;
  repeats remain DEBUG. These flags are constant-size, not maps or histories.
- Reuse 20/s burst-40 ordinary admission. Text operational events have a second
  bounded bucket so DEBUG cannot consume their admission; DEBUG also leaves
  16 ordinary record slots and 16 KiB free, including against qlog admission. Each human record has a fixed size cap, making
  rate/count/byte bounds finite without another byte-rate subsystem. Queue and
  total diagnostic memory budgets are unchanged. Flapping can exhaust admission;
  blocked/broken sinks cannot guarantee delivery. No unbounded fallback is added.
- Text detects journald only when JOURNAL_STREAM device/inode matches stderr.
  Prefix real priorities and omit the duplicate timestamp; otherwise emit UTC
  timestamp/severity. No ANSI output. JSON remains unprefixed. Writer ACK is
  application write/flush completion, not proof of downstream retention.

## Compatibility and migration

Omitted diagnostics now means human operator mode, not legacy JSON/status.
Pin format=json and snapshots=true in collectors before upgrading. Generated
fixtures and benchmark/reference consumers do so explicitly. Normal benchmark
collection rejects missing/malformed snapshots instead of using empty metrics.
The deliberately blocked-collector scenario retains its explicit incomplete
coverage exception. Existing full diagnostic schema/field meanings are unchanged;
operator-only events never enter legacy JSON. TRACE currently adds no separate
raw upstream transport stream beyond safe DEBUG detail.

The spec's mandatory correlation/SC-010 guarantees now explicitly require JSON
capture with a functioning in-capacity collector. Disabled capture is not
successful complete collection. This changes an output policy, not protocol,
credential validation, resource admission, retry or forwarding semantics.

## Verification

Exercise default idle/forwarding, size warn-once, public request flood, initial
permanent refusal, retry silence and real accepted recovery, debug saturation,
malformed config, privacy, bounded records, journal framing, JSON regression and
broken status output. Run a short native legacy benchmark collector smoke.
Record actual checks and limits in [operator log validation](../validation/operator-logs.md).
Historic performance results retain their original binaries and policies; a
collector smoke is not a new baseline, independent interoperability or live VPS
qualification. Independent review of the plan does not imply a code re-review.


## Implementation review follow-up

The independent implementation review requested changes: restore the legacy fatal
marker, avoid GOAWAY's false outage/recovery narrative, expose temporary client
CONNECT setup failure, and include qlog in operator queue headroom. The follow-up
keeps ordinary typed messages separate from a JSON-only `runtime failed: <typed
RuntimeError>` marker so existing collectors never parse English error prose.

Reconnect scheduling explicitly distinguishes failure from graceful draining.
Actual loss of a verified primary or a draining connection with accepted streams
is still visible; retiring an empty draining connection is not an outage.
Transient HTTP 502/503/504 and setup timeout set one fixed setup-unavailable flag,
not a periodic incident. A later acceptance clears it and reports initial
acceptance or renewed setup success without claiming surviving flows had failed.
The flags and queue remain constant-size. Cached owner timestamps are clamped
only for operator-event admission, never for retry/protocol deadlines.

Queue-level operator reservations are enforced before byte/count charging for
all low-priority producers, including qlog. JSON-mode budgets are unchanged.
