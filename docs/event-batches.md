# Events update state before the next paint

The runtime owns application state on one thread. Calloop waits for work from
Wayland, the bounded worker channel, IPC and existing deadlines. One dispatch
handles the events already admitted to that batch; it does not wait for every
producer to become quiet.

```text
wait in calloop
  pointer/key/worker/command event
    → application update
    → effects, native reconciliation and invalidation
    → current CPU input layout when needed
  other events in this dispatch
    → update the same owned state
dispatch returns
  → one GPU draw pass over configured, drawable dirty surfaces
  → view → layout → renderer → Vulkan submission
  → FrameSubmitted messages and their effects
wait again
```

An update requests a redraw; it does not call the renderer. A native frame
callback releases that surface's pending-frame admission; it does not render
inside the callback handler. Consequently a queued content update can reach the
next view before painting, instead of painting an intermediate snapshot from
the middle of event dispatch. Each surface is visited at most once per pass.
This batches available work, rather than imposing a fixed animation frequency.

## Input does not wait for the GPU

A second click or key in the same dispatch must use current controls, even if
the previous buffer is waiting for its compositor callback. The runtime keeps
CPU input layout separate from GPU submission. Affected interactive surfaces
refresh their hit geometry and focus scope after state changes. Pointer,
keyboard and delayed clipboard delivery explicitly resolve the current tree.
Passive backgrounds need no additional input layout for each video update.

Synchronizing a tree retires ordinary primary-button capture when its control
disappears or changes its capture geometry. A held press cannot later release
the callback of a removed control. Stable control identities remain the
client's responsibility; domain commands still validate their current handles
and request generations. Native input authority never transfers to a recreated
surface with the same logical name.

## Submission followups cannot get stranded

`FrameSubmitted` runs after successful submission and bookkeeping. Its client
message can expose or invalidate another configured surface that this pass has
already visited. If that surface can draw immediately, the runtime sends one
coalesced calloop ping for the following dispatch. It does not recursively enter
the renderer.

`FrameDemand::can_begin()` observes the existing admission rule without changing
it: dirty, no pending compositor frame, and no pending render-retry deadline.
Callback-blocked animation, unconfigured surfaces and deferred retries do not
cause these pings. Idle applications keep sleeping. Retry timers and tooltip
deadlines retain their separate existing ownership.

## Boundaries and verification

The application owns state, messages, animation curves and design tokens.
Domain ports and concrete adapters keep their existing responsibilities. The
framework owns event batching, generic input, frame admission and submission;
neither Vulkan nor the compositor receives the component tree.

This change does not prove 60 FPS, fix host paging or cap resident memory.
Submission counters measure submitted buffers, and `FrameSubmitted` does not
mean GPU completion or scanout. Keep native pacing and process-memory results
separate from the runtime's scheduling tests and from older diagnostic sources.
