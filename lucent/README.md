# Lucent

A native Rust Wayland shell framework and a Lucid-inspired desktop client. The
client implements the floating bar, morphing dock/launcher, wallpaper carousel,
widget selector, dark/light palette and draggable desktop widgets through the
framework API, including native notification toasts/history and separate secure
lock/login clients. Hyprland provides composition and secure locking; PAM and
greetd provide authentication/session startup. Omarchy services remain available
for rollback.

## Try the prepared VM

```sh
python3 vm/launch.py lucent  # from the repository root on the lab host
```

Inside the guest:

- **Super+K:** searchable Omarchy keybindings, rendered by the native `lucent-menu`
  framework client. Search actions or key combinations; arrows / Tab select,
  Enter runs the action, and Escape or an outside click cancels.
- **Super+Space:** open/close the application launcher. Type to filter, arrows to
  select, Enter to launch, Escape or a click outside to close. Tab / Shift+Tab
  cycle launcher sections; arrows and Enter work in Commands, Themes and Widgets too.
- **Super+Ctrl+Space:** wallpaper carousel. Arrows select, Enter applies; clicking
  the centered card applies it too.
- **Widgets** in the launcher header: toggle calendar, clock, weather, media,
  system monitor, notes and focus timer. Reset positions from the same panel.
- Drag a widget's background with the left mouse button. Release snaps its top-left
  corner to the 16-logical-pixel spacing grid and saves the position. Screen edges
  take precedence so the whole widget stays visible.
  Buttons and text fields remain interactive. Widgets live below app windows.
- Workspace pills switch real Hyprland workspaces. Dock icons launch or focus
  their application. Omarchy's usual terminal, tiling and lock shortcuts remain.
- The palette icon offers a shared light/dark theme. Power opens safe commands.

## Build and install

Arch dependencies: `rust clang pam pkgconf wayland libxkbcommon fontconfig vulkan-icd-loader`
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
`bindings.lua`, `autostart.lua` and `looknfeel.lua`, plus a marked PATH block in
`.bash_profile` for lock and native menu-command wrappers. The appearance block applies the shared
panel radius to Foot terminal windows. The service hands off notifications and hides Omarchy's bar **after
the desktop has rendered and Lucent owns the notification bus**. The handoff uses
a guarded stock-shell restart; unlock before first activation. Stopping, crashing or rolling back
restores its previous visibility. Packaged Omarchy configuration is untouched.

`lucent-menu` is a separate, short-lived framework client with an exclusive
Wayland keyboard surface. `MenuEntry` holds display text and the original return
value; the pure `Selection` use case handles search and navigation. The component
never executes actions. The installed Omarchy keybinding script still discovers
and dispatches bindings, including user overrides. Native select/input wrappers
also preserve icon-prefixed options and their subtext return values. Palette,
spacing, rounded corners and opening motion use the shared design tokens.

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

## Design system and high-density rendering

Edit `design/tokens.json`, then run `python3 scripts/generate-design-tokens.py`.
`lucent-design` exposes typed primitive, semantic and component tokens plus
`Theme` recipes. Views use these tokens for colors, type, spacing, corners,
geometry and motion. The reusable engine remains independent of the theme.

Rounded controls use separate horizontal and vertical padding through
`Element::padding_xy`; their hit target retains the full outer bounds. The widget
grid uses `component.widget_layout.grid_step`, which references `space.lg` (16).
Terminal corners use `component.window.radius`, an alias of `radius.panel` (20).
The generator also emits `configs/lucent/window-rules.lua`; rerun activation after
changing that rule. This Hyprland version supports a maximum radius of 20.

`design/app-icons.json` maps exact desktop IDs to original Lucent pictograms. Edit
the catalog and run `python3 scripts/generate-app-icons.py` to regenerate SVGs and
the typed Rust catalog. Both the dock and launcher tint the monochrome artwork
with the semantic foreground on a transparent background. Icons are rasterized at
128 pixels for high-density rendering. Unknown IDs retain their installed XDG
icon, with a Lucent fallback when it is unavailable. The local design-system page
includes the complete icon gallery.

