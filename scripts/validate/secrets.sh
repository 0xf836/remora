#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
if ! command -v gitleaks >/dev/null 2>&1; then
  echo 'Gitleaks is required in PATH to scan secrets.' >&2
  exit 2
fi
case "${1:-history}" in
  history) options=(--log-opts=--all) ;;
  staged) options=(--pre-commit --staged) ;;
  *) echo "Usage: $0 [history|staged]" >&2; exit 2 ;;
esac
exec gitleaks git "${options[@]}" --config .gitleaks.toml \
  --gitleaks-ignore-path .gitleaksignore --ignore-gitleaks-allow \
  --redact=100 --no-banner .
