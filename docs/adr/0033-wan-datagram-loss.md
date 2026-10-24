# ADR 0033: Bound WAN loss amplification without discarding young datagrams

Status: Implemented; Phase 15 laboratory checks, paired soak and independent
read-only final review complete (APPROVE), 2026-10-24. This is not release or
live-WAN acceptance.

## Context

The owner reproduced 87.37% endpoint loss at 200 Mbit/s with 1200-byte iperf
payloads and approximately 54 ms RTT. Sender snapshots recorded 77200
`queue_full` and 21387 `queue_expired` drops without kernel overflow or PMTU
errors. A separate 1000-byte run lost 89.79%. The current CUBIC window and
32-packet application queues cannot absorb all offered traffic during recovery.
An earlier controlled five-outer-packet experiment already demonstrated loss
amplification; historical CUBIC/BBR2 results used different executables.

The original native outgoing DATAGRAM expiry retained the earliest historical
deadline until the queue emptied and purged every remaining element. This
discarded younger data, including after the original oldest packet was sent.
Expiry invoked during generation also lost its diagnostic count.

## Decision

Keep QUIC congestion control, the exclusive owner, bounded queues, cancellation,
generation isolation and immutable committed ciphertext. Do not retransmit
application datagrams or add a reliable fallback.

Track the original expiry of each outgoing native datagram. Purge only expired
entries and recompute the next deadline after every mutation. The pinned quiche
`dgram_purge_outgoing` visits its VecDeque in FIFO order; use ordinal metadata,
not payload equality, and test identical payloads with distinct lifetimes.

Accumulate expiry results in the connection until the reactor consumes them,
including expiry during zero-quota preparation, generation errors and teardown.
The returned count of `expire_native` is informative and is not a second source
of diagnostic accounting. Retain separate PMTU and outer-loss meanings.

Full native input is backpressure, not a reason for an immediate identical
application enqueue continuation. Previously admitted input, physical output,
ACKs and original deadlines retain the responsibility for progress.

The pinned gcongestion integration needs a different pacing boundary from
classic CUBIC. `get_packet_send_time` reads the pacer after `on_packet_sent`
advances it; the pinned tokio-quiche worker obtains the release decision before
calling send. Capture that pre-send decision for BBR2. Stop generation when
the next release is beyond quiche's recommended bounded future horizon (also
capped at half the committed output residence). Retain a connection-owned
generation timer so cancellable input resumes without busy polling. Do not
extend the original ciphertext deadline or alter classic CUBIC's SendInfo.

This additional correction follows measured BBR2 failure at 100 ms RTT: all
three original integration runs repeatedly retired connections with transport
code 1001 and lost 99.99784% of offered datagrams. A bounded low-rate native
pacing regression fails before the correction because ciphertext was generated
beyond the recommended horizon. This evidence does not itself qualify BBR2.

Freeze the acceptance policy before candidate measurements. Compare CUBIC and
BBR2 on the same executable and topology, including cold traffic, rate changes,
exact five-packet loss, ACK disturbance and bounded owner pauses. Change defaults
only after the selected profile passes the frozen WAN and existing regression
checks. No historical executable's qualification transfers to the new binary.

## Compatibility and consequences

There is no wire-format change. Explicit configuration values retain their
meaning; native outgoing expiry becomes selective rather than purge-all. The
incoming native receive epoch remains conservative unless a separately tested
change is needed. The owner's 200 ms transitional latency budget is an end-to-end
laboratory criterion, not an independent allowance for every queue.

The implementation must preserve memory accounting for metadata as well as
payloads, fair progress, pacing and the original committed-output deadline.
Larger queues alone cannot guarantee lossless UDP above sustainable capacity.
The generic CUBIC / 32-packet / 20-ms defaults remain in place. The selected WAN
profile is explicit rather than a silent configuration migration.

The first focused 32-session/875-Mbit/s comparison rejected 8192/190 with the
default classic startup setting: selected endpoint loss was 1.49/2.87%, while
the same candidate with original limits matched the control. Early ENOBUFS
and outer QUIC loss coexist with a 25–29-ms path RTT and a slow rise from about
2.2 to 3 MB cwnd. The queues clear roughly ten seconds after measurement start.
This did not establish HyStart as the unique cause. Bounded startup-exit
labels/cwnd and the same-executable `transport.hystart=false` experiment were
added. Disabling it worsened that screen and was not selected. The default
remains true; a CSS exit label in the pinned backend can also accompany loss.
The later pressure-retry comparison established a separate local discard
mechanism while preserving the original failed profile results.