Text uses supersampled coverage, kerning-aware measurement and physical-pixel
alignment. Images prefer larger/vector originals, with filtered mip levels for
animated reductions. The output buffer still follows the compositor's integer
scale; fractional scaling is not implemented. The current asset budgets target
up to 2× density. See the local site's `/design-system/` and `/docs/` pages.

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

For a fresh disposable lab guest, the reference assets and private location are
reproducible with the existing pinned-source tooling:

```sh
python3 scripts/setup-look.py lucent
python3 scripts/set-weather.py lucent /path/to/private-location.json
systemctl --user restart lucent.service
```

The location file contains `name`, `latitude` and `longitude`; keep it outside Git.

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
python3 vm/test-design-system.py  # themes + temporary 4K/2×, restores the output
```

See [the validation report](../reports/lucent-framework.md) for executed checks
and measurements. The old card prototype and its tests are historical milestones.

The prepared VM uses **software Vulkan (Mesa lavapipe)**. The host GPU is not
passed through; the compositor uses virgl. [Graphics details](../docs/lucent-vulkan.md).
The reference layout and primary launcher/selector transitions are implemented;
this is not complete Lucid feature or pixel parity. Tray hosting, clipboard/emoji modes, custom control-center dialogs, automatic
wallpaper palette extraction, widget resizing, fractional scaling, multi-output
placement and full Unicode shaping remain future work. Notification history is
in memory; images, markup, inline replies and sound are not implemented. Stock Omarchy continues
handling session services. The framework has no Qt/GTK dependency.

Native visual tests render the real client through Vulkan at 1× and 2×. Run
`python3 vm/test-visual.py` from the repository root to use the prepared VM, or
`cargo test --manifest-path lucent/Cargo.toml -p lucent-desktop visual_regressions -- --ignored`
with a local Vulkan driver. Review `reports/local/visual-tests/index.html`.
`python3 vm/test-launcher.py` verifies actual keyboard events in the unlocked VM.
See [keyboard state and visual tests](../docs/lucent-framework.md#keyboard-state-and-visual-regression-checks)
for baseline review and CI behavior.


## Native lock and login

The bar lock button and **Super+Ctrl+L** request `lucent-lock.service`, which renders
through the same framework on secure Wayland lock surfaces. The installed Omarchy
PAM policy verifies the current account; Esc never unlocks. The idle and sleep-lock commands route through narrow user-level wrappers; stock
locking remains a recovery fallback. Notification history opens from the bar's bell or
`lucent-cli notifications toggle`.

Login integration is a separate root-level step in the test VM:

```sh
sudo pacman -S --needed greetd
sudo python3 scripts/lucent-login.py install
sudo python3 scripts/lucent-login.py test       # isolated VT9 greeter
sudo systemctl stop lucent-greeter-test
sudo python3 scripts/lucent-login.py activate  # selects the next boot's manager
```

The login installer backs up the existing display-manager selection and greetd
configuration. It never ends the running desktop. Roll back with
`sudo python3 scripts/lucent-login.py rollback` before rebooting. Keep a TTY or SSH
path available while testing login integration. The greeter supports PAM's visible
and secret prompts; no autologin, credential storage, or improvised authentication.


Lock and login screens use the selected desktop wallpaper with centered cover
fitting. The locker loads Omarchy's current background asynchronously, so image
decoding does not delay secure locking. Missing images leave an opaque fallback.
The login installer publishes one account's wallpaper under
`/var/lib/lucent/wallpapers/<uid>/wallpaper`. The directory belongs to that account
and the greeter group (2750); the copied file is 0640. The home remains private.
`lucent-wallpaper-sync.path` follows changes and copies atomically without sudo.
Use `--wallpaper-user ACCOUNT` when installing as root without `SUDO_USER`.
PNG, JPEG and WebP retain source detail up to a 4096-pixel edge.
