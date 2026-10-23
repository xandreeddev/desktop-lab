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
4. On completion the client matches the operation generation and revalidates the
   exact notification version and current content URI. A stale completion cannot
   admit another opening or change a newer operation. Current content is checked
   independently of the prepared view cache.
5. The client converts the SDK outcome into the existing opaque
   `ApplicationLaunchContext`, then schedules `UriPort::open(target, context)` on
   the controls queue. Replacement, clear, a newer opening or client retirement
   cancels work that has not started. An already-running OS operation cannot be
   undone; its late completion cannot affect a newer intent.
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
