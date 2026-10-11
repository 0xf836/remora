# Remora test traffic

Separate, unpublished test-only package; it is not part of the production CLI.
Build with `cargo build --locked --release --manifest-path tools/traffic/Cargo.toml`.
Run its accounting checks with `cargo test --locked --manifest-path tools/traffic/Cargo.toml`.

`responder --listen ADDRESS` accepts a generic datagram and echoes its exact
bytes/boundary, including empty packets. The numbered `sender --to ADDRESS`
and `receiver --listen ADDRESS` commands use test-only session/direction/
sequence/integrity fields. A responder's `--reverse-pps RATE` produces reverse
traffic independently of forward arrival. `--pps RATE`, `--size BYTES`,
`--seconds N`, `--start-ms UNIX_MS` and `--drain-ms N` describe the measurement.
`--session N` separates senders; a receiver accepts at most 32 sessions.
For 32 sessions launch 32 senders with distinct IDs and a common start time.
The fixed header requires sizes 40..1200 for sequence accounting.

`exercise --to ADDRESS --sessions 1|2|32` instead checks exact mixed sizes
0, 1, 128, 512 and 1200 against a responder. Its setup probes may retry;
measurement packets are not deliberately retried. This is functional coverage,
not capacity or sustained isolation qualification. Production Remora treats
all these bytes as opaque UDP payloads.

`baseline-server --listen PROXY --to TARGET --io mmsg|single` and
`baseline-client --listen APPLICATION --to PROXY --io mmsg|single` form an
unencrypted two-role relay. They use the production Linux I/O adapter, matched
kernel buffer requests, bounded flow/slab state and default diagnostics, with
an eight-byte session association prefix. Baseline polling sleeps 100 us when
idle; this initial tool is not proof of a sustainable baseline ceiling.

Use the owned [benchmark runner](../../scripts/validate/bench.sh), its
[profile](../../benchmarks/mvp-x86_64.toml) and [performance record](../../docs/performance.md)
for reproducible paired runs. A fresh output directory is mandatory. Preserve
invalid results and all manifests, counters, samples and RTT observations.
