# Genuine third-party tray acceptance

`vm/test-third-party-tray.py` prepares a reproducible acceptance run with the
installed **NetworkManager Applet**, rather than Lucent's owned protocol fixture.
This document describes test tooling. **No genuine application or VM run has
been accepted for this new harness yet.** The existing controlled SNI results
remain valid within their original scope; they do not establish compatibility
with a third-party application.

The harness changes no domain, adapter, framework, renderer or desktop code.
Lucent's existing `TrayPort` adapter owns the watcher, item discovery and
DBusMenu calls. Its client owns the themed bar icon and menu. The external
`nm-applet` process exports the real item and menu, and receives the selected
action. Our Python helper is an observer and test owner; it does not implement
an SNI producer, register another watcher or invoke menu actions directly.

## What an accepted run would establish

The source-specific private receipt must contain every step and successful
cleanup before it can be called a pass:

1. The installed and running Lucent binary match the supplied build digest.
   Its process, active supervisor, native compositor socket and sole output
   remain unchanged.
2. A fresh, bounded user unit execs the identified installed
   `/usr/bin/nm-applet --indicator --no-agent`. Its PID, start time, executable
   digest, argument array, private marker, exact cgroup and systemd invocation
   match the pre-exec ownership receipt. Private XDG directories and the
   in-memory GSettings backend isolate its preferences.
3. The actual SNI watcher still belongs to that Lucent desktop. One actual
   registered item and the applet's application bus name resolve to the exact
   owned applet process. The item supplies real icon metadata and appears as
   a native Lucent bar control with actual submitted frames.
4. Actual QMP right-button input opens Lucent's native menu. A bounded
   read-only `GetLayout` identifies the exact, enabled, visible root leaf
   **Connection Information**, after GTK mnemonic normalization. A submenu,
   toggle, editor, fuzzy label, duplicate ID or changed owner/revision is
   rejected. Actual QMP left-button input selects that same native menu row.
5. One real Wayland window titled **Connection Information** appears, with
   the exact owned applet PID and expected application ID. No window contents,
   IP addresses or profile names are copied into public reports.
6. Only the exact owned user unit is stopped. Its process/cgroup are quiescent,
   its application bus owner/item/native menu disappear, and preserved state
   matches the checkpoint. The cursor returns to its original position only
   while its exact input ownership remains valid.

This is a bounded single-application, single-output acceptance slice. It does
not establish physical Wi-Fi/Bluetooth operation, secret-agent behavior,
notifications, clipboard interoperability, nested menus, every application,
icon pixel fidelity, presentation FPS or GPU acceleration. `frames` and
`native_frames` are submission evidence, not presentation feedback.

## Why the guest must be wired only

