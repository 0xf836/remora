# ADR 0026: Endpoint configuration and proxy DNS

Date: 2026-10-18. Status: accepted by owner request after independent usability
review. This defines the initial application configuration interface.

## Context

Routine configuration should describe the proxy and UDP destination once,
without making the operator repeat the TLS identity, HTTP authority and standard
MASQUE URI. Public deployment also needs DNS rotation, concrete TLS provisioning
and a clear distinction between public and local listener ports. The owner chose
`proxy = "https://remora.example.com"`, identifying the HTTPS origin. There are
no existing application clients requiring compatibility or migration machinery.

## Options

Expose independent fields for every derived protocol value; accept a URL as
the primary setting; or use one explicit host:port endpoint with narrow overrides.

## Decision

Use `schema_version = 1` with required `proxy` on both roles and client `target`. Require
an HTTPS origin for `proxy`, with an optional nonzero port (default 443) and
bracketed IPv6. Accept only an empty or root path, without credentials, query or
fragment. Normalize equivalent scheme/host forms, the optional root slash and
explicit default port to one authority; omit 443 and retain other ports. Derive
TLS reference identity, HTTP authority and the default HTTPS MASQUE URI from it.
`target` and `connect_address` remain host:port endpoints with explicit ports. `connect_address` optionally overrides only the dial
IP/port. A custom client `uri_template` and server `path_prefix` remain advanced
settings. The server's public authority remains independent of its listeners.
Support only this configuration shape. Reject unknown fields and unsupported
schema versions; provide no compatibility aliases or migration layer.

Reuse the existing bounded resolver: one owned proxy-lookup future, maximum
128 answers and existing worker permits retained through blocking libc work.
Apply `setup_timeout_ms` per lookup. Reject unusable/mismatched-port answers,
normalize/deduplicate and alternate address families, preserving within-family
order. Connect sequentially with existing per-handshake timeout and retry
backoff. Resolve again after exhaustion and on recovery from an established
connection. Stop/permanent rejection drops the only owned future; completions
have no detached callback to transport state. No parallel handshakes, per-packet
tasks, new dependencies, unsafe code or changes to the UDP module.

TLS always verifies the configured proxy identity before credentials are sent.
Disable subject-CN fallback for DNS references and require DNS SAN.
For IP references use BoringSSL's safe IP verification parameter on the new
connection before handshake output; omit DNS SNI. Never verify against a DNS
answer in place of the configured name. Existing target all-answer allowlisting
and auth-before-DNS remain unchanged. Proxy DNS is transport discovery, not a
server-side target permission.

Use static known field/type/range diagnostics and source positions for syntax
errors; never emit raw parser messages, values or unknown field names. Keep
check-config offline and print a success acknowledgement. Public examples use
UDP/443; fixtures remain unprivileged. Document explicit CA trust, renewal and
symlink restrictions, bind capabilities, UDP forwarding and operator-owned
WireGuard routing/MTU.

## Consequences

Routine configs contain one proxy endpoint and one explicit schema version. Sequential
fallback can spend a handshake timeout plus retry delay on an unreachable
candidate; this is not a parallel Happy Eyeballs implementation or an arbitrary
DNS convergence-time guarantee. libc DNS cannot be forcibly cancelled; its
actual workers remain bounded. A full-tunnel VPN must keep all selectable proxy
addresses outside its own route, or pin `connect_address`. TLS/auth/policy
rejections remain permanent and cannot be bypassed by another DNS address.
No automatic trust discovery, provisioning, socket activation, deployment
or VPN management is introduced.
