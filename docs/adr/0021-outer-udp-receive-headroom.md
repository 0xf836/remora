# ADR 0021: Increase outer UDP burst headroom

Status: implemented; focused CUBIC regression comparison not accepted; full qualification pending.
Date: 2026-10-16.

## Context and measured cause

The final x86_64 reference cycle failed after a small outer loss event amplified
into 1.975% reverse application loss. Controlled loss of five packets reproduces
2.78–2.87% application loss. A controlled 5ms client receive pause causes
164–180 outer socket drops and 2.10–2.22% reverse loss with the former 256KiB
buffer request. Requesting 1MiB prevents outer drops in all three repetitions,
reducing reverse application loss to 0.020–0.021%. See the
[diagnostic report](../validation/phase8-final-loss-diagnosis.md).

A separate 18-run algorithm comparison records three control/drop-five pairs
for CUBIC, Reno and `bbr2_gcongestion`. Reverse injected-run loss is respectively
2.04–2.77%, 10.96–11.42% and 0.00066–0.00073%. BBR2 limits amplification in this
test, but does not prevent receive overflow or qualify a complete alternative
profile. Reno is not selected as a remedy. These algorithms are already exposed
by the pinned dependency and configuration; no dependency is added.

## Decision and alternatives

Change the requested outer RX/TX buffer default from 262144 to 1048576 bytes.
Retain `io.outer_socket_buffer_bytes = 262144` as an explicit compatibility
override. Keep the 131072-byte application-buffer request, queue counts/bytes,
residence deadlines, exclusive transport ownership and CUBIC default unchanged.
The tested value is a bounded candidate, not a guarantee under arbitrary pauses.
It is not proven to be the smallest adequate setting.

Add test-only baseline CLI `--outer-socket-buffer-bytes` with the same default,
nonzero/i32 bounds and existing effective-buffer diagnostics. A matched baseline
must use the candidate's outer setting. Continue evaluating BBR2 as a separate
configuration in focused mixed-size/capacity/resource comparisons; changing the
production CC default requires broader evidence than the five-packet test.

Increasing application queues/residence would retain more data and delay expiry
without preventing the initial kernel loss; it is not selected. Increasing
receive scheduling priority cannot guarantee service through external process
pauses; the fixed owner service bounds remain intact. Changing CC alone leaves
kernel drops possible. Disabling congestion response is not an option.

## Bounded memory and security assessment

On this host, Linux effective RX/TX limits each increase from 524288 to 2097152
bytes. The combined outer ceiling rises from 1MiB to 4MiB per socket (+3MiB).
This is not eager allocation or process RSS. Record effective/clamped limits,
actual socket counts, observed queued memory and process RSS independently.

Server connections share configured outer listeners: at the validated maximum
16 listeners, combined outer ceilings increase from 16MiB to 64MiB (+48MiB).
Clients own connection sockets, bounded by `max_connections` (default 64):
at that bound, combined outer ceilings increase from 64MiB to 256MiB (+192MiB).
The focused one-listener/one-connection topology adds 3MiB per role; at 32
sessions the server's application ceilings remain 16MiB, separate from 4MiB
outer, while the client has 0.5MiB application plus 4MiB outer ceilings.

Larger sockets can retain more unauthenticated outer traffic, but introduce no
new address, authorization, trust or payload parsing path. Existing Retry,
connection/setup/resource caps, kernel validation and finite owner visits stay
in force. No new unsafe operation is introduced. Claims remain limited to
measured effective values; explicit overrides preserve lower memory budgets.

## Verification and acceptance boundary

Freeze comparison inputs/bounds before candidate measurement. Keep the preceding
numeric regression bounds for 128-byte, capacity and resource workloads, add
the failed 512-byte workload, require matched sustainable baselines, SC thresholds,
strict adjacent small-packet preservation and per-role CPU/RSS/kernel budgets.
Add a prospective <=0.1% candidate capacity-loss guard. Perform three repetitions
and retain invalid/failed results without waiver. Evaluate BBR2 separately with
the same SC thresholds; its optional diagnostic result cannot hide a CUBIC failure.

The production configuration change reopens T076 on the new binary; historical
T076 remains passed only for its recorded inputs. T077/T078 remain pending,
ARM64 unqualified and T079 excluded. No release claim follows from focused tests.

## Completed focused comparison

All 48 rows completed in 34m49s. CUBIC passed 10/12 frozen checks: capacity
repeat 0 reverse loss 0.019732% exceeded the retained 0.003676% bound, and
small-packet repeat 2 server CPU 0.408422 core exceeded 0.402575 (owner CPU
also exceeded its bound). General SC limits passed, but this does not waive
the frozen regression failures. The capacity row still recorded 92 server
outer socket drops with the larger buffer. Its receive-service trigger remains
unresolved. Adjacent old-control CPU was 0.406689, also above the historical
absolute bound; that suggests measurement/host variation may contribute to
the CPU finding, without proving equivalence or invalidating the result.

BBR2 passed its 12 general SC checks. It was prospectively a separately labelled
algorithm diagnostic without the historical CUBIC CPU regression bounds;
applying those bounds diagnostically would reject all three small-packet rows.
Small-packet CPU was 10.1–12.5% above adjacent CUBIC. At 512 bytes, BBR2 forward
loss reached 0.086727%, within but close to the 0.1-point loss target. No default
CC change or qualification is accepted from this result. See
[the comparison report](../validation/phase8-outer-buffer-comparison.md).
