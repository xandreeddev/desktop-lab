# International text and input methods

The component still owns its ordinary `String` and `TextFieldState`. It declares
`Element::text_input` or `Element::text_area`; the framework emits relative
`TextEdit` values. Complex-script shaping and the Wayland input method do not
introduce a domain service, persistence layer or second document model.

`Application::fonts()` continues supplying immutable font bytes. The runtime
creates `lucent_ui::text::Font`, which retains those OpenType tables alongside
fontdue's rasterizer. Consumers constructing a `Layout` or headless renderer
use that framework font type as well. Fontdue's `FontSettings` remains accepted.
The source bytes are necessary: a pre-rasterized character map cannot resolve
contextual substitutions, ligatures or script positioning.

## One source map

`text::shape` resolves visual directional runs with Unicode bidi, then shapes
script runs through rustybuzz. A shaped line contains actual glyph IDs, fallback
face identities, logical positions, source byte clusters, visual grapheme stops
and selection segments. The renderer rasterizes those glyph IDs at the existing
2× physical coverage density. It does not shape characters independently.

Single-line hit testing, horizontal scrolling, caret and disjoint selection
painting use the same positions as the renderer. Multiline editors map those
stops back through tab projection and soft-wrap rows; Up/Down keeps its visual x,
while Left/Right walks visual stops. Logical source offsets remain UTF-8
extended-grapheme boundaries. A ligature containing several editable graphemes
gets interpolated interior caret stops; this initial implementation does not read
optional OpenType GDEF ligature-caret tables.

ASCII preserves the prior character advances and pair kerning exactly. Density
changes do not reflow text or change logical positions. The native renderer keeps
its existing size, glyph and mask allocation checks and coverage supersampling.

## Bounded fallback

Font fallback is deterministic and does not scan the machine during layout.
The primary application face wins whenever it covers a whole grapheme. Otherwise
the framework tries its bundled Noto faces in a fixed order. Missing unsupported
characters remain the primary face's missing glyph; they are not silently
removed. Bundled fonts and their separate license files live in
`lucent/crates/lucent-ui/assets`.

The fallback set includes Arabic, Devanagari, Hebrew, common Han ideographs and
Extension A, Kana/Bopomofo and Hangul syllables/Jamo. CJK and Hangul are static
400-weight subsets derived from the official Google Fonts Noto sources, with
renamed derivative family names. The asset manifest records source and derived
SHA256 values, Unicode ranges, codepoint counts and individual OFL license files.
The CJK subset contains 28,323 codepoints in 10,167,004 bytes; Hangul contains
11,623 in 2,697,016 bytes. Each rasterizer initializes independently only when
that fallback is actually selected. A missing emoji therefore does not parse all
CJK outlines. Coverage tables are borrowed from immutable embedded bytes.

## Input-method ownership

The optional native `zwp_text_input_v3` adapter binds one text-input object for
the selected keyboard seat. It enables input only when both native keyboard
focus and text-input `enter` identify the exact current surface, and the freshly
resolved focused element is an ordinary controlled field. A surface role alone
never grants authority. Focus loss, hidden/recreated controls and seat retirement
remove authority. Secure-session runtimes do not bind the manager.

Masked inputs expose only a count and redacted edits. They never supply a
canonical text hit, surrounding text, preedit string or clipboard authority.
The adapter cannot recover their secret value from the scene. Authentication
and ordinary application editing remain independent.

Surrounding text is a UTF-8 window no larger than 4000 bytes. Cursor and anchor
are byte offsets inside that window. An oversized selection is omitted rather
than split. Cursor bounds come from the actual logical caret geometry and are
sent in surface-local coordinates. Native composition events are buffered until
`done`; deleting surrounding text and inserting committed text become one
`TextEdit::InputMethod` so client byte admission and undo stay atomic.

Preedit is transient UI-thread presentation. It replaces the selected span only
in the painted projection, supports a composition caret/selection and underline,
and never enters persisted text or inspector hit values. The next committed edit
still applies through the component's normal update callback. Content and focus
changes retire stale composition. Native serials defer protocol state updates
when the compositor is behind, while retaining the current field lifetime guard.

## Independent consumer and verification

```sh
cargo run --manifest-path lucent/Cargo.toml --locked -p international-text
```

This example imports the API, design system and Wayland runtime only. It presents
mixed Arabic, Hebrew, Devanagari and CJK text in single/multiline fields plus a
count-only masked probe. It uses the same controlled API as the desktop; there
is no special client-side IME registration or renderer dependency.

The implementation is under development. Source regression cases are being
added for shaping, source stops, mixed selections, atomic edit admission,
surrounding UTF-8 windows and focus retirement. Their existence is not evidence
of successful compilation, native compositor IME interoperability, 1×/2× pixel
acceptance or performance. Those checks will be recorded after execution.
Accessibility bridging remains explicitly deferred by the user.
