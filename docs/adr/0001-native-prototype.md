# Native framework, proved with a layer-surface prototype

Status: accepted for Milestone 1, not a stabilized framework API.

The first deliverable is a real Wayland surface rendered through wgpu, responding to pointer input and closing cleanly. It runs alongside the stock Omarchy shell. The remaining desktop features in the original master plan are a roadmap, not implemented capabilities.

| Choice | Benefit | Cost | Decision |
| --- | --- | --- | --- |
| SCTK + wgpu directly | Own lifecycle, scheduling and replaceable desktop client | Services and higher UI abstractions require implementation | Selected |
| Wrap Amane | Existing widgets and services accelerate a complete shell | Experimental API and service ownership constrain the public framework boundary | Useful reference; not a dependency |
| Fork Amane | Maximum immediate reuse | Own a substantial fork and its upstream merge work | Rejected for the initial slice |

Sources were inspected at the revisions in `manifests/upstream-lock.json`: [Amane architecture](https://github.com/MystiaFin/amane/blob/main/ARCHITECTURE.md), [Suzuha](https://github.com/MystiaFin/suzuha), and the [official SCTK simple_layer example](https://github.com/Smithay/client-toolkit/blob/v0.21.1/examples/simple_layer.rs). We borrowed architectural ideas, not either complete project. SCTK's registry, seat and layer-handler contracts necessarily follow its public API.

The implemented separation is `lucent-domain` (pure drag, animation, geometry and frame-demand models), `lucent-render` (wgpu resources and prototype text rasterization), `lucent-wayland` (native connection/surface/input ownership) and `lucent-desktop` (the replaceable example client). `hello-layer` demonstrates a second client. Crates for unimplemented services, IPC, configuration and widgets will be added when their first vertical slice exists; empty crates would imply capabilities we do not yet have.

One compositor frame callback gates outstanding work. Input marks a surface dirty, and active finite animation timelines request follow-up frames. A final settled frame ends the chain. With no dirty state the event loop blocks. The GPU surface is dropped before its native wl_surface and display. Failed presentation exits with an error; device-loss recovery is future work.

The widget host is a transparent output-sized overlay surface with a rounded input region restricted to the visible card. Widget movement happens inside the stationary surface, using the compositor's implicit pointer grab while dragging. The domain tracks a drag threshold, clamping, saved position and interruptible cubic easing. The platform layer handles input, frame callbacks and position persistence; the renderer receives a visual snapshot.

Vulkan is now mandatory, with premultiplied alpha. The previous OpenGL/opaque fallback was removed. The prepared VM validates software Vulkan via lavapipe, because the Venus renderer failed under QEMU's process-spawning sandbox. The original VM graphics definition was restored; see [the graphics report](../lucent-vulkan.md). A Vulkan backend is not by itself evidence of hardware rendering.

The Vulkan shader draws the rounded card, border, shadow, gradient and animated transforms. Only changed text is rasterized/uploaded to its cached texture. A six-vertex quad bounds fragment work to the card and shadow. Text currently uses fontdue without full shaping. Vello, a glyph atlas, fractional scaling, per-output widget hosts, resizing and GPU memory accounting remain future work.

Next: introduce a minimal component/view/message API around implemented row, text and button use cases. Then add an event-driven Hyprland workspace adapter and one working top bar before expanding the domain models. Secure locking remains delegated to a proven locker.
