# Native simultaneous-output acceptance

`vm/test-native-multi-output.py` is an opt-in acceptance tool for the existing
Lucent VM. It adds one uniquely named native Hyprland headless output alongside
the guest's original output. It changes no Rust API, desktop preference,
configuration file, original output mode, wallpaper selection or application.

**Native execution is pending.** The tooling has source checks and owned-fixture
tests; those do not establish that this guest/backend accepts headless input.
A successful future run establishes two simultaneous compositor outputs and
native button interaction. It does not establish physical cable hotplug,
hardware GPU rendering, suspend/resume, multi-GPU behavior or scanout FPS.

## Prerequisites and execution

Run this only in the VM execution lane, with other builds/benchmarks and desktop
test drivers stopped. The VM must already be running and logged into Lucent.
The driver checks the identified release from its private build report; it never
boots, unlocks, builds, deploys or restarts anything.

Required initial state:

- One active, untransformed native output, at most 2560×1440 physical pixels,
  with even extents and scale 1×, exactly one existing ordinary workspace and no
  special workspace. The secondary still exercises 2× and 1.25×.
- No ordinary windows on any workspace, no foreign layer surfaces (every layer
  PID must equal the captured desktop PID, including `lucent-` namespaces), no lock or
  authorization UI, and no Wi-Fi/Bluetooth credential form or pending request.
- Closed idle panels, no notification toasts, no widget editing, saved writable
  preferences, a ready background and settled wallpaper transitions.
- The existing stay-awake marker is present. The operator establishes this
  separately; the tool does not disable lock/idle policy or create the marker.
- Exactly one `lucent-desktop`, owned by the active `lucent.service`, whose
  `/proc/PID/exe` SHA-256 matches `binaries.lucent-desktop.sha256` in the supplied
  build report. Its `source_commit` is recorded as that build's source identity.
- The configured current Lua API supports `hl.monitor`, `hl.dispatch` and
  `hl.dsp.cursor.move`. A backend without headless output/input support is blocked.
- `hyprctl -j version` must report the reviewed v0.56.2 source commit
  `efb50993780079460b0cbed1363e2166a2de1d9f`, with `dirty=false`. Other releases or
  patched builds need a matching source review before admission. The observed
  version and pinned removal source URL enter the private receipt.
- No plain Escape, left-button or catch-all compositor binding may intercept the
  tested input. Caps/Num Lock variants count as plain. Normal Super+mouse binds
  remain permitted. Initial and fresh input guards inspect actual bindings.

From the repository containing the prepared private VM inventory:

```sh
python3 vm/test-native-multi-output.py --run \
  --build-report reports/local/daily-wifi-release-build.json
```

A review worktree can use `--lab-root /absolute/path/to/desktop-lab` to read the
existing inventory and write its private report there. It does not copy SSH keys.
The driver targets only `qemu:///session` / `desktop-lab-lucent` and the prepared
`omarchy` test account. Supply the actual current build receipt, rather than
assuming the example filename identifies a later deployment.

The owner has an absolute 240-second lease. It runs as a uniquely named guest
user unit, with a separate 300-second systemd limit and 20-second stop bound.
Requests cannot extend its deadline. Avoid wrapping the host in an earlier hard
kill: normal Ctrl+C/SIGTERM runs balanced-input cleanup; SIGKILL cannot do that.

## What the driver checks

The original output remains configured throughout. The owned output uses a
bounded 1280×960@60 workload to the right of the original logical rectangle.
Its scale cases are 1×, 2×, 1.25× and back to 1×; the fractional case is an exact
1024×768 logical extent. The test does not reduce density or alter rendering
quality. Its fixed dimensions are a test workload, not a framework policy.

