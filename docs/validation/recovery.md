# Phase 6 recovery and shutdown evidence

Date: 2026-10-12; review correction 2026-10-13. Scope: T058–T063, Linux
x86_64 incremental validation. Latest corrected-executable evidence is in the
review-correction section; preceding results retain their pre-review scope.

## Behavior and security assessment

The single reactor owns explicit primary/draining generations, finite retry
and setup schedules, and a persistent application listener. Connection
replacement creates a new connected outer socket and repeats configured
CA/name/ALPN/settings checks. Every replacement request repeats Bearer
authentication and default-deny target authorization. Old application/native
queues, pending setup callbacks and unsent ciphertext are discarded; outage
payloads are never reassigned or replayed. Accepted lower-ID GOAWAY flows
remain on their old bounded draining connection; both generations debit
connection/FD budgets. With max_connections=1 replacement waits rather than
evicting an accepted flow or exceeding the limit. Validated NAT rebinding
retains its original generation. See [ADR-0010](../adr/0010-client-recovery-and-draining.md).

This implementation security assessment checked trust configuration reuse,
no early data/downgrade, permanent rejection latching despite flow cleanup or
new source tuples, generation cancellation, original target monitoring and
bounded sockets/attempts/queues. Native renewed-rejection tests require exact
authentication/401, authorization/403, trust/0 and capability/0; endpoint
monitoring continues after refusal. New fixed numeric diagnostics contain no
credentials, source tuples, target names or payloads. No dependency or unsafe
code is added; Linux syscall ownership stays in the existing UDP module.
This is implementation assessment and automated evidence; an independent
review was not invoked for this increment.

## Executables and measurement boundaries

Pre-review release executable SHA-256:
`fe36ae3958b53bbd875ad052fb15e7f9c59b35cecd0b20f6775257a6b12118ef`.
Pinned Go peer SHA-256:
`87cb121f728842bdfc473e4a22a4c955c3dd44f9f0634400b84352716d383113`
(quic-go v0.63.0, masque-go v0.6.0, Go 1.27.2; unchanged module sums).
Root process tests use their separately built debug executable; namespace
recovery and independent interoperability use the exact release hash above.
The shutdown runner records its debug process test logs and peer hash.

The recovery runner creates fresh anonymous user/network namespaces, owned
veths and a 1Gbit/s, 20ms RTT reference shape; no host/VPS routes or services
are changed. Each case has a fresh CA/credential and target echo endpoint.
At 10 packets/s, bounded 128-byte test tags carry sequence, common monotonic
time and SHA-256. The actual destination reports every offered/received tag.
The listener socket inode is compared before/after the fault.

Fault onset is recorded immediately before dropping packets, stopping the
server or replacing the client address. Restart downtime includes process
stop, and restored availability starts only after the new UDP socket is bound.
Blackhole restoration starts after both qdiscs are restored; the long case
lasts 30 seconds. Fresh successful replies must be sent after restoration
and arrive within 10 seconds; observable Recovering must occur within
15 seconds of fault onset. Test monotonic/UTC anchors and clock drift are
retained. Address replacement removes the original client address and adds
a new route/address in owned namespaces. No failure-only throughput assumption
stands in for endpoint monitoring.

Native QUIC may declare failure around its 5000ms idle deadline before the
10000ms watchdog. Deterministic tests separately withhold authenticated
progress while generating PTO/probe output and verify watchdog expiry.
The nine-second idle case sends actual keepalive probes without changing
application Session inactivity. A separate process case uses an intentional
2500ms Session idle override to prove UDP inactivity still expires; production
default remains 120000ms. This test-only deviation does not change the profile.

## Pre-review automated checks

- Locked release build, formatting and all-target/all-feature Clippy pass.
- Full root Rust suite: 142 passed, 2 ignored. The 44 library tests include
  seven recovery owner/real-QUIC/fake-clock regressions. The 14 E2E tests
  include IPv4/IPv6 ten-second restart, persistent listener/no replay, idle
  expiry and fatal initial HTTP 401/code-1/listener-release regression.
- Separate traffic package: 11 tests and Clippy pass.
- Python validator suite: 38 passed, including bounded probe tags/integrity
  and partial-final-NDJSON handling. Shell syntax and Python compilation pass.
- Pinned Go fixture builds and passes go vet. Pre-review independent interop matrix:
  29/29 passes (IPv4/IPv6 both roles plus exact monitored negative causes).
- Signal matrix: SIGINT/SIGTERM during Starting, Active, Recovering and held
  request setup, eight process cases. The Go held-response fixture validates
  authentication/target then deliberately withholds 200; the ignored Rust
  setup test is executed explicitly by this runner. Listener reuse, process
  exit, fixed Stopped snapshot and zero Session/FD reservations are checked.
  Owned child process groups are reaped. Both signals share the <=5s cleanup/
  status-writer budget; no second DNS worker wait is added.

