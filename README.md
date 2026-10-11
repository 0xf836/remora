# Remora

A minimal UDP tunnel over QUIC and HTTP/3 CONNECT-UDP, written in Rust with
Cloudflare quiche. One executable provides client, server and `check-config`.
The Phase 3 MVP forwards opaque UDP datagrams with verified TLS, Bearer
credentials, exact target authorization and separate session relay sockets.

```bash
cargo build --locked --release
./target/release/remora --help
./target/release/remora check-config --config /path/to/client.toml
```

Start a validated configuration with `remora client --config PATH` or
`remora server --config PATH`. Use the [quickstart](specs/001-masque-udp-tunnel/quickstart.md)
for private test fixtures and a reproducible IPv4/IPv6 walkthrough, and the
[build guide](docs/build.md) for the pinned Rust/native prerequisites.

Native Linux x86_64 MVP checks and initial low-rate measurements are recorded in
[validation](docs/validation/mvp.md), [interoperability](docs/validation/interop.md)
and [performance](docs/performance.md). The local
[30-minute WireGuard/generic UDP check](docs/validation/application-compatibility.md)
passed. This is an incremental implementation; full release thresholds,
recovery and native ARM64
qualification remain tracked in [tasks](specs/001-masque-udp-tunnel/tasks.md).
GSO/GRO and runtime filtered qlog are deferred.

Setup packets may be dropped. UDP delivery/order is best effort; there is no
reliable fallback or private fragmentation. Live session generations and
connected sockets prevent internal cross-client routing, but external UDP
cannot distinguish delayed replies after identical tuple reuse or restart.
Already generated/kernel-submitted traffic cannot be recalled by stream close.
See the [wire contract](specs/001-masque-udp-tunnel/contracts/masque-wire.md) and
[ADR-0006](docs/adr/0006-mvp-acceptance-and-ownership.md) for exact guarantees.
