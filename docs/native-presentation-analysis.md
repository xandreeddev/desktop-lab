# Reading actual video presentation evidence

The frozen `8e6d074bfcb30a0bcc0b371afddcd50c08993af0` desktop completed an
instrumented native RADV run with valid background presentation/source joins.
Its selected background history advanced distinct prepared revisions at
**26.9992 per second**. The median presentation interval was **33.333 ms**,
the 95th percentile **50 ms**, the 99th **83.333 ms**, and the maximum **150 ms**.
These are actual metadata observations under substantial host pressure; they do
not establish the 60 Hz target, pixel visibility, or eligible performance.

The [sanitized result](../reports/measurements/lucent-native-presentation.json)
contains exact integer nanosecond statistics, provenance hashes, joins, resource
observations, and restoration scope. Private receipts and prior failed attempts
remain unchanged.

## Which interval is being measured?

The collector retains rolling history from its first inspection. The raw file
contains 3,496 presentation samples across six surfaces, including startup
updates from the bar, dock and widgets. Those samples are not one animation rate.
Its 3,397 background samples also include 121 requests made before the first
metadata observation. Dividing either count by the 120-second resource sampler
would use mismatched boundaries.

The offline helper reports three explicitly named views:

| View | Background callbacks | Own compositor timestamp span | Callback intervals/s |
| --- | ---: | ---: | ---: |
| All retained background history | 3,397 | 125.616661 s | 27.034630 |
| Collector's request lower bound | 3,276 | 121.299995 s | 26.999177 |
| Nominal resource-duration request window | 3,239 | 119.966662 s | 26.990832 |

The collector view selects requests at or after the first observed desktop uptime
and ends at the last captured feedback. The nominal resource-duration view adds
an upper request boundary using the measured 120.000563344 resource seconds.
The request and resource clocks were not synchronized by this offline operation;
that additional view is a labeled comparison, not exact resource/presentation
clock alignment. Every reported rate uses its own compositor timestamp span.

A rate counts **intervals**, not endpoints: `(N - 1) / (last_time - first_time)`.
Quantiles use nearest rank `ceil(N*p)` without interpolation, retaining integer
nanoseconds until output. Samples must retain one clock, strictly increasing
presentation timestamps, immutable frame identities and an unambiguous resource
join. Conflicting duplicates, truncated traces and unknown revisions refuse.

## What the source joins establish

All 3,276 selected background feedbacks reference exactly one known prepared
video revision. All 3,276 are distinct; there are no adjacent repeats. Between
its first and last referenced revision, the source catalogue contains 3,404
consecutive prepared revisions, with no missing catalogue positions. Of those,
128 do not appear in this selected successful-feedback set. This measures
prepared resources absent from that set; it does not establish their precise
fate or whether the remaining resources supplied visible pixels.

The prepared catalogue itself has same-cycle media PTS gaps: median 33.333333 ms,
95th percentile 83.333333 ms, 99th percentile 100 ms, and maximum 150 ms. Thus
cadence gaps already exist by the time prepared frames reach the framework;
the renderer alone cannot explain all of them. The trace is recorded after
video admission/conversion and is not a census of every decoded frame.

The source is nominally 60 Hz H.264 at 3840×2160, fitted to 1920×1080 at scale 1
on a 60 Hz virtual output, with the `source` rate setting. Non-increasing PTS
increments the client's cycle identifier. The helper excludes five cycle
crossings instead of subtracting a reset PTS or inventing a clip duration.
Within same-cycle prepared endpoints, 3,840 nominal 60 Hz positions are not
represented. This is a nominal timestamp-grid estimate, not a proved count of
decoder drops: variable timestamps, admission policy and host stalls need
separate stage evidence.

All selected compositor feedbacks report refresh `16,666,666 ns` and flags7
(`vsync | hw_clock | hw_completion`; no `zero_copy` bit). These are the
compositor's protocol flags in this virtualized environment. They do not prove
physical-display scanout, pixel visibility, or that a 60 Hz video stream was
presented at 60 distinct frames per second.

## Resource scope and restoration

