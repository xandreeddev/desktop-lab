# Notification images: values, asset effects and native paint

Lucent supports raw `image-data` hints, local `image-path` references, application
icons and local body `<img>` blocks. It displays one associated image using the
[notification specification's priority](https://specifications.freedesktop.org/notification/latest-single/):
valid image-data, valid image-path, app_icon, then deprecated icon_data. Body
images remain separate content blocks; remote body image URLs become alternative
text and are never downloaded.

## The complete path

```text
Application Notify call
  → native adapter borrows and validates the wire hint
  → NotificationImage / NotificationPixels domain values
  → bounded inbox snapshot
  → root delegates Snapshot to notifications::Center
  → parsed body + current visible artwork demand
  → one task on the existing ASSETS effect queue
  → injected AssetPort / NativeAssets prepares immutable ImageData and mips
  → revision-keyed Artwork result returns to the UI thread
  → view describes Element::image and loading/error controls
  → framework layout resolves boxes and clips
  → renderer uploads reusable texture resources and paints Vulkan commands
  → compositor frame callbacks pace the short image reveal
```

The domain holds image content and validated source references, never GPU
textures, layout sizes or file handles. `NotificationPixels` uses immutable
RGBA storage. `NotificationImageSource` accepts bounded icon names, absolute
paths and local file URIs. Serialization reconstructs the same invariants;
older notification records without an image retain their default `None`.

The presentation `AssetPort::notification_images` contract returns prepared
framework images. Its requests include received-version/role keys, so one
wire ID's replacement cannot install old artwork. Views contain no file reads,
image decoding or mip generation. The framework knows no notification IDs,
image hint priority, source paths or alternative text policy.

## Demand and motion

The client requests only bounded visible toasts or the selected history entry.
One active batch and one latest pending batch share the existing asset worker;
a thousand superseding requests do not create a thousand queued decodes.
Completions retain only keys still wanted. Hiding, replacing or paging drops
unneeded prepared images; late replies cannot restore retired demand. Missing
images keep alternative text and offer an explicit Retry images control or
`lucent-cli notifications retry-images`.

Each newly prepared image gets a framework `Motion` with the design system's
enter duration/easing. The client samples opacity using `ViewContext::now` and
reports active motion through `Application::animating`. The runtime schedules
dirty frames through compositor callbacks; Vulkan blends the supplied opacity.
There is no image-specific polling timer or new effect lane. The fixed image
box preserves surrounding geometry while artwork appears.

Body images and the associated picture have token-based 200×100 logical boxes,
with aspect-preserving containment and rounded clipping. Preparation retains up
to 400×200 physical pixels for 2× density, without enlarging small raster
originals. Cards containing images have bounded controlled scroll viewports;
wheel, thumb and keyboard reveal keep lower content, retry and actions reachable
on small outputs. History uses its existing controlled viewport.

## Resource limits and failure scope

- A raw image has positive dimensions, edges up to 512 and at most 65,536
  pixels. The native adapter checks channels, 8-bit samples, stride, required
  rows and at most 1 MiB of wire byte content before copying to tight RGBA.
  It does not clone the full variant to inspect it. D-Bus transport allocations
  happen before this application validation.
- Live inbox/history raw pixel retention is at most 8 MiB, keeping newest
  images first. Text, actions and notification entries remain retained when
  older raw pixels exceed that budget. Path/icon references are small metadata.
- Durable archives keep newest raw pixels within 256 KiB, leaving room under
  the existing 4 MiB JSON envelope. Their text/history policy is unchanged.
  Recovery recognizes the same receive when an older archive omitted pixels.
- Local image files must be regular, at most 4 MiB and opened nonblocking.
  FIFOs/devices are rejected. Raster headers and decoders have a 16 MiB
  allocation envelope and bounded dimensions. This is a decoder setting,
  not total process memory or a cancellation guarantee for kernel file I/O.
- Notification SVGs cannot resolve nested file/data image resources and reject
  filters. Their prepared output stays bounded; this policy does not promise
  a total SVG parser memory/time budget.
- The child retains at most 8 MiB of prepared artwork payload. Inspector
  `notification_artwork.prepared_bytes` deduplicates immutable allocation IDs.
  This excludes active work, queued messages, scenes, GPU caches/driver memory
  and all other client allocations; it is not RSS.

The server advertises `body-images` and `icon-static`, alongside its formatted
body/link/action support. It does not advertise animated icons, sound, activation
tokens or the protocol's optional persistence guarantee.

## Evidence and remaining acceptance

Owned private-bus tests exercise actual image hints, replacement and fallback.
Pure tests cover value/serialization invariants and inbox/archive retention.
Owned files test decode, alpha, local URI handling, denied SVG references/
filters and FIFO rejection. Injected client tests cover latest-demand coalescing,
retirement, explicit retry and keyboard reachability of the final action.
Synthetic software-Vulkan references cover ready/loading/failed pictures,
history scrolling, small outputs and reveal motion at 1x/2x in both palettes.

Real applications/compositor input, icon-theme interoperability, physical
filesystem stalls, guest deployment and current RSS/frame pacing remain deferred.
See the complete requirement ledger; image support does not establish full
desktop completion.
