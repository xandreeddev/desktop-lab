# Anchored overlays

`AnchoredSpec` resolves an overlay beside an element in the same surface. Its
preferred side can flip when the opposite side has more space, and its rectangle
is clamped to the viewport inset. IDs identify presentation elements. They are
not application IDs, monitor identities or compositor-global coordinates.

The normal tree lays out first. Deferred overlay placement reads that frame's
actual anchor bounds and clip, then uses the same layout, paint and input engine.
Overlays do not occupy row or column flow space. Missing, duplicate or clipped
anchors cannot host a visible overlay. Actual rounded and diagonal clip coverage
uses the same logical pixel-center rule as native input. Nonfinite geometry is
rejected. Only requested anchors retain metadata. A fully hidden or unrevealed
owner cannot declare an active overlay. `input_only` keeps a popover's input
scope while suppressing all its paint; passive labels under it are omitted.

## Passive tooltips

```rust
theme.button("Open", Message::Open)
    .id("open")
    .tooltip(theme.tooltip("Open the selected application", OverlaySide::Above))
```

The client supplies content, side and design recipe. `Tooltip` carries a bounded
width, dwell delay and transition; the generic SDK has no Lucent colors or timing
policy. The Lucent recipe uses semantic colors, primary spacing/font/radius
tokens and component aliases for width, dwell and motion.
Nonfinite/negative dwell and duration, unbounded fade curves and invalid width or
placement policy are rejected before a native timer is registered. A zero-duration
transition displays immediately after dwell.
The width limit also constrains content that requests a larger fixed width;
its paint is clipped to the resolved hint rectangle.

Pointer dwell uses a single cancellable calloop deadline across current native
surfaces. Waiting requests no frames and creates no worker. At the deadline, the
still-current target is invalidated once; the short fade then follows normal
compositor frame callbacks. Keyboard focus displays the label immediately.
Pointer leave, a changed/retired anchor or Escape hides it. A press suppresses
the current pointer label until the target changes.

Tooltip content contributes paint only. Its input handlers, focus targets and
native regions are discarded, so a label cannot steal a click, clipboard focus
or keyboard focus. Tooltips remain bounded by their actual native surface.

The desktop uses this API for tray descriptions and closed-dock application
names. It leaves a small token-sized transparent band in those native buffers;
the bar still reserves exactly 48 logical pixels, and the dock retains its
display-grid position, original hide travel and short-buffer launcher handoff.
The band adds no input region. Active launcher/drag content has no dock labels.

## Explicit interactive popovers

```rust
root.popover(Popover {
    id: "choices".into(),
    anchor: "open".into(),
    placement: AnchoredSpec {
        side: OverlaySide::Below,
        align: OverlayAlign::Start,
        gap: component::popover::GAP,
        inset: component::popover::INSET,
    },
    content: Box::new(theme.popover(menu_content)),
    dismiss: Message::Close,
})
```

Text colors are node-local. Set a semantic foreground such as
`.color(theme.on_surface)` on plain text inside popover content; the wrapper
does not override a child's explicit color.

The component owns whether this value is present and handles its messages. The
engine supports one interactive popover per surface in this initial contract.
It moves focus into the scope, traps Tab/Shift+Tab there, offers Escape before
the application's global shortcut handler and dismisses on an outside press.
Other keys reach the scope's ordinary controls before application-global key
handlers too. This prevents an underlying launcher Tab/Enter route from taking
the popup's input. Native input refreshes current scope geometry on the CPU even
when the compositor has not allowed another GPU submission. Affected-surface
refresh also follows asynchronous/IPC updates and reconciliation before
clipboard requests or focus observation, so an old editor does not retain
clipboard authority after a scope opens.
The runtime shares `current_tree` and one optional `current_scene` CPU layout for
multiline editing, scope admission and clipboard authority. A post-reconciliation
caret-reveal/dismissal batch carries pending clipboard requests forward before
their admission. Native input has one refresh path; text and overlays are not
laid out separately. Clipboard transfer callbacks force that same path before
their epoch and exact native-surface checks. Eligibility comes from one unique
ordinary editor hit in the admitted scene, so passive tooltip fields, rejected
scopes and masked inputs have no clipboard authority.
The outside press cannot release an action underneath. Closing restores the
previous focus only if that element still exists and is reachable; retiring the
anchor cancels captured presses and emits the supplied dismissal. Moving/removing
a pressed content control retires its old capture; an unchanged control receives
its current callback. Initial focus selects an already reachable control, without
activating a scrolled-out item. IDs must be unique, including
the scope's synthetic backdrop/panel IDs. Invalid scopes are rejected.
Each distinct continuously rejected scope ID emits one dismissal. An ignored
dismissal cannot repeatedly trigger updates/frames. Removal or successful
admission ends that rejection lifetime; reintroducing the ID can emit again.
Clients must use distinct IDs for distinct scopes and validate captured client
lifetimes in messages.

