# Real application clipboard acceptance

`vm/test-gui-clipboard.py` extends the existing native Notes tests with a real
Chromium Wayland window. **Native execution of this new test is pending.** Its
transport unit tests establish bounded protocol handling, not clipboard success.

The two transfers use actual VM `Ctrl+C`/`Ctrl+V` keyboard delivery:

1. Select an owned Chromium textarea containing multiline UTF-8 text, a combining
   mark, CJK and an emoji sequence; copy it and paste over the native Notes
   selection. Check the accepted and durably saved Notes value.
2. Edit Notes with ordinary native keys, select/copy its multiline value, then
   paste over a selected sentinel in the browser textarea. Independently read
   the resulting browser DOM and compare the expected UTF-8 hash.

The browser's DevTools protocol is used only to prepare/select the owned
textarea and read its DOM. It does not call a clipboard API, dispatch synthetic
keyboard events or manufacture clipboard contents. `wl-paste` observes transfer
offers; it is not the source of either test transfer. The existing restoration
helper may use `wl-copy` afterward to restore the original clipboard payload.

## Execution

Use the exclusive VM test lane after deploying and verifying the identified
release. Do not run this alongside builds, benchmarks, another input driver or
manual interaction. It does not boot, unlock or deploy the VM.

```sh
python3 vm/test-gui-clipboard.py \
  --source-commit FULL_RELEASE_SOURCE_COMMIT \
  --desktop-sha256 ACTUAL_DEPLOYED_DESKTOP_SHA256
```

The current test supports the existing `desktop-lab-lucent` VM on
`qemu:///session`, user `omarchy`, SSH port 2243, one unrotated 1920×1080 output at
scale 1, an unlocked idle Lucent desktop, no ordinary windows, no authorization
UI, and an English US keyboard with no conflicting tested chords. Feature
worktrees resolve the canonical Git checkout's private VM inventory.

Chromium must already be installed. The test never installs a browser or opens
an existing profile. Its new private profile, local HTML file, generated class,
unique named workspaces and transient unit belong only to that invocation.
Native layer/hit geometry supplies the Notes pointer destination. Cursor
positioning uses the compositor's validated global logical-pixel
cursor dispatcher and require exact observed arrival. Positioning is not a
physical-input result: QMP button presses and native keyboard delivery exercise
the actual focus and clipboard interactions.
The two workspaces let the actual browser window and desktop Notes receive focus
without changing tiling rules or introducing an always-on-top test surface.
Notes clicks bind the submitted field, full visible clip, surface instance/output
and exact native layer address/PID/geometry. They revalidate that lifetime after
pointer settling and as the final input guard before physical button-down;
moved, clipped, retired or recreated targets refuse the press. An attempted
button-down always receives its balancing release, including lost replies.

Before each input the driver checks lock/authorization state, native output
geometry, current workspace, closed shell panels, and every managed window.
Every native layer must also belong to the current verified desktop executable;
a matching `lucent-` namespace alone never grants input authority.
Only the exact browser class with the original typed systemd `ExecStart` argv
and process cgroup is admitted. The native window address, original main PID,
process birth, executable metadata and unit InvocationID remain frozen across
input. The browser must remain on its dedicated workspace; Notes input also
requires that no managed window has keyboard focus. A native active window alone is insufficient:
the owned page must also report `document.hasFocus()` and the correct active
textarea immediately before browser keyboard input. Tested text keys, chords,
left-button and catchall bindings are rechecked against the compositor. Copy/paste additionally requires the selection's
hash to be the original captured offer or one of this invocation's fixtures.
Independent cursor movement permanently retires this invocation's pointer and
input authority, even if the pointer later returns to its previous position.

