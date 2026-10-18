# Surface roles and concrete instances

`SurfaceSpec.id` names a logical role such as `bar`. An optional validated
`SurfaceInstance` distinguishes concrete copies of that role. Their typed
`SurfaceAddress` is the native runtime key; its printable form is diagnostic
text, never an identity parser. No dynamic string is leaked into a static ID.

```rust
let instance = SurfaceInstance::new("DP-1")?;
// In this output's SurfaceSpec:
// id: "bar", instance: Some(instance.clone()),
// output: Some(OutputName::new("DP-1")?), ...
effects.redraw_surface(SurfaceAddress::new("bar").instance(instance));
```

Instance identity and output selection are separate. A client may choose the
output name as its instance value, as the independent example does, or use a
different validated identifier. Output names select current protocol objects;
instances select application state and native surfaces. An address must be
unique among the application's declarations, including invisible declarations.
Two different instances may share one role. Existing `instance: None` clients
retain their original role identity and unwrapped events.

## Registration and rendering

Run the independent consumer in a layer-shell/Vulkan session:

```sh
cargo run --manifest-path lucent/Cargo.toml --locked -p multi-output
```

It starts with no surfaces, receives the registry's `OutputsChanged` catalog,
and declares one top bar per unique named output. Unnamed and ambiguous outputs
are omitted. Each bar has its own counter, text value and `TextFieldState`.
Disconnecting an output removes that example's transient state; persisting a
desktop layout is a separate domain/client policy. `--help` needs no session.

The runtime validates declarations before native mutation, reconciles by typed
address, and resolves each optional output selector through its current registry
bindings. Each address owns its native role, scene, focus, pointer capture and
frame demand. All use the existing shared GPU device. Layout and rendering know
neither monitor policy nor the meaning of an instance string.

`ViewContext` carries the role and optional instance together with the actual
configured dimensions and monotonic time. The client selects the corresponding
state and builds an ordinary `Element` tree. A callback captures that instance
in its typed message, or a child tree uses `Element::map` to wrap it. Element IDs
need only be stable within their native surface; both bars may have an `edit`
field without sharing keyboard state.

## Events, effects and animation

Native key, pointer boundary/button, outside, scroll, resize and submitted-frame
events for an instanced role arrive as `Event::Scoped { instance, event }`.
The inner event retains its logical role. Output catalog observations are global
and are not wrapped. A client routes the wrapper into its own typed messages;
the runtime does not discover or mutate a monitor-specific component for it.
Scene callbacks already carry their client-produced instance messages.

`Effects::redraw("bar")` invalidates every current instance of that role and any
matching redraw group. `redraw_surface(address)` targets one concrete address.
Both coalesce dirty demand and obey the existing compositor frame callback;
neither paints immediately. `Effects::delegate` preserves targeted redraws and
maps asynchronous/clipboard completions while lifting child messages.

`Application::animating_instance(address, now)` lets a client report animation
demand for just that instance. Its default delegates to the existing
`animating(role, now)` method. Client-owned `Motion` performs time interpolation;
the compositor paces dirty buffers and the renderer paints sampled geometry.
Creating instances adds no continuous polling or idle redraw loop.

## Clipboard and retirement

Address ordinary fields with
`ClipboardTarget::new("bar", "edit").in_instance(instance)`. The native adapter
requires the exact focused address and field, a visible keyboard-capable current
declaration, the current focus epoch and the actual native surface object.
Reusing a role on another output cannot authorize an old transfer. The existing
client ticket also rejects completions for changed text/selection.

Retiring one address clears its native keyboard authority and capture; it does
not retire another copy of the same role. Output target changes still destroy
and recreate the native role, with GPU resources retired before protocol roles.
Secure session-lock retains its separate all-output path and authenticated gate;
ordinary instance/output declarations cannot narrow secure coverage.

`Effects::recreate_surface(address)` explicitly renews an ordinary non-reserving layer-shell
native lifetime while keeping its logical address and instance. Use it when a
client intentionally starts a new coordinate/input lifetime. It is not an
implicit consequence of every resize. Delegation preserves the exact address;
hidden or unknown addresses have no native lifetime to replace.
Current committed/pending and replacement declarations must all have
`exclusive_zone <= 0`. Reserved layers are rejected before native mutation:
layer-shell does not provide an atomic reservation transfer between surfaces,
so this API does not briefly double-reserve or release a bar's reserved area.

The old surface becomes pointer- and keyboard-inactive immediately. One
last-good native presentation remains while the replacement configures; only
the replacement's successful submission retires it. Retry/error does not claim
a presented replacement, and repeated requests cannot replace retained good
pixels with an unpainted pending surface. Input, configure, retry and frame
events are admitted by current native object identity, rechecked after
synchronous clipboard/client callbacks and between native message deliveries. The replacement gets
fresh scene/focus/capture state and native Enter coordinates, while logical
submission counts and the latest bounded timing/profile history continue.
Inspector `frames` is that logical count; `native_frames` counts the current
renderer and `retiring_native_surfaces` reports retained presentations. Profile
payload accounting includes both current and retiring resources.

Hiding/removing the address, output retirement, failure and runtime teardown
release both lifetimes with GPU resources dropped before protocol roles.
Session-lock runtimes reject any recreation request before the effect batch
can perform tasks, clipboard operations or native mutations. Recreation never
creates a second lock surface or authorizes unlocking. This adds no worker,
polling or idle redraw. Native stationary-pointer and visual-handoff acceptance
must still be recorded for the exact deployed source.

## Validation scope

Pure tests exercise validated identities, typed-key collision resistance,
legacy/scoped lifecycle events, delegated targeted effects, independent example
state and rejection after disconnection. The complete workspace checks compile
the actual runtime and every existing neutral consumer. Actual compositor
placement, simultaneous native input, clipboard transfer between outputs,
disconnect/reconnect, frame pacing and secure multi-output acceptance remain
deferred. The subsequent [desktop monitor implementation](monitor-layouts.md) adds
persisted profiles and hosts using this API. Actual compositor acceptance and
fractional scaling remain unfinished. This SDK example is a separate client.
