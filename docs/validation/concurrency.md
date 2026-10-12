# Phase 4 concurrent-session validation — 2026-10-12

Scope: T039–T047, US2. The implementation and qualification retain the single
production package/executable and exclusive reactor owner. See
[ADR-0008](../adr/0008-concurrent-admission-and-fair-service.md) and the
[performance record](../performance.md). Subsequent phases and full release
qualification are outside this increment.

## Recorded build and local artifacts

Native Linux x86_64, revision `00d1dbe06fd0c67eb9053101a20f1e3eed158509`
plus uncommitted Phase 4 sources. Build revision alone is not source attestation:
each manifest records source, lock, runner and executable hashes, kernel and
CPU affinity. Original Phase 4 qualification executable SHA-256 (before the
independent review corrections described below):
`f0d22b9400ce1666cff4cac86f611cbe579c4df5dbada79710f70aff814f4e3a`.

Raw manifests, sanitized configuration, samples, stdout snapshots, stderr events
and summaries stay in ignored local `benchmarks/results/` directories under the
[artifact retention contract](README.md). Credentials/private keys are generated
inside temporary fixtures and are neither recorded in reports nor committed.
These local artifacts are not distributed with a fresh checkout.

| Original qualification artifact directory | Qualification |
|---|---|
| `phase4-reference-native` | Pre-change paired reference and bound policy |
| `phase4-candidate-complete` | 12/12 paired comparisons pass |
| `phase4-isolation-complete` | 1800-second isolation passes; zero cross-delivery/integrity errors |
| `phase4-overload-complete` | 600 seconds pass; worst per-flow timely reply ratio 598/600 |
| `phase4-overload-allocator-complete` | Separate instrumented 600-second overload passes; warmed allocator capacity stays stable |
| `phase4-churn-complete` | 1000 cycles pass; cleanup 0.4904 seconds |
| `phase4-interop-complete` | Independent Go peer: 29/29 cases pass in both directions |

## Independent review corrections

The requested read-only review confirmed the original duration outcomes and
found two P2 issues: a previously existing ignored local critical-stream STOP,
and an unsupported new numerical claim of 32 operations/visit. Both were accepted.
The adapter now identifies opened, unfinished control/QPACK streams, checks them
independently of ordinary writable rotation and closes their connection with
0x104 on STOP/native absence. The owner cancels only that connection's Sessions;
new setup/enqueue is blocked, old ciphertext is discarded and close-packet
generation stays enabled. A hostile peer that does not independently poll H3
exercises all three critical streams in both roles and receives the local close.
The regression fails before the correction. Owner integration checks cancellation
scope and continued close generation.

Commitment and active-capacity servicing now each rotate at most four selected
Sessions per visit. A nine-stream owner regression observes activation 4/8/9 and
retirement 4/4/1; removing the quotas reproduces the failure (9 instead of 4).
ADR-0008 reports per-class selected-item limits and at most eight owner visits
between timer/cancellation checks. Metadata enumeration and quiche internal work
are not falsely counted as 32 native calls or 256 instructions.

The original 1800/600-second and allocator artifacts above remain tied to their
original binary; they do not qualify the corrected binary by inference.
Post-correction checks and artifacts are recorded separately below. The reviewer
has not audited the correction itself a second time.

The final correction executable SHA-256 is
`9a7504b55ed9ab90425e0f152da3c9d59851fc19d6dbe610e1ef8fd967e39d53`.
Release build, formatting, Clippy (`-D warnings`) and the complete
all-target/all-feature Rust suite pass: 117 tests, plus one ignored independent
peer case dispatched separately. No production unsafe code, dependency,
configuration schema or UDP syscall path changes were needed. Continuation is
requested only when a full commitment/capacity quota made progress with work
remaining; ordinary healthy capacity and ACK-waiting receipts do not spin.

| Final correction artifact directory | Outcome |
|---|---|
| `phase4-review-qualified-candidate` | 12/12 pass under the unchanged original reference/policy |
| `phase4-review-qualified-interop` | 29/29 independent Go peer cases pass |
| `phase4-review-qualified-isolation` | 60-second/32-session smoke passes; no cross-delivery/integrity errors |
| `phase4-review-qualified-churn` | 1000 cycles pass; cleanup 0.6757 seconds; max warmed RSS growth 0.1680 MiB |

