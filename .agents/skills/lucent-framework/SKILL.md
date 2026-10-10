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
- Production selection belongs in composition roots. Tests inject adapters. `AssetPort` is a presentation contract because it returns decoded pixels; do not move it into the pure domain.
- The runtime owns application state on one UI thread. `view` describes a tree; `update` changes state and requests effects. Child routing uses `Element::map` and `Effects::delegate`. Redraw requests coalesce per surface; they do not paint immediately.
- There is one ordered effect worker. Preserve settings-write order when changing scheduling; do not describe it as an existing concurrent async executor. Subscription identity retains captured values until removed/recreated.

Use [the library guide](../../../site/src/pages/docs/library.astro) for public API flow and [the review](../../../site/src/pages/docs/review.astro) for current limits. Read [the domain guide](../../../site/src/pages/docs/domain.astro) when changing service contracts. Avoid duplicating client policy in renderer code.

Run appropriate behavior tests plus `cargo fmt`, `cargo clippy --locked --workspace --all-targets -- -D warnings`, and `python3 -m unittest discover -s tests -v`. Use `--manifest-path lucent/Cargo.toml` from the root. If Cargo is not on PATH, this workspace may have toolchains under `.cache/cargo` and `.cache/rustup`; set CARGO_HOME/RUSTUP_HOME for that command only.

For rendered changes, use the desktop skill's visual workflow. Update live source excerpts if contract markers change; `cd site && npm run check && npm run build` catches broken excerpts. Report actual checks and remaining architectural limits, not just crate names.
