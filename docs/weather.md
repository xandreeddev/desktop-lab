# Native Weather

This source slice adds current conditions and a three-day forecast from
[Open-Meteo's primary API documentation](https://open-meteo.com/en/docs).
Compilation, behavior tests, rendered references and native acceptance are
**pending**. Written fixtures are not executed evidence.

## Domain and adapter

`WeatherCoordinates` checks finite latitude/longitude and their geographic
ranges on construction and deserialization. `WeatherLocation` adds a bounded
display label. `TemperatureUnit` persists an explicit `celsius` or `fahrenheit`;
`WeatherTemperature` stores finite Celsius values and rejects conversion overflow
and temperatures below absolute zero. Unit changes use pure conversion.

`WeatherCondition` contains provider-neutral meanings. `WeatherDay` checks its
minimum/maximum, and `WeatherSnapshot` admits exactly three consecutive dates.
The domain contains no HTTP, files, renderer types or screen policy.

`WeatherPort::legacy_preferences` performs a read-only compatibility import.
`WeatherPort::forecast` receives checked coordinates and a domain `StopSignal`.
The executable supplies `OpenMeteo`; tests inject adapters. Its implementation
owns provider codes, current/day arrays, unit checks and timestamp conversion.
Unknown codes remain Unknown rather than becoming clear sky through narrowing.

The adapter explicitly requests Celsius, Unix timestamps, the selected location's
timezone and three daily high/low/condition values. Forecast dates use the
response's UTC offset; current observation timestamps remain UTC. The provider
uses model conditions and a fixed response offset, so this is not a station
reading or a DST-aware hourly timeline.

Legacy input is limited to 4 KiB, opened with O_NOFOLLOW/O_NONBLOCK,
and must be regular. Responses are limited to 64 KiB; curl has a four-second
deadline, with a five-second outer bound and cooperative cancellation. The
owned process is killed/reaped on cancellation or failure. Coordinates are sent
through curl's stdin configuration, never command arguments. Provider bodies,
URLs and coordinates are absent from error messages and inspector output.

## Client and scheduling

The `weather::Weather` child owns its controlled location form, clipboard tickets,
opening motion, scroll, request generation and one in-memory last-good report.
Views borrow those values and generated design recipes; they perform no I/O.
The root retains `DesktopSettings.weather` and is the sole preference writer.

One `weather:<opaque generation>` subscription replaces the old weather polling
slot. It emits Started/Observed and sleeps between fifteen-minute refreshes.
Refreshes during a request coalesce into one latest demand. Location changes
discard old-location presentation and reject late results; display-unit changes
retain the canonical report and make no HTTP request. No tenth task lane is added.
Closing the panel retires its form/clipboard authority while configured weather
continues updating for the widget and passive bar.

Missing preferences permit one legacy read. A successful import returns a normal
root save intent. An explicit saved disabled preference forbids later import.
Invalid legacy files remain untouched and expose a correctable error; failed
desktop settings loading pauses weather rather than bypassing saved authority.

A failed refresh retains same-location last-good data and its timestamp. The
panel distinguishes initial loading, refreshing, disabled/unconfigured and errors;
age/staleness does not pretend a failed request succeeded. Data is stale after
thirty minutes since fetching or the reported observation. The bar remains
informational; `~` marks retained stale/failed data. A restart has no offline cache.

## User configuration

Open **Weather and location** in launcher Commands, press the weather widget's
explicit action, or run `lucent-cli weather open`. Enter a label and numeric
coordinates, choose °C/°F, then **Apply and enable**. Typing performs no network
operation. Applying or refreshing discloses the selected coordinates to
Open-Meteo. No IP/GPS lookup or automatic location detection occurs.

**Turn weather off** stops requests and persists explicit disabled authority.
The center clock/weather bar does not open a menu. Refresh and save failures have
separate retry actions. `weather refresh`, `weather close` and `weather off` are
also available through IPC. Keep location-bearing preferences private.

## Written validation and limits

Domain/adapter cases cover finite/ranged inputs, conversion overflow, ordered
dates, malformed arrays/units and bounded read-only legacy import. Client cases
cover last-good recovery, opaque generations, refresh coalescing, explicit off,
legacy precedence, invalid forms, units, readonly preferences and retired clipboard
responses. Visual fixtures cover ready/night/rain/loading/offline/stale/configuration,
narrow panels and actual resized widget bounds in both palettes at 1×/2×.

All new Rust tests and GPU/native checks remain unexecuted until the shared build
owner runs them. City search, persistent disk cache, alerts and hourly graphs are
outside this slice. Accessibility remains separately deferred; text shaping/IME
are existing framework limitations.