All final correction manifests match the production source and executable hash
above; process-run manifests also match the unchanged concurrency runner. The
31 continuously active isolation flows receive 60/60 probes per direction.
The deliberately idled/recreated flow offers 57 per direction and receives
56 forward / 55 reverse within one second, retaining setup losses explicitly.
Isolation lasts 61.017 seconds including final reply drain and has zero lost
terminal diagnostic coverage. Churn exchanges 32832 tagged datagrams per
direction including warmup and settles session/setup/relay gauges to zero.
Its settled RSS growth is `[155648, 172032, 155648, 176128, 172032]` bytes
(server, then clients). Both process runs have maximum FDs
`[43, 12, 12, 12, 12]`, zero integrity/cross-delivery/generator errors, all role
exit codes zero and released listeners.

Benchmark
comparison uses the original pre-Phase-4 reference and unchanged frozen policy;
it runs serially before the separate process laboratories. Interop/isolation/churn
laboratories may overlap. The 60-second isolation smoke and normal Cargo overload
regression are incremental checks, not repeated 1800/600-second qualification.
The original allocator experiment is not repeated for this correction.

The incomplete first post-review benchmark (`phase4-review-candidate`, an
intermediate binary) fails in the baseline traffic tool with `Connection refused`
and an empty client snapshot, causing the existing runner to raise `IndexError`.
It is retained and does not count as a passing comparison. Intermediate
`phase4-review-final-*` artifacts precede the final continuation refinement;
the final evidence uses `phase4-review-qualified-*` only.

## Original automated checks and regression coverage

Release build, Rust formatting and Clippy with `-D warnings` pass. The full
all-target/all-feature suite passes 113 tests with one explicitly ignored
independent Go peer test, including the closed-control identity-credit regression.
The subsequently added normal-writability regression also passes separately:
114 Rust tests were verified in total.
The Python tooling's existing 28 tests pass; shell syntax,
Python compilation and whitespace checks pass.

The ignored interoperability test is also explicitly executed separately with
the already built, pinned Go 1.27.2/masque-go v0.6.0/quic-go v0.63.0 fixture.
All 29 process cases pass against the final release executable in both
independent directions, including empty/max/multiplexed payloads, exact refusal
statuses, zero monitored target packets for negatives and diagnostic sentinel
checks. Its manifest records peer/module/build and final Remora hashes; see
[interoperability](interop.md) for the pinned fixture and assertion limits.

Owner component/property tests in `tests/lifecycle/sessions.rs` and
`tests/lifecycle/udp_reuse.rs` are included under `cfg(test)` inside the reactor
module, so they exercise private owner state rather than a parallel fake owner.
They cover pending identity debit before DNS, duplicate/stale callbacks, exact
credit on cancellation, sparse UDP within 120 seconds, keepalive-only expiry,
joint stream/socket release, closed-control permit retention/collection, a
mapping rejection latch across source churn and connection replacement, and
five continuously pending bodies plus native producers. All five bodies consume
data within two owner visits; closing one preserves the other four.

The 128 sequential-stream transport regression crosses the initial peer stream
limit and verifies returned native stream credit. Restoring the old first-four
HashMap body selection in a separate scratch copy fails the five-body regression;
disabling stopped-writable reclamation fails the 128-stream regression. Neither
deliberate failure is counted as a qualification pass.

## Process topology and measurement

The runner owns one server, four clients, two credentials, two authorized UDP
targets and eight distinct source sockets per client: 32 live streams, sharing
and distinguishing target endpoints. Tagged 128-byte messages include direction,
source index, sequence and checksum. It validates exact bytes, target socket,
client source/listener and unique live relay tuples. Forward echo and independently
generated reverse traffic are checked separately. An intentionally idled source
expires and recreates while 31 others continue.

The process tests in Cargo run short isolation/overload/churn regressions through
the same Python runner. The duration runners use the final release executable
through `scripts/validate/e2e.sh`. Each uses separate owned fixtures/processes;
duration/churn laboratories may overlap. The benchmark comparison runs serially
before those laboratories, without overlapping builds or tests.

Test-only idle overrides are 2000 ms for isolation/overload and 500 ms for churn.
They accelerate closure/recreation and are explicit RFC SHOULD deviations.
The production default remains 120000 ms. The deterministic owner test covers
the two-minute application clock independently of these short settings.

Saturation admits a separate heavy flow with 256-message bursts every 50 ms,
64 excess source attempts and 64 malformed outer packets per burst. The 32 probe
flows each send one 128-byte request/second and receive independent reverse
traffic. Limits are four connections, 33 total sessions/relay sockets, 17 per
identity, 32 per connection, 16 setups, two application packets/direction and
64 native DATAGRAM packets. Measured `queue_full` must be nonzero; every probe's
forward and reverse response ratio within one second must be at least 99%.
This is configured queue/admission saturation on responsive loopback, not link
exhaustion or a sustainable throughput/capacity measurement.

