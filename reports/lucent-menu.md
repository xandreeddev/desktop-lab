# Native keybinding menu validation

Tested on the prepared Lucent VM: Omarchy 4.0.4, Hyprland 0.56.2,
6 vCPUs, 6 GiB RAM, Mesa software Vulkan, 1920×1080 at scale 1.

Super+K opens the native `lucent-menu` client. Omarchy's installed keybinding
script still discovers bindings and dispatches actions. No packaged bindings or
Omarchy shell sources were edited. The full stock shell still supplies remaining
background, idle, polkit and shell IPC services.

Executed checks:

- 46 Rust unit tests passed; the separate Vulkan and D-Bus tests are opt-in.
- 21 Python tests passed, including argument/value compatibility and rollback.
- Cargo formatting and workspace/all-target Clippy passed with warnings denied.
- All 76 native Vulkan visual comparisons passed, including 16 new menu images
  covering four states, two palettes and two pixel densities. Existing images
  remained unchanged. Reference data is synthetic.
- 14 live checks passed: Super+K, binding inventory, Tab/Shift+Tab, arrow scrolling,
  focused search, stock terminal launch, focus release before dispatch, empty
  result safety, Escape, discovery and dispatch of a temporary custom Lua
  binding, select subtext return value, text input and outside-click cancellation.
- The temporary binding and its marker file were removed. The test terminal was
  closed. No permanent test shortcut was added.
- Astro check and production build passed.
- A settled menu's frame counter stayed unchanged across an idle interval.

The user-selected Hyprland girl wallpaper was copied from the installed host
asset into the guest's wallpaper collection. Omarchy's persistent background
link selects it, and the login publisher's bytes match. The source artwork and
live screenshots are not added to repository fixtures.

Run `python3 vm/test-menu.py` for real input checks and
`python3 vm/test-visual.py` for the Vulkan comparisons. Local screenshots and
machine-readable results live in ignored `reports/local`.
