# ADR 0016: Preserve minimum QUIC payload after congestion-credit clamping

Status: adopted correction; candidate13 x86_64 functional qualification passed,
full numerical qualification unsuccessful; pinned Git-source migration adopted.
Date: 2026-10-15.

## Reproduced fault

The source12 focused comparison still fails reverse resource delivery even after
collector isolation. Temporary numeric-only instrumentation reproduces 44.69%
reverse loss, three initial 36-byte PING congestion events, no outer UDP/qdisc
drops, and physical submissions within 14us. The peer receives corresponding
small packets for a known connection but does not accept them as fresh QUIC
packets. This explains why extra socket/native capacity or disabling HyStart
was insufficient. The instrumentation is confined to ignored diagnostic builds.

Checksum-verified official quiche 0.30.0 limits remaining output space by
congestion credit after its initial minimum-payload check and ACK packing. It
can then emit a one-byte PING with only 1–3 payload bytes available. Mandatory
padding cannot fit and its insertion silently fails. With short packet-number
encodings, header protection samples backing storage outside the encrypted
suffix actually transmitted. The receiver cannot recover a valid header and
silently discards the packet; subsequent ACKs make the sender declare congestion
loss. CUBIC then remains below the path BDP, amplifying one control loss into
application queue expiry. This is malformed packet generation, not evidence
of authentication bypass or a crypto-key disclosure.

[RFC 9001 section 5.4.2](https://www.rfc-editor.org/rfc/rfc9001.html#section-5.4.2)
requires the sample to be contained in the protected packet and describes minimum
padding for short packet-number encodings. The correction preserves quiche's
existing conservative four-byte minimum for all encodings.

## Decision and bounded behavior

After congestion-credit clamping, if no ACK/control frame was already packed
and fewer than four payload bytes remain, return `Done` without consuming
pending control state. Clear app-limited state as in the existing initial-space
checks. Once an ACK frees credit, generate and accept the deferred PING normally.
Do not append unauthenticated padding to committed ciphertext, suppress loss
accounting, inflate congestion windows or change CUBIC/HyStart settings.

Already-packed ACK-only output remains exempt from congestion control and this
guard. PTO probes retain upstream's `usize::MAX` credit exemption. No new state,
queue, allocation, unsafe operation, worker, dependency version or public API is
added. Incoming packet validation and crypto remain unchanged. Review includes
near-full congestion, deferred control progress, low-window ACK progress and
upstream PTO/migration/interoperability cases; final native interop still runs
on the actual application executable before acceptance.

## Provenance and checks

Use the reviewed [Git fork](https://github.com/0xf836/quiche) at full commit
`154ce1995f23efa434e72dac269f707145be2015`, based on official tag `0.30.0`
(`be47c5011215b9f13bad06bd7627d3ae49888a19`). Only `quiche/src/lib.rs` and
`quiche/src/tests.rs` differ from that base. The public test is named
`ping_padding_near_full_cwnd`. Root, traffic and fuzz `[patch.crates-io]`
overrides pin the same commit. Upstream workspace octets/qlog are redirected to
their prior registry versions so the other resolved entries remain unchanged.

The initial checksum-verified vendoring was used to diagnose and qualify the
patch; the user selected a separate Git fork for continuing maintenance. The
old source tree and provenance are retained outside this repository at
`/tmp/remora-quiche-vendor-archive-20261015/vendor`. No dependency Rust sources
remain in the project tree. `dependencies/quiche.json` now records upstream
archive/base provenance and exact Git source hashes, including reviewed
README/license symlinks. The verifier resolves Cargo's actual checkout and
rejects source/revision drift. The published-crate test manifest and lock remain
as small metadata files in `dependencies/quiche-tests/`; an ephemeral harness
uses the verified Git sources and fixtures for the same library suite.

This remains a narrow maintained source patch because the public API cannot
repair missing plaintext padding after encryption without corrupting QUIC state
or ciphertext commitment. The distribution change does not select new quiche
behavior or justify replacing congestion control. See
[dependency maintenance](../../dependencies/README.md) for build/verification
and eventual upstream removal steps.

The added in-memory regression covers CUBIC/Reno, four packet-number lengths
and 1/2/3 bytes remaining congestion credit. All 24 cases fail before the guard
and pass after, retaining pending PING then proving fresh encrypted receive
progress when credit returns. The complete dependency library suite passes
1164 tests, including the existing BBR2/PTO/migration tests. Those tests ran on
the checksum-derived isolated source before import; the imported production
source and application require their own checks and performance evidence.

CI/release gates verify the pinned Git source hashes and run the dependency
tests. Release, benchmark, fuzz and dependency evidence bind those inputs.
The dependency's retained test lock remains distinct from production locks.
Advisory applicability and affected application checks are refreshed after this
source migration; earlier vendored-binary measurements remain historical.
No independent reviewer was requested.

The user-authorized upstream report is
[cloudflare/quiche#2835](https://github.com/cloudflare/quiche/issues/2835).
A separately authorized minimal fork is now available at
[0xf836/quiche](https://github.com/0xf836/quiche), branch
`fix/ping-minimum-payload`, commit
`154ce1995f23efa434e72dac269f707145be2015`, based on upstream tag `0.30.0`
(`be47c5011215b9f13bad06bd7627d3ae49888a19`). Its only changes are
`quiche/src/lib.rs` and `quiche/src/tests.rs`; the public test is named
`ping_padding_near_full_cwnd`. No upstream PR has been created.

The Git fork is now the selected distribution mechanism. Migration follows the
completed T077 run; that full run remained unsuccessful and is not relabelled.
The separately queued fork-source library check passed all 1164 tests and
verified the published commit. Git-source integration checks are recorded in
`benchmarks/results/phase8-quiche-git-migration-20261015`; full functional and
numerical acceptance of a later final binary remain separate obligations.

## Targeted runtime milestone and continuation

The uninstrumented qlog candidate13 is SHA-256
`ff75ff9fd493235e16affca4b8827d6514ebb6617b6ae160381c2bbd3312bc76`.
Three paired 60s `resource-patched13` repeats pass unchanged absolute
loss/latency/delivery/CPU/RSS and all evidence-validity/sample gates. Forward
500.72352/500.736/500.736 Mbit/s; reverse 500.736 each; loss forward
0.002492%/0/0, reverse zero; p95 <=20.589ms, CPU <=0.916 core and
RSS <=12824576 bytes per role. Original and all failed candidate results remain
failed and retained. This does not qualify unmeasured workloads or final SCs.

The guarded background continuation records a fresh old reference, freezes a
matched prospective policy, checks five alternating old/new/direct repeats for
each affected workload, and only on complete success starts fresh source-bound
fuzz/dependency/security/interop/VM qualification. Its progress lives under
`benchmarks/results/phase8-padding-followup-20261015`; full T077 is separate.
The latest 93 Python checks also cover SIGTERM/SIGHUP cleanup of an actual
owned process group. CLI runners now reach their existing cleanup blocks on
these signals; the earlier reference driver snapshot is preserved, and the
termination-only runner refresh before comparison is separately recorded.
No numerical timing/scheduling/threshold or prior result changes with that
cleanup correction. Pending tasks stay unchecked until their evidence exists.
