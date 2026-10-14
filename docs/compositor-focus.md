# Compositor focus, monitor selection and workspace presentation

The pure `CompositorPort` now reports monitor observations alongside workspaces
and windows. `CompositorMonitor` contains a validated `MonitorId`, observed focus
and optional active/special `WorkspaceRef` values. The same validated identity
is used in `Workspace.id` and optional `Window.workspace`: a positive number,
a regular name, or a special overlay name. These values have no sockets,
Wayland outputs, renderer types or persisted-layout policy.

`CompositorSnapshot::focused_monitor` accepts exactly one focused, uniquely
identified monitor. `monitor(id)` likewise rejects duplicate identities. The
Hyprland adapter omits invalid/disabled and ambiguously named monitor records.
Zero/invalid workspace IDs and empty native addresses mean no reference. Modern
addresses and explicit types take precedence over display titles. Regular bare
names retain even native-looking prefixes; the dispatch selector is not the
observed address. Legacy negative IDs normalize
through their names; they never become numeric destinations. Display names can
change without changing a workspace identity. Duplicate workspace identities are
omitted, rather than assigning ambiguous action authority.

## What runs and where

```text
platform::desktop_adapters()
  → inject Arc<dyn CompositorPort> implemented by Hyprland
  → desktop compositor Subscription starts its observing worker
  → connect event socket before initial snapshot queries
  → bounded JSON reads become pure CompositorSnapshot
  → Message::Compositor reaches the UI thread
  → fresh focus selects a currently connected exact MonitorId
  → normal surface reconciliation moves the shared panels/dock
  → each bar view selects its own observed active workspace
```

The native Wayland output catalog supplies connected buffer hosts, dimensions
and density. The compositor port supplies desktop focus/workspace observations.
The client joins the two by validated literal names. Neither the domain port nor
the renderer owns that join. Host generations still guard callbacks, physical
input and clipboard; a focus observation cannot resurrect an old native surface.

The independent consumer exercises the same port without a renderer or Wayland
runtime:

```sh
cargo run --manifest-path lucent/Cargo.toml --locked -p compositor-service -- --snapshot
cargo run --manifest-path lucent/Cargo.toml --locked -p compositor-service -- --observe --seconds 5
```

It prints counts and whether focus is unique, without display names, serials,
window titles or native paths. `--help` runs before environment discovery.
Snapshot reads and observation are read-only; this consumer sends no dispatches.

## Selection policy

A new compositor focus observation selects its available native host and adjusts
the launcher geometry/shared panel targets. If the matching Wayland output has
not arrived, its exact name waits in `pending_monitor_focus`. Discovering another
output never substitutes that name. Background resize and density feedback do
not discard the pending observation.

Entering a bar, using a widget or `monitor select NAME` makes an explicit client
selection and retires pending focus. Unrelated title/window/workspace updates
with unchanged focus do not undo that selection. A subsequent actual focus
change or recovered observing connection is fresh authority. Focus loss or
transport failure does not invent a new output, overwrite layouts or discard
the last good compositor snapshot.

`compositor_available` and `compositor_error` belong to this observation path.
Unrelated command completions cannot clear its error. A successful fresh snapshot
recovers it. `compositor_focused_monitor`, `pending_monitor_focus` and
`active_monitor` distinguish observations, pending joins and client selection in
local inspection.

## Per-monitor bars

The workspace strip remains client policy. It supplies the existing numbered
shortcut slots and uniquely observed numbered, named and special destinations. A named bar uses that
monitor's observed active reference for its indicator; it does not copy the
global focused workspace to every display. Missing/ambiguous monitor metadata
produces no claimed active indicator. An uninstanced legacy bar retains its
global snapshot behavior. Named controls show the first letter of their identity;
special controls show S. Both use existing size, spacing, font and color tokens.
The bar remains bounded on narrow displays and reveals the current active slot.

`WorkspaceCommand::Select` accepts regular identities; `ToggleSpecial` accepts
a special identity. The adapter rejects a mismatched command before opening a
native connection. The client rejects stale named/special destinations and actions
while observation is unavailable. Numbered shortcuts can create their regular
destination. An acknowledgement does not set the active indicator: a fresh
observed snapshot does that. No action writes settings.

```sh
lucent-cli workspace select 9
lucent-cli workspace select 'name:Writing room'
lucent-cli workspace toggle-special notes
lucent-cli workspace toggle-special default
```

These CLI named/special actions require the uniquely observed destination, matching
the bar. `default` requests the compositor default special overlay. The adapter
encodes numbers and regular names for `hl.dsp.focus`, and special names for
`hl.dsp.workspace.toggle_special`. It quotes Lua strings explicitly, preserving
quotes/backslashes as data. Numeric named workspaces are an unsupported Hyprland capability and are rejected
before dispatch. Its default `special`/`special:special` native aliases normalize
to one empty special name. Native selector prefixes belong to the adapter, not
the domain or rendering engine. See the [upstream dispatcher implementation](https://github.com/hyprwm/Hyprland/blob/main/src/config/lua/bindings/LuaBindingsDispatchers.cpp).

## Native transport lifetime and limits

The adapter follows Hyprland's two-socket interface. See the
[upstream IPC description](https://wiki.hypr.land/IPC/) and the
[monitor JSON implementation](https://github.com/hyprwm/Hyprland/blob/main/src/ipc/s1/Commands.cpp).
It registers the event connection before initial queries, so changes during
snapshot collection remain queued. Snapshot queries share a two-second absolute
deadline across connection, write and complete response reads. Individual
dispatch requests have the same two-second envelope. Each UTF-8 response is
limited to 4 MiB; over-limit/truncated data is never accepted as a valid snapshot.
Connections close after completion or failure.

Event framing retains partial bytes across socket read timeouts, recognizes
complete event names and coalesces relevant complete events in one read into one
refresh. A line is limited to 64 KiB. Focus, workspace move/rename, special
workspace, monitor and config-reload events trigger observation, with no parsing
of window-title payload as authority. A broken or over-limit stream reports
unavailability, retires its partial frame and retries after the existing recovery
delay. The healthy connection does not poll snapshots. Cancellation checks use
the bounded event-read wait; an in-flight snapshot may finish within its absolute
deadline. These are sequential observations, not an atomic compositor transaction.

## Evidence and remaining acceptance

Owned Unix socket tests exercise actual request/response connections, deadlines,
size rejection/recovery, subscribe-before-query ordering and a focus event split
across a read timeout. Injected desktop tests exercise pending discovery, explicit
selection, unchanged focus, connection recovery and independent error ownership.
Pure workspace policy tests verify separate monitor indicators without changing
the observed snapshot. Reviewed software-Vulkan bar references cover both
displays at 1×/2× in both palette directions.

A read-only snapshot of the running host compositor was executed successfully;
no display names, titles or serials enter public evidence. That check proves
query compatibility, not graphical multi-monitor behavior. VM deployment, global
shortcut placement, actual focus movement, native input/clipboard, reconnect,
secure coverage and current RSS/frame pacing remain deferred.
Per-monitor wallpaper selection, output arrangement and the wider
text/client/release ledger remain incomplete.
