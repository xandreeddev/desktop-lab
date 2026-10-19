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
script runs through rustybuzz. Direction, selected font and Unicode
Script_Extensions each participate in internal itemization: one consumer-supplied
font covering both Latin and Devanagari still receives separate script buffers.
Common punctuation inherits a compatible neighboring script, inherited marks
stay with their extended grapheme, and no script boundary splits that grapheme.
These are internal shaping details, not another public component API. The
[HarfBuzz responsibilities](https://harfbuzz.github.io/what-harfbuzz-doesnt-do.html)
explain why the layout engine must itemize scripts before shaping. A shaped line
contains actual glyph IDs, fallback
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

Visual stop affinity is transient framework state: one logical byte boundary can
have two positions where directional runs meet. Walking the visual stops keeps
that affinity, including at soft wraps. Word movement continues to use the
component's logical word-boundary editing rules. Native candidate-window bounds
come from the same caret rectangle as the painted cursor.

Rich text resolves bidi for the complete paragraph before splitting its visual
runs into fonts, styles and links. Adjacent spans selecting the same font use one
complete shaping buffer. Each style paints the shared whole-run mask through its
source-grapheme coverage clips, so GSUB ligatures and GPOS positioning survive
color/link boundaries. A style starting inside a grapheme does not split that
grapheme; its first scalar owns its coverage. Links keep their own disjoint hit
regions and underlines. ASCII style-boundary geometry remains unchanged.
Actual positioned glyph extents can expand a row's ascent and descent, so stacked
marks share a baseline without clipping. Ordinary ASCII retains its prior metric
envelope. An oversized textarea grapheme keeps its complete canonical shaping
buffer while `LayoutOptions::raster_width` bounds only the generated mask's
coverage. Culling does not slice strings or change the editing source map.

`LayoutOptions` exposes only implemented controls: paragraph direction, shared
line metrics and optional raster coverage width. Joining context is borrowed
internally from the complete source line when shaping its font spans; consumers
do not assemble context fragments or maintain a second shaping state.

ASCII preserves the prior character advances and pair kerning exactly. Density
changes do not reflow text or change logical positions. Within each explicit
monotone script/font run, cluster-end lookup uses a binary search and adjacent
same-cluster glyphs merge directly. A test-only slow collector of raw rustybuzz
output checks exact glyph positions, source stops, selection segments and ink
metrics on long CJK, combining-Latin, Devanagari, Arabic and Hebrew rows. This
removes two quadratic scans without introducing a retained cache. It is a source
complexity correction; runtime latency remains a separate measured result.
The native renderer keeps
its existing size, glyph and mask allocation checks and coverage supersampling.

See [the baseline review](text-baseline-review.md) for the row-local typography
changes, unchanged visual tolerances and source-specific acceptance boundaries.

## Bounded fallback

Font fallback is deterministic and does not scan the machine during layout.
The primary application face wins whenever it covers a whole grapheme. Otherwise
the framework tries its bundled Noto faces in a fixed order. Missing unsupported
characters remain the primary face's missing glyph; they are not silently
removed. Bundled fonts and their separate license files live in
`lucent/crates/lucent-ui/assets`.
The test-only unmodified Hind Regular face covers Latin and Devanagari in one
primary font; it proves that fallback face changes cannot hide a missing script
boundary. `assets/test-fonts.json` records its official source, SHA256 and separate
OFL license. It is not part of the production fallback set.

The fallback set includes Arabic, Devanagari, Hebrew, common Han ideographs and
Extension A, Kana/Bopomofo, Hangul syllables/Jamo, Greek/Cyrillic, Thai, Bengali
and Tamil. CJK and Hangul are static
400-weight subsets derived from the official Google Fonts Noto sources, with
renamed derivative family names. The asset manifest records source and derived
SHA256 values, Unicode ranges, codepoint counts and individual OFL license files.
The CJK subset contains 28,323 codepoints in 10,167,004 bytes; Hangul contains
11,623 in 2,697,016 bytes. Coverage and shaping tables borrow immutable embedded
bytes. Fallback selection does not construct a fontdue rasterizer or copy those
large source files into an Arc. The renderer reads only each requested glyph's
bounds and outline through ttf-parser, then prepares an antialiased tiny-skia mask
at the existing 2× physical coverage density. The ordinary primary-face fontdue
path remains unchanged.

Glyph dimensions, finite bearings and pixel count are validated before coverage
allocation. One temporary outline has a bounded command count; masks are released
after composing the string. No additional retained per-glyph map exists: the
renderer's existing byte-budgeted text texture cache remains the reuse boundary.
This closes the source-level whole-font outline allocation path. Actual cold/warm
RSS, performance and 1×/2× raster acceptance still require recorded execution;
source inspection alone is not a measured memory improvement.

## Input-method ownership

The optional native `zwp_text_input_v3` adapter binds one text-input object for
the selected keyboard seat. It enables input only when both native keyboard
focus and text-input `enter` identify the exact current surface, and the freshly
resolved focused element is an ordinary controlled field. A surface role alone
never grants authority. Focus loss, hidden/recreated controls and seat retirement
remove authority. Applications explicitly opt in with
`Application::input_method_enabled()`. Its default is `false`; the desktop, menu
and ordinary text examples opt in. Login, lock and authorization clients retain
the default and do not bind the manager.

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
changes retire stale composition. A `done` batch with an older serial still
applies to the current field, as required by text-input-v3. It defers outgoing
state requests until a matching `done` arrives; focus/control retirement still
revokes that field's authority immediately. Delete and commit are validated as a
single batch before component admission, including UTF-8 and grapheme boundaries.

## Independent consumer and verification

```sh
cargo run --manifest-path lucent/Cargo.toml --locked -p international-text
```

This example imports the API, design system and Wayland runtime only. It presents
mixed Arabic, Hebrew, Devanagari and CJK text in single/multiline fields plus a
count-only masked probe. It uses the same controlled API as the desktop and opts
in once through the application capability; individual fields need no IME
registration or renderer dependency. `vm/test-international-text.py` exercises
the example with a controlled, non-replacing native input-method peer. It refuses
an existing input-method owner and requires exact deployed binary hashes and an
idle unlocked guest before sending native input.

The initial source cohort compiled the core crates and executed targeted API,
Wayland and raster tests. It exposed incorrect test assumptions about Arabic
glyph counts and bidi caret affinity, and a fixture assertion that read the
intentionally empty legacy hit value. Subsequent source changes correct those
tests, preserve visual affinity and add paragraph, metrics, atomic-edit and native
protocol regression cases. This final cohort still requires its own compile,
test and 1×/2× visual runs. Remote CI for `0bfdd8a0f69` passed strict workspace
Clippy, then ran 94 UI tests successfully and exposed two failures: independently
shaped Arabic styles and oversized-grapheme culling. The shared-mask and bounded
coverage corrections above follow those failures; their new cohort has not yet
been verified. No native compositor IME interoperability or memory
acceptance is claimed by the presence of the harness or these source cases.
Accessibility bridging remains explicitly deferred by the user.


## Resource observation in this source cohort

Fallback glyph preparation remains on demand, with borrowed immutable tables and
one bounded outline at a time. The combined source cohort adds the isolated
`font_resources::cold_and_warm_fallback_preparation_resources` probe and opt-in
compositor presentation feedback. These observe preparation and actual compositor
reports separately from CPU submission counters. Bounded stream identity/revision
traces are associated with those exact presented submissions; the client retains
separate bounded media-PTS associations. These distinguish repeated references
from distinct prepared-frame advancement, with no pixel-visibility claim. None
of this changes text density,
frame demand or the domain/component contracts. See [daily-performance.md](daily-performance.md)
for the matched static/video workloads and protocol-flag limits. Compilation and
measured resource/presentation acceptance for this combined cohort remain pending.
