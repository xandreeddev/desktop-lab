# Compatibility and ownership

Test baseline: Omarchy 4.0.4-1, Hyprland 0.56.2-2, x86_64 KVM with virtio graphics. Later Omarchy snapshots are research references, not substitutes for the installed version. `doctor.sh` records installed versions, startup text and its SHA-256 before writes.

Omarchy's bootstrap places `~/.config/?.lua` before `/usr/share/omarchy/?.lua`. The lab shadows only `default.hypr.autostart`: it copies the installed module to the user configuration directory and replaces its single `omarchy-launch-shell` command with a systemd user service. The other startup commands remain intact. `hypr.autostart` loads only the lab's binding overrides. No file under `/usr/share/omarchy` is changed. Omarchy 4.0.4 lacks the newer `o.rebind` helper, so each replacement explicitly calls `hl.unbind` then `o.bind`.

A packaged startup change invalidates the saved checksum. Re-run doctor, review the diff, and stage a fresh override before logging in again. This is a tested integration for one baseline, not a guarantee across future Omarchy updates.

| Responsibility | Lucid VM | Noctalia VM | Lucent VM |
| --- | --- | --- | --- |
| Compositor, windows, input, monitors | Omarchy Hyprland | Omarchy Hyprland | Stock Omarchy Hyprland |
| Panels, dock, launcher, notifications | Lucid | Noctalia v5 | Stock shell; prototype is an overlay |
| Idle and sleep lock | Dedicated Hypridle + Hyprlock | Dedicated Hypridle + Hyprlock | Stock Omarchy |
| Polkit dialogs | Lucid | Noctalia native agent | Stock Omarchy |
| Wallpaper | awww + shell-only Matugen palette | Native Noctalia wallpaper/palette | Stock Omarchy |
| Terminal/app theming | Existing Omarchy theme | Existing Omarchy theme | Stock Omarchy |
| Omarchy menus and shell-specific IPC | Partly replaced; see shortcuts | Partly replaced; see shortcuts | Stock functionality |

Lucid's own Idle page is deliberately disabled: changing it would write and restart another Hypridle configuration. Lock shortcuts use Hyprlock. Lucid still exposes its upstream session/lock UI; it is not represented as a separately audited authentication implementation. Noctalia's session lock action is routed to Hyprlock. The stock Omarchy sleep-lock monitor is masked only in the two replacement guests because it otherwise calls a shell that is no longer running. Hypridle handles logind sleep/lock events instead.

The Lucid Bluetooth probe has one documented patch: bound `bluetoothctl show` to two seconds so guests without a Bluetooth controller do not accumulate indefinitely waiting probes. The patch is stored independently under `patches/` and applied only to the installed, pinned Lucid copy.

| Feature | Lucid | Noctalia | Lucent prototype |
| --- | --- | --- | --- |
| Floating capsules and dock | Upstream implementation | Supported config approximation | One rounded card |
| Morphing dock/launcher | Upstream implementation | Separate dock and launcher | Unimplemented |
| Calendar, clock, weather, media widgets | Bookends upstream preset | Declarative desktop widgets | Unimplemented |
| Wallpaper-derived colors | Matugen, shell only | Native palette generation | Fixed prototype colors |
| Notifications, tray, settings | Upstream | Upstream | Unimplemented |
| Wallpaper/app/terminal theme bridge | No app overwrites; shell palette is authoritative | No automatic bidirectional bridge | Unimplemented |

Wireless radios, physical brightness, battery hardware, physical multi-monitor hotplug and host suspend are not provided by these guests. Their UI presence does not prove those hardware paths work. VM CPU/RSS numbers also cannot establish a toolkit-wide performance ranking.
