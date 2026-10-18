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

`MediaPlayerId` and `MediaTrackId` are nonzero transient handles. They are not a
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

`MediaSnapshot` contains current players. A successful empty snapshot means no
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
The child revalidates the command and schedules an effect on the existing audio
work lane. The native adapter admits it to a bounded queue, resolves the captured
handle to its current unique owner and calls that owner directly. It never
substitutes whichever player happens to be globally active later.

Admission, a method reply and observed playback are distinct. The child keeps
its pending/error state separately and waits for observations to change the
displayed playback status. A successful reply does not rewrite the snapshot.
Unknown outcomes are reported and not automatically replayed. Player retirement
cancels local pending authority; it cannot undo a method already delivered to
the application.

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

The native catalog admits at most 16 players, with 32 waiting commands. Signals,
metadata and native reads have separate limits; these are not a total process
memory budget. Startup has one shared three-second deadline; each accepted
read/control has its own three-second absolute deadline. At most one property
read per player and one control are active; signals coalesce the latest refresh
demand behind a read already accepted. Failed reads have at most three recovery
attempts at two-second intervals until a new native signal resets demand.
Transport reconnection waits two seconds. Connection-close teardown follows
the existing native adapter pattern and has no separate explicit deadline.
Observing workers
must keep their callbacks responsive and never call a synchronous control
method from the owning observation callback.

The desktop distinguishes loading, no players, unavailable observation,
per-player errors, command failures and artwork failures. Retained last-good
content is not current control authority. Reconnection creates fresh handles;
the user must act on current controls. Media and audio currently share one
ordered effect lane; a stalled media method can delay a queued volume action
until its bounded call returns. Rendering and the urgent session lane remain
independent. Independent audio/media command scheduling remains follow-up work.

Private D-Bus protocol fixtures, pure policy tests, injected child tests and
Vulkan visual checks are separate from actual player interoperability. A
controlled fixture does not establish browser, Spotify, VLC or physical-device
acceptance. Current source-specific results belong in the implementation
ledger and measurement report. Rate/volume editing, playlists, track lists and
OpenUri are outside this initial control surface and remain future work.

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

Compatibility media keys and shorthand `lucent-cli media play-pause`, `next`
and `previous` capture the current selected player during command admission.
Explicit `media pause PLAYER` and `media seek PLAYER TRACK MICROSECONDS`
commands retain their named targets. No shell media action invokes global
`playerctl`; headless distribution tools outside this adapter may still use it.
