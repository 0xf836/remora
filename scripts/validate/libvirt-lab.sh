#!/usr/bin/env bash
# Explicit operator tool for disposable guests; never called by Remora or CI.
set -euo pipefail
exec python3 "$(dirname "$0")/libvirt_lab.py" "$@"
