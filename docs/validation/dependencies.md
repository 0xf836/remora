# Phase 8 dependency audit — current Git source, 2026-10-15

**Scope update (2026-10-17):** [ADR 0024](../adr/0024-defer-arm64-release-scope.md)
defers ARM64 outside current requirements. Older ARM64/T072 pending statements
below are historical audit scope notes, not an active release gate.

T075 is complete for the current native x86_64 Git-source build/check profile.
Root, traffic and fuzz resolve quiche 0.30.0 from `https://github.com/0xf836/quiche`
at full commit `154ce1995f23efa434e72dac269f707145be2015`. The migration's
`lock-comparison.json` confirms that all other package entries, versions and
registry checksums are identical to the preceding locks; octets 0.3.7 and
optional qlog 0.18.1 remain registry dependencies.

All 22 audit/graph/native-build checks pass in
`benchmarks/results/phase8-quiche-git-migration-20261015/dependencies`.
All five Rust lock audits report zero known vulnerabilities against RustSec
commit `edb58f5974b83eb8302332821cb6d389777aa351` (1329 advisories, updated
2026-10-14T22:18:32+02:00). The retained test-only lock still has the previously
recorded yanked `yoke-derive` 0.8.3 warning; no warning-free test-graph claim.
Go applicability and native default/qlog/traffic/peer builds pass. ARM64 and
final release acceptance are not qualified by this audit.

The source verifier checks Cargo's actual pinned Git checkout and reviewed
file hashes/symlinks. The old vendored tree is archived outside the project;
only source provenance and reproducible library-test manifest/lock metadata
remain in `dependencies/`. See [dependency maintenance](../../dependencies/README.md).
Fetch dependencies before offline verification: `cargo fetch --locked`, then
`python3 scripts/validate/quiche_inputs.py`. Other runner commands below remain
applicable; paths to older source trees in historical evidence are preserved
as history, not current build instructions.

## Historical registry audit — 2026-10-13

T075 is complete for the locally claimed native **x86_64 build/check profile**;
release acceptance and ARM64 are unqualified. Production dependencies/locks and
the independently pinned peer modules were not changed.

Run `scripts/validate/dependencies.sh --output benchmarks/results/deps-UNIQUE`
with Rust 1.99.0, Go 1.27.2, cargo-audit 0.22.2 and govulncheck 1.9.0 installed.
The runner records resolved all-feature production, traffic, fuzz and safety
Rust graphs; Go module/import graphs and checksum verification; each RustSec
JSON result and Go advisory records; compiler/scanner versions, UTC collection
time and lock/build-input hashes. A fresh output directory preserves failures.
It builds locked default/qlog production and traffic and readonly Go peer on
the native executing host. Record target-native hardware separately; a build
on one architecture does not qualify another. The existing digest-pinned
Dockerfile and immutable Debian archive procedure remain the container recipe.
No new container reproduction run or hosted CI result is asserted here.

## Recorded results

`benchmarks/results/phase8-dependencies-20261013` contains all 19 successful
checks, native builds and resolved inputs. RustSec database commit
`d8325cf4f578e2df4cc5b98a30e9f46b3b88e9f0`, last updated
2026-10-13T12:39:39+02:00, contained 1309 advisories. Production (157 resolved
entries), traffic, fuzz and safety locks had zero reported vulnerabilities or
informational warnings. This is a timestamped database result, not proof that
no vulnerabilities exist. Production Cargo.lock SHA-256:
`73f95573f9fa739062f98e414be62f3e1cf06b80dec65d44b80cef587e251cc7`.

Govulncheck 1.1.4 was incompatible with Go 1.27 AST and panicked; that failure
is preserved separately in the initial audit attempt. Version 1.9.0 completed
symbol analysis against the Go database updated 2026-10-08T22:31:09Z. It emitted
10 **module-level** findings, no imported affected-package or called-symbol
findings. Package applicability was additionally checked against `go list
-mod=readonly -deps ./...`, and archived in
`benchmarks/results/phase8-dependencies-applicability-20261013` with report and
input hashes. The final runner automates that check and requires review when an
affected package is imported or advisory metadata is insufficient.

