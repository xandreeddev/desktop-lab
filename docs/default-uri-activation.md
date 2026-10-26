# Default URI activation

A notification hyperlink is an explicit opening intent. The notification client
owns the selected content revision and the pending operation; the framework owns
fresh native input authority; `UriPort` declares the opening contract; `XdgUri`
resolves the default handler through the operating system.

The path is:

1. `notifications::Center` accepts a link only from the current notification
   content and validated `ExternalUri` policy. History links can open content
   without regaining authority to invoke a retired notification sender.
2. The client retains one generation-bound opening and asks
   `Effects::activation_token(None, callback)`. `None` means the destination
   application's identity is unknown until the OS resolves the handler. Known
   application launch/Open/action clients continue to pass `Some(app_id)`.
3. The Wayland runtime requires the same fresh physical input dispatch as a known
   application launch. It sends that event's serial, seat and requesting surface.
   For an unknown destination it omits optional `set_app_id`; it never invents an
   identity such as `xdg-open`. Secure-client opt-out, bounded pending requests,
   timeouts and one-use input ownership remain unchanged.
4. The client yields its own notification layer's keyboard and pointer input
   while retaining the visible history. It requests
   `Effects::after_input_release` for that exact surface and retains the checked
   `ApplicationLaunchContext` until both the activation outcome and release
   acknowledgement arrive. Their order does not matter. A failed release keeps
   current history and restores its input with ordinary error feedback.
5. Only after both completions does the client revalidate the operation,
   notification revision, URI and initiating presentation, then schedule
   `UriPort::open(target, context)` on the existing controls queue. Hide/reopen,
   another foreground owner, replacement, clear or a newer opening cancels queued
   handoff work. An already-running OS operation cannot be undone. Success
   closes only the same initiating history session; a stale result cannot close
   newer UI. Toast-origin work never owns a later history presentation.
6. `XdgUri` sends one literal URI argument to `xdg-open` through the shared
   independent application-unit launch policy. The fresh token is passed through
   `XDG_ACTIVATION_TOKEN` by environment, never embedded in argv or inspector
   output. `DESKTOP_STARTUP_ID` is cleared. Without an explicit token, both
   inherited activation variables are cleared, including values inherited from
   the user service manager.

The domain contains the validated URI and launch context, with no Wayland,
renderer, D-Bus or process types. The generic framework knows neither notification
versions nor default-handler policy. Views contain no OS operations.

### Who owns each step?

| Owner | Responsibility | Implementation |
| --- | --- | --- |
| Desktop client | Select and revalidate the notification/link; own the history session and cancellation. | `notifications::Center` in `apps/lucent-desktop/src/notifications.rs` |
| Framework API | Carry typed activation and input-release requests and completions. | `Effects::activation_token` and `Effects::after_input_release` in `crates/lucent-api/src/` |
| Wayland backend | Capture the current native input event, submit the client's input policy with the buffer, and correlate its callback with the exact surface lifetime. | `crates/lucent-wayland/src/activation.rs`, `input_release.rs` and the normal rendering transaction in `lib.rs` |
| Desktop client | Wait for both outcomes, then revalidate and schedule the opening use case. | `Center::dispatch_link_if_ready` |
| Domain port | Declare the validated URI-opening contract and application launch context. | `UriPort` in `crates/lucent-domain/src/uri.rs` |
| OS adapter | Resolve the default handler and launch it with the checked context. | `XdgUri` in `crates/lucent-services/src/applications.rs` |

All paths in this table are relative to `lucent/`. The framework reports an input
release outcome to the client; it does not call the URI adapter itself. This keeps
notification policy in the client and operating-system work behind the domain port.

An unavailable activation protocol, missing current input, timeout or rejection
does not prohibit opening selected content. The opening gets an empty context
and the compositor retains its normal focus policy. Token delivery, observed
focus and actual consumption by a destination are separate observations; there
is no compositor forced-focus fallback or reuse of an earlier token.

## Validation scope

Source tests cover optional-target delegation and identifier validation, actual
serialized serial/seat/surface/app-id requests on an owned socket pair, stale and
duplicate notification completions, cancellation of retired queued work, context
delivery to an injected URI adapter, and literal launch/environment policy.
These tests must actually run before they can be reported as passing.

Native acceptance must additionally exercise a real default handler, selected
through a private owned test configuration, using current pointer and keyboard
input. It must verify the selected URI and handler lifetime and restore the
original handler configuration. The existing explicit-application Open tests do
not prove this default-handler path, physical focus behavior or direct native
token consumption. Preparing this implementation or harness closes none of
those native validation requirements by itself.

## Native input-release acknowledgement

