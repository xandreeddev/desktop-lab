# Controlled video decode and conversion experiment

This diagnostic has been **compiled and executed in four fresh host processes**.
It is isolated behind `lucent-services`' opt-in `video-stage-benchmark` feature and
`cfg(test)`. Normal library/desktop builds do not compile the probe. No domain,
port, desktop, renderer or public service API was added, and production playback
was unchanged by this experiment. The tested source is
`6f19f8e296b6d8b68e4a647aedbc26933a502f6a`; subsequent documentation commits do
not represent another code build. The [sanitized report](../reports/measurements/lucent-video-stage-benchmark.json)
binds the source, compiled binary and private receipts.

The benchmark addresses the [native presentation observation](native-presentation-analysis.md):
prepared timestamps already have cadence gaps before rendering, but host pressure
prevents attributing those gaps to a decoder, conversion or presentation stage.
The source uses `Config::count(1)`, which also selects `Type::None`; merely
changing the count to 2 would still request no threading. The experiment requests
`None/count 1` or explicit `Frame/count 2`, then records and requires the actual
opened decoder type/count. It accepts only the controlled H.264/yuv420p workload.

## What each fresh process does

One invocation runs exactly one of four cases: serial decode, frame2 decode,
serial decode plus conversion, or frame2 decode plus conversion. Use one compiled
test binary and identical input, prefix length, target and library environment.
Each case is a separate process; no preceding case can pollute its RSS high water.

1. Read a private bounded configuration and create an absent 0600 report in an
   owned 0700 directory. Existing reports are never replaced.
2. Pin the regular input descriptor with `O_NOFOLLOW|O_NONBLOCK`, validate its
   supplied SHA256, and retain exact device/inode/mode/owner/group/size/mtime/ctime
   identity. Reading atime is excluded. FFmpeg opens this descriptor through
   `/proc/self/fd`, not a potentially replaced source pathname.
3. Run an unpaced timed prefix without full-pixel hashing. Decode-only receives
   source frames. Conversion calls the **existing private `FrameConverter`**, with
   its unchanged BILINEAR physical-cover sizing, owned row copy and immutable
   `MediaFrame` output. No lower resolution, rate, effects or quality setting is
   substituted. The source `PresentationTimeline` still normalizes timestamps.
4. Freeze that pass's wall time, process user/system CPU, RSS samples and lifetime
   high water before quality validation. Demux, decoder API, converter setup,
   actual conversion/copy and decoder retirement have separate wall durations.
5. Decode the same prefix again for validation. Hash ordered raw/normalized PTS,
   visible YUV420P rows and, where applicable, actual converted RGBA plus geometry.
   Ignore native stride padding. Require identical timed/validation PTS, counts
   and dimensions. Record validation cost separately, and verify the input digest
   and pinned identity again.

Full pixel hashes are deliberately outside the timed throughput pass: hashing
several gigabytes would both consume CPU and allow asynchronous decoding to run
during unrelated hashing. Validation's resource/throughput numbers are labeled
separately and are not the benchmark result. The initial file digest warms the
input cache for every case; this is a warm stage experiment, not cold startup.

The timed pass does not use `FrameClock`, sleep, latest-frame delivery, the SDK's
image preparation or GPU rendering. It processes every output frame of its
bounded prefix. Stage throughput therefore cannot be renamed native FPS. FFmpeg
worker CPU can overlap conversion, so summed API wall durations do not partition
worker CPU. Process CPU includes all decoder workers.

RSS is sampled every 16 output frames and at boundaries. `VmHWM` is a process
lifetime high water through that pass, including test startup and source
validation; it is not a resettable interval-only peak. Before/after values and
sampled RSS/threads remain explicit. Root must also record external CPU quota,
thread limits, host pressure, source provenance and the compiled test-binary hash.
One-core quota would conceal the proposed parallelism.

## Executed result and its scope

The release probe passed formatting, strict Clippy and its six nonignored Rust
boundary tests. The media test remained explicitly ignored in that ordinary test
run, then ran once per case under its explicit configuration. All four cases
completed their 360-frame prefix and separate quality pass. The 17 Python
comparator boundary tests also pass.

The input was the same 3840×2160 H.264/yuv420p, 60/1 source used in the earlier
native presentation observation. Both conversion cases used the existing
1920×1080 physical-cover BILINEAR conversion and owned RGBA copy. Ordered PTS and
visible decoded YUV hashes matched across all four cases; converted RGBA hashes
matched between the conversion cases. Actual opened decoder policies were
`None/count 1` and `Frame/count 2`.

| Timed case | Prefix wall seconds | Unpaced output frames/s | All-thread CPU seconds | CPU / wall, % of one core | Sampled RSS max, MiB | Final lifetime VmHWM, MiB |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Serial decode | 4.797 | 75.040 | 4.721 | 98.410 | 77.441 | 76.797 |
| Frame2 decode | 2.792 | 128.947 | 5.033 | 180.282 | 102.543 | 102.684 |
| Serial decode + conversion/copy | 7.295 | 49.348 | 7.224 | 99.024 | 94.270 | 93.590 |
| Frame2 decode + conversion/copy | 4.178 | 86.157 | 7.932 | 189.829 | 118.934 | 118.707 |

Throughput is `360 / timed wall seconds`, including decoder setup and retirement.
CPU is the process user-plus-system delta across that pass, including decoder
workers; the percentage divides that CPU delta by wall time. Frame2 finished
sooner while using more total CPU and higher observed residency. RSS samples and
the final `/proc` VmHWM are separate Linux observations. Some sampled RSS values
exceed the final sampled VmHWM slightly; both are retained without treating
either as an exact interval peak. The lifetime VmHWM includes process startup and
the initial source validation. Neither column measures the normal desktop or GPU
memory, and later hashing-pass residency is excluded from this table.

