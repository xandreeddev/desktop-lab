# Native framework, proved with a layer-surface prototype

Status: accepted for Milestone 1, not a stabilized framework API.

The first deliverable is a real Wayland surface rendered through wgpu, responding to pointer input and closing cleanly. It runs alongside the stock Omarchy shell. The remaining desktop features in the original master plan are a roadmap, not implemented capabilities.

| Choice | Benefit | Cost | Decision |
| --- | --- | --- | --- |
| SCTK + wgpu directly | Own lifecycle, scheduling and replaceable desktop client | Services and higher UI abstractions require implementation | Selected |
| Wrap Amane | Existing widgets and services accelerate a complete shell | Experimental API and service ownership constrain the public framework boundary | Useful reference; not a dependency |
| Fork Amane | Maximum immediate reuse | Own a substantial fork and its upstream merge work | Rejected for the initial slice |

Sources were inspected at the revisions in `manifests/upstream-lock.json`: [Amane architecture](https://github.com/MystiaFin/amane/blob/main/ARCHITECTURE.md), [Suzuha](https://github.com/MystiaFin/suzuha), and the [official SCTK simple_layer example](https://github.com/Smithay/client-toolkit/blob/v0.21.1/examples/simple_layer.rs). We borrowed architectural ideas, not either complete project. SCTK's registry, seat and layer-handler contracts necessarily follow its public API.

The implemented separation is `lucent-domain` (pure counter and frame-demand model), `lucent-render` (wgpu resources and prototype text rasterization), `lucent-wayland` (native connection/surface/input ownership) and `lucent-desktop` (the replaceable example client). `hello-layer` demonstrates a second client. Crates for unimplemented services, IPC, configuration and widgets will be added when their first vertical slice exists; empty crates would imply capabilities we do not yet have.

One compositor frame callback gates outstanding work. Input marks a surface dirty. A callback with no new work does not request another callback or render. The GPU surface is dropped before its native wl_surface and display. Failed surface presentation exits with an error; device-loss recovery is future work.

The GPU instance owns a clone of the Wayland backend through its `HasDisplayHandle` implementation. Supplying this display at instance creation is required for EGL presentation. Alpha mode is selected from advertised surface capabilities: premultiplied when available, otherwise an explicit opaque canvas. The virgl guest exercises the latter path; transparent corners on a Vulkan driver are not yet validated.

The prototype rasterizes simple Latin text and rounded cards into a cached CPU image, uploads it only for a dirty frame, and presents it with wgpu. This proves GPU/native-surface integration, not the final rendering architecture. Vello should be evaluated for vector scenes, gradients and clipping after this prototype is tested. Full shaping, a glyph atlas, fractional scaling, multi-output widgets and GPU allocation accounting remain future work. Do not describe the current renderer as a finished UI framework.

Next: introduce a minimal component/view/message API around implemented row, text and button use cases. Then add an event-driven Hyprland workspace adapter and one working top bar before expanding the domain models. Secure locking remains delegated to a proven locker.
