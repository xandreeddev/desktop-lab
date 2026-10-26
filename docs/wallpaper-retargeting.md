# Same-asset playback retargeting

This is a separate library capability and an independent observer prototype.
The desktop still uses the existing static `WallpaperPlaybackPort::play` API.
The new source has not been compiled or measured; it makes no memory, cadence
or improvement claim. Earlier observer measurements use different source.

`RetargetablePlaybackOptions` contains only frame-rate and looping policy.
`WallpaperTargetPort` supplies the sole physical target as a
`WallpaperTargetDemand`: a checked `WallpaperPixelTarget` and a nonzero
`WallpaperTargetRevision`. These are transient domain values, independent of
monitor selection, saved preferences, Wayland and rendering.

`LatestWallpaperTargetAdapter` implements that port in services. A control
writer calls `set(expected_revision, target)`. A stale expected revision fails;
the same target with the current revision is a no-op. Changed demand increments
with `checked_next`, which fails before mutation at exhaustion. The adapter
stores one latest value, not a command queue. Decoder reads use `try_lock` and
return `None` during contention. Initial `None` is an explicit unavailable
error before decoder admission; later `None` keeps the last admitted demand.
Regressing snapshots and equal-revision/different-target snapshots fail.

`play_retargetable` is an additional port method. Unsupported adapters report an
explicit unavailable capability error; they never call static play as a hidden
restart fallback. `NativeVideoWallpapers` validates the same owned asset as
static play and consumes target snapshots during its existing decode work.
There is no new control-polling thread or timer worker.

Within a playback cycle, target changes keep the admitted input, decoder,
decoder threads, source shape, presentation timeline, source clock and I/O
deadline owner. Only a change in actual converted pixel extents retires the
converter and its bounded publication pool. The old pool drops before the new
one allocates; already published immutable pixel owners remain valid. Alpha,
palette and unknown-alpha formats keep the existing source-size fallback;
their changed target revision is still attached even when no converter resize
is needed. Source aspect, timestamps, straight alpha, row validation and
filtering policy are unchanged.

Every published `TargetedMediaFrame` carries the revision captured before
conversion. A new demand can arrive during conversion. The consumer must compare
the result to its current intent and discard stale results; it must never
relabel old pixels with a newer revision. Converter lifetime no longer decides
whether a decoded frame is the first source prefix: source admission separately
tracks dimensions/format and the first decoded PTS. Retargeting therefore cannot
reclassify a later missing timestamp as an unseekable initial prefix.

EOF behavior remains the existing source policy: fully drain reordered frames,
hold the final source duration, and reuse an eligible live H.264 session through
the bounded seek/flush path. Unsupported or failed EOF seeks retain the existing
reopen path. Target updates themselves do not reopen. One port call does not
claim that every codec/EOF condition retained a single native context.

## Independent comparison

The services `video-allocation-observer` accepts optional private JSON
`"retarget": true`. The default remains the measured three-reopen sequence.
The retarget mode makes one play call on one worker for FHD → 4K → FHD. The
coordinator updates only the target adapter and rejects stale revision results.
It keeps one latest allocation across phase boundaries until a valid
replacement arrives, then cancels, joins and drops the final latest once.
An unsupported, failed, early-returning or unjoined producer never authorizes
another playback worker. No phase output is emitted until cleanup completes.

Each target has a bounded five-second readiness wait and three process-memory
points, with no allocator query while playback is alive. The observation period
starts after the first matching target frame is observed. These phase clocks
include different startup/readiness work from the old reopen mode, so
publication counts or elapsed-time differences are not FPS or a speedup.
Per-target RSS includes the live decoder and held latest frame. Those points
must not be compared directly with the old after-join/after-drop measurements.

With `allocator_stats` enabled, the whole retarget run has three allocator
observations: before the worker, after its exact join with the latest held,
and after that latest drops. Only the final stopped-state observations offer a
comparable allocator-accounting scope. Free arena bytes remain distinct from
resident RSS; no allocator trimming, overrides, allocation padding, process
offloading, resolution reduction or cadence reduction is used.

The new `retarget_phase`/`retarget_finished` output requires a separately
reviewed, source-bound runner. Existing frozen runners and receipts must not be
rewritten or reused as proof for this mode. Native output/pixel/EOF/cancellation
validation and the same-process memory comparison must precede desktop wiring.
