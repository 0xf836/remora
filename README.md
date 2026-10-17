# Remora

A UDP tunnel over QUIC and HTTP/3 CONNECT-UDP, written in Rust with Cloudflare
quiche. One executable provides client, server and `check-config`, with verified
TLS, Bearer credentials, explicit target authorization and bounded sessions.

```bash
cargo build --locked --release
./target/release/remora --help
./target/release/remora check-config --config /absolute/private/client.toml
```

Start a configured role with `remora client --config PATH` or
`remora server --config PATH`. The [quickstart](specs/001-masque-udp-tunnel/quickstart.md)
provides isolated test fixtures; the [build guide](docs/build.md) explains the
pinned toolchain and quiche fork. See [operations](docs/operations.md) for trust,
credentials, updates/revocation, compatibility and filtered diagnostic tracing,
and [WireGuard](docs/wireguard.md) for independently managed peers.

The selected **native Linux GNU x86_64 CUBIC profile** passed functional,
security, independent interoperability, 30-minute generic/WireGuard and bounded
reference performance qualification. [Results and limits](docs/performance.md)
identify the exact executable, workload and remaining risks. Linux GNU x86_64
is the sole current required platform; [ARM64 is deferred](docs/adr/0024-defer-arm64-release-scope.md)
outside this scope. Its prepared tooling is experimental. GSO/GRO remain effective off.
The [changelog](CHANGELOG.md) describes the current unreleased implementation;
these local results do not assert a published release or deployment.

Setup and outage packets can be dropped. UDP delivery/order is best effort;
there is no reliable payload fallback or private fragmentation. Applications
must fit the current payload ceiling. Remora does not manage VPN/TUN state,
WireGuard keys, routing or firewalls. Live sessions reject identifiable stale
work, but external UDP cannot establish freshness after identical tuple reuse.
Already generated or kernel-submitted traffic cannot be recalled by stream close.
See the [wire contract](specs/001-masque-udp-tunnel/contracts/masque-wire.md).
