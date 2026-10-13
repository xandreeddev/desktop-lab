# Widget layout editing

Open the launcher's Widgets section and choose **Edit layout**. The launcher
closes; widgets expose two size buttons and a bottom-right resize grip. The
buttons are ordinary SDK controls: pointer activation or keyboard focus/Enter
changes both dimensions by the design-system grid step. A grip drag changes
width and height independently. Escape from a widget, **Done**, or
`lucent-cli widgets edit off` exits the editor.

```text
lucent-cli widgets edit on
lucent-cli widgets size notes 400 240
lucent-cli widgets edit off
```

Editing adds a dedicated footer for controls below the existing content, so
notes captions and calendar dates remain unobstructed. Saved dimensions describe
the card without that editor footer. The footer height comes from the handle
and inset tokens and is included in native surface/input bounds.

Editing mode is private session state and does not survive a restart. Preferred
sizes are saved in `DesktopSettings.widget_sizes`, keyed by catalog identity:

```json
{"widget_sizes":{"notes":{"width":400,"height":240}}}
```

Merge fields into an existing file rather than replacing unrelated settings.
Stop the desktop before manual edits. Missing sizes use existing design defaults;
invalid dimensions fail loading and preserve the original file.

## Domain, policy and client boundaries

`WidgetSize` is an invariant-bearing domain value. Construction and serde
deserialization require finite, positive width and height. It contains no
renderer, Wayland, monitor or design-token types.

`lucent-usecases::widgets` constrains and snaps an extent using explicit minimum,
maximum and grid inputs. It validates before mutating the settings aggregate.
The domain and use case do not choose a widget catalog, output or spacing token.

The desktop catalog supplies the minimum design size. The active viewport
supplies a maximum. The client supplies the grid token. Resizing quantizes
absolute dimensions; if no grid point fits an axis, the minimum extent remains
to preserve required content. An existing default may not itself be a multiple
of the current grid. Moving still uses the same token to snap placement.

`widgets::Host` owns interaction state and its private `sizing::Resizer` owns one
pointer resize. A preview borrows the root settings aggregate and never edits
it. Only release or an explicit size action commits through the pure use case
and returns `Intent::Save`. The root persistence coordinator remains the sole
producer of settings writes. An unrelated notes/theme save therefore contains
the last committed size, not the current preview.

## Native surface and rendering flow

1. The SDK grip captures a pointer drag and emits logical-pixel `DragEvent`s.
   The generic Wayland adapter has no widget-size or grid policy.
2. A native physical press establishes a fresh gesture. The host freezes the
   source surface layout, widget position and source size during capture.
3. Source trees become input-only. The existing pointer-pass-through fullscreen
   preview draws the changed card bounds and token spacing grid. The preview
   receives neither pointer input nor keyboard focus.
4. Views compose ordinary SDK elements and immutable prepared artwork. Content
   keeps its original typography and design extent, centered within the changed
   card. This does not scale a bitmap or reduce physical rendering density.
5. Release snaps and commits the size, removes the preview and recalculates
   bounded native buffers, including rounded clipping and shadow support.

Hide, reset, leaving edit mode, output changes and explicit size changes cancel
capture. Late releases cannot overwrite a later size. A new physical press can
begin a fresh gesture. Reset clears both saved placements and preferred sizes.

Saved preferences survive output changes: presentation is constrained without
rewriting the preferred size. Existing grouping still merges overlapping paint
bounds so drawing order does not depend on same-layer compositor ordering.

## Verification and remaining work

Behavior checks exercise actual SDK grip selection/capture, keyboard size
buttons, launcher routing, checked IPC, cancellation, serialization, saved-size
restoration and unrelated saves. Synthetic Vulkan references review editor,
preview, settled, narrow and keyboard-focus states at 1×/2×. Executed results
belong in the measurement report.

VM input/display acceptance remains deferred under the user instruction. This
does not implement per-monitor layout persistence, text reflow in every widget,
touch resizing or settle animations. Native gesture acceptance, current RSS,
GPU/decode peaks and measured pacing remain distinct work.