The deliberately hostile overload may overflow the bounded terminal diagnostic
ring. Coverage loss remains explicit in the result and is not reclassified as
successful SC-010 failure correlation. Status write failures/arithmetic overflow
still fail the runner; isolation/churn require zero diagnostic coverage loss.

Final isolation measures the full 1800-second interval (1801.185 seconds
including final reply drain). All 31 continuously active flows receive
1800/1800 forward and 1800/1800 reverse probes within one second. The deliberately
idled/recreated flow offers 1797 probes per direction: forward progress is
1796/1797 and reverse progress 1795/1797, with setup-time UDP loss retained
explicitly. No application first-packet delivery guarantee is inferred.
There are zero cross-deliveries, integrity or generator errors and zero lost
terminal diagnostic coverage in all five roles. Maximum tick lateness is
0.279 ms; maximum FDs are `[43, 12, 12, 12, 12]`. Every role exits zero and
all listeners are released. Closing/recreating that flow leaves the other 31
continuously usable.

Final overload offers 600 forward probes and 600 reverse messages to each
admitted probe flow. Forward replies within one second are 598/600 for three
flows, 599/600 for five and 600/600 for 24: worst ratio 99.6667%. Reverse
progress is 600/600 for all 32. The criterion passes without averaging across
flows. Maximum generator tick lateness is 8.016 ms. There are zero cross-deliveries,
integrity or generator errors. Maximum FDs are `[44, 12, 12, 12, 12]`; all five
roles exit zero and listeners are released. Explicit lost terminal diagnostic
coverage is `[0, 122007, 128593, 128107, 129316]` (server, then clients). This
hostile overflow qualifies bounded robustness, not SC-010 correlation
completeness. The runner checks every recorded resource gauge/peak against
its declared bound and observes actual application queue saturation.

## Churn accounting

The final run creates/closes 1000 additional associations in batches of at most
32, after a warmed 32-stream baseline. It passes with zero cross-delivery,
integrity or generator errors: 32832 forward and 32832 reverse messages including
setup/warmup traffic. All global session/setup/relay-FD gauges settle to zero
in 0.4904 seconds after the final closed batch, below the unchanged 60-second
criterion. Server FD maximum is 43; each client maximum is 12. Relay descriptors
return to the warmed baseline minus 32 server relay FDs. Every role exits zero
and all listeners can be rebound after owned process shutdown.

Settled RSS growth in bytes, server followed by four clients:
`[122880, 172032, 163840, 155648, 172032]`. Maximum growth is 0.1641 MiB,
below the unchanged 10 MiB release criterion. This is retained-memory growth
against a warmed baseline, rather than peak RSS or a claim that native heaps
contain no allocations.

| Settled retained capacity | Server | Each client |
|---|---:|---:|
| Application slabs, bytes | 66000 | 48000 |
| Ciphertext slabs, bytes | 5400 | 5400 |
| Scratch slabs, bytes | 44550 | 43200 |
| Pool descriptor capacity, bytes | 3680 | 3040 |
| Session queue descriptor capacity, bytes | 0 | 0 |
| Native/control/output descriptor capacity, bytes | 40960 | 10240 |
| Native send/receive queued bytes | 0 / 0 | 0 / 0 |

The four idle server connections and one per client remain reusable during
settled sampling; shutdown releases them. The native/control descriptor gauge
includes the finite closing-ID ledger and is distinct from all quiche/H3/TLS
heap memory. RSS measures retained native heap growth separately. The existing
2 MiB diagnostic retention ceiling is reported as a bound, not a measured heap
breakdown. Per-role `/proc` samples separately record owned UDP socket inode
counts and kernel TX/RX queue allocations; requested/effective kernel buffer
ceilings remain in snapshots and are not added to RSS.

## Allocator investigation

An intermediate overload run's server RSS grew from about 9.05 to 11.72 MiB.
A separate observational probe used glibc 2.44 `mallinfo2()` once per second
to distinguish occupied allocator memory from reusable arena capacity. Its
temporary `LD_PRELOAD` library adds one sampling thread per Remora process;
it does not interpose allocation functions or inspect payloads. This changes
the process being measured, so its results are separate diagnostic evidence,
not benchmark CPU/RSS qualification or a static bound for all quiche/H3/TLS
allocations. The final probe repeats the same 600-second overload profile.

| Server allocator, seconds 60–589 | Intermediate FIN-signalled cleanup | Final native-absence cleanup |
|---|---:|---:|
| Occupied allocator bytes, min–max | 1859472–2857024 | 1698608–1761808 |
| Arena capacity bytes, min–max | 2195456–3293184 | 1994752–1994752 |
| Reusable arena bytes, min–max | 226576–508224 | 232944–296144 |
| Mapped allocator bytes, min–max | 143360–1126400 | 0–0 |

