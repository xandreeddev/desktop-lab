# Independent native media consumer

This consumer uses `MediaPort` and `Mpris` without the desktop, Wayland or a
renderer. Select the bus explicitly; help/default arguments connect to no bus.

```sh
cargo run --manifest-path lucent/Cargo.toml -p media-service -- --observe --seconds 5 --session-bus
cargo run --manifest-path lucent/Cargo.toml -p media-service -- --stdio --seconds 60 --bus-address OWNED_FIXTURE_ADDRESS
```

Observation prints readiness and counts only. `--stdio` explicitly enables
current player/track IDs and titles plus JSON commands, one per line:

```json
{"command":"pause","player":1}
{"command":"seek","player":1,"track":2,"position_us":12000000}
{"command":"quit"}
```

Use observed IDs rather than the illustrative numbers above. `play`, `next`,
`previous`, `stop` and `raise` take the current player ID. Replies acknowledge
method calls; actual playback remains observed state. Commands are limited to
4096 bytes. `--seconds` ends observation/input admission even with blocked input;
an admitted native operation may take until its deadline before shutdown finishes.
An explicit fixture address never falls back to the host session bus.

MPRIS properties/Seeked events refresh position samples; consumers can project
playing progress from their monotonic sample/rate. The adapter does not poll
ready players. Failed reads have at most three bounded recovery attempts before
waiting for a new native signal. Rate/volume settings, playlists and OpenUri are
outside this slice. Protocol fixtures are private test-owned D-Bus daemons;
their existence does not establish native application or VM acceptance.
