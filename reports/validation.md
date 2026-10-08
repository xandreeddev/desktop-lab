# VM validation — 2026-10-08

Three independent VMs were prepared from the existing stopped Omarchy installation. The original VM disk and host desktop were left untouched. Lucid and Noctalia replace only their own guest shell startup. Lucent is the working first prototype requested for this handoff, launched alongside stock Omarchy.

## Environment

- Omarchy 4.0.4-1; Hyprland 0.56.2-2; Quickshell 0.3.1-1.
- Lucid 1.20 at `e662954f73a39275f214923bb4dab6c85b3800ba`.
- Noctalia 5.0.1-1 from Arch's official extra repository.
- KVM/QEMU, 2 vCPUs and 3 GiB RAM per VM; one lab guest running at a time.
- AMD Ryzen 5 7430U host CPU exposed to guests; virtio/virgl graphics on AMD Radeon; guest Mesa 26.2.2. Output `Virtual-1`, 1920×1080, scale 1.
- Rust 1.99.0; SCTK 0.21.1; wgpu 30.0.1. The complete dependency graph is locked in Cargo.lock.

## Executed checks

| Check | Lucid | Noctalia | Lucent prototype |
| --- | --- | --- | --- |
| Real graphical startup | Passed, including cold boot | Passed, including cold boot | Passed in stock Omarchy |
| Single replacement shell | Passed | Passed; no Quickshell process | Stock shell intentionally retained |
| Hyprland config errors | None | None | None |
| Launcher starts a real application | Foot launched from Lucid | Foot launched from Noctalia | Installed desktop entry launches prototype |
| Secure lock and password unlock | Hyprlock passed | Hyprlock passed | Stock Omarchy unlock passed |
| Notification delivery | Verification toast observed | Verification notification delivered | Stock service retained |
| Clipboard round-trip | Passed | Passed | Stock service retained |
| Selected-city weather | Forecast cache matches selection, daily forecast populated | Resolved city and live forecast received | Unimplemented |
| Default audio and network | `wpctl` reports volume; NetworkManager connected | Same | Stock services retained |
| Native shell runtime | Qt/Quickshell by design | No Qt/GTK libraries in process maps | No Qt/GTK libraries in process maps |
| Source/config validation | Pinned installer hash, Lua/IPC checks | Installed v5 schema validation and full export | fmt, Clippy, tests, release build |
| Recovery | Shared recovery regression suite; backups recorded | Actual rollback, reboot into stock, reinstall and reboot into Noctalia | Right-click clean exit |

Noctalia rollback was tested after activation: the user startup shadow disappeared, stock `quickshell -n -p /usr/share/omarchy/shell` returned after reboot, and Hyprland reported no configuration errors. Noctalia was then reinstalled and activated again. Lucid uses the same tested recovery implementation, but a separate full Lucid rollback/relogin cycle was not executed.

The Rust integration harness sent actual QEMU pointer events through Hyprland. The first frame displayed the card and text; one left-click changed the counter/color and rendered a second frame. No additional frame was rendered during 120 idle seconds. Right-click closed the process cleanly, reporting two frames and one click. The stock secure lock had to be unlocked during debugging; the harness now checks lock and display state before testing.

The Rust GPU instance owns the native display handle required for EGL presentation. The tested virgl backend advertises opaque presentation only, so this prototype draws its rounded card on a dark rectangular canvas. Transparent presentation on Vulkan has not been tested.

Six Python recovery/configuration tests and three Rust domain/scheduling tests passed. ShellCheck, `cargo fmt --check`, and `cargo clippy --workspace --all-targets -- -D warnings` passed. An Arch package was built with `makepkg --nodeps` using the isolated toolchain; build and check stages ran. Dependency enforcement and installation via pacman were not tested. The VM uses the local executable/menu installer.

## Measurements

Each sample is one real 120-second interval, captured before personal weather configuration. CPU is a percentage of **one logical CPU**, calculated from process CPU ticks. RSS is sampled once per second. Child processes, compositor cost, GPU allocations and whole-desktop totals are excluded. These are VM observations with different feature sets, not a general toolkit benchmark. The Rust card is far less capable than either complete shell.

| Process | Mean RSS | Maximum RSS | Idle CPU | Service restart → IPC |
| --- | ---: | ---: | ---: | ---: |
| Lucid / Quickshell | 419.01 MiB | 435.63 MiB | 14.833% | 2.992 s |
| Noctalia | 134.21 MiB | 136.89 MiB | 0.317% | 0.731 s |
| Lucent prototype | 60.71 MiB | 60.71 MiB | 0.000% | Not a shell service |

The restart measurement is a single warm-cache service restart until shell IPC responds, with 100 ms polling. It is not first-frame or login duration. Zero measured prototype CPU means no measurable tick accumulation during this interval, not a promise of zero resource use in all circumstances.

Raw, sanitized results: [Lucid idle](measurements/lucid-idle.json), [Lucid readiness](measurements/lucid-startup.json), [Noctalia idle](measurements/noctalia-idle.json), [Noctalia readiness](measurements/noctalia-startup.json), [Rust integration and idle](measurements/lucent-integration.json), [Rust runtime/adapter](measurements/lucent-runtime.log).

## Screenshots

These are real guest captures, not generated mockups. They precede personal weather configuration to avoid publishing location data. Lucid and Noctalia use Lucid's reference wallpaper. Upstream artwork keeps its original rights; the repository's MIT license covers the lab code, not that artwork.

- [Stock Omarchy](screenshots/stock-omarchy.png)
- [Lucid desktop](screenshots/lucid.png) and [launcher](screenshots/lucid-launcher.png)
- [Noctalia desktop](screenshots/noctalia.png)
- [Rust prototype after a click](screenshots/lucent.png)

## Remaining checks and scope limits

Wi-Fi/Bluetooth radios, physical brightness and battery hardware are unavailable in these guests. Their hardware interactions, tray interaction, media controls with a real player, screenshot selection UI, sleep/resume, multi-monitor hotplug, fractional/HiDPI scaling, future Omarchy upgrades, animation frame-time distributions and ten-minute idle were not validated. The verified screenshot capture path is `grim`. The secure lock was manually exercised; sleep-triggered locking remains a separate test.

The prepared guests now use a selected weather city, stored only in guest configuration and ignored host state. IP location detection is disabled. Media widgets remain in their idle state without a player. Some Omarchy menu bindings still depend on its stock shell; the common alternative-shell mappings are documented in [shortcuts](../docs/shortcuts.md).

Lucent has no launcher, dock, OS service layer, complete widget API, settings interface or notification server yet. The next work is a minimal component API around real primitives, then a compositor adapter and functional bar. Its framework/client separation is established at the current prototype scale only.

Before/after package inventories and full configuration exports stay in each guest under `~/.local/state/desktop-lab`. Lucid's package baseline was reconstructed from the untouched base clone because its first recorded inventory followed dependency preparation. The original uncorrected inventory is retained separately for audit.
