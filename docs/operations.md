# Operating Remora on Linux

Remora runs one client or server role per process, with immutable startup
configuration. The current implementation is tested natively on Linux x86_64.
The selected CUBIC profile has [Phase 8 qualification](validation/release.md);
[ARM64 is deferred outside current requirements](adr/0024-defer-arm64-release-scope.md). See
[build prerequisites](build.md), [operations evidence](validation/operations.md),
[configuration reference](configuration.md)
and [troubleshooting](troubleshooting.md).

## Install and provision

Build with Rust 1.99.0 and the checked-in lockfile, on Linux 6.1+ GNU with the
C/C++/CMake/Clang prerequisites in the build guide:

```bash
cargo build --locked --release
install -D -m 0755 target/release/remora "$HOME/.local/bin/remora"
remora --version
```

Use an ordinary service user; UDP/443 needs the capability described below. Remora does not configure firewall,
routing, a VPN, TUN, WireGuard keys or interfaces. Provide UDP reachability
between roles, access from the server to the explicitly permitted UDP target,
and server certificate/trust material out of band. The tunnel protects the
client-to-server leg; destination UDP confidentiality belongs to the application.

Create separate mode-0700 configuration directories on the role hosts. Copy the
[client](../examples/client.toml) and [server](../examples/server.toml) templates,
then set `proxy = "https://remora.example.com"` on both roles and the client's
`target`. The public name must resolve to the server, unless the client sets
`connect_address`. See [all settings](configuration.md).

Follow the [TLS guide](tls.md) to provision either a public-CA certificate with
an explicit client trust bundle or a private CA with a matching SAN. Files are
relative to the config directory and must be regular files: symlinks are
rejected in every path component. Private inputs deny group/world access
(normally `0600`). The client token must belong to the client process user;
server keys and credentials may belong to that user or root, but the service
still needs read access. Protect all configuration directories against changes
by other users. No verification bypass is provided.

Generate a 256-bit token and its SHA-256 digest of **canonical ASCII text** in
an otherwise private provisioning directory. This command writes files and
prints neither the secret nor its digest:

```bash
umask 077
python3 - <<'PY'
from pathlib import Path
import hashlib, secrets
secret = secrets.token_hex(32)
Path('client.token').write_text(secret + '\n', encoding='ascii')
Path('credentials.toml').write_text(
    '[[identities]]\nid = "operator"\n'
    f'token_sha256 = "{hashlib.sha256(secret.encode("ascii")).hexdigest()}"\n'
    '[[identities.targets]]\nhost = "127.0.0.1"\nport = 51820\n'
    'addresses = ["127.0.0.1/32"]\n')
PY
chmod 0600 client.token credentials.toml
```

Run this once in a fresh provisioning directory. Transfer only `client.token`
to the client and `credentials.toml` to the server through your private channel.
Remove unneeded provisioning copies according to your credential policy. Adapt
the exact target rule before use. The digest hashes the 64 ASCII hex characters,
excluding the optional trailing newline; it does not hash decoded token bytes.
Never put credentials in command arguments, environment overrides, URLs or logs.

Rules require an exact canonical host and UDP port plus explicit allowed numeric
address ranges. All DNS answers must be authorized before one is pinned to a
Session. Case and trailing dots normalize; non-ASCII names, ambiguous numeric
forms and IPv6 zone identifiers are refused. Loopback/private/link-local
addresses need explicit ranges, and unscoped IPv6 link-local is unsupported.
Do not use broad address ranges just to make a failing DNS lookup succeed.
Wildcard host/port rules are unavailable; invalid/empty credentials are refused.

## Validate, start and establish readiness

```bash
remora check-config --config /absolute/private/server.toml
remora check-config --config /absolute/private/client.toml
remora server --config /absolute/private/server.toml >server.status.ndjson 2>server.events.ndjson
# In the client terminal/host:
remora client --config /absolute/private/client.toml >client.status.ndjson 2>client.events.ndjson
```

`check-config` validates files, schema, trust syntax, policy and budgets. It
performs no bind, DNS lookup or peer traffic, and can succeed while the intended
port is occupied. Runtime readiness requires a bound listener and verified
transport/settings. Session acceptance additionally requires authentication,
authorized resolution/socket setup and the successful CONNECT-UDP response.
Only an application response proves observed destination responsiveness. Initial
and recovery-period UDP packets are dropped, so use an application-aware
readiness probe that can safely retry. Remora itself never replays them.