The official applet source parses `--no-agent` by disabling its own secret
agent and handles `--indicator` only when indicator support was compiled in.
It uses application ID `org.freedesktop.network-manager-applet`.
See [upstream main.c](https://github.com/NetworkManager/network-manager-applet/blob/main/src/main.c).

Opening its indicator menu can request a scan for every Wi-Fi device. The
tool therefore requires all NetworkManager devices to be Ethernet or
loopback, with exactly one already active Ethernet connection. It never
disables a device or radio to create that precondition. Its safety boundary
must be reviewed again if the installed applet's behavior changes.
See the scan callbacks in [upstream applet.c](https://github.com/NetworkManager/network-manager-applet/blob/main/src/applet.c).

The same source connects **Connection Information** to the read-only
information dialog; indicator mode does not provide the ordinary About menu
item. The test does not invoke connection entries, networking toggles,
notification toggles, VPN actions or **Edit Connections**. The dialog's source
is [upstream applet-dialogs.c](https://github.com/NetworkManager/network-manager-applet/blob/main/src/applet-dialogs.c).

Startup does run the ordinary connected-device notification path. In the
reviewed upstream `applet_do_notify`, the shared notification function returns
before sending whenever `applet->agent` is absent; `--no-agent` is therefore
also significant for UI-history preservation. This is a source-dependent
constraint, not an assumption that an idle NetworkManager snapshot guarantees
no notifications. The helper additionally preserves the existing Lucent
`notifications.json` identity/hash and requires no active toasts. Unexpected
notifications or a history-file change block acceptance; they are not globally
cleared or restored. A changed installed source must be reviewed before relying
on this behavior. The process-local GSettings backend is `memory`; no host or
shared GSettings preference is changed to silence the applet.

Profile preservation uses only `ListConnections`, documented **nonsecret**
`GetSettings`, and property observations. Only hashes of settings reach the
private checkpoint; no `GetSecrets`, update, save, activation, enable or scan
method is called by the helper. Independent daemon/profile/radio/connection
changes invalidate the receipt and are never overwritten.
See [NetworkManager Settings.Connection](https://networkmanager.dev/docs/api/latest/gdbus-org.freedesktop.NetworkManager.Settings.Connection.html).

## Preconditions and command

Use an already running, prepared `desktop-lab-lucent` session VM. The test does
not boot, log in, unlock, install packages, change resolution, close foreign
applications or stop the normal desktop. Pause other native acceptance jobs
and wait for the shared heavy lane before running it.

Required guest state:

- Existing official Arch `network-manager-applet` package, root-owned installed
  binary and compiled indicator support; Python's `dbus` module is also needed.
  Absence is unsupported, with no automatic installation or XEmbed fallback.
- One 1920×1080 output at origin `(0, 0)`, scale 1, normal transform and DPMS on.
  Absolute QMP delivery must reach each declared point exactly. Other input
  backends/layouts are blocked rather than approximated.
- Empty native tray, no pre-existing nm-applet process/application bus owner,
  no ordinary windows on any workspace, no authentication surface, no pending
  credentials, no unrelated open/busy panel and no notifications.
- Every existing layer belongs to the captured Lucent desktop PID. A matching
  `lucent-` namespace by itself grants no input authority.
- Existing stay-awake marker, ready background, saved preferences, and no
  wallpaper operation. The test preserves the current idle policy.
- No unmodified left/right-button, Escape or catch-all compositor binding
  that could intercept the test, including Lock/NumLock variants.
- Bounded existing settings/configuration/media: 64 profiles/devices/active
  connections, 576 preserved files, at most 512 MiB per referenced file and
  1 GiB total. A larger or unreadable state is explicitly blocked.

Once the operator has arranged those conditions and deployed the identified
release, run from this repository/worktree:

```sh
python3 vm/test-third-party-tray.py --run \
  --lab-root /absolute/path/to/desktop-lab \
  --build-report /absolute/path/to/source-specific-release-build.json
```

The existing build receipt must provide `source_commit` (40 hex digits) and
`binaries.lucent-desktop.sha256`. The harness records both and the hashes of
its three tooling files. It requires the existing session-only inventory,
`omarchy` account and port 2243; it never copies the private SSH key.

## Preservation, cancellation and cleanup

Private host receipts are created under ignored `reports/local/third-party-tray-*`
with directory mode 0700 and file mode 0600. Guest scripts/checkpoints/logs remain
in one exact new `/tmp/lucent-third-party-tray-<24 hex>` directory with the same
privacy. They are retained for review; there is no recursive shared-path cleanup.

The checkpoint preserves settings, wallpaper link/target, referenced live media,
Hyprland configuration, existing nm-applet configuration, dconf database and
stay-awake marker, plus Lucent's notification history, including file/directory
identity and hashes where applicable. Custom XDG state/config roots are blocked.
No shared preference/profile is intentionally modified or restored. An
independent edit causes failure; a checkpoint is not permission to overwrite it.
The applet gets its own config/cache/data directories and memory-only GSettings.

Fresh unlocked/layer/PID/output/binding checks follow potentially slow profile
or menu observations and precede physical button down and release. The native
target's layer address, SDK instance, geometry, and menu ID/owner/revision must
remain current. If an owned press becomes stale while the session remains
admissible, Escape cancels Lucent's capture before balanced button release.
After guard loss, the harness sends no new keys or pointer moves, while still
attempting release of its own held button.

Cursor ownership is sticky: any observed coordinate outside the captured
position or declared owned move permanently removes input/restoration authority.
The tool does not adopt an unknown compositor warp or later user movement.
Loss of cursor authority still permits exact-unit cleanup and state checks.
An unverified absolute input backend therefore produces a blocked receipt,
not a fabricated input success.

The applet unit has `RuntimeMaxSec=300`, `TimeoutStopSec=10`, a 192 MiB memory
limit and 64-task limit. This provides independent applet cleanup if the host
or SSH transport disappears. Normal completion/Ctrl-C/SIGTERM also checks the
original invocation/PID/start/executable/token/cgroup before stopping the unit,
then checks its cgroup recursively for processes, threads and populated
descendants. A reused unit, PID or cgroup is never stopped under old authority.
If startup failed before the exec receipt, the normal cleanup refuses an
unidentified process and leaves the finite unit lifetime to expire.

SIGKILL or host loss while a physical button is down cannot be balanced by a
guest application service. The operator must release it through the VM console
before resuming interaction. Lost QMP release acknowledgements and incomplete
cleanup are explicitly blocked even when exact applet cleanup succeeds. The
tool never reboots the VM or kills a foreign application to recover.

## Validation so far

The focused Python suite exercises exact menu semantics/bounds and lifetime
changes, typed QMP events, binding admission, sticky cursor ownership,
nonsecret NetworkManager observations, wired-only rejection, actual private
file/symlink preservation, private receipt modes, actual owned temporary
cgroup-shaped tree quiescence, foreign PID cleanup refusal, fresh pre-press
admission, stale-menu cancellation and release-failure cleanup ordering.

```sh
python3 -m unittest discover -s tests -p test_third_party_tray.py -v
python3 -m py_compile vm/third_party_tray_policy.py \
  vm/third_party_tray_guest.py vm/test-third-party-tray.py
```

Those tests do not launch an applet or connect to a guest, compositor or D-Bus.
They are not native acceptance. A future source-specific VM receipt must
report whether the package/indicator/input preconditions were actually met,
and must retain blocked/unsupported evidence rather than changing this scope.
