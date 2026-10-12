# ADR-0009: Dynamic PMTU enforcement and bounded numeric profiling

Date: 2026-10-12. Status: implemented for native x86_64 verification.

One UDP message remains one unreliable application datagram. Current native
DATAGRAM capacity and verified path PMTU bound enqueue and packet generation;
connected target route MTU bounds actual relay submission. No reliable fallback
or private/IP fragmentation is introduced. Outer sockets use DF/PROBE so QUIC
DPLPMTUD probes can exceed a cached route size; target sockets use DO, and IPv6
also uses DONTFRAG. Configured receive caps reject intact oversized packets.

Pinned quiche 0.30.0 revalidation resets the verified path PMTU to 1200 bytes,
while `dgram_max_writable_len()` can still use the larger recovery maximum.
Clamp the difference between recovery allowance and verified path allowance,
then subtract only H3 association/Context prefixes. Tests reproduce the actual
public API discrepancy, queue a large and an empty message, and verify that the
large reservation alone is purged without closing the flow. Requery each turn;
do not cache a permanent small application ceiling. QUIC framing overhead is
already included in the native writable capacity.

Error queues are untrusted metadata: validate native ancillary lengths,
family, permitted LOCAL/ICMP/ICMP6 origins, packet-too-big type/code and original
destination. Process four entries per visit; discard quotes without recording
payload. Connected target destinations must match their immutable peer. Shared
outer PMTU metadata affects only matching active connection paths. An outer
EMSGSIZE discards the exact unaccepted FIFO front; no Session loss is invented
for encrypted packets and previously accepted prefixes are never replayed.
Non-PMTU target ICMP errors are observations, not automatic session closure.

Linux can expose an earlier UDP error on a later send, including a send to
another destination on the shared server socket. The errno alone cannot name
the affected connection or revalidate its PMTU. Both receive and send paths
preserve associations on peer errors; checked error-queue destination metadata
alone can revalidate an active QUIC path. A failed send accepted no packet:
discard only its unaccepted FIFO front, never replay an accepted prefix, count
an outer observation without attributing Session loss, and continue bounded
service. Relay immediate/queued sends count the unaccepted application packet
as `socket_error` and retain the same pinned Session/socket. Hard socket errors
still terminate the affected owner/Session. Native regressions cover an error
queue longer than one four-entry visit and a send to an unrelated live peer,
plus both relay submission paths after the same target recovers.

Profiling uses fixed numeric scalar counters for reviewed syscall boundaries,
payload slabs, custom quiche factories, explicit copies and scheduler ages.
It does not install a global allocator, dump packet buffers or export new
endpoint labels. Counter overflow invalidates measurements without changing
already-completed send progress. OS context switches and CPU IP samples are
harness measurements, not inferred from scheduler visit counts. Crypto and
metadata allocations/copies remain outside these instrumentation boundaries.

Changing these semantics preserves the configuration/wire format; snapshots
add fixed schema-1 numeric fields. Optional outer offloads remain governed by
ADR-0002. Release/ARM64 qualification is separate from native incremental tests.

The native burst characterization exposed synchronous ENOBUFS (errno 105)
from runner-owned netem queue pressure. Preserve the association: drop only the
unaccepted outer FIFO front, count `outer_send_pressure`, and allow QUIC control
recovery. It is not a PMTU observation or fabricated Session application loss.
Relay ENOBUFS drops the exact application packet with `socket_error` without
closing its Session. EAGAIN continues to retain its original suffix/deadline;
hard errors retain numeric errno in the connection failure record. Adapter
`send_enobufs`/`send_emsgsize` count actual failed native submissions, separately
from PMTU prechecks. Scripted accepted-prefix tests cover both failure classes;
original native failures remain retained before the fix.

Reduced-MTU native testing reproduced legal request-before-SETTINGS ordering:
loss of the client's control-stream packet let its later request stream arrive
first. HTTP/3 streams have no cross-stream delivery order. The server now
retains at most 32 fields/16 KiB in a pending Session charged to the existing
global/per-connection Session and pending-setup budgets, with the original
five-second setup deadline. It starts no authentication/DNS/relay/response/data
forwarding until peer SETTINGS and required DATAGRAM capability are verified.
Repack native header capacities into exact buffers, include their fixed memory
in control metadata accounting, and service four deferred requests with a
separate rotating cursor per connection. Expiry, reset, duplicate headers,
close and unsupported settings release the reserved Session and its headers.
Missing settings is not a success or permission to bypass capability checks.
The MTU 1280 namespace reproduction passes after this change; its original
failures remain retained.
