# Maintained fuzz boundaries

Run `scripts/validate/fuzz.sh 30 benchmarks/results/fuzz-smoke-UNIQUE` after
installing cargo-fuzz 0.13.1 and nightly-2026-10-12. The nightly is test-only;
production remains Rust 1.99.0. `fuzz/Cargo.lock` is separate and versioned.
Four targets invoke production code: MASQUE fields/URI/datagrams/fragmented
Capsules; canonical host, authentication and all-answer policy; ancillary,
extended-error and unsupported GRO metadata; bounded lifecycle event sequences.

The lifecycle target retains small queue/retry properties and also drives the
actual reactor methods through `support/lifecycle.rs`. Its independent input
expectations distinguish current, stale-session and stale-connection packets at
`schedule_datagrams`; successful late setup completions must not allocate relay
or identity credit or alter session state. Close cancels application/native
work while preserving committed ciphertext. A scripted send boundary checks
EAGAIN and exact accepted-prefix progress, and replacement connections receive
old callbacks. A separate real loopback admission check rejects new senders
and reconnect attempts after permanent mapping rejection. A fixed prelude
reaches these transitions even with an empty corpus input; up to 32 additional
operations fuzz their ordering. The same harness has native deterministic
regressions in the reactor lifecycle test module.

`build.rs` compiles current source files into this unpublished test package;
it does not maintain another owner implementation. Only module/doc paths and
five exact, count-checked existing `cfg(test)` send-hook sites are adapted in
`OUT_DIR`. Root package features and production APIs are unchanged. The runner
hashes all source, adapter, harness and manifest/lock inputs. Direct adapter
checks use `cargo clippy --locked --manifest-path fuzz/Cargo.toml --lib --bins
--all-features -- -D warnings`; native tests run in the root package. The
source-included adapter is not a separate supported production package.

This is bounded transition coverage, not fuzzing the complete asynchronous
reactor loop, DNS service, GOAWAY, Linux scheduling or WAN behavior. QUIC/TLS
peer exchanges are in memory; local UDP is used only for admission assertions.
The expanded target is substantially slower per input than the helper-only
predecessor, so historical iteration counts are not comparable.

The wrapper owns generated private fixtures and exports their path; direct
`cargo fuzz run policy` or `cargo fuzz run lifecycle` requires
`REMORA_FUZZ_CONFIG` pointing to a private server config and the sibling
`client.toml`. Use a fresh fixture per concurrent process; the wrapper runs one
target at a time. Seeds are public synthetic bytes, not real credentials.
Generated corpus, crashes, logs, input hashes and exact toolchain are retained
in each run directory. Do not delete a crash to make a campaign pass: minimize,
add a targeted regression and repeat the affected target. AddressSanitizer is
the cargo-fuzz default. Smoke runs establish only bounded campaign evidence,
not parser completeness. Scheduled CI runs 900 seconds per target and uploads
corpus/crashes/logs even on failure. Private fixtures are excluded from uploads.
