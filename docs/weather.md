# Native Weather: configuration and execution

Lucent implements its own Weather panel, widget and passive bar display. It uses
ordinary framework components and generated design tokens. Weather data comes
from an injected service adapter; neither the renderer nor the compositor knows
about forecasts.

Local strict checks, reviewed visual coverage, the source-pinned release and
guarded expected legacy migration on the existing VM pass. Scoped native
opening/focus/input-release checks and one production forecast observation also
pass. Native form mutation and broader provider/failure/retry behavior remain
open. The measured software-Vulkan live-video workload exceeds the 300 MiB RSS
target; its current sample and remaining pacing limits are recorded below. See the
[sanitized evidence](../reports/measurements/lucent-weather.json) for exact sources
and scopes. These are separate acceptance steps.

## Configure Weather

Open **Weather and location** in launcher Commands, use the weather widget's
explicit action, or run `lucent-cli weather open`. The center date/clock/weather
capsule stays informational and does not open a menu.

Enter a label and numeric latitude/longitude, select °C or °F, then choose
**Apply and enable**. Editing the form performs no network request. Applying a
location, explicitly refreshing, or running the configured forecast subscription
sends those coordinates to Open-Meteo. No IP, GPS or automatic city detection is
performed. Display-unit changes reuse canonical Celsius observations without a
new forecast request for the same location.

**Turn weather off** stops requests and saves an explicit disabled preference.
The IPC actions are `weather open`, `weather close`, `weather refresh` and
`weather off`. Refresh failures and preference-save failures have separate retry
actions. Keep location-bearing settings and screenshots private.

Weather preferences live in the normal private `desktop.json` aggregate. When
weather preferences are absent, Lucent may read the older `weather.json` once.
A successful legacy import returns the same save intent as an ordinary form.
Saved disabled preferences outrank legacy configuration. Invalid legacy files
remain untouched and report a correctable error. If the desktop settings cannot
be loaded safely, Weather pauses instead of bypassing saved authority.

## How the code starts

A Rust *crate* is a compiled package. A Rust module such as `mod weather` groups
source code inside that package. It does not register a plugin with Linux or
cause a separate source file to be loaded when you open the panel. The operating
system loads the compiled `lucent-desktop` executable through the existing
Lucent user service and supervisor.

The first explicit application call is in
[`main.rs`](../lucent/apps/lucent-desktop/src/main.rs):

```rust
lucent_wayland::run(Desktop::new(platform::desktop_adapters()))
```

Read this from the inside out:

1. [`platform::desktop_adapters`](../lucent/apps/lucent-desktop/src/platform.rs)
   constructs production implementations, including `OpenMeteo`.
2. [`Desktop::new`](../lucent/apps/lucent-desktop/src/desktop.rs) passes the weather
   implementation into `Weather::new`. `Arc<dyn WeatherPort>` means a shared
   implementation accessed through the port's contract, not a dynamic plugin.
3. The desktop loads settings and calls `weather.configure` with saved authority
   and whether saving is available. That selects legacy loading, a configured
   forecast, or explicit disabled state.
4. `lucent_wayland::run` manages the native event loop. The root explicitly maps
   the child's subscriptions and messages into the application's message type.
   No engine scans component structs to discover work automatically.

## Which layer owns what

| Layer | Responsibility | Source |
| --- | --- | --- |
| Domain values | Checked coordinates, location labels, temperature units, current conditions and three dated forecasts | [domain/weather.rs](../lucent/crates/lucent-domain/src/weather.rs) |
| Domain port | `WeatherPort::legacy_preferences` and `forecast(checked coordinates, StopSignal)` | [domain/weather.rs](../lucent/crates/lucent-domain/src/weather.rs) |
| Adapter | `OpenMeteo`: legacy files, curl transport, provider JSON/codes and timestamp conversion | [services/weather.rs](../lucent/crates/lucent-services/src/weather.rs) |
| Desktop child | Form, selection/clipboard lifetime, scrolling, opening motion, refresh generations and last-good presentation | [weather/mod.rs](../lucent/apps/lucent-desktop/src/weather/mod.rs), [weather/view.rs](../lucent/apps/lucent-desktop/src/weather/view.rs) |
| Desktop root | Inject adapters, route child messages and save `DesktopSettings.weather` through the normal persistence coordinator | [desktop.rs](../lucent/apps/lucent-desktop/src/desktop.rs), [persistence.rs](../lucent/apps/lucent-desktop/src/persistence.rs) |
| Framework | Generic elements, input, layout, subscriptions, animation and GPU/native surfaces | [framework library guide](../site/src/pages/docs/library.astro) |