The generic SDK effect requires the client to request `Keyboard::None` and
`PointerInput::None` on an existing ordinary layer surface. The Wayland backend
captures that exact native object. A private, never-reused callback tag is bound
only to a successful buffer submission after barrier admission. The compositor's
real `wl_surface.frame` callback for that submission acknowledges processing of
the commit carrying those input policies. The ordinary pacing callback and render
commit remain unchanged; no extra surface commit, blocking roundtrip, fixed delay
or focus command is used. `FrameSubmitted` alone cannot complete this effect.

The callback revalidates native lifetime and current requested/committed policies.
Retirement, interactive supersession, unsupported/secure roles, busy admission and
a two-second timeout are typed outcomes. Cancellation suppresses completion. Eight
requests and eight outstanding tagged callbacks bound retention, including callbacks
whose cancelled or timed-out caller is gone. Old or failed submissions cannot
acknowledge a newer request. A hidden or DPMS-disabled compositor can withhold frame
callbacks; the effect times out without dispatching a handler.

For a live native surface, cancelled or timed-out callbacks remain counted until
their `Done` arrives. On native teardown, a private owner locally destroys only its
tagged callback proxies before the renderer and surface role are dropped. A
`wl_callback` has no destructor request; this uses the Wayland library's local
object-destruction API and preserves server `delete_id` bookkeeping. Frame records
share a retirement flag rather than owning the native surface's callback guard.
An already queued valid `Done` survives protocol-proxy death; only explicit native
retirement allows those records to be discarded. A late callback cannot release
a replacement surface with the same logical name.

The two-second limit is a deadline, not a promise that application code runs at an
exact wall-clock instant. Completion and timeout messages are dispatched by the UI
event loop. If that loop is delayed, delivery is delayed too; an overdue callback
still cannot report `Released` when dispatch resumes.

Passive toasts request `Keyboard::None` and keep `PointerInput::Content`, so their
buttons and links remain clickable without requesting native keyboard ownership.
History requests exclusive keyboard input for navigation; yielding and exit paint
disable both inputs. Successful handoff or an ordinary history close restores only
passive pointer actions while a toast remains. It never restores toast keyboard
ownership before a newly opened application maps. The compositor's global focus
configuration is unchanged.

This is a commit-processing acknowledgement, not a scanout, compositor focus,
destination mapping or trusted keyboard-input guarantee. Source `3640bd86c6ec`
passes its coherent 17-stage build and 823 targeted tests, including the protocol
and client regressions, and is coherently deployed as an unmerged VM candidate.
Its [cold-browser trial](../reports/measurements/lucent-cold-default-uri-3640.json)
passes pointer opening and trusted document `KeyA`, but the separate Tab/Enter
route still fails browser focus and key delivery. The new ordering has no verified
keyboard fix. Earlier native outcomes and cleanup flags remain unchanged.

## Separate cold-browser acceptance

`vm/test-cold-default-uri-notification.py` prepares two independent cold tests:
a pointer link, followed by a Tab/Enter link. Each route allocates a new private
profile and Chromium process birth. The second route cannot start unless the
first feature checks and cleanup succeed. The maintained warm-browser harness
and its blank control window retain their existing contract.

The repository harness retains its earlier `eb3481f4` preparation binding and
performs no VM actions by default. Actual trials use separate immutable bindings
to their deployed source, release manifest, fresh desktop identity and test VM.
The archived eb348 run and the later source 3640 run below retain distinct
controller hashes and outcomes. A binding or a prepared harness establishes no
native pass or performance result by itself.

Before native dispatch, the cold fixture pins an absent-profile startup
declaration containing the one owned HTTP URI. It does not launch a blank
browser or change the canonical startup manifest. The first real link must
create the original recorded process, map its exact class and PID, and expose
one exact document through its original DevTools endpoint. Identity changes and
extra pages fail; no process, profile, target or lease is refreshed or adopted.

Each route then requires a trusted, unmodified native `KeyA` in that exact
document after the history closes and releases modal input. CDP only arms and
reads the bounded observer. The harness does not focus a browser, activate a
target, click browser content or hide the shell to make the key succeed.
Reading an active window or seeing an activation variable is insufficient.

The independent configuration guardian revokes queued work and retires the
exact cold Chromium scope before canonical entry cleanup. It also handles a
still-original startup wrapper without waiting for window or CDP readiness.
If the original process has already disappeared before any moved-scope lease
was recorded, cleanup remains failed: an unknown scope cannot be searched for,
adopted or signalled. MIME/entry restoration still runs with its existing
inode and content ownership checks. Notifications expire naturally; only an
owned empty history may be closed during cleanup. All windows and user-added
content in these new disposable test profiles are retired; existing personal
profiles and foreign applications are preserved.

## Recorded cold trial on eb348

