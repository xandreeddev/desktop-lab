# Lucent — Milestone 1 prototype

A native Rust Wayland layer surface rendered through wgpu. Left-click increments a counter and changes the card color; right-click closes it. It blocks on Wayland events while idle and requests frames only after input, configuration or scale changes.

This is the working prototype from Phase 0/Milestone 1. It is **not a complete desktop shell**. Omarchy remains active in the Lucent VM. No launcher, dock, OS services, secure locker, reusable widget toolkit or notification server is implemented yet.

Build on Arch with `rust`, `pkgconf`, `wayland`, `libxkbcommon`, `fontconfig`, a font, and a working Mesa/Vulkan driver installed:

```sh
cargo build --locked --release
cargo run --locked --release -p lucent-desktop
cargo run --locked --release -p hello-layer
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
```

Run inside a Wayland compositor supporting `zwlr_layer_shell_v1`. The VM supplies a launcher in its application menu after provisioning. The prototype uses a 540×190 overlay with no exclusive zone or keyboard grab. Left-click changes state; right-click exits. Standard integer output scale changes are handled; fractional scaling and monitor hotplug behavior need a later milestone.

The GPU backend is selected by wgpu. Logs identify the actual adapter and print one line per rendered frame. Software Vulkan is still software rendering; do not compare its memory or CPU figures to physical-GPU results. See [architecture decision](../docs/adr/0001-native-prototype.md).
