# Phase 8 safety verification — 2026-10-13

T074 is complete for the implemented native x86_64 profile. This is an author
safety audit with automated evidence, not an independent reviewer audit.
The original qualification below kept production Rust and its dependency lock
unchanged. The 2026-10-15 packet-service candidates subsequently modify only safe
owner/readiness scheduling in `src/reactor.rs`/`src/session.rs` and bounded
idle-cache retirement in `src/buffers.rs`. `src/udp.rs`, unsafe operations and
dependency locks remain unchanged.
Current-candidate native regressions and final requalification are recorded in
[release status](release.md); the original results below retain their input hashes.

Run `scripts/validate/safety.sh benchmarks/results/safety-UNIQUE` with the pinned
production compiler and test-only nightly-2026-10-12/Miri available through
rustup. Miri applies to `tools/safety`, a separate unpublished package that
includes the **actual** `src/buffers.rs`. Its small `config::Limits` adapter
supplies only field types; it does not implement buffer, ledger or queue logic.
The adapter's scope is explicit: config validation is covered by native
properties, not by these Miri tests. Both locked native ownership tests and Miri
passed. Full all-feature library properties/components and native IO/lifecycle
suites also passed in `benchmarks/results/phase8-safety-20261013`.

The two Miri cases exercise unique slab leases, failed count/byte/arithmetic
reservations, invalid slice ranges, zero-length datagrams, queue saturation,
cancellation/drop, idle trimming while a borrow is held, unrelated slab reuse,
and exactly-once accounting return. Miri checks Rust memory/aliasing invariants
for these exercised paths; it cannot establish safety of Linux syscalls,
BoringSSL, all possible state sequences or real network operation.

The 2026-10-15 cache candidate adds a third source-included Miri regression.
All three pass in
`benchmarks/results/phase8-performance-fix-20261015/candidate9-miri.log`.
It verifies bounded retirement and reserve reuse while a live slab's bytes are
borrowed, preservation of lent leases and preallocated return slots, and exact
reservation release. A matching native IO regression also passes. The method
drops only idle owned `StoredSlab` values, keeps descriptor capacity for returning
leases and adds no unsafe operation. These targeted checks do not substitute
for current-candidate full qualification.

## Syscall/FFI audit

Reviewed `src/udp.rs` at SHA-256
`57e039ff069a57a7b37bc845aba0335fe29482c1c3b5cbb5a3c0a4dec3c4f3b6`.
The necessity and local SAFETY comment at each unsafe operation were checked.

| Boundary | Invariant checked | Verification |
|---|---|---|
| C descriptor zero initialization | Only integer, byte storage and raw pointer fields; populated before calls | Native descriptor-reuse/syscall tests |
| Socket options / MTU queries | Owned live FD; exact aligned writable scalar size; returned size/value validated; no retained pointers | PMTU tests, separate families and socket buffer tests |
| recvmsg/recvmmsg | Boxed batch metadata stable for synchronous call; disjoint live exclusive slab leases; bounded batch; output lengths, address family and truncation validated | Empty/mixed/truncated receive, pktinfo, ancillary properties and ASan fuzz |
| sendmsg/sendmmsg | Immutable payload bytes live through synchronous call; stable address/control spans; only returned accepted prefix advances | Partial/EAGAIN/EINTR/EMSGSIZE and commitment/cancellation tests |
| Error queue | Initialized bounded control storage; kernel-reported length checked; quoted payload ignored; metadata cannot authorize a destination | Native error-queue/PMTU tests and ancillary fuzz |
| sockaddr/ancillary decoding | Family/min/max length validation; integer repr(C) values read unaligned; checked offsets and lengths | Malformed/truncated/duplicate metadata cases and fuzz |
| pktinfo construction | usize-aligned initialized control storage; checked total capacity; aligned header and explicit unaligned payload writes | Wildcard reply source-selection tests |
| timerfd / clock_gettime / read | Unique OwnedFd transfer; exact initialized timespec/u64 spans; checked monotonic conversion and bounded retry/read outcomes | Timer deadline/rearm/readiness tests |

No new unsafe operations were introduced. The private generic `read_value<T>`
is used only with integer-field libc structs/u32; its all-bit-patterns-valid
precondition remains a maintenance obligation. Payload buffers have initialized
capacity, and raw pointers do not escape a synchronous syscall. Reusable raw
headers are rebuilt before use; stale descriptors are not dereferenced later.
Safe buffer code expresses unique ownership through non-cloneable reservations
and slabs; queue/connection generations preserve association identity.

Selected GSO/GRO remains effective off, with configured-off or not-implemented
reasons. There is no aggregate splitter or offload buffer ownership to certify.
Unexpected UDP ancillary/GRO markers are rejected and fuzzed. This report does
not imply an enabled offload optimization or native ARM64 safety qualification.
Kernel-induced overflow and privileged route changes retain their separate
runner obligations; Miri does not replace those checks.
