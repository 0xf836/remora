# Configuration and proxy DNS — 2026-10-18

Scope: owner-approved configuration, CLI and operator-documentation usability
changes following an independent read-only audit. Proxy configuration uses an HTTPS origin,
`proxy = "https://host[:port]"`, with port 443 as the default. See
[ADR 0026](../adr/0026-endpoint-configuration.md), the
[public reference](../configuration.md) and the
[updated contract](../../specs/001-masque-udp-tunnel/contracts/cli-config.md).

## Functional and security evidence

The single-format checkpoint native all-target/all-feature run passes **195 tests**. Both ordinary
ignored independent-peer prerequisites are run explicitly: the 29-case
independent protocol matrix and the held-response shutdown case pass. The 100
Python checks, formatting, warnings-denied Clippy and locked default/qlog release
builds pass. No dependencies, lockfiles, unsafe code or UDP syscall code change.

New tests cover:

- Identity/authority/template derivation, explicit-port and bracketed IPv6
  syntax, fixed transport override, independent public/listen ports, required
  proxy/target fields, unknown-field and unsupported-version rejection.
- Safe field/type/range and TOML-position errors, including secret sentinels;
  subcommand help and visible offline check-config acknowledgement.
- Bounded proxy resolution, duplicate removal, family alternation, invalid and
  excessive answers, deadline, endpoint fallback, generation replacement,
  fresh lookup after exhaustion, listener preservation and stop/late completion.
- Process E2E forwarding through system-resolved `localhost`, numeric override
  with DNS identity and literal IP with IP SAN.
- Explicit trust failures for wrong DNS/IP SAN, a DNS SAN containing an IP text
  used as an IP reference, and a CA-signed matching CN without SAN.

Proxy DNS selects only the transport address; configured TLS identity remains
unchanged. Blocking resolver jobs retain their existing permits after async
cancellation. Existing target authentication-before-DNS and all-answer CIDR
checks remain in force. Sequential fallback is not parallel Happy Eyeballs
and does not establish an arbitrary DNS convergence-time bound.

The wider endpoint/DNS implementation received the requested independent
read-only usability and DNS/TLS/config security review. DNS subject-CN fallback
was disabled; deferred offload `auto` and the supported custom-template subset
are described accurately. The follow-up parser cleanup changes no reactor,
TLS, policy or UDP code and adds no dependencies. The only application format
is `schema_version = 1` with proxy/target endpoints; all fixtures and runner
adapters use it. No compatibility parser branches, aliases or migration guide
are part of the interface.

The earlier endpoint/DNS source snapshot passed four 30-second ASan campaigns:
MASQUE 3,525,081 executions, policy 4,457,177, ancillary 20,288,239 and lifecycle
843. Each finished in 31 seconds. Those results remain bound to that earlier
source/executable; they are not rerun or transferred to the current parser.
Current configuration malformed input is covered by the native property suite.

## Operator documentation checks

The private-CA OpenSSL recipe executes successfully in a fresh temporary
provisioning directory. Both exact public examples pass check-config
with that provisioned material, including port 443 without binding. The systemd
unit passes syntax verification with its installed binary path substituted by
the tested local executable. The initial sandbox systemd/UDP failures are
prerequisite failures; the unrestricted local checks pass. Relative-link
validation covers the new guides and updated public documents.

These checks do not install a service, grant host capabilities, issue a public
CA certificate, change WireGuard/routes/firewalls or verify a remote deployment.
Local process tests use owned ephemeral unprivileged listeners. No new live
UDP/443 deployment or full WireGuard qualification is claimed.

## Single-format checkpoint executable and evidence

Checkpoint qlog executable SHA-256:
`b896031402f0f144510fdc41c0c7c42a18939870c4edc9a27be461901a888769`.
195 native tests, 100 Python checks, the explicit 29-case independent matrix,
held-response shutdown, locked default/qlog builds, Clippy, format and public
example/TLS/unit checks pass. Updated Python runners compile. Checkpoint raw logs,
independent results, source/binary hashes and document checks are retained in
`benchmarks/results/single-config-20261018/`. No new traffic-path change or
performance claim is introduced by the startup parser and harness cleanup.

## Target diagnostic correction