Root ignored expensive overload qualification remains separate later work.
Native sockets required execution outside the restricted command sandbox;
sandbox EPERM in an initial traffic test was not counted as a pass.

## Retained failures and corrections

The initial pre-change reverse benchmark lost collector validity because the
runner omitted reverse rate when sizing its bounded receive table. Corrected
`--reverse-pps` and recollected all 30 valid reference rows before production
edits. Original invalid rows remain in `phase6-prechange`; accepted reference
is `phase6-prechange-valid` with frozen manifests/policy in `phase6-reference`.

An early recovery policy test constructed an internally inconsistent numeric
allowlist, which correctly failed configuration validation. The corrected
fixture authorizes a different port on the monitored host and requires exact
403/no target delivery. Original partial run remains `phase6-recovery-native`.

The missing-capability Go injection waited for proxy SETTINGS although the
proxy could reject it first. The fixture now emits its DATAGRAM-disabled
SETTINGS without that dependency; the runner requires exact capability/0 and
zero actual target traffic. Original failed run remains `phase6-interop-final`.

`phase6-interop-verified` then exposed a production initial HTTP 401 failure:
the general H3 error handler swallowed the refusal's Transport return after
latching rejection. It now propagates any latched initial permanent failure
until at least one Session was accepted. Added an exact process regression;
final root suite and full 29-case interop pass. Accepted unrelated-flow and
post-startup permanent-latch behavior remain tested. The failed timeout and
intermediate binary results remain retained and are not final-hash evidence.

Intermediate recovery/shutdown results (`phase6-recovery-final`,
`phase6-recovery-qualified`, `phase6-shutdown-native`,
`phase6-shutdown-qualified`) belong to their recorded executable revisions.
Pre-review completed evidence uses `phase6-recovery-complete`, `phase6-shutdown-complete` and
`phase6-interop-qualified`. Raw fixtures, certificates and generated output
remain private/ignored; tracked reports contain fixed numbers and hashes.

## Scope limitations

This evidence covers deterministic ownership/security behavior, native owned
IPv4 recovery paths, IPv4/IPv6 loopback process restart and independent
IPv4/IPv6 interoperability. It does not establish native ARM64, production
WAN/VPS behavior, sustained high-rate capacity, all final resource/diagnostic
requirements or release acceptance. Full qualification remains in T076/T077.
The loaded watchdog/probe comparison and functional idle-probe evidence are
separate; no idle CPU-cost or strict capacity inference follows.

## Pre-review native results

All nine scenarios passed on the pre-review release hash; listener inode was
unchanged and outage replay/corruption/duplicates were zero in every case.

| Case | Detect seconds | Fresh reply after restore seconds | Connection attempts | Probes | Recovering drops to target | Rejected drops to target |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| address-change | 4.996 | 5.373 | 2 | 0 | 3 | 0 |
| blackhole | 4.999 | 0.473 | 3 | 0 | 54 | 0 |
| capability | 5.016 | — | 3 | 0 | 53 | 38 |
| idle | — | 0.026 | 1 | 4 | 0 | 0 |
| long-outage | 5.009 | 0.290 | 6 | 0 | 250 | 0 |
| policy | 5.010 | — | 3 | 0 | 55 | 37 |
| restart | 5.024 | 0.653 | 3 | 0 | 55 | 0 |
| revoked | 5.009 | — | 3 | 0 | 53 | 38 |
| trust | 5.001 | — | 3 | 0 | 54 | 37 |

Permanent cases intentionally have no resumed traffic; exact refusal is
authentication/401, authorization/403, trust/0 or capability/0, and attempts
stop increasing during the subsequent monitored three seconds. Native watchdog
timeouts were zero: native idle/path failure triggered recovery first. The
deterministic watchdog test is the distinct expiry evidence. Idle retained one
connection/Session and sent four probes; there was no replacement. Address
change's restoration origin is its completed address/route change, so detection
time is included in its fresh-reply latency.

Pre-review signal matrix: 8/8 passed, recorded stops took 5–10ms, including both
signals during held setup. Full logs and hash-bound summaries remain under
`benchmarks/results/phase6-{recovery,shutdown}-complete/`; independent interop
has its complete 29-row summary in `phase6-interop-qualified/`. These are
local ignored evidence, not repository-distributed runtime promises.

## Pre-review T063 loaded-path comparison

