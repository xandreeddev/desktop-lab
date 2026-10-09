# Desktop Lab

Three isolated Omarchy experiments for comparing Lucid, native Noctalia v5, and an independent Rust Wayland framework. The repository owns configuration, pinned upstream sources, auditable installers, backups, VM scripts and test reports. It does not change the host desktop or packaged Omarchy files.

| VM | Purpose | Scope |
| --- | --- | --- |
| `desktop-lab-lucid` | Closest match to the Lucid reference | Pinned Lucid 1.20, floating capsules/dock, Bookends widgets and matching wallpaper |
| `desktop-lab-noctalia` | Native C++ comparison | Official Noctalia v5, a Lucid-inspired declarative configuration |
| `desktop-lab-lucent` | Native Rust development | Rust framework/client with bar, dock, launcher, selectors and draggable widgets |

Lucent now has a reusable declarative Rust framework and a desktop client using Vulkan. Its launcher morphs out of the dock, selectors animate, and widget positions persist. Omarchy retains secure locking and session services. The VM uses software Vulkan; see [framework concepts](docs/lucent-framework.md) and [current validation](reports/lucent-framework.md).

The [local Astro site](site/README.md) provides a visual desktop tour, interactive
framework walkthrough, measured results and a hands-on VM guide. Start it with
`cd site && npm ci && npm run dev`, then open <http://127.0.0.1:4321>.

On the prepared host, start one VM at a time:

```sh
python3 vm/launch.py lucid
python3 vm/lab.py shutdown lucid
# Substitute noctalia or lucent.
```

Use the source Omarchy VM's existing guest login. Credentials are kept only in the source VM's local login note; the Git repository contains none. All three guests appear under **QEMU/KVM User session** in Virtual Machine Manager. Their SSH ports are loopback-only: 2241, 2242 and 2243 respectively. Disks and runtime inventory live in the ignored `.local-vms/` directory. Do not move it while the VMs reference it.

- [Installation, cloning and rollback](docs/install-and-rollback.md)
- [Compatibility, service ownership and remaining gaps](docs/compatibility.md)
- [Test shortcuts](docs/shortcuts.md)
- [Lucid installer audit](docs/lucid-installer-audit.md)
- [Lucent build and usage](lucent/README.md)
- [Validation and actual measurements](reports/validation.md)

Inside the Lucid or Noctalia VM, run `~/desktop-lab/scripts/verify.sh` from a graphical terminal. Run `~/desktop-lab/scripts/rollback.sh` and log out/in to restore the original user configuration. Packages remain installed, with before/after inventories available for review.

In the Lucent VM, **Super+Space** opens the Rust launcher and **Super+Ctrl+Space** opens the wallpaper carousel. The launcher's Widgets tab controls desktop widgets; drag their backgrounds to move them. [Build, usage and rollback](lucent/README.md).

The prepared guests have a locally selected weather city with IP detection disabled. Personal location data stays outside Git; a fresh install needs your city in Location settings. Media widgets show their idle state until a player publishes MPRIS metadata.

Upstream references: [Omarchy](https://github.com/omacom/omarchy), [Lucid](https://github.com/Sn3akyy1/lucid), [reference screenshot](https://github.com/Sn3akyy1/lucid/blob/main/assets/desktop.webp), [Noctalia v5](https://docs.noctalia.dev/noctalia/), [Amane](https://github.com/MystiaFin/amane), [Suzuha](https://github.com/MystiaFin/suzuha). Exact inspected revisions and archive checksums are recorded in `manifests/upstream-lock.json`. Noctalia's installed package version is recorded separately; its current source tree is a documentation reference.

Lucent’s shared visual tokens live in `design/tokens.json`. The local site adds
`/design-system/` for the live token catalog and `/docs/` for the state-to-pixels
walkthrough. See [the framework guide](docs/lucent-framework.md) for the native API.
