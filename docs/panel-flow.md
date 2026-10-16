# Ordinary panels, bar input and keyboard focus

Lucent owns this presentation policy. Omarchy, the domain ports and the Vulkan
renderer do not decide which panel is in front. Secure lock and authentication
clients have their own protocol authority; this guide describes ordinary desktop
panels only.

## What happens when a bar control is clicked

1. Hyprland delivers the pointer event to the surface whose native input region
   contains that position. Ordinary overlays leave the bar strip outside that
   region, so the bar can receive the first click even while a panel is open.
2. The SDK tests the bar's prepared, rounded hit geometry. Its button produces a
   typed child message, mapped through the root's monitor route.
3. `Desktop::update` delegates the message to that child. The child changes its
   own open state and requests any service effects it needs.
4. `Desktop::panel_opened` compares the child's state before and after update.
   On a new opening, it sends normal Close/Hide messages to the other panels.
   Those children retain their own cancellation and fade-out behavior.
5. The runtime reconciles `SurfaceSpec` values, draws dirty surfaces and commits
   their pixels and input regions to Wayland. The closing panel releases its
   exclusive keyboard mode; the new panel becomes the keyboard owner.

The client policy is in `lucent/apps/lucent-desktop/src/desktop/panels.rs`. It covers the
launcher, audio, network, Bluetooth, power, tray, live-wallpaper settings and
notification history. There is no duplicate global “active panel” state: each
child owns its state and the root observes transitions. A notification toast or
an incoming snapshot is not itself a history-panel opening.

This distinction matters for asynchronous messages. A Bluetooth prompt or a tray
error may open a panel after a native operation completes. The same foreground
rule applies to that actual opening. Repeated updates in an already-open panel
do not repeatedly cancel work elsewhere. Opening a different panel retires a
pending direct tray action so its late failure cannot reopen dismissed UI.

## Input regions do not resize windows

`PointerInput` is a generic presentation API in `lucent-api`:

```rust
PointerInput::Inset {
    top: reserved_header_height,
    right: 0,
    bottom: 0,
    left: 0,
}
```

Insets are unsigned logical pixels. The Wayland adapter clamps all four against
the configured native extent, using saturating arithmetic. If opposite edges
meet or cross, the region is empty. The adapter creates a normal `wl_region` and
submits it through `wl_surface.set_input_region`. It contains no bar identity,
design token or panel rule.

The desktop supplies `component.panel.bar_height` when its bar is visible and
zero when hidden. The input region follows the actual native size; it does not
alter the viewport, painting, framebuffer, GPU shader or window placement.
`Content`, `All` and `None` remain available for other surface policies.

A captured primary press outside the tree's hits becomes `Event::Outside`.
Launcher and notification history map it to dismissal. Other panel children use
their existing backdrop hit and Close message. Inert hits inside panel padding
prevent that padding from becoming an outside click; they disable hover feedback
and keyboard tab stops. The clock capsule also stays inert.

The live-wallpaper panel is centered within the area below the bar. Its bounds
use the existing shell grid and spacing tokens, and its controls retain the
shared scroll viewport. It no longer paints its header over the bar on a short
output. This geometry is client policy in `shell_layout`.

## Keyboard and pointer ownership are separate

An ordinary open panel uses `Keyboard::Exclusive`; closing releases it.
Notification history now follows that rule, so Tab and Escape work immediately
after opening from the bar. Passive notification toasts retain on-demand keyboard
behavior. Their delivery and history storage are not cancelled by another panel.

Inspection exposes `keyboard`, `pointer_input` and, for inset coverage,
`pointer_insets` in top/right/bottom/left order. These are declarations and prepared
runtime state. Real pointer/key acceptance is still needed to prove compositor
behavior; a unit assertion about a SurfaceSpec is not enough.

Use `lucent-cli notifications hide` (or `close`) and
`lucent-cli wallpaper live close` to dismiss those panels. Closing live-wallpaper
settings does not turn off playback; `wallpaper live off` is a separate command.

## Validation

The client tests all 64 ordered panel pairs, repeated updates, pending tray work,
notification dismissal and bar/grid geometry. SDK tests check inset bounds,
resizing, zero extents and overflowing edge values.

`python3 vm/test-panel-flow.py` exercises real bar clicks, history padding,
keyboard focus/Escape, outside dismissal and a hidden bar in the existing VM.
`vm/test-tray.py` also checks direct application activation while the launcher is
open. Both require the prepared unlocked guest and preserve saved settings and
wallpaper. Executed results belong in the acceptance report; having a harness
in the repository is not evidence that it passed.
