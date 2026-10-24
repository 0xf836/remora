# Verification and benchmark contract

Version: 1, no result is asserted. Defines the reproducible evidence required for FR-026/027/028 and SC-001..010. Accepted [A2](../proposed-amendments.md) separates early incremental measurement from full release qualification; numeric targets stay unchanged. No measurement is required before writing the initial MVP.

## Incremental MVP measurements and release qualification

Keep profiles, numerical policy templates and concise outcome reports in Git.
Generated manifests, counters, samples, logs and comparison results belong in
ignored local run directories or separately published artifact archives; their
retention does not require committing them. Preserve original failures there.
Bind the versioned limits to reference hashes in a local frozen policy before
candidate work, without changing the limits; see [artifact retention](../../../docs/validation/README.md).

The first usable implementation increment is an authenticated end-to-end client/server tunnel with validated server trust, default-deny target authorization, preserved datagram boundaries, isolated supported associations, finite resources and privacy-safe diagnostics. Implement the same-topology unencrypted UDP relay/load harness alongside it, not as a separate disposable feasibility project. Begin on x86_64; no ARM64 hardware or measurements are prerequisites.

As soon as the basic tunnel works, record reproducible baseline/tunnel runs in each direction and simultaneously. Measure delivered throughput/loss, sparse-probe RTT, CPU/RSS and available local-drop/queue statistics. Early runs may cover one session or shorter declared intervals while concurrency/features mature; declare coverage, configuration and interval rather than presenting partial measurements as a full SC pass. Preserve scripts/manifests/results so later runs can repeat the baseline.

Use observed bottlenecks to guide subsequent work: improve correctness/observability before optimizing uncertain paths, then evaluate batching/offloads/CC/buffer choices where evidence warrants it. There is no development stop merely because an initial numeric target is unmet. Fix or prospectively review target/architecture changes before claiming acceptance; never relabel failures or remove security checks for speed.

The full workload, three bounded-reference-v2 paired runs, 32-session/resource/recovery/security/interop matrix and all unchanged numerical thresholds below apply to release acceptance. Linux GNU x86_64 is the sole current required platform. ARM64 is deferred outside release/completion criteria by [ADR 0024](../../../docs/adr/0024-defer-arm64-release-scope.md); prepared ARM64 profiles are experimental and do not imply support. After the checked MVP/first-measurement checkpoint, independent-peer and prepared-WireGuard qualification may remain explicitly pending while dependency-ready product work continues; their absence cannot be recorded as a pass or bypass checks for implemented behavior.

## Prospective regression comparisons

The WAN loss-remediation increment additionally uses
[`wan-loss.json`](../../../benchmarks/policies/wan-loss.json) and the
prospective [candidate expansion](../../../benchmarks/policies/wan-loss-candidates-v2.json).
These are extra workload gates, not replacements for SC-003/004/005/007 or
their reference workloads. The [current validation record](../../../docs/validation/wan-loss.md)
separates individual screen results, invalid/missing evidence and full acceptance.
No default controller/queue change or live installation follows from an isolated
passing result. Every comparative metric requires a valid matched baseline.
In a 20/50/100/200-Mbit/s offered-rate transition test, the >=190-Mbit/s
recovery target applies at the final transition to 200 Mbit/s. It does not
apply to the deliberately offered 20-Mbit/s initial phase; constant-200-Mbit/s
cold-start recovery retains its original target and deadline.

The matching unencrypted relay establishes release throughput/loss/latency criteria; a recorded **pre-change Remora revision** establishes performance regression comparisons. Preserve both references. The first MVP measurements need no prior Remora reference or invented tolerance and cannot claim a regression pass.

After the first MVP measurements, before implementing a critical-path change or measuring its candidate, freeze a regression policy in a new comparison manifest; never rewrite the immutable baseline run to add later decisions. Identify the pre-change revision and manifest hash, the currently implemented workload/profile/platform coverage, and the metric bounds: maximum throughput decrease and maximum increases in delivered loss, p95 RTT, CPU and RSS, with explicit units and absolute/relative interpretation. Declare matching offered load/directions/session counts, repetition/interval/pair-order and noise-handling method. Justify tolerances using reference variability and the intended trade-off; do not invent universal percentages before evidence exists. If reference evidence is insufficient, collect additional declared pre-change samples in normal incremental work. New workloads require their own reference before an improvement/regression claim; they do not require a full release matrix or ARM64 evidence before x86_64 development.

