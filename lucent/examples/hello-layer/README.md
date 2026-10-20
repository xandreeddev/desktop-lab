# Native examples

`hello-layer` demonstrates interactive controls and finite token-driven motion.

`frame-pacing` is a separate, passive diagnostic using the same public framework
API. It fills one explicitly named output with an opaque semantic-token backdrop
and a moving rounded rectangle. Geometry reads `ViewContext.now`; returning true
from `Application::animating` requests the next compositor-available frame. It has
no video, images, text, worker, subscription, timer or fixed-FPS control. Marker
dimensions and its four-second path are diagnostic policy; colors and corners
use the design system. Its socket/instance name is `lucent-frame-pacing`.

Build and test explicitly:

```sh
cargo build --manifest-path lucent/Cargo.toml --locked -p hello-layer --bin frame-pacing
cargo test --manifest-path lucent/Cargo.toml --locked -p hello-layer --bin frame-pacing
```

Run `frame-pacing --output NAME` in an ordinary Wayland session. A missing named
output waits rather than selecting another display. For a video comparison, the
measurement owner must verify the same 1920×1080, scale-1, 60 Hz output, adapter,
presentation mode and host conditions, keep the diagnostic visible, and retire
the video workload before measuring it. Do not layer it behind a live desktop
background and treat an occluded result as equivalent. Runtime duration and
cleanup belong to the caller's bounded test unit; there is no internal timer.

The existing `LUCENT_PROFILE_PRESENTATION=1` runtime option exposes per-surface
`wp_presentation` timestamps/refresh/flags through this client's own inspect
socket. `LUCENT_PROFILE_FRAMES=1` is a separate CPU-stage diagnostic and should
remain off for a matched presentation-only comparison. Histories are bounded,
so collect them before they roll over. Stream associations are empty for this
geometry-only client; the desktop's video-producer counter guard does not apply.
An observed rate characterizes the entire runtime/WSI/compositor path under the
recorded conditions. It does not prove a VM refresh cap, pixel visibility,
hardware scanout or native 60 FPS by itself.
