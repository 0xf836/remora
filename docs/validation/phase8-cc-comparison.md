# Congestion-control comparison — 2026-10-16

Status: controlled-loss and broader focused comparisons complete; no full
BBR2 qualification or CC default change.

The owner subsequently selects CUBIC and accepts the measured output-order
CPU cost; see [ADR 0023](../adr/0023-select-cubic-and-accept-measured-cpu-cost.md).
Further BBR2 diagnosis is suspended for this Phase 8 increment. The existing
BBR2 option and the earlier controlled-loss evidence below remain valid within
their exact scope. Final CUBIC qualification is pending.

The pinned quiche 0.30.0 supports Reno, CUBIC and `bbr2_gcongestion`; see its
[algorithm API](https://docs.quic.tech/quiche/enum.CongestionControlAlgorithm.html).
These are sender-side QUIC algorithms configured by Remora's `transport.cc`,
not the operating system's TCP congestion-control setting. Both test roles use
the selected algorithm. No new dependency or implementation was added.

## Method

`benchmarks/results/phase8-final-cc-comparison-20261016/` retains 18 runs:
three algorithms × three alternating control/drop-five pairs. Algorithm order
rotates each repeat. Every run uses the same qualified executable
`1229d87f5fe1fbfbf5c0ccf231d033af2e96f1645184d05cd7f826c94e78cdee`,
traffic tool `cb2f0d710ce8c6f705051bb958aa683b1499c4cdf6d9d8bfd4a09a17cb866f50`,
one session, 512-byte simultaneous application traffic at 91552 datagrams/s
per direction, three-second loaded warmup and 15-second measurement. Topology
is native x86_64 private namespaces/veth, 1Gbit/s each direction and 20ms RTT;
mmsg32, no GSO/GRO. Client/server/tool CPUs and collector are disjoint.

At measured +6s, an atomic rule in the owned client namespace drops the first
five server-to-client outer UDP packets larger than 400 bytes. Retained nft
counters confirm exactly five drops each time. Matching controls install a
zero-drop rule. Existing outer buffer request is 262144 bytes. Source and
executable hashes remained unchanged through the campaign; all generator and
integrity checks pass. Initial baseline CLI implementation edits were not
built during this campaign; its measured traffic executable is the old hash above.

## Results

| Algorithm | Reverse injected-run losses, three repeats | Missing application datagrams | Injected-run p95 RTT | Highest role CPU |
|---|---|---|---|---:|
| CUBIC | 2.769064%, 2.326619%, 2.039788% | 38027, 31951, 28012 | 21.058–21.396ms | 0.7054 core |
| Reno | 11.421050%, 10.961275%, 11.185701% | 156843, 150529, 153611 | 21.053–21.691ms | 0.7247 core |
| BBR2 gcongestion | 0.000655%, 0.000728%, 0.000655% | 9, 10, 9 | 20.332–20.407ms | 0.7127 core |

Reverse control loss is zero except one Reno run at 0.006554% and one CUBIC
run at 0.000146%. BBR2 reverse controls are all zero. Every BBR2 forward run
is also below the 0.1 percentage-point threshold (maximum 0.018059%).
All algorithms' injected runs remain below 11MiB peak role RSS.

BBR2 prevents the large loss amplification in this scenario: application loss
is close to the datagrams carried by the five deliberately lost packets. It
does not recover those unreliable datagrams. Reno amplifies loss more severely
than CUBIC at this offered rate. No memory or CPU penalty is apparent in these
short runs; this is not a statistical claim of equal efficiency.

## Decision and limits

BBR2 is a useful existing configuration candidate for lossy paths:

```toml
[transport]
cc = "bbr2_gcongestion"
```

It cannot prevent initial local receive-buffer overflow. The outer-buffer
candidate in ADR 0021 addresses that trigger and retains CUBIC by default.
The completed [focused matrix](phase8-outer-buffer-comparison.md) additionally
compares BBR2 with enlarged buffers on 128-byte, 512-byte, capacity and resource
workloads. BBR2 meets the general SC thresholds in all twelve cases but uses
10.1–12.5% more small-packet CPU than adjacent CUBIC, and its 512-byte forward
loss reaches 0.086727%. No full acceptance or unconditional default switch follows.

Neither the five-packet test nor that focused matrix qualifies shared-bottleneck
fairness, real NIC/WAN behavior, other RTTs, persistent random loss, native ARM64
or complete T077 acceptance. The production CC default remains CUBIC. Any
future default/profile selection must explicitly bind its evidence and risks.
Machine-readable values are retained in the campaign's `analysis.json`.

## Current CUBIC controlled-loss characterization

`benchmarks/results/phase8-cubic-output-order-loss-20261016/` retains three
alternating control/drop-five pairs on the corrected output-order qlog binary
`7f7a3c4de7341b8edf0860ff3c8457642ff423cf4658f9aa93076db678cfe3b1`.
Each run is 1x512 bytes, 91552pps/direction, three-second warmup and 15 measured
seconds at 20ms RTT/1Gbit/s, mmsg32/offloads disabled. This candidate requests
1MiB outer buffers and 128KiB application buffers. At measured +6s the private
client namespace drops exactly five server-to-client outer packets larger than
400 bytes; retained nft counters prove five/zero for impaired/control cases.

| Repeat | Control forward / reverse loss | Injected forward / reverse loss | Injected reverse missing |
|---|---|---|---:|
| 0 | 0% / 0% | 0% / 2.198022% | 30185 |
| 1 | 0.006408% / 0.000510% | 0.002476% / 1.956192% | 26864 |
| 2 | 0% / 0% | 0.000728% / 2.514200% | 34527 |

All six generator/integrity/calibration checks pass and production inputs stay
unchanged. Injected p95 RTT is 21.224–21.333ms. Thus the order correction does
not remove high-rate CUBIC application-loss amplification after outer loss.
No reference SC loss threshold is applied to deliberately impaired runs, and
no broad lossy-path support claim follows. Rules and namespace holders are
cleaned up after characterization. Full native reference and functional
qualification remain separate; these results do not reopen BBR2 investigation.
