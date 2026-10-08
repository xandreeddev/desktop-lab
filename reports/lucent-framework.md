# Lucent framework and desktop validation

Validated on the existing isolated Omarchy 4.0.4 / Hyprland 0.56.2 VM, with
1920×1080 output at scale 1, two virtual CPUs and 3 GiB guest RAM. The compositor
uses virgl. Lucent uses Mesa llvmpipe/lavapipe through wgpu's Vulkan backend.
This is software Vulkan, with no host GPU passthrough.

## Executed checks

- Rust formatting, warning-free Clippy over all targets, and 20 Rust unit tests.
- Eight Python configuration/rollback tests; ShellCheck and Python syntax checks.
- Three native layer surfaces render with Vulkan and premultiplied alpha.
- Dock opens an animated launcher; real keyboard input filters 57 installed apps.
- Enter launches a real Foot terminal; workspace pills change actual Hyprland workspaces.
- A widget drag persists on release and restores after a complete client restart.
- Widget toggles, position reset, one-line notes, and timer start/pause/reset work.
- Wallpaper carousel responds to pointer and keyboard selection; applying changes
  the actual Omarchy background link through its supported command.
- Super+Space and Super+Ctrl+Space work as real compositor bindings.
- The secure Omarchy lock engages and accepts the existing guest login afterward.
- Audio mute/unmute changes the actual WirePlumber sink and restores its state.
- Full VM reboot/login starts the new desktop automatically.
- Full integration rollback and reactivation preserve valid Hyprland configuration.
- Stopping Lucent restores the original stock-bar visibility; restarting hides it
  after Lucent has rendered. Other Omarchy session services remain running.
- The independent `hello-layer` client accepts child-component clicks, animates
  and closes cleanly. A real terminal receives clicks through the dock host's
  transparent region.

The input harness uses QEMU pointer/keyboard events, not synthetic component
messages. It checks runtime inspection data and actual compositor window state.
It preserves the original widget settings and wallpaper when rerun. The second
client and reboot/secure-lock checks were also executed separately.

## Measured performance

Results are recorded in [the machine-readable performance report](measurements/lucent-framework-performance.json).
Earlier samples were discarded because the stock screensaver could hide top-layer surfaces. The final measurement holds Omarchy’s supported stay-awake state and checks the visible desktop. The current
backend selects Mailbox when supported because the framework already schedules
frames through compositor callbacks; FIFO remains a fallback.

| Measurement | Result |
| --- | --- |
| Idle Rust process CPU, one logical core | 0.15% |
| Idle service CPU including helper commands | 0.924% |
| Mean / maximum process RSS | 255.16 / 262.94 MiB |
| Dock frames during 120 seconds idle | 0 |
| Bar / widget frames during idle | 2 / 2, from minute changes |
| Launcher median frame intervals | 33.20–34.36 ms (about 30 fps) |
| Maximum sampled frame interval | 53.86 ms |

The software-Vulkan VM does **not** meet a 60 fps animation target or the initial
200 MiB RSS aspiration. Changing presentation mode did not produce a meaningful improvement in these VM tests. The interaction curves are implemented and finite; hardware-GPU
smoothness still needs a machine with working GPU-backed Vulkan.

Measurements cover the main Rust process and separately the service cgroup's CPU
(including its helper commands). They exclude the retained Omarchy shell and
compositor. GPU allocations were not measured independently; process RSS can
include mapped software-driver memory. These are actual VM observations, not a
toolkit-wide comparison or a claim about hardware Vulkan performance.

## Visual reference and remaining gaps

Real captures are included below. Lucid's reference wallpaper retains its original
rights. Font attribution and OFL licensing are included with the client assets.

![Rust desktop](screenshots/lucent-framework-desktop.png)
![Rust launcher](screenshots/lucent-framework-launcher.png)
![Rust wallpaper carousel](screenshots/lucent-framework-wallpapers.png)

The Rust implementation follows the reference layout, palette, typography and
primary dock/launcher/selector transitions. It is not complete Lucid parity:
notification history, tray hosting, clipboard/emoji modes, full control-center
and settings panels, waveform/media art integration, wallpaper palette extraction,
widget resizing, rich text, fractional scaling and multi-output placement remain
outside this implementation. Stock Omarchy still supplies lock, notifications,
background and session services. Media buttons are connected through playerctl,
but playback against a real media player was not exercised in this run. Wi-Fi,
Bluetooth, physical brightness, battery, monitor hotplug and suspend/resume were
not validated by this VM test.
