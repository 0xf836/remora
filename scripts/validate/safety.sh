#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
output=${1:?usage: safety.sh FRESH_OUTPUT}
mkdir -m 700 "$output"
output=$(realpath "$output")
sha256sum src/buffers.rs src/udp.rs tools/safety/Cargo.lock > "$output/inputs.sha256"
cargo test --locked --manifest-path tools/safety/Cargo.toml > "$output/ownership.log" 2>&1
cargo test --locked --all-features --lib > "$output/properties-components.log" 2>&1
cargo test --locked --all-features --test io --test lifecycle > "$output/syscalls-lifecycle.log" 2>&1
: "${REMORA_MIRI_TOOLCHAIN:=nightly-2026-10-12}"
rustc +"$REMORA_MIRI_TOOLCHAIN" --version --verbose > "$output/miri-toolchain.txt"
cargo +"$REMORA_MIRI_TOOLCHAIN" miri test --locked --manifest-path tools/safety/Cargo.toml \
  > "$output/miri.log" 2>&1
printf 'passed\n' > "$output/status"
