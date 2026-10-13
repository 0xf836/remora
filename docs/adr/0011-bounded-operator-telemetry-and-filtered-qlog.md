# ADR-0011: Bounded operator telemetry and filtered qlog

Date: 2026-10-13. Status: implemented for T064–T070; T071 overhead and final
security/release review remain pending. No independent reviewer was requested.

## Context and options

The MVP already has finite event/status queues, a versioned fixed diagnostic
schema, terminal/live failure ring and writer acknowledgements. Complete
operator diagnosis needs cumulative class totals after cleanup, queue/pool
peaks, fixed histograms, sampled path statistics, update/revocation evidence
and optional transport metadata. Raw upstream qlog includes H3 headers, CIDs,
custom fields and frame data; direct or temporary disk spooling is unacceptable.
An unrestricted tracing subscriber also bypasses diagnostic privacy/bounds.

Options were raw spooling plus postprocessing, an unbounded JSON object tree,
a second disk-writing loop, or incremental typed reconstruction into the
existing bounded writer. The first two violate privacy or resource invariants;
a second output loop duplicates scheduling/reservation ownership.

## Decision and security boundary

Use the existing owner/writer handoff. Qlog activation reserves 64 KiB per
connection against a 512 KiB pool before allocating: 32 KiB input plus up to
32 KiB serde escape/nesting scratch. Unknown nested data is skipped without
materializing values. Typed schemas accept pinned quiche 0.30.0/qlog 0.18.1's
sequential file header and reconstruct only numeric packet/time/recovery
metadata; no raw input API reaches the filesystem. Unknown/H3 events and
fields are discarded; malformed/oversize/new file schema disables that trace.
Frame metadata is omitted entirely as a conservative permitted subset.

The process owns at most eight filters, one fixed monotonic rate window and
lifetime directory bytes. Count/byte quotas are checked before queueing.
Conservative reservations persist after a partial/failed output. A fresh,
private, empty directory is opened at startup with NOFOLLOW and held by FD;
the writer creates mode-0600 numeric files with openat/EXCL/NOFOLLOW. Trusted
operator parent directories are not an untrusted peer input. Directory symlink,
wrong ownership/mode and prior output are refused. No existing file is deleted
or overwritten and churn does not create an unbounded filename/FD registry.
File handles follow filter/queued-record ownership. Only sanitized records reach
writer file creation; raw scratch has no temporary-file path.

Qlog uses the same 768 KiB/768-record event reservation; status keeps its own
256 KiB/256-record reservation. Total diagnostic retention remains 2 MiB:
1 MiB output, 512 KiB filter charge and 512 KiB non-qlog storage. The latter
covers the 64 KiB summary ring, owner formatting/status copies, parser/output
metadata, one 128 KiB writer stack and bounded channel/ACK bookkeeping. Qlog
record reconstruction is under 2 KiB, below the existing 64 KiB formatter cap.
Disk failures disable only optional tracing and are counted separately. No
keylog API is called and no arbitrary upstream log target is forwarded.

Status gains fixed cumulative failure counters, lifecycle totals, finite
histograms and active-path statistics sampled once per five seconds during
packet work. Admission/FD/pool and individual Session queue peaks are actual
ledger/queue peaks. Aggregate queue peaks are explicitly observed sample
maxima; the schema names that sampling limitation. Active-path retransmit/PTO
values represent currently sampled connections, while the existing process
lost-packet estimate remains cumulative. Neither is inner delivered loss.

A malformed/sized packet's known association can be correlated before dropping,
without retaining user tuples. Requests refused before Session allocation get
an opaque bounded request ID, not allocated relay state. Pre-session TLS faults
retain connection-only attribution. The ring coalesces only repeated live
classes and preserves distinct class/generation failures. Multi-chunk status
ACK now checks any failed write before retiring covered entries; the regression
covers a failed first chunk followed by a successful final ACK.

## Compatibility, evidence and performance boundary

Production remains one Rust package/executable and one exclusive reactor.
No new dependency, unsafe operation, wire extension, secret override or live
reload is introduced. Schema version 1 gains fixed additive fields and the
`size` class; collectors must accept added fixed keys/classes. qlog_directory
previously had no runtime implementation; it now requires the optional build
feature and a fresh empty private directory. Sanitized files are Remora NDJSON,
not a raw qlog replay format. No credentials or target identifiers are included.

[Operations](../operations.md) and [troubleshooting](../troubleshooting.md)
describe collector limits, meanings, controlled restart completion, explicit
rollback policy selection and external UDP provenance/commitment boundaries.
[Validation](../validation/operations.md) distinguishes process checks,
deterministic fault completions, intentional overflow and unmeasured overhead.

Before edits, base revision was `34d4a8fa13aa67436e057773d1a3199b69aba233`;
the existing default release executable SHA-256 was
`dee40610068f3f20ccdb8799230bfef8a74022d5c6e02fb5ab09e97f7ca49602`.
These identify historical inputs, not a diagnostics A/B reference measurement
or an independently matched build claim. The user explicitly excluded T071;
no diagnostic-mode benchmark, prospective trade-off policy or overhead
acceptance is performed here. Before T071 candidate measurements, freeze its
matching reference/config/workload and prospective metric bounds. Functional
and finite-state checks do not waive that outstanding constitutional
performance obligation or the final explicit security review before merge/release.


## T071 follow-up measurement — 2026-10-13

A later user request collected scoped diagnostic-mode repeatability measurements after the
original T071 exclusion. Production Rust remains unchanged: preserve the
pre-T071 Phase 7 executable/source identity, collect a matched reference,
freeze mode/metric bounds from variability, then measure the candidate using
the same binaries. The test-only harness records four safe modes, owner/process
CPU, RSS, delivery/RTT and collector/trace limits. It preserves mandatory
status even when ordinary detail is off. Deliberately blocked collection is
incomplete evidence, not SC-010 success. [Performance evidence](../performance.md#phase-7-t071-diagnostics-measurement-method--2026-10-13)
records 70/70 per-run passes and the explicit one-Session/100pps scope; no
retroactive pre-T064 or full release performance claim is made.


Review correction: this same-binary comparison is repeatability evidence only.
At that review checkpoint T071 regression acceptance remained pending; it required a matching pre-Phase-7
implementation and prospectively justified trade-off limits. No retrospective
freeze-before-edits evidence or requirement exception is claimed. Later code
corrections require verification on their own executable identity.


## T071 cross-revision acceptance follow-up

A fresh controlled candidate reconstruction after frozen Phase 6 reference /
mode budgets now passes all 70 paired checks on the declared low-rate native
profile. Separate physical cores, interleaved reference repetitions and bounded
taskset readiness are measurement controls. They change no production protocol
or deployment settings. Original baseline-drift/startup-invalid results and
policies remain retained; the original missed freeze before authorship is not
rewritten. [Final evidence](../performance.md#final-t071-cross-revision-results)
records exact identities, noise allowances and remaining release/high-rate scope.
