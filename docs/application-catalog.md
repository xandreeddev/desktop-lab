# Application catalog and dock preferences

`ApplicationPort` exposes `discover`, `watch`, `watch_updates` and application
launch/open commands. The production `XdgApplications` adapter captures its source
directories, absolute PATH lookup roots, session desktop and locale at
construction. The composition root injects it; clients never read
`.desktop` files or choose production implementations.

## How applications reach the launcher

1. `Desktop::subscriptions` registers the `applications` stream. Its worker calls
   the port's `watch_updates` method with the runtime's cooperative stop signal.
2. The native adapter registers Linux directory notifications before discovery.
   Missing directories have parent watches; newly created subdirectories and
   replaced roots are registered again before reading their catalog.
3. Source files are sorted and desktop-file IDs derive from their relative path.
   Earlier XDG roots mask lower-priority files, including hidden/invalid entries.
   Discovery honors visibility, desktop filters, locale and executable checks.
4. Changed catalogs become typed `Catalog::Observed` messages. The UI thread
   updates its application aggregate, preserves launcher selection by `AppId`
   and reconciles dock/launcher geometry. An empty ready catalog differs from
   loading or an unavailable observation. Last-good data survives failures.
5. The catalog coordinator prepares application icons through `AssetPort` on
   the existing asset queue. It retains one active task and latest pending
   demand. Stale results cannot replace a newer catalog; retired icon keys are
   removed. Icon slots are namespaced by application ID, with shared decoded
   fallbacks reused inside one preparation.
6. Views borrow applications and prepared images. `view` performs no file reads
   or image mip generation. Native layout/rendering still uses the SDK tree.

Notifications debounce for 150 ms, with a 500 ms maximum batching interval.
The cancellation check waits at most 100 ms during an idle watch; idle catalogs
are not periodically rescanned. Production also watches the captured executable
directories, every PATH candidate for a named TryExec (including currently
missing candidates), absolute TryExec/icon dependencies and XDG icon/pixmap trees.
Legacy `~/.icons` is observed in the production source. The parser and observer
use the same captured executable lookup. Symlink prefixes are subscribed before
resolution; expanded missing targets remain watched through their nearest
existing ancestor, allowing deletion/recreation, retargeting and cycle repair.
Resolution is bounded to 40 symlink hops and 8,192 components per dependency;
dependency identities and native watches each have a 4,096 bound.
These use the same notification descriptor. Tree recursion is bounded to eight
levels. The observer reports `ApplicationCatalogUpdate.artwork_changed` when icon
content changes without descriptor changes; the client coalesces icon preparation
on its existing asset queue. Views never inspect file mtimes or poll icon themes.
There are limits of 20,000 source files, 1 MiB per file, 16 MiB retained catalog
string content and 4,096 directory watches. Non-regular entries are rejected
through a nonblocking file-open path, while their presence still masks lower-
priority files with the same identity. These are separate from image,
framebuffer, D-Bus and total process memory budgets. Filesystem operations happen
on the observer/asset workers; kernel or remote-filesystem stalls are not made
cancellable by the stop signal.

The identity/priority behavior follows the
[desktop entry specification](https://specifications.freedesktop.org/desktop-entry-spec/latest-single/).
Native observation uses directory notifications, not a subprocess watcher.

## Saved pins and order

`DesktopSettings.pinned` retains ordered, validated application IDs. The new
`dock.default_pins` flag distinguishes initial suggestions from an explicitly
empty list. Older settings keep their defaults, and existing nonempty lists
retain their order. Uninstalled apps remain saved so reinstalling restores the
pin; they are omitted from the current rendered list.

The pin button beside a launcher result toggles its saved pin. Ctrl+P toggles
the selected application's pin without changing the launcher section. Pinning,
unpinning and order changes use pure use cases; the existing persistence
coordinator remains the sole producer of ordered settings saves.

Drag previews belong to the dock coordinator and never mutate preferences.
Release commits a validated order. Catalog/compositor/viewport changes cancel
an active gesture; a new physical press distinguishes a new gesture from a
stale release. Input-only sources preserve capture while painted previews move.
Dragging a running app can make it a pin; saved pins precede other running apps.

Commands are literal IPC values, not shell snippets:

```text
lucent-cli dock pin org.example.App.desktop
lucent-cli dock unpin org.example.App.desktop
lucent-cli dock move org.example.App.desktop before org.example.Other.desktop
lucent-cli dock move org.example.App.desktop
lucent-cli dock auto-hide on
lucent-cli dock auto-hide off
```

## Visibility and framework events

`dock.auto_hide` defaults to false. The command palette can toggle it, and IPC
can set an explicit value. A hidden dock retains only a small input-only hotspot.
Entering reveals it through token-based motion; leaving schedules a generation-
bound hide timer. Dragging and an open launcher keep it visible. The launcher
continues to use the existing dock-to-launcher morph and frame-submission gates.

The SDK adds surface pointer-boundary events, named mouse buttons and character
shortcut events with typed modifiers. These are general input concepts: the
Wayland adapter translates native events, while the dock client decides what
those events mean. The renderer knows nothing about pins or auto-hide.

## Independent consumer and acceptance

`application-service --observe` prints counts, `--list` opts into names/IDs, and
`--launch ID` explicitly launches the current catalog entry. `--data-dir` selects
an absolute isolated source. This consumer depends on domain/services only; it
has no Wayland, rendering or desktop-client dependency.

Owned temporary-directory tests exercise kernel creation, edits, masking,
deletions, missing/replaced roots and cancellation. Injected client checks cover
current selection, icon demand, explicit empty pins, transient drag state and
stale hide timers. Executed results belong in the measurement report. Native VM
input, sleep/display changes, D-Bus activation and broader desktop-entry features
remain separate acceptance/work; implementation is not a claim those passed.
