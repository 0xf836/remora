# ADR 0027: Finite commitment sweeps and interrupted syscall budgets

Date: 2026-10-18. Status: implemented; verification is recorded in
[the review-fix record](../validation/final-review-p2.md).

## Context

The final independent review found two P2 gaps in the existing bounded-owner
contract. A four-session commitment visit requested continuation only if all
four receipts were ready, skipping later ready receipts behind an ACK-pending
prefix. UDP receive/send and timerfd reads retried EINTR without an attempt
limit, preventing the owner from polling cancellation under persistent errors.

## Decision

Retain the four-session quota and the existing generation-fair cursor. Each
connection tracks a finite count of commitment candidates remaining to inspect.
The count is independent of activation success. A finished sweep remains
finished until native input, a due QUIC timer, or response setup changes the
connection state. These events request a fresh sweep without resetting the
fair cursor. Session removal caps the remaining count to the live candidate
count. ACK-pending receipts alone cannot restart a sweep or drive a busy loop.

Use one private retry helper for receive, send and timerfd read, with at most
four syscall attempts per adapter visit (the initial call and three retries).
Only EINTR is retried; success, partial/empty acceptance and all other errors
return immediately. UDP counters still count every actual attempt and EINTR.
Exhaustion returns Interrupted with no accepted messages; existing owner
continuations retain original packet suffixes, generations and deadlines while
polling cancellation and other connections between visits. Timer readiness is
retained for a later bounded poll.

## Alternatives and consequences

Increasing quotas or scanning every session at once weakens bounded service;
continuing merely because receipts remain recreates an ACK-wait busy loop.
Unbounded interrupted retries violate cancellation service. Failing a connection
on EINTR would turn a retryable local observation into an unnecessary outage.

The decision adds fixed per-connection sweep metadata and no queues, tasks,
dependencies, configuration fields, wire changes or new unsafe operations.
The native FFI calls and accepted-prefix rules remain unchanged. Regressions
exercise real selectively ACKed response buffers and complete owner stages,
plus the retry policy shared by the production syscall adapters. A focused
pre-change/candidate performance comparison uses prospectively frozen existing
bounds. Historical qualification is not transferred to the changed executable.
