# Opt-in GPU allocator snapshots

With the existing `LUCENT_PROFILE_FRAMES=1` diagnostic instrumentation, each
ordinary inspector request includes `profile_memory.gpu_allocator`. Normal
inspection keeps `profile_memory=null` and never queries the allocator. This
uses the existing inspector and instrumentation policy; it adds no timer,
redraw, per-surface query or session/authentication interface.

The renderer owns `Gpu::allocation_snapshot(enabled)`. It uses wgpu 30.0.1's
safe public `Device::generate_allocator_report`, without another dependency,
Cargo feature, required device feature or unsafe HAL access. Wayland requests
one snapshot from the shared device per enabled inspection, outside its surface
loop. Domain models, ports, adapters and desktop components are unchanged.

An available report exports four scalar fields:

- `total_allocated_bytes`: live allocations tracked by that backend allocator;
- `total_reserved_bytes`: tracked memory blocks, including unused regions;
- `block_count` and `allocation_count`: checked conversions to 64-bit counts.

The status is `available`, `unavailable` or `count-overflow`. The reusable
renderer API also represents `disabled`; a disabled call never invokes the
backend. Unsupported reporting is distinct from an available zero-byte report.
No resource names, allocation labels, offsets, addresses or debug vectors are
serialized. Counts are not converted to guessed memory sizes.

The Vulkan implementation locks and copies allocator bookkeeping. The query
does not submit GPU work, poll/wait, free resources, trim allocators, reset a
device, alter `MemoryHints::MemoryUsage` or change rendering policy. It creates
temporary CPU report vectors and may contend for the allocator lock, so collect
occasional explicit diagnostic snapshots. The backend API builds the complete
report before aggregation; this interface does not impose an upstream allocation
count bound. Inspector frequency and diagnostic overhead belong in the evidence.

Allocator bytes, Lucent texture/geometry payload, OS RSS/PSS and GPU residency
have different scopes and must stay separate. An allocator snapshot does not
include every swapchain/driver allocation, identify CPU codec/heap ownership or
prove that reserved GPU blocks account for anonymous RSS. CPU profiling is also
enabled by this existing flag, so this is an instrumented cohort, not an
uninstrumented memory-budget measurement. No memory gain or resource target is
accepted by adding this observation.

At source `4a0f932a7c82ac0459715007bdbd314d145a4ae4`, formatting and strict
release Clippy for renderer and Wayland all targets passed. All three allocation
tests and 84 Wayland tests passed, and the release Lab binary built successfully.
The allocation tests cover disabled/no-query, unavailable versus an empty
allocator, and one-query aggregate byte/count mapping.

A fresh offscreen native Vulkan export on the same AMD RADV backend as the
previous catalogue passed its strict check: 9 stories, 5 palettes, 480 captures
and 117 producer inputs. All 480 PNGs are byte-identical to the previous catalogue;
only the manifest records the new source and producer inputs. This offscreen
export does not exercise the inspector allocator query. Native allocator report
availability and cold/4K/warm observations remain unverified; these checks do not
identify the retained-memory cause or accept an RSS, FPS or other resource target.