Exit 0 means successful validation or completed graceful stop; exit 2 means
invalid command/configuration, including private-file permissions; exit 1 means
fatal runtime failure, initial permanent rejection or incomplete diagnostic
collection. A post-start permanent rejection latches the mapping against new
setup until controlled restart. Already accepted unrelated flows remain usable.
An unknown command/role does not forward anything. See the fixed error fields
and numeric failure codes in [troubleshooting](troubleshooting.md).

The client `listen` field is required; the example binds loopback. A non-loopback listener requires explicit
`allow_non_loopback = true`; it exposes relay use to applications that can reach
it and must be protected by your host/network policy. IPv4 and IPv6 listeners
are separate; IPv6 uses IPV6_V6ONLY. Wildcard listeners preserve the original
local destination address in replies and distinguish sender/local-destination
pairs. The public server example binds IPv4 wildcard UDP/443; add a separate
IPv6 listener if needed. These instructions do not apply live host networking changes.

## Listening on UDP/443

The public examples use port 443. Remora accepts any nonzero UDP port; 7443 and
other high ports in validation fixtures let tests run without bind capabilities.
TCP/443 is a separate socket and can serve an existing HTTPS site. Another
HTTP/3 or QUIC service already using the same UDP address/443 conflicts with
Remora unless an external deployment component explicitly routes that traffic.

For a system service, install the binary at `/usr/local/bin/remora`, create a
dedicated `remora` user/group, and provision `/etc/remora/server.toml` and its
files for that service. The [example unit](../examples/remora-server.service)
grants only `CAP_NET_BIND_SERVICE` for the privileged port. Install it as
`/etc/systemd/system/remora-server.service`, run `systemd-analyze verify` on it,
validate the config as the service user, then use your normal systemd start and
logging workflow. It uses direct socket binding; Remora does not consume
socket-activation descriptors. Make private files owned by `remora:remora` with
mode `0600` when the service must read them without root access.

For a manual run from the source build, a privileged administrator can grant
the same file capability once:

```bash
sudo setcap cap_net_bind_service=+ep ./target/release/remora
./target/release/remora server --config "$HOME/.config/remora/server.toml"
```

Grant this only to the intended executable. Replacing/reinstalling the binary
can remove file capabilities; a systemd ambient capability avoids that renewal
step. To remove a file capability, use `sudo setcap -r PATH`.

Alternatively, manage UDP/443 forwarding to an unprivileged port using your
existing host/network tooling: keep `proxy = "https://remora.example.com"` on both
roles and set the server `listen = ["0.0.0.0:7443"]`. The client uses the public
port. An HTTP reverse proxy alone does not provide this UDP forwarding. Remora
does not install the forwarding rule or open firewall ports.

## Limits, payload size and lifecycle

Defaults admit 64 connections, 128 total Sessions, 32 per identity and per
connection, 16 concurrent setups, and 256 relay sockets, subject to stricter FD
and byte budgets. Queues hold at most 32 packets per direction/Session and
65536 combined payload bytes; retained slab budgets are 8 MiB application,
4 MiB ciphertext and 2 MiB scratch. Native DATAGRAM queues have separate
count/byte limits. Admission exhaustion produces 503; packet pressure drops
the exact unaccepted datagram with fixed directional counters. These are
bounds, not a throughput or Internet admission guarantee.

Application payload maximum defaults to 1400 bytes, outer cap to 1452. The
actual supported payload can be smaller because of current QUIC DATAGRAM
capacity, framing, verified outer PMTU or the destination path. Outer probes
are distinct from application loss. Oversize messages are never truncated,
privately fragmented, reliably forwarded or retried onto a new association.
A local receive truncation is reported as `size/3` and `socket_truncated`; it
cannot recover omitted bytes or claim an exact original length. Adjust the
application's datagram size/MTU with actual path evidence. For independently
managed WireGuard, use the [WireGuard guide](wireguard.md); 1400-byte payload
capacity is not a universal safe WireGuard interface MTU.

