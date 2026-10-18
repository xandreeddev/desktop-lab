# Multiline editing and Notes

`Element::text_area` is an ordinary controlled editor. It reuses `TextEdit`,
`TextSelection`, `TextFieldState`, `ScrollSpec` and the existing clipboard effects.
It adds no desktop domain port, persistence mechanism or engine-owned document.
The standalone `text-area` example imports the framework and design library only:

```sh
cargo run --manifest-path lucent/Cargo.toml --locked -p text-area
```

## The model belongs to the client

Keep the canonical `String`, `TextFieldState::multiline()` and vertical offset in
component state. Pass `field.selection()` to the view. Apply each emitted
`TextEdit` against that current String through `field.update`, then redraw.
Store a `ScrollEvent.offset` without editing or persisting the text.

```rust
let editor = theme.text_area(
    &self.value,
    self.field.selection(),
    "Write something…",
    ScrollSpec::new(self.offset, component::scroll::LINE_STEP).unwrap(),
    Message::Edit,
    Message::Scroll,
).id("body");
```

The multiline policy is immutable and survives `reset`. Accepted Insert/Replace
and paste normalize CRLF and CR into LF before byte admission. Loaded content is
not silently rewritten; undo restores exact removed spans, including older CRLF
content. Empty lines and a final hard line remain editable. A newline breaks the
ordinary typing undo group. The default `TextFieldState` remains single-line.
Its original CR/LF removal and submit behavior are preserved.

Enter and Shift+Enter insert LF in a textarea; Ctrl/Alt/Logo+Enter do not edit it.
Tab and Shift+Tab traverse controls. Horizontal grapheme/word editing, selection,
copy, cut, paste and bounded undo/redo use the same ordinary editing model.
Ctrl+Home/End reach the document endpoints. Bare Home/End reach the current
visual row; Up/Down and PageUp/PageDown retain a horizontal goal across short
rows. Shift extends selection. These visual moves are resolved into ordinary
byte-based `TextEdit::Select`, rather than adding rendering-dependent moves to
the pure editing model.

A textarea is not suitable for secrets. Its normal strings appear in element,
scene, row and renderer text data; bounded ordinary history and clipboard tickets
also retain plaintext. Authentication continues to use count-only masked input
and zeroizing client-owned credential storage.

## One row map drives input and paint

`lucent-ui/src/text_area.rs` prepares measured visual rows with source byte ranges
and extended-grapheme stops. Greedy wrapping prefers whitespace boundaries,
preserves the whitespace itself, and admits a whole oversized grapheme rather
than splitting its UTF-8 bytes. Tabs display as four spaces while their source
range remains one tab. Existing CRLF content is one hard break.

The same rows drive pointer hit testing, two-dimensional drag selection, visual
navigation, selection rectangles, caret placement and reveal. At a soft wrap,
one byte is both the previous row end and the next row start. `Interaction`
therefore retains transient row affinity alongside the vertical goal. It keeps
those values only across the expected selection update against the same value,
row geometry, font metrics, viewport and focus identity. Content, horizontal
movement, incompatible geometry and focus retirement invalidate them.

Text and scrollbar capture are mutually exclusive. Own-scroll changes rebase a
live selection or thumb drag; replacement content and incompatible viewport or
row geometry retire capture. Caret reveal emits a clamped controlled offset after
an actual text, selection or viewport change. Wheel scrolling an unchanged caret
away does not immediately pull it back. Selection reveal is input-driven; there
is no continuous autoscroll timer while a pointer remains stationary outside.

Painting emits only visible rows and one row of overscan per edge, plus visible
selection rectangles and the caret. Each row is an independent bounded-height
`Paint::Text`; the full document never becomes a tall text texture. This preserves
the renderer's existing density and rasterization path, including its 1024-pixel
text-height limit. Layout still measures the entire client-owned document; this
is not virtualized layout or a general large-document editor.

## Pending frames do not freeze editing geometry

A compositor frame callback controls GPU presentation pacing. It does not control
when client state changes. The native runtime applies messages synchronously.
Before geometry-dependent input, it can build the current `app.view` and CPU
`Layout` scene and synchronize input without calling `FrameDemand::begin`,
renderer preparation, native regions/commit or `FrameSubmitted`.

After normal effects reconciliation installs current requested geometry, focused
editors affected by that redraw get current CPU geometry and a controlled caret
reveal batch. Unrelated wallpaper messages do not rewrap an idle Notes editor.
Native configure also handles a client that reads `ViewContext` directly without
consuming Resize. A pending sequence such as Insert → Down → Down → Shift+Up thus
uses current source offsets before another buffer can be submitted. Existing
`draw_all` remains the sole GPU submission gate.

This path may rebuild a document more than once during an editing interaction.
The worst-case 8192-byte typing/pointer cost needs measurement before claiming a
latency or CPU improvement. No extra worker or continuously repainting timer is
introduced by the editor.

## Notes is a client of that API

The desktop's canonical text remains `DesktopSettings.notes`; the pure domain's
`NOTES_MAX_BYTES` remains 8192. `widgets::Host` owns the multiline field, ordinary
history/clipboard ticket and transient scroll offset. The Notes view borrows
those values and uses `Theme::text_area` with existing semantic colors, font,
input insets, rounded control and scrollbar tokens. The generic SDK defaults
placeholder opacity to one; the design recipe supplies
`component.input.placeholder_opacity`, an alias of the primary
`opacity.placeholder` token. Single-line, masked and multiline painting all
consume that explicit style property.

The Notes card supplies its actual resized bounds to that view. The field fills
the area between the heading and save status instead of staying a fixed-height
single-line field in the middle of an enlarged card. Other widget layouts retain
their existing catalog dimensions.

Only accepted content changes return `Intent::Save`. Selection and scrolling
redraw without saving; the root persistence coordinator remains the sole settings
writer. Clipboard targets retain the current native group and monitor instance.
Hiding Notes or cancelling its host interactions retires pending clipboard work,
so a late response cannot resurrect an edit in a hidden/recreated field.

## Verification scope

The source contains regression cases for line normalization and atomic budgets,
undo boundaries, hard/soft rows, grapheme stops, sticky-x and wrap affinity,
pending-frame navigation, pointer/scroll capture, caret reveal, visible-row paint
bounds, single-line/masked submit compatibility, Notes save policy and stale
clipboard retirement. New visual fixtures cover empty, multiline, selected,
scrolled, narrow and resized Notes in both themes at 1×/2×.

These cases and fixtures require execution and review on the integrated revision.
They are not evidence of native acceptance merely because they exist. Real Notes
focus, Enter/Shift navigation, external clipboard transfer, thumb dragging,
restart persistence and exact settings/wallpaper restoration still require the
prepared VM. No document contents should be exported in public native evidence.

Complex-script shaping, bidirectional layout, font fallback, preedit/IME and
platform accessibility remain separate requirements. The current glyph metrics
support pair kerning and grapheme editing; that does not establish those features.
