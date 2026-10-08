#!/usr/bin/env bash
# Lucid-only wallpaper palette: no terminal, GTK, or Omarchy theme writes.
set -euo pipefail
wallpaper=${1:?Usage: set-wallpaper.sh IMAGE [dark|light]}
mode=${2:-$(cat "$HOME/.cache/current_mode" 2>/dev/null || echo dark)}
[[ -f "$wallpaper" ]] || { echo 'Wallpaper does not exist' >&2; exit 1; }
[[ "$mode" == dark || "$mode" == light ]] || exit 1
if ! awww query >/dev/null 2>&1; then
  awww-daemon >"$HOME/.local/state/desktop-lab/wallpaper.log" 2>&1 &
  for ((i=0; i<30; i++)); do awww query >/dev/null 2>&1 && break; sleep 0.1; done
fi
awww img "$wallpaper" --transition-type fade --transition-duration 0.4
mkdir -p "$HOME/.cache/quickshell"
palette=$(mktemp "$HOME/.cache/quickshell/palette.XXXXXX")
trap 'rm -f "$palette"' EXIT
matugen image "$wallpaper" -m "$mode" --prefer colorfulness --dry-run --json hex -q |
  jq '.colors | with_entries(.value = .value.default.color)' >"$palette"
jq -e '.primary and .surface' "$palette" >/dev/null
mv "$palette" "$HOME/.cache/quickshell/matugen.json"
printf '%s\n' "$wallpaper" >"$HOME/.cache/current_wallpaper"
printf '%s\n' "$mode" >"$HOME/.cache/current_mode"
printf '%s\n' matugen >"$HOME/.cache/current_theme"