The final run passes the ordinary overload assertions, with worst forward
progress 599/600 and reverse progress 600/600 for every probe. Occupied memory
at seconds 60 and 589 is 1702368 and 1716656 bytes; the warmed arena is constant.
The uninstrumented final overload separately records server RSS of 9.406 MiB
at second 59 and 9.430 MiB at second 599. These observations support bounded
retention over the measured interval; they do not establish an indefinite
leak-free guarantee or replace the 1000-cycle reclamation criterion.

The intermediate `phase4-overload-allocator` and final
`phase4-overload-allocator-complete` artifacts retain probe source/library,
instrumentation metadata and numeric stderr samples. Probe source SHA-256 is
`de922cf29ff3e33bd2f755dd75c688c8c12baf137a3f05449c41809e2afbf6e4`;
library SHA-256 is
`dfcb9e10cd7de8c0a90986166f7d6a46ae35f2467bc7198d49c6faa6cb7f386f`.
The probe is not part of production code or the normal validation runner.

## Retained failures and trust boundary

The intermediate FIN-signalled cleanup predicate releases a closing ID before
all native send data is ACKed. This can omit the final H3 metadata cleanup.
The controlled FIN-before-ACK component regression fails under that predicate
and passes when only actual native absence releases the permit/ID. Ordinary
live writability also must not request owner continuation: the intermediate
`phase4-candidate-ack` benchmark records a near-one-core client and fails five
runs under the unchanged 0.5-core ceiling. The normal-writability regression
and the final 12/12 comparison validate its correction. Generation horizons
and reserved FIFO closing visits prevent continual arrivals from extending
an old service round; a continuous-churn regression covers older producers.

The original `phase4-churn-1000` run fails with
`TimeoutError: session 3 setup`: completed STOP_SENDING streams retained peer
stream credit after the application Session had already gone. The finite
stopped/closing-stream service and H3 public error-path cleanup fix that bug.
Closed IDs retain global/identity permits until native collection; they retain
no payload/relay socket. A stalled control association has a 60-second deadline
which fails its connection. The final 1000-cycle run qualifies this corrected
binary. Intermediate successful binaries and interrupted duration runs remain
separate historical artifacts, not final duration evidence.

An early saturation smoke with a four-packet native queue failed the probe
criterion; it exposed legitimate opaque-packet queue loss, not misdelivery.
The final test keeps the documented 64-packet native queue, saturates the
two-packet application queues of a separate load flow and does not prioritize
or inspect probe contents inside Remora. Early runner warmup/counting failures
and interrupted runs are retained; their summaries do not satisfy acceptance.

Self-review of the original changed trust/ownership paths finds authentication before
DNS/forwarding, exact immutable identity authorization, connected target sockets,
generation checks, finite admission/queues/control cleanup and numeric privacy-safe
resource diagnostics intact. No unsafe code or dependency was added. Existing
security and conformance negative tests pass. The subsequent explicitly requested
independent review is recorded above; its confirmed findings and their corrections
must accompany the original qualification rather than implying an unconditional
protocol/scheduling acceptance.

Identifiable old callbacks, queued packets and closed stream IDs cannot attach
to a recreated association. A native close/rebind experiment separately delivers
an authorized delayed UDP reply to an exactly reused tuple; UDP supplies no
generation field distinguishing it from a current reply. A wrong source is
filtered. See [the provenance boundary](../adr/0001-transport-profile.md).
No unconditional external provenance or quarantine guarantee is asserted.

## Reproduction and limits

```bash
cargo build --locked --release
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
python3 -m unittest discover -s tests -p 'test_*.py'
REMORA_INTEROP_BIN=/tmp/remora-interop \
  REMORA_INTEROP_OUTPUT=benchmarks/results/us2-interop-new \
  cargo test --locked --test protocol interop_cases -- --ignored
./scripts/validate/e2e.sh --scenario concurrency --suite isolation \
  --seconds 1800 --output benchmarks/results/us2-isolation-new
./scripts/validate/e2e.sh --scenario concurrency --suite overload \
  --seconds 600 --output benchmarks/results/us2-overload-new
./scripts/validate/e2e.sh --scenario concurrency --suite churn \
  --cycles 1000 --output benchmarks/results/us2-churn-new
```

Use fresh output paths and permit native UDP sockets. No host network changes,
remote deployment, publication or commits are part of this workflow. The optional
interop command requires the qualified fixture from the interoperability guide.
ARM64, a controlled 1-Gbit/s/20-ms
reference link, release-duration performance matrix, full telemetry (T064),
diagnostic-overhead comparison and later recovery/PMTU/offload work are not
qualified by these native Phase 4 checks.
