#!/usr/bin/env bash
# Qualification only: consumes operator-prepared peers; never provisions a VPN.
set -euo pipefail
exec python3 "$(dirname "$0")/wireguard.py" "$@"
