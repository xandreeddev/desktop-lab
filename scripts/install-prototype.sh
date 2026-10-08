#!/usr/bin/env bash
# Build by default, or install an already built, locally supplied prototype binary.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ $# -gt 1 ]]; then
  echo 'Usage: install-prototype.sh [path/to/built/lucent-desktop]' >&2
  exit 2
fi
if [[ $# -eq 1 ]]; then
  binary="$1"
else
  cargo build --manifest-path "$root/lucent/Cargo.toml" --locked --release -p lucent-desktop
  binary="$root/lucent/target/release/lucent-desktop"
fi
install -Dm755 "$binary" "$HOME/.local/bin/lucent-desktop"
install -Dm644 "$root/packaging/lucent-desktop.desktop" "$HOME/.local/share/applications/lucent-desktop.desktop"
# An absolute executable also works in a session without ~/.local/bin on PATH.
sed -i "s|^Exec=.*|Exec=$HOME/.local/bin/lucent-desktop|" "$HOME/.local/share/applications/lucent-desktop.desktop"
echo 'Installed Lucent Prototype in the application menu. Right-click its card to exit.'
