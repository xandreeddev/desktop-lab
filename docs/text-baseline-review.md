# Reviewing international text baselines

The international text cohort changes OpenType positioning for rows that contain
Unicode punctuation or combining marks. It does not change font-size tokens,
coverage density, the 2× physical text supersample, surface geometry or visual
test tolerances. Pure ASCII rows retain the historical advance and pair-kerning
path. A row containing an ellipsis or middle dot now uses the font's complete
OpenType shaping rules, including the Latin text around that punctuation.

The first complete visual report inspected here is CI run
[39295441194](https://github.com/xandreeddev/desktop-lab/actions/runs/39295441194)
at `ce2d4030fac269758bc2c33d62ba16acb9a87b79`. Its strict workspace checks and
ordinary tests passed; 407 of 796 desktop snapshot cases exceeded the existing
visual tolerances. That failure is evidence of changes requiring review, not a
passing desktop visual result. Its original report and diagnostic crops remain
private local artifacts. Baseline replacement must use the final exact-source
artifact and an explicit reviewed case list, retaining passing baselines.

## Reviewed changes by component family

Expected, actual and thresholded-difference crops were compared at both 1× and
2×. Representative light variants were also compared for text, controls,
notifications, themes, weather and menus. The following groups explain the
changes; a group is not permission to accept unrelated geometry changes.

| Case family | Why the text changes | Representative reviewed case |
| --- | --- | --- |
| Application actions | Ellipsized long action labels use shaped advances; their existing clipping and selected-row surfaces stay fixed. | `application-actions-narrow` |
| Audio | Ellipsis status text and middle-dot device metadata use OpenType positioning. | `audio-loading` |
| Bluetooth | En-dash PIN guidance, ellipsis operation status and middle-dot metadata use shaping; masked bullets and caret keep their separate count-only path. | `bluetooth-pin` |
| Live wallpaper | Middle-dot target/rate and source labels, plus ellipsis progress labels, change row-local positioning. | `live-wallpaper-source-rate` |
| Media | Ellipsis loading text and middle-dot unavailable status use shaping. | `media-unavailable` |
| Menus | Middle-dot footer instructions and ellipsis placeholders change text positioning. | `menu-input` |
| Monitor widgets | The clock date contains a middle-dot separator. Other widget geometry stays fixed. | `monitor-widgets-left` |
| Network | Middle-dot remembered-network and Wi-Fi status, en-dash guidance and ellipsis requests use shaping. | `network-hidden` |
| Notes | Ellipsis placeholders and save/retry status use shaping. | `notes-area-empty` |
| Notifications | Middle-dot history counts/status use shaping; shared rich text keeps its own link coverage. | `notification-rich` |
| Panel host | Hosted audio/live-wallpaper/notification rows and the media status in the preserved bar use the same changed label paths. | `panel-host-launcher` |
| Power | Middle-dot battery/device summaries and ellipsis operation status use shaping. | `power-controls` |
| Text selection | The decomposed accent is composed/positioned as one grapheme; selection and caret use the same source stops. | `text-grapheme-selection` |
| Themes | The applied palette's middle-dot status changes positioning. Palette surfaces and icons stay fixed. | `themes` |
| Tray | Ellipsis player/tray loading status uses shaping. | `tray-bar-wide` |
| Wallpaper search | Middle-dot result metadata and multiplication-sign dimensions use shaping. | `wallpapers-online` |
| Weather | Degree signs, middle-dot forecast metadata, ellipsis progress and Unicode minus guidance use shaping. | `weather-ready` |
| Widget persistence | Ellipsis save/retry status uses shaping. | `widget-resize-saved` |

These representative text-ink envelopes were measured with the same threshold
and crop for each expected/actual pair. They distinguish changed kerning width
from a smaller font. Bounds are physical pixels within each diagnostic crop;
they do not substitute for the existing visual test.

| Row | 1× expected → actual | 2× expected → actual |
| --- | --- | --- |
| Audio loading | 216×14 → 216×14 | 433×27 → 433×27 |
| Live wallpaper target/rate | 290×14 → 287×14 | 580×27 → 573×27 |
| Notes placeholder | 101×12 → 100×12 | 206×25 → 203×25 |
| Combining-grapheme selection | 44×18 → 41×18 | 86×36 → 82×36 |

The text heights remain equal. Width differences follow actual glyph positioning
or composition. The rendered panel surfaces, icons, outlines and field bounds
remain in their previous places in these comparisons.

## Independent input checks

`unicode_label_shaping_keeps_input_hits_selection_and_painted_caret_together`
exercises ellipsis, middle-dot, degree and combining-accent values in controlled
single-line and multiline fields. Every visual stop must hit its canonical UTF-8
byte, full selection coverage must match the shaped advance, the painted caret
must equal the native candidate rectangle, and the declared font size remains
20. The existing tall-fallback-mark test covers expanded vertical metrics too.
The new punctuation regression is source code awaiting its own Rust execution;
this document does not mark it passed.

The renderer's Arabic style-mask test compares complete glyph alpha across style
boundaries at both densities; the alias test checks actual bound stream revisions
against rendered pixels. Their ignored filters are explicit CI steps, not tests
permanently omitted from the workflow.

## Acceptance boundaries

Keep the current channel threshold of 8, whole-image fraction of 0.001 and worst
tile fraction of 0.01. Do not weaken them or accept every generated file. Record
an explicit case ID, old/new PNG hashes, rendering source and family explanation
for every changed baseline. A mismatch outside those reviewed rows requires
another review. Newer accepted Wi-Fi, display and application features must retain
their behavior when branches combine; conflicting scene baselines require a new
combined-source render rather than replacement with an older feature snapshot.

The final API cleanup removes an unused public fragment-context concept. Joining
context is borrowed internally from each complete line. It does not introduce a
new spacing or typography policy. Actual guest IME, cold/warm font RSS, wallpaper
source cadence and physical display acceptance remain separate native gates.
