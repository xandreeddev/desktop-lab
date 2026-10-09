# Native Omarchy keybinding menu

Press **Super+K**. Lucent renders the search input, selected rows and shortcut
descriptions using its Component API, layout engine, design tokens and Vulkan
renderer. The opening animation uses the shared enter duration/easing; selection
fill and outline update together without a trailing animation.

## Ownership

```text
Hyprland: existing Super+K binding
  → installed omarchy-menu-keybindings: discover bindings and Lua actions
  → user omarchy-menu-select wrapper: adapt options to a typed JSON request
  → lucent-menu: Selection use case → Component → Wayland/Vulkan
  → original selected value, after the surface is destroyed
  → installed omarchy-menu-keybindings: dispatch the chosen action
```

The menu is an independent framework client, so a picker failure does not take
down the desktop. Its domain entries contain labels, details and return values;
search/navigation have no OS dependencies. The executable owns stdin/stdout.
The UI component never starts a shell or executes a caller-supplied command.

The same bridge implements Omarchy's `select` and `input` contracts: positional
or stdin options, width/height hints, icon-prefix removal and exact subtext
return values. Requests are bounded to 2 MiB and 10,000 entries. Cancellation
returns status 1, failure status 2, acceptance status 0 and the chosen value.
Only one picker runs at a time. Input text and return values are omitted from
the diagnostic socket. This picker is for ordinary menu input, not passwords.

## Installation and rollback

Build the workspace and run `scripts/lucent-setup.py install` then `activate`.
The two menu wrappers live in `~/.local/lib/lucent/bin`; packaged Omarchy files
and the Super+K binding are unchanged. The existing PATH integration selects
the wrappers. `rollback` disables them before removing the PATH block, so even
long-running applications with the old PATH return to stock selection UI.

The full stock shell is still running for remaining services. Root menus that
call shell IPC directly are not converted by this change.

## Verification

- Unit tests cover search by action/key, duplicate labels, original return
  values, empty results, cancellation, keyboard focus and uncropped last rows.
- Python tests preserve the installed Omarchy caller's argument/value contract.
- Native visual fixtures cover normal, narrow/scrolled, empty and input states
  in dark/light palettes at 1× and 2×.
- `python3 vm/test-menu.py` tests real Super+K, keyboard search and navigation,
  action dispatch, a temporary custom binding, cancellation and input in the VM.

The geometry and screenshot fixtures use synthetic data; live screenshots stay
in ignored local reports.
