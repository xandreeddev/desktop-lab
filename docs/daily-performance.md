# Measuring daily desktop resources and animation

The target remains a normal desktop at or below 300 MiB process RSS with fluid
animations. Source-level bounds, texture payload counts and green tests do not
prove that target. A software-Vulkan guest also includes driver mappings in the
process; RSS cannot be relabeled as independent GPU memory.

## Current concrete allocation path

Fontdue eagerly enumerates a font's cmap/GSUB glyphs and stores their flattened
outline geometry when constructing a Font. Deferring construction until the
first CJK/Hangul character still retained the complete large fallback face
thereafter. The framework now borrows immutable fallback tables, and the renderer
prepares one requested glyph outline/mask at a time. Finite glyph bounds, pixel
counts and temporary outline commands are checked before coverage allocation.
The primary fontdue path and 2× physical supersampling stay unchanged; there is
no additional per-glyph cache beyond the existing bounded text texture cache.
This removes that specific eager path. A measured memory reduction requires an
identical before/after executable probe and full desktop workload.

The isolated ignored test `font_resources::cold_and_warm_fallback_preparation_resources`
runs without a compositor or GPU. Choose one fixed `LUCENT_FONT_RESOURCE_CASE`
(`latin`, `arabic`, `devanagari`, `cjk` or `hangul`) and run that exact case alone
with `--ignored --nocapture --test-threads=1`, once in each source cohort. It
records RSS/PSS before primary initialization, after the first 2× preparation,
and after twenty repeated preparations, plus dimensions/coverage checks. Cold
means the first preparation in that process; no OS page-cache purge is implied.
It measures font preparation, not the renderer's normal texture-cache hits,
process startup or overall animation performance. Keep runs bounded and execute
only after the host's build lane is idle.

## Compositor presentation feedback

Set `LUCENT_PROFILE_PRESENTATION=1` temporarily to observe `wp_presentation` on an
ordinary IPC-enabled framework runtime. Secure/greeter runtimes do not bind or
record it. This requests feedback alongside the existing frame callback only in
the renderer's before-present hook, after successful acquire/paint and before
WSI commits that buffer. It creates no timer, redraw or additional frame demand.

A pending request captures the exact native surface object and logical submission
ID. The global pending set is capped at 256 and each surface retains at most 120
presented samples. Retired/recreated native surfaces cannot append old feedback
to a replacement's history. Missing protocol support is reported separately;
there is no fake feedback or fallback FPS based on submission counters.

Each presented sample records its protocol clock ID, nanosecond timestamp,
refresh hint, output-sequence number and raw flags. Discarded commits have their
own counter. Hardware-clock and hardware-completion claims require the matching
flags. A compositor-reported presentation is still distinct from proof that
sixty *different decoded video frames* reached a physical monitor. Successful
submission counters and CPU-stage profiling keep their previous meanings.

## Matched native collection

`vm/measure-daily-resources.py` is read-only. Prepare identical static-idle and
animated-background workloads separately, preserving their settings/source/output
identities. Supply the actual release build receipt with `source_commit` and
`binaries.lucent-desktop.sha256`:

```sh
python3 vm/measure-daily-resources.py \
  --release-manifest reports/local/release-build.json \
  --label candidate-animated --phase animated-background --seconds 120
```

The collector refuses a mismatched running/installed executable, a replacement
process, changed settings bytes/inode, output changes or a changed playback
generation/target. It samples Linux process RSS/CPU every 250 ms, captures
smaps_rollup before/after and observes bounded presentation histories every
500 ms. It does not restart the VM/shell, change wallpapers/preferences, open
panels or send native input. Reports are private mode 0600 until reviewed.
CPU-stage profiling must be disabled for normal-resource samples. Optional
presentation instrumentation and inspection overhead are recorded explicitly.

Use `vm/measure-framework.py --presentation` for a separate guarded startup and
launcher interaction pass. Its first open/close pair is labeled
`first_after_process_restart`; later pairs are warm. It reports actual compositor
presentation intervals alongside the existing submission intervals. Its private
runtime override is removed and the original runtime restarted on failure.
This helper intentionally restarts the shell and performs interactions; schedule
it separately from read-only sampling and host builds.

Compare the same release cohort, source media/rate, display/density, widget set,
font content and GPU backend. Keep static idle, source-rate video, cold/warm
launcher interactions and peaks separate. Record actual constraints and unmet
targets. Do not cut density, effects, source cadence or the design system to make
a smaller number.
