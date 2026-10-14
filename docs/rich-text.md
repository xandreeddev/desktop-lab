# Inline text: content, interaction and painting

`Element::rich_text(Vec<TextSpan>)` is a framework control. It knows registered
font indices, colors, underline and opaque action identities. It does not parse
HTML, interpret a URL, load an image or open an application.

```rust
Element::rich_text(vec![
    TextSpan::new("Read "),
    TextSpan::new("the details")
        .face(bold_face)
        .color(theme.info)
        .underline(true)
        .link("details"),
])
.id("body")
.on_link(|id| Message::Selected(id.to_owned()))
.width(Length::Fill)
.wrap(8)
```

Clients register font bytes through `Application::fonts`, then select a face by
index. Missing indices use the first registered face. The ordinary `Style`
font/color apply to spans without overrides. `Theme::apply` supplies generated
underline, hover and focus tokens. The engine has no bold/italic naming policy.

## Layout and input

The layout engine groups words across span boundaries and wraps using the same
fontdue advances and pair kerning as plain text. Oversized words break at
grapheme boundaries. Lines share a baseline across faces, retain explicit
newlines and respect a maximum line count with ellipsis.

Each displayed line/run becomes ordinary `Paint::Text`; underlines and focus
use ordinary shapes/outlines. A long body is split into line primitives, so its
total height does not depend on the renderer's 1024-pixel text-mask height limit.
The renderer's cache still bounds unused resources. This is inline flow, not a
virtualized document or continuous repaint loop.

One link identity creates one `Hit`, even when wrapping or changing font face
creates several fragments. `Hit.parts` defines its disjoint pointer coverage;
its bounding rectangle supports normal keyboard/scroll reveal. Native input
regions use the individual fragments and the same rounded ancestor clips.
Tab/Shift+Tab traverse logical links; Enter and clicking a fragment dispatch the
typed callback. `Element::map` preserves the callback when composing children.
Changing fragment geometry/clip retires a pending click capture.

The renderer keeps advance width separate from ink bounds. Italic glyphs can
overhang their advance; coverage masks include that ink and carry their left
bearing back into GPU placement. Text still rasterizes at twice physical output
density and resolves once into a single-channel coverage texture.

## Notification client

`lucent-usecases::notifications::body` parses the notification XML subset into
pure `NotificationBody`, `BodyBlock`, `BodySpan` and `BodyStyle` values. Inputs
are bounded to 4 KiB; nesting, tag count and image references have limits.
Escaped and numeric entities become text. Malformed nesting falls back to
literal text without external authority. Unsupported tags have no behavior.

`notifications::Center` prepares these values when receiving snapshots, retains
them by exact `NotificationTarget` and supplies a declarative view. That view
selects genuine bold/italic static faces and semantic link colors. Plain bodies
keep the existing text control and layout. The client validates a link against
the retained content revision before queuing an effect.

`ExternalUri` is a validated domain value for http, https, mailto and local file
targets. `UriPort::open` is the domain contract; `applications::XdgUri` is the
production adapter. It sends a literal target to `xdg-open` in an independent
systemd application unit. No shell evaluates notification text. A saved link
opens its content; it never invokes the retired sender's notification action.

Images currently retain alternative text. Body images/icons and native image
hint preparation remain a separate incomplete feature; `body-images` is not
advertised. The server advertises `body`, `body-markup`, `body-hyperlinks` and
`actions`. The supported subset and optional image behavior follow the
[freedesktop notification specification](https://specifications.freedesktop.org/notification/latest-single/).

## Independent example and evidence

```sh
cargo run --manifest-path lucent/Cargo.toml --locked -p rich-text
```

This consumer depends on the framework and optional design system. Selecting a
link changes local example state; it performs no desktop or domain operation.
Its `--help` path runs without Wayland. Native interaction requires a Wayland
layer-shell session and Vulkan.

Behavior checks cover style-spanning words, grapheme wrapping, line limits,
per-line paint, mapped callbacks, one keyboard target, disjoint pointer coverage
and capture retirement. Pure/client tests cover parser limits, URI policy,
literal arguments, current-version actions and saved-content links. Render
references exercise bold/italic baselines, clipped links and visible keyboard
focus at 1x/2x with both palettes. Real compositor input remains deferred;
complex-script shaping, bidi, fallback, IME and accessibility remain open.