At each affected behavioral checkpoint, run applicable paired comparisons against that frozen reference. A missing/unmatched reference, missing prospective policy or insufficient data is an inconclusive/invalid regression comparison, never a pass. Exceeding a tolerance is a failed regression check requiring correction or a prospectively reviewed trade-off before accepting the affected change. Retain the original failure; any revised policy applies only to a new comparison, not to relabelling existing results. Security, correctness, conformance and resource-bound checks remain mandatory. Regression allowances cannot waive or change SC-003/004/005/007 or any other release criterion. No regression evidence is required before writing the first MVP.

An explicitly authorized early-MVP development exception may adopt a separate,
broader CPU smoke ceiling without additional profiling or traffic runs. Record
its rationale and scope, preserve the original policy/failure, and label any
reassessment of existing data as retrospective, never a prospective regression
pass. This permits continued implementation; it does not waive evidence validity,
non-CPU bounds, security/correctness/conformance checks or release criteria.
The Phase 3 application is recorded in [ADR-0007](../../../docs/adr/0007-review-corrections-and-contract-alignment.md)
and [performance evidence](../../../docs/performance.md).

## Automated verification layers

Pure unit/property tests cover canonical target policy, URI encoding/decoding once, varints/checked sizes, Context ID demultiplexing, arbitrary Capsule fragmentation, fixed queue accounting, session/connection generations, deadline/backoff bounds, fairness and shutdown. Fuzz boundaries include HTTP-to-policy validation, Capsule/Datagram codecs, ancillary/GRO parser and lifecycle event sequences. Assert bounded retained state, no panic/out-of-bounds, no unauthorized/cross-generation forwarding, and preserve every failing corpus input.

Component tests drive a fake clock and scripted I/O: two streams with close/expiry before quiche packet generation (cancellable discard), after generation plus EAGAIN/partial send (committed immutable output on the original association), replacement/stop (no old output replay), output deadline unchanged by close and unaffected-stream progress; EAGAIN, partial prefix sends, EINTR, hard errors, stale setup completion, expired queue/cancellation, H3 edge-triggered draining, pacing eligibility, control reservation and finite service quanta. Protocol tests reject final 204/205/206 and forbidden Capsule message fields before forwarding; parse fragmented type-0 Capsules through Context ID/checked payload length before unsupported-carrier discard; verify 65527/65528 boundaries, truncated Context ID/FIN and parser resynchronization after a valid skipped capsule. Idle lifecycle tests retain sparse valid activity within the two-minute default, ensure QUIC probes do not refresh application inactivity, and close stream/socket together on expiry. A permanent-rejection test churns local sender tuples/cleanup/reconnect and proves the mapping latch prevents new setup while accepted unrelated flows continue.

The first transport/MVP checks include its initial GOAWAY behavior: apply the stream-ID cutoff, stop new/excluded requests and continue accepted lower-ID flows without reassignment or payload replay. Later recovery checks add verified replacement-connection creation and request placement. Initial GOAWAY verification cannot wait for that later subsystem.

Actual Linux adapter tests cover zero-length UDP, truncation/ancillary truncation, wildcard pktinfo/source selection, separate v4/v6, RX overflow, PMTU errors, offload fallback and GRO boundaries. Miri applies only to pure Rust modules it can execute; it is not proof of BoringSSL or real syscall safety.

Process E2E uses generated local CA/secret files and unprivileged ports. Matrix includes payload equality, 32 sessions with shared/distinct targets/clients/identities, invalid/revoked credentials, forbidden/mixed/changed DNS, untrusted/expired/mismatched certs, early-data rejection, malformed protocol, size limits, overload, restart/recovery, five-second shutdown and diagnostics sentinels. A1 tests require zero cross-client delivery for distinct active associations and identifiable retained/stale-generation data, including closure/recreation and delayed callbacks. Separately characterize indistinguishable external replies after tuple reuse/restart under the documented architectural strategy; no fixed quarantine window or impossible unconditional provenance pass is asserted. Raw external captures used only by controlled tests are distinct from Remora privacy-safe diagnostics.

