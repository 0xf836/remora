# Follow-ups to the independent review

Date: 2026-10-26. All changes were local. The owner's server and the VMs were
not used in this iteration. The independent audit covers the previous state;
a second independent review of the new changes is not claimed.

## Implemented changes

### Kernel telemetry completeness in collectors

`benchmark.py` and `reference.py` now retain a separate summary field,
`kernel_rx_overflow_complete`. It is true only when both role snapshots
explicitly report complete statistics and zero query errors. Incomplete,
unknown or legacy snapshots without the new fields produce false. For the
direct baseline, the field is null because the metric does not apply.

Incomplete data cannot qualify as a successful tunnel measurement.
`calibration_invalid_reasons`, low-rate status and WAN loss/regression gates
check this indicator separately from `coverage_lost`. A WAN tunnel arm cannot
claim the baseline exemption. Historical JSON snapshots remain readable, but
missing fields cannot establish the absence of kernel UDP drops.

Added checks cover a missing field, incomplete=true, a nonzero error count,
invalid types, a missing role snapshot, the direct baseline and propagation
of an actual JSON field through the parser to the summary gate.

### Traversing live sockets without rescanning a HashMap

`Runner` now has a BTreeSet of logical keys for live sockets. A client outer
socket uses its connection generation as the key; a relay uses its session
generation; server outer sockets have immutable indices. Socket ownership is
unchanged, raw fds are not used as identities and no worker or task is added.

The registry is bounded by existing resource limits. Entries are added when
sockets are created and removed when they close; stale entries cannot
accumulate. Periodic collection resumes an ordered range after the last key
without rescanning a HashMap prefix. Each sweep freezes its final key; sockets
added beyond that boundary wait for the next sweep. One periodic portion
processes at most 16 keys. Changing client socket indices does not invalidate
other connections' keys. Text mode does not populate the registry.

A newly created socket may still wait until the next sweep. This does not
promise a strict wall-clock interval under arbitrary load; the cost at high
configured limits was not measured separately.

### Final queries and tests of actual closure paths

Added targeted Rust tests:

- `rx_drop_real_client_connection_retirement_preserves_other_socket_keys`:
  checks a final increment and read failure through `remove_connection`, key
  removal, transfer of I/O statistics and the remaining socket's correctness
  after index renumbering.
- `rx_drop_real_relay_retirement_samples_final_delta_and_error`:
  creates the relay through production `setup_done`, then uses the actual
  `close_session` path to check the final increment or error and release of
  the session and key.
- `rx_drop_shutdown_closes_remaining_sockets_and_records_failure`:
  checks that the production shutdown operation `close_udp_sockets` samples
  and releases remaining outer/listener sockets, records an error and permits
  repeated cleanup without double counting. Both JSON and Text are covered.
- `rx_drop_sweep_keeps_endpoint_and_progress_across_churn`:
  checks multiple portions, removal of a visited key without shifting the
  cursor, and deferral of a new key beyond the current sweep endpoint.

The final outer/listener closure operation was extracted from `run` into a
method shared by production and the test. The new test exercises that
operation rather than directly calling the accumulation helper. End-to-end
signal/shutdown tests also passed in the full suite; precise injection of each
last increment is checked locally inside the reactor.

### Quota and readiness wording

The contract and ADR explicitly distinguish the 16 queries in a periodic
portion from additional final queries during retirement/shutdown. The latter
are bounded by overall resource limits. Initial failed readiness probes from
the previous VM campaign remain in the historical report; their outcomes were
not relabelled and no new campaigns were run for them.

## Validation of the current state

| Check | Result |
| --- | --- |
| `cargo test --locked --lib rx_drop -- --nocapture` | 7 passed |
| `cargo test --locked --all-targets --all-features` | 246 passed, 0 failed, 3 ignored |
| `python3 -m unittest discover -s tests -p 'test_*.py'` | 155 passed |
| `cargo clippy --locked --all-targets --all-features -- -D warnings` | PASS |
| `cargo build --locked --release` | PASS |
| `cargo fmt --all -- --check` | PASS |
| `git diff --check` | PASS |

The tests marked ignored and their requirements are unchanged: two independent
Go peer scenarios and the root/ACL scenario described in the previous report.

The first expanded focused Rust run had one EADDRINUSE bind failure in an older
test that reused one configuration for several Runner instances. Independent
mode combinations were given separate port fixtures; the next focused and full
runs passed. This is not presented as reproduction of a runtime defect. The
first sandboxed Python run also failed because sockets were prohibited; the
full run with local sockets allowed passed. The logs are retained.

Artifacts: `/tmp/remora-drop-followups-20261026/`, including `rust.log`,
`python-final.log`, `clippy.log`, `build.log` and the original sandboxed
`python.log`.

SHA-256 of the current `target/release/remora`:

```text
b03db0b1c73120a157c9036c374844f6f45feead64566bd62d1e0668c75ba9e9
```

The previous VM campaign and read-only review examined an earlier build with
a different SHA-256; their results are not a new VM measurement of this binary.
The current iteration is limited to local tests and static checks. Deployment
and verification of the scenario on the affected server remain with the owner.
