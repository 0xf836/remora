# Configuration

Remora reads one TOML file at startup. Choose a client or server role; changes
require a restart. Paths are relative to the configuration file's directory.
Use `remora check-config --config PATH` to validate files and settings without
binding sockets, resolving DNS or contacting the peer.

## Client

```toml
schema_version = 1

[client]
listen = "127.0.0.1:51821"
proxy = "https://remora.example.com"
target = "127.0.0.1:51820"
ca_file = "ca.pem"
token_file = "client.token"
```

Application (e.g. WireGuard) sends UDP to `listen` on the client host. Remora connects to
the server at `proxy`, which forwards the traffic to `target`. In this example,
the destination is UDP/51820 on the Remora server itself. Replies travel back
through the same tunnel.

| Field | Required | Meaning |
| --- | --- | --- |
| `listen` | Yes | Local numeric IP and nonzero UDP port. Applications send here. |
| `proxy` | Yes | HTTPS address of the Remora server. Port 443 is the default; use `https://remora.example.com:7443` for another port. DNS, IPv4 and bracketed IPv6 are accepted. |
| `target` | Yes | UDP destination `host:port`, reached **from the server**. `127.0.0.1` means the server itself; `10.0.0.5:9000` would reach another service in its network. The server must explicitly authorize the destination. DNS and bracketed IPv6 are accepted. |
| `ca_file` | Yes | PEM trust bundle. Only these certificates are trusted; system roots are not loaded implicitly. See [TLS](tls.md). |
| `token_file` | Yes | Private file containing 64 lowercase hex characters, with one optional final LF. |
| `allow_non_loopback` | No | Default `false`. Set `true` explicitly to expose a non-loopback local listener. Protect access with host/network policy. |
| `connect_address` | No | Fixed numeric IP:port used instead of resolving the host in `proxy`. Normally omitted. The server name used for TLS verification still comes from `proxy`. |
| `uri_template` | No | Absolute HTTPS URI template for another MASQUE deployment. Defaults to `<proxy>/.well-known/masque/udp/{target_host}/{target_port}/`. Its literal authority must match the canonical proxy authority (omit port 443). |

### Proxy name and connection address

For normal setup, set `proxy` and omit `connect_address`. Remora resolves the
proxy's hostname through DNS, connects using QUIC/UDP and verifies the server's
certificate against that hostname. HTTPS uses port 443 by default;
`listen`, `target` and `connect_address` always include an explicit port.

To reach the same service at a specific IP, for example while testing a new
server before updating DNS:

```toml
proxy = "https://remora.example.com"
connect_address = "203.0.113.10:443"
```

This connects to `203.0.113.10:443` while verifying the certificate for
`remora.example.com`. An override can also use a private IP or a different port
when reaching the same service through another network path. It stays fixed on
reconnect: update it if the server's address changes.

**SNI** is the server name the client sends when starting TLS. Remora sets it
automatically from the hostname in `proxy`; there is no separate SNI option.
Setting `connect_address` leaves that name unchanged. If `proxy` contains a
literal IP instead, no DNS SNI is sent and the certificate must cover that IP
(IP SAN). See [TLS certificates and trust](tls.md).

### DNS and recovery

For a hostname proxy without `connect_address`, Remora resolves the name at
startup and again after an established connection fails. It tries IPv4/IPv6
addresses one at a time, alternating families, with handshake timeouts and retry
delays. Exhausting the address list starts a fresh lookup. A wrong certificate
remains a permanent trust failure; another address never bypasses verification.

Each lookup has `recovery.setup_timeout_ms` to complete and accepts at most 128
answers. At most `limits.max_pending_setup` system lookups run at once. A lookup
that times out keeps its slot until the underlying operating-system call
finishes. Stopping the client cancels pending connection attempts.

### URL format and custom routes