Start native x86_64 CI with locked build, rustfmt, Clippy -D warnings and tests for implemented contracts. Native x86_64 build/property/component/syscall/process checks and applicable full qualification are required. ARM64 checks are optional experiments outside current acceptance; any later support claim needs explicitly reintroduced scope and native evidence. Scheduled fuzz/network jobs and release qualification run privileges only in the harness. Build-only cross checks supplement, never replace target-native execution. Refresh Rust/Go advisory checks for resolved graphs, explicit security review, and keep reports/artifacts tied to revisions.

## Independent interoperability

Test-only Go fixture using masque-go commit c1cf0e4dd6439aea94d5491b27439a4736f00246, quic-go 0.63.0 and Go 1.27.2; commit go.mod/go.sum and fixture source. Source/API inspected at the pin; fixture build/run remains future validation. Independent fixture uses standard CA validation, Request.Header Bearer and proxy authentication/approved numeric socket before ProxyConnectedSocket. Its wire protocol is implemented by quic-go/masque-go. Configure the pinned Transport.DialAddr callback to dial the numeric loopback proxy endpoint while TLSClientConfig retains ServerName=remora.test and the generated RootCAs. This avoids system DNS or /etc/hosts changes and preserves normal certificate verification.

| Direction | Required positive cases | Required negative/profile cases |
|-----------|-------------------------|---------------------------------|
| Remora client -> independent proxy | v4/v6, empty/mixed/max supported sizes, multiple local source flows, both directions | bad/missing token, forbidden target, unsupported capability, closed flow |
| Independent client -> Remora server | exact default/template URI encoding, literal/hostname approved target, multiplexed/separate streams, byte equality | revoked/bad token, denied/mixed DNS, early datagrams, malformed path/context, absent settings |
| Remora -> Remora | Full functional/security/recovery/load matrix | Self-interoperation labelled component/E2E evidence, not independent |

Each result records peer commit/module hashes, toolchain, trust/credential fixture version, settings/profile, actual destination traffic and byte/boundary equality. Missing peer/role/capability produces an explicit limitation, not a passing placeholder. Optional Google QUICHE provides extra evidence only after a pinned suitable credential/trust arrangement is verified.

## Reference topology and workload

Application sender -> client role -> full-duplex 1 Gbit/s bottleneck with 20 ms client/server RTT -> server role -> uncongested controlled UDP endpoint. Linux, >=4 available logical cores/role; exact CPU, kernel, NIC, software and path recorded. Namespace/veth emulation is useful for functional checks; reference performance claims require recorded hardware and shaping/offload behavior. No injected loss for pass/fail reference measurements; paths must carry 1200 application bytes plus encapsulation.

Baseline is an unencrypted UDP relay at both role locations across the same two-leg route, with identical endpoint/session mapping, I/O mode, limits/buffers, route and diagnostic settings. It is not direct traffic on a shorter route or an echo-only proxy baseline. Independent open-loop senders at both ends provide directional and simultaneous streams; sparse echo probes measure RTT. Harness payload embeds test-only session id/sequence/integrity marker; Remora remains opaque to it.

The test-only baseline CLI accepts `--outer-socket-buffer-bytes` with the same
default as Remora `io.outer_socket_buffer_bytes`; zero and values above i32::MAX
are invalid. An explicit old request of 262144 preserves the prior outer limit.
Comparisons changing this value must set matching baseline/tunnel requests,
record actual effective/clamped RX/TX limits and count owned outer sockets.
Kernel ceilings and observed queued memory are separate from process RSS.

Establish sustainable delivered baseline rate at <=0.1% loss for each 128/512/1200-byte payload, one/32 sessions, each direction separately and simultaneous traffic separately. Search offered rates with the bounded-reference-v1 stopping policy below; record rejected rates, actual load, generator headroom and receiver loss. Keep simultaneous directional values, never only a sum.