The later read-only review found that prohibited client targets exposed the
internal field name `target_host`. The loader now maps that failure to
`client.target` with a static address-restriction hint, preserving the Range
classification and withholding input values. The regression fails before and
passes after the fix for six IPv4/IPv6 cases, including exact CLI diagnostics
and exit code 2. All 18 config integration tests and nine config unit/property
tests pass, along with format, warnings-denied Clippy and locked default/qlog
release builds. This diagnostic-only correction does not rerun the full suite,
ASan, interop or performance checks; their earlier provenance remains intact.
Diagnostic-correction checkpoint qlog SHA-256: `392498d369883e2fa5b45adecba5a7e35f07847cf51f1c018cb032a34f877aa4`.
Raw evidence: `benchmarks/results/target-diagnostic-20261018/`.

## HTTPS origin configuration

The initial proxy interface is an HTTPS origin with default port 443. Bare
host:port values are rejected. Root slash, scheme/host case and an explicit 443
normalize to one authority; nondefault ports remain explicit. Credentials,
non-root paths, queries, fragments, scoped/unbracketed IPv6 and malformed ports
are rejected without echoing input. Target and numeric transport override keep
explicit-port endpoint syntax. The three setting tables expose Required as a
separate column.

199 native tests, 100 Python checks, 29 independent protocol cases and the
independent shutdown case pass. New unit/property and real E2E coverage verify
normalization and trust-preserving exchange between equivalent origin spellings.
Default/qlog builds, Clippy, format, both public examples with TLS material,
service-unit syntax and 151 relative links pass. No reactor, TLS, policy or UDP
implementation changes are needed; no new performance/ASan run is claimed.
Current qlog executable SHA-256: `88f1870fc62a37b4221065be5c68be302fe449c4c7698253933a19e1ffae5070`.
Raw evidence: `benchmarks/results/https-origin-20261018/`.

## Earlier prospective performance comparison

The following comparison belongs to the earlier endpoint/DNS executable and
its frozen source/runner hashes. The current executable above is different;
these measurements are retained as historical evidence and are not transferred.

The measured qlog executable SHA-256 is
`ca9c3d41f438d2b3afd4c318410a0a17710424ecd7db3e95f85df5af8caaa774`.
The control is the preceding T082 qlog executable
`a4b10cf925fea1739595c6b30d2d46a4535ebf9dce0d5398431cca9c77da557f`.
The [frozen policy](../../benchmarks/policies/phase11-endpoints.json) SHA-256 is
`07df6c9f1b466bdaa3809de39cce56c67c8c8e14d48671654f2e4b9cd2efa8ca`.

Before measurements the policy binds source/dependency, runner/profile/tool and
both binary hashes, three alternating repetitions, and a 1800-second wall
budget. The affected steady-state workloads are simultaneous small packets
(1 session, 128 bytes, 64849 packets/s/direction, 15 seconds) and resource load
(32 sessions, 1200 bytes, 52160 packets/s/direction, 60 seconds), each with
three-second warmup and direct/control/candidate members.

Existing per-repeat bounds remain unchanged: delivery >=99% of control, added
endpoint loss <=0.1 percentage point, p95 RTT <=control+1 ms, process/owner CPU
<=control*1.05+0.01 core and RSS <=max(control*1.1, control+1 MiB). Direct endpoint
loss must independently be <=0.1%; applicable absolute resource bounds remain
500 Mbit/s per direction, <=2 cores and <=128 MiB per role. No failed repetition
may be averaged away and no thresholds may be changed after collection starts.

All **18 members / 6 per-repeat comparisons pass** in 780.60 seconds
(13m01s including cleanup), within the 1800-second budget. Every candidate
resource direction delivers 500.736 Mbit/s. Candidate endpoint loss is zero in
all six members. Maximum p95 RTT increase versus adjacent control is 0.030 ms;
process CPU differences range from -2.86% to +2.35%, within the unchanged
ratio-plus-margin bound. Maximum process/owner CPU is 0.910608/0.909440 core
and peak RSS is 12,693,504 bytes (12.11 MiB) per role. Each repeat independently
passes; no criterion was changed and no failed member was averaged away.
The measured endpoint/DNS increment completed T083, T084 and T085.
This focused regression is not a new medium/capacity search, full T076/T077,
WireGuard, WAN, ARM64 or release qualification. Historical qualification remains
bound to its original executable. No performance result transfers automatically
to the new binary.

Local raw output is retained in ignored
`benchmarks/results/config-v2-20261018/`: verification logs/summary, source and
binary snapshots, ASan results, frozen policy/driver and comparison members.
Generated private fixture material remains excluded from Git.
