# WireGuard over Remora

WireGuard is an opaque UDP application to Remora. The operator owns peers,
keys, routes, TUN interfaces and firewall configuration. Remora does not
create, update or inspect private keys, install routes, or change VPN state.

## Connect an existing peer

Start with working WireGuard peers and a server listening on UDP/51820. With
the [example configs](../examples/client.toml), the Remora client binds local
UDP/51821 and forwards to `127.0.0.1:51820` **on the Remora server**. Allow that
exact destination in `credentials.toml` and establish Remora using the
[quickstart](../README.md#usage).

Change the existing client's WireGuard peer endpoint to the local Remora port:

```ini
[Interface]
# Keep your existing keys and addresses.
MTU = 1120

[Peer]
# Keep your existing server public key and AllowedIPs.
Endpoint = 127.0.0.1:51821
PersistentKeepalive = 25
```

These are edits to an existing WireGuard configuration, not a complete peer
file. Remora supplies no keys, interfaces or routes. `PersistentKeepalive` is
optional WireGuard behavior and is separate from Remora's QUIC keepalive.
The server-side WireGuard endpoint must already be reachable from Remora.

`1120` is a conservative initial interface MTU for the current 1200-byte
Remora payload profile, also used by the local validation setup. WireGuard
data adds a 16-byte transport header and 16-byte authentication tag, and may
pad the inner packet to a 16-byte boundary: 1120 leaves room within 1200.
The UDP/IP headers outside WireGuard are not part of the payload that Remora
carries. See the [WireGuard transport format](https://www.wireguard.com/protocol/#subsequent-messages-exchange-of-data-packets). A smaller effective tunnel/path ceiling can require a lower MTU;
verify your route and application packets rather than treating 1120 as a
universal guarantee. IPv6 inner traffic also has its own minimum-MTU
requirements; this starting profile is for the documented IPv4 inner setup.

For a full-tunnel WireGuard configuration, keep the Remora proxy's transport
route **outside WireGuard** or the tunnel will try to carry its own connection.
Use your existing route manager or `wg-quick` policy routing. A proxy with several
DNS addresses needs correct routing for every address that may be selected;
`connect_address` can pin the transport while retaining certificate verification.
Remora does not create route exceptions or update them when DNS changes.

After starting both roles, generate application traffic in both directions over
the inner WireGuard addresses. Check `wg show INTERFACE latest-handshakes` and
`wg show INTERFACE transfer`, then test actual application data (for example,
ICMP to each configured inner IPv4 address if permitted). Confirm Remora's
application counters increase in both directions. A handshake alone does not
prove bidirectional data or the chosen MTU. First packets during Remora setup
or recovery are dropped; WireGuard's retries supply fresh packets.

## Maintainer qualification

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
