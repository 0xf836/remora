# Diagnosing Remora

Collect stdout status and stderr events for both roles. Each schema-1 record
contains UTC time, a process epoch and monotonic sequence. Connection/Session
identifiers are opaque and valid only within that epoch. They are correlation
fields, not user labels. Never request payloads, headers or credential contents
for a diagnostic report. Include executable hash, config **field names and
nonsecret bounds**, kernel/tool versions and the observable failure class.

## Read the boundaries correctly

| Field | What it establishes |
|---|---|
| `recovery.client_state = active` | Verified transport and required settings; no destination response proof |
| `lifecycle.active` / `accepted_total` | Currently accepted Sessions / cumulative acceptances |
| `lifecycle.destination_response_datagrams` | Server observed messages from pinned destinations; zero can mean silent UDP |
| `app_ingress_*` / `app_egress_*` | Full application messages at role boundaries, with explicit direction |
| `quic_dgram_enqueued` | Accepted by native QUIC queue, before outer socket submission |
| `relay_socket_sent` | Successful application UDP syscall, before destination delivery |
| `outer_socket_*` | Submitted/received outer UDP messages and bytes |
| `local_drops` | Exact observed process drops by fixed reason/direction, each terminal drop counted once |
| `kernel_rx_overflow` | Observed kernel socket overflow, without invented Session attribution |
| `outer_quic_lost_packets` | Cumulative QUIC lost-packet estimate, distinct from inner delivered loss |
| `quic` | Active-path RTT/rttvar maxima, cwnd sum, minimum payload limit, retransmit/PTO counts for currently sampled live connections; sampled at most every five seconds, reset when retired |
| `histograms` | Cumulative fixed-bucket observations of queue age, ciphertext age, pacing lateness, emitted logical burst and completed recovery duration |
| `profile.io` | Actual syscall/message/ENOBUFS/EMSGSIZE/EAGAIN/EINTR/partial-prefix observations; no fabricated offload probe |
| `resources` | Current admission, queue, FD, slab/native/control storage and documented peaks |
| `failure_totals` | Cumulative observations/repeats for fixed failure classes, surviving summary ACK/cleanup |
| `qlog` | Filter/scratch/accepted/discarded/stop/file-reservation/output-failure counts; no raw trace text |

Histogram bounds are `[0, 10, 100, 1000, 10000, 100000, 1000000, +infinity]`
with inclusive upper limits. Time histograms are microseconds, burst is logical
UDP messages. Queue age samples are service visits (a deferred front can be
observed again); ciphertext and pacing samples are accepted outer messages;
nonempty emitted bursts are successful send prefixes. They are lifetime
observations, not a measurement-window p95 or independent delivery count.

Admission/FD peaks and retained pool peaks are actual ledger maxima. Per-Session
queue peaks retain the largest individual queue packets/bytes seen, including
Sessions closed before collection. `queued_*_observed_peak` is the largest
sampled aggregate queue gauge; a transient aggregate between samples can be
higher. Report this sampling boundary rather than treating it as an exact
concurrent global maximum. `authenticating` is zero because authentication is
synchronous inside an owner visit; bounded setup can be starting/authorizing.
Kernel memory, native TLS/control allocations and total RSS are separate from
these slab/descriptor counts.

GSO/GRO report requested `off|auto`, `effective_on` and fixed
`configured_off|not_implemented|capability_unavailable|path_ineligible|enabled`
reasons. Availability never implies every packet is eligible. An auto mode with
`not_implemented` did not run a failed capability probe. Read the current
[performance evidence](performance.md) before claiming an acceleration.

## Act on fixed failure classes

| Observation | Operator action |
|---|---|
| Invalid config, exit 2 | Fix the named schema/field/range/file/permissions; re-run check-config |
| Runtime `Bind`, exit 1 | Check role listener ownership/port conflicts; validation performs no bind |
| `trust/0` | Check CA bundle, DNS SAN identity, certificate validity and numeric dial destination; do not weaken verification |
| `authentication/401` | Check privately provisioned token and canonical-text digest; controlled restart after correction |
| `authorization/403` | Check exact host/port and every DNS answer's numeric address permission |
| `authorization/404` | Check exact proxy route/authority profile |
| `capability/501` or capability refusal | Peer must support h3, DATAGRAM and Extended CONNECT; no reliable fallback |
| `resolution/502` | Observed resolver failure; inspect host/resolver availability privately |
| `setup_timeout/504` | Setup deadline expired; distinguish this from observed DNS NXDOMAIN/socket errno |
| `resource/503` | Admission exhausted; inspect Session/setup/identity/FD gauges and configured limits |
| `resource/1` / `resource/2` | Application slab / Session queue pressure; reduce load or review finite budgets |
| `size/1` / `size/2` / `size/3` | Configured application cap / current QUIC path ceiling / local receive truncation; adjust application size/MTU |
| `socket/<errno>` | Observed native socket failure; 111 is Linux ECONNREFUSED, 90 EMSGSIZE; absence of a response alone is insufficient |
| `peer_timeout/1` | Authenticated-progress watchdog expired; inspect path/server availability |
| `peer_timeout/0` | Native timeout without an additional close code in this profile |
| `transport/<numeric>` / `malformed/<numeric>` | QUIC/H3/parse/close category; use the [diagnostics contract](../specs/001-masque-udp-tunnel/contracts/diagnostics.md) for the fixed code space |
| `stopped/0` | Owned socket/Session cleanup completed; check writer/collection outcome separately |

Native transport categories 0..22, H3 wire errors and local 1001/1002/1003
codes do not include a peer's close reason. No arbitrary strings become reason
labels. Connection-wide faults identify each affected live Session plus the
connection. Rejected requests without an allocated Session receive a bounded
opaque request correlation ID; pre-session TLS trust errors identify only the
connection. No affected Session is invented before its creation.

## Assess collection coverage

Ordinary events have a 20/s bucket with burst 40. Filtered verbosity or suppressed
events do not remove status summaries. Each summary carries class/code,
connection/Session ID when available, terminal sequence, monotonic milliseconds
and repeats. The 256-entry/64 KiB ring coalesces repeated live classes, retains
distinct classes and converts pending summaries to terminal before cleanup.
Only a completed status batch ACK retires its covered versions. A failed earlier
chunk is not forgiven by a successful later chunk; newer updates survive old ACKs.

`diagnostics_dropped` measures ordinary event/queue loss. `status_write_failures`,
`coverage_lost`, `lost_first` and `lost_last` report different collection gaps.
A nonzero coverage loss cannot reconstruct the missing Session correlation.
`counter_overflow` makes numeric measurement invalid. Status has reserved
capacity and priority at writer record boundaries, but cannot interrupt a
blocked sink already writing a record.

At shutdown, one shared one-second drain sits inside the total five-second
stop budget after packet/socket cleanup. An incomplete drain/unfinished writer
returns `DiagnosticCollectionIncomplete` and exit 1. Preserve the output and
fix collector availability; do not call such a run complete SC-010 coverage.
Deliberate broken-sink/ring-overflow tests prove bounds and explicit gaps, not
successful delivery. Complete correlation requires a functioning collector and
in-capacity injected workload. External disk/log retention remains your policy.

Use [the reproducible operations walkthrough](operations.md#reproduce-the-walkthrough)
and [its measured functional evidence](validation/operations.md). T071 now records [scoped diagnostic overhead](performance.md#phase-7-t071-diagnostics-measurement-method--2026-10-13);
full performance, independent final-revision interop, native ARM64 and release
claims require their own qualification.