Three paired baseline/tunnel repetitions, each three-second loaded warmup and 15-second measurement; resource pairs and the explicitly declared 32-session simultaneous capacity pairs measure 60 seconds. The complete cycle has a 90-minute wall budget with 30 seconds reserved for cleanup. Budget exhaustion is invalid/incomplete, never a pass. Randomize pair order, use stable routing/identical settings, record seeds and sample counts/spread. Every run passes the relevant criterion; averaging cannot hide failed runs. Offered rate excludes IP/UDP/QUIC overhead; wire occupancy is reported separately.

| Criterion | Measurement and threshold |
|-----------|---------------------------|
| SC-001 | Generic UDP and independent WireGuard traffic 30 min, byte equality, no cross-delivery/manual restart |
| SC-002 | 100% security matrix cases emit zero unauthorized datagrams; all diagnostics contain zero secret/payload sentinels |
| SC-003 | 1200-byte payload delivered throughput >=80% matching baseline, one/32 sessions, each direction separately and simultaneous per direction |
| SC-004 | Offer 50% matching baseline sustainable rate; added delivered loss <=0.1 percentage points for all three sizes/directions, including simultaneous |
| SC-005 | Same load; increase in end-to-end p95 RTT <=5 ms; one-way timing only with established clock accuracy |
| SC-006 | Detect failure <=15 s from onset; new payloads <=10 s after stable restored availability; no outage replay or weaker authorization |
| SC-007 | 32 sessions, 1200 bytes, 500 Mbit/s each direction; per-role peak RSS <=128 MiB, average CPU <=2 core-seconds/second; 1000 churn cycles settle <=10 MiB above warmed baseline by <=60 s |
| SC-008 | Stop/release resources <=5 s in Starting/Active/Recovering |
| SC-009 | 30-min isolation plus 10-min configured-budget saturation without link exhaustion; every admitted low-rate probe gets >=99% replies <=1 s; probes 128 bytes and 1 request/s/session |
| SC-010 | Functioning collector captures every injected observable failure class/session, including terminal cleanup during ordinary log suppression; status workload within documented capacity, zero terminal/status coverage loss; overflow/failed collection is failed or invalid, never pass; clean-install flow succeeds unprivileged |

Separate test-tool-generated loss/overload from Remora local drops, kernel overflow and outer QUIC lost-packet estimates. Delivered loss uses unique received sequences versus submitted offered messages, with declared drain window and duplicate/reorder/corruption counts. Record generator enqueue failures separately; runs with saturated generators cannot establish Remora capacity. Resource churn includes all state retained by the chosen lifecycle/isolation strategy and its bounded cleanup. No fixed socket-retention interval is assumed, and stale payloads cannot be retained for reassignment. Report kernel socket memory independently of process RSS.

## Manifest and result artifacts

The MVP comparator validates the reference and candidate before applying any
bound: exactly one valid run per `(pair, mode, direction)` for every declared
pair, both baseline/tunnel modes and forward/reverse/simultaneous directions.
Duplicates, missing workloads, unexpected keys or invalid pair indices cannot
pass based on the total row count. A frozen schema-version-1 policy must contain
each of the six workloads exactly once and all mandatory metrics: active
direction delivery/loss, p95 RTT, and each role's process CPU, owner CPU and
peak RSS. Require the defined units and finite nonnegative bounds, including
the appropriate minimum for delivery and maximum for every other metric.
Malformed/incomplete evidence is invalid; synthetic tests exercise this without
depending on archived real measurement files.

Each run directory contains manifest.json, counters.ndjson, latency histogram, samples.csv, summary.json, optional CPU/syscall profiles and an explicit pass/fail/invalid status for each criterion. Manifest fields: Remora/baseline/peer revisions, Rust/Go/dependency locks, compiler/build/profile, arch/CPU/core placement/governor, kernel, NIC/driver/offloads (requested/effective modes and fixed fallback reasons), MTU/socket buffer requested/effective/kernel memory, qdisc/delay/rate/queue rules, FD/process limits, CC/pacing/batch/pool settings, diagnostic modes, topology endpoints, payload/session/direction/seed, offered/actual rates, run/drain intervals and clock evidence. Regression comparisons additionally store the pre-change revision/reference-manifest hash, frozen policy/metric bounds and units, reference variability, comparison method, coverage and separate regression outcome; initial MVP runs explicitly have no regression claim. Hash all manifests/lockfiles; preserve failures and original targets.

