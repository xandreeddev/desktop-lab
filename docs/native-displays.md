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
edits perform no OS calls. Opening the panel requests a fresh observation while
retaining an existing dirty draft. **Reset draft** requests a fresh complete
layout and replaces that draft only after the read succeeds. Loading disables
editing and Apply; failure leaves the previous draft available with an error.
Apply
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
  transaction identities, configurations, revisioned observations and the
  `DisplayPort` methods `read`, `watch`, `apply`, `keep` and `revert`.
  No socket, file, process, timer implementation or rendering type enters it.
- **Use case:** `lucent-usecases::displays::validate_display_draft` validates a
  whole draft without mutating it or doing IO.
- **Adapter:** `lucent-services::displays::HyprlandDisplays` owns discovery,
  current native authority, child-process admission and observed transaction
  results. Explicit reads and watch captures serialize native queries and assign
  monotonically ordered revisions; a delayed query cannot publish an older
  layout as newer. `native` encodes Lua string literals; `guard` owns the deadline and
  rollback; `persistence` owns captured file identity and atomic Keep.
- **Composition:** `platform::desktop_adapters()` injects the adapter through
  `DesktopAdapters.displays`. The root subscribes once and maps observations
  into `Displays::Message::Observed`; updates delegate into the child.
- **Client:** `displays::Displays` retains a draft, selected output, ordinary
  field/clipboard state, admission revisions and animation. Its borrowed view
  composes the SDK's text fields, buttons, controlled scroll and panel geometry
  using generated `component.displays` tokens. Only explicit actions schedule
  the existing controls queue. Open/Reset capture a request generation before
  scheduling `DisplayPort::read`; `Refreshed` accepts only the still-open current
  request and never replaces a newer observation with an older revision.
  Closing retires the request; a Reset delivered after close performs no read.
  A late transaction admission after close is reverted using its exact identity.
- **Framework:** the existing component, effects, subscription, relative text
  editing, surface and renderer APIs are reused. No display policy is added to
  the generic UI engine.

The guardian dispatch in `main` must remain before desktop creation. Rebuild and
deploy that executable together with its client and adapter. The observer keeps
native output changes and transient guardian results distinct from command
admission, and it preserves error states when the compositor becomes unavailable.

For example, typing X=16 changes only the client's `DisplayConfiguration` draft.
Apply validates it, delegates an effect onto the controls queue, and calls the
injected port. The adapter starts the independent guard, which issues the native
monitor command and checks its observation. A snapshot then reaches the root's
subscription, routes to the child and requests a redraw. The view describes the
confirmation card; the framework lays out and paints that tree. Neither the view
nor the generic renderer calls Hyprland or decides whether the new layout is safe.

## Validation and limits

Source tests cover checked values, complete-layout validation, stale baselines,
last-output protection, disabled-mode parsing and literal Lua encoding. A private
fake driver exercises explicit Revert, timeout, UI-pipe EOF, Keep, failed Keep,
partial native application and a clock error after mutation. Client tests cover
draft-only editing, explicit admission and a late result after closing, fresh
read failure, watch/read ordering, replaced requests and late Reset after close.

`vm/test-display-settings.py` is the guarded native acceptance harness. It uses
real panel input and observed compositor state, with an independent SSH-started
recovery process and private exact-file checkpoints. It is not executed by
ordinary unit tests. The [acceptance report](../reports/measurements/lucent-display-settings.json)
records the compiled source `a7f41ecc`, strict source `a7709d6d`, actual native
harness source `c43df300` and offline validator source `b259ea29` separately.
The compiled source changes only four reviewed loading PNGs from the strict
source; later verification tooling and documentation change no executable code.

The recorded one-output QEMU run completed these seven real-input checks:

1. Open through the compatibility entrypoint and close with Escape.
2. Change an advertised mode draft without changing the native layout.
3. Apply a mode and explicitly Revert.
4. Apply scale and let the independent 15-second deadline restore it.
5. Apply position and Keep it in the user monitor file.
6. Restore the captured native layout and exact original monitor-file inode.
7. Kill the captured UI process and observe EOF rollback and rendered supervisor recovery.

All saved file hashes and identities, settings, exports, media, clipboard/history
and managed scripts remained exact after restoration. The original run's generic
fingerprint assertion **failed** because it required the service PID and restart
counter to remain unchanged after the deliberate crash. That failed report is
retained. A subsequent pure validator checked the private captured before/after
evidence, plus one read-only recovery probe: it permits exactly the verified new
supervisor PID and one restart, and requires unchanged unit/argv/executable/cgroup,
new process births, retirement of the old UI, and rendered recovery beyond the
deadline. This is an explicit post-run acceptance-contract correction, not a
fresh replay of the seven interactions. Negative tests reject unrelated file,
identity, service and recovery-proof changes.

