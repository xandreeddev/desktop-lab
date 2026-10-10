---
name: lucent-desktop
description: Implement Lucent desktop behavior, saved preferences, design tokens and Omarchy integration; verify native visuals and deploy to the existing desktop-lab VM when requested. Use for desktop and user configuration work rather than generic framework design.
---

# Lucent desktop

Work from the repository root and inspect the existing state before edits. Read [user configuration](../../../site/src/pages/docs/desktop.astro) and [ownership](../../../site/src/pages/docs/system.astro) for the boundary relevant to the requested feature.

- `Desktop` gets implementations through `DesktopAdapters`; contracts retain Port names. Concrete production selection belongs in `platform.rs`.
- Geometry and animation targets share `shell_layout.rs`. Layout preferences belong in persisted domain settings, while screen limits and token-based geometry belong to the client. Preserve backward defaults, selection reachability and unrelated preferences.
- Edit `design/tokens.json`, then run `python3 scripts/generate-design-tokens.py`. Never hand-edit generated Rust/CSS or window rules. Native view styles use semantic/component tokens. Check rounded insets and keyboard focus as well as pointer hover.
- Lucent owns shell UI and theme authority. `scripts/lucent-session.py` supervises the native desktop; polkit uses libpolkit-agent with native presentation, idle uses headless swayidle, and clipboard capture is separate. Keep secure lock recovery working. Never restart stock shell UI as a crash fallback; stock restoration is an explicit rollback.
- `ThemePort` receives a typed mode; `LucentTheme` exports generated tokens outward to apps. Omarchy’s headless templates may consume our colors, but stock themes must never overwrite the Lucent palette. Do not introduce a reverse theme hook. Keep backups and wallpaper preservation intact.
- Keybinding/root/submenu rendering uses the native picker, but discovery and actions remain Omarchy-owned. Preserve user extensions and exact selected values. Lock/login are separate secure clients; ordinary overlays cannot replace session-lock authentication.

For validation, use `cargo test --manifest-path lucent/Cargo.toml -p lucent-desktop` and the related domain/adapter tests. Run the ignored `visual_regressions` test explicitly on a Vulkan-capable system. It writes `reports/local/visual-tests/index.html`; review actual/diff images at 1×/2× and narrow sizes. Update goldens with `LUCENT_UPDATE_GOLDENS=1` only after reviewing intentional changes, then rerun without that flag. Keep unrelated golden changes out of the commit.

For a requested VM test/deployment:

1. `python3 vm/lab.py status`; use the existing Lucent guest and keep other profiles untouched. Check host memory before boot/build. The guest can be killed by host OOM independently of Lucent correctness.
2. `python3 vm/lab.py start lucent`; `python3 vm/run.py --sync lucent` transfers sources only. Use `vm/run.py --session lucent` for commands needing graphical-session variables. SSH credentials remain in ignored `.local-vms`; never publish them or feed them to an ordinary terminal.
3. Build the requested binaries, preserve a rollback copy, install through a temporary file and atomic rename, then restart only `lucent.service`. A shell UI change does not require reinstalling login or changing disks. Check service readiness, `lucent-cli inspect`, keyboard/click interaction and settings after restart.
4. Use existing `vm/test-*.py` helpers where applicable; inspect their effects before running a broad suite. Capture sanitized local evidence and distinguish VM-only verification from hardware tests.

For docs, keep library explanations under the library guides and runtime preferences under the desktop guide. `cd site && npm run dev -- --host 0.0.0.0` serves on LAN when requested; verify the actual listening port and HTTP response. Do not terminate unrelated servers. Source/built links must respect `withBase` for GitHub Pages.

Follow the existing protected-main PR workflow. Checks must pass before a merge. This skill does not itself authorize publishing, changing repository permissions or modifying the host desktop. Preserve the alias identity for commits and keep personal paths, location, credentials and unreviewed VM captures out of public artifacts.