A **port** is the contract describing an operation. An **adapter** is the
implementation connected to the outside world. A fake adapter can return known
observations in a test without HTTP. The production adapter does not own which
panel opens, how a card looks, or when the root saves preferences.

## One request, update and frame

The child declares a stream subscription named `weather:<opaque generation>`.
The runtime retains the producer while that identity stays the same. An idle
location change or explicit refresh starts a new generation and retires the old
producer. During a busy forecast request, changes and refreshes instead coalesce
into one latest demand. Old-location presentation is discarded immediately; a
reply for coordinates no longer desired is not displayed. When that bounded
request completes, the child starts the latest desired generation and the old
producer retires. This replaces the existing weather slot and adds no blocking
task lane.

The producer sends `Started`, calls the adapter away from the UI thread, then
sends `Observed` with the result. It sleeps between fifteen-minute refreshes.
The root wraps those values in `Message::Weather`; the child accepts only the
current generation. Late replies cannot replace a newer location. Domain
`StopSignal` exposes cancellation without depending on framework message types.

For Apply, the child validates the draft and returns `Intent::Save(preferences)`.
The root updates `DesktopSettings.weather` and requests its existing ordered
save coordinator. The view does not call the settings adapter. **Saved** means
that adapter completed the write; a loading error or failed save remains visible.
This is not a guarantee that an abrupt power loss cannot lose a write.

After an accepted message, effects invalidate the affected views. The ordinary
panel has the separate `weather-controls` role; the `weather` role still names
the catalog widget. Editing, selection, scroll and closing frames redraw only
the panel. Report/preference/loading changes also invalidate the shared bar and
widget groups on their monitor hosts.

The view constructs ordinary `Element` rows, columns, inputs, buttons, clipped
scrolling and text. The framework lays them out, records paint commands and
submits dirty surfaces through its renderer when frame admission allows. Vulkan
executes the GPU work; it does not decide where a forecast goes or interpolate
the child's state. The child's `Motion` values interpolate on the CPU at supplied
times, using generated duration/easing tokens. Padding, colors, icon sizing and
rounded shapes come from the design recipes. The widget keeps its padded content
inside a separate child, so the outer card's positioning cannot erase that inset.

Closing retires the form IDs and clipboard tickets and releases foreground input.
A native close sample observes keyboard and pointer set to none while the closing
surface is still present; it does not measure fade timing. Configured weather
continues updating for the widget and bar. Native editing, scrolling, clipboard
transfer and animation quality remain separate acceptance.

## Data checks and failures

`WeatherCoordinates` enforces finite geographic ranges on construction and
JSON deserialization. `WeatherLocation` trims and bounds its label to 96 UTF-8
bytes and rejects controls. `WeatherTemperature` stores finite Celsius, rejects
conversion overflow and values below absolute zero. `TemperatureUnit` is an
explicit persisted enum. `WeatherDay` checks its minimum/maximum;
`WeatherSnapshot` admits exactly three consecutive forecast dates.

The domain condition enum contains meanings such as freezing drizzle and
freezing rain. `OpenMeteo` translates WMO codes into those meanings; unknown codes
stay Unknown. It requests Celsius, Unix timestamps, a timezone derived by the
provider from the coordinates (`timezone=auto`), and three daily high/low/condition
values. There is no timezone form setting. Current observation times remain UTC;
forecast dates use the response's fixed UTC offset. These are provider model
conditions, not station readings or a full DST-aware hourly timeline.

The Rust adapter invokes curl with default curlrc loading disabled as its first
argument. Coordinates go through curl's stdin configuration, never process
arguments. Proxy environment support remains available. Provider bodies, URLs
and coordinates are absent from public error messages and the inspector.
Legacy input is at most 4 KiB and must be a regular file opened with
`O_NOFOLLOW | O_NONBLOCK`. Provider responses are at most 64 KiB. Curl has a
four-second deadline with a five-second outer bound and cooperative cancellation;
the owned child is killed and reaped on cancellation or failure.

A failed same-location refresh retains the last-good report and its age. Loading,
refreshing, disabled/unconfigured and failed states remain distinct. Data becomes
stale thirty minutes after fetching or the observation; `~` marks stale/failed
retained data in the bar. Changing location discards old-location presentation.
Only one in-memory report is retained; restarting provides no disk offline cache.

## Executed validation and remaining work

Strict source `2723503b` passes 736 regular Rust tests, with 76 ignored, plus 104
Python checks, 576 generated tokens, formatting, strict all-target Clippy and the
independent example help command. Owned tests cover checked values, malformed
responses, legacy bounds, last-good recovery, generations/coalescing, disabled
precedence, form/clipboard lifetime and root preference behavior. A real local
curl fixture checks default-config isolation without HTTP or a HOME change.

