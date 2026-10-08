# ADR 0002: Declarative framework with a separate desktop client

Status: implemented vertical slice. Supersedes the card-only client in ADR 0001.

The draggable Vulkan prototype established native layer surfaces and compositor
frame scheduling. The next requirement is a reusable API plus a working desktop,
with Lucid's launcher and selector as the interaction reference.

We keep the independent SCTK + wgpu stack. `lucent-api` declares components,
elements, effects, subscriptions, surfaces and motion. `lucent-ui` handles layout
and input; `lucent-render` consumes paint primitives; `lucent-wayland` owns native
handles and execution. These crates have no dependency on desktop domain types.
`lucent-domain` and `lucent-usecases` describe desktop behavior independently;
`lucent-services` implements the actual OS boundary. Client modules compose them.

The primary animation is one persistent rounded dock container changing geometry
into the launcher or wallpaper carousel. It uses shared interruptible timelines
and callback-driven rendering. Stable widget IDs make layout persistence
independent of rendering. The separate counter client exercises child-component
message mapping and effect delegation without importing any desktop design code.

We use typed CLI adapters where they provide a working vertical slice now:
WirePlumber, playerctl, NetworkManager and Omarchy. They execute on background
workers with bounds and deadlines. Hyprland workspace/window changes use its
native event socket. Native D-Bus adapters can replace polling later. A complete
notifications server, tray host or secure locker is not implied by this milestone.

The framework deliberately omits speculative plugin ABI, compositor abstraction
inside visual components, dynamic scripting, browser views and authentication
code. Multi-output and fractional-scale support need further native integration
work. The test VM's Vulkan driver is software; its timings do not establish
hardware GPU performance or parity with Lucid.