The instrumented 120.000563344-second resource sample measured 219.4954 MiB mean
RSS, 219.4961 MiB interval peak and 113.6495% CPU, expressed as a percentage of
one logical core. The desktop process reported no swap. However, the host
swapped in 610 pages and spent 56.6234% of its observed interval in I/O full
stall. The uninstrumented reference also failed eligibility, with 217.2819 MiB
mean and 221.9219 MiB interval peak RSS. Host-pressure comparability failed too.
Both actual measurements remain recorded, and the normal 300 MiB budget result
remains **unaccepted**. This run does not support a causal overhead comparison.

The separate V3 restore operation passed in an owner-recorded 42.774 seconds.
Profiling flags returned off, normal desktop/helper lifetimes remained stable
for 35 seconds, and original source, settings, media and security state were
preserved. Restarting the supervised session creates fresh lifetimes; no claim
of preserving the original PIDs is made. Earlier failed V1/V2 attempts remain
failed historical receipts.

## Where to investigate next

The decoder adapter currently runs one serial worker with
`Config::count(1)`, whose cached ffmpeg-next 9 default is `Type::None`. It decodes
full-resolution source frames, admits them by source time, then performs the
existing BILINEAR conversion and immutable RGBA preparation. Frames already more
than one source hold late are rejected before conversion. A full input decode
can therefore remain serial even when conversion rejects stale frames.

This source path and the observed prepared PTS gaps justify a bounded stage
comparison; they do not prove whether decode, conversion, scheduling, storage
pressure or presentation is the dominant cause. Compare serial decode and
explicit **Frame threading with count 2**, first decode-only and then with the
same 1920×1080 BILINEAR RGBA conversion. Preserve source, fitted resolution,
rate setting, effects and output. Merely changing count 1 to count 2 leaves
`thread_type=0` and would not test frame threading.

If a measured stage comparison supports the change, keep policy private to the
media adapter, separating live playback from preview and poster work. Existing
poster preparation shares the decoding function, so testing only whether a
preview argument is present would also change poster decoding unintentionally.
A second frame worker can retain additional full-resolution reference pictures
and contexts; one 4K 8-bit YUV420 visible picture is 11.865 MiB before alignment,
side data and stack costs. Exact RSS impact must be measured. No production
thread policy or architecture boundary changed for this analysis.

## Reproduce the offline calculation

```sh
python3 vm/analyze_presentation_metadata.py /path/to/private-measurement.json
python3 -m unittest discover -s tests -p test_presentation_metadata_analysis.py -v
```

The helper only reads a bounded local JSON file and emits aggregate JSON. It
opens no socket, launches no subprocess, and never changes the VM or runtime flags.
Tests cover repeated resources at 60 Hz, 24 Hz source advancement with 60 Hz callbacks,
PTS resets, prepared skips, invalid joins, history trimming, clock identity,
nanosecond quantiles, deterministic ordering and exclusion of private fields.


## Later live-H264 threading trial: performance remains unaccepted

The separate coherent `9b5420b7` candidate changes only live H264 to explicit
two-frame decoding. A normal, uninstrumented 120-second run observed 374.464 MiB
mean RSS, 398.652 MiB interval peak and 174.232% of one logical core. The guest
process recorded no swap. These observed values exceed the 300 MiB memory budget.

A separate metadata-enabled lifetime records 3,865 distinct joined background
revisions: 3,864 advances over 122.116662 seconds of compositor timestamps, or
31.6419 advances/s. The median presentation interval is 33.333 ms, p95/p99 50 ms
and maximum 83.334 ms. Of the 7,200 consecutive prepared revisions between the
selected endpoints, 3,335 do not appear in that selected successful-feedback set.
That is not a decoder-drop count. Source PTS spacing describes the media timeline;
it is not producer wall-clock throughput or visible-pixel FPS.

Both intervals have host swap activity and substantial I/O stalls, with unmatched
pressure between phases. Neither establishes an eligible performance target or a
causal comparison with the earlier `8e` result. The candidate remains unmerged.
Profiling was restored off and a fresh read-only capture confirms the normal
configuration; source files, settings, media, security and output state remain
preserved. Process lifetimes change during the deliberate restarts; shared runtime
parent timestamps are not claimed restored.

[The new source-specific report](../reports/measurements/lucent-live-threading-9b-native-trial.json)
contains exact source/artifact/input hashes, metrics, protocol flags, restoration
and limits. Older reports retain their original source and outcome. Persistent
decoder-session work is a separate candidate, with no memory saving or smoothness
claim until it is actually measured.