Diagnostic robustness separately saturates ordinary event/qlog output, terminal ring and blocked/failed sinks. Status reservation and acknowledgement must preserve correlation under ordinary suppression; intentional retention/output overflow must expose coverage gaps without unbounded memory or packet-loop blocking. It is not an SC-010 success case.

The native resource collector uses the tools CPU mask, disjoint from both role
masks, throughout each run. Record requested/effective collector affinity and
verify it at each sample; restore caller affinity on success or failure. A change
to collector placement requires matching fresh reference evidence before a
regression comparison; preserve earlier runs with their original method.

Planning packet-rate budget: 500e6/(1200*8) = 52,083 payload datagrams/s/direction, about 104,167 handled application datagrams/s/role. Two CPU cores permit ~19.2 microseconds aggregate CPU/datagram before controls. This is necessary capacity arithmetic, not proof; native profiles determine whether single-owner design is adequate. The two-core process budget does not give a single packet owner two cores: record owner-thread CPU/headroom separately from auxiliary workers. Include SC-009 probes when measuring shared-queue purge/fairness and control-timer progress; do not infer adequacy from aggregate CPU arithmetic.

## Optimization and impairment experiments

After correctness/security pass, paired A/B: single vs mmsg; batch ceilings 1/8/16/32/64; GSO/GRO off vs auto; CUBIC vs BBRv2 (Reno diagnostic); pool/queue sizes; timerfd lateness versus coarse timer baseline. Evaluate tokio-quiche/custom-handler reuse against the same resource/pacing contract when relevant API constraints or measured bottlenecks justify it; this is not a mandatory pre-implementation experiment. Measure syscalls, wakeups, copies/allocations, CPU profiles, queue age, bursts, RSS and all delivery/latency criteria. Do not trade correctness/privacy/pacing for isolated throughput.

ADR 0022's output visit may submit an already eligible committed prefix before
native generation and the remaining eligible output afterwards. The shared
visit bounds remain 16 generated packets and 16 submission descriptors total,
counting descriptors offered even on partial progress or errors. Expiry/PMTU
preparation precedes the initial submission; FIFO commitment and pacing apply
to both calls. Comparisons record the two call opportunities without treating
them as two independent submission quotas.

Characterize 50/100 ms RTT, 0.1%/1% loss, asymmetry, bursts, reduced PMTU and changed network addresses. Report behavior/results, without imposing reference throughput promises on every impaired path. Privileged namespace/WireGuard procedures are reproducible optional test jobs; Remora does not configure VPNs or production hosts.

## Historical Phase 5 reference method (schema 2)

Before the user-approved Phase 8 revision, the reference profiles and runner selected schema-2
full methodology; `--plan-only` emits the exact 18 sustainable searches and
185 baseline/tunnel pairs. The seed randomizes mode order and pair scheduling.
The owned namespace runner supplies a full-duplex 1Gbit/s veth with 10ms egress
delay each way, records complete qdisc/link/feature settings and assigns four
disjoint available logical cores to each role. Generator/endpoint workers use
the remaining cores. No host NIC/qdisc/route/sysctl is changed.

Calibration starts at 1% payload bottleneck steps (10Mbit/s) and stops at the
first >0.1% delivered-loss point, retaining every accepted/rejected measurement.
Its declared 10-second loaded warmup/30-second sample is preliminary; each
selected 60-second loaded warmup/300-second paired baseline must independently
confirm sustainability. Generator scheduling/submission errors, >=0.9-core
single generator/receiver CPU, integrity/clock/status gaps invalidate capacity
calibration instead of being labelled Remora loss. Rates exclude encapsulation;
per-socket IO byte counters and netem qdisc byte counters retain wire information.
Explicit small per-session rounding is recorded in actual pps.

