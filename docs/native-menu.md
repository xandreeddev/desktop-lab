# Native Omarchy menus

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
A submenu can request a Back control; Back or Escape returns status 3 without
a value. Disabled entries remain visible, cannot activate, and are skipped by
keyboard selection. Close and outside clicks cancel the entire menu tree.
Only one picker runs at a time. Input text and return values are omitted from
the diagnostic socket. This picker is for ordinary menu input, not passwords.

## Menu routes and adapter boundary

Selecting System, Style, Setup and other `omarchy-menu` actions now opens the same
native picker, including nested menus. The integration reads the installed
`default/omarchy/omarchy-menu.jsonc` and merges user extensions per field. It does
not maintain a duplicate shortcut list.

| Layer | Responsibility |
| --- | --- |
| `scripts/menu_model.py` | Pure definition parsing, merging, aliases, links and entry projection; condition evaluation is supplied by the caller |
| `scripts/lucent-menu-routes.py` | Omarchy adapter: source files, condition processes, dynamic providers, navigation orchestration and dispatch |
| `lucent-domain` / `lucent-usecases` | Typed entries/outcomes and pure search/selection policy |
| `apps/lucent-menu` | Presentation and interaction through the public Component API; no filesystem or command execution in the component |
| Framework crates | Generic layout, input, animation, Wayland and Vulkan; no Omarchy route knowledge |

Native replacements use explicit route IDs: `style.background` opens Lucent's
wallpaper selector, `style.theme` lists themes in the native picker, and the apps
provider opens Lucent's launcher. User extensions are merged **after** these
defaults, so custom actions retain precedence. Fonts and themes use installed
Omarchy list/current/set commands. The adapter also supports the power-profile
provider used by newer definitions; that provider is not present in the tested
4.0.4 menu source.

Native/provider actions remain argument arrays. Existing action and condition
strings are trusted executable Omarchy/user configuration and keep their shell
semantics. Search text, display labels and returned selection IDs never become
shell code. Results must match an offered, enabled entry before dispatch. A
condition timeout aborts navigation instead of enabling an uncertain action.

Each navigation opens a short-lived picker and releases its surface before the
next step. Back currently returns to a fresh parent search/selection state; it
does not preserve a navigation history or animate between menu pages.

## Installation and rollback

Build the workspace and run `scripts/lucent-setup.py install` then `activate`.
The three menu wrappers (`omarchy-menu`, `omarchy-menu-select`, and
`omarchy-menu-input`) live in `~/.local/lib/lucent/bin`; packaged Omarchy files
and the Super+K binding are unchanged. The existing PATH integration selects
the wrappers. `rollback` disables them before removing the PATH block, so even
long-running applications with the old PATH return to stock selection UI.

The stock shell still supplies background, idle, polkit and specialized panels.
An action that calls `omarchy-shell` directly may still open a stock panel; this
is not a claim that all stock shell services have been replaced.

## Verification

- Unit tests cover search by action/key, duplicate labels, original return
  values, empty results, cancellation, keyboard focus, disabled rows, parent
  navigation and uncropped last rows.
- Python tests preserve the installed Omarchy caller's argument/value contract.
- Native visual fixtures cover normal, narrow/scrolled, empty and input states
  in dark/light palettes at 1× and 2×.
- `python3 vm/test-desktop-polish.py` checks native submenus reached through
  Super+K, parent navigation, theme/font providers, drag grid and browser corners.
- `python3 vm/test-menu.py` tests real Super+K, keyboard search and navigation,
  action dispatch, a temporary custom binding, cancellation and input in the VM.

The geometry and screenshot fixtures use synthetic data; live screenshots stay
in ignored local reports.
