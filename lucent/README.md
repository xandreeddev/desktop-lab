# Lucent — draggable Vulkan prototype

A native Rust Wayland widget on a transparent layer surface. Drag the card with the left mouse button; release to save its position. A click changes its color. Right-click fades it out and closes it. Opening, hover, press/release and color changes animate; frames stop when the scene settles. Clicks outside the rounded card pass through to applications underneath.

This is the working prototype from Phase 0/Milestone 1. Omarchy remains active in the Lucent VM. The bar, launcher, dock, settings and lock screen still belong to Omarchy. Those features and the complete reusable widget toolkit remain future work.

Build on Arch with `rust`, `pkgconf`, `wayland`, `libxkbcommon`, `fontconfig`, a font, `vulkan-icd-loader`, and a Vulkan driver:

```sh
cargo build --locked --release
cargo run --locked --release -p lucent-desktop
cargo run --locked --release -p hello-layer
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
```

Run inside a compositor supporting `zwlr_layer_shell_v1`. The widget host covers one compositor-selected output on the overlay layer, with no reserved space or keyboard grab. Its Wayland input region follows the card, including its rounded corners. Pointer coordinates belong to the stationary host surface, so dragging does not jump as the card moves. A 5-logical-pixel threshold separates drags from clicks. Positions are clamped to output bounds and saved atomically in `$XDG_STATE_HOME/lucent/position` (default `~/.local/state/lucent/position`). Remove that file while the demo is closed to recenter it.

The renderer explicitly selects **Vulkan** and requires premultiplied alpha. It fails with an error if either is unavailable. Logs identify the actual adapter. The prepared guest uses Mesa **lavapipe software Vulkan**, not host GPU acceleration: Venus initialization was blocked by QEMU's process-spawning sandbox, and the VM was restored to its original virgl graphics definition. Its compositor still uses virgl; Lucent uses Vulkan. No host sandbox setting was changed. See [VM graphics setup](../docs/lucent-vulkan.md).

Rounded shapes, gradients, borders, shadows and transforms run in the Vulkan shader. Text is rasterized only when its content or integer scale changes and then cached in a texture. Rendering is restricted to the card/shadow quad on a transparent target. This is a small renderer, not a complete text shaping or vector framework. Fractional scaling, multiple outputs, resizing widgets and GPU device-loss recovery remain future work.

From the repository root, `scripts/install-prototype.sh` builds and installs the executable and menu entry under `~/.local`. A compatible prebuilt binary can be passed as its sole argument. Open **Lucent Prototype** from the application launcher. The installer never changes shell startup. Close the existing demo before installing a new build. Uninstall by removing `~/.local/bin/lucent-desktop` and `~/.local/share/applications/lucent-desktop.desktop`; remove the position file if desired.

`python3 vm/test-prototype.py` exercises the prepared, unlocked guest using real pointer events. It verifies Vulkan and alpha selection, animated opening/color changes, click-through into a terminal, dragging without an accidental click, cached text uploads, position persistence across restart, a 120-second interval with no extra frames, and animated exit. Reports and screenshots first go into ignored `reports/local/` for review.
