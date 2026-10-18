# Native Phase 5 size and impairment characterization

Date: 2026-10-12. Linux x86_64; functional characterization, no SC throughput,
resource, recovery-duration or ARM64 qualification claim.

Run `./scripts/validate/network.sh --output benchmarks/results/network-NEW`
from the repository root after a locked release build. `--scenario` selects
reference, rtt50, rtt100, loss01, loss1, asymmetric, burst, mtu1280 or mtu-change.
The bootstrap creates fresh user/network namespaces with inherited descriptors
of the original namespaces. The worker rejects descriptors for its current
namespace, rejects wrong descriptor types and requires a setup namespace with
only loopback. Create anonymous owned client/server namespaces, move a private
veth into those namespaces, and apply tc only through their retained child PIDs.
Clean up owned process groups before reaping their leaders; namespace/veth
lifetime ends with those owned children. No host interfaces, routes, sysctls,
firewall, named /var/run namespaces or production state are changed.

Each direction is shaped at 1Gbit/s; nominal 20/50/100ms RTT uses half-delay per
veth. The asymmetric case uses 10ms/40ms. Loss is seeded 0.1%/1% per egress;
burst uses a 32-packet netem queue versus the ordinary 2048-packet queue. Reduced
MTU is 1280 initially, or 1500 -> 1240 during the same IPv4 association. The
latter forces an application/ciphertext boundary that exceeds the new path.
Reject a generator/shaping-rate mismatch before any capability work. Record
complete link/address/qdisc statistics and all netdev feature names with a
bounded read-only ethtool ioctl inventory; these are veth features, not proof
of physical NIC throughput or of implemented Remora GSO/GRO.

Every case uses exact UDP equality for empty/1/128/512/960/1200-byte messages,
20 sequential attempts/size and a tagged 64-message burst. Timed-out late
known replies are counted separately; unexpected bytes/source and duplicate
burst tags fail. Empty and one-byte replies establish boundary integrity,
not sequence-based loss attribution. Attempt 1201 bytes and verify both no
matching response and zero messages of that size at the actual target.
A healthy 128-byte probe after pressure/reduction must still reply. Small
messages must make progress; larger messages may correctly drop under PMTU.

IPv4 fragment-only tcpdump observers on both owned veths retain header-only
text and require zero fragments and zero kernel capture drops. Remora socket
options for IPv4 DF/DO versus PROBE and IPv6 DONTFRAG are checked by native Rust
syscall tests. IPv6 empty/mixed/at/above-limit equality is covered separately by
process E2E, not by this IPv4 shaped topology. Do not infer IPv6 impaired-path
or physical-network qualification from these cases.

Final functional matrix: all nine cases pass integrity, actual-endpoint size
rejection, post-pressure control progress and no-fragment observation. Table
shows median measured replies, the first burst and lifetime numeric observations;
loss/PMTU/pressure counts are not all the same quantity.

| Scenario | Reply median RTT (ms) | Burst received | Outer PMTU client/server | Outer send pressure client/server |
|---|---|---|---|---|
| asymmetric | 50.48 | 41/64 | 0/0 | 0/0 |
| burst | 20.41 | 30/64 | 0/0 | 2/2 |
| loss01 | 20.38 | 34/64 | 0/0 | 0/0 |
| loss1 | 20.30 | 34/64 | 0/0 | 0/0 |
| mtu-change | 20.38 | 34/64 | 32/0 | 0/0 |
| mtu1280 | 20.48 | 34/64 | 27/25 | 0/0 |
| reference | 20.22 | 34/64 | 0/0 | 0/0 |
| rtt100 | 100.41 | 33/64 | 0/0 | 0/0 |
| rtt50 | 50.48 | 34/64 | 0/0 | 0/0 |

The active-MTU reduction retains control progress and records 32 outer PMTU
observations plus 19 application PMTU drops at the client. Kernel overflow and
outer QUIC lost-packet estimates remain separate in each saved diagnostic
snapshot. No loss or throughput guarantee is extrapolated to impaired paths;
20 attempts and 64-message bursts are characterization, not capacity estimates.

The nine-case matrix is bound to executable `90baeff339e86975c5dfaf86093f13f75cc26740bb0cb3ebcd0dc1c6fa2709b8`. The final
executable `bd9f20254da298ac43e7a46c8fdfccf58a4d59837b0f2d3fa3175f47694120f5` differs only by restoring the existing trust/0 diagnostic
code on TLS rejection; that final change is separately verified by the complete
Rust suite and all 29 pinned independent Go cases. The original native failures
are retained: burst exposed ENOBUFS closing connections; MTU 1280 exposed legal
request-before-SETTINGS ordering. Both are fixed and regression-tested. A
fragment-observer whitespace parsing error and an unavailable parent /proc
namespace read were harness failures and are retained, not passing placeholders.

Artifacts: `benchmarks/results/phase5-network-final-v2/`; earlier failures in
`phase5-network`, `phase5-network-verified`, `phase5-burst-*`, `phase5-mtu-*`.
PMTU observations may include synchronous and error-queue reports for one
operation. They are not unique lost packets or fabricated Session attribution.
