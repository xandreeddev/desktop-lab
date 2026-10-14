# Fractional buffer density

The SDK's validated `Density` is a positive, reduced integer ratio. It contains
no native protocol handle. `Density::new(150, 120)` and `Density::new(5, 4)` are
the same value. `ONE` and `integer(2)` retain ordinary 1×/2× use. Zero ratios and
overflowing physical extents are rejected before allocation.

`ViewContext.density` describes the current surface's rendering density.
`Event::Resize` carries both that value and the observed integer `scale`
fallback. The event also fires when preferred density changes without changing
logical dimensions. Instances retain their scoped event wrapper. Layout and
input coordinates remain logical; clients must not scale their padding, font
size, pointer coordinates or persisted positions again.

## Native registration and fallback

At runtime initialization, the Wayland adapter binds fractional-scale v1 and
viewporter v1 if both globals exist. Every native layer and secure lock surface
then owns one scale object and viewport. The compositor's preferred numerator
is converted with denominator 120 into the SDK value. Before feedback arrives,
the current positive integer scale supplies the density. If either extension
is unavailable, the original integer buffer-scale path remains active.

The [protocol specification](https://github.com/wayland-mirror/wayland-protocols/blob/main/staging/fractional-scale/fractional-scale-v1.xml)
defines this feedback and viewport mapping. Lucent keeps `wl_surface` buffer
scale at 1 on the extension path and sets the viewport destination to the
configured logical width/height. It does not apply integer and fractional
scaling together. A viewport destination is updated only when logical dimensions
change, before a successful buffer presentation can commit it.

Preferred-scale callbacks match the actual live surface and extension object.
Late callbacks for retired instances cannot update a replacement. Equal reduced
densities schedule no additional redraw. Extensions are destroyed after the GPU
surface and before their native role; managers retire before the connection's
final flush. Secure lock keeps its all-output coverage and authenticated gate.
No output density event can authorize unlock.

## From logical geometry to physical pixels

```text
preferred native scale / current integer fallback
  → validated exact Density
  → scoped Resize and ViewContext
  → unchanged logical component tree, layout, hits and clips
  → checked physical framebuffer size
  → text coverage prepared at that fractional density
  → Vulkan paint and buffer presentation
  → viewport destination maps the buffer to logical surface coordinates
```

Layer buffers round both physical edges outward: 65×3 logical pixels at 1.25×
need 82×4 physical pixels. This is Lucent's layer-buffer policy, not an xdg
toplevel rounding implementation. Device texture, aligned readback and host
slice limits still apply. The renderer exposes `paint_scaled`, `snapshot_scaled`
and `render_scaled`; existing integer convenience entrypoints delegate to them.

Rounding can make the actual physical-to-logical ratios differ slightly on each
axis. The shader receives those actual ratios for sample-center clip tests,
matching the viewport mapping rather than assuming one rounded scalar. Text
uses fractional-density coverage, its existing bounded 2× supersample and
logical glyph advances. Density changes do not reflow the layout. Cached text
resources distinguish their raster density; images retain their prepared mip
chains. The renderer still validates scenes before submitting GPU work.

## Desktop behavior and inspection

The desktop's presentation `BackgroundTarget` uses the exact density from its
background configure/resize event. Named hosts retain that density independently
of integer catalog metadata. A catalog refresh must not overwrite configured
fractional feedback. The existing shared wallpaper worker prepares enough pixels
for the maximum physical width/height across connected outputs, with its existing
one-active/latest-demand policy. Density changes cancel the corresponding drag
capture and request presentation refits; they do not change selection, colors or
saved layouts.

Native inspection distinguishes `scale` (integer fallback), `density` (rendering
factor), `buffer_scale` (protocol setting) and actual `buffer_width/height`.
Do not assume `buffer_width == width * scale` on a fractional output. The
independent `multi-output` consumer displays the actual context density using the
same SDK. It adds no desktop or service dependency.

Current checks exercise reduced ratios, overflow/readback limits, fractional
text preparation, per-axis GPU clip sampling and unchanged integer convenience
pixels. Existing software visual references must remain unchanged. Actual
compositor negotiation, density migration between displays, native input,
hotplug/suspend, secure lock coverage and current RSS/frame pacing are separate
acceptance checks deferred by the user. No current VM deployment is implied.
Shaping, bidi, font fallback, IME and accessibility remain separate work.
