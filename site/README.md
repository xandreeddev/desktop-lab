# Desktop Lab site

A local Astro site explaining Lucid, Noctalia and the Lucent Rust framework.
It includes real VM captures, an interactive architecture walkthrough, a browser
motion illustration, actual performance data and a practical testing guide.

No React. Static Astro pages, handwritten CSS and a small TypeScript module for
keyboard-accessible tabs, copying commands and the motion illustration.

## Run locally

Use Node 22.12 or newer, from this directory:

```sh
npm ci
npm run dev
```

Open <http://127.0.0.1:4321>. The server binds only to loopback. The dev script uses
Astro's explicit background mode; manage it from the same directory:

```sh
npm run status
npm run stop
```

The site is independent of the VMs and does not start or modify them. Commands in
the guide are examples to copy and run in the indicated host or guest environment.

## Check and build

```sh
npm run check
npm run build
# Stop the development server before using the same port for preview:
npm run stop
npm run preview
```

The static output is in `dist/`. CI checks both root hosting and the project
Pages prefix, including links, image sources and fragment targets in the built HTML.

## GitHub Pages

The source repository is <https://github.com/xandreeddev/desktop-lab>.
`.github/workflows/pages.yml` publishes the site whenever a PR merges into
protected `main`. Every update to `main` triggers it, without path filters.
Manual recovery runs are also restricted to `main`. It checks the Astro source, builds the static site,
validates its links/assets, uploads only `site/dist` and deploys that artifact.
It does not publish VM images or change repository visibility.

Before merging, `main` requires the `site`, `integration-code` and `rust` checks
from GitHub Actions and an up-to-date PR branch. The Pages environment accepts
deployments only from the `main` branch. Pull requests run validation without
publishing a preview or receiving Pages write permissions. Repository access and
protection maintenance are documented in [Contributing](../CONTRIBUTING.md).

One-time repository setup: in **Settings → Pages → Build and deployment**,
choose **GitHub Actions** as the source. This requires an authenticated account
with permission to manage Pages. Private repositories also need a GitHub plan
that supports Pages. After enabling it, run **Publish site to GitHub Pages** from
the Actions tab if the initial push happened before Pages was enabled.

The workflow reads the actual origin and base path from GitHub Pages metadata.
Project hosting normally uses `/desktop-lab/`; an account-level custom domain
can change the origin. The successful deployment's URL is authoritative.
Do not add a custom domain to this repository unless intentionally changing it.

To reproduce project hosting locally:

```sh
SITE_BASE_PATH=/desktop-lab npm run build
SITE_BASE_PATH=/desktop-lab python3 scripts/check-build.py
SITE_BASE_PATH=/desktop-lab npm run preview -- --port 4322
# Open the printed preview URL plus /desktop-lab/ (normally port 4322).
```

`npm run dev` continues to serve at `/`. Internal links and public assets use
the shared `withBase` helper; imported assets are prefixed by Astro. The workflow
sets `SITE_URL` and `SITE_BASE_PATH`; no deployment secrets are embedded in output.

## Content and assets

- `/`: desktop tour, three experiments, architecture/API, motion, surfaces and measurements.
- `/docs/start-here/`: beginner walkthrough from Linux boot to registration, loading and rendering.
- `/docs/execution/`: source-backed construction/declaration/execution path, adapter wiring, first-frame handshake, interactive coordinate illustration, launcher trace and architecture assessment.
- `/docs/`: programming model, interactive frame lifecycle, API, domain and rendering.
- `/docs/domain/`: actual domain models, all service ports, adapters, use cases and failures.
- `/docs/framework/`: component contract, workers, layout, input, motion and Vulkan internals.
- `/docs/client/`: dependency injection, state lifetimes, launcher/bar/widget flows and native clients.
- `/design-system/`: themes, typography, spacing, motion and a searchable token catalog.
- `/guide/`: VM startup, interactions, ownership, build, state, rollback and validation.
- `/reports/performance.json`: the actual repository measurement data.
- `/licenses/fonts.txt`: font license, distributed with the site.

Screenshots and font assets are imported directly from the repository, so keep
the full checkout when building. Astro generates responsive WebP images. Fonts
are served locally. There are no external font, analytics or CDN requests.

Visual tokens come from `design/tokens.json`. Regenerate Rust/CSS with
`python3 scripts/generate-design-tokens.py`; CI rejects stale generated files.
The token catalog reads the same JSON directly.

Implementation guides import selected Rust sources with `?raw` at build time.
`src/data/source.ts` extracts snippets using explicit markers and rejects missing
markers during the build. Keep prose, adapter tables and source links aligned
with implementation changes; snippets alone cannot verify narrative claims.

The execution guide's coordinate explorer illustrates `shell_layout::panel_origin`
in browser JavaScript/SVG using current design tokens. It does not execute Rust
or Vulkan. Its sample sizes are the initial dock and minimum launcher dimensions;
the live desktop derives final sizes from content. Keep its calculation aligned
with that Rust function and check controls, keyboard navigation, scaling and
mobile layout when editing the guide.

The technical source of truth is `docs/lucent-framework.md`,
`reports/lucent-framework.md`, `lucent/README.md` and the measurement JSON under
`reports/measurements/`. Keep narrative claims in sync when those change. The
site distinguishes implemented features from future work and software-Vulkan VM
observations from unmeasured hardware performance.

Screenshot wallpapers retain their original rights. The fonts use the SIL Open
Font License; upstream attribution is in the client assets directory. The motion
study is explicitly a browser illustration, not the native shell running in a page.

Browser validation covers gallery/architecture tabs and arrow-key navigation,
motion expansion/collapse, clipboard text including newlines, guide accordions,
internal links, the measurement endpoint, responsive layouts and reduced motion.