The laboratory WireGuard variant provisions keys/interfaces only inside the
runner's verified disposable namespaces. It explicitly checks handshake and
bidirectional counters with bounded ICMP probes before the measurement start;
it does not use a loaded bandwidth warmup. The plaintext relay must explicitly
use 1400/1452 buffers for encrypted WG payloads; its historical 1200/1350 profile
remains unchanged for older performance contracts. Missing/failed control data
cannot establish an added-delay or added-loss pass.

### Local output-pressure retry

The buffer sweep on the original netem-2048 reference topology retained failed
repeats with both outer-4-MiB-only and relay-1-MiB-only requests. Enlarging netem
removed pressure in a diagnostic screen but changes the topology. Keep default
buffers and the original regression topology, and test bounded retry of the
exact unaccepted outer ciphertext instead.

The single owner retains immutable ciphertext after ENOBUFS, keyed by its
existing `(Generation, sequence)`. Retry at most once per 1-ms timer interval;
a 5-ms window anchored to the first failure discards only that still-unaccepted
front on exhaustion. Original generation/pacing/expiry timestamps are unchanged,
and original ciphertext expiry may fail the connection earlier. A later EAGAIN
still leaves the maximum-window deadline armed. Never replay an accepted prefix,
use shared-socket POLLOUT as a retry clock, or suppress another connection.

One identity check governs service, readiness and deadlines. It ignores stale
state after critical H3 output clearing or connection replacement; explicit stop
also resets it. Scoped Session close still preserves committed mixed ciphertext.
Count failed ENOBUFS submissions separately from retry-window discards, without
inventing application attribution. No public retry setting or dependency is added.
The prospective constants are retained in `wan-loss-pressure-retry.json`.

This bounds only the extra local retry mechanism. Adding the residence and retry
windows is not proof
of the full 200-ms added-delay gate: pacing, owner lateness and kernel buffering
still require the unchanged end-to-end measurements. Preserve failing-before
regressions and compare the candidate with the pre-retry executable before any
broader performance/WAN qualification or default promotion.

### Selected residence and explicit CPU tradeoff

The prospective 190-ms profile had a valid 60-ms/1000-byte cold loss failure of
1.0064%, with zero steady/outer QUIC/kernel RX loss and local queue expiry.
Keep that result. A declared 194/195-ms refinement preserved all original
loss/latency thresholds and the same binary/buffers/pool. Both passed their
three-repeat screen; 194 ms had the lower measured latency tail and was selected.
The complete 54-run normal WG matrix then had zero steady loss and maximum
measured added one-way delay 196.523 ms. Disturbance and rate-step matrices and
fresh 32-session profile-delta pairs also passed. The final paired 1800-second
WG soak passed with about 199.94 Mbit/s each way and a maximum added delay of
194.610 ms. Residual loss after the first two seconds was 104 / 34 datagrams
out of 37,457,734 offered steady slots per direction; it is not called zero.
Whole-interval resources and counts reconcile; the final independent soak
audit approved completion of this laboratory increment.

The owner explicitly accepted the observed approximately 0.05–0.06-core CPU
increase for this increment. The separate regression-v2 policy reports relative
paired CPU without blocking acceptance; absolute <=2-core CPU, <=128-MiB RSS,
paired RSS, throughput/loss and latency limits remain. The original strict CPU
failure stays failed. Reassessment of retained rows is explicitly retrospective,
not a prospective strict-CPU pass. This exception is scoped to this increment
and does not transfer to a release or a future critical-path change.

The steps-100 checker initially applied a >=190-Mbit/s startup target to a
deliberately offered 20-Mbit/s phase. Its applicability was corrected, with a
negative regression proving the final high-rate requirement remains enforced.
All other recorded check values remain identical; original gates/failure are
retained and the corrected assessment is not a new traffic measurement.

Live deployment, publication and operator WAN acceptance are outside this
increment. The owner explicitly requested a read-only reviewer after laboratory
verification. See `docs/validation/wan-loss.md` for progress and evidence.