Full pairs cover capacity, 50% subcapacity and simultaneous 32-session/1200-byte
>=500Mbit/s each-direction resource workload. Validate each applicable direction
and pair separately: SC-003 >=80%, SC-004 <=0.1 added percentage points,
SC-005 <=5ms added p95, SC-007 <=128MiB/<=2 CPU cores per role; 1000-cycle churn
must settle <=10MiB above warmed RSS within <=60 seconds. Require all workloads,
>=five pairs, the exact declared intervals, valid loaded generator counts,
>=10 resource samples/second and >=1000 RTT samples/session, same-sender clock,
<10ms wall/monotonic drift, info diagnostics and zero recorded coverage/overflow.
Synthetic result tests reject one bad run without averaging and malformed or
incomplete evidence before numeric thresholds.

SC-007 requires **delivered**, not merely offered, payload throughput of at
least 500Mbit/s in each direction, measured as unique datagrams * payload bits
/ measurement seconds. Both the matched baseline and tunnel must carry that
load; the baseline must also independently meet <=0.1% loss. Each of all 32
Sessions must have unique delivery in both directions. Required schema-2
`unique_per_session` rows are ordered by source Session and must sum exactly
to aggregate `unique`; missing/unmatched rows invalidate evidence. Older
schema-2 artifacts lacking this field must be regenerated, not filled with
invented per-session counts. The runner offers slightly above 500Mbit/s to
allow the baseline's declared <=0.1% loss while preserving the unchanged
delivered-load threshold. Low CPU/RSS cannot qualify a run with insufficient
delivery. No averages or retrospective tolerance waive this gate.

Runner-owned namespace holders have no timed expiry and live until explicit
teardown; the full matrix alone takes at least 37 hours. Holder exit aborts
subsequent namespace work. Resource sampling rejects traffic/role exit inside
the measurement interval. After measurement, expected traffic-worker completion
is recorded explicitly and that worker is no longer sampled; missing /proc
paths are tolerated only when verified completion explains the race. No zero
resource samples are fabricated, and production-role exit remains invalid.

`--development-smoke` executes short, low-rate loaded 1/32-session methodology
checks with 200ms drain; it intentionally lacks release intervals/search/churn
and is **invalid for release qualification** even when process checks succeed.
Full reference execution and numerical final-revision acceptance remain T077.
The early schema-1 comparator continues to validate separately frozen manifest/
summary hashes, matching profiles/tool binaries, complete workload coverage and
per-metric units/bounds; release thresholds cannot be altered by that policy.
The `--traffic-binary` option preserves the exact retained test-only fixture in
prospective comparisons after harness source changes.


## Phase 8 reference generator pacing

Prospective reference cycles use `generator_pacing_quantum_us = 100` on both
native profiles, passed as `--pacing-quantum-us 100` to every generator/endpoint.
Raw endpoint JSON includes `pacing_quantum_us`; the controller rejects a missing
or mismatched value. Schema-1 endpoint output gains this additive field.
Existing functional commands default to zero quantum. Reference profiles from
before this change must be regenerated with the explicit field; their archived
results remain historical evidence and cannot be mixed into a new cycle.

Slot offsets are grouped down to 100us boundaries, preserving exact declared
counts across whole-second warmup/measurement boundaries. Service visits are
bounded to 32 slots, overdue visits do not idle-sleep, and the final 2ms guard
is finite. The cutoff is checked before each slot. All intended slots that
remain unscheduled still invalidate the run; no late catch-up or error waiver
is allowed. Baseline and tunnel use the same arrival pattern, source and
executable, recorded in immutable manifests. ADR 0012 records the observed
invalid cycles and rationale. This methodological change requires fresh full
calibration and pairs; it does not alter SC acceptance thresholds.


## Bounded reference method (schema 3, superseded prospectively by schema 4)

The user approved a prospective 60–90 minute budget on 2026-10-14. Profiles
select `methodology = "bounded-reference-v1"`, schema 3, three repetitions and
75 pairs: 18 subcapacity workloads × three repetitions, six 1200-byte capacity
workloads × three repetitions, and three simultaneous 32-session resource pairs.
The same 18 separate/simultaneous baseline searches cover all payload/session
combinations. Capacity at 128/512 bytes is not an SC-003 requirement and is
removed from paired capacity coverage, while its loss/RTT coverage remains.