The serial/frame2 wall-time ratios were **1.718 for decode** and **1.746 for
decode plus conversion/copy**. These describe one warm, fixed-order observation
of each case, not a causal or statistical speedup estimate. Every process first
validated the source digest, warming the input cache. Cases ran in the table's
order, without repeated or randomized trials, under `CPUQuota=200%`,
`MemoryHigh=900M`, `MemoryMax=1400M`, `TasksMax=256` and `RuntimeMaxSec=180`.
Each internal pass had a 60-second bound. No cgroup throttling counter was
captured, so the amount of quota throttling is unknown.

Host pressure counters bracketed each **whole process case**, including setup
and the separate quality pass, rather than just its timed prefix. They recorded
125, 137, 395 and 112 swap-in pages respectively, and I/O full-stall fractions
between 0.527 and 0.568. These observations do not establish a pressure-matched
comparison. Asynchronous decoder work can overlap conversion, so API wall-stage
durations cannot identify an exclusive CPU bottleneck.

The measured conversion case supports investigating a separate, live-only
adapter policy trial with unchanged pixels and timestamps. It does not establish
native 60 FPS, source-cadence delivery, physical scanout, hardware decoding,
normal desktop memory at or below 300 MiB, other codecs, or the same result on
another machine. The production trial and its native validation require their
own source, build and measurement evidence.

## Configuration and execution

The source-preparation milestone was followed by the build and four cases
reported above. The commands below describe reproduction; they do not add
another execution result. The actual bounded build used the release profile,
offline locked dependencies and one build job.

Compile the private feature in an explicitly selected, source-matched target
location; never reuse a stale binary from another checkout:

```sh
cargo test --release --locked --offline -p lucent-services \
  --features video-stage-benchmark --lib --no-run --message-format=json
```

Run the six nonignored private boundary/hash tests with the same feature. They
have source coverage for SHA256's standard vector, explicit serial/frame2 policy,
configuration bounds, visible-pixel versus stride-padding hashing, existing-report
preservation and source-mutation refusal. All six passed in the recorded build:

```sh
cargo test --release --locked --offline -p lucent-services \
  --features video-stage-benchmark --lib video::benchmark:: -- --test-threads=1
```

Create four private JSON configurations, replacing the source/report placeholders
and supplying the independently captured lowercase source SHA256. Use separate
absent report paths and keep configuration files 0600 and their parent 0700. This
example is a template, not a real media path or captured digest:

```json
{
  "source": "/absolute/path/to/owned/source.mp4",
  "source_sha256": "<64 lowercase hexadecimal characters>",
  "report": "/absolute/private-directory/serial-convert.json",
  "threads": "serial",
  "stage": "convert",
  "frames": 360,
  "seconds": 60,
  "width": 1920,
  "height": 1080
}
```

Valid thread modes are `serial` and `frame2`; stages are `decode` and `convert`.
The probe bounds prefixes to 1800 frames, each pass to 120 seconds and requested
edges to 4096. Actual source dimensions retain the adapter's existing 4096-edge /
64 MiB envelope, 512 MiB file bound and 16 MiB packet bound. Its interrupt and frame
loop check the per-pass deadline; an externally reviewed `RuntimeMaxSec`,
`MemoryMax`, `TasksMax` and CPU quota remain mandatory for the process, including
source hashing and decoder retirement. Choose limits that allow intended
parallelism without running alongside another heavy/native job.

With the compiled test executable identified from Cargo's JSON artifact record,
invoke exactly this ignored test under that reviewed process scope:

```sh
LUCENT_VIDEO_STAGE_CONFIG=/absolute/private-directory/serial-convert-config.json \
  /absolute/path/to/source-matched-test-binary \
  --exact video::benchmark::video_decode_stage_benchmark \
  --ignored --nocapture --test-threads=1
```

Repeat in fresh processes for all four configurations. Media paths/digests and
raw reports stay private until root reviews a sanitized result. The probe writes
no other files, invokes no subprocess, opens no network or Wayland connection,
and never changes running wallpaper configuration.

Compare only completed, source-matched, full-prefix reports:

```sh
umask 077
python3 vm/compare_video_stage_reports.py \
  /private/serial-decode.json /private/frame2-decode.json \
  /private/serial-convert.json /private/frame2-convert.json > /private/comparison.json
python3 -m unittest discover -s tests -p test_video_stage_reports.py -v
```

The comparator refuses different source/code/library identities, actual thread
modes, targets, prefixes, PTS, decoded pixels or converted RGBA. Its 17 Python
boundary tests check this refusal behavior. The actual four-case comparison also
passed; these are distinct checks.

## How to use the result

If serial decoding already exceeds the source's demand while conversion does
not, frame threading has not been established as the fix. If explicit frame2
improves the real conversion case with identical pixels and PTS, it supports a
separate adapter-only live-playback policy trial, followed by source-specific
native cadence and eligible 120-second normal RSS/CPU checks. No automatic policy
change follows from this script.

A future live-only policy must keep poster and preview serial. Poster creation
currently shares `decode()` with live playback; testing only whether a preview
argument is absent would inadvertently change poster decoding too. Keep any
purpose/configuration private to the adapter, with no threading knobs in domain
ports or presentation components.

Frame2 can create two decoder worker pthreads in addition to the existing
producer/demux/conversion thread, retain more source pictures and add decoder
pipeline delay. One 3840×2160 8-bit YUV420 visible picture is 12,441,600 bytes /
11.865 MiB before alignment, reference metadata, contexts and thread stacks. This
is not a fixed incremental RSS estimate; measure actual ownership/high water and
cold/warm behavior. The converter remains serial with one reusable destination,
and no additional RGBA queue or toolkit is introduced.
