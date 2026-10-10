# Compatibility and ownership

Test baseline: Omarchy 4.0.4-1, Hyprland 0.56.2-2, x86_64 KVM with virtio graphics. Later Omarchy snapshots are research references, not substitutes for the installed version. `doctor.sh` records installed versions, startup text and its SHA-256 before writes.

Omarchy's bootstrap places `~/.config/?.lua` before `/usr/share/omarchy/?.lua`. For Lucid and Noctalia, the lab shadows only `default.hypr.autostart`: it copies the installed module to the user configuration directory and replaces its single `omarchy-launch-shell` command with a systemd user service. The other startup commands remain intact. `hypr.autostart` loads only the lab's binding overrides. No file under `/usr/share/omarchy` is changed. Omarchy 4.0.4 lacks the newer `o.rebind` helper, so each replacement explicitly calls `hl.unbind` then `o.bind`.

A packaged startup change invalidates the saved checksum. Re-run doctor, review the diff, and stage a fresh override before logging in again. This is a tested integration for one baseline, not a guarantee across future Omarchy updates.

| Responsibility | Lucid VM | Noctalia VM | Lucent VM |
| --- | --- | --- | --- |
| Compositor, windows, input, monitors | Omarchy Hyprland | Omarchy Hyprland | Stock Omarchy Hyprland |
| Panels, dock, launcher, notifications | Lucid | Noctalia v5 | Native Lucent |
| Idle and sleep lock | Dedicated Hypridle + Hyprlock | Dedicated Hypridle + Hyprlock | swayidle; native secure lock + independent hyprlock recovery |
| Polkit dialogs | Lucid | Noctalia native agent | Lucent |
| Wallpaper | awww + shell-only Matugen palette | Native Noctalia wallpaper/palette | Lucent |
| Terminal/app theming | Existing Omarchy theme | Existing Omarchy theme | Lucent |
| Omarchy menus and shell-specific IPC | Partly replaced; see shortcuts | Partly replaced; see shortcuts | Native menus and explicit compatibility routes; unsupported stock plugins fail |

Lucid's own Idle page is deliberately disabled: changing it would write and restart another Hypridle configuration. Lock shortcuts use Hyprlock. Lucid still exposes its upstream session/lock UI; it is not represented as a separately audited authentication implementation. Noctalia's session lock action is routed to Hyprlock. The stock Omarchy sleep-lock monitor is masked only in the two replacement guests because it otherwise calls a shell that is no longer running. Hypridle handles logind sleep/lock events instead.

The Lucid Bluetooth probe has one documented patch: bound `bluetoothctl show` to two seconds so guests without a Bluetooth controller do not accumulate indefinitely waiting probes. The patch is stored independently under `patches/` and applied only to the installed, pinned Lucid copy.

| Feature | Lucid | Noctalia | Lucent framework client |
| --- | --- | --- | --- |
| Floating capsules and dock | Upstream implementation | Supported config approximation | Floating capsules and dock |
| Morphing dock/launcher | Upstream implementation | Separate dock and launcher | Implemented through Rust framework |
| Calendar, clock, weather, media widgets | Bookends upstream preset | Declarative desktop widgets | Implemented; plus system, notes and timer |
| Wallpaper-derived colors | Matugen, shell only | Native palette generation | Shared dark/light reference palette |
| Notifications, tray, settings | Upstream | Upstream | See current scope below |
| Wallpaper/app/terminal theme bridge | No app overwrites; shell palette is authoritative | No automatic bidirectional bridge | See current scope below |

Wireless radios, physical brightness, battery hardware, physical multi-monitor hotplug and host suspend are not provided by these guests. Their UI presence does not prove those hardware paths work. VM CPU/RSS numbers also cannot establish a toolkit-wide performance ranking.

## Lucent integration scope

Lucent uses separate native clients and marked user configuration blocks. It
replaces the stock visual shell through user PATH adapters, retaining Omarchy
window bindings and backend commands. Theme authority belongs to Lucent tokens;
headless Omarchy templates only propagate generated colors to applications.
Wallpaper presentation, notifications, permission prompts, locking and login
have native clients. swayidle and wl-paste supply headless session integration.
Network setup and Bluetooth pairing use independent terminal applications;
tray hosting and optional stock plugins remain unsupported. See the
[current ownership map](https://xandreeddev.github.io/desktop-lab/docs/system/)
and [Lucent README](../lucent/README.md) for exact scope and recovery.
