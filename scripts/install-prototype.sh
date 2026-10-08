#!/usr/bin/env bash
# Compatibility entry point for the former prototype installer.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ $# -gt 1 ]]; then
  echo 'Usage: install-prototype.sh [path/to/built/lucent-desktop]' >&2
  exit 2
fi
if [[ $# -eq 1 ]]; then
  binary_dir="$(dirname "$1")"
else
  cargo build --manifest-path "$root/lucent/Cargo.toml" --locked --release -p lucent-desktop -p lucent-cli
  binary_dir="$root/lucent/target/release"
fi
exec python3 "$root/scripts/lucent-setup.py" install --binary-dir "$binary_dir"
