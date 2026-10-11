# Independently prepared WireGuard validation

WireGuard is an opaque UDP application to Remora. The operator owns peers,
keys, routes, TUN interfaces and firewall configuration. Remora does not
create, update or inspect private keys, install routes, or change VPN state.

For the T037 qualification runner, prepare two exclusively used peers
and a permitted server-side WireGuard UDP endpoint. Configure the local
WireGuard endpoint to the Remora client's application listener; its CONNECT-UDP
target is the approved server endpoint. Provide a private fixture describing
how to reach each test endpoint, read-only handshake/transfer observations,
the two Remora status collectors and the independently managed traffic path.
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
./scripts/validate/libvirt-lab.sh test \
  --seconds 1800 --output /tmp/remora-wg-qualification-new
```

This is an explicit opt-in qualification command. It reuses a verified shared
local image cache and removes its disposable VM/network/storage resources on
completion, failure or supported interruption. Ordinary build/unit/CI checks
do not start VM. Persistent guests require the separate manual `create` mode.
For externally prepared peers use `wireguard.sh --fixture ... --start-remora`
directly; that runner owns only its test processes and does not remove guest
or peer state.

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

T037's local two-guest 30-minute qualification passed for the recorded x86_64
profile, with exact delivery on both WG/generic channels in both directions
and verified disposable-lab cleanup. See the
[run record](validation/application-compatibility.md) and
[application compatibility](validation/application-compatibility.md) for
verified coverage and limitations.
