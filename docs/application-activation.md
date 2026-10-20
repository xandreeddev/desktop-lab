# Standard application activation

Source scope: accepted main `d01ff7db` includes PR89's independent d1 consumer
and the earlier PR87 runtime. Application integration `bb304907`, fresh intent
tokens, explicit file/URI Open and expanded XDG parsing shipped in the coherent
`8e6d074b` release. Its later real-Chromium local-file run passes nine feature
and eight restoration checks. The VM now runs a coherent, unmerged `9b5420b7`
performance trial; its separate HTTP URI run passes eleven native checks and
nine restoration checks. Both observe first-launch focus without forcing it.
Neither result establishes another application, default URI handler, external
navigation or Chromium's consumption of a particular token. The `9b` application
result does not accept its performance. Earlier failed and successful reports
retain their exact sources and results; see [current readiness](current-readiness.md),
[local-file evidence](../reports/measurements/lucent-native-application-intents-d1.json)
and [HTTP URI evidence](../reports/measurements/lucent-native-http-uris-9b.json).

An application can now declare a command or standard desktop-service activation.
`ApplicationLaunch::Command` carries its parsed argument vector.
`ApplicationLaunch::DesktopService` carries an optional executable fallback.
A service-only `.desktop` entry is no longer discarded because it lacks `Exec`.
There is no invented executable sentinel.

`ApplicationPort` remains the domain contract. `XdgApplications` is its supplied
adapter; the launcher and dock consume that port. Existing pure window-class
matching and prefer-running focus behavior remain unchanged.

## Native behavior

