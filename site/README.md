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

The static output is in `dist/`. There is no deployment configuration. CI runs
type checking and a production build.

## Content and assets

- `/`: desktop tour, three experiments, architecture/API, motion, surfaces and measurements.
- `/docs/`: programming model, interactive frame lifecycle, API, domain and rendering.
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
