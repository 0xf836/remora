# Socket drop telemetry fix: verification handoff

This change removes production SO_RXQ_OVFL enablement and replaces it with
JSON-only SO_MEMINFO sampling. It does not change transport configuration, QUIC
congestion control, session queue limits, or the diagnostics writer lifecycle.

## Focused checks

`cargo test --locked --lib rx_drop -- --nocapture` covers:

- Real loopback receive-queue pressure with SO_RXQ_OVFL confirmed disabled,
  both Single/Mmsg modes and outer/relay socket settings; drops observable before
  receiving another packet, and payload reception still works.
- Counter wrap, no duplicate accounting, failed reads preserving the baseline,
  and rejection of truncated SO_MEMINFO responses.
- Text/JSON crossed with snapshots on/off; no text-mode query, 16-query turn
  budget, cadence, nonfatal failure coverage and final-sample catch-up.
- JSON with snapshots off reports the first incomplete-coverage transition and
  suppresses unchanged subsequent health status.

Additional follow-up tests cover actual client connection retirement, relay
retirement and shutdown socket closure (including a last nonzero increment,
read failure, index renumbering and repeated cleanup). Ordered sweep tests cover
budget, progress after removal and deferral of keys beyond the frozen endpoint.
Python tests cover incomplete/unknown socket telemetry through parsing,
summary coverage and qualification gates, independently of diagnostic record loss.

Current follow-up results are recorded in
[socket-drop-telemetry-followups.md](socket-drop-telemetry-followups.md).

## Initial implementation result (2026-10-26)

The final focused run passed all 3 tests (0 failures; 85 unrelated library tests
filtered out), with test execution taking 0.06 seconds after compilation.
`cargo fmt --all -- --check` and `git diff --check` passed. The initial sandboxed
attempt could not bind UDP sockets; the successful run used the authorized
unsandboxed local test command. These results establish local Linux behavior,
not acceptance on the affected server kernel. No full suite, release build,
clippy run, VM/WAN workload or production deployment was performed.

## Original handoff — subsequently executed in the linked validation report

The initial implementation postponed these checks. Their executed outcomes are
recorded in [the validation report](socket-drop-telemetry-report.md); the current
follow-up has its own results linked above:

```sh
cargo build --locked --release
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
```

Native VM / owner-approved server acceptance:

1. Use the affected kernel and WireGuard-over-Remora configuration. Test text
   output first, then JSON with snapshots both off and on. Do not re-enable
   SO_RXQ_OVFL to reproduce the old kernel fault.
2. Exercise SSH to the server's tunnel address, browsing and sustained transfers
   with enough load to cause receive-queue pressure. Verify continued tunnel
   delivery, reconnect after server restart, and no new kernel Oops/usercopy log.
3. Check normal and loaded shutdown/restart: no zombie reactor with a surviving
   diagnostics thread, bounded service stop, and successful subsequent startup.
4. Inspect JSON drop totals, query counts and coverage flags across session
   retirement and client reconnect. An unavailable SO_MEMINFO counter must not
   prevent traffic; known incomplete coverage must be visible.
5. On IPv4 and IPv6, compare packet delivery and CPU under the same offered load
   before/after the fix. Include many relay sockets and session churn to check
   timer/cancellation responsiveness during multi-turn telemetry sweeps.

These are acceptance tests, not a claim that the existing diagnostic thread is
independently robust against arbitrary kernel thread termination. No deployment
or production restart is part of this implementation task.
