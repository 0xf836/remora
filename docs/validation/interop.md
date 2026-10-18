# Independent interoperability evidence

This is a historical outcome report. The named matrices and raw logs remain
in ignored local `docs/validation/interop-results/` archives; they are not part
of a fresh checkout. See [artifact retention](README.md).

2026-10-11, native Linux x86_64. The separate Go 1.27.2 fixture uses masque-go
v0.6.0, whose authenticated module-origin metadata resolves to commit
`c1cf0e4dd6439aea94d5491b27439a4736f00246`, and quic-go v0.63.0 at
`9d085cc690f7c96451e8ae5659eb0e64671da47a`. `go.mod`/`go.sum`, the fixture
source, binary SHA-256, Go build metadata and runner/source hashes identify the
inputs. Dependencies downloaded with standard Go checksum verification;
`go build -mod=readonly` and `go vet ./...` pass. No production dependency or
second production executable is added.

The verified-code set has 21
passed actual process cases. Numeric proxy dialing retains generated RootCAs
and ServerName=remora.test; certificate verification is never disabled. Positive
cases in both independent directions cover IPv4/IPv6, two request/local flows,
exact byte equality and boundaries at sizes 0/1/128/512/1200, with an actual
monitored target. The independent client multiplexes request streams in one
QUIC/H3 connection. No Remora codec implements the independent wire framing.

| Direction | Actual negative evidence |
|---|---|
| Go client → Remora server | Bad, revoked and missing token: HTTP 401; forbidden target: HTTP 403. Archived zero-packet coverage has the denied-target limitation below. |
| Remora client → Go proxy | Bad/revoked token, denied target, wrong TLS name and absent DATAGRAM capability: refusal and zero target packets |
| Go response → Remora client | Final 204/205/206, Content-Length:0, Content-Type, Transfer-Encoding: zero target packets and H3_MESSAGE_ERROR 0x10e |
| Go Capsule → Remora client | Missing Context ID: H3_MESSAGE_ERROR 0x10e; Context-0 payload 65528: H3_DATAGRAM_ERROR 0x33; zero target packets |

The final-revision profile matrix extends this with approved
hostname/dual-answer localhost, mixed-DNS denial, malformed route, pre-request
DATAGRAM, truncated Context ID, missing peer settings, and independent request
Capsule injections. Its immutable summary records **29 passed cases** with exact request/response
Capsule codes and positive target byte/boundary coverage; no missing cases are
inferred from the earlier 21-case set. Localhost fixtures
require both ::1 and 127.0.0.1 answers and authorize all answers for the positive
case; the mixed case explicitly permits only ::1. No /etc/hosts or resolver
configuration is changed. Controlled DNS replacement/remote infrastructure,
0-RTT replay, exhaustive peer/control limits, closed-flow/churn under load and
final-duration acceptance remain release qualification; deterministic same-stack
policy/lifecycle/protocol evidence is labelled as such in [MVP evidence](mvp.md).

Third-party arbitrary reason logging is discarded by the fixture. Reports are
fixed outcomes; private credentials/TLS material stay in owned temporary
fixtures and are removed. All collected output is scanned for token and
payload sentinels. Negative cases require observed refusal/error/discard evidence
and zero monitored target packets, rather than treating silent UDP alone as proof.

## Negative assertions after the results audit

The results audit identified two runner weaknesses, corrected after the archived
29-case matrix. Remora-client refusals now require both exit code 1 and a
structured client failure with the exact expected class/code: authentication/401
for bad or revoked tokens, authorization/403 for a denied target, trust/0 for a
wrong TLS name, and capability/0 for absent DATAGRAM support. The Go client and
Python runner now require the exact denial HTTP status: 401 for credential
cases, 403 for denied target or mixed DNS, and 404 for a malformed route.
Unrelated startup/transport failures and mismatched denial statuses cannot pass.

Both denied-target directions now request the controlled loopback endpoint
bound by the monitor and authorize a different port in the receiving proxy's
fixture policy. Summaries record the monitored endpoint and observed refusal.
The Go client stops on CONNECT-UDP denial without sending application payload:
this case proves the request refusal and absence of observed target traffic,
not rejection of an attempted forbidden application payload. Same-stack process
security tests provide the separate application-sentinel coverage.

In the archived Go-client denied-target case, the requested target was
192.0.2.1 while the monitor was bound to loopback. Its recorded HTTP 403 remains
valid; its zero loopback packets do not prove absence of traffic to 192.0.2.1.
The saved raw logs contain the expected refusal causes/statuses for the other
cases, but the earlier runner did not assert each one precisely. Original
results, source hashes and summaries remain unchanged. At the project owner's
request, these runner corrections were not followed by new test execution;
the archived 29 passes are evidence for the earlier inputs, not a new pass for
the corrected runner or rebuilt Go fixture.

## Reproduction

After the nine review corrections, the
review-fixes matrix again passed
all 29 cases on native x86_64 with the same pinned peer and normal CA/name
validation. The separate Rust ignored test was explicitly executed and passed
as well. This validates this increment's independent wire compatibility; it
does not replace the final release matrix or independently audit earlier
archived outcomes.

```bash
(cd tools/interop && go build -mod=readonly -o /tmp/remora-interop . && go vet ./...)
./scripts/validate/interop.sh --fixtures /tmp/remora-validation \
  --peer-ref c1cf0e4dd6439aea94d5491b27439a4736f00246 \
  --direction both --peer-binary /tmp/remora-interop --output /tmp/remora-interop-results
REMORA_INTEROP_BIN=/tmp/remora-interop \
  cargo test --locked --test protocol interop_cases -- --ignored
```

Output must be fresh. The runner validates the supplied private fixtures and
creates isolated per-case fixtures. The normal Rust suite marks this one test
ignored with its external prerequisite; it was explicitly run with the pinned
peer and passed. This is separate executed evidence, not a normal-CI pass.
Hosted CI and ARM64 interoperability have not run.

## Preserved failures and resolution

Initial results preserve the failure of the first
bad-token independent case: Remora queued rejection headers then aborted its
write stream during session cleanup. The target saw no unauthorized traffic,
but the expected HTTP rejection never arrived. Cleanup now retires the flow,
resources and cancellable datagrams while preserving the final refusal/FIN.
A real encrypted Rust regression proves the peer receives HTTP 401 after local
session removal; all independent denial cases pass after the correction.

Rejection-fix preserved an early Go-server
fixture race: Remora started before the independent server bound UDP and got
ICMP refusal. The Go fixture now binds explicitly and emits a fixed readiness
record before the Remora client starts. Extended
preserved an invalid wrong-name test configuration, corrected by consistently
changing client authority/template/name so the negative reaches actual TLS
verification. All original failures remain visible; neither was a passing
compatibility result. The final sets include normal byte/boundary forwarding
and tested protocol refusals. Performance impact of the production cleanup fix
is compared against the pre-change frozen reference policy.

Sources: [pinned masque-go API](https://github.com/quic-go/masque-go/tree/c1cf0e4dd6439aea94d5491b27439a4736f00246),
[quic-go v0.63.0](https://github.com/quic-go/quic-go/tree/v0.63.0).