| Advisory IDs | Affected module/package | Disposition on this exact fixture graph |
|---|---|---|
| GO-2026-5932 | x/crypto/openpgp and subpackages | Not imported |
| GO-2026-6303, GO-2026-6354, GO-2026-6355 | x/crypto/ssh | Not imported |
| GO-2026-6603, GO-2026-6610, GO-2026-6611, GO-2026-6612, GO-2026-6617 | x/net/http2 | Not imported; http2/hpack is a distinct imported package |
| GO-2026-6629 | x/text/secure/precis | Not imported |

The fixture uses HTTP/3, not x/net's HTTP/2 transport. Go's bundled net/http
implementation is on Go 1.27.2, the patched version named by these stdlib
advisories. No dependency-wide clean claim is made: the findings remain in the
archive and become review obligations if the imported graph changes. The
fixture is test-only and exposes no production OpenPGP/SSH/HTTP2 service.

## Dependency purpose and build inputs

| Dependency group | Purpose / boundary |
|---|---|
| quiche 0.30.0 / BoringSSL / boring 5.2.0 | QUIC/H3/TLS and in-memory PEM validation; no second production TLS stack |
| Tokio / socket2 / libc / rustix | Single reactor readiness/setup, Linux sockets and bounded startup filesystem/FD access |
| serde / TOML / JSON / time | Typed configuration and fixed diagnostic serialization/timestamps |
| tracing / subscriber / appender | Bounded privacy-filtered diagnostics |
| sha2 / hmac / subtle / getrandom | Token digest/constant-time check, Retry protection and randomness |
| proptest | Test-only malformed-input and state invariant checks |
| libfuzzer-sys 0.4.10 / cargo-fuzz 0.13.1 | Separate unpublished fuzz harness; not linked into production |
| tools/safety serde | Source-included ownership-only Miri package; no Linux/TLS dependency |
| masque-go / quic-go / qpack / URI template | Independent test-only protocol implementation at existing pins |

Exact version resolution and checksums are in the corresponding locks and
recorded metadata. Fuzz preserves production Rust lock versions while adding
three test-only packages. Safety has a separate lock. Nightly-2026-10-12 is only
for fuzz/Miri; production stays pinned to Rust 1.99.0. Root lock hashes, advisory
freshness and compiler pins must be rechecked after dependency changes. Native
ARM64 remains T072 and has no recorded successful build/runtime evidence.


## Patched quiche source-bound audit — 2026-10-15

This entry supersedes the historical input applicability above for candidate13:
quiche 0.30.0 is locally vendored with the minimum-payload correction recorded
in ADR 0016; production transitive dependency versions are unchanged. Exact
source/lock hashes and upstream archive provenance are retained in
`benchmarks/results/phase8-packet-fix-qualification-resume1-20261015/dependencies/summary.json`.
All 22 graph/audit/native-build checks pass on x86_64. This completes T075 for
that native scope; ARM64 and final release acceptance remain unqualified.

The previous `phase8-packet-fix-qualification-20261015` attempt is retained as
failed: fetching `windows-sys` 0.52.0 encountered a crates.io TLS EOF, and RustSec
fetches failed before vulnerability analysis. Passing `CARGO_HTTP_PROXY` and
`HTTPS_PROXY` from the existing `HTTP_PROXY` to the resumed job allowed those
fetches to complete. No lock, production source or measured binary changed.
The old four-target fuzz and safety passes are reused only after exact input
matching, with retained artifact/log hashes in the new progress/prerequisite
records; neither is relabelled as a new execution.

All five Rust lock audits report zero known vulnerabilities against RustSec
commit `edb58f5974b83eb8302332821cb6d389777aa351`, updated
2026-10-14T22:18:32+02:00, containing 1329 advisories. Production, traffic, fuzz
and safety have no reported warnings. The separate upstream quiche test lock
reports **yanked `yoke-derive` 0.8.3**, through
`quiche -> url -> idna -> idna_adapter -> icu_normalizer -> icu_collections -> yoke`.
The scanner attaches no security advisory to this warning; the yank reason has
not been assessed. The test-only upstream lock/checksum is retained for
reproducibility rather than silently updated during qualification. This is not
a warning-free claim for that graph, and production/traffic do not contain the
warning. The ten Go module findings remain `not-imported` after current package
applicability analysis; raw results are preserved with the summary.

Native locked default/qlog/traffic and independent Go peer builds pass. These
checks establish build/audit evidence for the recorded inputs and advisory
snapshot, not live protocol correctness or absence of unknown vulnerabilities.
