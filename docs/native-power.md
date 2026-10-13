# Native power service and panel

`PowerPort` describes observations and explicit power operations. `PowerServices`
implements it in `lucent-services/src/power/`. The desktop composition root injects
that adapter into the native `Power` child; `power-service` consumes the same
library independently of Wayland, rendering, desktop components or Omarchy UI.

The current implementation covers battery information, power profiles, built-in
backlights and logind sleep/restart/shutdown requests. VM and physical-device
acceptance are deferred. Local protocol and visual results are recorded only
after their checks run; implementation does not establish hardware acceptance.

## Domain values and policy

`Percentage` accepts only finite values in the inclusive 0–100 range.
`PowerObjectId` identifies a current profile or backlight in this adapter
instance; removal and transport replacement retire handles. Never persist them
or derive them from a transport path. `PowerOperationId` identifies one admitted
operation, not a hardware device or session.

`PowerSnapshot` separates independently available capabilities. A ready battery
value of `None` means no display battery or UPS. It differs from an unavailable
battery service; profiles and session controls can still be available. Ready
empty profile/backlight collections are also distinct from unavailable services.
Records remain public data; the checked types enforce their stated invariants.

Battery data comes from UPower's aggregate display device. The client uses a
coarse battery level when provided instead of presenting the accompanying
percentage as an exact reading. Estimates remain optional. See the
[UPower interface](https://upower.freedesktop.org/docs/UPower/) and
[device properties](https://upower.freedesktop.org/docs/Device.html).

`validate_power_command` checks current targets and pending operation state.
Session policy accepts an available action or one requiring authorization;
denied/unsupported actions remain disabled. The native service makes the actual
authorization decision. Policy performs no filesystem, D-Bus or rendering work.

## Startup, observation and native ownership

1. Construction starts no worker. The composition root supplies an
   `Arc<dyn PowerPort>` and a subscription calls `watch(callback, stop)`.
2. One observing worker connects to the system bus, subscribes to exact service
   owner changes, resolves unique owners and subscribes to their relevant
   property/sleep signals. Ownership changes close the connection and retire its
   handles before reconnecting; native calls never silently target a replacement.
3. The adapter reads UPower, power-profiles-daemon and logind capabilities. The
   modern profile endpoint is preferred; the legacy `net.hadess.PowerProfiles`
   endpoint is supported. Native profile choices, active state and degradation
   determine the UI. See the
   [power-profile API](https://upower.pages.freedesktop.org/power-profiles-daemon/gdbus-org.freedesktop.UPower.PowerProfiles.html).
4. A private backlight catalog reads bounded numeric sysfs values and keeps
   directory identity, maximum and kernel control names outside the domain.
   Kernel backlight uevents wake refreshes; the idle tick checks cancellation,
   rather than polling power state. File reads run on the service worker, never
   on the Wayland/UI thread. D-Bus deadlines do not make kernel file reads
   cancellable; driver faults remain a hardware acceptance concern.
5. Backlight writes use logind's `Session.SetBrightness` for this user's active
   Wayland session. The adapter validates session ownership and the current
   backlight identity. It installs no permission workaround or permissive policy.
   System power actions retain logind authorization and inhibitor handling.
   See the [logind API](https://github.com/systemd/systemd/blob/main/man/org.freedesktop.login1.xml).

One observer owns transport lifetime. Admission holds at most 32 waiting
requests and permits one native operation at a time. The query/admission budget
is five seconds; interactive profile/session operations permit 120 seconds.
Signal queues and catalogs are bounded by count, not a total memory budget.
Metadata decoding, kernel/service state and driver allocations remain separate.
Observer callbacks must stay responsive and must not synchronously call the
port from its own owner thread.

## What happens after a click

The view describes SDK elements and emits a typed child message. The child
validates current policy and submits an effect on the existing controls lane.
`control(command)` returns a `PowerOperationId` after admission, before any
interactive authorization completes. The native local executor runs the method
while observation keeps routing signals. The client waits for operation phase
and actual observed profile/brightness changes; it never rewrites its snapshot
after an acknowledgement.

Sleep/restart/shutdown need an explicit native confirmation in the desktop.
Authorization dialogs continue through Lucent's existing polkit client. Closing
the panel cancels local confirmation and unsubmitted brightness demand; it does
not promise to undo an already admitted system action. Uncertain errors retire
the connection without automatic replay.

Brightness input keeps at most the latest desired value while a native write is
pending. The desired preview belongs to the child, separate from the observed
snapshot and persisted preferences. Retired targets, unavailable observation,
errors or close discard unsubmitted demand.

## Run the independent consumer

```sh
cargo run --manifest-path lucent/Cargo.toml --locked -p power-service -- --observe --seconds 10
cargo test --manifest-path lucent/Cargo.toml --locked -p lucent-services power_protocol -- --ignored
```

Default observation is read-only and prints counts/status. `--stdio` opts into
detailed local metadata and JSON control; each line is limited to 4096 bytes.
An isolated test requires both `--bus-address` and `--backlight-root`. IDs below
are illustrative; use current snapshot handles. Admission returns an operation
ID and later snapshots report completion.

```json
{"command":"profile","target":2}
{"command":"brightness","target":3,"value":55}
```

The native desktop opens with `lucent-cli power open`; `power toggle` and
`power close` share the same child. Existing compatibility routes open this
panel. The shell does not load a toolkit or delegate power UI to Omarchy.
