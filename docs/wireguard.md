# Independently prepared WireGuard validation

WireGuard is an opaque UDP application to Remora. The operator owns peers,
keys, routes, TUN interfaces and firewall configuration. Remora does not
create, update or inspect private keys, install routes, or change VPN state.

For the WireGuard qualification runner, prepare two exclusively used peers
and a permitted server-side WireGuard UDP endpoint. Configure the local
WireGuard endpoint to the Remora client's application listener; its CONNECT-UDP
target is the approved server endpoint. Provide a private fixture describing
how to reach each test endpoint, read-only handshake/transfer observations,
the Remora status collectors for both channel pairs and the independently
managed traffic path.
The runner must own only its traffic/processes/results, never peer state.
No `wg show ... dump` output should be collected because it exposes private
material; use the specific `latest-handshakes` and `transfer` views instead.

Choose the WireGuard application MTU from the effective Remora payload ceiling
and WireGuard encapsulation overhead on the actual route. Remora's initial
1200-byte application limit is not a WireGuard interface-MTU guarantee, and
path limits may be smaller. Verify handshake freshness and independent
bidirectional application data for 30 minutes, correlate increasing peer
counters with both Remora directions, and preserve byte/boundary and timeout
results. A handshake alone, an echo-only capacity measurement or traffic that
bypasses Remora cannot qualify SC-001.

For a disposable local fixture, use the separate, explicitly invoked
[two-VM operator tool](validation/local-kvm.md). It prepares its own guests,
keys, routes and guest firewall, without changing existing host VPN peers.
The qualification runner itself does not perform this provisioning.

```bash
cargo build --locked --release --features qlog
cargo build --locked --release --manifest-path tools/traffic/Cargo.toml
./scripts/validate/libvirt-lab.sh test \
  --binary target/release/remora \
  --seconds 1800 --output /tmp/remora-wg-qualification-NEW
```

This is an explicit opt-in qualification command. It reuses a verified shared
local image cache and removes its disposable VM/network/storage resources on
completion, failure or supported interruption. Ordinary build/unit/CI checks
do not start VM. Persistent guests require the separate manual `create` mode.
For externally prepared peers use `wireguard.sh --fixture ... --start-remora`
directly; that runner owns only its test processes and does not remove guest
or peer state.

For already prepared dedicated peers, this is the controller command (the
private fixture must already exist; it is not a public example credential):

```bash
./scripts/validate/wireguard.sh --fixture /absolute/private/fixture.json \
  --start-remora --seconds 1800 --output /tmp/remora-wg-external-NEW
```

The private JSON fixture has `schema_version: 1`, an absolute `ssh_config`
path, exactly `roles.a`/`roles.b`, and a sanitized `public_environment`.
Each role provides `ssh_host`, `wg_interface`, `wg_bind`, `wg_peer`,
`generic_bind` and exactly two dedicated Remora `units`; the initiating
generic role also provides `generic_peer`. Bind/peer values are IPv4
`address:port` pairs in this runner. Environment records must contain
`wireguard_mtu` and `remora_binary_sha256`. The fixture and SSH config must
be 0600. See the [controller](../scripts/validate/wireguard.py) for the
current interface and the operator tool for a working fixture generator.

The runner requires installed matching binaries/traffic source, guest clocks
within one second, inner routes through the declared WG interface, and a
matching MTU. `--start-remora` starts fresh, bounded test units from the
prepared `/opt/remora-t037` configs and refuses to replace existing units.
Without it, the named Remora units must already be active and remain
operator-owned. Random traffic units and any Remora units started by the
runner are stopped on exit; WireGuard state is retained. Interrupted remote
work is bounded by systemd runtime limits.

Each direction independently sends integrity-checked WG and generic UDP
traffic, after warmup, at 10 packets/second per channel with 64/128/512/1000-byte
payloads. Passing requires exact measured delivery, no duplicates/integrity
errors/send errors/skipped intervals, growing WG transfer counters, fresh
handshakes, unchanged Remora process identities and both Remora application
directions in structured status. Specific WG observations exclude key material
from output. Public artifacts record the profile and hashes. Failures remain
invalid results. Only `--seconds 1800` can set `duration_qualified: true`.

## Current duration evidence and limits

The selected Phase 8 T076 run on 2026-10-16 repeated the 30-minute test with
qlog-build executable
`7f7a3c4de7341b8edf0860ff3c8457642ff423cf4658f9aa93076db678cfe3b1`.
It received **72000/72000 measured datagrams** across the generic and WireGuard
channels in both directions, with zero measured loss, corruption, duplicates,
send errors or skipped intervals. The **73 pre-readiness warmup send errors**
remain separate evidence. The guest firewall prevented a direct target bypass,
and both guests were shut down after the run. The
[selected T076 report](validation/release.md) records exact inputs and cleanup;
the earlier [T037 record](validation/application-compatibility.md) remains
historical evidence for its own binary.

This is a functional duration workload: IPv4 inner/outer path, fixture WG MTU
**1120**, 64/128/512/1000-byte traffic payloads and 10 packets/s per channel per
direction. Fixture QUIC idle timeouts are 15000 ms for WG and 5000 ms for the
generic path; offloads and runtime qlog tracing are disabled. These explicit
fixture overrides are not a change to Remora's 1200-byte default application
ceiling or the production session idle default of 120000 ms.

The test does not establish high-rate WireGuard capacity, WAN reliability,
IPv6 WireGuard qualification, a universal safe WG MTU or native ARM64 behavior.
The generic [reference performance profile](performance.md) is a separate
measurement and must not be presented as WireGuard throughput. If application
or path MTU changes, verify actual bidirectional application traffic again;
a fresh handshake and increasing counters alone are insufficient.
