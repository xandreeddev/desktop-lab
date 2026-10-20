# Frame callback and draw-attempt timing

`LUCENT_PROFILE_FRAMES=1` enables an additional runtime-only `frame_pacing`
inspector section. It leaves the existing successful `frame_profiles` schema
unchanged. The trace observes existing work: it requests no redraw, frame
callback, timer, flush, roundtrip or polling worker, and changes no present mode.

The trace was built and exercised in the separate `91b964ae` diagnostic source.
This integration starts from accepted main `15e87287` and brings over only the
standalone example and observability code, without the unaccepted renderer/video
candidate ancestors. This main-based source and its compilation, CI and delivery
identities are separate from the `91b` native run; those checks do not extend its
native scope. The managed `35ed87ed` desktop remains unchanged and does not
contain this trace.

## Records and ownership

The top-level `frame_pacing` describes whether capture is enabled, native
generation allocation and bounded retirement counters. Each live surface has
its own `frame_pacing`, or `null` when disabled. A generation belongs to one
native surface lifetime; it does not follow a logical address through recreation
or the existing successful-submission history handoff.

Each live trace retains at most 512 events, plus one optional callback request,
one readiness reference and one admitted attempt. Ordinary animated frames
produce two events: a callback record and an attempt result. `events_total`,
`trimmed_events` and each event's sequence number make retention loss explicit.
A collector must detect sequence gaps and native-generation changes. It cannot
treat trimmed history as a complete capture or missing presentation frames.

- A `callback` event records `request_frame`, `requested_at_ns`,
  `handler_at_ns`, `request_to_handler_ns`, raw `callback_data_ms` and a
  `correlation` of `matched`, `unmatched` or `ambiguous`.
- An `attempt` event records its independent attempt ID, `next_frame`, optional
  `callback_event`/`callback_handler_at_ns`, `started_at_ns`, `finished_at_ns`,
  `duration_ns`, optional `callback_to_attempt_ns`, and a fixed outcome label.
  Retries can share `next_frame`; their attempt IDs remain distinct.
- Outcome labels are `presented`, `retry`, `render_error`, `pre_render_abort`
  and `unclosed`. Only `retry` has a `retry_after_ns` value. No native error
  body, message, application content, scene or path enters these records.

The request is observed immediately before the existing `wl_surface.frame`
request. The attempt begins only after the existing frame-demand admission and
ends when the renderer returns, or at a pre-render fractional-destination
failure. It includes view/layout/input/render work and excludes later successful
submission bookkeeping and client `FrameSubmitted` handling. These durations
overlap existing successful CPU stages; do not add them together.

SCTK's ordinary frame handler provides the surface, without the callback proxy
or our request identity. The trace joins only a single outstanding request. If
another request appears before its callback, correlation becomes permanently
ambiguous for that native lifetime: later callbacks have no request/frame/latency
join, including after the pending count returns to zero. The trace never
silently replaces an older request with a newer one. Pending, overlapping,
uncorrelated, unmatched and unclosed counts remain explicit.

Exact native retirement discards timing state and adds bounded aggregate
retirement counters. A late callback for a retired surface is counted without
targeting a replacement. Initial attempts can have no preceding callback; idle
callbacks can have no following attempt. An admitted attempt still in progress
is exposed as `active_attempt`. A replaced unclosed attempt is recorded as
`unclosed`; a fatal process exit is not a promise of a persisted final record.

## Clocks and limits

All `*_ns` event timestamps use this process's relative `Instant` epoch. Their
intervals are wall time, including scheduling and blocking. Invalid backwards or
saturated intervals serialize as `null`, never as zero latency.

Core Wayland callback data uses milliseconds with an undefined base. Its raw
`u32` values remain separate from local timestamps and from `wp_presentation`'s
clock ID. Do not subtract or align these clocks. Short same-lifetime wrapping
callback-data deltas describe only that clock's spacing.

`request_to_handler_ns` combines the remaining render/present call after the
request, outbound queue/flush, compositor waiting, transport, OS scheduling and
client event dispatch. It is not compositor-only
latency. `callback_to_attempt_ns` measures the next admitted attempt after the
handler, including any idle interval or preceding client work; it is not a
standalone scheduler-delay measurement. This first slice does not instrument
the socket-read or queue-batch boundary, application updates, GPU execution,
compositor emission time or physical scanout. Existing presentation feedback
keeps its separate frame joins, timestamps and protocol flags.

## Source-specific native evidence

The [callback timing report](../reports/measurements/lucent-native-callback-timing.json)
records a separate `91b964ae` diagnostic build: formatting, strict Clippy, 66
runtime-library tests and three example tests pass there. Four native execution
and five restoration checks pass with the normal `35ed87ed` desktop preserved
and all its profiling OFF. The diagnostic alone has frame and presentation
profiling enabled. These checks do not execute this later main-based source.

The selected window retains 5,286 contiguous events: 2,643 matched callbacks and
2,643 presented attempts. Mean local request-to-handler time is 44.175852 ms,
handler-to-next-attempt is 0.006112 ms and admitted attempt duration is
1.603372 ms. Separately, 2,641 joined presentation feedbacks record 21.972535
intervals/s in their own compositor clock. The clocks and window boundaries
remain separate; neither count is inferred from the other.

These observations locate measured intervals, not their cause. Request waiting
still includes transport and client dispatch, and the small post-handler interval
does not attribute the earlier wait to the compositor. No eligible resource or
60 FPS target, causal improvement, GPU time or physical scanout is accepted.
Earlier shape and CPU-shape results keep their original sources and scopes.

For the clean integration, run the focused test command below, the full runtime
library suite and strict formatting/Clippy. A later native capture must use its
own newly built, source-pinned diagnostic. CI, merge and publication are separate
from the earlier native result.

```sh
cargo test --manifest-path lucent/Cargo.toml -p lucent-wayland --lib pacing::tests
```
