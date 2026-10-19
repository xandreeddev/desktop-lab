# Per-monitor desktop layouts

Lucent maps each uniquely named Wayland output onto a validated domain
`MonitorId`. Every connected named monitor gets its own top bar, background,
bounded widget groups and temporary drag-grid surface. Its widget host owns
gesture capture, calendar navigation, timer and text selection state. Service
subscriptions, theme, app catalog, wallpaper selection and notes remain shared
in the desktop composition root.

The launcher/dock, notifications and control panels use the active monitor.
Entering a monitor's top bar or interacting with one of its widgets selects it.
The initial fallback is the first unique name in sorted order. Fresh compositor
focus then selects its matching available host or waits for that exact name. See
[compositor focus](compositor-focus.md) for recovery and explicit selection policy. A disconnected
active monitor selects another available name. You can also select explicitly:

```sh
lucent-cli monitor select DP-2
lucent-cli widgets open
lucent-cli widgets edit on
lucent-cli widgets size notes 384 240
lucent-cli widgets edit off
```

The widget selector reflects that monitor's visibility. Show/hide, size, drag
release and Reset affect its profile. Notes text is shared. Drag previews remain
transient and snap on release using the same design grid. Physical input,
clipboard targets and callbacks carry the concrete instance. A completion from
a retired monitor host cannot mutate its replacement, even when the compositor
reuses a global number. Clipboard completion does not select a different monitor.

## Saved configuration

The root persistence coordinator still writes one `desktop.json` through the
injected `SettingsPort`. No widget or monitor host writes a file. Version 1 keeps
the existing top-level `positions`, `widget_sizes` and `visible_widgets` keys as
the default layout. A new `monitor_layouts` map stores overrides:

```json
{
  "version": 1,
  "visible_widgets": ["calendar", "clock", "weather", "media"],
  "positions": {},
  "widget_sizes": {},
  "notes": "Shared notes",
  "monitor_layouts": {
    "DP-1": {
      "visible_widgets": ["calendar", "clock"],
      "positions": {"calendar": {"x": 64, "y": 112}},
      "widget_sizes": {}
    },
    "DP-2": {
      "visible_widgets": ["notes", "weather"],
      "positions": {"notes": {"x": 64, "y": 112}},
      "widget_sizes": {"notes": {"width": 384, "height": 240}}
    }
  }
}
```

An output without a saved profile borrows the default layout. Its first widget
interaction materializes only that layout; it does not clone the desktop or
service state. Registry discovery and clock ticks do not create profiles or
request saves. At most 64 profiles are accepted. Disconnected profiles remain
on disk; remove an obsolete map entry manually with Lucent stopped if needed.
Invalid IDs, positions and sizes fail validation and preserve the existing file.
Edit a backup of the settings while the service is stopped, then restart it.

## The complete declarative path

```text
SCTK output observation
  → SDK OutputsChanged catalog
  → Desktop::update(Outputs)
  → Monitors maps protocol names to domain IDs and transient host generations
  → Application::surfaces declares role + instance + output
  → framework reconciles typed addresses and current native output bindings
  → configured ViewContext carries actual dimensions + instance
  → Desktop borrows this profile, host, snapshots and prepared artwork
  → ordinary Element tree, layout, Vulkan paint and frame demand

widget input
  → mapped OnMonitor(route, Widgets(message))
  → validate the live host generation
  → child update borrows WidgetLayout and shared notes
  → transient preview or validated pure layout mutation
  → exact instance redraw; Save intent goes to the root persistence coordinator
```

The pure domain contains `MonitorId` and `WidgetLayout`. Widget use cases mutate
that layout directly; the default and named profiles use the same algorithms.
The desktop `Monitors` host owns protocol-name mapping, current geometry and
callback generations. The reusable runtime owns native surfaces/input/frame
demand. It has no widget catalog, profile migration or desktop monitor policy.

## System widget redraw routing

The existing five-second system subscription stays shared. An unchanged
`SystemSnapshot` requests no redraw; a changed snapshot updates the shared value
and the desktop resolves the current surfaces whose members include `system`.
An overlap group can have another role, such as `calendar`, so invalidation uses
membership rather than assuming a surface named `system`. Each connected host
uses its saved layout or borrows the default, without creating a profile. Saved
disconnected profiles do not receive redraws; the legacy host keeps its exact
uninstanced address.

During drag or resize capture, the host retains its frozen source groups and
also paints the cards in its temporary `widgets` preview. Both relevant exact
addresses are invalidated. Hosts that do not paint `system` and unrelated groups
remain untouched.
This is client routing over the existing surface API; it adds no polling,
persistence, renderer or domain behavior. Focused source regressions accompany
the change; compilation and native/performance acceptance remain pending.

## Geometry, density and startup

Background configure events establish each monitor's logical viewport and
actual rendering density. Resizing, catalog geometry changes or settings replacement
cancels that host's capture. Presentation clamps cards to the current viewport
without rewriting stored preferred positions or sizes. Reconnecting recreates
transient host state and reuses the saved layout.

Background pixels are prepared once through the existing coalesced appearance
worker for the maximum physical width and height needed across connected
monitors. All views share those immutable pixels and the existing GPU device;
each native buffer still has its own cost. This is not a total memory budget.

Activation and supervision share the readiness helper. It matches explicit
roles/instances and requires submitted frames for every declared monitor's bar,
background and widget groups, plus the shared dock. Printable inspector IDs are
never parsed as addresses. Deploy the helper with the new client. Older
uninstanced releases keep their legacy readiness contract.

If every output is unnamed, Lucent retains its single compositor-selected
fallback. When unique names exist, unnamed outputs are omitted. Ambiguous named
outputs wait; they never get duplicate profiles/surfaces. With no usable outputs,
ordinary surfaces retire and saved layouts remain. The secure lock client keeps
its separate all-output authenticated protocol path.

## Established checks and deferred acceptance

Owned tests exercise simultaneous declarations, independent profile commits,
reflow cancellation, reconnect generations, scoped physical input, clipboard
addressing, shared notes, unchanged legacy JSON and readiness across all hosts.
Software-Vulkan references cover distinct profiles, the drag grid and the
profile-aware selector at 1×/2× in both palette directions.

Actual compositor placement, simultaneous native keyboard/pointer/clipboard,
hotplug/suspend, service restart and secure multi-output acceptance are deferred
under the user's instruction. Current process RSS and animation pacing remain
unmeasured. The optional fractional protocol path is described in [fractional scaling](fractional-scaling.md).
Per-monitor wallpaper selection, output naming changes and native output
arrangement settings remain separate work. This source is not deployed to the VM.
