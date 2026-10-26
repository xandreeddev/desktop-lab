# Component lab

The component lab gives Lucent a place to refine controls before fitting them
into menus. The native client and browser capture exporter use the same Rust
stories. The Astro explorer displays the resulting images; it does not rebuild
our buttons, fields or menus in HTML and CSS.

The checked-in catalogue records source `3640bd86c6ec`, **118 producer inputs**,
nine stories, five palettes and 480 actual Vulkan captures. Its fresh native
export matches all 480 PNGs from the earlier `4a0f932a` RADV reference byte for
byte. The new input-release API changes the producer roster without changing
those pixels. The actual exported manifest is promoted; the identical image
files are retained. This is offscreen rendering evidence, separate from native
input, application focus and performance acceptance.

Open `/component-lab/` on the documentation site. Choose a story, then compare
its available palette, state, viewport and pixel density. The URL records the
selection and zoom, so a review can point to one exact combination. Search also
matches the story's token names. Arrow keys move through the story list;
press `/` to focus search. Browser controls choose captures, while the native
client lets you interact with the actual Rust controls.

## Run the native client

From the repository root, in a Wayland session with layer-shell and Vulkan:

```bash
CARGO_BUILD_JOBS=1 cargo run --manifest-path lucent/Cargo.toml --locked -p lucent-lab
```

This is an independent layer-shell client, not an xdg-shell application window
or a replacement desktop session. It uses the framework's real layout, input
and renderer. Its example values belong to the lab; changing a slider, menu
selection or text field does not change audio, network, power or saved desktop
preferences. Close the lab to return to your normal session.

In the Menu story, Tab and Enter normally belong to the focused lab control.
Choose **Try menu keyboard** to enter the production Menu's standalone keyboard
policy: arrows and Tab change the selected entry, Enter accepts, and Escape
cancels the fixture interaction and returns to ordinary lab controls. It does
not close the lab or run the selected desktop action. Reset creates a fresh
fixture; re-entering after completion does the same. Outside that explicit
session, the Menu's search field remains an ordinary editable field and the
lab's palette, story and close controls keep their own keyboard behavior.

Story selection can also be supplied at startup:

```bash
cargo run --manifest-path lucent/Cargo.toml --locked -p lucent-lab -- \
  --story buttons --palette ocean --state focus --viewport narrow
```

The story registry defines supported names and states. The browser's controls
only offer combinations present in the exported manifest. A viewport is a
logical layout size; it is separate from output pixel density.

## Export native captures for the browser

The exporter needs Vulkan, but no Wayland compositor or running desktop. Commit
the producing source first: every rendering input must be tracked and unchanged
from HEAD. Unrelated documentation or website work may remain dirty. Export
records the actual commit and rechecks its input hashes and HEAD before
publishing the manifest. It creates a new output directory and refuses to
overwrite a previous catalogue. Choose a fresh output name for each run. From
the repository root:

```bash
python3 scripts/generate-design-tokens.py --check
python3 scripts/generate-app-icons.py --check
mkdir -p .cache
CARGO_BUILD_JOBS=1 cargo run --manifest-path lucent/Cargo.toml --locked \
  --release -p lucent-lab -- --render .cache/component-lab-next
python3 scripts/check-component-lab.py --catalog .cache/component-lab-next
```

`--render OUTPUT` writes `manifest.json` and PNGs under `OUTPUT/images/`.
An explicit `--source-commit FULLSHA` must match the current Git revision; the
manifest also records hashes of renderer/story inputs and the token source.
The image dimensions and hashes describe the actual files, not a CSS simulation.
The integrity check verifies current producer/token inputs, the complete
declared variant matrix and every image's hash and dimensions. Review the actual
images, preserve the previous public catalogue in an unused ignored archive
directory, then promote the reviewed output to `site/public/component-lab`.
Run the checker again at the public location:

```bash
python3 scripts/check-component-lab.py
```

For the first catalogue, export directly to an absent
`site/public/component-lab` instead. Regenerate captures after changing stories,
recipes, fonts, rendering or design tokens. Rebuilding Astro alone cannot refresh
the native pixels. The checker rejects stale images rather than silently
publishing them with newer source documentation.

