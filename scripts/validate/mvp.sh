#!/usr/bin/env bash
# Implementer verification only; never invokes the independent reviewer.
set -euo pipefail
cargo build --locked --release
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
cargo build --locked --release --manifest-path tools/traffic/Cargo.toml
cargo fmt --all --manifest-path tools/traffic/Cargo.toml -- --check
cargo clippy --locked --manifest-path tools/traffic/Cargo.toml --all-targets -- -D warnings
cargo test --locked --manifest-path tools/traffic/Cargo.toml
bash -n scripts/validate/fixtures.sh scripts/validate/e2e.sh scripts/validate/bench.sh scripts/validate/interop.sh scripts/validate/wireguard.sh scripts/validate/libvirt-lab.sh scripts/validate/mvp.sh

python3 -m unittest discover -s tests -p test_benchmark_compare.py
python3 -m unittest discover -s tests -p test_wireguard_traffic.py
python3 -m unittest discover -s tests -p test_libvirt_lab.py
