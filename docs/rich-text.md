# Inline text: content, interaction and painting

Accepted main includes Text `8e6d074bfcb30a0bcc0b371afddcd50c08993af0`, merged
through [PR87](https://github.com/xandreeddev/desktop-lab/pull/87), including the
soft-wrap bidi correction. Its [exact-source CI](https://github.com/xandreeddev/desktop-lab/actions/runs/39313647281)
passes the workspace, six controlled service-protocol filters and all 832 Vulkan
desktop cases. The coherent `8e` release was deployed. Its later
[owned mapped-peer IME run](../reports/measurements/lucent-international-text.json)
passes five feature and five restoration checks; a separate
[Chromium–Notes clipboard run](../reports/measurements/lucent-native-gui-clipboard-2044.json)
passes nine interaction and twelve restoration checks on unchanged `8e`.

These results cover identified sources and one virtual output, with independently
recorded test peers and harnesses. They are not a current VM observation. Third-party
IMEs, other clipboard applications, delayed transfer/focus races and physical or
cross-output input remain open. Earlier failed runs and recoveries keep their
original results. [Current readiness](current-readiness.md) separates those scopes.

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

The layout engine groups words across span boundaries and uses the shared text
shaping path. ASCII retains its established advances and pair kerning; complex
scripts use paragraph bidi resolution and script/font shaping across style and
link boundaries. Oversized words break at grapheme boundaries. Lines share a
baseline across faces, retain explicit newlines and respect a maximum line count
with ellipsis.

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
keep the existing text control and layout. A link captures the current content
revision, requests fresh user activation, then validates that version and URI
again before queuing an opening. Replacement, clear and a newer intent retire
the pending opening and cancel work still waiting on the controls queue.

`ExternalUri` is a validated domain value for http, https, mailto and local file
targets. `UriPort::open(target, context)` is the domain contract;
`applications::XdgUri` is the production adapter. It sends a literal target to
`xdg-open` in an independent systemd application unit with the explicit launch
context. An unknown default handler requests an SDK token without a target app
ID; it does not pretend that `xdg-open` is the destination application. No shell
evaluates notification text, and absent authority never reuses inherited tokens.
A saved link opens its content; it never invokes the retired sender's notification
action. [Default URI activation](default-uri-activation.md) follows this path and
its validation boundaries.

Images use separate parsed blocks and the presentation asset port. The
[image guide](notification-images.md) follows preparation, bounds, scrolling and
reveal motion. The server advertises `body`, `body-markup`, `body-hyperlinks`,
`body-images`, `icon-static` and `actions`; unsupported remote images retain
alternative text. The subset follows the
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
focus at 1x/2x with both palettes. These checks alone do not prove native link
input. Notification links and default-handler input have separate
[source-specific evidence](default-uri-activation.md); the standalone rich-text
example's interaction is not established by those client tests.

The accepted Text cohort implements whole-paragraph bidi and script/font shaping
with shared style masks and bounded fallback. [International text](text-shaping.md)
records its five feature and five restoration checks with an owned mapped peer.
That native example evidence does not establish every rich-text client, third-party
IMEs, all scripts/layouts or physical/cross-output behavior. Accessibility remains
user-deferred.
