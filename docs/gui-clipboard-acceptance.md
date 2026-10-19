# Real application clipboard acceptance

`vm/test-gui-clipboard.py` extends the existing native Notes tests with a real
Chromium Wayland window. The first native `d28fbe6b` run failed before transfers
at `original_browser_wrapper_receipt_available`; only clipboard checkpoint/history
pause passed. Its original failed/restoration flags remain unchanged. Corrected
tooling still requires a fresh native run. Transport tests establish bounded
protocol handling, not clipboard success.

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

The frozen `41826937` native run passed browser-to-Notes transfer, exact
multiline/combining/emoji/CJK preservation and replacement of the Notes
selection. It then reached the Notes copy offer but refused reciprocal
browser input because the original fixed 170-second input window expired.
All 11 automatic restoration checks passed. The longer finite budgets below
address measured guard/SSH overhead; reciprocal browser paste remains
unaccepted until a fresh native run completes. That failed receipt is retained.

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
Before requesting browser workspace/window focus, the driver retires its old
keyboard target and uses its existing guarded cursor dispatcher to reach the
center of the freshly admitted persistent browser window. This anticipates the
compositor's normal focus-centering warp. The PID/address/class/workspace and
finite, fully visible window geometry are pinned and rechecked through that
focus sequence. A moved/replaced window or independently moved cursor refuses
the operation. This declares and verifies a destination before focus; it never
adopts a point after an unexpected warp, and generic cursor ownership is unchanged.
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
The original service's 360-second limit alone is not proof that this scope and
its descendants have drained. The wrapper starts a separate owned watchdog
before exec. Its original unit receipt is durable before launch continues; it
captures the same browser lease used by the host, requests graceful PIDfd
retirement after 330 seconds, and verifies the original service and scope drain.
The watchdog has its own 390-second systemd limit and 385-second process alarm,
preserving the existing 55-second retirement allowance. The host input window is
a fixed 300 seconds from before browser startup and is never renewed by successful
guards. Keep an outer host limit of at least 600 seconds with a 90-second stop
allowance so normal interruption can run the guarded restoration path, including
the 35-second desktop stability check. None of these limits grants permission to
retry input or skip ownership/focus checks.
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
receipt can nominate that original lifetime for guarded retirement. Returning the
receipt reads immutable pre-exec evidence; it neither requires page readiness nor
grants input or signal authority. A separate durable process lease pins the
installed root-owned Chromium executable and current running identity without
requiring CDP, the endpoint file or the document. Before its first durable lease,
this Arch fixture waits until kernel membership identifies Chromium's existing,
registered application scope. An early observation in the original wrapper's
service is not frozen; an already frozen lease never follows a later migration.
The test-only
[Chromium lease](native-chromium-lease.md) retains
the original PID birth and service invocation across the normal scope migration.
It freezes the existing registered scope invocation/cgroup, opaque process-title
digest. Full browser admission separately requires the exact owned page and pins
the DevTools port-file inode/digest and original control target. Only this full
admission can authorize DOM preparation or input.
Subsequent checks compare these stable identities instead of rehashing Chromium
on every key.
A refused or lost start reply cannot authorize stopping an existing lookalike.
All explicit browser retirement paths carry the original wrapper receipt and
captured lease, recheck it after opening a PIDfd, and signal only that original
main process. They refuse same-name successor invocations or later migrations.
They never stop a guessed application scope or use name-only fallback cleanup.
Only the separately captured watchdog is retired through the generic unit API.
The wrapper fsyncs both its receipt file and parent directory before exec.

Fallback retirement and the independent watchdog use that same durable process
lease, even if page admission or a handshake fails. They do not call page
admission to acquire cleanup authority. A drained original process can be
recognized only by the captured service/scope and recursive-cgroup proof;
retirement never adopts a successor or signals a guessed scope. Failed runs keep
the private process lease alongside origin and page-admission evidence.

