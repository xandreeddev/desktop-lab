# Lucent

A native Rust Wayland shell framework and a Lucid-inspired desktop client. The
client implements the floating bar, morphing dock/launcher, wallpaper carousel,
widget selector, dark/light palette and draggable desktop widgets through the
framework API. Omarchy still provides the compositor, secure lock, notifications,
background surface and session services.

## Try the prepared VM

```sh
python3 vm/launch.py lucent  # from the repository root on the lab host
```

Inside the guest:

- **Super+Space:** open/close the application launcher. Type to filter, arrows to
  select, Enter to launch, Escape or a click outside to close.
- **Super+Ctrl+Space:** wallpaper carousel. Arrows select, Enter applies; clicking
  the centered card applies it too.
- **Widgets** in the launcher header: toggle calendar, clock, weather, media,
  system monitor, notes and focus timer. Reset positions from the same panel.
- Drag a widget's background with the left mouse button. Release saves position.
  Buttons and text fields remain interactive. Widgets live below app windows.
- Workspace pills switch real Hyprland workspaces. Dock icons launch or focus
  their application. Omarchy's usual terminal, tiling and lock shortcuts remain.
- The palette icon offers a shared light/dark theme. Power opens safe commands.

## Build and install

Arch dependencies: `rust pkgconf wayland libxkbcommon fontconfig vulkan-icd-loader`
and a Vulkan driver. Desktop adapters use `playerctl`, `wireplumber`,
`networkmanager` and `curl`; missing services show their unavailable state.

```sh
cargo build --manifest-path lucent/Cargo.toml --locked --release --workspace
python3 scripts/lucent-setup.py install
systemctl --user start lucent.service
# After checking the running client:
python3 scripts/lucent-setup.py activate
```

Installation is per-user. Activation adds marked blocks to Hyprland's user
`bindings.lua` and `autostart.lua`. The service hides only Omarchy's bar **after
all three Lucent surfaces have rendered**. Stopping, crashing or rolling back
restores its previous visibility. Packaged Omarchy configuration is untouched.

```sh
python3 scripts/lucent-setup.py rollback
```

Rollback removes the managed blocks, restores stock shortcuts and stops Lucent.
It retains binaries, widget settings and the last selected wallpaper. To restore
the prepared VM's original wallpaper as well:

```sh
omarchy-theme-bg-set "$(cat ~/.config/lucent/original-wallpaper)"
```

## API and clients

See [the framework guide](../docs/lucent-framework.md) for concepts, boundaries,
examples and the actual supported API. `examples/hello-layer` is an independent
counter application using the same layout, input, animation and Vulkan runtime:

```sh
cargo run --manifest-path lucent/Cargo.toml --locked --release -p hello-layer
```

The desktop has no raw Wayland objects or GPU commands. Its domain entities and
use cases have no UI or operating-system dependencies. The renderer shares one
Vulkan device and a bounded image/text cache across surfaces. Repainting follows
invalidation and compositor frame callbacks; animations stop at their endpoint.

## State and IPC

`~/.local/state/lucent/desktop.json` stores the versioned layout, visible widget
IDs, pinned app IDs, notes and palette choice. Saves are atomic. Invalid versions
are left intact and disable saving until corrected. Notes are currently one line.
A running timer uses a deadline and catches up after delayed callbacks; it is
not persisted across a restart.

Place wallpapers in `~/Pictures/Wallpapers` or `~/.config/lucent/wallpapers`.
Weather is optional: `~/.config/lucent/weather.json` accepts numeric `latitude`
and `longitude`; conditions come from Open-Meteo every 15 minutes. No IP lookup
is performed and personal location is excluded from this repository.

```sh
lucent-cli launcher toggle  # also open / close
lucent-cli wallpapers open
lucent-cli widgets open
lucent-cli inspect
lucent-cli quit
```

IPC is a mode-0600 Unix socket in `$XDG_RUNTIME_DIR/lucent.sock`. The command
allowlist never evaluates shell text. Inspect includes frame counters and hit
geometry for real-input tests; it also includes the current query and note, so
review it before sharing. A file lock prevents duplicate clients.

## Validation and limits

```sh
cargo fmt --manifest-path lucent/Cargo.toml --all --check
cargo clippy --manifest-path lucent/Cargo.toml --locked --workspace --all-targets -- -D warnings
cargo test --manifest-path lucent/Cargo.toml --locked --workspace
python3 vm/test-framework.py  # unlocked, prepared 1920×1080 VM
```

See [the validation report](../reports/lucent-framework.md) for executed checks
and measurements. The old card prototype and its tests are historical milestones.

The prepared VM uses **software Vulkan (Mesa lavapipe)**. The host GPU is not
passed through; the compositor uses virgl. [Graphics details](../docs/lucent-vulkan.md).
The reference layout and primary launcher/selector transitions are implemented;
this is not complete Lucid feature or pixel parity. Notification history, tray
hosting, clipboard/emoji modes, custom control-center dialogs, automatic
wallpaper palette extraction, widget resizing, fractional scaling, multi-output
placement and full Unicode shaping remain future work. Stock Omarchy continues
handling session services. The framework has no Qt/GTK dependency.