Dismiss messages should capture the owning component's current lifetime when a
client can replace a scope while asynchronous operations are pending. Application
actions and their domain validation remain client/service responsibilities.
This API does not create an `xdg_popup`, authorize secure-session behavior or
promise nested menu navigation. Existing tray menus remain unchanged.

Run the independent consumer inside a Wayland session:

```sh
cargo run --manifest-path lucent/Cargo.toml --locked -p anchored-overlays
```

Its menu consumes generic framework controls; the example's own `Motion` supplies
the opening fade. The renderer does not invent animation timing. The example's
global Escape exits only after the popover has had the chance to dismiss.

## Validation scope

Strict checks at `8b2956ed` pass 717 regular Rust tests, with 76 opt-in fixtures
ignored, 103 Python checks, formatting, strict all-target Clippy, 573 generated
design tokens and the independent example's help command. Cases cover placement,
invalid/duplicate/clipped anchors, dwell/focus, passive input, scope restoration,
outside-press capture and synthetic-ID collisions. Fractional bounds, retired
hover deadlines, scoped wheel input, multiline focus navigation and capped-width
wrapping have regression coverage.

Source `28bdf070` passes all 712 software-Vulkan visual comparisons and the
existing `compact_dock_pixels` regression. The 24 new references cover six
regular/narrow tray, dock and popover scenarios in both palettes at 1×/2×.
All were reviewed; existing references and tolerances are unchanged. The final
comparison ran without updating references. The original tray hover fixture
continues to cover its pre-tooltip state.

The release desktop and CLI from `28bdf070` were deployed to the existing VM.
Running-binary identity, readiness and 36.531 seconds of stable supervision pass.
The existing supervisor and effective `KillMode=mixed` remain unchanged; settings
bytes and selected wallpaper are preserved without preference restoration.

A separate pointer-only native check passes on one 1920 × 1080 output at 1× in
its current palette. The bar reserves 48 logical pixels while its buffer is
144 pixels high. Bar and dock hint hits/focus remain unchanged, pointer leave
retires each label, and the dock's empty hint band causes native pointer leave.
Each settled bar/dock sample submits zero additional frames over two seconds.
Exact cursor/dock state, settings bytes/inode, wallpaper/source/theme artifacts,
empty clipboard/history, monitor/workspace and the running process are preserved
during that workload. The acceptance helper and its reused Notes helper have
separate hashes in the [sanitized report](../reports/measurements/lucent-anchored-overlays.json).

Four private native captures were reviewed: both labels are fully visible with
readable contrast, clear spacing and rounded shapes, and absent after leaving.
These are static observations. RPC sampling missed the early dwell window and
visible-scene frame progression, so it does not establish dwell/fade duration,
animation smoothness or scanout. The configured 0.5-second dwell and 0.16-second
enter transition remain source values rather than measured timings.

Native generic-popover focus, keyboard, dismissal and clipboard acceptance remain
pending. Actual bar hint-band click-through is also pending because the inspector
does not expose native input regions. Current RSS/CPU, sustained pacing, hardware,
fractional/multi-output behavior and shaping/bidi/fallback/IME remain separate
work; accessibility stays deferred. Astro checks at `d3f9b539` pass with zero
diagnostics; both root and GitHub Pages builds pass 13 pages and 489 links/assets.
PR #77 merged normally as `f57d709c` after all six final-head checks passed at
`d239eb77` with a clean merge state. GitHub Pages run `39222213152` succeeds at
that merge; the public library page returns HTTP 200. The current main docs also
serve through the existing LAN preview service on port 4321. These delivery
receipts do not extend the native, timing or resource acceptance above.
