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

Passive toasts request `Keyboard::None` and keep `PointerInput::Content`, so their
buttons and links remain clickable without requesting native keyboard ownership.
History requests exclusive keyboard input for navigation; yielding and exit paint
disable both inputs. Successful handoff or an ordinary history close restores only
passive pointer actions while a toast remains. It never restores toast keyboard
ownership before a newly opened application maps. The compositor's global focus
configuration is unchanged.

This is a commit-processing acknowledgement, not a scanout, compositor focus,
destination mapping or trusted keyboard-input guarantee. The source implementation
and protocol/client tests are prepared for review; they have not been compiled or
run at this checkpoint. A new coherent archive and fresh cold-browser pointer and
Tab/Enter tests, including trusted document `KeyA`, remain required. Earlier native
trial outcomes and their cleanup flags remain unchanged.

## Separate cold-browser acceptance

`vm/test-cold-default-uri-notification.py` prepares two independent cold tests:
a pointer link, followed by a Tab/Enter link. Each route allocates a new private
profile and Chromium process birth. The second route cannot start unless the
first feature checks and cleanup succeed. The maintained warm-browser harness
and its blank control window retain their existing contract.

This new harness is source-only. It pins the actual `eb3481f4` release manifest
with digest `e48716b0…`; its default command performs no VM actions. `--run`
also requires that exact manifest, a fresh desktop PID and the exact test VM
UUID before contacting the VM. This preparation establishes no cold native
pass, focus result or performance result.

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
