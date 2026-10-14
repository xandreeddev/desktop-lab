# Output targets: declaration, registry and surface lifetime

An ordinary `SurfaceSpec` can declare `output: Some(OutputName::new("DP-1")?)`.
`None` leaves output selection to the compositor. Names are literal, validated
protocol strings. A missing or ambiguous named target waits; it never silently
moves to another display.

```sh
cargo run --manifest-path lucent/Cargo.toml --locked -p hello-layer -- --output DP-1
```

The independent example receives `Event::OutputsChanged(Vec<DisplayOutput>)`
through its normal typed message/update path. It displays its requested name and
catalog count, without accessing native protocol objects. `--help` works before
connecting to Wayland. The subsequent [desktop monitor implementation](monitor-layouts.md)
uses explicit targets and instances for named profiles. Actual compositor
acceptance and fractional scaling remain unfinished.

## From a declaration to native objects

The runtime's SCTK output state registers each advertised `wl_output` and
collects completed information. Native callbacks rebuild a sorted catalog and
emit a change only when that catalog changes. Each `DisplayOutput` exposes an
optional validated name, optional positive logical size and current integer
scale. Its global number is transient; persist a chosen name rather than that
number. Compositors may omit names or logical size. The
[pinned SCTK output source](https://docs.rs/crate/smithay-client-toolkit/0.21.1/source/src/output.rs)
defines these observations.

Reconciliation resolves a declaration against current native bindings. For an
available unique named target, it passes that `wl_output` to
`LayerShell::create_layer_surface`. SCTK forwards it into the protocol's
`get_layer_surface` request. The renderer receives the resulting `wl_surface`,
not an output name; layout still uses the configured surface dimensions. See
[SCTK layer-shell implementation](https://smithay.github.io/client-toolkit/src/smithay_client_toolkit/shell/wlr_layer/mod.rs.html).

```text
SurfaceSpec.output
  → current registry catalog and name resolution
  → layer-shell role bound to the chosen wl_output
  → compositor configure
  → typed Resize, client view and generic layout
  → shared GPU renderer presents this surface's buffer
```

Changing the resolved native output retires the old surface and creates a new
role. It does not attempt to change an existing role's bound output in place.
Keyboard focus/serial state is retired before its logical ID can be reused;
clipboard transfers already guard the actual native surface identity. GPU
surfaces drop before their protocol roles. Component/application state remains
in the client; old native pointer capture does not move into the new buffer.

## Disconnection and closure

Removing an explicitly bound output retires its named surfaces. The declaration
continues waiting and can bind when a newly advertised output has the selected
name. Matching uses the current native object, so a recycled global number
cannot revive an old closed binding.

A compositor-closed named role is retired without closing unrelated surfaces or
immediately recreating a role on the same object. It waits for a new native
binding or an explicit hide/remount. Late closure callbacks for already retired
roles do nothing. Compositor-selected default layers retain their existing
whole-client close behavior; this checkpoint does not establish general desktop
hotplug recovery for neutral defaults.

Inspector output includes the catalog, each ordinary surface's declared target
and closed named surface IDs. These are local diagnostics; public evidence uses
synthetic names and does not publish host display metadata.

Secure session-lock remains a separate runtime path. It creates lock surfaces
for every advertised output through the secure protocol and retains its
authentication gate. An ordinary layer output selector cannot restrict that
coverage or authorize an unlock.

## What is established

Pure SDK/runtime policy tests cover literal name invariants, unavailable/unnamed
and ambiguous targets, closed bindings, reconnect identity and reused global
numbers. All normal consumers compile with their neutral default. The example
demonstrates registration and typed output observation through the SDK.

Actual target placement, input/clipboard retirement, disconnection/reconnection
and secure multi-output acceptance still require a real compositor. They are
deferred under the current user instruction. No new renderer scaling support,
per-monitor persisted desktop layouts or fractional-scale protocol support is
claimed. The subsequent [surface instance API](surface-instances.md) supplies
concrete identity for multiple copies of one role; its independent example does
not establish the full desktop's persisted monitor policy.
