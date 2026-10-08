#!/usr/bin/env bash
set -euo pipefail
profile=$(cat "$HOME/.local/state/desktop-lab/profile")
if pgrep -u "$UID" -f '^quickshell .* /usr/share/omarchy/shell' >/dev/null; then
  echo 'The stock shell is already running; refusing a competing shell.' >&2
  exit 1
fi
case "$profile" in
  lucid)
    if [[ -f "$HOME/.local/share/desktop-lab/wallpaper.jpg" ]]; then
      "$HOME/.config/hypr/scripts/wallpaper/set-wallpaper.sh" "$HOME/.local/share/desktop-lab/wallpaper.jpg"
    fi
    exec "$HOME/.config/lucid/launch-shell.sh"
    ;;
  noctalia) exec noctalia ;;
  *) echo "Unsupported profile: $profile" >&2; exit 1 ;;
esac
