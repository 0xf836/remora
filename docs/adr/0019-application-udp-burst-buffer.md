# ADR 0019: Increase the application UDP buffer default after queue-service fixes

Status: implemented; short diagnostic and full focused comparison pass;
release qualification remains pending.
Date: 2026-10-16.

## Evidence and decision

The history32/drain-before-producer candidate passes native/E2E checks and two
complete small/capacity/resource comparison repetitions. Its third small row
loses 13 forward messages of 972735. All 13 are client application-socket kernel
drops observed around measurement +6.75s. Session queue-full counters remain
unchanged during measurement; outer QUIC loss is zero. The frozen two-message
regression gate fails, and that failed comparison remains failed.

A separate same-binary three-repeat alternating configuration diagnostic uses
65536 versus 131072 requested application socket buffer bytes. One old-setting
row loses seven reverse messages, matching server application-socket kernel
drops; all three enlarged-setting rows have zero measured loss/socket drops.
The diagnostic is evidence for burst headroom, not full qualification or proof
that larger buffers eliminate every possible kernel drop.

After the reactor corrections, select 131072 requested bytes as the next
`io.relay_socket_buffer_bytes` default. This existing setting applies to RX/TX
on the client application listener and server per-session relay socket. Keep
outer buffers, user-space/native queues, expiry, pacing and admission unchanged.
No new configuration field or role-dependent hidden multiplier is introduced.
Explicit `65536` preserves the former setting. The configuration contract and
both-role parser/legacy-override test document this compatibility change.

## Prospective resource trade-off and author review

On the measured Linux host each requested 128 KiB becomes an effective 256 KiB
RX buffer and 256 KiB TX buffer, versus 128+128 KiB previously. This increases
the configured combined kernel RX/TX limit by 256 KiB per application socket.
These are limits, not eager allocations or process RSS. Capture effective
values, socket counts and observed queue-memory peaks; kernel clamping on a
different host must remain visible.

For the focused 32-session reference topology, the new combined application
socket limit is at most 16 MiB on the server (32 sockets) and 512 KiB on the
client (one shared listener). The increases are 8 MiB and 256 KiB respectively.
With default server limits, at most 128 sessions can own relay sockets, so the
configured increase is bounded by 32 MiB. General configured limits scale with
the admitted socket count; this does not introduce an unbounded memory pool.

Author review accepts that explicit kernel-memory cost for burst absorption
after correcting unnecessary software queue drops. Authentication and target
authorization still precede server relay allocation. Existing connection,
identity, session and FD ceilings remain in force. Larger kernel buffers do
not extend the residence time of already admitted user-space/native payloads
or create reliable retransmission. There is no new unsafe code, dependency,
task, lock, protocol extension or production-host sysctl change.

## Verification

Freeze a new prospective policy before the new candidate measurements. Preserve
every delivery/loss/RTT/CPU/RSS bound from the preceding policy, including the
two-message small-packet gate and all absolute SC targets. Declare the intended
buffer change and its kernel budget separately; do not relabel either failed
comparison. Require effective RX/TX application buffers of 262144 bytes on this
host, at most one client and `sessions` server application sockets, and record
configured/observed kernel memory independently of RSS.

Pass locked builds, formatting, Clippy, config/native/lifecycle/security/E2E and
independent-peer process checks before repeating the focused three-repeat
comparison. Full T076/T077/T078 qualification remains pending; T079 is excluded.

## Final evidence

The final qlog executable is
`1229d87f5fe1fbfbf5c0ccf231d033af2e96f1645184d05cd7f826c94e78cdee`.
Policy SHA-256 is
`e16362935def466725f5862fe23a9803a92b36abc9201ecadea0c696a1709dd6`.
All 27 direct/control/candidate rows complete and all nine candidate checks pass
in 1374 seconds (22m54s). Small/resource candidate endpoint loss is zero in both
directions in all three repeats. Capacity loss is at most 0.001171% per direction,
with delivered payload at least 874.89536 Mbit/s. Each individual repeat passes
the retained bounds; no averaging or tolerance waiver is applied.

The two-leg UDP baseline is rebuilt with matching enlarged application buffers.
Generator/responder source and dependency locks remain unchanged, their separate
package checks pass, and both old/new tool hashes are retained. The reference
and numeric bounds remain bound to the unchanged pre-correction Remora binary;
fresh adjacent controls/direct baselines validate the new comparison.

Locked default/qlog builds, formatting, Clippy, 179 ordinary Rust tests, one
explicit independent-peer process test and 20 traffic tests pass. Production
inputs remain unchanged throughout the comparison. Reports, effective socket
limits/counts, observed queue-memory peaks and preserved failed attempts are in
`benchmarks/results/phase8-density-window-20261016/drain-order/receiver-buffer/`.