The frozen prospective policy passes all 30 final rows (baseline/tunnel,
forward/reverse/simultaneous, five paired repetitions). All enabled watchdog/
probe loaded-path CPU, owner CPU, RSS, RTT, delivery and loss bounds pass.
No threshold/reference was changed; no development exception was required.
Every active direction delivered 3000/3000 tagged messages (1.024Mbit/s),
with zero integrity/measurement errors. This is low-rate loaded-path evidence,
separate from the four actual idle probes and native recovery timings above.
Exact executable/policy hashes, reference method and per-role ranges are in
[performance.md](../performance.md#phase-6-t063-prospective-loaded-path-comparison--2026-10-12).
The clean candidate artifacts are `benchmarks/results/phase6-candidate/`.

## Independent-review correction — 2026-10-13

The requested read-only reviewer found one P1: after an accepted flow kept
the process alive, permanent mapping rejection left other Starting and
AwaitingAcceptance Sessions able to submit CONNECT or consume a delayed 200.
The local terminal admission latch could be bypassed by already-created setup;
this was not evidence of bypassing server authentication/authorization.

Permanent rejection now cancels every not-yet-accepted client generation before
another H3 turn, releases its setup work and sender association, and retains
previously accepted Active flows. CONNECT submission and delayed-success
acceptance have independent latch guards. A valid delayed success for cancelled
setup is discarded as cancellation, not a malformed peer response. Mapping
refusal is recorded with original connection/session ids before retirement.
Cancellation uses the existing stream/queue/generation cleanup; immutable mixed
ciphertext committed before cancellation retains its original contract boundary.
No new application payload is activated or reassigned to cancelled flows.

The new production-owner regression exercises 401/403/501 with an accepted
flow, a refusing flow, a real awaiting stream with withheld valid 200 and a
queued Starting flow. It verifies zero setup permits, removed sender mappings,
no new CONNECT, no late activation, cancelled retries and real encrypted QUIC
delivery of only the accepted flow's datagram. Fault-injected stale setup tests
both independent guards. All eight recovery library regressions pass; full
root suite is now 143 passed/2 ignored. Locked root/tool release builds and
Clippy, traffic 11 and Python 38 pass. Go fixture/source/module sums unchanged.

The optional reviewer recommendation is also implemented: shutdown prebuilds
its exact process executable via Cargo JSON (respecting custom target dirs),
records its SHA-256 and checks it remains identical across the matrix. Debug
process evidence is explicitly separate from release recovery/interop evidence.
Final corrected release SHA-256:
`dee40610068f3f20ccdb8799230bfef8a74022d5c6e02fb5ab09e97f7ca49602`.
Shutdown debug SHA-256:
`8a62a774a786ae5b765c7ac837e3317d83d85be789c85a75653e38b1a3b3fc08`.
New artifacts use the `benchmarks/results/phase6-review-fixes-*` prefix; the
pre-review results above remain evidence only for their original hashes.
No independent post-fix approval is claimed.

Corrected-executable native recovery: 9/9 PASS, unchanged listener, zero outage
replay/corruption/duplicates. Exact renewed permanent refusal stays denied.
Independent interop: 29/29 PASS on the corrected release hash. Shutdown: 8/8
PASS, every stop 5ms, with both signals during held setup executed explicitly.

| Corrected case | Detect seconds | Fresh reply after restore seconds | Attempts | Probes | Recovering drops to target | Rejected drops to target |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| address-change | 5.002 | 5.380 | 2 | 0 | 3 | 0 |
| blackhole | 5.014 | 0.368 | 3 | 0 | 53 | 0 |
| capability | 5.009 | — | 3 | 0 | 54 | 37 |
| idle | — | 0.030 | 1 | 3 | 0 | 0 |
| long-outage | 5.003 | 3.245 | 7 | 0 | 279 | 0 |
| policy | 5.022 | — | 3 | 0 | 54 | 37 |
| restart | 5.015 | 0.521 | 3 | 0 | 54 | 0 |
| revoked | 5.008 | — | 3 | 0 | 56 | 35 |
| trust | 5.011 | — | 3 | 0 | 52 | 39 |

These are new measured outcomes with the same fault/restoration origins and
unchanged profiles. The longer outage needed seven bounded attempts; actual
fresh traffic resumed 3.245s after restoration, within the ten-second bound.
Native watchdog timeouts remain zero because native idle detection fired
first; the separate deterministic test still establishes watchdog expiry.

Corrected-executable T063 comparison: **30/30 strict PASS**, with all 30 rows
valid and every original CPU/owner CPU/RSS/RTT/delivery/loss bound unchanged.
All active directions delivered 3000/3000 messages; loss, integrity errors and
measurement errors were zero. Reference/policy/traffic hashes are the original
frozen inputs, not a new post-correction reference. No exception was needed.
See [performance.md](../performance.md#phase-6-independent-review-correction-comparison--2026-10-13).
