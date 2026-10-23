# Operator logging validation — 2026-10-23

Scope: owner-approved [ADR 0032](../adr/0032-operator-log-output.md). This report
supersedes the initial key/value text experiment from the same session. Default
operator text is now separate from explicitly selected legacy JSON/status.

## Initial implementation results (before code review)

- `cargo test --locked --all-targets --all-features`: **227 passed**, three ignored.
  Bins: 80 library, 32 E2E, 26 I/O, five lifecycle, 22 protocol, 62 security.
- `python3 -m unittest discover -s tests -p 'test_*.py'`: **113 passed**.
- `cargo build --locked --release`: passed.
- `cargo build --locked --release --features qlog`: passed.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy --locked --all-targets --all-features -- -D warnings`: passed.
- `git diff --check` and `bash -n scripts/validate/fixtures.sh`: passed.
- `systemd-analyze verify examples/remora-server.service`: passed. This is static
  unit validation, not execution under a live systemd service.

Native socket tests run outside the sandbox. The Python restricted invocation
could not create the two existing UDP test sockets; its unrestricted repeat
passed. Earlier Rust iterations exposed test synchronization before handshake/
signal registration and an obsolete exact CLI-error expectation; these were
corrected without weakening forwarding/privacy assertions. Final full-run logs
are in ignored `benchmarks/results/operator-logging-validation-20261023/` with
source hashes and final qlog executable identity. No generated credentials or
private fixtures are retained there or staged.

## Behavior checked

- Default human output: startup, transport readiness, accepted tunnel, orderly
  stop; unchanged output during healthy idle and empty stdout.
- A size-fault burst emits one useful WARN, valid UDP continues, and token/payload/
  target-name sentinels and machine-field arrays are absent from operator logs.
- Initial authentication rejection explains HTTP 401, the corrective action and
  exit; public-server refusals do not produce default WARN/ERROR noise.
- A real local server crash/restart produces one loss notice across failed setup
  attempts. Successful replacement handshake alone does not announce recovery;
  actual accepted tunnel/forwarding emits one restoration notice.
- Synthetic owner transitions spanning two minutes emit one outage notice; later
  permanent mapping refusal is emitted once and explicitly preserves the possible
  continued operation of already accepted flows. Existing native recovery tests
  cover the actual accepted-flow preservation invariant.
- 10,000 synthetic per-request failures do not build an operator correlation
  ring or output queue. Two hours of synthetic idle produce no status output.
- DEBUG saturation cannot consume important-event admission or all queue slots;
  human records remain below 1024 bytes. Suppression has no per-peer map/history.
- A genuinely unread, small stderr socket does not stop UDP forwarding. SIGTERM
  exits with incomplete-output status within the bound, without synchronously
  writing again to the blocked sink.
- Journal framing uses real priorities only for a matching stream identity;
  standalone output retains UTC time. Malformed logging fields remain private.
- Existing JSON/status ACK, overflow/failed-write, qlog, lifecycle, parser/property
  and native syscall cases pass. Explicit JSON without healthy snapshots still
  retains correlated failure/final status. Operator mode makes no SC-010 claim.

## Real legacy collector smoke

`./scripts/validate/bench.sh --profile /tmp/remora-logging-smoke.toml --output
benchmarks/results/operator-logging-smoke-20261023 --remora-binary
target/release/remora` completed **valid-low-rate**, one loopback forward run,
128-byte payloads, 100 pps, one-second warmup and one-second measurement. The
exact profile is copied into that output directory.

Both roles produced schema-1 final snapshots with zero coverage loss, status
write failures, diagnostic drops and counter overflow. The runner reports zero
scheduling/errors and integrity errors. Parser regression cases reject absent,
human-text, malformed or incomplete-schema output instead of fabricating empty
metrics; deliberately blocked collection keeps its explicit incomplete exception.

This short smoke establishes executable/collector compatibility, **not** a new
throughput baseline, zero delivered-loss claim, paired regression acceptance or
SC-010 qualification. Historic measurements retain their original binaries and
profiles. No full reference/WAN/WireGuard performance rerun is claimed.

## Unavailable / not rerun

The three ignored Rust cases require the pinned independent Go peer (two tests)
or root/UID delegation with POSIX ACL support (one test). They were not run.
No new independent interoperability, Miri/ASan/fuzz campaign, hosted CI, live VPS
or live journald-retention validation was performed for the initial increment.
An independent implementation review subsequently requested changes; see below.


## Implementation review corrections — 2026-10-23

The independent read-only audit found three P2 defects and a qlog admission
limitation. These follow-up changes address them without a logging framework:

- JSON fatal output again emits `runtime failed: <typed RuntimeError>`; operator
  errors keep human messages. A blocked-capture executable test verifies the
  exact marker. The shared collector parser is tested on raw marker lines.
- Reconnect scheduling distinguishes graceful GOAWAY from actual failure. The
  native wire GOAWAY case runs in both profiles; operator cases separately cover
  empty draining retirement versus actual loss with accepted streams remaining.
  Neither graceful replacement nor empty retirement invents outage/recovery.
- Temporary client HTTP 502/503/504 and setup expiry at live transport emit one
  scoped warning. An accepted request clears the fixed flag and reports initial
  acceptance or successful renewed setup; accepted sibling sessions remain active.
- Operator queue headroom is enforced centrally for all low-priority producers,
  including qlog, by both bytes (16 KiB) and records (16). JSON-mode budgets are
  unchanged. Tests saturate large low-priority records and the real qlog filter.
- Cached callback times are normalized only for operator admission; retry,
  lifecycle and protocol deadlines are not changed.

### Real blocked collector and existing validator

`benchmarks/results/operator-logging-review-blocked-20261023/` records a real
loopback simultaneous run: 128-byte payloads, 100 pps per direction, two-second
warmup and five-second measurement. Both directions offered/delivered 500/500,
with zero measured loss, scheduling errors and integrity errors in this short
sample. Both writers were observed blocked; both roles exited 1 in about 1.01 s
with exactly `["DiagnosticCollectionIncomplete"]` in `role_runtime_failures`.

The unchanged `diagnostic_regression.valid(row, profile, "default", True)` accepts
the actual collected row. `validator.json` records that check and the summary
hash. This verifies the previously missed executable/collector/validator path,
not a full pinned matrix, new performance baseline or complete SC-010 capture.
The blocked run deliberately reports incomplete collection.

### Verification and harness limits

- `cargo test --locked --all-targets --all-features -- --test-threads=1`:
  **231 passed**, three ignored (82 library, 33 E2E, 26 I/O, five lifecycle,
  22 protocol, 63 security). The ignored prerequisites remain those listed above.
- `python3 -m unittest discover -s tests -p 'test_*.py'`: **114 passed**.
- Default and qlog release builds, all-target/all-feature Clippy with warnings
  denied, format check and `git diff --check`: passed.
- Final passing logs, source hashes and qlog executable identity are retained in
  ignored `benchmarks/results/operator-logging-review-fixes-20261023/`.

Two parallel verification attempts had fixture-level failures: one selected UDP
port returned EADDRINUSE; another target received an unexpected 43-byte datagram.
The payload-integrity assertion was retained. The latter packet's origin was
not attributed, and no causal production or harness fix is claimed from retries.
Final verification uses `--test-threads=1`, avoiding concurrent test cases while
retaining each scenario's own multi-session/concurrent traffic. This is not a
claim that parallel harness interference has been resolved. Earlier failure logs
remain under /tmp; final passing evidence is retained separately.

The implementation-review fixes have not yet had an independent re-review.