The DevTools client accepts only that profile's loopback port and exact page
target for the owned file/title. It verifies the WebSocket handshake, rejects
extensions, masking errors and binary messages, bounds messages/fragments/events
and uses absolute response deadlines. It neither reads a browser-wide endpoint
nor accepts a remote host or proxy. Protocol references:
[Chrome DevTools Runtime](https://chromedevtools.github.io/devtools-protocol/tot/Runtime/)
and [RFC 6455](https://www.rfc-editor.org/rfc/rfc6455.html).

## Preservation and limits

The existing Notes harness retains the original settings bytes and inode via a
private hard-link checkpoint. It allows only the named Notes visibility/content
changes and restores them only after the normal Lucent unit is stopped and its
complete cgroup is quiescent. Unrelated changes block restoration.

The initial clipboard must be empty or bounded equivalent UTF-8 text aliases.
Nontext, sensitive, unequal or unrecorded offers remain untouched and block this
test. The active history helper is paused before owned transfers; its original
file, referenced images and state are verified on restoration. Payload and text
MIME compatibility are restored, rather than the identity of the original
clipboard-provider process. Original text and Notes never enter the public
report. Private evidence remains under `reports/local/` with mode 0700/0600.

Chromium can move its original main process into a registered application scope.
The original service's 210-second limit alone is not proof that this scope and
its descendants have drained. The wrapper starts a separate owned watchdog
before exec. Its original unit receipt is durable before launch continues; it
captures the same browser lease used by the host, requests graceful PIDfd
retirement after 180 seconds, and verifies the original service and scope drain.
The watchdog has its own 240-second systemd limit and 235-second process alarm.
An unverified lease or failed drain remains a failure with private evidence.
**A host crash does not automatically restore
Notes, clipboard history or its helper.** The private checkpoint and recovery
metadata remain available for guarded operator recovery; do not delete them or
blindly overwrite settings. Normal interruption/failure runs the restoration
path, checks desktop readiness for 35 seconds, and restores the original
workspace, cursor and idle marker when ownership still holds. A foreign workspace
change blocks the restoration focus dispatch rather than taking focus away from
that workspace.

Typed unit-name absence is required during initialization and immediately before
launch. The original wrapper writes a private InvocationID/PID-birth receipt
before it execs Chromium. Only an acknowledged start followed by that original
receipt can authorize retirement; full browser admission additionally waits for
the owned page, pins the installed root-owned Chromium executable and captures its
running identity. The test-only [Chromium lease](native-chromium-lease.md) retains
the original PID birth and service invocation across the normal scope migration.
It freezes the existing registered scope invocation/cgroup, opaque process-title
digest, DevTools port-file inode/digest and exact original control target.
Subsequent checks compare these stable identities instead of rehashing Chromium
on every key.
A refused or lost start reply cannot authorize stopping an existing lookalike.
All explicit browser retirement paths carry the original wrapper receipt and
captured lease, recheck it after opening a PIDfd, and signal only that original
main process. They refuse same-name successor invocations or later migrations.
They never stop a guessed application scope or use name-only fallback cleanup.
Only the separately captured watchdog is retired through the generic unit API.
The wrapper fsyncs both its receipt file and parent directory before exec.

Notes restoration starts only after original-browser retirement and a fresh
unlocked, idle, no-foreign-client/cursor guard. If either fails, the driver leaves
the desktop service running and retains the private Notes/clipboard checkpoint
for guarded recovery. It does not stop the desktop under new authorization UI.
Fixture reads pin device, inode, mode, owner, group, size, modification time and
change time, plus bounded bytes and pathname identity. Read-access time alone is
excluded because a cold read may legitimately update it under relatime.

This test does not establish rich HTML/image clipboard behavior, other
applications, delayed-provider/focus-retirement races, physical hardware, IME
input or rendered complex-script correctness. Those remain separate acceptance.

Transport boundary checks without a VM:

```sh
python3 -m unittest discover -s tests -p test_cdp_peer.py -v
python3 -m unittest discover -s tests -p 'test_gui_clipboard*.py' -v
python3 -m unittest discover -s tests -p test_owned_chromium_lease.py -v
```
