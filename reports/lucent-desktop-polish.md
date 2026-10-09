# Native submenus, drag grid and window radius

Validated on the prepared Lucent guest: Omarchy 4.0.4, Hyprland 0.56.2,
6 vCPUs, 6 GiB RAM, Mesa software Vulkan, 1920×1080 at scale 1.

The desktop now shows a decorative spacing grid during widget dragging and
hides it on release. Positions still snap to the 16-logical-pixel spacing token.
Native menus now handle installed Omarchy root/submenu definitions, including
System, Style, themes and fonts, with Back and disabled-action support. Ordinary
app corners use the shared 20-pixel token through Hyprland's decoration default.

The implementation keeps four distinct responsibilities:

- Pure menu definitions and Rust selection use cases contain no OS execution.
- The Omarchy adapter reads installed definitions and user extensions, evaluates
  availability, and dispatches accepted actions after the picker releases focus.
  Native routes use explicit IDs; user action overrides retain precedence.
- The native menu and drag-grid views compose the existing framework API.
- Hyprland owns browser/window clipping. The token generator emits its user
  configuration; there is no Chrome class special case in the renderer.

Executed validation:

- 49 Rust unit tests passed. The Vulkan comparison and notification D-Bus tests
  remain separate opt-in tests; Vulkan was run as described below.
- 29 Python tests passed, including architecture boundaries, user override
  precedence, invalid/disabled selection rejection and reversible configuration.
- Workspace formatting and all-target Clippy passed with warnings denied.
- All 84 Vulkan visual comparisons passed. Eight new reference images were
  reviewed for the drag grid and submenu in dark/light at 1×/2×. The preceding
  76 references remained unchanged.
- 12 live checks passed in `vm/test-desktop-polish.py`: Chromium and a Chrome app
  window both reported radius 20; Hyprland reported no config errors; real
  Super+K dispatched the native System submenu; Back/Escape/Close worked;
  theme/font providers rendered natively; the grid appeared while held, vanished
  on release and matched the snapped position. The test restored widget positions
  and workspace, retaining open applications and the selected wallpaper.

- All 14 existing live menu checks also passed: binding discovery, real keyboard
  input, stock terminal dispatch, a temporary custom binding, exact select/input
  return values and cancellation.
- Astro check and production build passed with no diagnostics.

Run the focused check with an unlocked VM, a visible calendar widget and a
Chrome/Chromium window. Live screenshots and machine-readable results are in
ignored `reports/local`; public image fixtures contain synthetic data.

Remaining limits: specialized actions that call stock shell IPC can still open
stock panels. Background, idle and polkit services remain stock. Menu navigation
starts a fresh picker per page and resets parent search state. Widgets can
still overlap. Explicit compositor app rules and fullscreen/gapless policies
retain precedence over the default radius. These tests do not establish physical
multi-monitor, fractional scaling or hardware-GPU behavior.
