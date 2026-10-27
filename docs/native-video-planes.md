# Native video planes: opt-in prototype

This source slice connects real FFmpeg decoding to a reusable Vulkan plane painter through neutral immutable values. It is **not enabled in the desktop**. Existing `MediaFrame`, `ImageData`, normal playback, preview, poster and UI paint paths keep their RGBA contracts. The source alone does not establish compilation, GPU correctness, memory savings, 60 fps, VM behavior or the whole-shell 300 MiB target.

The earlier same-source retarget experiments preserved decoder sessions but had variable retained-heap results. They remain separate evidence. This prototype explores removing RGBA conversion and publication storage, without changing the allocator or weakening source admission.

## Concrete path and boundaries

1. The independent `native-video-planes` example calls the existing `WallpaperPlaybackPort::prepare`. The real adapter imports immutable media and writes its validated poster/descriptor. It never fabricates an asset.
2. It explicitly calls the new optional `play_native`. Existing adapters get a default implementation that delegates to source-sized RGBA playback; existing clients still call `play`.
3. `NativeVideoWallpapers` uses the existing bounded input, source clock, H.264 frame-thread configuration, EOF draining/rewind, callback stop and cancellation logic. The core delivery seam can carry either existing RGBA or `NativeMediaFrame::Yuv420`.
4. `video/native_planes.rs` admits only the supported native subset. Its FFmpeg ownership and unsafe access stay inside the concrete adapter.
5. `lucent-video-frame` is a small shared crate with checked `Plane` and `Yuv420Frame` values, explicit BT.709 range/transfer/chroma metadata, and immutable `Bytes` owners. It depends only on `bytes`; neither domain nor framework gains a codec, OS or GPU dependency. The API and domain re-export the same values.
6. `PlanarPainter` receives these values through the framework API. It uploads three reusable R8 textures, samples them in a shader, and draws one opaque video into the existing nonlinear-sRGB `Bgra8Unorm` target. Geometry or output-size changes do not require a new decoded source extent. No compositor calls or service operations enter a view.

This initial painter is a concrete rendering entry point, not a widget or desktop wallpaper integration. It fills its target; cover crop, multiple output surfaces, transitions, current/last-good mailbox generations and normal frame callbacks are future client integration work. No automatic desktop flag activates it.

## Ownership and initialized bytes

