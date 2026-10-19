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
Native layer/hit geometry supplies the Notes pointer destination. The two
workspaces let the actual browser window and desktop Notes receive focus
without changing tiling rules or introducing an always-on-top test surface.

Before each input the driver checks lock/authorization state, native output
geometry, current workspace, closed shell panels, and every managed window.
Only the exact browser class with the original typed systemd `ExecStart` argv
and process cgroup is admitted. A native active window alone is insufficient:
the owned page must also report `document.hasFocus()` and the correct active
textarea immediately before browser keyboard input. Tested text keys, chords,
left-button and catchall bindings are rechecked against the compositor. Copy/paste additionally requires the selection's
hash to be the original captured offer or one of this invocation's fixtures.

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

The browser unit's independent 210-second limit retires its entire process cgroup
if the host driver disappears. **A host crash does not automatically restore
Notes, clipboard history or its helper.** The private checkpoint and recovery
metadata remain available for guarded operator recovery; do not delete them or
blindly overwrite settings. Normal interruption/failure runs the restoration
path, checks desktop readiness for 35 seconds, and restores the original
workspace, cursor and idle marker when ownership still holds. A foreign workspace
change blocks the restoration focus dispatch rather than taking focus away from
that workspace.

This test does not establish rich HTML/image clipboard behavior, other
applications, delayed-provider/focus-retirement races, physical hardware, IME
input or rendered complex-script correctness. Those remain separate acceptance.

Transport boundary checks without a VM:

```sh
python3 -m unittest discover -s tests -p test_cdp_peer.py -v
```
