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

## Lucent prototype

Use the stock Omarchy keyboard shortcuts. The Rust card has its own pointer controls:

| Action | Result |
| --- | --- |
| Left-button drag anywhere on the card | Move it; release saves its position |
| Left-click without dragging | Animate between blue and teal |
| Hover / press / release | Animate highlight and scale |
| Right-click | Fade out and close the prototype |
| Click outside its rounded outline | Interact with the application below |

Launch **Lucent Prototype** again from the menu to restore the saved position. No modifier key is needed to drag it, and it does not enter Hyprland's tiled window layout.