Each search starts at 125 Mbit/s, doubles after valid loss-passing points until
1000 Mbit/s or a rejected loss point, then bisects the accepted/rejected bracket.
Stop after six points, or earlier on a valid 1000 Mbit/s point. Each point uses
2 seconds loaded warmup, 8 seconds measurement and 500ms drain. Retain every
point; any generator/integrity/clock/coverage failure invalidates calibration.
Select the highest tested loss-passing point, not an interpolated or exact
physical capacity. Each paired baseline independently confirms <=0.1% loss.
Brackets can remain coarse; reported rates are confirmed tested estimates.

Pairs use 3 seconds loaded warmup, 15 seconds measurement and 500ms drain;
resource measurements use 60 seconds. Pair order/seed, per-run thresholds,
generator CPU/clock validity, >=10 resource samples per measured second, RTT
probe counts, spread, owner-thread headroom and kernel memory remain recorded.
Three repetitions give limited variance evidence; 15-second intervals do not
establish long-term line-rate stability or the sensitivity of five-minute
measurements. No averaging hides a failed run. SC numerical thresholds and
1000-cycle churn are unchanged. T076 long functional tests remain distinct.

Nominal maximum intervals including common 2-second traffic startup are
18×6×12.5 + 72×2×20.5 + 3×2×65.5 = 4695 seconds (78.25 minutes).
This excludes parsing/process teardown/churn; the 5400-second budget reserves
30 seconds for cleanup. Check the budget during traffic and churn and terminate
owned process groups on exhaustion. Preserve incomplete evidence. Use fresh
profiles/sources/binaries; schema-2 historical suites retain their strict
original validator, and neither old invalid nor interrupted cycles can be
relabelled as schema-3 acceptance. See ADR 0013.

Schema-3 paired runs require at least 100 RTT probes per session (10Hz probes
nominally provide 150 during 15 seconds), versus 1000 for historical schema 2.
The smaller p95 tail sample is explicitly weaker evidence; insufficient probes
invalidate a run. Resource sampling still requires >=10 samples/second.


## Current completion-reserve method (schema 4)

Current profiles select bounded-reference-v2 / schema 4 and require
`generator_completion_guard_ms = 20`, passed as `--completion-guard-ms 20`.
Retain schema-3 matrix, search policy, repeat count, measured intervals, sample
requirements, 90-minute budget and all numerical thresholds. Retain exact
packet counts but uniformly compress generation into the measurement interval
minus 20ms; keep collection/resources over the full declared interval.
This explicitly changes the arrival waveform: active rate factor S/(S-.020),
while full-interval average load stays unchanged. Both members match. Raw
endpoint JSON records the reserve; reject missing/mismatched values. The first
slot outside the declared count is at the original cutoff, not inside reserve.
No late catch-up or waiver of missing slots is allowed. Defaults for functional
commands remain guard=0. See ADR 0014 for observed scheduling delays and risks.

Keep schemas 2/3 as strict historical validators. Schema 4 requires explicit
method/guard identity; regenerate all paired evidence for the changed waveform.
Source/binary/profile hashes must match. Diagnostic tail service-gap and last
submission-before-cutoff values expose scheduler/cutoff sensitivity. The reserve
does not guarantee success under arbitrary host load. Targeted 32-session
8/15/60-second verification precedes a fresh complete cycle.

## Prospective capacity interval extension (2026-10-16)

Before new T077 measurements, both native profiles explicitly select
`capacity_32_simultaneous_seconds = 60`. Schema-4 suites carry the same field;
both members of each of those three pairs measure 60 seconds. Other intervals,
probe requirements, numerical thresholds, calibration and repeat counts remain
unchanged. Without the extension, historical schema-4 suites still require
15 seconds for this row. Schema 2/3 cannot opt into the extension.

The nominal plan becomes 4965 seconds (82m45s), adding 270 seconds to 4695;
the total deadline remains 5400 seconds with 30 seconds reserved for cleanup.
Insufficient RTT samples still invalidate a run. Historical failures cannot
be relabelled. See [ADR 0020](../../../docs/adr/0020-capacity-rtt-measurement-window.md).
