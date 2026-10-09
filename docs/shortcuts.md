# Test shortcuts

The same window-management, workspace, monitor, input, terminal and application bindings from Omarchy remain loaded. These are the common replacement-shell shortcuts:

| Shortcut | Lucid | Noctalia |
| --- | --- | --- |
| Super+Space | Launcher | Launcher |
| Super+Alt+Space | Launcher | Launcher |
| Super+Ctrl+V | Clipboard launcher mode | Clipboard panel |
| Super+Escape | Session UI | Session panel |
| Super+Ctrl+L / Super+L | Hyprlock | Hyprlock |
| Super+Alt+S | Settings | Settings |
| Super+Ctrl+Space | Wallpaper launcher mode | Wallpaper panel |
| Print | Screenshot UI | Region screenshot |
| Super+Shift+Alt+Comma | Notification panel | Notification panel |
| Super+Shift+Space | Bar settings | Toggle bar |

Other Omarchy shell-specific actions are not silently claimed to work. In particular, theme menus, reminders, capture menus and some hardware menus call stock shell IPC. Use each alternative's Settings/control center or run Omarchy CLI update commands from a terminal. Super+Shift+Space opens Lucid's bar settings because the pinned Lucid revision does not document a bar-toggle IPC target.

Lucid uses `qs ipc call -- TARGET METHOD [ARG]`; the `--` is required before method arguments. Noctalia uses `noctalia msg COMMAND [ARG]`. Inspect the installed IPC help when upgrading; do not substitute legacy Noctalia v4 commands.

## Lucent framework client

| Action | Result |
| --- | --- |
| Super+Space | Rust launcher toggle |
| Super+K | Native searchable Omarchy keybinding menu |
| Super+Ctrl+Space | Wallpaper carousel |
| Type / arrows / Enter / Escape | Search, select, launch/apply, close |
| Widget background left-drag | Show spacing grid; snap and persist on release |
| Launcher Widgets tab | Toggle widgets or reset positions |
| Launcher palette tab | Shared light/dark theme |
| Dock application icon | Launch or focus a running window |
| Top-bar workspace pill | Switch workspace |

Omarchy's terminal, tiling and secure-lock shortcuts remain. The Rust widgets are
Wayland layer surfaces and stay out of the tiled window layout. Close Lucent with
`lucent-cli quit`; its service restores the stock bar. See [usage](../lucent/README.md).

The keybinding menu uses Omarchy's installed `omarchy-menu-keybindings` unchanged.
It discovers active Hyprland bindings and resolves Lua actions; Lucent's native
picker replaces `omarchy-menu-select` through a per-user PATH wrapper. Type an
action or shortcut, use arrows or Tab / Shift+Tab to select, and Enter to execute.
Escape, Close, or an outside click cancels. The picker releases its keyboard
surface before returning the exact original value to Omarchy's dispatcher.
Custom bindings remain discoverable; there is no second hardcoded shortcut list.

Other Omarchy callers of `omarchy-menu-select` and `omarchy-menu-input` use the
same native picker. `omarchy-menu` routes, including nested System, Style and
Setup menus, use native selection with Back/Escape navigation. Actions that call
`omarchy-shell` directly can still open a specialized stock panel. Rollback disables the wrappers even for existing processes
that retain their old PATH.
