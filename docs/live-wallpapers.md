# Native live wallpapers

Lucent plays local MP4, WebM and animated GIF files. The desktop client and
playback coordinator are Rust. FFmpeg libraries decode the media; the existing
wgpu/Vulkan renderer presents it on background Wayland surfaces. There is no
external wallpaper player or embedded browser.

Video is the normal choice for full-screen playback. GIF is also accepted, with
its decoded timestamps and frame durations. MP4/WebM describe containers; actual
codec availability comes from the installed FFmpeg build. Wallpaper Engine's
video files can be used as media. Its Scene projects and HTML/JavaScript Web
projects require different engines and are outside this implementation. See
[Wallpaper Engine's format overview](https://docs.wallpaperengine.io/).

## Choosing a wallpaper

Download a clip from [Pixabay](https://pixabay.com/videos/search/wallpaper/) or
[Pexels](https://www.pexels.com/search/videos/desktop%20background/), or use your
existing local video/GIF. Start with a short loop at a resolution suitable for
your display. A still image from Wallhaven is still a static wallpaper.

Open **Wallpapers → Live**, **Style → Live wallpapers** in the native menu,
or **Commands → Live wallpapers**. The picker lists local MP4, WebM and GIF
files from `~/Videos/Wallpapers`; choose an option to import and play it. Use
**Refresh** after adding files. Discovery runs on a service worker, keeps the last
good list on errors and is bounded to 32 choices. The path field remains available
for media elsewhere.

Two original pixel-landscape loops are included: **Moonlit Lake** and **Amber Lake**.
They keep the terrain and camera still while water moves. Install the samples with
`python3 scripts/install-live-wallpaper-samples.py`, then refresh the picker.
Their decoded first/last frames match exactly; see
[asset provenance and measurements](../assets/live-wallpapers/README.md).
The installer does not activate a wallpaper or change preferences.

CLI equivalents:

```sh
lucent-cli wallpaper live open
lucent-cli wallpaper live play /absolute/path/to/loop.mp4
lucent-cli wallpaper live pause
lucent-cli wallpaper live resume
lucent-cli wallpaper live pause-all
lucent-cli wallpaper live resume-all
lucent-cli wallpaper live retry
lucent-cli wallpaper live rate source
lucent-cli wallpaper live rate 60
lucent-cli wallpaper live off
```

The native panel accepts a local path with ordinary keyboard selection, editing
and clipboard behavior. **Import wallpaper** copies the source to private owned
storage, actually decodes it and creates a PNG poster before a preference is
accepted. An invalid file does not replace the current configuration. The source
is preserved. Closing the panel cancels pending delivery; already-running native
file work may finish and retain an unused owned asset.

Choose the default wallpaper or a connected monitor. Monitor overrides take
precedence over the default. **Use static wallpaper** disables that monitor's
live override, or removes the default live wallpaper when the default is selected.
Pause/resume in the panel applies to the selected profile. **Pause all** is a
separate global preference, preserved when importing another clip. CLI
`pause`, `resume`, `retry` and `off` always target the default profile;
`pause-all` and `resume-all` explicitly control every profile. A monitor can
**Use default wallpaper** to remove its override. Paused playback freezes its
last prepared frame and stops its producer; a resume starts the loop again.
Changing its FPS retains that frame. A restored paused profile prepares its
poster once and starts no decoder. Decode failures retain the last good image and offer **Retry playback** for the
selected profile. Save progress/errors and **Retry save** use the root persistence
coordinator; changing a control does not claim that storage has finished. Opening
the panel focuses its library controls, Escape or an outside click closes it, and
ordinary inputs support the shared editing/undo contract. The native settings
panel uses existing semantic colors,
spacing, radii, typography, scrolling and frame-paced motion.

**Pause on battery** and **Pause in fullscreen** are enabled by
default and can be switched off independently in this panel. Automatic pauses
do not modify either manual pause preference. Returning to AC power or leaving
fullscreen resumes playback only when no other pause still applies. The status
names the reason for the selected display. A shared default may still show
**Playing** when another display is using it.

Fullscreen is evaluated on each output's visible workspace. Maximized windows,
client-only fullscreen hints and windows on hidden workspaces do not pause it.
When two outputs share a stream, the covered output freezes its own image and
receives no further frame redraws; the visible output keeps the single decoder
running. Once every consumer is paused, the decoder subscription stops. A
resumed decoder restarts its loop rather than seeking to its previous timestamp.
Unavailable battery/compositor observations clear those automatic reasons;
they are not treated as proof of battery power or fullscreen. A known sleep
pause remains in effect until a successful power observation reports resume.

An observed secure session lock also pauses every live output. This automatic
reason is always enabled; it does not change the manual pause controls. The
client retains a known lock pause across unavailable observations and resumes
only after an explicit unlocked observation, when no other pause applies.
An initial unavailable observation does not establish that the session is locked.
Locking and unlocking still belong to the secure locker and compositor protocol.

The default live import also supplies its validated poster to the normal static
wallpaper transaction. That gives secure/session clients a still background and
retains ordinary image/theme export behavior. Export failure stays visible through
the existing wallpaper coordinator; an import does not claim an atomic update of
all external applications. Default live imports reserve the same selection intent as static choices and
online downloads. A newer choice retires an older pending import; historical
accepted static commits can finish without clearing a newer live preference.
Selecting a different current successfully committed default static wallpaper
removes the default live selection. Closing the panel or turning off that target
retires its pending import, so a late result cannot turn playback back on.

## Source changes and animation

The desktop owns a `BackgroundPresentation` for each current output surface.
A newly prepared source queues a diagonal cut using the shared
`motion.wallpaper_reveal` timeline (850 ms) and `motion.cut` curve. The timeline starts after the first
background frame for that selection submits. Waiting for a compositor callback
or rendering that first frame does not consume the reveal duration. Completion
uses the time sampled by the final rendered view, not its later submission time;
a slow partial frame cannot retire the old image early. Submission is not a
scanout acknowledgement, and slow rendering can still reduce intermediate frames.
First load appears immediately;
refitting a static image or delivering the next video frame does not restart
that timeline. Missing preparation keeps the previous good image.

The outgoing frame is an immutable SDK `ImageData::snapshot`: its pixels are
shared with the source allocation, but its GPU identity is separate from the
stream texture that subsequent frames update. This matters when another output
still uses that stream. Each presentation retains one current and at most one
outgoing image; a submitted final frame releases the outgoing reference and
requests a cleanup redraw. Disconnected or replaced output generations retire
their presentation state. No second decoder is started for a transition.

A rapid third selection keeps the dominant previous image, rather than
allocating a GPU screenshot of the intermediate mixture. This bounds retained
images but does not preserve that interrupted mixture exactly. The incoming
image remains opaque on the revealed side; the old image stays visible ahead of
the diagonal edge. The SDK's `Element::diagonal_reveal(progress)` clips both paint
and input consistently, with antialiasing in the Vulkan shader. No extra render
target or full-screen screenshot is allocated for the cut.

The live selector uses the same clip at stationary panel bounds, opening and
closing in opposite directions. New mounts wait for their first submitted frame
before starting the timeline, so native allocation time does not consume the
opening. Closing and reopening midway reverses continuously. The runtime paces
changing frames through compositor callbacks and stops redrawing when settled.
Prepared-image byte counters are not process RSS or a GPU memory total.

## Preferences and resource limits

`desktop.json` stores `live_wallpapers` with `default`, `monitors` and a global
`paused` flag and a `policy` object. A selected profile contains an owned `asset`, a validated frame-rate policy in `fps`,
`paused` and `enabled`. Existing settings without these keys continue to load.
The root persistence coordinator remains the only writer.

```json
"policy": {
  "pause_on_battery": true,
  "pause_when_fullscreen": true
}
```

This object belongs inside `live_wallpapers`. Existing configurations without it
receive the defaults; explicit `false` values survive save/restart. Runtime pause
reasons and fullscreen output observations are never written to `desktop.json`.

The saved value `"fps": "source"` selects source timing; historical numeric
values such as `"fps": 30` remain explicit caps and are preserved. Missing `fps`
uses source timing. To roll back to a binary from before source timing, select
numeric limits for Source profiles first, or restore that binary's matching
preferences backup. New imports use **Source frame rate**. Playback follows the clip’s presentation
timestamps, including fractional rates such as 59.94 FPS and variable frame
durations. A 60 FPS source can provide 60 frames per second; the bundled 15 FPS
clips remain 15 FPS. The panel also offers optional 15/30/60 FPS limits; custom
limits can be 1–60. Caps skip excess/late frames and never synthesize additional
frames. The runtime presents available frames at compositor callbacks, so a slow
decoder or display can still deliver fewer frames than the source.
When timestamps are absent, playback continues from the previous frame duration;
missing duration metadata uses the source's average rate, or 30 FPS when that is
also unavailable. Shared identical media/frame-rate profiles use
one producer. Different source profiles have separate playback timelines.
Disconnected outputs acquire no decoder, and observation of a known sleep
transition suspends playback. Explicit pause is always available. Session lock
observation supplies its own pause reason; fullscreen is not used as a lock
detector. No wallpaper observation ever authorizes an unlock.

Initial envelopes:

- Local regular sources up to 512 MiB; MP4, WebM and GIF containers only.
- Source decoded dimensions at most 4096 per edge and 64 MiB per RGBA frame;
  a smaller playback destination does not admit a larger source.
- At most four enabled profiles, 32 retained monitor preference entries and
  32 imported assets. Remove unused owned media when the asset store is full;
  there is no automatic deletion of configured/rollback media.
- One latest mailbox frame and one wake notice per producer. A slow UI does not
  queue a sequence of full video frames.
- One immutable current frame per player and one reusable GPU stream texture.
  An automatically paused output can retain one additional immutable frame with
  a static GPU identity while another output advances their shared stream.
  Static image mip preparation and cache policy remain available separately.

These are admission/lifetime limits, not a total process/GPU memory guarantee.
The normal desktop target remains 300 MiB RSS. Actual current video workloads,
physical GPU frame pacing, hardware decoding and power use still need measurement.
This implementation decodes on the CPU with one codec thread; Vulkan renders the
result. It does not claim Vulkan Video or hardware decode support.

## Display-sized playback pixels

`PlaybackOptions.target` accepts an optional domain `WallpaperPixelTarget` with
validated positive physical-pixel dimensions. `None` preserves source dimensions
until display geometry is known. Import validation and poster preparation keep
that source-sized default; playback fitting does not reduce poster quality or
relax the source envelope.

The client derives each connected consumer's demand from its actual background
viewport and integer or fractional rendering density. Consumers sharing a
media/frame-rate producer contribute the maximum width and maximum height to
that producer's target, including paused consumers. Independent producers keep
independent targets. The focused output and a global desktop extent cannot
substitute for these per-consumer observations.

Before a named output catalog arrives, the default route can use its validated
legacy background extent. An observed catalog containing only unnamed outputs
still does not establish a live-wallpaper route in the current coordinator;
extending that legacy fallback remains separate work. A genuinely empty catalog
continues to retire playback.

For formats proven opaque by FFmpeg's pixel descriptor, the adapter converts
only the RGBA destination to the smallest uniformly scaled size sufficient to
cover both target edges, with checked outward rounding and no upscaling.
Alpha-capable or paletted formats retain source-sized conversion, even when a
particular frame happens to be opaque. An unavailable format descriptor also
keeps source size. FFmpeg's `AV_PIX_FMT_FLAG_ALPHA` contract describes these
values as straight alpha; resampling before the SDK premultiplies can introduce
invisible RGB into visible edges. Source-sized conversion preserves the existing
premultiply then GPU-filter path. This conservative policy means GIF and
alpha-bearing media do not receive the conversion-memory reduction;
alpha-safe fitted conversion is separate work.

Source aspect metadata passes through `MediaFrame` into the
prepared SDK image, so rounding odd dimensions does not alter cover geometry.
The existing private FFmpeg frame and owned row copy remain; published bytes are
never borrowed from conversion scratch.

A changed target retires the producer generation and its latest-frame mailbox.
The last good immutable image remains visible while its replacement prepares;
paused consumers keep their frozen pixels. This adds no decoder per output,
worker, polling or redraw source. Source timestamps, frame-rate policy, bounded
delivery and the existing reveal animation continue to govern playback.
The replacement decoder starts at the beginning of the clip. Preserving the
playhead across a target change requires a separate seek/resume contract.

An output enlarged while paused keeps the exact pixels it previously froze;
it cannot gain detail that those pixels no longer contain. An active shared
producer includes the enlarged demand, and a resumed output uses its newly
prepared frame. Re-decoding the exact frozen timestamp at a larger size remains
separate work; this change does not seek, replace frozen content with a poster,
or retain additional full-resolution source snapshots.

The live-wallpaper inspector's `producers` entries report target, stored frame
size and source aspect separately, without exposing media paths. These counters
support matching a resource sample to the actual conversion demand; they are
not decoder scratch, GPU staging or total process-memory measurements.

## Architecture and startup path

```text
saved LiveWallpapers or explicit import
  → domain WallpaperFrameRate policy (Source or validated WallpaperFps cap)
  → optional per-producer WallpaperPixelTarget from background consumers
  → WallpaperPlaybackPort supplied by NativeVideoWallpapers
  → producer subscription owns FFmpeg context and timing
  → decoded MediaFrame with validated destination pixels and source aspect
  → SDK ImageStream prepares a single immutable image level on the worker
  → latest mailbox replaces previous unpublished content
  → small Wake message reaches the client's UI thread
  → coordinator accepts the current revision and updates its frame
  → root maps monitor IDs to exact current SurfaceAddress instances
  → view describes Element::image(...).cover()
  → renderer updates the existing stream texture with queue.write_texture
  → a dirty Wayland surface is submitted at its available frame callback
```

Domain models/ports know nothing about FFmpeg, file operations, Wayland, wgpu or
Lucent's appearance. Services own local storage, native codec setup, frame
validation and timing. The client owns configuration policy, monitor selection,
import revision, transient panel state and mailbox delivery. The root joins native
hosts and saves preferences. Views perform no decoding or filesystem work.

`ImageStream` is a generic framework feature, usable by any client with changing
pixels. Each `ImageData` frame remains immutable and keeps its own allocation ID.
A separate stream resource ID and revision let the renderer update the same GPU
allocation when dimensions match; a dimension change replaces that resource.
Per-frame mip generation is omitted. Already submitted work keeps its native
resource lifetime. The renderer has no wallpaper or codec policy.

`WallpaperPlaybackPolicy`, `WallpaperPlaybackContext` and `WallpaperPauseReason`
are domain values. The pure `wallpaper_pause_reason` use case applies explicit
pause, sleep, session lock, battery and per-output fullscreen precedence without I/O. The
existing `PowerPort` and `CompositorPort` observations supply facts. The Hyprland
adapter normalizes its internal fullscreen mode, mapped/hidden flags, monitor
identity and visible workspace; its socket event triggers a fresh snapshot,
not a boolean toggle. Repeated fullscreen events therefore cannot accidentally
resume a paused output. See the upstream [IPC event contract](https://wiki.hypr.land/IPC/),
[command serialization](https://github.com/hyprwm/Hyprland/blob/main/src/ipc/s1/Commands.cpp)
and [fullscreen modes](https://github.com/hyprwm/Hyprland/blob/main/src/managers/fullscreen/FullscreenTypes.hpp).

The domain `SessionPort.watch_lock` reports `SessionLockState::Locked` or
`Unlocked`, or an observation error. `LucentSession` implements it on a
subscription worker. Its Hyprland adapter reads `j/monitors` through the existing
bounded socket transport, with a two-second query deadline and one-second wait
between reads. Identical observations emit no new client message. The client
owns this subscription only while a live wallpaper has an output, including
while paused; ordinary static wallpapers add no lock polling.

The supported Hyprland revision has no lock-change IPC event. Its
[`solitaryBlockedBy` serialization](https://github.com/hyprwm/Hyprland/blob/efb50993780079460b0cbed1363e2166a2de1d9f/src/debug/HyprCtl.cpp)
includes `LOCK`, which reflects compositor lock state even after a locker exits.
The [monitor check](https://github.com/hyprwm/Hyprland/blob/efb50993780079460b0cbed1363e2166a2de1d9f/src/output/Monitor.cpp)
can return early when a workspace is missing: `WORKSPACE`, missing/malformed
fields and unreadable outputs therefore cannot establish an unlock. A readable
unlocked output can establish it; any explicit `LOCK` takes precedence. This
adapter detail stays outside the domain, views and generic renderer. It provides
advisory background-work policy, not authentication or permission to release a
secure lock. Observation latency is separate from the compositor's immediate
security boundary.

The root joins observations into the transient context. The child coordinator
owns per-output presentation routes and shared producer lifetimes. A paused
route uses `ImageData::snapshot()` so future writes to a peer's stream texture
cannot change its frozen pixels. Policy controls emit typed intents through
`change_live_wallpaper`, and the existing persistence coordinator saves only
the changed preferences. No compositor, D-Bus or codec operation runs in a view.

The decoder reuses one private RGBA conversion buffer for each playback pass.
Each delivered frame still receives owned bytes: reusing conversion scratch
cannot mutate an image retained by a surface, a transition or another output.
The renderer observes prepared image storage weakly through the generic
`ImageData::lifetime()` API. Once every CPU consumer has released those pixels
and no canvas pins the texture, existing cache maintenance can retire it even
below the reuse budget. Value clones and frozen transition snapshots share the
same pixel lifetime; each stream upload supplies its new frame's lifetime.
This avoids retaining unreachable image textures without changing decode,
wallpaper policy, output density or animation. It adds no idle polling and does
not establish how much memory a driver or allocator returns to the process.
The image preparation worker also records whether every pixel is opaque during
its existing alpha pass. `ImageData::is_opaque()` reads that prepared fact without
scanning pixels on the UI thread. The background client omits its solid fallback
under an opaque cover image; transparent media and fades with transparent outgoing
content retain that fallback. Dimensions, frame rate, filtering and motion tokens
are unchanged. This removes a repeated allocation and an unnecessary full-screen
draw; it does not establish a process-memory or frame-time improvement by itself.

Only new content marks a background dirty. Panel `FrameSubmitted` bookkeeping
does not request another paint when settled. Frame callbacks pace submissions;
they do not cause idle surfaces to repaint continuously. Codec timestamps and the
FPS cap schedule media frames independently of UI component motion.

## Evidence and remaining acceptance

The integrated display-fit source passes 662 regular Rust tests, strict Clippy,
95 Python checks, generated tokens, three streamed-image GPU checks, 56 unchanged
live-wallpaper visual comparisons and both documentation prefixes. Focused
tests cover odd source dimensions and aspect,
fractional density, differently shaped shared outputs, independent profiles,
resize/hotplug generation retirement, retained paused pixels and unchanged
source timing. A tiny two-frame transparent GIF and descriptor checks cover
source-sized alpha conversion, contrasting invisible RGB and premultiplied edge
coverage. Native resize/density acceptance and compositor alpha-edge acceptance
remain separate. Repeat the
same full-resolution workload and source cadence when
measuring RSS/PSS, retained/uploaded bytes and presented frames. No display-fit
native performance sample or 300 MiB pass is claimed here; the historical
measurements below retain their recorded source identities.

Owned fixtures encode tiny synthetic MP4/WebM/GIF files and decode them through
this exact native adapter. Tests cover actual pixels, frame counts/timestamps,
loop cancellation, failed import cleanup and foreign asset rejection. Client
checks cover sharing, pause without a producer, disconnected outputs, stale
messages, bounded latest-frame delivery and preservation of the last good image.
Policy tests exercise saved-setting migration/opt-outs, manual-pause precedence,
power observations and disconnect/recovery, shared stream pause/resume, static
image identity and per-output redraw routing. An owned Unix-socket fixture sends
fragmented and repeated fullscreen events through the actual Hyprland adapter.
These checks do not establish physical-battery or native multi-output acceptance.
Session-lock fixtures exercise actual owned sockets, repeated observations,
unavailable/recovered state and cancellation. Client checks cover observer
lifetime, preserved pixels, paused producer retirement and manual-pause
precedence. A focused native check is available as
`python3 vm/test-live-wallpapers.py --lock-only --login-file PRIVATE_LOGIN_FILE`.
It uses the existing guarded native keyboard helper for authenticated unlock;
the file must be private to its owner. It also restarts the desktop while the
independent secure locker remains active and restores the normal desktop through
the existing checkpoint. On source `00f3e8721d054503d7e7d0878c8e6cc58b59dba8`,
the single-output VM passed actual secure lock, zero decoder/background redraws
while locked, desktop restart under an existing lock, authenticated unlock with
resumed playback, and a second lock/unlock preserving manual pause. The session
was left unlocked and exact settings, wallpaper, theme/login exports and idle
policy were restored. See the [native lock report](../reports/measurements/lucent-live-wallpaper-lock.json).
This does not establish physical-session/multi-output behavior or the broader
authentication and recovery audit.

A focused VM check is available as `python3 vm/test-live-wallpapers.py --policy-only`.
It uses one synthetic MP4 and a test-owned terminal to exercise fullscreen,
workspace visibility, manual pause, the native policy toggles and restart.
It skips resource benchmarks and restores the caller's exact preferences,
wallpaper, exports, workspace/focus and idle policy through the existing checkpoint.
The native Hyprland 0.56.2 VM passed this check on implementation source
`faec272e21fe301284f6212a59b7de84cc8752aa`: visible fullscreen stopped the
decoder and background submissions, a fullscreen window on a hidden workspace
did not pause playback, and leaving fullscreen preserved manual pause or resumed
an unpaused profile. Keyboard changes to both policy controls survived restart.
The original workspace/focus, exact preferences, wallpaper, theme/login exports
and idle policy were restored, and the test-owned window/assets were removed.
See the [policy VM report](../reports/measurements/lucent-live-wallpaper-policy.json)
for the implementation and harness identities. This was one 1920×1080 output
using software Vulkan, without physical battery transitions or new resource
benchmarks. Native multi-output behavior and physical power/pacing remain open.

A software-Vulkan test verifies real changing pixels, one reused GPU texture,
no upload for a repeated revision and replacement on dimension changes.

The daily-control implementation was exercised in a real Hyprland VM on
source `63a4518b5140f7082f4bc3fed0ef8b5d5c1ea64c`: synthetic MP4/WebM/GIF
background pixels changed, pause froze frames and pixels, resume worked, invalid
imports preserved preferences, and restart restored playback/FPS. Native path
editing, submission, FPS, autofocus, keyboard undo/redo and Escape were exercised.
Importing another clip preserved explicit pause-all. Exact settings, wallpaper, theme,
login exports and idle policy were restored. See the sanitized
[live wallpaper VM report](../reports/measurements/lucent-live-wallpaper-vm.json).
This was software Vulkan on one 1280×800 output with a synthetic 160×90 clip
capped at 15 FPS. Two 120-second samples measured static RSS at 224.66 MiB mean /
233.70 MiB peak and playback at 259.36 MiB mean / 260.12 MiB peak. Main-process
CPU was 1.067% and 69.891% of one core respectively; software rendering contributes
to those values. GPU allocations were not measured separately. Both tested
process peaks fit the 300 MiB allowance. Full-resolution media, actual secure-client
presentation, multi-output/reconnect/suspend and physical-GPU pacing/power remain
separate acceptance; these synthetic samples do not establish them.

A later 120-second full-resolution workload (1280×800 pixel-landscape video at
15 FPS, displayed at 1920×1080 using software Vulkan) measured 373.35 MiB mean
RSS / 373.38 MiB peak and 113.616% of one logical CPU core. That source predates
the conversion/coverage changes. It exceeds the 300 MiB target and supersedes
any inference that the small synthetic sample proved full-resolution efficiency.
Reference source `00f3e8721d054503d7e7d0878c8e6cc58b59dba8` has two
120-second samples with profiling disabled on the same output/media dimensions:

| Workload | Mean RSS | Peak RSS | CPU, one logical core |
| --- | ---: | ---: | ---: |
| Settled static desktop | 242.34 MiB | 247.00 MiB | 1.225% |
| 1280×800 H.264 loop, 15 FPS | 348.55 MiB | 348.88 MiB | 80.008% |

Playback continued through the interval, with 1808 background submissions.
The static sample meets the 300 MiB RSS target; the full-resolution video still
exceeds it, and static idle CPU is above the 1% target. See the
[current resource report](../reports/measurements/lucent-current-video-resources.json)
for the binary, workload, measurement procedure and verified exact restoration.
Those reference measurements did not themselves establish an improvement over older binaries.
Software rendering contributes to process CPU/RSS; GPU allocations and
physical-GPU pacing/power remain unmeasured. Hardware decoding remains open.

Source `8e06cfc1ab9129b569a7d7ea2ddd5bb1ec2699b4` adds weak image-storage
lifetime observation in the framework cache. Repeating the same two 120-second
workloads gives 242.33 MiB mean / 247.02 MiB peak static, and 333.10 MiB mean /
333.39 MiB peak video. The observed video peak is 15.49 MiB lower but still
exceeds 300 MiB. Video CPU is higher in this sample: 86.074% of one core,
compared with 80.008%; static is 1.050%. This is one run per implementation on
software Vulkan, not repeated trials or a CPU/pacing improvement claim.
After playback, the GPU cache reports 5,702,394 payload bytes, of which
5,467,816 are pinned by active draws; these exclude native buffer/driver memory.
Actual compositor captures also verify intermediate red/blue fade pixels and
outgoing-image retirement in both change directions. Both the resource and
pixel checks restore exact settings, wallpaper, exports and idle policy. See
the [image lifetime report](../reports/measurements/lucent-image-lifetime.json).

A fresh normal-mode sample of PR73 source
`d2aed42199ed6b29f6eef214c1a6ee490658adc6` preserves the currently selected live
wallpaper on the 1920×1080 scale-1 software-Vulkan output. Over 120 seconds it
measures 548.29 MiB mean / 609.41 MiB peak RSS and 250.132% of one logical CPU
core. There are 2232 background submissions, and the live producer retains
33,177,600 bytes of RGBA pixels. Warm launcher submission medians range from
32.98 to 48.67 ms, with maxima up to 98.97 ms. This different workload is not a
matched comparison with the earlier 1280×800, 15 FPS sample. It exceeds the
300 MiB target; source-size conversion, display demand and decoder/driver
costs require investigation. Source timing, display density and effects were
unchanged. Settings/wallpaper and the running binary hash were verified.
See the [current live resource baseline](../reports/measurements/lucent-live-wallpaper-4k-baseline.json).
These are software-VM process and submission measurements, not physical-GPU
memory, execution or scanout results.

## Diagonal cut and local library acceptance

Implementation `8b26f6d4ae5fa92ffaa0cfc4455e904f36f3f5ce` passed
`python3 vm/test-wallpaper-cut.py` and `python3 vm/test-live-library.py` on the
1920×1080, single-output software-Vulkan VM. Actual compositor recordings show
simultaneous old/new opaque pixels on opposite sides of the cut in both change
directions. Selector opening and closing show the same spatial cut while their
layout and native extent remain stationary. These replace the historical fade
behavior described in the earlier image-lifetime measurements.

Native pointer clicks through **Wallpapers → Live** imported both bundled
samples, saved their exact sources and started producers with advancing
background submissions. Both checks restored exact preferences, wallpaper,
theme/login exports, workspace/focus, bar visibility and idle policy; only new
test-owned imports were removed. The two source videos remain in the library.
See the [cut and library report](../reports/measurements/lucent-wallpaper-cut-library.json).
This checks interaction and compositor pixels, without claiming new CPU/RSS
benchmarks, hardware-GPU smoothness or multi-output acceptance.