The adapter checks all visible plane spans against real `AVBufferRef` owners before constructing a slice. It rejects invalid bounds and falls back for nonpositive strides. Admission **consumes the owned decoder frame**, moving its existing AVFrame and buffer references into a private immutable owner. Unsupported capability returns that same owned frame for software conversion. Neither `av_frame_ref` nor `Video::clone` is needed. A borrowed admission helper would leave the caller's mutable `Video` alive; its safe `data_mut` method does not enforce copy-on-write, so merely retaining another AVFrame header would not establish Rust immutability. [FFmpeg's frame API](https://ffmpeg.org/doxygen/trunk/group__lavu__frame.html) documents its reference ownership and writable-buffer protocol.

A packed plane (`stride == visible width`) can publish a `Bytes::from_owner` view. Its private owner retains the consumed AVFrame; no safe mutable Rust frame handle remains. The FFmpeg decoder's own reuse must respect the outstanding AVBuffer references. Returning from the decoder does not invalidate retained planes; the final byte owner releases its references. [Bytes' ownership API](https://docs.rs/bytes/1.12.1/bytes/struct.Bytes.html#method.from_owner) keeps the supplied owner alive through retained views.

Allocated padding is not assumed initialized. Every padded plane instead copies only visible samples row by row into a packed buffer. No slice spans uninitialized row gaps. `referenced_bytes` and `copied_bytes` report these publication choices, not the full underlying AVBuffer allocation or process memory. Retained buffers can outlive decoding; consumers remain responsible for limiting retained frames. Normal streaming has no CPU RGBA publication in the admitted native branch, but GPU upload still uses wgpu staging copies. This is not zero-copy GPU import or hardware decoding.

For an even 3840×2160 source, visible YUV4208 occupies 12,441,600 bytes; RGBA8 occupies 33,177,600. These are arithmetic payload sizes, not measured memory savings. Decoder reference frames, allocator arenas, staging, target buffers and driver memory are additional costs.

## Explicit color contract

Admission requires progressive 8-bit YUV420P, square pixels, no crop or decode error, explicit BT.709 matrix and primaries, known full/limited range, BT.709 or sRGB transfer, and left/center chroma siting. ICC, mastering, light-level and display-transform side data fall back. Unsupported/untagged/alpha formats retain the existing software RGBA branch; this prototype has no default color guess for ordinary untagged videos.

The shader removes the range offset, reconstructs nonlinear RGB with the BT.709 matrix, then converts a tagged BT.709 transfer to sRGB. BT.709 and sRGB share the supported primaries and D65 white; this subset needs no chromatic-adaptation matrix. The BT.709 curve follows FFmpeg's continuous coefficients, and sRGB encoding follows W3C's conversion. [FFmpeg color utilities](https://github.com/FFmpeg/FFmpeg/blob/n8.1/libavutil/csp.c), [W3C sRGB conversion](https://www.w3.org/TR/css-color-4/#color-conversion-code).

Lucent's current image textures use `Rgba8Unorm`, and its shader pipeline, snapshot target and actual Wayland `SurfaceConfiguration` use `Bgra8Unorm`. The video shader therefore writes **nonlinear sRGB code values**. Using this painter with an sRGB attachment would apply the transfer twice and is outside its contract. This does not establish color-managed output or HDR support. The old software RGBA converter may preserve a different transfer; byte equality to that converter is not the oracle for correctly tagged BT.709 conversion.

Chroma coordinates derive from luma sample coordinates, explicit siting offsets and ceil-sized chroma planes. Reusing normalized luma UVs for odd extents would shift the chroma at edges. The sampler clamps at the actual plane boundary. `Queue::write_texture` accepts checked non-256-aligned row strides; the separate readback path retains its required padded-buffer alignment. [wgpu layout requirements](https://docs.rs/wgpu-types/30.0.0/wgpu_types/struct.TexelCopyBufferLayout.html).

## Validation scope

Authored tests cover checked odd geometry/strides, shared owner lifetime, matrix/range/transfer anchors, FFmpeg buffer bounds before slicing, consumed packed ownership and retained immutable clones, padded-row copies, exact fallback-handle return, real tagged-H.264 EOF/timing/rewind/cancellation without RGBA scratch, and unchanged RGBA fallback pixels/timestamps. The example's parser stays inert without explicit `--run` and rejects ambiguous paths.

Three explicitly ignored `video_planes_gpu_` tests exercise actual Vulkan texture sampling, independent f64 color/resampling expectations (matrix derived from luma weights), both transfers/ranges, odd edges/strides/siting, scaling, opaque output and texture reuse. They permit one output-code step for quantization/interpolation and are added to the existing hosted Vulkan suite. They have not been run locally. Old RGBA visual tests remain required, and no existing goldens/catalogue hashes were refreshed.

After hosted compilation and real Vulkan tests pass, the independent example can be run in an explicitly owned environment:

```sh
cargo run --manifest-path lucent/Cargo.toml --locked -p native-video-planes -- \
  --run --source /absolute/tagged-video.mp4 \
  --state /absolute/new-plane-test-store --output /absolute/new-plane-test.png
```

It stops after the first frame, waits for the decoder body to return, then draws retained native planes with Vulkan and writes a validation-only PNG. Unsupported input writes the established RGBA fallback and prints that distinction. The RGBA snapshot/readback is deliberately excluded from any future normal-stream allocation claim. It creates an explicit import store but never selects a wallpaper or changes a running session.

Before production enablement we still need actual GPU results, an owned streaming consumer using the real presentation path, source-rate and memory comparisons with identical inputs/backends, ordinary metadata defaults consistent with established software behavior, and client lifecycle tests for stale results, last-good errors, pause/retry/source/fps/output retirement, scaling and transitions. The retarget prototypes and recorded failures remain immutable.
