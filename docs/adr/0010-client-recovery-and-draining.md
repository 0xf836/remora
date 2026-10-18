# ADR-0010: Bounded client recovery and draining connection ownership

Date: 2026-10-12. Status: implemented; deterministic, native recovery and shutdown checks passed.

## Context and options

The MVP could retire a failed connection but lacked verified client replacement.
Choosing an arbitrary connection for new senders cannot distinguish primary
from GOAWAY draining state. Reusing an old connected socket also retains an old
local address after a network change. Local PTO/probe sends must not hide a
blackhole by pretending the peer has made progress.

Options were reusing the old transport/socket, replaying queued application
traffic, adding per-attempt tasks, or retaining the existing exclusive owner
with explicit primary selection and finite replacement state. The first three
do not preserve generation, security, unreliable delivery or bounded work.

## Decision

Keep one current-thread reactor. A mapping has an explicit primary generation,
one reconnect deadline and one handshake in flight. Each client connection owns
a fresh connected outer UDP socket; draining and primary connections/sockets
both debit the existing connection/FD budgets. Retiring one closes its socket,
adjusts other socket indices and folds its numeric IO counters exactly once.
The configured application listener remains bound through every attempt.
Client CID routing rejects a packet received on another connection's socket.

Client state is Starting -> Handshaking -> Negotiating -> Active, with fresh
Recovering -> Handshaking transitions and terminal Rejected/Stopping/Stopped.
Active describes verified TLS/settings, not successful target responsiveness.
Replacements repeat the same configured CA/name/ALPN/capability verification,
then each new request repeats Bearer authentication and default-deny target
authorization. No early data, trust downgrade, private ping or transport fallback
is added. Incoming unavailable/setup data is counted and dropped, not retained.
Failure/replacement cancels old callbacks and discards both cancellable data
and old unsent ciphertext; nothing is transferred to a new generation.

Retry has a configured >=100ms floor plus uniform jitter capped so total delay
never exceeds 2000ms. Its exponent saturates; entropy draws have a finite retry
bound. Connection admission failures retain the same schedule. Transient
request refusals additionally impose a mapping-wide setup cooldown. TLS/name,
credential/policy and capability rejection latch new admission until restart.
Before any accepted Session a permanent failure may exit; after startup it
reports Rejected while preserving unrelated accepted flows. Flow cleanup,
changed source tuples and transient recovery cannot clear the latch.
The 2026-10-13 review correction cancels every not-yet-accepted client Session
when latching rejection, recording the mapping refusal before retiring its
opaque generation. Queued CONNECT submission and delayed-success acceptance
also check the latch. Previously accepted Active flows remain; a delayed valid
200 for cancelled setup is discarded without inventing a malformed-peer error.

For active flows, schedule `send_ack_eliciting` every 2000ms of otherwise idle
generated transport output, then use ordinary QUIC pacing and send ownership.
This neither emits application data nor updates Session UDP inactivity. A
separate 10000ms peer-progress watchdog uses pinned quiche's fresh processed
packet counter, which increments after decryption/frame processing/new packet
number acceptance. Local PTO/probe output, duplicates and invalid input do not
reset it. Its expiry and probe deadlines share timerfd with quiche's unchanged
5000ms idle/PTO behavior and 5000ms handshake/settings/setup deadlines.

GOAWAY retains accepted lower-ID Sessions on their original bounded connection.
Only new/excluded requests select a fresh primary; their original triggering
payload is dropped. Repeated cutoffs must obey the native contract. Resource
pressure does not evict lower flows to bypass budgets. A max_connections=1
configuration cannot overlap a replacement with its draining connection; it
waits under bounded retry/admission pressure. Defaults allow overlap.
Validated NAT rebinding stays on its original generation; a failed local
network path can trigger a fresh socket/connection without changing the target.

Signals are registered before reactor socket/TLS initialization and checked
before each owner turn. Stop cancels setup/retry work, ends admission, discards
unsent data and releases role sockets before consuming the shared <=5s shutdown
budget. One fixed `stopped/0` terminal status publishes release even before the
first periodic snapshot. The CLI adds no second DNS-worker wait after that
budget: bounded uncancellable libc lookups lose all reactor callbacks and end
at process exit. No post-stop payload transmission is promised.

## Consequences and verification

Configuration/wire fields remain unchanged. Validation now enforces the already
contracted retry cap: values above 2000ms fail check-config. Schema-1 snapshots
add fixed recovery state/counters; watchdog peer_timeout/1 identifies the
application watchdog, separately from native idle timeout observations. Normal
shutdown can include a terminal stopped/0 record. No dependency or unsafe code
is added, and syscall/FFI ownership remains in the UDP module.

Fake-clock and real quiche/H3 tests cover minimum jitter, long capped retries,
PTO/probe output without peer progress, fresh generations, cancellation/stale
callbacks, permanent admission latching, GOAWAY placement and resource pressure,
validated rebinding and UDP-versus-transport inactivity. Process tests and the
owned namespace runner cover ten-second blackhole/restart, changed address,
long outage, renewed security rejection and both signals during lifecycle/setup.
The prospective T063 reference and numeric policy were frozen before production
changes; loaded-path comparisons and idle probe evidence are separate. Full
release throughput/resource/duration and ARM64 qualification remain T077.
See [recovery evidence](../validation/recovery.md) for final hashes, timing
origins, security assessment, retained failures and comparison outcomes.