For each case, the driver observes both outputs simultaneously through
`hyprctl -j monitors`, the runtime's `outputs`, and the client's named
`monitor_hosts`. Each host must have its own current native background, bar and
configured widget surfaces. It checks actual logical viewport, density, physical
background buffer extent, layer PID/address, nonempty bar hit geometry and
successful native surface submissions. Original Wayland global, client host
generation and native layer identities must survive the unrelated output change.
The evidence also records live-playback route/payload observations if present;
it does not require or install a video workload.

Each output's audio control is clicked using actual QMP left-button down/up
events. The independent guardian moves the cursor only to an independently
observed allowed bar hit, using its documented Lua dispatcher
and verified independently through `cursorpos`. The driver revalidates the host,
surface and hit geometry after the warp. After potentially slow settling it
rechecks OS/layer/credential/binding/cursor authority and freshly binds the host
generation, native global and hit before pressing. It then verifies
that QMP button delivery did not warp the cursor elsewhere. The audio panel must
actually open on the exact target host and render before Escape closes it.
There is no `lucent-cli audio open` fallback. If real input cannot be established,
the result remains blocked.

Hyprland layer `x/y` are **output-local**, while `cursorpos` is global. Therefore
the tested mapping is:

```text
global cursor = monitor logical origin + native layer local origin + local hit
```

The fractional scale is not applied a second time. Hit centers use the visible
intersection of hit bounds, clip bounds and surface bounds. Selection is by
explicit output, host instance and native namespace/PID; the helper never picks
whichever host happens to be focused.

The hotplug case presses the previously unfocused network button, observes a
fresh pointer-driven focus change on that exact old native surface, and verifies
that the foreground panels remain closed before removal. A cursor warp or QMP
acknowledgement alone does not establish that the down event reached the client.
It removes only the owned output while the release-activated button is pressed,
then waits for that host's Wayland output, client host and
surfaces to retire, releases the button, and verifies no surviving panel opened.
Recreating the same generated name must produce a new Wayland registry global
and larger client host generation. New real input must work afterward. Native
diagnostic surface IDs may legitimately repeat; they are not lifetime authority.

## Ownership, cleanup and private evidence

`vm/multi_output_guard.py` is a separate lease owner. It creates, configures and
removes the output itself through a private Unix socket; the host driver cannot
ask it to mutate an arbitrary output, position, mode or scale. It serializes
test ownership with a private runtime lock. The generated name must be absent
before creation. The owner records the successful creation's exact native
identity, initial layout and Wayland global, then admits only its finite
requested layout transitions. Lost configure/remove replies can reconcile only
their exact authorized postimage/absence; they never authorize another output.

Before each operation and cleanup it checks the captured compositor PID/start
time/socket inode, desktop supervisor/process identity, original-output layout,
owned-output lifetime, unlocked/no-authorization state and absence of ordinary
windows. The original workspace must remain the captured workspace or an empty
workspace created by this lease. A foreign workspace/special-workspace change
blocks restoration. Headless creation never authorizes reuse of a preexisting
workspace.

Cursor authority is separate from output-removal authority. Only the captured
initial position, an independently verified guardian hit warp, or a declared
exact postimage is owned. The host cannot supply arbitrary coordinates. The
pinned v0.56.2 `CMonitor::onDisconnect` moves to the sole backup monitor's
transformed physical center. With the original output at 1× and even extents,
the guardian computes that exact point before its removal, verifies the native
result and accepts it only if the pre-removal cursor was still lease-owned.
An unpredicted backend relocation or user movement permanently retires cursor
authority, even if the cursor later returns to an earlier point. It blocks new
test input/configuration and our workspace/cursor restoration. The independent
owner can still remove its exact unchanged output; that compositor operation
may itself relocate the cursor. A separate `cursor_restored` receipt must pass
before the complete harness can pass. Nothing adopts an unknown physical/QMP
backend warp as a new valid cursor position.

