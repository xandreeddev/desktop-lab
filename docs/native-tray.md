# Native tray service and client

`TrayPort` is a domain capability: observe items, activate an application, open
its menu and invoke a current menu row. `StatusNotifier` implements it on the
session bus. The composition root injects the adapter and the presentation
`AssetPort` into the `Tray` child. Neither the domain nor the framework knows
about Lucent's panel, design tokens, D-Bus paths or concrete adapters.

## Startup and ownership

Construction starts no thread. The desktop subscription calls `watch`, which
claims both KDE and freedesktop watcher names without replacing or queueing
behind an existing watcher. It serves `/StatusNotifierWatcher`, advertises a
host and observes registration and unique-owner disappearance. A competing
watcher produces an unavailable state. The service reconnects after failure;
all old handles retire and accepted actions are never replayed.

A registration can supply a service name or an object path. A service name must
resolve to the registering sender's unique connection; a path belongs to that
sender. A caller cannot register another application's service. Items are
identified by the unique connection and path, with a fresh `TrayItemId` for each
observed incarnation. Handles are temporary; never persist them in settings.
The watcher publishes registered/unregistered signals for both interfaces.

The protocol follows the [StatusNotifier specification](https://specifications.freedesktop.org/status-notifier-item/latest/)
and the [KDE watcher interface](https://raw.githubusercontent.com/KDE/kstatusnotifieritem/master/src/org.kde.StatusNotifierWatcher.xml).
The host reads KDE items and also accepts the freedesktop item interface.

## Observation, artwork and events

The adapter reads item properties and subscribes to current unique-owner/path
signals before reading. Passive items remain in the snapshot but are hidden by
client policy. Attention, title, plain tooltip text, icon variants, overlays and
menu capability are data. Tooltip markup is stripped; no HTML engine is used.

Pixmap bytes are network-order ARGB. The adapter validates dimensions, selects
the largest accepted variant up to 256 pixels per edge and converts it to
straight RGBA. `TrayPixmap` enforces dimensions and byte length. The asset
adapter prefers a valid icon-theme name, falls back to the pixmap, premultiplies
and prepares mip levels on the asset queue. Views borrow prepared resources.
Application-supplied absolute paths and custom icon-theme paths are not read.
Attention and overlay icons remain application artwork. Bar controls without
valid artwork show an initial from the current title/application, or `?`.
The panel retains its labelled action rows. Movie/animated tray icons are not implemented.

Native tray requests use their own ordered effect queue. The runtime allows nine
bounded effect workers, with 64 waiting tasks per queue. A slow app cannot block
the urgent controls queue used for pairing replies. Workers are created only
when used; this limit is not a process memory guarantee.

The client retains one active icon preparation and the latest pending snapshot.
Stale results do not replace newer demand. Menu signals increment an observation
generation; an open stale menu returns to the item list. Native method waits have
a three-second total request budget; signals can queue during those waits.
The 100 ms idle wake checks cancellation, without polling application properties.

## Menus and actions

`Activate`, `SecondaryActivate`, `ContextMenu` and `Scroll` call the current
application owner directly. A method reply acknowledges that call; it does not
prove a new window appeared. A bar icon maps primary click to Activate, middle
click to SecondaryActivate and right click to Lucent's DBusMenu panel. If an
item offers no DBusMenu object, right click calls ContextMenu instead. For
`ItemIsMenu`, primary click uses that same menu path instead of Activate. The
overflow panel also exposes labelled Open, Secondary action and Menu buttons.

`Activate`/`SecondaryActivate`/`ContextMenu` still pass a neutral `(0, 0)`
screen-position hint. Wayland does not give these layer surfaces a global
origin; surface-local coordinates would be incorrect hints. App-owned popups
may therefore choose a different position. Lucent's own menu uses the clicked
bar's output route and does not depend on this native hint.

The SDK owns clipped hit testing and matching-button capture, exposed through
`Element::on_secondary_click`, `on_middle_click` and `on_wheel`. `Element::map`
lifts all callbacks through the component tree. Native axis events normalize
to signed logical lines, retaining fractional horizontal and vertical values.
A leaf wheel handler consumes the event before ancestor scrolling; unrelated
covered controls never receive it. A focused secondary action can also use
Context Menu or Shift+F10. This does not give the non-focusable top bar keyboard
focus; use the keyboard-accessible overflow panel there.

The tray child translates gestures into domain actions on the existing ordered
tray queue. Fractional wheel demand accumulates per current target. One call
runs at a time, with at most one latest pending target and 64 unsent whole steps
per axis. Changing targets discards the old pending demand. Closing, retirement
and failure discard unsent work; accepted native actions cannot be undone.
Late failures cannot reopen a dismissed panel. Direct activation does not close
other panels; actually opening a tray menu/error panel does.

Menus use `AboutToShow`, `GetLayout` and `Event` from
[DBusMenu](https://docs.rs/dbusmenu-zbus/latest/src/dbusmenu_zbus/dbusmenu.rs.html).
The adapter accepts at most 256 unique nodes and eight nested levels. The client
renders submenus, separators, visibility, disabled rows and checked/radio states
using Lucent SDK controls. Selection indicators are token-based vector shapes,
so their correctness does not depend on font fallback. Labels remove mnemonic
underscores for presentation.
Keyboard focus, controlled scrolling and token-based rounded panels are shared
with other native controls. Menu-specific icons, shortcuts, indeterminate toggles and lazy submenus
beyond the bounded snapshot are not implemented.

Before an invocation, the adapter re-reads the menu and validates the supplied
revision, row ID, enabled/visible ancestry and leaf status. Removed, disabled,
invisible or old rows cannot be invoked through a cached client menu. Closing or
retiring the target discards late client replies. Closing does not undo an action
already accepted by the application. No shell command is built from menu data.

## Boundaries and limits

The watcher retains at most 64 items and 16 hosts. Registration wakes coalesce;
commands have 32 admission slots. Item/menu signal queues have 16/8 entries per
registered target, and the bus-owner queue has 128. A refresh batch shares a
three-second deadline, giving each remaining item a fair share so one slow
application cannot consume the entire read budget. These are count/content limits, not a bound on D-Bus
message allocation or a guarantee against a hostile peer flooding the bus.
Payloads are decoded by zbus before retained image/menu limits are checked.

The bar shows up to `component.bar.max_tray_items` (four) active/attention icons
in spare space without moving existing clock, media or workspace groups. Their
size, padding, gap, radius and attention background use design tokens. An
overflow control remains available even when every icon fits. IPC is `tray
open`, `tray toggle` or `tray close`. At very narrow widths the bar hides its
tray shortcut to retain the active workspace control; IPC remains available.
Other native control panels close when this one opens. XEmbed legacy tray
proxies, visual order/pinning preferences and third-party application acceptance
remain pending.

## Independent consumer and validation

`cargo run --manifest-path lucent/Cargo.toml -p tray-service -- --observe --seconds 5`
uses the same library without Wayland, rendering, desktop components or Omarchy.
Run it in a session without a competing watcher. Observation publishes counts;
`--stdio` explicitly opts into current IDs/titles and commands: `activate ID`,
`menu ID`, `invoke ID REVISION ENTRY`, `quit`. Input is bounded to 4096 bytes.
`--bus-address` selects an explicitly owned fixture bus.

Private protocol tests spawn and destroy their own `dbus-daemon`. They exercise
registration authority, pixel conversion, native methods, DBusMenu, owner
retirement and competing watcher behavior without touching the host tray.
`python3 vm/test-tray.py` uses an owned time-limited Python SNI/DBusMenu fixture
and real QMP input in the existing unlocked VM. It requires an empty tray and
single 1920×1080 output, checks native gestures/menu/overflow/owner retirement,
and preserves saved settings and wallpaper. Results stay under ignored
`reports/local`; merely adding the harness does not establish a pass.
Executed results belong in the measurement report. Third-party interoperability,
hardware input and performance remain separate acceptance.
