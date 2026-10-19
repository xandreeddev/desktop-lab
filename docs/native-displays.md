# Native display settings

Lucent's Displays panel edits a complete draft and sends it through the pure
`DisplayPort`. The Hyprland adapter supplies the implementation. The compositor
still owns mode setting, output placement and window/workspace migration; the
panel uses the same SDK controls and semantic tokens as the rest of the desktop.

Open it with `lucent-cli displays open`. The existing Omarchy display-menu bind
also opens this child through the compatibility adapter. No Omarchy display UI
or second shell is launched. Brightness remains in the Power panel.

Select a connected output, enable or disable it, choose an advertised resolution
and refresh rate, enter its logical X/Y position and scale, or rotate it. Draft
edits perform no OS calls. Reset draft reimports the current observation. Apply
validates the whole layout, including unique current outputs, unchanged baseline,
advertised modes, integral logical dimensions and at least one enabled output.
Negative positions are supported. Overlapping outputs are intentional valid
placements; mirroring is not synthesized from overlap. Native mirror and advanced HDR/active-VRR setups are
shown as noneditable because monitor observations cannot reconstruct their full
policy. Unchanged such outputs are not reapplied or added to the saved tail.

## Apply, Keep and Revert

Apply admits one transaction. Its identity is transient and must never be saved.
The adapter starts the currently running desktop executable with the private
`--display-guard` mode, before loading any Wayland or GPU runtime. The independent
Rust process:

1. Captures the current Hyprland socket identity and a bounded, owned, regular
   user `hypr/monitors.lua` file. It checks that the supported `hypr.monitors`
   module is loaded and compiles the proposed file without executing it.
   A symlink or ambiguous Lucent tail block is
   rejected before the first display mutation.
2. Reobserves and validates the complete baseline and draft. Stale drafts are
   rejected instead of overwriting a change made by another program.
3. Enables requested outputs before disabling others, applies Lua monitor
   requests for changed outputs in one bounded native exchange, then checks the actual observed mode, scale, position and rotation.
   A native `ok` reply alone does not establish success.
4. Starts a 15-second confirmation deadline before the first mutation. The
   Linux boot-time clock includes suspend. A client exit or closed pipe also
   triggers restoration; compositor frame delivery and the UI loop are not
   required for that rollback.
5. Keeps revalidating current outputs during confirmation. Timeout, Revert,
   closing the panel, a changed display set or failed persistence restores the
   captured configuration for still-connected original outputs. An armed scope guard
   also makes one bounded retry if an error interrupts restoration. It retains
   recovery intent rather than silently accepting an unverified layout. The guardian
   reports failure if restoration cannot be verified, rather than claiming that
   disconnected hardware was restored.

Keep is a separate explicit action. It first rechecks the live preview and
revalidates the captured file bytes and inode. It stages a single marked tail
block in the existing user `monitors.lua`, leaving all preceding user rules
intact, syncs it, atomically exchanges the file, verifies the outgoing inode and bytes,
and syncs the directory. A
hard-link checkpoint retains the original inode through the commit. An unexpected
second external replacement leaves recovery checkpoints for inspection instead
of overwriting that replacement. Files that
changed concurrently are left alone and the display preview is reverted.
Packaged Omarchy files are never modified. Keep does not trigger a full Hyprland
reload or rewrite widgets, wallpaper preferences, theme exports or keybindings.
The normal user monitor module loads the saved tail block on the next config
load. A successful Keep changes the monitor file inode by design; an unapplied
or reverted draft leaves it intact.

The marked block is `-- BEGIN LUCENT DISPLAY CONFIGURATION` through
`-- END LUCENT DISPLAY CONFIGURATION` at the end of the file. Removing that
block restores the preceding user rules on the next normal reload. A recovery
session can always reapply known-good monitor settings over SSH. Do not disable
all outputs manually while debugging.

## Boundaries and loading path

- **Domain:** `lucent-domain::displays` contains checked mode, scale, position and
  transaction identities, configurations, observations and the `DisplayPort`.
  No socket, file, process, timer implementation or rendering type enters it.
- **Use case:** `lucent-usecases::displays::validate_display_draft` validates a
  whole draft without mutating it or doing IO.
- **Adapter:** `lucent-services::displays::HyprlandDisplays` owns discovery,
  current native authority, child-process admission and observed transaction
  results. `native` encodes Lua string literals; `guard` owns the deadline and
  rollback; `persistence` owns captured file identity and atomic Keep.
- **Composition:** `platform::desktop_adapters()` injects the adapter through
  `DesktopAdapters.displays`. The root subscribes once and maps observations
  into `Displays::Message::Observed`; updates delegate into the child.
- **Client:** `displays::Displays` retains a draft, selected output, ordinary
  field/clipboard state, admission revisions and animation. Its borrowed view
  composes the SDK's text fields, buttons, controlled scroll and panel geometry
  using generated `component.displays` tokens. Only explicit actions schedule
  the existing controls queue. A late admission after close is reverted using
  its exact identity.
- **Framework:** the existing component, effects, subscription, relative text
  editing, surface and renderer APIs are reused. No display policy is added to
  the generic UI engine.

The guardian dispatch in `main` must remain before desktop creation. Rebuild and
deploy that executable together with its client and adapter. The observer keeps
native output changes and transient guardian results distinct from command
admission, and it preserves error states when the compositor becomes unavailable.

## Validation and limits

Source tests cover checked values, complete-layout validation, stale baselines,
last-output protection, disabled-mode parsing and literal Lua encoding. A private
fake driver exercises explicit Revert, timeout, UI-pipe EOF, Keep, failed Keep,
partial native application and a clock error after mutation. Client tests cover
draft-only editing, explicit admission and a late result after closing.

`vm/test-display-settings.py` is the guarded native acceptance harness. It uses
real panel input and observed compositor state, with an independent SSH-started
recovery process and private exact-file checkpoints. It is not executed by
ordinary unit tests. Native acceptance is separate from these source fixtures;
record actual commands, executable identity, checks and restoration results when
it runs. Host loss between successful Keep and capturing its new file identity
can retain the original hard-link checkpoint for manual recovery; automatic
file restoration is not claimed for that interval. Multi-output hardware, output unplugging, suspend/resume and monitor
modes not exposed by the test guest require their own acceptance records.

This feature does not add custom modelines, HDR/color-management editing,
mirroring, EDID overrides or a new compositor. It cannot recover hardware that
has disappeared or a killed guardian plus killed independent recovery process.
Configuration reload and physical monitor visibility are distinct from a command
acknowledgement and a submitted client frame.

The supported module name follows [Omarchy’s user configuration](https://github.com/omacom/omarchy/blob/master/config/hypr/hyprland.lua).
The adapter follows the current official [Hyprland monitor Lua API](https://wiki.hypr.land/configuring/core/monitors/)
and queries active and inactive outputs with `monitors all`; native command
transport follows [Hyprland's hyprctl documentation](https://github.com/hyprwm/Hyprland/blob/main/docs/hyprctl.1).