Guest failures emit bounded JSON with a fixed operation/class/allowlisted reason
and at most 12 source-basename/function/line locations. The staged `fixture.py`
name is normalized to its source name. A rejected WebSocket upgrade may add only
the bounded HTTP version and three-digit status code; no reason phrase, headers,
URL, exception text, document or clipboard content is emitted. Upgrade admission
requires a well-formed HTTP/1.1 status line with status 101 and a legal optional
reason phrase, rather than one literal English phrase. The existing accept-hash,
unique-header, Upgrade/Connection, extension refusal and size/deadline checks
remain required. Status metadata alone grants no admission. The host retains
the failed operation separately from any
later restoration failure; private command logs contain the guest diagnostics.
The host additionally records its fixed suite/restoration phase, error class,
and at most 12 basename/function/line frames from the exact loaded host, Notes
and input-policy sources. A reason is exposed only when it is a bounded label
literal on that trusted call path; arbitrary exception values and private paths
remain excluded. Each outer restoration failure has its own record. A successful
peer retry clears its earlier failure, and a peer error is attributed to the
suite only when it is the same propagated exception. An earlier scope-readiness
retry cannot become the diagnosis of a later unrelated host error.

Notes restoration starts only after original-browser retirement and a fresh
unlocked, idle, no-foreign-client/cursor guard. If either fails, the driver leaves
the desktop service running and retains the private Notes/clipboard checkpoint
for guarded recovery. It does not stop the desktop under new authorization UI.
Fixture reads pin device, inode, mode, owner, group, size, modification time and
change time, plus bounded bytes and pathname identity. Read-access time alone is
excluded because a cold read may legitimately update it under relatime.

The first native attempt coupled receipt retrieval to full page admission, so
an unrelated page/endpoint failure could hide an already published origin and
block normal host cleanup. Its 50-byte generic stderr did not identify that
underlying failure; no alternate HTTP status or transport defect is inferred.
After that failed run, a separate guarded recovery verified original browser and
watchdog/service/scope drainage, unchanged settings/wallpaper/media/exports, and
restored history/service/workspace/cursor/idle state. Its first refused recovery
and the original failed test remain historical. The new separation and
diagnostics have source/pure-test evidence, not a passed native transfer.

The later `2fc161b` native attempt also failed before transfers. Read-only
diagnostics showed its durable lease had been captured before Chromium's normal
scope migration; the original process birth/executable and opaque command stayed
the same, but the frozen cgroup/scope correctly refused that change. Independent
recovery then observed `HTTP/1.1 101 WebSocket Protocol Handshake` with the exact
expected accept hash and WebSocket Upgrade header. The old literal reason-phrase
check rejected this valid status. The corrected parser follows
[RFC 6455 section 4.1](https://www.rfc-editor.org/rfc/rfc6455.html#section-4.1)
and [RFC 9112 section 4](https://www.rfc-editor.org/rfc/rfc9112.html#section-4):
malformed control bytes, separators, protocol versions and non-101 responses
remain refused. A separate guarded recovery restored the original desktop,
settings/media/clipboard/workspace/cursor/idle state. The failed attempt remains
failed; scope readiness and status parsing corrections have local boundary-test
evidence pending another native transfer run.

The subsequent frozen `b6323e9` attempt passed exact real-browser admission and
owned-browser retirement, then failed with a generic host `ValueError` before
clipboard transfers. Notes/cursor restoration did not pass. The original report
retains those results; the added host phase/source diagnostics do not establish
the underlying cause or a successful transfer. Independent recovery is separate
from this failed test's automatic-restoration flags.

The later frozen `cfec77f` diagnostics identify the actual failure in
`browser_copy → focus_browser → input_guard → CursorOwnership.observe`:
the compositor's focus dispatch centered the cursor in the admitted tiled
browser below the bar, outside the previously declared point. The sticky cursor
guard correctly refused it. No copy had occurred, and owned-browser retirement
passed; automatic Notes/cursor restoration remained failed. Separate guarded
recovery subsequently passed all eleven restoration checks, including normal
desktop readiness for 35 seconds. The prepositioning correction has focused
local tests; a fresh full native transfer result is still required.

This test does not establish rich HTML/image clipboard behavior, other
applications, delayed-provider/focus-retirement races, physical hardware, IME
input or rendered complex-script correctness. Those remain separate acceptance.

Transport boundary checks without a VM:

```sh
python3 -m unittest discover -s tests -p test_cdp_peer.py -v
python3 -m unittest discover -s tests -p 'test_gui_clipboard*.py' -v
python3 -m unittest discover -s tests -p test_owned_chromium_lease.py -v
```