Serve the documentation using its existing Astro commands:

```bash
npm --prefix site run dev
```

The server binds to `127.0.0.1:4321` by default. Open
`http://127.0.0.1:4321/component-lab/`. The explorer uses the site's configured
base path, including project GitHub Pages deployments. Generated captures must
exist before Astro checks or builds this page.

## Read the preview correctly

- **Palette** selects a native render using a complete semantic palette. The
  browser does not recolor an existing image.
- **State** selects an actual story state. Hover and keyboard focus have their
  own renderer captures; the browser image itself does not take desktop input.
- **Viewport** changes the logical space available to the story, exposing
  wrapping, clipping and alignment at narrow sizes.
- **Pixel density** selects the render's physical detail. At 2×, an 800 × 512
  logical viewport produces a 1600 × 1024 image.
- **View zoom** changes how large that image appears in the browser. “100%
  logical” displays one logical pixel per CSS pixel; “Actual pixels” displays
  one captured pixel per CSS pixel. Fit can shrink the preview without changing
  the captured density. An enlarged stage can be scrolled with the keyboard.

The recipe shows the Rust composition. The token list names its design-system
dependencies; follow the design-system catalog to inspect their values and
references. Capture provenance includes source, token and image digests.
Download PNG saves the chosen native image without the site's framing.

The production-reference section is deliberately separate. Those images are
existing desktop visual-test baselines, useful for seeing the launcher and
right-hand menus in context. They are not interactive lab stories or proof of
native service behavior.

## Where the code belongs

`lucent-lab` is a client of the framework and optional `lucent-design` recipes.
The framework remains independent of its story IDs, palettes and controls.
Each story names its source symbol, recipe, semantic/component tokens and
supported states. The same view creates the interactive scene and offscreen
captures. State updates stay in the lab; OS adapters are not involved.

The browser consumes the versioned manifest at
`site/public/component-lab/manifest.json`. It contains story metadata and the
actual palette/state/density/viewport variants. Each image path is relative to
the component-lab public asset root. The website only adds navigation, viewing
controls, source links and documentation. Keep design values in
`design/tokens.json`; do not add a second browser implementation of a native
component or filter a screenshot to invent another palette/state.

Prefer a small story for a control or shared composition. Include empty,
focused, disabled, long-content and error cases when they make sense. A disabled
control should not acquire actionable hover feedback; keyboard focus should be
visible without pointer hover. Keep fixture data deterministic and avoid real
credentials, user settings or network/service calls.

## Validation boundaries

Source preparation, successful export, visual review and a native interaction
run are different checks. Generated PNGs exercise the actual offscreen
renderer, but do not prove layer ordering, compositor input, secure-session
behavior, animation smoothness or performance. A motion story samples fixed
times; changing between its images does not measure frame pacing.

The existing production visual suite remains separate:

```bash
LUCENT_VISUAL_FILTER=audio cargo test --manifest-path lucent/Cargo.toml \
  --locked -p lucent-desktop visual_regressions -- --ignored --nocapture
```

It writes actual and diff images to `reports/local/visual-tests/`. Inspect those
images before accepting intentional golden changes. Keep the explorer's
capture integrity checks alongside the normal source/token and production
regression checks; do not treat a successful Astro build as native acceptance.

For opt-in native acceptance on the prepared Lucent VM, the harness defaults to
a plan and does not contact the VM:

```bash
python3 vm/test-component-lab.py
```

Its explicit `--run` mode requires an absolute built binary path, its SHA-256
digest and the VM UUID. It uses actual pointer and keyboard input against an
idle, unlocked 1280 × 800 guest. It checks story selection, visible focus, fixture
events, popover/menu keyboard ownership and animation submissions settling.
It stages and removes only its own client, and verifies restoration of the
original shell, settings, wallpaper, output, focus and cursor. These checks do
not establish pixel fidelity, clipboard/IME interoperability or frame pacing.
Detailed run receipts stay under `reports/local/`; retain failed runs as well
as successful runs.