It snapshots user Hyprland configuration directory/file/link identities, exact
settings file identity/hash, the wallpaper symlink/target, referenced live media
and the existing stay-awake marker. Inode, mode, size and timestamps remain
checked during operations; final validation rehashes all preserved files. It
does not copy note text or settings contents into evidence, and it never
overwrites concurrent changes. Configuration trees with directory symlinks,
oversized/unbounded files or too many entries require operator review.

On normal finish, timeout, client loss or a controlled failure, the independent
owner removes **only** its still-identical output, restores the captured ordinary
workspace/cursor, and verifies preservation. If identity/layout or another guard
has changed, it refuses that mutation and retains its private receipt. It never
reloads Hyprland, disables a display, deletes a profile or writes back a backup.
The unique output's runtime monitor rule can remain in Hyprland's in-memory
configuration until a normal later reload; it matches only that generated name
and no configuration file is changed.

The ignored host directory `reports/local/native-multi-output-UUID/` is mode 0700;
receipts are 0600. The independent guest directory is
`/tmp/lucent-native-outputs-RANDOM`, also 0700. `checkpoint.json` records private
identities/hashes; `finished.json` records exact cleanup/preservation outcome.
The host records the build identity and SHA-256 of all three tooling files.
Stdout contains check labels/counts and cleanup booleans, not desktop note text.
Guest receipts are intentionally retained for review rather than deleted by an
unverified cleanup path.

If the host is killed while the button is down, the guest can retire its owned
output but cannot emit a real QMP button release from inside the VM. An operator
must inspect the private receipt and release the exact test-owned left button
through the normal QMP lane before resuming interaction. Likewise, a compositor
or supervisor restart, concurrent layout/config change, blocked guard or lost
creation acknowledgement remains explicit incomplete/blocked evidence. Do not
blindly remove an output by a copied name or call a global reset.

## Checks available without a VM

```sh
python3 -m unittest discover -s tests -p test_native_multi_output.py -v
python3 -m py_compile vm/multi_output_policy.py vm/multi_output_guard.py \
  vm/test-native-multi-output.py
```

Pure tests exercise output-specific selection, negative/global/fractional
coordinates, clipped/nonfinite/ambiguous hits, typed button payloads, stale-host
input retirement and exact cleanup authority. Two real private Unix-socket and
child-process fixtures demonstrate independent expiration after the client
disappears or sends slow partial input, without a finish request or deadline
extension. Their backend is
explicitly fake; it is not a VM, Wayland, GPU or physical-device acceptance test.
An environment that prohibits Unix-socket binding cannot run those two tests.

Primary sources checked for the command/coordinate contract:

- [Hyprland: using hyprctl](https://wiki.hypr.land/configuring/core/advanced-configuration/using-hyprctl/)
  documents headless create/remove, monitors, layers, cursorpos and locked.
- [Hyprland: dispatchers](https://wiki.hypr.land/configuring/core/dispatchers/)
  documents `hl.dsp.cursor.move({x,y})` and workspace focus.
- [Hyprland monitor Lua reference](https://wiki.hypr.land/0.55.0/Configuring/Basics/Monitors/)
  documents logical layout positioning and explicit scale.
- [Hyprland command implementation](https://github.com/hyprwm/Hyprland/blob/main/src/ipc/s1/Commands.cpp)
  confirms Lua dispatch wraps `hl.dispatch(...)`, layer serialization and global cursorpos.
- [Hyprland layer implementation](https://github.com/hyprwm/Hyprland/blob/main/src/desktop/view/LayerSurface.cpp)
  adds monitor position to layer-local geometry when constructing global damage.
- [Reviewed v0.56.2 output removal](https://github.com/hyprwm/Hyprland/blob/efb50993780079460b0cbed1363e2166a2de1d9f/src/output/Monitor.cpp#L439)
  defines the exact declared backup-center cursor transition. Runtime admission
  requires this source commit, rather than inferring installed behavior from main.
- The current Display adapter's `lucent-services/src/displays/native.rs` uses the
  same `hl.monitor({output,disabled,mode,position,scale,transform})` path.