All 56 new Weather references were reviewed in dark/light at 1×/2×. Initial
widget padding/icon clipping and condition-row alignment were rejected and fixed
in the client, then reviewed again. No framework/card or tolerance workaround
was used. Sixteen existing references were separately reviewed: the Commands
panel grows for a ninth command, and widget fixtures show the new child state.
The widget-drag-grid fixtures also contain disclosed calendar antialias speckles
and subtle shadow raster differences; their geometry/grid/selection stay visually
unchanged. The right-monitor changes are exactly confined to Weather contents.

Local visual coverage combines **752 passes from the full run at `ad4b0a07`**
with **16 focused passes at `579c62cb`**, after committing only the specifically
reviewed references. The Rust source is unchanged. The original full run exited
101; it has not been relabeled as a single final full pass. Focused comparisons
did not update references, and no tolerance changed. Final-head CI will run all
768 independently before merge.

The private deployment transaction's actual storage code also passes seven cases
and 35 operations in owned temporary homes: checkpoint bytes/inode, expected
migration admission, stable observation, rollback and rejection of independent
edits or unsafe restore. External artifacts and quiescence are fakes in that
test. It establishes storage ownership behavior, not a running VM or service.

The release from `579c62cb` and guarded desktop/CLI deployment pass. The
existing service restarts gracefully and remains stable for 37.43 seconds with
the accepted supervisor and `KillMode=mixed` unchanged. Only the expected Weather
preference migration changes settings semantics; desktop settings bytes change
intentionally. Legacy weather bytes/inode, unrelated preference semantics,
wallpaper/source/theme exports and initially empty clipboard/history are
preserved. No rollback was needed or exercised on the VM; the fake-home rollback
cases above retain their separate scope.

A separate production-native check passes center-bar passivity, CLI opening and
inert panel padding, exclusive Audio-to-Weather switching, Escape, the actual
widget action, Tab into a field and outside-click closing. One close sample
observes pointer/keyboard none before the surface disappears; it establishes no
fade duration or smoothness. Exact settings bytes/inode, legacy/media/export
artifacts, initially empty clipboard/history, cursor/dock state, monitor/workspace
and the supplied process are preserved during this workload. Four private static
captures are reviewed for readable current conditions, three forecast rows,
padding, field focus and closed UI. Location-bearing captures are not published.

The configured migration subscription also supplies one successful production
forecast: inspection reports configured, idle, fresh last-good data with no
failure, Celsius units and a closed panel. This is one observed fetch, not broad
provider interoperability or failure/retry proof.

The native helper deliberately skips invalid Apply and performs no text edit,
clipboard transfer, preference mutation or explicit provider refresh. Actual
Apply/off/units/reload, native validation, scroll/transfer and failure/retry tests
remain open, along with animation quality, frame pacing, native multi-output and
hardware acceptance. Current delivery and final-head validation are tracked in
[PR #78](https://github.com/xandreeddev/desktop-lab/pull/78) and the sanitized report.

Documentation at `f4dd34b2` passes Astro with zero diagnostics and both root/Pages
builds and link checks: 13 pages and 489 links/assets under each prefix.

### Current resource sample

A 120.001-second sample with 121 observations uses the same deployed process on
software Vulkan (llvmpipe), four virtual CPUs and one 1920×1080 output. One active
3840×2160 video is fitted to 1920×1080; its retained frame payload is 8,294,400
bytes, a separate quantity from process RSS or total GPU allocations.

| Measurement | Observed value |
| --- | --- |
| Process RSS, mean / maximum | 424.12 / 424.15 MiB |
| Process CPU, percent of one logical core | 148.649% (about 1.49 cores) |
| Service cgroup CPU, percent of one logical core | 148.718% |
| 300 MiB process RSS target | Not met |

Settings bytes, wallpaper, monitor/workspace and the same process/binary remain
preserved; the sampler performs no guest UI, settings, clipboard, idle-marker or
service writes. No heavy host job runs during the interval. Process RSS may
include software driver mappings and excludes decoder/helper processes; service
CPU covers its cgroup. The animated workload does not assess the static-idle CPU
target or isolate Weather cost. This is a current sample, not a matched old/new
comparison. Dock submissions do not advance; the other frame deltas span
inspector boundaries and prove neither FPS, scanout, video pacing nor smoothness. City search, persistent offline cache, alerts and hourly graphs are
outside this slice. Shaping/bidi/fallback/IME remain open; accessibility stays
deferred. Existing Notes and performance reports keep their own sources and scopes.