Use the [size-profile table](wireguard.md#choosing-a-size-profile) for matching
endpoint settings, the WG1280 starting profile, WG1360 tuning and rollback. QUIC status includes
`application_payload_lower_bound_min`: the sampled conservative local-send
application bound across connections, with the longest association ID reserved.
It excludes the remote application cap and destination route. Check both roles
and actual full-size bidirectional traffic; setup/revalidation can lower it.

Valid UDP activity in either direction retains a Session for the default
120000 ms. QUIC keepalives do not reset application inactivity. A shorter
`idle_session_ms` is an intentional RFC SHOULD deviation: document sparse-flow
first-packet loss and repeated setup/source-port changes. Application/native
queue residence defaults to 20 ms and generated ciphertext to 100 ms. Close
purges cancellable queued traffic. Previously generated transport output or
kernel/network traffic may complete only on its original association and
cannot be recalled. Identifiable earlier-generation work is never reassigned.

Connected relay sockets, separate live associations and generation checks
prevent active cross-delivery. Opaque external UDP has no Session tag: after
identical relay tuple reuse or process restart, a delayed authorized reply can
be indistinguishable from a new reply. An application restart reusing a live
sender tuple can also be invisible. Application-level freshness/cryptographic
protocols are the mitigation when this matters; there is no absolute provenance
or fixed socket-quarantine promise.

## Compatibility and supported profile

| Path or capability | Verified scope / behavior |
| --- | --- |
| Generic UDP | Native x86_64 IPv4/IPv6 forwarding, empty and supported-size datagrams, separate sender/local-destination associations; applications must tolerate UDP loss and fit the payload ceiling. |
| Independent CONNECT-UDP peer | masque-go v0.6.0 / quic-go v0.63.0 in both roles with normal CA/name validation; 29 current qualification cases cover positive traffic and monitored refusals. |
| Payload carrier | HTTP Datagrams over QUIC DATAGRAM, Context ID 0. Valid reliable DATAGRAM Capsules are parsed/discarded as unsupported carriers; no HTTP/1.1, HTTP/2 or reliable payload fallback. |
| WireGuard | Independently prepared IPv4 peers, fixture MTU 1120, 30-minute low-rate generic/WG traffic; see the [fixture and limits](wireguard.md). No VPN/TUN management. |
| Recovery | Reconnect/re-authorize after connectivity or server interruption, preserve the client listener, never replay outage traffic. GOAWAY drains accepted lower streams and places new work on a replacement connection. |
| Tracing / offloads | Filtered qlog is optional and bounded. GSO/GRO are effective off; deferred `auto` reports `not_implemented`. High-rate active tracing and enabled offloads are unqualified. |
| Platform | Linux GNU x86_64 is the sole current required platform; ARM64 is deferred, experimental and unsupported. |

The independent implementation is a pinned test peer, not a blanket guarantee
for every MASQUE product or extension. See the [wire contract](../specs/001-masque-udp-tunnel/contracts/masque-wire.md)
and [current qualification](validation/release.md) for exact evidence.

## Controlled update, revocation and rollback

Configuration, credential digests, permissions and trust are loaded once.
Prepare complete private candidate files beside their replacements with mode
0600. Replace files with same-filesystem atomic rename, then validate the
candidate config with the candidate executable, including its relative private
file paths, before stopping the old process. A changed credentials file does
**not** update a running process. Until the old process exits, its accepted policy generation remains
in effect. Pause application traffic if the application requires a clean cut.

For processes started by the operator's shell:

```bash
remora check-config --config /absolute/private/server.toml
kill -TERM "$old_server_pid"
wait "$old_server_pid"
remora server --config /absolute/private/server.toml >new.status.ndjson 2>new.events.ndjson &
new_server_pid=$!
```

Use your supervisor's equivalent stop/start procedure when it owns the process;
`wait` above requires a child of the current shell. Save the old PID at initial
startup. Revocation completes at **old process exit plus replacement readiness**.
Observe the new process epoch and bound listener, then verify an allowed
application probe where one remains and rejection of removed credentials/rules.
A policy intentionally allowing no former destination can be ready while
rejecting that old probe. Prior in-flight UDP belongs to the old generation;
do not interpret its arrival as new authorization. Coordinate a client restart
when its permanent-rejection latch needs to be reset.

Rollback is another controlled restart with an explicitly selected saved policy
and trust generation. Review whether it restores a removed credential or grant.
There is no automatic permissive fallback. Keep the candidate/revocation policy
when undoing only an executable change if the old executable supports that
schema. Preserve diagnostic records for both process epochs; external log
retention and access remain the operator's responsibility.

## Migrating to the selected CUBIC build

Configuration remains `schema_version = 1`; there is no database or mandatory
file-format migration. The selected build pins the corrected quiche Git revision
in the [build guide](build.md). Replace the executable through the same controlled
stop/start procedure as policy changes, validate private files first, and record
the executable hash for each process epoch. A rebuilt binary is not identified
by an earlier result merely because its version string matches.

CUBIC remains the default. `bbr2_gcongestion` and `reno` are explicit settings,
but the selected full reference qualification is CUBIC. Do not select BBR2 as an
inferred universal fix for loss; [performance evidence](performance.md) records
both its comparative results and CUBIC's known loss amplification.

The current socket buffer requests are **1048576 bytes for outer QUIC sockets**
and **131072 bytes for application/relay sockets**, each requested for receive
and send. Older implicit defaults were 262144/65536. Explicit overrides stay
explicit; an omitted field adopts the new default after restart. Review memory
and kernel limits when upgrading many processes. Linux effective values can be
doubled/clamped; read `socket_buffers` diagnostics for the actual RX/TX values.
On the selected reference host they were 2097152/262144 bytes per respective
RX/TX buffer. Remora does not change host sysctls. Kernel queue charges are
reported separately from process RSS, and larger buffers do not guarantee WAN
loss tolerance or tail latency. Application/native/ciphertext count and residence
limits are unchanged; kernel waiting precedes the application residence clock.

## Logging and systemd journal

The default **operator profile** writes readable single-line messages to stderr.
Stdout is empty during normal client/server operation. It reports startup,
transport readiness, tunnel acceptance, connection loss, accepted recovery and
shutdown. Healthy steady state is silent: no retry reminders, attempt totals,
periodic metrics or JSON failure arrays. Graceful GOAWAY replacement/retirement is not an outage. Temporary client
CONNECT refusal or setup timeout is reported once, even with live transport.
A later accepted request reports renewed setup without claiming other accepted
flows had failed. A transport handshake alone does not
announce a restored UDP tunnel, and acceptance does not prove destination response.

```toml
[diagnostics]
level = "info"
format = "text"
snapshots = false
```

These are also the defaults when the table is omitted. `warn` hides routine
startup/acceptance/recovery/stop notices; `error` hides warnings too. Final CLI
configuration/runtime errors are always reported. `RUST_LOG` is not used.
Settings are startup-only: validate with `check-config`, then restart the unit.

| Level | Operator output |
|---|---|
| error | Permanent client refusal affecting new setup after prior acceptance; startup/final failures explain whether the process exits |
| warn | One connection-unavailable/lost or client setup-failure notice until acceptance; first observed datagram-size failure per process |
| info | Startup/listener readiness, first secure transport, first accepted tunnel, accepted recovery, orderly resource cleanup |
| debug | Individual retries/state transitions, safe refusal/socket/resource/size details and short process-local correlation IDs |
| trace | Includes debug; currently no additional raw transport stream |

Repeated size warnings are shown once per process, not once per client or
interval; later details require debug/diagnostic collection. Public-server
request refusals and packet/resource faults do not generate per-packet WARN.
Ordinary operator messages are not a complete per-session audit. No correlation
ring is collected in text mode and it cannot establish SC-010 completeness.

systemd receives real syslog priorities when `JOURNAL_STREAM` matches the actual
stderr descriptor. The application omits its duplicate timestamp in that case;
`journalctl` supplies unit/time/process context and its own coloring. For example:

```sh
journalctl -u remora-server.service -n 50
journalctl -u remora-server.service -p warning
```

No ANSI escapes are emitted. Without a matching journal stream, output has a UTC
timestamp and severity. Human lines are <=1024 bytes. They share the existing
bounded nonblocking writer. DEBUG has a separate admission bucket and leaves
16 record slots and 16 KiB for operational messages, including against qlog; each bucket permits 20/s, burst 40.
Rapid flapping can still hit these finite limits. Blocked/broken output cannot
guarantee delivery; shutdown retains a bounded writer wait. No incident history,
per-peer suppression map, reminders, file backend or rotation is added.

### Machine diagnostics and upgrade migration

Existing collectors/benchmarks must explicitly select the **legacy diagnostic
profile**, including periodic snapshots when their measurements need them:

```toml
[diagnostics]
level = "info"
format = "json"
snapshots = true
snapshot_interval_ms = 1000
```

It preserves schema-1 NDJSON events on stderr, full status/metrics on stdout,
mandatory correlated failures independently of ordinary `level`, bounded ring,
coverage counters, ACK/retry and final shutdown collection. `snapshots=false`
in JSON disables healthy periodic dumps, not pending failure status. An interval
>=1000 ms applies to JSON status, not operator state-transition messages.
Text plus `snapshots=true` is rejected, not silently ignored.
Runtime failures retain the fixed machine marker `runtime failed: <typed error>`
on stderr in JSON mode; collectors do not parse human descriptions.

**Compatibility change:** omitted fields now select text/no snapshots. Before
upgrading automated consumers, pin `format="json"` and `snapshots=true`; do not
parse human messages. The repository's generated test fixtures and measurement
runners pin the machine contract. Missing/invalid snapshots invalidate normal
benchmark collection instead of producing empty metrics. Historic results retain
their original binaries/profiles; parser compatibility is not a new performance
qualification.

Profiles are alternatives, not simultaneous outputs. For diagnostic capture,
redirect both JSON streams to an explicitly managed collector. systemd collects
both streams into the same journal if used unchanged; selecting JSON in the unit
will therefore restore verbose status output. Configure collector retention
externally. Remora does not create diagnostic files or manage journal retention.

## Optional filtered qlog

Build `cargo build --locked --release --features qlog` and supply a **fresh empty,
role-owned mode-0700 directory** through `diagnostics.qlog_directory`. A build
without this feature rejects activation. `check-config` checks the directory
without creating files. Reusing a nonempty directory is refused so prior data
cannot silently evade the retention ceiling. Use a new directory per process
and manage archived retention yourself. Parent paths and the host are trusted.

```toml
[diagnostics]
level = "info"
format = "json"
snapshots = true
snapshot_interval_ms = 1000
qlog_directory = "qlog-this-start"
```

Only reconstructed packet numbers/types, lengths, nonnegative timing and
numeric recovery metrics reach `trace-N.ndjson`. This sanitized NDJSON is a
Remora schema-1 metadata stream, not an upstream qlog file. H3 events, headers,
frames, raw data, CIDs, addresses, custom fields and reason text are omitted.
Unsupported/malformed/schema-changing or oversized records disable that trace;
records are limited to 32 KiB input within a 64 KiB input/parser scratch charge.
No keylog is enabled; `SSLKEYLOGFILE` and arbitrary upstream `RUST_LOG` targets
are ignored.

At most eight filters reserve 512 KiB scratch; qlog and events share the
768 KiB/768-record ordinary queue, leaving the 256 KiB/256-record status
reservation intact. The entire diagnostic budget remains 2 MiB. Rate ceilings
are 1000 events and 1 MiB in each monotonic one-second process window; tracing
stops at 16 MiB per connection or 128 MiB per process directory. Churn never
resets directory/rate accounting. Queued/potentially partial disk writes keep
conservative lifetime byte reservations. File/queue/rate/schema stop counts
and output failures appear in bounded status. Disk failure disables optional
tracing and preserves the ordinary status writer. All disk work belongs to
that writer; a slow writer can exhaust optional tracing capacity.

T071 now records scoped native loopback diagnostic overhead and blocked-sink
responsiveness in [performance](performance.md#phase-7-t071-diagnostics-measurement-method--2026-10-13).
Its one-Session/100pps results do not qualify high-rate tracing cost or final
release performance. Safe off suppresses ordinary detail while preserving
mandatory status/counters; a blocked collector still cannot guarantee delivery.

## Reproduce the walkthrough

```bash
cargo build --locked --release --features qlog
./scripts/validate/operations.sh --binary target/release/remora \
  --output /tmp/remora-operations-new
```

The output must not exist. The runner refuses root, copies the executable into
an isolated user install, generates short-lived private test certificates and
credentials, establishes exact bidirectional UDP traffic and collects correlated
failures. It also runs update/rollback, status-writer and qlog regressions. It
retains public evidence and exact hashes; private fixtures and owned children
are cleaned. It does not deploy a service or configure WireGuard/networking.
