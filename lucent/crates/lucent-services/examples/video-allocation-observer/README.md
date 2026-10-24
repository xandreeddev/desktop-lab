# Independent playback allocation observation

This example consumes `WallpaperPlaybackPort` directly. It uses an already
prepared owned asset and never imports media, changes the desktop, or creates
a renderer, GPU device, Wayland surface or application window. It runs three
finite phases in one process: 1920×1080, 3840×2160, then 1920×1080 again.
Each phase retains only its latest domain frame, or optionally the existing SDK
shared image bridge. All pixel owners from the previous phase are dropped only
after its producer has observed cancellation and its exact worker has joined.

No arguments prints an inert plan. `--run` reads at most 64 KiB of JSON from
private stdin. Supply the actual existing prepared descriptor, for example:

```json
{
  "state": "/absolute/private/live-wallpapers",
  "asset": {
    "source": {"path": "/absolute/original.mp4", "name": "Original"},
    "path": "/absolute/private/live-wallpapers/media-owned/original.mp4",
    "poster": {
      "path": "/absolute/private/live-wallpapers/media-owned/poster.png",
      "name": "Poster"
    }
  },
  "phase_seconds": 10,
  "sdk_bridge": false,
  "allocator_stats": false
}
```

`phase_seconds` accepts 1–30; an initial native diagnostic should use 10–30.
The existing adapter validates the asset's immutable metadata and local owned
storage. No source path is accepted in argv. Keep stdin and **stderr private**:
native FFmpeg or panic diagnostics may contain private data. A future runner
must redirect stderr to an exclusive mode-0600 file and bound the standalone
process with its own user unit. A stop-ignoring backend fails its 10-second join
bound, starts no next phase, and exits this process; it is not reported joined.

Stdout contains fixed labels, publication counts, actual emitted dimensions,
the latest and maximum published vector capacity, and allowlisted samples of
this process's RSS, anonymous memory, high-water RSS and swap. Missing counters
are null. The consumer does not expose media paths, pixels, native error text,
command lines, environment or credentials. Output failures stop the sequence.

The reported pixel capacity covers only the consumer's latest allocation; it
does not count the decoder, conversion scratch, allocator retention or library
globals. RSS includes those and the diagnostic itself. The after-join and
after-drop samples are observations, not proof that freed storage was returned
to the OS. Optional SDK preparation validates opacity and keeps the existing
alpha-copy rules; it still creates no renderer. This example isolates one
consumer path for later comparison and cannot attribute desktop RSS growth to
a codec or GPU, establish a memory target, or prove visible frame cadence.

The source and mock tests are prepared for review. They have not been compiled,
run against media, or used to collect native measurements at this checkpoint.

## Recorded follow-up

The later [bf7 observer report](../../../../../reports/measurements/lucent-playback-allocation-observer-bf7.json)
records strict release Clippy, nine focused tests and an actual release build.
Two separate native processes then complete the domain-frame and SDK-image
sequences against the same existing imported video. All producers join, latest
pixel owners drop, owned processes drain, and desktop/media preservation passes.
After the final drop, RSS remains about 149 MiB in both runs, versus about
52–57 MiB after the first FHD phase. This is an independent CPU-path observation;
it neither assigns the desktop's retained memory to this path nor proves an
allocator/codec cause, improvement, memory target or FPS. The CPU quota and
separate process births also prevent treating mode differences as a gain.

## Optional glibc allocator accounting

`allocator_stats` defaults to `false`. Disabled mode makes no allocator-statistics
FFI calls and adds no allocator fields to the existing output. With it enabled,
the example samples only at `before_play`, `after_join_latest_held` and
`after_latest_dropped`, with the exact playback worker stopped. The regular
250 ms memory samples do not query the allocator. There are at most nine
allocator observations across the three successful phases; an unsuccessful
phase does not authorize another playback worker or later boundary samples.

Each boundary sample gains a separate `allocator` object. GNU/Linux builds use
`gnu_get_libc_version` and `mallinfo2`; other targets explicitly report
`available: false` with `unsupported_target`, rather than zero counters. Direct
linking to `mallinfo2` requires glibc 2.33 or newer. This is not an old-glibc
compatibility shim. The actual runtime version is included in successful
observations. This diagnostic does not call `malloc_trim`, `mallopt`, install an
allocator, change environment settings or add a sampling thread.

`allocator.bytes` contains `arena`, `arena_accounted_in_use`, `arena_free`,
`direct_mmap` and a checked `malloc_accounted_in_use_total` (arena in-use plus
direct mmap). An overflowing total is null. `allocator.counts` contains
`arena_free_chunks` and `direct_mmap_regions`; malloc does not provide a live
object count here. These byte values are separate from the existing
`memory.*_kib` counters and must not be added to RSS or PSS.

The [reviewed glibc implementation](https://github.com/bminor/glibc/blob/glibc-2.42/malloc/malloc.c)
visits all arenas, taking their locks sequentially. Arena allocations can
themselves be backed by mmap; the separate direct-mmap counter covers malloc's
individually mapped chunks. Freed chunks in a thread cache can still appear in
arena in-use accounting. Worker exit drains that worker's cache, but the main
thread and library state can remain. Therefore these counters do not identify
live Rust objects, allocation owners, fragmentation, a leak, or resident pages.
A rise in free arena bytes after join/drop is consistent with allocator
retention; it does not prove which pages account for retained RSS.

The [GNU safety notes](https://github.com/bminor/glibc/blob/glibc-2.42/manual/memory.texi)
describe allocator locking, initialization and concurrent-setting caveats.
The call is outside signal handlers, follows ordinary input allocations and
never races an allocator-setting operation performed by this example. It is
not an atomic whole-process snapshot, and the diagnostic and its saved JSON
also allocate memory. Keep the native run bounded and treat this as additional
accounting evidence, not an allocation profiler or a performance target pass.

This optional extension and its injected tests are source-only at this
checkpoint. It needs a new reviewed build and native run; earlier observer
measurements do not contain these fields.
