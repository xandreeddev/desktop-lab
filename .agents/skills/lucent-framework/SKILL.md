---
name: lucent-framework
description: Implement or review Lucent framework APIs, domain models, ports, adapters and rendering boundaries in desktop-lab. Use for library and architectural changes; desktop preferences and VM operations use lucent-desktop.
---

# Lucent framework

Read repository paths relative to its root. Start with the concrete client and follow its calls into the engine; the original project prompt contains proposed crates and features that do not exist.

- Public component/runtime contracts: `lucent/crates/lucent-api/src/lib.rs`.
- Engine: `lucent-ui` (layout/input), `lucent-wayland` (event loop/surfaces/workers), `lucent-render` (Vulkan resources/presentation).
- Domain contracts are **ports**; concrete or supplied implementations are **adapters**. The desktop composition root is `platform::desktop_adapters()` and its injected bundle is `DesktopAdapters`.
- Domain values and use cases cannot perform OS operations or expose renderer/transport types. Value objects should enforce a demonstrated invariant; public field structs and newtypes are not automatically validated. Keep screen/grid policy and design tokens in the client.
- Compositor adapters report actual state, including special workspaces. Shortcut slots and their ordering belong to the desktop workspace presentation policy. Shadows carry color/offset/blur through `Shadow` and `Paint::Shadow`; the design system supplies elevation tokens, while the renderer implements only the generic falloff.
- Construct application identities with `AppId::new`; persisted IDs use the same validation. Use `Application::matches_window_class` for dock state and activation rather than duplicating identity rules or matching empty hints. Valid settings keep their string ID representation; invalid settings must remain preserved on disk.
- Production selection belongs in composition roots. Tests inject adapters. `AssetPort` is a presentation contract because it returns decoded pixels; do not move it into the pure domain.
- Prepare images with `ImageData::from_rgba` on an asset worker. Its validated immutable storage includes premultiplied mip levels; `pixels()` is premultiplied. Views and paint never generate image mips. Each Canvas retains its current scene's GPU resources. Cache eviction removes unused least-recently-used entries; active payload can exceed the reuse budget and is reported separately from driver/framebuffer memory.
- `Clip` is shared presentation geometry for paint, pointer hits and Wayland input regions. Preserve all rounded ancestors. Cover images crop source UVs while retaining destination bounds/radius. Native regions quantize by logical pixel centers; edge antialiasing does not imply input coverage. Keep clipping behavior in the framework and verify GPU pixels at 1×/2× when changing it.
- The runtime owns application state on one UI thread. `view` describes a tree; `update` changes state and requests effects. Child routing uses `Element::map` and `Effects::delegate`. Redraw requests coalesce per surface; they do not paint immediately.
- Effects execute on bounded, named ordered queues (at most eight workers, 64 waiting tasks per queue). Keep dependent writes on the same queue; independent queues can complete in any order. `Effects::delegate` preserves queue and cancellation. Running work cancels cooperatively; queue rejection and panics reach `Application::runtime_failure`. Subscription identity retains captured values until removed/recreated. Event delivery is still unbounded; do not describe this as a general async executor.

Use [the library guide](../../../site/src/pages/docs/library.astro) for public API flow and [the review](../../../site/src/pages/docs/review.astro) for current limits. Read [the domain guide](../../../site/src/pages/docs/domain.astro) when changing service contracts. Avoid duplicating client policy in renderer code.

Run appropriate behavior tests plus `cargo fmt`, `cargo clippy --locked --workspace --all-targets -- -D warnings`, and `python3 -m unittest discover -s tests -v`. Use `--manifest-path lucent/Cargo.toml` from the root. If Cargo is not on PATH, this workspace may have toolchains under `.cache/cargo` and `.cache/rustup`; set CARGO_HOME/RUSTUP_HOME for that command only.

Boundary checks recurse through source modules and resolve local package dependencies across aliases, workspace inheritance, target tables and build/dev dependencies. Keep those declarations covered when extending manifests. Source checks are textual and the dependency graph is workspace-local; they are not a complete semantic or transitive registry audit.

For rendered changes, use the desktop skill's visual workflow. Update live source excerpts if contract markers change; `cd site && npm run check && npm run build` catches broken excerpts. Report actual checks and remaining architectural limits, not just crate names.
