# Native media players and controls

`MediaPort` is the domain contract for observing players and delivering explicit
commands. `Mpris` implements it in `lucent-services/src/media/`. The desktop's
composition root injects the adapter into its `Media` child. The independent
`media-service` example consumes the same contract without Wayland, widgets or
the desktop application.

This service controls applications that expose the standard
[MPRIS interface](https://specifications.freedesktop.org/mpris/latest/).
Wallpaper decoding is a different feature with a different port; see
[live wallpapers](live-wallpapers.md).

## Follow the data from startup to a painted control

1. `platform::desktop_adapters()` constructs `Mpris`. Construction performs no
   bus calls and creates no thread. `DesktopAdapters.media` holds the injected
   `Arc<dyn MediaPort>`.
2. The `Media` child declares a subscription. The framework owns the worker and
   cancellation lifetime. The worker calls `watch(emit, stop)`; a small adapter
   translates framework cancellation into the domain's `StopSignal`.
3. `Mpris` connects to the session bus. It subscribes before discovery, lists
   running MPRIS players and resolves their unique owners. It never starts a
   player merely to observe it. D-Bus names, object paths and property values
   stay inside the adapter.
4. The adapter reads capabilities and metadata, translates them into a
   `MediaSnapshot`, then emits observations after native signals and command
   completion. The framework delivers these values as child messages on the UI
   thread. There is no two-second metadata polling subprocess.
5. The child retains a manually selected player while its observed identity
   exists. Automatic selection prefers a playing player, then an available fallback. A
   manual choice is not stolen by another application's playback. Selection,
   open state, scroll offset and a seek preview are transient client state.
6. `view` borrows the child. It creates ordinary SDK text, buttons, images,
   scrolling and slider elements using semantic/component tokens. Views do no
   bus calls, file reads, downloads or image decoding. The bar and widget borrow
   the same selected-player presentation as the control panel.
   Playback controls stay in a fixed footer while artwork, metadata and progress
   scroll above them, so a small viewport keeps its transport actions visible.
7. An observation invalidates the relevant client surfaces. Layout and paint
   run through the shared framework; Vulkan uploads prepared artwork and draws
   primitives. Compositor frame callbacks pace dirty surfaces. A stopped or
   hidden progress display does not need a continuous animation loop.

## What the domain means

`MediaPlayerId`, `MediaTrackId` and `MediaOperationId` are nonzero transient handles. They are not a
persisted application ID, bus name or transport path. Removing a player or
replacing its owner retires the handle. A track change retires its track handle,
including a change away from and back to the same native path. Updating metadata
for the same observed track can preserve its handle. These values make stale
actions distinguishable from new actions aimed at the current player.

`MediaTime` stores nonnegative microseconds representable by a signed native
timestamp. `MediaRate` validates a finite nonzero speed and permits reverse
playback. A `MediaPositionSample` pairs the observed position with a monotonic
sample instant and rate. None of these values opens a connection or queries a
clock during pure policy evaluation.

`MediaSnapshot` contains current players and an optional current operation. A successful empty player list means no
running players. A global error means observation is unavailable. Each player
also distinguishes ready playback from unavailable metadata; one broken player
must not manufacture an empty successful library. Capability flags govern
individual controls. `CanRaise` is independent of general playback control.

`validate_media_command` checks a unique current player, readiness, capability
and, for seeking, the exact current track and known length bounds. The adapter
performs native revalidation before sending a method. Domain records remain
plain data; checked constructors enforce only their documented invariants.

## What happens when you click Pause or seek

The element captures `Pause(player_id)` for the player it actually displayed.
The child revalidates the command and schedules `MediaPort::start` on the existing
controls work lane. This call only admits one job to a bounded local mailbox and
returns its `MediaOperationId`; it does not wait for D-Bus, validate native target
ownership or guarantee that the method will succeed. Busy, offline or full
admission fails without dispatch. Contended local guards also fail immediately;
`start` never waits to acquire them. The observer's executor resolves the captured
handle to its current unique owner, revalidates it and calls that owner directly.
It never substitutes whichever player happens to be globally active later.
After a method reply, a background property refresh may still be pending. That
refresh does not reject an otherwise current follow-up action: the adapter
revalidates its capabilities and seek identity immediately before native dispatch.

Admission, a method reply and observed playback are distinct. The watcher reports
`MediaOperation { id, command, phase }`, whose phase is `InProgress`,
`Acknowledged` or `Failed(DomainError)`. Acknowledgement means the addressed
method replied successfully; playback changes only through observed properties.
The adapter retains the terminal result until it has been emitted, and will not
admit another operation before that publication. The desktop keeps one local
request and joins its revision, captured command and admitted operation ID with
that observation. A terminal watch update can arrive before the admission task
reply; unrelated IDs or commands cannot settle the request.

Close, selection changes and target retirement cannot revoke an admitted native
method. The client keeps waiting for its exact result while the adapter rejects
retired player/track authority. A whole-observer error retires the local request,
cancels any admission task still queued and reports an uncertain outcome. Late
admission/results cannot resurrect it. Unknown outcomes are never automatically
replayed, and there is no cancellation API that could imply an already-sent
method had no effect.

A slider drag previews a fraction locally. Only release submits
`SetPosition { player, track, position }`. Changing player, track or capability,
closing the panel or cancelling retires the preview so a stale release cannot
seek a different track. Native `SetPosition` includes the private current track
path as required by the
[Player specification](https://specifications.freedesktop.org/mpris/latest/Player_Interface.html).

The adapter's executor keeps routing owner and property signals while methods
are pending. A delayed old player cannot block discovery of its replacement.
Removing an owner retires its pending work and IDs; uncertain transport failures
are surfaced rather than silently retargeted.

## Progress and artwork have separate lifetimes

MPRIS does not emit ordinary position advances as property changes. The pure
`project_media_position(playback, now)` use case projects from the last sample,
status and rate, clamps to zero/known length and handles delayed or earlier
clock inputs. A visible selected playing track can request a one-second client
tick. Native `Seeked`, track/status/rate changes and completed actions refresh
the observed anchor. The adapter does not query the bus on every display tick.

`MediaArtworkSource` validates a bounded file/HTTP(S) location without opening
it. Decoded pixels belong to the presentation `AssetPort`, never `MediaPort` or
the pure domain. `NativeAssets` delegates to `services::images::media` on the
existing asset worker. The child admits one active preparation and the latest
pending demand; revision checks discard obsolete completions. Retired artwork
is released instead of accumulating a per-track image cache. Failed artwork
shows a fallback and an explicit retry without disabling playback.

Local artwork uses bounded regular-file reads. HTTP(S) artwork uses a bounded
literal-argument `curl` adapter, TLS verification and restricted redirects;
there is no embedded browser or HTTP client in the desktop view. Encoded input
is capped at 4 MiB, raster dimensions at 4096 per edge and 4,194,304 pixels,
then prepared at up to 512 pixels for display. Transport, decoding peaks,
prepared bytes, GPU texture bytes and process RSS have different scopes.

## Bounds, failures and acceptance

The native catalog admits at most 16 players and one admitted operation, either
waiting for native dispatch or awaiting its method result. A second request is
rejected while that operation is active or its terminal result is unpublished. Signals,
metadata and native reads have separate limits; these are not a total process
memory budget. Startup has one shared three-second deadline; each accepted
read/control has its own three-second absolute deadline. At most one property
read per player and one control are active; signals coalesce the latest refresh
demand behind a read already accepted. Failed reads have at most three recovery
attempts at two-second intervals until a new native signal resets demand.
Transport reconnection waits two seconds. Connection-close teardown follows
the existing native adapter pattern and has no separate explicit deadline.
Observing callbacks must remain responsive. `start` does no native I/O, but the
desktop still schedules its short local admission on a framework effect lane.

The desktop distinguishes loading, no players, unavailable observation,
per-player errors, command failures and artwork failures. Retained last-good
content is not current control authority. Reconnection creates fresh handles;
the user must act on current controls. Short media admission shares the existing
controls lane with other brief control work; audio volume stays on the audio
lane. Native media validation and method waiting live in the observing adapter,
so a stalled player method does not occupy the volume lane. The framework still
uses the same nine bounded workers; no media command worker was added. Existing
application activation can still occupy the controls lane for up to five seconds,
so media admission may wait behind that work; this does not block the audio lane.

Private D-Bus protocol fixtures, pure policy tests, injected child tests and
Vulkan visual checks are separate from actual player interoperability. A
controlled fixture does not establish browser, Spotify, VLC or physical-device
acceptance. Current source-specific results belong in the implementation
ledger and measurement report. Rate/volume editing, playlists, track lists and
OpenUri are outside this initial control surface and remain future work.

The accepted release from source `76176f83b19` passed real keyboard/pointer
interaction with two owned players in the test VM, including selected-player
compatibility keys, release-only seeking and retirement rejection. Its six-client
deployment preserved settings and wallpaper; see
[the source-specific report](../reports/measurements/lucent-native-mpris.json).
That release predates the operation-admission contract described above.
Corrected source `d2aed42199e` subsequently passed the owned native media suite,
reversible deployment and supported launcher-keyboard subset; see
[its separate report](../reports/measurements/lucent-media-admission.json).
Private-bus/injected checks prove admission/volume-lane separation. The VM suite
does not measure cross-service latency or establish third-party interoperability.

The native panel opens with `lucent-cli media open`, toggles with
`lucent-cli media toggle` and closes with `lucent-cli media close`. Its surface role is `media-controls`; the desktop
widget's existing role remains `media`. Inspector IDs and command arguments are
transient and must not be saved as preferences.

```sh
cargo run --manifest-path lucent/Cargo.toml --locked -p media-service -- --observe --session-bus --seconds 10
cargo test --manifest-path lucent/Cargo.toml --locked -p lucent-services media_protocol -- --ignored
```

The consumer requires an explicit `--session-bus` or `--bus-address` choice.
Observation prints counts/state by default. `--stdio` opts into local metadata
and newline-delimited JSON commands, each limited to 4096 bytes. A seek command
uses current handles and microseconds, for example
`{"command":"seek","player":1,"track":2,"position_us":30000000}`.
Those numbers are illustrative, not stable identities.

A successful command line reports `{"admitted":true,"operation":ID}`. Watched
snapshots separately report that operation's command, phase and typed error.
Either line may arrive first; admission alone never reports native success.

Compatibility media keys and shorthand `lucent-cli media play-pause`, `next`
and `previous` capture the current selected player during command admission.
Explicit `media pause PLAYER` and `media seek PLAYER TRACK MICROSECONDS`
commands retain their named targets. No shell media action invokes global
`playerctl`; headless distribution tools outside this adapter may still use it.
