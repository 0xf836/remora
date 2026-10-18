#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
: "${REMORA_FUZZ_TOOLCHAIN:=nightly-2026-10-12}"
seconds=${1:-30}
output=${2:?usage: fuzz.sh SECONDS FRESH_OUTPUT}
[[ "$seconds" =~ ^[0-9]+$ ]] && (( seconds >= 1 && seconds <= 86400 ))
mkdir -p -- "$(dirname -- "$output")"
mkdir -m 700 -- "$output"
output=$(realpath "$output")
./scripts/validate/fixtures.sh --output "$output/fixtures" > "$output/fixtures.log"
export REMORA_FUZZ_CONFIG="$output/fixtures/server.toml"
rustc +"$REMORA_FUZZ_TOOLCHAIN" --version --verbose > "$output/toolchain.txt"
cargo fuzz --version > "$output/cargo-fuzz.txt"
cargo fetch --locked
cargo fetch --locked --manifest-path fuzz/Cargo.toml
python3 scripts/validate/quiche_inputs.py > "$output/quiche-provenance.json"
mapfile -t inputs < <(rg --files --hidden src fuzz/fuzz_targets fuzz/support fuzz/src dependencies | sort)
sha256sum Cargo.toml Cargo.lock rust-toolchain.toml build.rs fuzz/Cargo.toml fuzz/Cargo.lock fuzz/build.rs "${inputs[@]}" > "$output/inputs.sha256"
for target in masque policy ancillary lifecycle; do
  mkdir "$output/$target" "$output/$target/corpus" "$output/$target/artifacts"
  cp -r "fuzz/seeds/$target/." "$output/$target/corpus/"
  CARGO_NET_OFFLINE=true cargo +"$REMORA_FUZZ_TOOLCHAIN" fuzz run "$target" "$output/$target/corpus" -- \
    -seed=73013 -max_len=65536 -rss_limit_mb=2048 -timeout=10 \
    -max_total_time="$seconds" -artifact_prefix="$output/$target/artifacts/" \
    > "$output/$target/run.log" 2>&1
  sha256sum --check "$output/inputs.sha256" > "$output/$target/inputs-check.log"
  python3 scripts/validate/quiche_inputs.py > "$output/$target/quiche-provenance.json"
  printf 'passed\n' > "$output/$target/status"
done
