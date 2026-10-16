# Pixel landscape samples

Two small, silent H.264 MP4 videos provide local live-wallpaper choices without a
download service or external account. Install them from the repository root:

```sh
python3 scripts/install-live-wallpaper-samples.py
```

The helper copies the samples to `~/Videos/Wallpapers`, which the live-wallpaper
picker discovers. Matching copies are skipped; conflicting existing files are
refused. It does not select a wallpaper, change a palette, or alter saved settings.
Use `--destination PATH` to prepare a different directory.

| Sample | Appearance | Video |
| --- | --- | --- |
| Moonlit Lake | Indigo mountains, pine silhouettes, cool moonlight | `moonlit-lake.mp4` |
| Amber Lake | The same landscape with muted warm clouds and amber reflections | `amber-lake.mp4` |

Both are 1280×800, 15 frames per second, 12 seconds, 180 frames, with no audio.
The camera and terrain remain still. The original loop uses localized horizontal
water ripples with nearest sampling and a periodic 12-second sinusoidal phase;
the loop is blended against its first frame using a periodic motion envelope to
match the endpoint frames. The second video applies a constant color grade.
These are video frames with changing water pixels, rather than a still image
moved across the screen. Lossless H.264 encoding preserves the matched endpoints;
the pixel format remains `yuv420p`. Each sample is approximately 1 MiB.

The source artwork was created for this repository with the built-in image
generation tool, using the original prompt in `art-prompt.txt`. The source loop
was authored from that artwork for Lucent. Amber Lake is derived from that loop;
it is not a separate scene or third-party footage. The repository's MIT license
applies to these original project assets.

The warm variant can be reproduced from `moonlit-lake.mp4` with FFmpeg:

```sh
ffmpeg -threads 1 -filter_threads 1 -filter_complex_threads 1 \
  -i moonlit-lake.mp4 -map 0:v:0 -an \
  -vf 'colorchannelmixer=rr=1.12:rg=0.08:rb=0.20:gr=0.04:gg=0.88:gb=0.08:br=0.10:bg=0.08:bb=0.62,eq=saturation=0.86:gamma=1.04' \
  -c:v libx264 -threads 1 -preset slow -crf 0 -pix_fmt yuv420p \
  -movflags +faststart -map_metadata -1 amber-lake.mp4
```

`manifest.json` records checksums and provenance. `validation.json` records actual
streaming decode measurements, including the last-to-first frame difference
against ordinary neighboring frame differences. Both samples have identical
full-resolution decoded first and last frames and changing intermediate frames.
This establishes the encoded wrap has no pixel discontinuity; playback timing
and presentation smoothness still depend on the player and display.
