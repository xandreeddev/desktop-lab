# Pinned Lucid installer audit

Reviewed `install.sh` at the revision and SHA-256 in `manifests/upstream-lock.json` before execution. Source is fetched into an ignored cache and its archive hash is verified. No upstream checkout is vendored or modified in place.

The stock installer can replace the entire user Hyprland configuration, append theme templates affecting other applications, install unrelated dock applications and a cursor plugin, mask rival notification units, and restart an existing Lucid process. Those behaviors matter on Omarchy.

The lab passes `--no-hypr --no-apps --no-look --no-theming --no-wallpapers --no-plugins --skip-deps`. Official package dependencies are installed separately. The remaining writes include the shell tree, Lucid launch helper, OFL font, preference seeds and Cava configuration; these locations are backed up. The audited base has no competing notification activation service. The installer is limited to a single installed Lucid profile per clone; it never runs in the host desktop.

One separately stored patch bounds `bluetoothctl show` to two seconds for guests without BlueZ hardware. It is applied to the installed shell after the upstream installer completes. All other shell source remains upstream. Fonts and wallpaper remain upstream runtime assets, with their own licenses; this repository links to and fetches them instead of relicensing them.

Palette handling is a lab-owned adapter at Lucid's expected wallpaper-command path. It calls awww and Matugen, writes only Lucid's cached Material colors, and preserves Omarchy's terminal/application theme. It does not install upstream's broad Matugen application hooks. Advanced static-theme and environment customization pages can still write user settings if invoked; the prepared comparison uses the wallpaper palette path.
