# Lucent component lab

The native viewer and exported catalogue use one story registry and the actual
`lucent-design` recipes. The Menu story embeds `lucent-menu::Menu`; completion
is recorded locally, child redraw and clipboard targets are lifted to the lab,
and no desktop action or settings adapter is supplied.

```sh
cargo run --manifest-path lucent/Cargo.toml -p lucent-lab -- --story input --palette dark --state filled --viewport narrow
cargo run --manifest-path lucent/Cargo.toml -p lucent-lab --release -- --render /tmp/lucent-component-catalogue --source-commit FULL_GIT_SHA
```

The native client uses a centered layer-shell surface with on-demand keyboard
input and its own `lucent-lab` IPC identity. It needs the running compositor;
it is not an xdg-toplevel window. Tab, pointer hover, selection, scrolling,
clipboard, IME and popover focus use the runtime. The hover snapshot preset
sets actual offscreen `Interaction` state; native hover follows the pointer.
The focus preset requests initial focus and later focus stays engine-owned.
Menu's standalone keyboard policy is entered explicitly with “Try menu keyboard”;
Escape returns to ordinary Lab controls. Outside that session, Tab and Enter
belong to the actual focused Lab control. Global wheel events are not delegated.
The bounded event log never records text contents. Menu cancellation closes
the fixture interaction, not the lab; Reset constructs a fresh Menu completion.

Export requires every producing input to be tracked and unchanged from HEAD,
and a new output directory. Unrelated documentation work may remain dirty. It writes
`manifest.json` and `images/*.png`: all 24 declared states, five palettes, two
logical widths (800/352 × 512), and integer 1×/2× density, for 480 images. One
GPU/Canvas is reused sequentially and old scenes are dropped. Each image is
hashed; producer Rust/WGSL, fonts, tokens, palettes and lockfile are hashed in
`renderer.inputs`. Source changes during export prevent manifest publication.
The browser selects these actual rendered states; it does not emulate native
input or prove compositor presentation, clipboard/IME interoperability, or FPS.

Ordinary snapshots sample the settled fixture at 1000 ms after initialization.
Motion presets freeze samples of the real shared `Motion` curve at 0/250/500 ms;
Replay uses normal compositor-paced `Application::animating`, without a timer.
Recipes list canonical design-token paths; geometry and colors use generated
tokens and semantic Theme roles. Desktop private panels are outside this initial
catalogue. Exporting a new palette/state does not imply native acceptance.
