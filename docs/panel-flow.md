# Ordinary panels, shared bar controls and keyboard focus

Lucent's desktop owns which ordinary panel has the foreground. Domain ports,
OS adapters and the renderer keep their existing responsibilities. Secure lock
and authentication clients have separate protocol authority.

## Follow a click from the launcher to audio

1. While the launcher is open, its native layer surface has exclusive keyboard
   focus. Its element tree contains both the launcher and the shared bar component.
2. Hyprland delivers the pointer event to that surface. The SDK tests the normal
   prepared, rounded bar hit geometry and emits the existing typed audio message.
3. `Desktop::update` delegates to the audio child. The child changes its own state
   and requests its service effects.
4. `Desktop::panel_opened` observes the closed-to-open transition and sends normal
   Close/Hide messages to other panels. Each child owns cancellation and exit motion.
5. The audio surface now presents the same bar component and owns keyboard focus.
   The standalone bar surface retains its exclusive zone, so tiled windows keep
   their position. Its contents on the active monitor stay empty during the panel.
6. When the last panel closes, the standalone bar presents the controls again.
   Bars on other monitors remain independent.

This is ordinary declarative composition: `compose_panel_bar` builds a stack of
panel content and `bar(&bar_context)`. Both use the same `Element` layout, design
tokens, hit testing and typed callbacks. The view does no native operations.
The SDK does not forward synthetic clicks between native surfaces or know about
a desktop bar. There is no extra domain port for this presentation policy.

The policy lives in
`lucent/apps/lucent-desktop/src/desktop/panels.rs`. It covers launcher, audio,
network, Bluetooth, power, tray, live-wallpaper settings and notification history.
Each child owns its open state; the root derives the current host from those
states, without keeping another active-panel state variable.

## Why the controls share a native surface

Wayland keyboard and pointer declarations are separate, but compositor behavior
must be tested. Hyprland 0.56.2 first checks exclusive layer surfaces and falls
back to one even when its input region does not contain the pointer. See its
[pinned input-routing implementation](https://github.com/hyprwm/Hyprland/blob/v0.56.2/src/managers/input/InputManager.cpp#L413-L424).
A native VM test confirmed that excluding the bar strip from the launcher's
input region still sent the click to the launcher.

The desktop therefore composes the related controls inside the focused native
surface. It keeps reliable keyboard ownership and uses the normal SDK event
path for those controls. `PointerInput::All` captures the open panel's extent;
`Content` and `None` retain their existing uses. No compositor patch or custom
input-region API is required.

The active panel declares the existing `redraw_group: Some("bar")`. Clock,
workspace, audio and tray observations that invalidate the bar also invalidate
its current host. Changing hosts invalidates the standalone bar. Hiding the bar
removes its composed controls as well as the standalone reservation.

The live-wallpaper panel stays below the bar on the shared spacing grid. Its
existing scroll viewport keeps lower controls reachable on shorter outputs.

## Child state, asynchronous replies and dismissal

A Bluetooth prompt or tray error can open a panel after an adapter finishes.
The foreground rule observes that actual opening. Repeated messages inside an
already-open panel do not repeatedly cancel unrelated work. Opening another
panel retires a pending direct tray action, preventing a late error from
reopening dismissed UI.

Notification history has exclusive keyboard focus while open; Tab and Escape
work immediately. Closing releases keyboard and pointer ownership during its
fade. Ordinary toast delivery and history storage continue independently.

Captured primary presses outside a hit produce `Event::Outside`; launcher and
history map it to dismissal. Other children use their backdrop Close callback.
History padding has an inert hit with hover feedback and tab stops disabled.
The shared clock capsule also stays passive.

Use `lucent-cli notifications hide` (or `close`) and
`lucent-cli wallpaper live close` to dismiss those panels. Closing live-wallpaper
settings does not stop playback; `wallpaper live off` is separate.

## Validation

Tests cover all 64 ordered panel pairs, repeated updates, pending tray work,
notification dismissal, host ownership, shared hit geometry/messages and bar/grid
geometry. Dedicated Vulkan panel-host fixtures complement the isolated component
references; screenshot tolerances remain unchanged.

`python3 vm/test-panel-flow.py` exercises real bar clicks, history padding,
keyboard focus/Escape, outside dismissal and hiding the bar in the existing VM.
`vm/test-tray.py` also checks direct app activation while the launcher is open.
Both require the prepared unlocked guest and preserve saved settings and wallpaper.
Having a harness does not prove acceptance; executed results are recorded separately.
