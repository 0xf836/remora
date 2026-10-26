# UDP drop telemetry fix validation, 2026-10-26

## Outcome and scope

The release build, clippy and the complete ordinary Rust test suite passed.
On two test VMs, the fixed build sustained SSH over WireGuard, HTTP transfers
and actual UDP drops. External injection of SO_MEMINFO results demonstrated
that a query error or a short response does not interrupt forwarding. No
critical defect in the fix was found in these checks. Implementation and test
coverage observations are recorded separately below.

The program sources were unchanged during this validation: the SHA-256 hashes
of all `src/*.rs` files match those recorded before the checks. New scenarios
and instrumentation reside in `/tmp`; this report was added to the repository.
Deployment to the owner's server and validation on its kernel remain with the
owner. No server changes were performed in this run. An attempted SSH
observation request was interrupted; no result was received and no local result
file was created. Following the owner's clarification, access to that server
was excluded from the scope.

## Build and automated tests

| Check | Result |
| --- | --- |
| `cargo build --locked --release` | PASS |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --locked --all-targets --all-features -- -D warnings` | PASS |
| `cargo test --locked --all-targets --all-features` | 242 passed, 0 failed, 3 ignored |
| `git diff --check` | PASS |
| SHA-256 of `src/*.rs` before/after | Identical |

Test groups: library 88; e2e 34; I/O 26; lifecycle 8; protocol 22; security 64.
The full run included the new rx_drop tests, IPv4/IPv6 forwarding, queue
exhaustion and errors, shutdown and diagnostics.

Three tests marked ignored were not executed:

- `shutdown::both_signals_cancel_awaiting_acceptance_setup_with_independent_peer`:
  requires a current independent Go peer build with `hold-response` support.
- `interop_cases::independent_go_roles_preserve_bytes_and_deny_monitored_negatives`:
  requires a separate peer built with pinned Go 1.27.2.
- `config::root_owned_acl_secrets_delegate_read_to_an_unprivileged_process`:
  requires root and an isolated ACL test that switches UIDs.

The available older independent peer binaries do not support `hold-response`.
The system Go version was 1.27.3, so a build with that version was not used as
evidence for the pinned 1.27.2 check. Skipped tests are not counted as passes.
No fuzz campaign, extended soak or full comparative performance campaign ran.

SHA-256 of the validated release build:

```text
b9a45862fc4b3dd809dd9950c620a8917f3f4921885f94efd2d4381b779c614c
```

## Test environment and workload

The two existing dedicated VMs, `remora-v031-check-a/b`, run Debian 13 with
kernel `6.12.111+deb13-cloud-amd64` on x86_64. The development host runs
`7.2.11-arch1-1`. This does not validate the original server kernel, `6.12.107`.

The VMs used a separate test binary, configurations and transient systemd units.
Remora ran as UID 938, with WireGuard MTU 1280, Remora profile 1452/1400 and
client policy routing that exempts the Remora UID. Each virtual network
interface had 27.5 ms of egress delay, yielding approximately 55 ms RTT. SSH
passed through WireGuard to `10.77.0.1:22`, with the test server's known host
key verified. The HTTP workload used four concurrent streams, each receiving
three 8 MiB responses; the contents of every received block were checked.

| Mode | Verified HTTP data | HTTP workload duration | SSH before/after load | Server stop |
| --- | --- | --- | --- | --- |
| Text, snapshots=false | 96 MiB | 27.09 s | PASS / PASS | 0.439 s |
| JSON, snapshots=false | 96 MiB | 26.04 s | PASS / PASS | 0.456 s |
| JSON, snapshots=true | 96 MiB | 24.64 s | PASS / PASS | 0.469 s |

After the load, a four-packet ICMP control series completed without loss. JSON
recorded up to 278 and 305 kernel drops on the server VM in the two JSON runs,
and up to 43 and 22 on the client, respectively. There were no counter query
errors. These are per-process maxima, not sums of repeated snapshots of a
cumulative counter. `Resource code=2` also occurred under load while forwarding
continued. That code alone therefore does not indicate reactor termination.

After normal shutdown, no `remora-drop-fixed` processes remained, including
zombies. No new kernel journal messages appeared during the check intervals.
The test transient units were stopped, the fixture's WireGuard interface and
policy rules were removed, and netem was returned to zero delay. The VMs,
which were already running before the checks, were left running.

This was a functional workload, not an A/B benchmark. Differences between the
three durations cannot establish a performance improvement. The old control
build with SO_RXQ_OVFL was not run in this campaign.

## SO_MEMINFO failures and multiple sessions

The validated release build used an external `LD_PRELOAD` interceptor in `/tmp`.
It counted SO_MEMINFO queries and could return ENOPROTOOPT or a successful
four-byte response. It also counted and rejected attempts to enable SO_RXQ_OVFL,
without enabling that kernel option. The interceptor did not modify Remora's
sources.

In addition to WireGuard, 30 separate local UDP sockets each sent 30 messages
to the client listener: 900 messages and 31 active sessions in total. This
exercised periodic collection across multiple portions. Delivery of these
arbitrary messages through WireGuard was not expected, since WireGuard rejects
them as invalid. Separate ICMP and SSH checks verified the working tunnel.

| Mode / injected result | Traffic and SSH | SO_MEMINFO queries, client / server before restart | SO_RXQ_OVFL enable attempts | Recovery after restart |
| --- | --- | --- | --- | --- |
| Text / ENOPROTOOPT | PASS | 0 / 0 | 0 | 7.44 s |
| JSON, snapshots=false / ENOPROTOOPT | PASS | 46 / 161 | 0 | 7.49 s |
| JSON, snapshots=true / short response | PASS | 44 / 161 | 0 | 7.46 s |
| JSON, snapshots=true / actual response | PASS | 46 / 161 | 0 | 7.51 s |

In both failure scenarios, JSON contained
`kernel_rx_overflow_incomplete=true` and an increasing
`kernel_rx_overflow_read_errors`. With snapshots=false, repeated query failures
alone did not produce a snapshot every second; additional snapshots on
connection loss and shutdown were expected. In the normal JSON run,
incomplete=false and read_errors=0. The interceptor's syscall counts agree
with the final I/O counters. Both processes stopped in less than 0.5 s in all
four scenarios.

## Preserved failed probes and observations

### 1. The initial short probes did not establish recovery behavior

The first VM driver started ICMP immediately after bringing up the interface.
Four packets at 0.2 s intervals were lost before the tunnel became ready. The
same series failed immediately after a server restart; SSH with a short
connection budget timed out in two modes. The original `vm-results.json`
retains `pass=false` and has not been relabelled as successful.

The second driver waited up to 30 s for an ICMP response and then checked SSH.
Initial readiness took 4.38–4.62 s; recovery after a server restart took
7.44–7.51 s. All four scenarios passed. This is consistent with nonzero normal
handshake/recovery time, but does not establish a strict 7.5 s recovery bound
under other conditions. This observation does not confirm a recovery defect.
Future scenarios need an explicit readiness gate.

### 2. Socket traversal cost is an observation, not a demonstrated regression

In each portion, `Runner::sample_rx_drops` (`src/reactor.rs:3578`) reconstructs
the iterator chain and calls `.skip(rx_drop_sample_cursor)`. For HashMap
sessions, skipping requires walking previously visited elements again. The
number of syscalls is limited to 16 per periodic portion, but at large N the
additional element traversal cost is approximately N²/16 per full sweep.
Changes in session membership can also postpone a particular socket until the
next sweep, as already documented in the contract.

No functional problem was found with 31 sessions. Scaling with substantially
higher limits and session churn was not measured separately. This calls for
future profiling or a clearer traversal cost bound; it is not evidence that
the observed hang remains unfixed. Sources were not changed during this run.

### 3. Automated regression coverage needs stronger lifecycle checks

The new `rx_drop_sampling_format_budget_errors_and_final_collection`
(`src/reactor.rs:4357`) checks final accumulation by calling `record_rx_drops`
directly, rather than going through `retire_session`, `remove_connection` and
the end of `run`. On its own, this test would not catch removal of one of the
production final-query calls. The VM checks additionally covered actual
restart/retirement and counter reconciliation, but did not inject a nonzero
last increment immediately before every closure variant. A possible follow-up
is targeted tests of those three lifecycle paths. Sources and tests were not
edited during this validation run.

Final queries during mass closure are separate from the periodic sweep's
16-query quota. That quota cannot be treated as a universal bound on all
telemetry syscalls in any turn that includes mass retirement/shutdown.
Shutdown remained fast with the 31 sessions tested.

## Artifacts and reproducibility

Local directory: `/tmp/remora-drop-verification-20261026/`.

- `build.log`, `clippy.log`, `tests.log`: original results of the main checks.
- `sources-before.sha256`: source integrity verification.
- `vm-results.json`, `vm-driver.log`, `vm-driver-retry.log`: original workload
  results, including the test check-config failure when run as root (the driver
  was corrected to run it as the service user) and the short failed probes.
- `*-snapshots-*-a.log`, `*-snapshots-*-b.log`: Remora logs on the VMs.
- `vm-fault-results.json`, `vm-fault-driver.log`, `fault-*.log`: SO_MEMINFO
  failures, multiple sessions and recovery with a readiness gate.
- `meminfo_probe.c`, `meminfo_probe.so`: the external syscall interceptor.
- Copies of the VM drivers and the artifact archive preserve the exact commands.

Related plan: [socket-drop-telemetry.md](socket-drop-telemetry.md).
The owner's deployment and verification of the usual SSH/browser scenario on
the affected server remain the final check for this particular incident.

## Subsequent changes

After this run, follow-ups addressed collector coverage, periodic traversal and
closure tests. The current binary has a different SHA-256. Results from this
VM campaign remain attached to the build validated here; new local results are
recorded in [socket-drop-telemetry-followups.md](socket-drop-telemetry-followups.md).
