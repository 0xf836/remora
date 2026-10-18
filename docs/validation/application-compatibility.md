# Application compatibility evidence

Raw summaries, observations and traffic/status logs remain in ignored local
`docs/validation/wireguard-results/` archives. This report preserves the tested
profile, binary identity, outcomes and limits; see [artifact retention](README.md).

2026-10-11: generic UDP forwarding is verified incrementally on native x86_64
and the pinned x86_64 build image. The process suites and IPv4/IPv6 runner
exchange unchanged 0/1/128/512/1200-byte datagrams for two distinct senders,
verify target socket isolation and shutdown, and independently deny unauthorized
traffic. The short one-session baseline/tunnel experiment has zero delivery or
integrity loss under its [declared coverage](../performance.md).

These finite functional cases and short measurements complement the duration
evidence below. Full concurrency/churn/impairment matrices and final-revision
release qualification remain separate tasks.

The operator-authorized [local KVM lab](local-kvm.md) now supplies two dedicated
Debian 13 guests and independent WG keys/routes/guest firewall state. The
Remora product and qualification runner do not provision this state. The
short successful run
measured 100/100 packets in each direction on each WG/generic channel, with
zero measured integrity/delivery errors, fresh WG handshake/counter evidence
and both Remora application directions. Its configured WG MTU is 1120 and WG
QUIC idle timeout is 15000 ms; these explicit fixture settings are documented
in the lab guide. Failed earlier attempts are retained alongside the result.

## T037: completed 30-minute local qualification

On 2026-10-11 the full run
passed with `seconds: 1800` and `duration_qualified: true`. Two independently
prepared Debian 13 x86_64 KVM guests exchanged WireGuard and generic UDP
traffic simultaneously through separate Remora client/server pairs. Each
direction/channel sent and received exactly 18,000 measured datagrams:
72,000 total, with zero measured loss, duplicates, integrity errors, send
errors or skipped intervals. Each of the 60 sequence intervals contains
300 received datagrams per direction/channel.

All four Remora process identities remained unchanged with zero restarts.
Each peer recorded 16 distinct nonzero WG handshake timestamps; final
handshakes were approximately 14–15 seconds old, and WG receive/send counters
grew on both peers. Four structured Remora status streams confirm application
traffic in both directions, with no kernel RX overflow. Warmup is separate:
peer B recorded 81 WG send errors before handshake readiness, and each Remora
client recorded one `not_active` setup drop. Those are retained in the evidence
and are not measured-interval successes or failures.

T037 is complete for this declared profile: WG MTU 1120, IPv4 inner/outer
traffic, 64/128/512/1000-byte test payloads, 10 packets/s/channel/direction,
WG QUIC idle timeout 15000 ms, generic timeout 5000 ms and offloads disabled.
The guest firewall prevents direct access to the target UDP ports. This
satisfies the generic-UDP/WireGuard duration component of SC-001 for the
recorded binary; it does not establish full final-revision SC-001/release
acceptance, native ARM64 compatibility, impairment/load coverage or release
performance. Failed and short earlier runs remain separately identified.

The lab is now an [opt-in disposable command](local-kvm.md): its base image is
reused from a shared verified local cache, and guests/network/disks are removed
after each one-shot test. A further
one-shot smoke
passed; a deliberately interrupted run verified cleanup and remains invalid
as qualification evidence. No Remora VM/network/volume resources were left
after this follow-up. The full 30-minute run also used the verified cached
image without downloading it and automatically removed its guests, network,
volumes and private credentials. Post-run libvirt and host-link inspection
confirmed that no Remora lab resources remained; the shared image cache and
small ownership manifest were retained.