The native test exposed a stale complete-layout cache after an external monitor
edit. Open/Reset now perform the ordered fresh-read path described above; the
accepted run uses that production path. The guest's advertised 74.99 Hz mode
reports exactly 74.994 Hz after application. The harness pins that measured
postimage; no production tolerance was broadened.

Strict formatting/Clippy, 821 Rust tests (76 ignored), 109 Python checks and 582
generated tokens passed at their recorded strict source. Later verification
tooling passed 116 Python checks. All 28 new Display scenarios pass final
non-updating Vulkan pixel comparisons across both palettes at 1×/2×, including
narrow, scrolled-last, loading, confirmation and failure states. These offscreen
pixels do not establish monitor scanout or animation performance.

Host loss between successful Keep and capturing its new file identity
can retain the original hard-link checkpoint for manual recovery; automatic
file restoration is not claimed for that interval. The later two-output result
below adds a scoped transaction check. Physical modes/visibility, native
enable/disable/rotation, cable hotplug, suspend/resume, compositor restart during
confirmation, secure-output changes and input on a fractionally scaled Displays
panel remain unverified. No new RSS or pacing improvement is claimed.

This feature does not add custom modelines, HDR/color-management editing,
mirroring, EDID overrides or a new compositor. It cannot recover hardware that
has disappeared or a killed guardian plus killed independent recovery process.
Configuration reload and physical monitor visibility are distinct from a command
acknowledgement and a submitted client frame.

## Two-output native transaction evidence

The separate `vm/test-multi-output-displays.py` maintainer harness at
`86df4e5bcff5cea3ba85f17e400b88cacd70f4f4` passes four native flows on the
unchanged archived `35ed87ed` desktop. The
[two-output report](../reports/measurements/lucent-native-two-output-displays.json)
keeps the harness source separate from the running release. The original output
remains 1920×1080 at scale 1; one owned headless peer uses 1280×960 at scale 1
and 1.25 during the preview.

1. Editing the peer's position draft changes neither the native layout nor the
   saved monitor configuration.
2. Apply changes only the peer's position; explicit Revert restores the complete
   captured layout.
3. Apply changes the peer to scale 1.25; the independent 15-second deadline
   restores its original scale and layout.
4. Apply changes the peer's position; Keep saves the complete two-output tail
   while preserving the preceding user rules.

All clicks and field edits occur on the original scale-1 output. Changing the
peer's scale does not establish input on a fractionally scaled Displays panel.
Five positive owner-restoration checks cover the original monitor bytes/inode,
removal of only the owned output, original workspace/cursor, unchanged
cohort/settings/media/profiling, and preservation/supervision/security/source
state. The separate ctime/link-count policy marker is not a sixth functional
check: hard-link checkpoints and atomic restoration can advance monitor ctime,
link count and parent entry timestamps. Those fields are not claimed unchanged.
The original owner process and recursive cgroup are independently verified
retired; fresh normal-state facts match the pre-run facts apart from advancing
frame counters, with all profiling OFF.

The initial `7ebd0e84` attempt failed before any feature check: its helper used
`hyprctl eval` to construct a cursor dispatcher instead of executing
`hyprctl dispatch`. Canonical cursor ownership rejected the unobserved movement
and blocked cleanup. That original report remains failed. Its separately recorded
guarded recovery succeeded without converting the test to a pass. The corrected
helper dispatches the move and still
requires its exact observed cursor postimage; it does not reset uncertain cursor
authority. Thirteen focused pure guard tests pass at the corrected harness source.

The operator supplies a reviewed private normal-OFF receipt and its SHA-256
through `--fresh-off` and `--fresh-off-sha256`. Relative receipt paths use
`--lab-root`, which also locates the private VM inventory and keys. Without
`--run` the driver only qualifies local source and receipt bindings; without a
receipt it refuses before VM access. Receipts and inventory stay outside Git.
Its focused guard tests run with `python3 -m unittest discover -s tests -p
test_multi_output_displays_policy.py`.

This result does not repeat the older UI-crash check or accept physical hardware,
cable hotplug, secure-output changes, suspend/resume, native enable/disable or
rotation, fractional-panel input, resources or presentation pacing. Host loss
after Keep but before capturing its new file identity retains the manual recovery
limit described above. Final site, exact-head CI and publication gates for this
follow-up remain separate from its completed native flows.

The supported module name follows [Omarchy’s user configuration](https://github.com/omacom/omarchy/blob/master/config/hypr/hyprland.lua).
The adapter follows the current official [Hyprland monitor Lua API](https://wiki.hypr.land/configuring/core/monitors/)
and queries active and inactive outputs with `monitors all`; native command
transport follows [Hyprland's hyprctl documentation](https://github.com/hyprwm/Hyprland/blob/main/docs/hyprctl.1).