`proxy` is an HTTPS origin: the scheme, host and optional port. DNS names are
normalized to lowercase, with a trailing dot removed; Unicode names, scoped
IPv6 and ambiguous numeric forms are rejected. A root trailing slash is
accepted; credentials, non-root paths, queries and fragments are rejected.
`https://host`, `https://host/` and `https://host:443` select the same origin.
The normalized host and port form the HTTP authority sent in requests. Port
443 is omitted from that authority and the generated URI; other ports are
retained. Examples include `https://203.0.113.10` and
`https://[2001:db8::1]:7443`. The default port follows the
[HTTPS URI scheme](https://www.rfc-editor.org/rfc/rfc9110.html#section-4.2.2).

Leave `uri_template` unset unless the proxy uses a nonstandard MASQUE route.
Custom templates must contain both `target_host` and `target_port`
variables using the supported RFC 6570 subset. The client supports path and
query expansion; Remora's server accepts the path form described below, so a
query template is for a compatible external proxy. See the
[wire contract](../specs/001-masque-udp-tunnel/contracts/masque-wire.md) for the exact subset.

## Server

```toml
schema_version = 1

[server]
listen = ["0.0.0.0:443"]
proxy = "https://remora.example.com"
certificate_file = "server.pem"
private_key_file = "server-key.pem"
credentials_file = "credentials.toml"
```

| Field | Required | Meaning |
| --- | --- | --- |
| `listen` | Yes | Array of 1–16 distinct numeric IP:port listeners. Ports must be nonzero. IPv4 and IPv6 binds are separate (`IPV6_V6ONLY`). |
| `proxy` | Yes | Public HTTPS address of this service; use the same value in the client configuration. Port 443 is the default. The local bind address and port are set separately by `listen`; this field causes no server-side DNS lookup. |
| `certificate_file` | Yes | PEM chain, leaf first, then intermediates. The leaf must match the private key. |
| `private_key_file` | Yes | Private PEM key. Encrypted keys are rejected without prompting. |
| `credentials_file` | Yes | Private TOML file of identities and exact target permissions. |
| `path_prefix` | No | Default `/.well-known/masque/udp/`. Must start/end with `/`; no query, fragment, whitespace, backslash or template braces. Requests append `{target_host}/{target_port}/`. |

For external UDP/443 forwarding to an unprivileged listener, use
`proxy = "https://remora.example.com"` and `listen = ["0.0.0.0:7443"]`. No HTTP
reverse proxy or socket activation is assumed. Direct UDP/443 requires a bind
capability; see [running on port 443](operations.md#listening-on-udp443).

## Credentials and file handling

```toml
[[identities]]
id = "client"
token_sha256 = "<SHA-256 of the client's 64 ASCII token characters>"

[[identities.targets]]
host = "127.0.0.1"
port = 51820
addresses = ["127.0.0.1/32"]
```

`host` and `port` select the exact permitted destination. `addresses` separately
restricts the numeric results: **every** DNS answer must fall within the allowed
CIDRs before a target is pinned to its request. These checks also apply to
literal IP targets. No wildcard names or ports. Loopback/private addresses need
explicit permission. An empty identity list is invalid; the limits are 128
identities, 128 target rules per identity and 128 CIDRs per rule. IDs are 1–64
ASCII letters/digits or `_-.`; IDs and token digests must be unique.

Hash canonical ASCII token text, excluding the optional LF, rather than the
32 decoded bytes. See [token provisioning](operations.md#install-and-provision)
for working commands.

Config files are at most 256 KiB, credentials 4 MiB and each PEM input 1 MiB.
All inputs must be regular files; symlinks are rejected at **every path
component**. Without an extended POSIX access ACL, the client token must be
owned by the process user with no group/world permissions. Server keys and
credentials may instead be root-owned under the same mode restriction and must
be readable by the service. Modes `0400` and `0600` are usual.

Linux POSIX access ACLs are supported for all three private inputs. The owner
must be the process UID or root. Effective access by every group, unrelated
named user, and `other` must be zero; named root and the process UID are trusted.
The ACL mask limits named-user and group entries, so a nonzero group mode field
can be safe. A root-owned file delegated to a non-root process must grant that
UID **read only**, after applying the mask. This includes the client token;
root ownership alone does not authorize an ordinary client token.

For example, a root-owned credential with the following ACL has mode `0440`
and is accepted for a service running as the indicated UID:

```text
user::r--
user:<service UID>:r--
group::---
mask::r--
other::---
```

This supports the ACL form of systemd `LoadCredential=` without copying a
credential to change its permissions. Point the TOML file fields at the
credential files visible **inside the service**, then run `check-config` with
the same service identity and credential setup. Neither a `/run/credentials/`
path nor `$CREDENTIALS_DIRECTORY` bypasses validation. Plain group-readable
`0440`/`0640` files still fail. Do not `chmod` a credential to work around an
ACL refusal: changing mode can change its ACL mask.

ACL inspection uses the opened file descriptor and fails closed on read/parse
errors. An absent ACL or Linux's explicit unsupported-POSIX-ACL result uses
the strict ordinary-file rules. Extended ACLs require readable kernel procfs
metadata at `/proc/sys/kernel/overflowuid`; an owner matching that overflow UID
or an unrepresentable named UID/GID is rejected to avoid ambiguous ownership.
The supported model is Linux POSIX access ACLs, not NFSv4 ACLs. See
[ADR 0029](adr/0029-private-file-acls.md) for bounds and namespace assumptions.

For a disposable systemd integration check, run the following as the non-root
service user, with system-manager authorization available to `sudo -n`:

```bash
mkdir -p benchmarks/results
python3 scripts/validate/systemd_credentials.py \
  --binary target/release/remora \
  --output benchmarks/results/systemd-credentials-fixed --sudo
```

The runner uses generated test secrets and a bounded transient service. It
requires actual root-owned `0440` credentials with a named ACL for that user;
an ownership fallback is reported as failure of this specific gate. Results
include systemd/kernel versions and executable provenance, without secret
contents. Use a fresh output directory for every run. To check a pre-fix binary,
provide its path and `--expect rejected`. This does not modify the real service.

CA bundles and certificate chains are public material;
protect their integrity and directories. Use atomic replacement and restart
for renewal or revocation. There is no live reload.

## Tuning

Optional tables below apply to both roles. Omitted fields take these defaults;
minimal configs do not need to repeat them. Byte limits count bytes, durations
are milliseconds, and counts are integers. Positive counts/bytes must fit the
platform size type; all derived arithmetic and the process FD budget are
checked before startup. Timeouts must be positive and representable.

| Section/key | Required | Default | Constraint / behavior |
|-------------|----------|---------|-----------------------|
| limits.max_connections | No | 64 | Active plus handshaking; no allocation beyond ceiling |
| limits.max_sessions | No | 128 | Active plus pending, all identities |
| limits.max_sessions_per_identity | No | 32 | <= global; admission rejects on exhaustion |
| limits.max_sessions_per_connection | No | 32 | <= global; separate HTTP streams |
| limits.max_pending_setup | No | 16 | Includes DNS and authentication/authorization setup |
| limits.max_relay_sockets | No | 256 | Active plus bounded cleanup relay sockets; admission rejects at ceiling |
| limits.session_queue_packets | No | 32 | Per direction/session, reject newest on full |
| limits.session_queue_bytes | No | 65536 | Both directions/session combined; checked before allocation |
| limits.application_pool_bytes | No | 8388608 | Global retained slabs/queue ownership, not only lengths |
| limits.ciphertext_pool_bytes | No | 4194304 | Includes all generated unsent QUIC packets |
| limits.scratch_pool_bytes | No | 2097152 | Shared syscall/GRO scratch, independent of session count |
| limits.quiche_dgram_packets | No | 64 | Per connection/direction, explicit enable_dgram counts |
| limits.connection_dgram_bytes | No | 262144 | Send+receive bytes cap including internal queued ownership |
| limits.idle_session_ms | No | 120000 | Default two-minute UDP association inactivity timeout; positive finite; close stream and socket together; shorter override is a documented RFC SHOULD deviation |
| limits.application_residence_ms | No | 20 | Includes native outgoing quiche queue; conservative purge-all at earliest queued expiry |
| limits.ciphertext_residence_ms | No | 100 | Stuck generated output fails its connection, not recreated |
| io.mode | No | mmsg | mmsg or single |
| io.batch_max | No | 32 | Batch count, range 1..64 |
| io.gso / io.gro | No | off / off | off or auto; outer sockets only; auto currently uses ordinary I/O with effective off / not_implemented |
| io.outer_socket_buffer_bytes | No | 1048576 | Requested outer RX/TX kernel buffers; report effective values/clamps; explicit 262144 retains the former default; include per-socket kernel ceilings/counts separately from process RSS |
| io.relay_socket_buffer_bytes | No | 131072 | Requested RX/TX buffers for client application listener and server relay sockets; report effective values/clamps; explicit 65536 retains the former default |
| transport.cc | No | cubic | cubic, bbr2_gcongestion, reno; no generic bbr alias exposed |
| transport.pacing | No | timerfd | User-space deadlines; no off mode |
| transport.outer_udp_max_bytes | No | 1452 | >=1200; PMTU probes and peer/path limits still apply |
| transport.application_max_bytes | No | 1400 | Actual limit may be smaller; empty payload supported |
| transport.idle_timeout_ms | No | 5000 | Finite liveness profile; keepalive when active flows otherwise idle |
| recovery.keepalive_interval_ms | No | 2000 | Active-flow otherwise-idle QUIC probe, normally paced |
| recovery.peer_progress_timeout_ms | No | 10000 | Watchdog based on authenticated peer progress, separate from quiche idle rules |
| recovery.setup_timeout_ms | No | 5000 | Bounded proxy DNS, handshake and request setup |
| recovery.retry_initial_ms / retry_max_ms | No | 100 / 2000 | Validate 100 <= initial <= max <=2000; scheduling floor plus uniform capped exponential jitter, total delay <=max; <=10 attempts/s/process, one handshake in flight |
| diagnostics.level | No | info | error/warn/info/debug/trace; same redaction at every level |
| diagnostics.format | No | text | text: human operator profile; json: legacy structured diagnostics |
| diagnostics.snapshots | No | false | JSON only: true enables periodic metrics; false retains mandatory failure status in JSON. Text has no status stream |
| diagnostics.snapshot_interval_ms | No | 1000 | Minimum 1000; JSON status cadence; operator lifecycle events do not wait for this interval |
| diagnostics.qlog_directory | No | absent | Disabled by default; qlog build feature plus fresh empty owner-0700 directory; filtered NDJSON only, never raw H3 |


For operator-selected transport/WireGuard sizes, use the
[size-profile table](wireguard.md#choosing-a-size-profile). Defaults are now
1452/1400. Configure both endpoints consistently and validate the path.
Existing explicit 1350/1200 values retain the previous profile. The pinned backend's smaller-peer probing
limitation is documented there; raising a cap does not fix it.

Additional coupled constraints:

- `max_sessions_per_identity` and `max_sessions_per_connection` cannot exceed
  `max_sessions`.
- `1 <= application_max_bytes <= 65527` and
  `1200 <= outer_udp_max_bytes <= 65527`.
- `connection_dgram_bytes` must cover
  `2 * quiche_dgram_packets * outer_udp_max_bytes`.
- Application and ciphertext pools must each hold their maximum configured
  message; scratch must cover `batch_max * max(outer, application)` bytes.
- Socket buffer requests must fit a positive signed 32-bit value. Linux may
  clamp them; status reports the effective values.
- `100 <= retry_initial_ms <= retry_max_ms <= 2000` and
  `snapshot_interval_ms >= 1000`.

The default 120000 ms association inactivity timeout follows RFC 9298. A shorter
override can increase repeated setup, source-port changes and first-packet loss;
record the deployment reason. QUIC keepalives do not refresh this application
inactivity timer. See [datagram and lifecycle limits](operations.md#limits-payload-size-and-lifecycle).