The adapter reads `DBusActivatable=true` and derives the well-known D-Bus name
from `AppId`, removing `.desktop`. It validates the native name and constructs the
object path by replacing dots with slashes and dashes with underscores. It invokes
`org.freedesktop.Application.Activate` with ordinary typed D-Bus arguments, never
an application-generated command string. See the
[freedesktop activation specification](https://specifications.freedesktop.org/desktop-entry/latest/dbus.html).

Native connection, method, close and permitted command fallback share one
five-second dispatch budget. Close gets at most 250 ms within that budget.
The connection's local executor is driven while the request waits; it creates no
permanent polling worker.
Native errors are classified without displaying their raw bodies.

A definitive missing service/object/method permits the parsed `Exec` fallback.
A denied call or uncertain reply timeout returns failure and starts no second
instance. Command fallbacks still use independent user application units and
literal argument arrays. Restarting Lucent does not put those applications inside
its shell service lifetime.

## Fresh user intent

The SDK's opt-in `Effects::activation_token` requests an opaque token for the current
physical pointer/key dispatch. The Wayland adapter binds `xdg_activation_v1`, captures
the actual seat, serial and initiating surface, then commits one asynchronous request.
No retained input serial or inherited `XDG_ACTIVATION_TOKEN` supplies authority.
There are at most eight pending requests and a two-second reply deadline. Secure
clients keep activation disabled. Unsupported protocols and IPC commands without a
physical input report an explicit outcome; ordinary launching can still proceed.

The desktop captures the selected `Application` and maps the SDK completion into an
`ApplicationLaunchContext`. It rechecks catalog authority before scheduling the port.
The domain context contains an optional checked, opaque token, with redacted Debug;
it has no Wayland handles. Production `XdgApplications` implements
`launch_with_context`, `launch_action_with_context` and `open`.

D-Bus `Activate`, `ActivateAction` and `Open` receive the fresh value under the
`activation-token` platform-data key. A command launch passes it only in the
`systemd-run` child's environment and imports it by name into the independent user
application unit. The token never appears in argv, and the global process
environment is never mutated. Without a fresh value, old activation/startup tokens
remain excluded. The compositor decides whether to focus the application; receiving
a token is not proof that focus or presentation occurred.

The native protocol is documented in the upstream
[xdg-activation XML](https://gitlab.freedesktop.org/wayland/wayland-protocols/-/blob/main/staging/xdg-activation/xdg-activation-v1.xml).

## Explicit files and URIs

`ApplicationOpenTarget` distinguishes checked absolute local paths from explicit
application URIs. Explicit application targets support valid URI schemes such as
`smb` or `steam`; this does not widen the separate notification-link policy.
`ApplicationOpenRequest` admits 1–64 targets with a 128 KiB
encoded-content bound. It performs no filesystem or transport work. The selected
application stays explicit: failure never silently opens a different default handler.

For a desktop service, `Open` receives the URI array and optional activation data.
Definitive unsupported service/object/method errors permit only that application's
current command fallback. Denial, uncertainty and timeout never repeat the request
through another process. `XdgApplications` freshly resolves the desktop source under
normal priority/masking rules and rejects changed captured descriptors.

For a command fallback, the adapter re-expands its current `Exec` template. `%F` and
`%U` preserve a list as separate arguments; `%f` and `%u` create separate invocations
for multiple targets. One shared five-second dispatch budget bounds the request;
an error can follow earlier successful invocations, so it is never automatically
replayed. Only the first invocation receives the intent token. Name, icon and
desktop-file fields expand exactly once. File
fields accept local paths/file URIs; they explicitly refuse remote resources rather
than claiming to download them. URI fields retain URI encoding. Quoted field codes,
multiple file/URI fields and multi-argument fields embedded inside another argument
are rejected. No shell interprets target contents.

Inside the graphical session:

```sh
lucent-cli applications open org.example.Editor.desktop '/absolute/a b.txt'
lucent-cli applications open org.example.Browser.desktop 'https://example.org/'
```

The CLI encodes each argument as a separate JSON value on the existing private
command socket. The command protocol retains its 4000-byte limit. Its reply admits
work; an actual native failure arrives through desktop error state. The independent
`application-service --open APP_ID FILE_OR_URI ...` consumer demonstrates the same
port without a renderer or Wayland dependency.

The independent graphical `application-intent` example demonstrates the other
boundary: a physical button dispatch requests the SDK token, maps its completion
into a domain launch context, then invokes an injected `ApplicationPort` on a
worker. Its composition root supplies `XdgApplications`; its view performs no OS
calls. It can launch, open the supplied targets or invoke one declared action:

```sh
cargo run --locked -p application-intent -- \
  --app org.example.Editor.desktop --target '/absolute/a b.txt' \
  --action NewWindow
```

Omit `--action` when the application does not declare one. `--data-dir` selects
an isolated XDG catalog for acceptance. The example's inspector exposes only
busy/completion counters and a boolean indicating fresh authority. Its socket
`open` command exercises the same port without physical input and must not reuse
the preceding button's serial or token. An acknowledged operation still needs
verification in the target application; the example does not equate dispatch
with observed focus or file loading.

## Verification scope

Declared desktop actions predate this milestone and already have owned native
acceptance; see [application-actions.md](application-actions.md). The token,
Open, parser and observation changes shipped in the accepted-baseline `8e` release,
whose full workspace/protocol CI and 832 visual cases pass. A later real-Chromium
run with the `d1fff2d8` independent consumer, now merged through PR89, passes nine feature and
eight restoration checks, including observed first-launch focus. This verifies
one local-document workload, not broad third-party compatibility. Current evidence and remaining checks
are tracked in `docs/daily-use-completion.json`.

`vm/test-application-intent.py` prepares an owned desktop entry and private real
Chromium profile, then exercises physical launch/open/action and socket/CLI
opening. The fixture records token **presence**, observes owned window/focus
state, and checks actual file URI/title loading through that profile's loopback
DevTools endpoint. Each physical Open, declared Action, later socket Open and
desktop CLI Open must create **two new CDP target IDs**, each with its exact
literal file URI and title; old matching tabs and partial new results cannot
satisfy a later request. The fixture action explicitly names both owned files.
Only those two fresh targets are closed after that request's assertion, keeping
the original blank control window, admitted browser process and private endpoint
identity. This bounds the browser workload to the control plus two documents
without silently reducing the requested application behavior. Child-environment
delivery and observed focus do not prove
that Chromium consumed a specific activation protocol message. The run requires
an identified release containing the independent example, an unlocked idle
single-output VM and bounded owned-process cleanup. Earlier failed attempts below
retain their results; the later source-specific successful run is recorded in
its own section.

The admission snapshot binds the desktop, supervisor and compositor to their
captured lifetimes. The independent example must match the release digest and
its exact transient unit's structured command, invocation, main PID, process
birth and cgroup. Its native layer address and private socket inode remain
frozen; every other layer must belong to the captured desktop. Example IPC
checks Linux `SO_PEERCRED` against that process before sending any command.

Each pointer action rechecks actual unmodified left-button/catch-all bindings,
closed panels, absence of credentials/foreign windows, the current hit geometry
and layer lifetime. Cursor ownership is exact and sticky: observed independent movement
blocks further test moves and restoration, even if the cursor returns. A stale
owned press retires only the identified example before attempting balanced
release; it sends no cancellation key to another surface. The existing watchdog
and finite example unit lifetime provide bounded process cleanup if transport is
lost. Host SIGKILL while a physical button is down still requires console/QMP
release by the operator; the guest watchdog cannot inject host input.

Before `exec`, each owned example, browser wrapper and watchdog durably records
its original transient unit invocation, main PID, process birth and cgroup in the
private run directory. The example additionally persists its admitted release
digest and socket identity. Host fallback and watchdog retirement reuse those
immutable receipts; refusing a replaced unit or socket never permits a retry
with only its current name or command. A lost start reply can retire the original
wrapper only when that wrapper already wrote its receipt. A loaded unit without
that proof is retained for bounded expiry/private recovery rather than adopted.
Completed-unit observations also keep the original invocation check. Those
source-only regression checks are distinct from the later native result below.

Browser page retirement rechecks the original unit invocation/process birth,
admitted executable metadata, exact `DevToolsActivePort` file identity and the
fresh target's URL/title before each target-specific close. It refuses redirects,
extra or replaced pages, endpoint replacement and profile-wide closing. A failed
page close retains the original ownership receipts for guarded unit cleanup.

Lightweight checks in `tests/test_application_intent_admission.py` exercise
foreign layer and socket peers, replaced sockets, stale layer/hit identity,
catch-all interception, cursor loss and captured-process retirement. They use
pure policy, mocked transport boundaries and an actual read-only fingerprint of
the test runner's `/proc` entry; they do not launch a guest, browser or native
surface. Preservation and owned cleanup are checked separately in the later
source-specific native result; pure checks alone do not establish acceptance.

`tests/test_owned_application_units.py` additionally covers original receipt
publication before `exec`, lost-reply cleanup, fallback/watchdog refusal,
same-name unit reuse after stopping, and cold manifest reads. File read identity
excludes access time alone: device, inode, mode, owner, group, size, modification
time and change time remain checked, including actual mid-read content mutation
and path replacement. Chromium may explicitly use the helper's bounded 512 MiB
executable limit; its normal default remains 128 MiB. Per-input observation uses
stable executable metadata and process birth after the admitted hash, avoiding
repeated full Chromium hashing.

`tests/test_application_intent_browser.py` covers fresh two-document success,
reuse of old matching tabs, only one fresh document, duplicate/unexpected pages,
control replacement, and target-specific cleanup refusals. These checks use
pure policy, temporary endpoint files and mocked browser transport; they do not
establish native Chromium interoperability.

The `1c3d500` native attempt did start a real owned Chromium window during the
first physical Launch. Its private child event records fresh token presence and
no legacy startup value. The original report nevertheless contains no passed
checks and fails cleanup; it remains unchanged. Independent process diagnostics
show the same original wrapper PID/birth and service invocation while Chromium
moved itself into its normal registered application scope. Its executable is
319,274,072 bytes, below the explicit 512 MiB limit. The fixture's strict original
cgroup check rejected that lifecycle; neither a size-limit failure nor an SDK
defect is established. No focus, document-loading or complete application pass
is inferred from that partial observation. Supporting normal application-scope
migration requires separately reviewed exact execution ownership; disabling the
browser's integration or accepting arbitrary new cgroups is not a substitute.

Failure reporting now retains the primary interaction phase and fixed error
independently from each cleanup failure. Failed fixture subprocesses retain their
operation, exit/deadline metadata and stderr byte count/hash. At most 64 KiB of
stderr per failure is stored in a mode-0600 file under a new mode-0700 ignored
host diagnostics directory. The private report records those files and the guest
scratch/origin/admission receipt locations, including the latest failed fixture
observation when a readiness loop subsequently times out. It never includes raw
exception/traceback text in diagnostic metadata or stdout. These bounded evidence
files are not authority to bypass a failed guard or adopt another process.

An earlier interaction failure survives cleanup failure instead of being replaced
by the generic preservation label. Cleanup failures remain separately named;
they still fail acceptance when the interaction itself passed. Reports are born
mode 0600 and refuse to overwrite an existing receipt. Stdout gives only the
fixed outcome/check summary and report location, leaving private artifact paths
inside the report. The reporting correction has local tests; it does not rerun
or relabel the original failed native attempt.

The later `d28fbe6b` attempt used the unchanged coherent `8e` desktop/example and
the corrected test-owned Chromium lease. It failed
`first_launch_native_focus` / `fresh_owned_browser_focus_observed`, with no guest
guard exception or failed fixture-stderr artifact. This is an actual unsuccessful
focus assertion, distinct from the earlier scope/duplicate-platform guard failures.
Its checks remain empty and its overall result remains failed. Automatic cleanup
retired the exact example, browser and watchdog, preserved settings/wallpaper and
output/workspace/cursor, and retained private diagnostic artifacts as intended.
It did not establish file loading, protocol-token consumption or complete App
interoperability. The original private report remains unchanged.

The host now keeps one bounded last focus observation before cleanup: whether an
active window exists, whether its class/PID/address match the admitted owned
clients, and whether the original combined focus predicate succeeds. It also
copies the submitted example's keyboard policy, logical/buffer sizes and layer
geometry, including comparison with its admitted geometry. It records no raw
window title, class, process identity, native address or field payload. Separate
fixed phases distinguish a false focus predicate from a guard exception or failed
focus observation. These diagnostics neither change the acceptance criterion nor
issue a focus command. Their source/pure checks do not rerun the failed attempt.

The prepared harness now records first-child token delivery independently from
first-launch focus. Its focus observation window remains bounded to 35 seconds.
A clean false predicate may proceed to the existing independent file/Open,
declared Action, socket and CLI assertions only after the same complete normal
interaction guard passes again. Any guard, identity, focus-read or transport
exception aborts immediately; it is never converted into a clean false timeout.
Those independent assertions remain unchanged. Their results can be collected
while focus is unresolved, but the combined token-and-focus check stays false.
The original first-launch focus requirement is enforced again at the end before
overall success. A later Open/action acquiring focus does not retrospectively
satisfy that first-launch checkpoint. No new native result follows from this
source/pure-test change.

Process retirement and published-artifact retirement are now separate required
cleanup observations. An admitted example's original PID birth and recursive
cgroup must drain even if systemd has already collected its unit. Only its exact
captured socket device/inode/mode/owner can be retired: a bounded connection must
be refused, the identity must still match afterward, and a live/replaced or
uncaptured socket is left intact. An already absent admitted socket is valid;
startup never adopts or clears an unknown stale socket. The host requires a
separate `example_socket_retired` flag for every admitted example.

The fixture captures its newly published desktop entry's identity from the
creating file descriptor, alongside its exact path and content hash in a private
receipt. After all owned example/browser lifetimes drain, retirement removes
only that unchanged entry, including on an unsuccessful test. The host requires
the separate `desktop_entry_retired` flag. A replaced entry, even with identical
contents, is refused. Repeating retirement accepts absence, and successful final
cleanup remains compatible with the already removed entry. Failed runs preserve
their private profile, documents and origin/admission receipts.

The original `d28fbe6b` report's successful process-retirement flags did not prove
socket or entry retirement. The next `3d961e32` attempt refused admission at
`independent_example_not_already_running` before mutations because the earlier
captured refused socket remained. Both reports retain their original results;
the new mandatory artifact flags are not retroactively added to them. The
artifact correction has local boundary tests, not a new native pass.

The frozen `8e6d074b` independent example keeps its Overlay/OnDemand surface
visible; the production launcher closes after requesting a launch. The later
`d1fff2d8` example source yields keyboard interaction in that same Request update
and retains the yielded state through token preparation and adapter completion.
Only a subsequent deliberate idle pointer press re-arms its keyboard policy;
hover, socket commands and completion cannot take it back. This uses the existing
SurfaceSpec contract, with no compositor focus command or framework policy.
Its separate bounded build and later native verification pass as recorded below.
That later result does not isolate the cause of earlier focus failures. The
compositor remains responsible for deciding whether to grant application focus.


## Opt-in native HTTP URI acceptance: Chromium passes

The `7a92749e` harness passes eleven native checks and all nine restoration
checks with `--http-uris` against the coherent `9b5420b7` candidate. Native pointer,
later socket and desktop CLI Open each load both fresh owned HTTP documents in
real Chromium. First-launch focus is observed without forcing compositor focus.
See [the source-specific result](../reports/measurements/lucent-native-http-uris-9b.json).
The default invocation retains the existing local-file workload. The `9b` build
is an unmerged performance trial; this application result does not accept its
memory use, CPU cost or presentation rate.

```sh
python3 vm/test-application-intent.py \
  --release-manifest /private/source-matched-cohort.json \
  --http-uris --report /private/absent-uri-report.json
```

The identified release must include the independent application-intent example.
That example already accepts typed URI targets, so this harness extension adds
no framework API, domain contract or production behavior. Its owned `%U`
desktop entry stays immutable during the run, and no user MIME associations are
written. The declared Action still opens the two local-file fixtures; physical
Open, later socket Open and desktop CLI Open instead use two encoded HTTP URIs.

During its existing 210-second wait, the original watchdog process serves only
two generated, hashed HTML bodies on an ephemeral IPv4 loopback listener. There
is no additional process or thread, directory listing, redirect or external
asset. Each connection has a one-second absolute header deadline, an 8192-byte
request bound and a half-second write timeout. Responses close the connection
and prohibit external resources with a content security policy. Readiness binds
the original watchdog invocation/process and exact listener descriptor/inode to
its current loopback TCP LISTEN row; later guards refuse changed authority.

Each Open still requires two fresh CDP target IDs with exact expected URL and
title, followed by retirement of only those targets and preservation of the
original blank control. The original focus observation is retained without
forcing compositor focus. Cleanup additionally requires the original watchdog
cgroup to drain, including when HTTP readiness fails before the example starts.

All 510 Python checks pass on the frozen harness source, including the 19 new
pure fixture/admission cases; those pure cases create no listener or browser.
The direct Rust parser regression also actually passes in exact-source
[CI run 39343459962](https://github.com/xandreeddev/desktop-lab/actions/runs/39343459962).
It covers successful HTTP `%u` and `%U` expansion with encoded percent,
Unicode, query and fragment values.
Native `%u`, default URI handlers, external navigation and broad browser
compatibility remain outside this `%U` fixture's scope.

The owner observed 95.035 seconds for the native run, not a performance
benchmark. The original listener/watchdog, example/socket, browser and desktop
entry retire. Settings, wallpaper, output/workspace and cursor remain preserved;
only owned scratch is removed. Older failed and successful reports retain their
original source and results. The older filename-labelled aggregate check also
includes the declared local-file Action in HTTP mode; the explicit HTTP check
identifies which three Open paths received URI coverage.

The following result sections preserve the delivery state at each native run.
Their capture-time unmerged flags remain historical; the current accepted main
above now includes the d1 consumer through PR89. The recorded production binaries
remain the separate coherent 8e release.

## Fresh real-application result: file opening passes, focus remains failed

The frozen `f6d004d9` harness ran against the unchanged deployed `8e6d074b`
desktop and independent intent example. Seven independent behavior checks pass:
fresh physical-input tokens reach the owned child; physical Open and declared
Action, later socket Open and desktop CLI Open each create both new local
documents. The documents include spaces, percent signs, Unicode and literal
shell-like filename characters. Exact new page IDs, file URLs and loaded titles
are checked for every request. Only those fresh targets retire; the original
blank control page survives. Socket/CLI requests do not reuse earlier physical
activation authority.

All eight restoration flags pass, including original example socket and published
entry removal, exact owned unit retirement, unchanged settings/wallpaper and
original output/workspace/cursor. Private failure evidence is retained.

The first-launch focus check remains **failed**: eleven coherent observations
find one owned mapped browser and no active managed window. The independent
example continues to advertise its on-demand keyboard layer. Production closes
its launcher on dispatch; that lifecycle difference is under investigation and
is not an established cause. No test forces compositor focus or changes the
acceptance predicate. Overall native acceptance remains failed.

The [sanitized result](../reports/measurements/lucent-native-application-intents.json)
identifies source/artifact hashes and exact check/restoration flags. The owner
observed 111.318 seconds for the bounded run; this is not a performance benchmark.
Token delivery does not prove Chromium consumed the Wayland activation token.
Other applications, remote URI handlers and physical hardware remain outside
this local-document workload. Earlier failed reports remain unchanged.

## Later d1 consumer: Chromium launch and file intents pass

The unchanged deployed `8e6d074b` desktop and CLI pass a later scoped native run
with harness `b6323e95` and the separately rebuilt `d1fff2d8` independent SDK
consumer. The consumer is **not merged into accepted main**. Its binary is
identified by the separate build and manifest hashes in
[the sanitized report](../reports/measurements/lucent-native-application-intents-d1.json);
the original coherent production archive remains unchanged.

All nine feature checks pass. The first coherent focus observation finds the
real owned Chromium window active, matching its admitted class, process and
native window identity. The example has submitted keyboard `None`, with its
448 × 384 surface and buffer geometry unchanged. No test command forces
compositor focus. Fresh physical-input tokens reach the owned child; observed
focus and child delivery do not prove Chromium consumed a particular protocol
token, nor does this one later result establish the cause of earlier failures.

Physical Open, declared Action, later socket Open and desktop CLI Open each
create both new local documents with exact fresh page IDs, file URLs and loaded
titles. Spaces, percent signs, Unicode and literal shell-like filename characters
are covered. Only those new targets retire, preserving the original blank
control target and admitted browser lifetime. Socket and CLI opening do not
reuse earlier physical activation authority.

All eight restoration checks pass: original example socket, example, browser,
published desktop entry and watchdog retire; settings/wallpaper and original
output/workspace/cursor remain preserved; owned scratch and entry are removed.
The owner observed 75.660 seconds for this bounded run, not a performance
benchmark. The earlier failed and partial reports above remain unchanged.
Other applications, arbitrary remote URI handlers, physical hardware and broader
launcher/catalog/dock workflows still need their own acceptance.
