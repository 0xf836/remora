# Independent review of the UDP drop telemetry fix

Date: 2026-10-26. Role: reviewer, performing a read-only audit of the working
tree, original kernel artifacts and retained validation results. No new builds
or workload campaigns ran, no sources were changed and no server connections
were made. This file records the reviewer's verdict and recommendations;
no fixes were applied during the review.

**Verdict: APPROVE WITH NON-BLOCKING FOLLOW-UPS.** No blocking defect was found
in the runtime solution. The current binary is suitable for the owner's manual
validation on the affected server. Checking its exact kernel remains an
operator acceptance step, rather than an identified defect in the fix.

## Rationale for the solution

The retained Oops and disassembly identify the failing call as a four-byte
SO_RXQ_OVFL control message. In Linux 6.12.107 sources, that branch depends on
SOCK_RXQ_OVFL. SO_MEMINFO reads the same sk_drops counter through a separate
kernel-local array, without using a source inside the skb. Removing enablement
of the option excludes the established failure path.

Primary sources: [net/socket.c](https://github.com/gregkh/linux/blob/v6.12.107/net/socket.c#L935),
[net/core/sock.c](https://github.com/gregkh/linux/blob/v6.12.107/net/core/sock.c#L1912).

In src/udp.rs:266, the owned fd lifetime, alignment and FFI buffer size are
correct. Short responses are rejected, the baseline updates only after a
successful read, u32 wrap is handled and repeated accounting is avoided.
Query failures do not become fatal forwarding errors. Text/JSON behavior and
JSON snapshots=false are implemented consistently. The interceptor artifacts
confirm no attempts to enable SO_RXQ_OVFL and no SO_MEMINFO queries in Text.

## 1. Additional gap: benchmark collectors omit incomplete kernel telemetry

Code: scripts/validate/benchmark.py:19, scripts/validate/reference.py:575,
:676, :682.

Classification: a non-blocking integration gap in validation tooling. The
runtime correctly emits kernel_rx_overflow_incomplete=true, but the collector
accepts that snapshot and carries only the older coverage_lost,
status_write_failures and counter_overflow into the diagnostic summary.

The reviewer ran a short check of the collector functions against a retained
snapshot from an actual fault-injection run:

```text
accepted_by_snapshot_parser = true
kernel_rx_overflow_incomplete = true
kernel_rx_overflow_read_errors = 1
pipeline_coverage_lost = 0
counter_overflow = false
```

A future benchmark summary could therefore omit the unavailability of the
kernel drop counter. This does not invalidate measured endpoint delivery or
latency, or the current VM report, which discusses the new coverage gap
separately. Before the next qualification campaign, preserve a separate kernel
telemetry completeness indicator and prohibit conclusions that kernel drops
are absent when coverage is incomplete. This indicator should remain separate
from loss of diagnostic log records.

## 2. Repeated .skip(cursor)

Code: src/reactor.rs:3582. Classification: a confirmed algorithm property and
a scaling risk, not a demonstrated forwarding regression.

Repeated HashMap traversal adds approximately N²/16 work per sweep; traversal
cost also depends on table capacity. The limit of 16 bounds periodic syscalls,
not element visits or portion duration. Session churn can cause a socket to
be queried again or skipped until the next sweep. The baseline prevents double
counting, but a strict per-socket delay bound under continuous churn has not
been established. No functional problem was found with 31 sessions. Much
higher limits require measurement and, if necessary, a stable cursor or a
queue of socket generations. An immediate redesign is not required for a
small number of sessions.

## 3. Final queries and regression tests

Test code: src/reactor.rs:4414. Classification: an actual automated coverage
gap, but no missing production call was found.

The test calls record_rx_drops directly and would not detect removal of one of
the actual final-collection calls. The reviewer traced their presence before
removal of a client outer socket (src/reactor.rs:868), release of a relay
(:1001) and final closure of the remaining outer/listener sockets (:4286).
Targeted tests should inject a nonzero last increment and a final read failure
through these three lifecycle paths. VM restart/retirement was additionally
checked, but does not replace precise regression coverage for each path.

## 4. Final queries beyond the periodic quota

Contract: specs/001-masque-udp-tunnel/contracts/diagnostics.md:289.
Classification: an acceptable limitation and ambiguous documentation.

Sixteen queries is the periodic portion limit. Mass remove_connection/shutdown
can additionally query every socket being closed. The count is bounded by
configured resources; shutdown was fast with 31 sessions. Suggested wording:
"at most 16 periodic collection queries per turn; final queries are accounted
for separately." Control latency should be measured at high limits.

## 5. Initial failed readiness probes

Report: docs/validation/socket-drop-telemetry-report.md:126.
Classification: a weakness in the first test driver, not a confirmed recovery
bug. The original pass=false was retained. Waiting for actual readiness showed
recovery in 7.44–7.51 s, followed by successful SSH. This does not guarantee the
same duration on another network; future scenarios need a readiness gate.

Related documents:
[implementation and tests](socket-drop-telemetry.md),
[validation results](socket-drop-telemetry-report.md),
[ADR 0034](../adr/0034-socket-drop-telemetry.md).

## Follow-up changes after the review

The findings in this read-only audit describe the state before subsequent
changes. The socket registry, collectors, targeted closure tests and quota
wording were updated after the audit; see
[socket-drop-telemetry-followups.md](socket-drop-telemetry-followups.md).
This document does not claim a second independent review of those changes.
