# ADR-0005: Startup validation and bounded owner-local primitives

Date: 2026-10-11. Status: implemented in Phase 2; forwarding integration pending.

## Context

T005–T009 need immutable configuration, finite storage and safe output before
network input can create sessions. The existing production dependencies already
include BoringSSL through quiche, and the lockfile already includes rustix and
time. A new runtime, proxy framework or cryptographic parser is unnecessary.

## Decisions

- Use the same `boring =5.2.0` backend directly to parse in-memory certificate
  bundles and private keys, and compare the leaf public key with the private
  key. An explicit callback prevents prompting; any callback invocation rejects
  the key, including successful decryption with an empty password. Normal peer
  certificate/name/expiry verification remains T018/T019.
- Use rustix 1.1.5 safe `openat`, UID and `getrlimit` APIs for startup only.
  Each path component opens against a held directory FD with `NOFOLLOW`;
  `NONBLOCK` prevents a FIFO from hanging before the regular-file check.
  Private-file ownership/mode checks use the opened inode, then bounded reads
  load the bytes. No locally authored unsafe is introduced. Packet syscall
  code still belongs exclusively to the reviewed `udp.rs` boundary.
- Keep the loaded role and settings behind shared accessors, with one owning
  startup object per policy generation. Secret-bearing types have no Debug or
  Serialize implementation. Errors retain only fixed field/reason values;
  upstream TOML, OS and crypto error strings are discarded.
- Bound startup input: 256 KiB configuration, 4 MiB credentials, 1 MiB per PEM
  input, 65 bytes token including optional LF. The initial parser also caps
  listeners at 16, identity IDs at 64 ASCII alphanumeric/`_-.` bytes, and CIDRs
  at 128 per target rule. Existing 128-identity/128-target limits remain.
  These are parser/resource ceilings, not new service features.
- Canonicalize static host/IP/CIDR rules in configuration. Actual credential
  comparison, default-deny authorization and all-answer DNS validation remain
  T010/T011. URI validation here checks startup shape/proxy identity; full
  RFC template encoding and wire validation remain T012/T013.
- Calculate a conservative FD ceiling: inherited/open startup FDs + listeners
  + maximum connections + relay sockets (including cleanup) + twice pending
  setup + 8 diagnostic FDs + 32 runtime/control FDs. Compare against the current
  soft `RLIMIT_NOFILE` without changing it. Admission must still hold permits;
  inherited FDs must remain stable during startup.
- Use reactor-local Rc/RefCell ledgers and non-cloneable RAII reservations.
  Fixed boxed slabs stay charged by full capacity while lent or cached; returning
  a slab clears bytes and uses a pre-reserved free-list slot. Descriptor/queue
  metadata capacities are finite and reported separately from payload capacity;
  trimming an entirely idle pool releases its metadata too. Kernel/native/control
  memory and actual RSS remain separate measured budgets, not hidden payload.
- Keep application and generated-output descriptors immutable, with checked
  ranges, original generations and deadlines. Native DATAGRAM count reservations
  are separate by direction and share one byte cap. Integration will enforce
  cancellation, commitment and connection FIFO ordering in later tasks.
- Emit only typed JSON events/snapshots; never register an arbitrary upstream
  tracing subscriber. Reuse time 0.3.55 for UTC RFC3339 timestamps and generate
  random process epochs with monotonic opaque local IDs. Fixed drop classes
  have explicit direction counts and checked increments.
- Use two bounded nonblocking writer channels, with record/byte charges retained
  until writing completes. Ordinary events use at most 768 KiB/768 records;
  status has 256 KiB/256 records; total output is 1 MiB/1024 records. Either
  byte or count exhaustion rejects admission. Status has record-boundary
  priority. Formatting uses a 64 KiB scratch bound; the writer stack is 128 KiB,
  within the separate 512 KiB storage reservation alongside finite bookkeeping.
  The remaining 512 KiB is reserved for future qlog filtering: no qlog is active.
  A blocked writer cannot block the owner; flush waits have a deadline, failures
  are counted, and incomplete flush is reported. One writer is allowed per owner;
  a detached blocked thread is reclaimed at process exit, never respawned in a
  loop. Terminal retention and acknowledgement guarantees remain T025/T026.
- Provision tests with Python standard-library randomness/hash and the OpenSSL
  CLI, not new production dependencies. Atomic mode-0700 directory creation
  refuses existing outputs. Three simultaneously held loopback UDP sockets
  select distinct ephemeral ports; releasing them creates the usual bind race,
  so future runners must bind immediately and recreate fixtures on collision.
  Tests own a private parent directory and kill/reap only their own process
  groups with bounded waits. No routes, firewall, VPN or host settings change.
  Following review F1, the child guard observes leader exit with safe rustix
  `waitid(EXITED | NOHANG | NOWAIT)`, signals the owned group while the leader
  remains unreaped, then collects the direct child's status. Retaining the
  leader pins its identifier through group cleanup; a cached status prevents
  further signalling after reap. This requires exclusive wait ownership and
  controlled children remaining in their group: no concurrent `wait(-1)`
  reaper, `setsid` or group escape. Orphaned descendants are reaped by the host's
  init; tests distinguish an exited zombie from a live process holding sockets.
  The guard bounds its wait, not kernel completion of SIGKILL for a process
  stuck in uninterruptible I/O. API evidence: [rustix 1.1.5 waitid](https://docs.rs/rustix/1.1.5/rustix/process/fn.waitid.html)
  and its [NOWAIT option](https://docs.rs/rustix/1.1.5/rustix/process/struct.WaitIdOptions.html#associatedconstant.NOWAIT).
- Keep parser property checks test-only, beside the private parser boundaries.
  The startup document parse is factored into a private function called by the
  existing secure loader; no public bypass or new dependency is introduced.
  Eight 128-case properties use a fixed seed and bounded inputs (up to 4096 raw
  bytes or 511 Unicode characters) for schema/credentials, host/authority and
  IPv4/IPv6 CIDR semantics. Structured cases complement malformed arbitrary
  inputs and assert fixed privacy-safe errors. File-mode/open/size checks remain
  integration tests; this is normal foundation verification, not the later
  networking fuzz campaign.

## Security assumptions and consequences

Provisioning directories and the executing user/root are trusted. No-follow
descriptor traversal prevents symlink substitution, but cannot prevent a trusted
owner editing an already-open inode. Provision files by atomic replacement;
loaded configuration does not support live reload. Hard links do not bypass
ownership/mode checks. Public CA/chain files need no private-file mode restriction.

PEM parsing and key matching are startup validation, not TLS peer authentication.
Buffers have exclusive leases and no payload diagnostics; clearing on return is
a conservative cross-owner safeguard, not a throughput improvement claim.
Partial/broken diagnostic writes can leave incomplete output and lose coverage;
the later status retention protocol must not claim delivery without acknowledgement.

Direct dependencies expose capabilities already in the locked graph; no package
version changes. The ordinary release dependency audit remains required. This
record is the implementer's explicit boundary/security review, not an independent
reviewer audit or evidence of networking/performance acceptance.

API evidence: locked source for `boring::x509::X509::stack_from_pem`,
`PKey::private_key_from_pem_callback`/`public_eq`, `rustix::fs::openat`,
`rustix::process::{geteuid,getrlimit}`, and `time::OffsetDateTime` was inspected.
The [configuration](../../specs/001-masque-udp-tunnel/contracts/cli-config.md)
and [diagnostic](../../specs/001-masque-udp-tunnel/contracts/diagnostics.md)
contracts remain the authority; [validation evidence](../validation/phase2.md)
separates implemented primitives from later integration obligations.
