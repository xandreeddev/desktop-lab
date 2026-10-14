# Declarative text editing

Ordinary text fields use a shared framework API rather than editing the last
painted string in the input dispatcher. The component owns its value and a
`TextSelection`. The framework emits `TextEdit` messages and renders the next
declared value/selection.

```rust
Element::text_input(&self.value, self.selection, "Search", Message::Edit)
    .id("search")
    .autofocus()
```

In the component's update, apply the message to the **current** value:

```rust
let change = self.selection.apply(&mut self.value, edit, 8192)?;
if change.content {
    // Refilter results or request persistence through the owning coordinator.
}
effects.redraw("panel");
```

The byte admission limit is supplied by the client. Selection changes request
paint but need not refilter applications or save preferences. A rejected
insertion/replacement changes neither value nor selection. Existing oversized
values remain deletable; the constructor/view does not silently truncate them.

## State and input

`TextSelection` owns only byte offsets. It holds no text, renderer handle, OS
operation or domain dependency. Every edit reconciles those offsets against the
current string at extended-grapheme boundaries. The selection's getters/range
helpers never split UTF-8. `TextEdit` debug output redacts inserted/replaced text.

Supported operations include insertion/replacement, backward/forward deletion,
clear, select all, grapheme/word navigation, word deletion, extending selection
and pointer selection. Arrow keys move; Shift extends; Ctrl+arrows move by word;
Home/End reach the ends; Ctrl+Backspace/Delete remove word spans.

Extended graphemes use the maintained
[Unicode segmentation implementation](https://docs.rs/unicode-segmentation/latest/unicode_segmentation/trait.UnicodeSegmentation.html).
This improves editing of combining characters and emoji sequences. It does not
establish complex-script shaping, bidi visual navigation or font fallback.

Relative edit messages are essential: several native key events can arrive
before a compositor frame. Each callback returns an intent such as Insert or
Move, and the UI thread applies it to the latest component state. An old scene
cannot overwrite the newly edited string with its prior value.

The Wayland adapter translates modified cursor keys and physical Shift presses
into generic API inputs. Application-level handling still runs first, so clients
retain launcher Tab modes, Escape, Enter and other owned shortcuts. Return no
message for keys the shared field should handle. Existing legacy controls can
normalize modifiers to preserve their prior behavior.

## Geometry and rendering

Layout prepares one immutable text-hit geometry per field. Grapheme boundaries
map to the same logical glyph advances used by the current renderer. Caret and
selection painting consume that geometry; pointer placement consumes the same
boundary positions. The active edge scrolls into view in a narrow field.

Pointer capture retains the selection anchor, while subsequent frames refresh
the displayed horizontal offset. Value, field geometry, clipping or lifetime
replacement cancels obsolete capture. A field callback maps normally through
`Element::map`; client IDs and field policy never enter the renderer.

Single-line edits remove incoming CR/LF. Existing line breaks are projected as
spaces for display without changing the stored value. Multiline layout/editing
is separate work. Selection uses the design system's primary role and component
opacity token; physical text density and supersampling remain unchanged.

## Clients and boundaries

The launcher owns query selection privately and refilters only after content
changes. The widget host owns notes selection while borrowing the root settings
aggregate; caret movement never creates a settings write. Notes content still
returns a save intent to the one persistence coordinator. Native menus use the
same control and preserve their original accepted-value semantics.

The independent `text-input` example composes the API with design recipes and
the normal Wayland runtime. It has no desktop-client/service dependency:

```text
cargo run --manifest-path lucent/Cargo.toml --locked -p text-input
```

Passwords continue using `masked_input`, a count and redacted `InputEdit`
intents. Their actual values stay in the client's zeroizing storage. Ordinary
`text_input` exposes its value to scenes and must not be used for secrets.
Secure clients retain their existing authenticated protocol and end-editing
behavior; this feature does not change authorization.

## Acceptance still open

Native VM/hardware acceptance is deferred under the user instruction. Current
shaping remains the existing kerning-based implementation. Clipboard transfer,
IME, bidi/complex-script shaping, font fallback, multiline/undo behavior and
platform accessibility remain separate requirements. Editing tests and
synthetic Vulkan references cannot prove those features.