The preparation section above describes the earlier unbound checkpoint. The later
[source-specific report](../reports/measurements/lucent-cold-default-uri-eb348.json)
uses bound harness `896e479f` and runtime `eb3481f4`, archive `e48716b0`. The pointer
route opens its exact fresh document and receives one trusted, unmodified KeyA.
Its nine named cleanup checks pass.

The separate Tab/Enter route reaches the exact document with fresh launch
context, but its focus observation is false and trusted KeyA delivery is not
observed before the bounded wait fails. Natural history/modal release and native
submission-advance guards pass before dispatch; all nine cleanup checks pass.
Cold acceptance remains incomplete. Cleanup does not turn that feature failure
into a pass, and no failure cause or production fix is established. Existing
warm-route evidence keeps its original scope.

## Recorded cold trial on 3640

The [source 3640 report](../reports/measurements/lucent-cold-default-uri-3640.json)
uses bound controller `c11b81d9` and coherent release manifest `075b88c5`.
The six user clients and root greeter are verified after deployment; settings,
user state and security configuration are preserved without restarting greetd.
The archive's original classification flags remain unchanged, and this is an
unmerged candidate deployment.

| Route | Actual result | Cleanup |
| --- | --- | --- |
| Pointer | One exact cold document receives one trusted, unmodified, nonrepeat `KeyA`. | All nine named checks pass. |
| Tab/Enter | The exact cold document opens, but the browser-focus observation is false and trusted `KeyA` delivery is not observed before the bounded wait fails. | All nine named checks pass. |

Both routes pass the last pre-key guards for natural history closure, modal input
release and native-frame advancement. The missing keyboard event count and
post-event observation remain `null`. The failure is
`browser_key_delivery_wait / owned_browser_received_native_key_a`; a successful
cleanup does not change that result.

Later read-only options and widget-policy observations occur after cleanup and
do not establish the keyboard owner at failure time. The cause remains open.
Protocol tests, deployment and the separate 118-input/480-image offscreen export
do not establish a keyboard fix, broader handlers, hardware or performance.

## Focus geometry and cursor-only diagnostic

The later [focus investigation report](../reports/measurements/lucent-cold-default-uri-focus-diagnostic-3640.json)
keeps runtime `3640bd86c6ec` unchanged. A read-only recorder repeats the canonical
pointer success and keyboard failure. At browser mapping, before `KeyA` and after
its dispatch, the owned browser is mapped, visible and accepting input in both
cases. The pointer document reports focus; the keyboard document does not.
Every case passes its own nine cleanup checks.

The keyboard cursor lies within the recorded Clock card bounds. The pointer route
ends at the selected notification link, outside that card's bounds but inside the
larger native group named `clock`. That group also contains Media controls, so its
keyboard capability cannot be inferred from its first member's name.

| Case | Actual observation | Acceptance boundary |
| --- | --- | --- |
| Canonical pointer with recorder | Document focus and one trusted, unmodified, nonrepeat `KeyA`. | Scoped pointer pass. |
| Canonical Tab/Enter with recorder | No document focus; trusted `KeyA` delivery is not observed before the bounded wait fails. | Keyboard failure preserved. |
| Tab/Enter with cursor moved to the pointer route's link point | Document focus and one trusted, unmodified, nonrepeat `KeyA`. | Diagnostic success; canonical acceptance is explicitly false. |

The last case adds only cursor movement. It retains Tab/Enter and adds no browser
focus request, activation, click or forced shell hide. The changed outcome supports
an under-cursor widget hypothesis. These snapshots are non-atomic, their input
modes are requested policy, and runtime 3640 exposes no actual seat keyboard owner.
Rectangle membership and document focus do not establish the failure mechanism.
The original negative trial remains unchanged.

Candidate `0a981631dca2` uses the existing root foreground lifetime to suppress all
widget keyboard demand while a panel remains open, including notification release
and URI dispatch. Otherwise, each group's current members determine its static
`OnDemand` capability: Notes and existing interactive controls remain available;
Clock/System-only groups are passive outside layout editing. Candidate
`b2f54551dfbe` adds read-only exact Lucent-native keyboard observations and separates
requested from committed input policies. A client observing no current keyboard
surface does not identify the compositor's global keyboard owner.

Exact `b2f54551dfbe` now passes the Ubuntu
[Rust job](https://github.com/xandreeddev/desktop-lab/actions/runs/40411820911/job/121884163923)
in [CI run40411820911](https://github.com/xandreeddev/desktop-lab/actions/runs/40411820911),
including workspace checks, controlled service-protocol suites and software Vulkan
visual regressions. All **11 widget-policy and six diagnostic tests** have
individually verified passing log entries. A coherent Arch release, candidate VM
deployment and native keyboard acceptance remain pending. A widget's requested
`None` can still await its own frame callback or render retry. Notification release
does not acknowledge every widget commit, so this policy retains no unconditional
focus or keyboard-fix claim.
