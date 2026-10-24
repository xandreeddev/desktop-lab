# Presentation feedback across native surface recreation

`LUCENT_PROFILE_PRESENTATION=1` adds diagnostic identity to the existing
presentation inspector. It requests feedback only for the submissions already
made by the renderer. It does not enable CPU frame profiling or change redraws,
callbacks, geometry, frame demand, rendering or native presentation mode.

Each inspected surface has `native_generation`: an opaque positive integer for
that native lifetime, or `null` when presentation capture is disabled. Scope it
to the captured runtime process and epoch. A recreated logical address receives
a different generation. Generations never wrap: exhaustion is reported and new
feedback requests are skipped, while rendering continues. This counter is
independent of the optional CPU `frame_pacing` trace, which is not enabled for a
presentation-only run. Protocol-level numerical object IDs can be reused and
are not exported as lifetime tokens.

Every existing `surfaces[].presentation.samples[]` entry now carries its
`native_generation`. The existing logical submission/history continuation is
preserved: a replacement may still expose records completed before recreation.
Group by each record's generation rather than assuming all records belong to
the currently inspected native surface.

Feedback arriving after retirement increments the existing
`presentation_feedback.retired_surface_results` counter. It also enters a
separate bounded `presentation_feedback.retired.records` ring. It never updates
the replacement's active history. A record contains:

- `record`: cumulative completion sequence, separate from frame IDs;
- `surface_id`, `native_generation`, `frame`, `requested_at_ms`;
- `outcome`: `presented`, `discarded` or `invalid`;
- `presentation`: the existing timestamp/clock/refresh/flags/stream-reference
  fields for a valid presented result, otherwise `null`.

The retired ring retains at most 120 completions. `completed`, outcome counters,
`evicted`, `dropped`, `truncated` and `counter_exhausted` make lost history
explicit. Active-history presentation/discard/invalid counters retain their
existing behavior. Pending feedback remains bounded to 256 requests globally;
`pending_records` identifies outstanding generation/frame pairs and whether
their native owner has retired. `skipped`, `unmatched_results`, `pending_lost`
and generation exhaustion prevent a collector from claiming complete coverage.
Unresolved pending requests are not completed frames. Runtime teardown clears
pending metadata and counts it as lost; no final inspector reply or persisted
receipt is promised after process/connection exit.

The runtime retains only protocol object identity and scalar submission context
in its pending map, and scalar context in completed records. It never stores a
`Native`, renderer or layer owner there. SCTK 0.21.1's existing feedback userdata
has a `WlSurface` proxy clone; this does not retain the displayed layer/window.
`LayerSurfaceInner::drop` explicitly destroys the layer role, and its owned
`compositor::Surface::drop` explicitly destroys `wl_surface`, even when proxy
clones remain. SCTK forwards terminal feedback using that proxy's original
identity without requiring a live surface. The maintained SCTK binding and
dispatch are retained; no second protocol implementation is introduced.
The existing last-good presentation handoff and its lifetime are unchanged.

Collectors should merge active and retired records by runtime, native
generation and frame, detect counter/sequence loss, and keep presentation-clock
intervals separate from local request timestamps and host RPC bounds. These
records do not mark exact application animation start/end, establish visible
pixels, or turn submission counts into FPS. Hardware timing scope still depends
on the recorded protocol flags. A launcher trial can describe its command-bound
window and explicit native-lifetime segments without inventing animation or
GPU/scanout timestamps.

The focused tests cover disabled capture, generation exhaustion, continued
logical histories with late feedback, discard/invalid scope, bounded eviction,
pending loss, and explicit destruction despite a proxy clone. The owned socket
fixture uses no compositor or GPU. Source preparation and these fixtures do not
establish native timing or performance acceptance.

```sh
cargo test --manifest-path lucent/Cargo.toml -p lucent-wayland --lib presentation::tests
```
