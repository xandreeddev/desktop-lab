# Declarative text editing

Ordinary text fields use a shared framework API rather than editing the last
painted string in the input dispatcher. The component owns its value and a
`TextFieldState` (which contains selection, bounded undo/redo history and one
clipboard ticket). The framework emits `TextEdit` messages and renders the next
declared value/selection.

```rust
Element::text_input(&self.value, self.field.selection(), "Search", Message::Edit)
    .id("search")
    .autofocus()
```

In the component's update, apply the message to the **current** value:

```rust
let change = self.field.update(
    &mut self.value, edit, 8192,
    ClipboardTarget::new("panel", "search"), effects, Message::Edit,
)?;
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
Ctrl+Z undoes; Ctrl+Shift+Z or Ctrl+Y redoes. The callback emits relative
`TextEdit::Undo`/`Redo` messages, and `TextFieldState::update` applies them against
the current component value. `TextSelection::apply` alone owns no history.

Native key translation keeps shortcut modifiers separate from printable text.
Ctrl+A and Ctrl+U select/clear with either letter case, including Caps Lock or
Shift; adding Alt or Super never turns those chords into an edit. Ctrl+Shift
clipboard/history shortcuts retain Shift while normalizing the control letter.
Unconsumed Super+letters remain shortcuts instead of inserting their UTF-8 text.
Ordinary and masked controls consume the same translated SDK keys; masked
controls retain count-only edits without clipboard or undo authority. Composed
UTF-8 text from the existing keyboard adapter keeps its original contents.
This does not implement compositor IME/preedit or complex-script shaping.

Extended graphemes use the maintained
[Unicode segmentation implementation](https://docs.rs/unicode-segmentation/latest/unicode_segmentation/trait.UnicodeSegmentation.html).
This improves editing of combining characters and emoji sequences. It does not
establish complex-script shaping, bidi visual navigation or font fallback.

Relative edit messages are essential: several native key events can arrive
before a compositor frame. Each callback returns an intent such as Insert or
Move, and the UI thread applies it to the latest component state. An old scene
cannot overwrite the newly edited string with its prior value.

## Undo and redo

Each ordinary field retains at most 128 edits and 64 KiB of removed/inserted
UTF-8 bytes across undo and redo combined. History stores replacement spans and
the original directional selections. One comparison snapshot is bounded at
64 KiB, for at most 128 KiB of retained text per field for history and exact
external-change detection. The canonical value remains component-owned.
Adjacent insertions at a collapsed caret coalesce into a group up to 4 KiB.
Navigation, pointer selection, clipboard actions, cancellation and history
playback break the group. Deletion, replacement, clear, completed paste and
completed cut form individual edits.

An accepted content edit retires redo. Rejected edits and identical content
preserve it. Undo and redo obey the current client byte admission limit; a
rejected restoration keeps its history entry for a later attempt. Existing
oversized values remain deletable. If an accepted edit needs more than the
history byte budget, it clears earlier history and completes without retaining
that edit. Oldest undo entries are evicted as count or payload limits are reached.

`reset` clears both histories and pending clipboard authority. `cancel` retires
clipboard authority and ends a typing group while preserving accepted history.
Externally replaced text retires history on the next update, using exact
snapshot comparison; playback also checks the current span before mutation.
History clears and remains disabled while the value exceeds the 64 KiB snapshot
limit; edits remain admitted by the client's normal byte limit. Returning to a
bounded value enables history for later edits. Call `reset` for an intentional
component/aggregate replacement, including replacement with the same text.
Undo/redo retire pending paste/cut tickets, so delayed replies cannot mutate the
restored value. History debug output never exposes retained text.

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
changes. Wallpaper search and hidden network names use the same relative-edit
control. Wallpaper typing never starts provider I/O until Search/Enter is
submitted; submitting or changing provider retires earlier paste tickets. The
network child checks the current form generation and admits at most 32 UTF-8
bytes before creating a hidden-network SSID. Editing the name never reads or
changes the form's zeroizing password, and stale replies cannot cross forms.
The widget host owns notes selection while borrowing the root settings
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

## Clipboard: intent, transfer, completion

Opt in with `Application::clipboard_enabled() -> true`. It defaults to false;
secure-session runtimes additionally refuse clipboard binding. Ordinary field
clients handle Ctrl+C/X/V through the same `TextEdit` callback as navigation.
`TextFieldState::update` queues a `ClipboardRequest` in `Effects`, addressed to
the current surface and element. `Effects::delegate` maps completion messages
with the child while preserving that target. This is presentation/runtime API;
clipboard transports do not belong in the pure desktop domain or renderer.

The component keeps its one canonical `String`. A pending operation retains a
bounded copy and selection so a late completion cannot paste into newer text or
cut a newer selection. Every subsequent edit/reset retires that ticket. Replies
from older tickets do not consume the current one. Selection-only changes and
copy requests do not refilter or save. Accepted paste/cut content follows the
same child intent and root persistence path as typed text.

The runtime binds SCTK's core Wayland data-device manager for the selected
keyboard seat. Copy/cut require a real native key/button serial, not a CLI
command or fabricated serial. The current keyboard surface, focused ordinary
field, live surface object and focus epoch must match. Completion checks a fresh
component view, not just the last painted scene. Losing keyboard/field focus,
seat removal and surface replacement retire stale requests. Masked controls
never pass the ordinary-field check and expose no clipboard action. Lock,
greeter and permission clients do not opt in.

Copy creates one source advertising UTF-8 text MIME types. The source owns the
selected bytes until replaced/cancelled or the runtime stops; it serves native
send requests. An already accepted send retains its bounded bytes until completion
or deadline, so replacing selection does not truncate an in-flight transfer. `WritePosted` means the request was queued using an actual input
serial. It is not an ownership acknowledgement: the
[core Wayland protocol](https://wayland.freedesktop.org/docs/html/apa.html#protocol-spec-wl_data_device)
provides none. Cut deletes only after this posted result and only while its
snapshot remains current. Clipboard persistence after client exit is not
promised; that requires a separate compositor/clipboard-manager policy.

Paste negotiates `text/plain;charset=utf-8`, `UTF8_STRING`, then `text/plain`.
The receiver requires valid UTF-8 at EOF. Unsupported formats, overflow, timeout
and disconnected pipes return typed generic errors; they never print clipboard
text or execute it as a shell command. Incoming CR/LF is removed by single-line
editing. Multiline fields normalize accepted CRLF/CR edits to LF and preserve
hard lines through the same history/clipboard admission path. Launchers and menus show failures without changing their query; notes
returns the existing typed error intent. Wallpaper search and hidden-network
name errors use their existing status/failure presentation. Clipboard history
and primary-selection UI remain separate features.

All pipe I/O lives on the native calloop poller with nonblocking descriptors.
There are at most eight active transfers, each limited to 64 KiB and an absolute
two-second deadline. One callback performs at most eight 4 KiB operations before
yielding. Partial UTF-8 waits for EOF; a slow peer cannot extend the deadline.
One timer exists while transfers may be pending and drops when they finish.
No clipboard worker, continuously ticking redraw or subprocess is started.
Source and pipe teardown precedes the existing protocol cleanup flush.

Run the independent example and test against a normal Wayland application:

1. Select a word by Shift+arrows or pointer dragging; press Ctrl+C.
2. Paste in an external application, then copy different text there.
3. Focus the example field and press Ctrl+V. Ctrl+X should remove the selected
   text only when its selection request is posted.
4. While a delayed transfer is pending, edit, close or change focus. The old
   reply must not change a newer field.

Those native interoperability steps remain deferred. The current automated
checks use pure client/model cases and owned local nonblocking pipes; they do
not replace real compositor serial/selection acceptance.

## Acceptance still open

`vm/test-input-settings.py` supplies a focused real-keyboard check for the
prepared VM: Ctrl+A/U with Shift/Caps and unconsumed Alt/Super combinations.
It checks keyboard/binding prerequisites, restores the original launcher query
and open state, and verifies unchanged settings and wallpaper. Running it
requires an unlocked guest; its existence does not establish native acceptance.

```sh
python3 vm/test-input-settings.py --skip-bound-chords --skip-remapped-caps
```

These explicit flags preserve the prepared guest's bindings and Caps-key mapping.
Conflicting chords and Caps-dependent cases are listed as skipped; a passing
supported subset is not full modifier acceptance. Caps Lock must already be off
when its physical key is remapped. The helper waits for stable native field/layer
geometry and real pointer arrival, then restores the original query/open/Caps
state and verifies unchanged settings and wallpaper.

The latest supported-case run against deployed cohort `76176f83b19` stopped at
`ordinary_launcher_field_focused`, with all recorded restoration checks passing.
The launcher incorrectly marked ordinary `text_edit` fields as non-focusable;
the admission milestone fixes that guard and adds an engine-pointer regression.
Corrected source `d2aed42199e` now passes the supported native subset with all
restoration checks; see
[the source-specific report](../reports/measurements/lucent-media-admission.json).
The explicit Caps/binding skips, clipboard interoperability and physical
acceptance remain open.

The malformed/oversized stored-notes native restart check remains pending.
Failed settings load can apply the default theme, so that check needs exact
theme/login export restoration as well as the original JSON file.

Native clipboard interoperability and hardware acceptance remain open. Current
shaping remains the existing kerning-based implementation. Clipboard native
interoperability, IME, bidi/complex-script shaping, font fallback and platform accessibility remain separate requirements.
The controlled multiline editor and Notes client are described in
[multiline editing](multiline-text.md); their integrated and native acceptance
remain separately recorded. Editing tests and
synthetic Vulkan references cannot prove those features.
