# Remora

**UDP tunneling over HTTP/3.**

Remora carries WireGuard and other UDP traffic through an authenticated MASQUE
proxy. Point your application at a local UDP port, and Remora forwards its
traffic to the configured destination through an encrypted QUIC connection.

```text
Application <-> Remora client <== HTTP/3 + QUIC ==> Remora server <-> UDP service
```

## Features

- Preserves UDP payloads and datagram boundaries, including empty datagrams.
- Verifies server identity and enforces client authentication and destination
  allowlists.
- Supports multiple clients and isolated sessions with configurable resource
  limits.
- Reconnects automatically after transient outages while keeping the local
  listener available.
- Supports IPv4 and IPv6, structured JSON diagnostics, and optional filtered qlog
  tracing.

## Build from source

Requires Linux 6.1+ (x86_64, glibc). Install the
[build prerequisites](docs/build.md#pinned-inputs-and-prerequisites), then run:

```bash
cargo build --locked --release
```

See the [build guide](docs/build.md) for container builds and optional features.

## Usage

This example forwards `127.0.0.1:51821` on the client to `127.0.0.1:51820`
on the server. Replace `remora.example.com` with your server's DNS name and
use a certificate for that name. The client resolves it automatically.

### 1. Prepare the configuration

On each host, create a private configuration directory:

```bash
install -d -m 0700 "$HOME/.config/remora"
```

Copy the [server example](examples/server.toml) there as `server.toml` on the
server, and the [client example](examples/client.toml) as `client.toml` on the
client. Keep the files at mode `0600`. Paths inside the configuration are
relative to this directory.

Use the [TLS guide](docs/tls.md) to obtain/provision these files with either
a public CA or a private CA:

| Host | Files |
| --- | --- |
| Server | `server.pem` — certificate chain for `remora.example.com`; `server-key.pem` — private key |
| Client | `ca.pem` — CA certificate used to verify the server |

### 2. Set the proxy and destination

Both example configs use `proxy = "https://remora.example.com"`. On the client:

```toml
[client]
listen = "127.0.0.1:51821"
proxy = "https://remora.example.com"
target = "127.0.0.1:51820"
ca_file = "ca.pem"
token_file = "client.token"
```

The server binds `0.0.0.0:443`; allow incoming **UDP/443** in your firewall.
Your application (e.g. WireGuard) sends to `listen` on the client. Remora connects to `proxy`,
then the server forwards UDP to `target`: here, its own local port `51820`.
Normally, `proxy` is resolved through DNS. For a fixed IP, see
[proxy name and connection address](docs/configuration.md#proxy-name-and-connection-address).

### 3. Authorize the client

Generate the client token and print its SHA-256 digest on the client host:

```bash
umask 077
openssl rand -hex 32 | tr -d '\n' > "$HOME/.config/remora/client.token"
sha256sum "$HOME/.config/remora/client.token"
```

On the server, save this as `credentials.toml` with mode `0600`. Replace
`PASTE_SHA256_HERE` with the digest printed above, keeping the token on the
client:

```toml
[[identities]]
id = "client"
token_sha256 = "PASTE_SHA256_HERE"

[[identities.targets]]
host = "127.0.0.1"
port = 51820
addresses = ["127.0.0.1/32"]
```

This rule permits only the server's local UDP service on port `51820`.
For another destination, update both the client target and this rule.

### 4. Validate and start

On the server (direct UDP/443 binding requires `CAP_NET_BIND_SERVICE`; see the
[service example and manual setup](docs/operations.md#listening-on-udp443)):

```bash
./target/release/remora check-config --config "$HOME/.config/remora/server.toml"
./target/release/remora server --config "$HOME/.config/remora/server.toml"
```

On the client:

```bash
./target/release/remora check-config --config "$HOME/.config/remora/client.toml"
./target/release/remora client --config "$HOME/.config/remora/client.toml"
```

Send application traffic to `127.0.0.1:51821` on the client. For WireGuard,
use that address as the peer's `Endpoint`; choose the interface MTU using the
[WireGuard guide](docs/wireguard.md). The destination service must already be
running on the server.

See the [operations guide](docs/operations.md) for service setup, updates and
credential revocation.

### Datagram handling

UDP delivery remains best effort. Packets received during setup or recovery
are dropped rather than replayed. The default payload limit is 1200 bytes;
the effective limit depends on the path. Oversized packets are dropped and
counted, without truncation or a reliable transport fallback.

The QUIC connection encrypts traffic between the client and server. Encryption
between the server and destination is the application's responsibility.

## Documentation

- [Configuration](docs/configuration.md) — fields, defaults and overrides.
- [TLS](docs/tls.md) — certificates and client trust.
- [Operations](docs/operations.md) — configuration, credentials, limits and updates.
- [Troubleshooting](docs/troubleshooting.md) — diagnostics and failure codes.
- [Performance](docs/performance.md) — measurements, methodology and limitations.
- [Changelog](CHANGELOG.md) — changes and compatibility notes.

## Contributing

Run the native checks before submitting changes:

```bash
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
```

See the [architecture decisions](docs/adr/) and
[validation guide](docs/validation/README.md) for design constraints and
additional test suites.

The [release guide](docs/releasing.md) explains development PRs, generated
release PRs, versioning and binary publication through Release Please.

With Gitleaks available in `PATH`, enable this clone's pre-commit check:

```bash
git config --local core.hooksPath .githooks
./scripts/validate/secrets.sh history
```

## License

[MIT](LICENSE).
