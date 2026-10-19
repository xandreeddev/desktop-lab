# Vulkan in the Lucent test VM

The prepared Lucent VM runs the full native desktop through **software Vulkan**:
wgpu selects Mesa lavapipe, reported as `llvmpipe` with `device_type: Cpu`.
Hyprland retains the guest's original virtio/virgl graphics device. The selected
renderer is observable in `lucent-cli inspect` and Lucent's startup log; using
the Vulkan backend does not establish hardware acceleration.

One wgpu device serves the desktop's native surfaces and shared resource cache.
The renderer requests Vulkan explicitly. Live-video decoding and fitting use
CPU FFmpeg through Rust bindings; GPU rendering and hardware video decoding are
separate capabilities. See [the framework](lucent-framework.md) and
[live wallpapers](live-wallpapers.md).

Inside an existing graphical session, these commands inspect the running shell
and the available Vulkan implementations without starting a second desktop:

```sh
lucent-cli inspect
vulkaninfo --summary
```

The packages used for this guest include `vulkan-swrast`, `vulkan-tools` and
`vulkan-virtio`. A physical Arch desktop needs its GPU's appropriate Vulkan
driver. Driver enumeration alone does not prove that Lucent selected that
driver or successfully presented through it.

## The controlled Venus trial on 2026-10-18

Venus forwards guest Vulkan operations to a host renderer. QEMU documents its
required blob resources and host-visible memory aperture in the
[VirtIO GPU guide](https://www.qemu.org/docs/master/system/devices/virtio/virtio-gpu.html).
The host's Radeon Vulkan driver independently ran offscreen renderer checks;
that result does not establish guest acceleration.

The later controlled trial used QEMU 11.1.1 and virglrenderer 1.3.0-2, shared
memfd guest backing, the existing single virtio GPU with `blob=true`,
`venus=true`, and a **512 MiB host-visible aperture**. Guest RAM stayed at
3 GiB with four vCPUs. This supersedes the older prototype's 1 GiB aperture
description. An aperture is an address window, not an upfront physical allocation
or a cap on total GPU memory.

Observed results were:

| Stage | Evidence and limit |
| --- | --- |
| Configuration preflight | XML validation and native command translation passed; unrelated arguments were unchanged. This did not prove renderer initialization. |
| Candidate boot | The host logged `failed to initialize venus renderer` and `virgl could not be initialized: -1`. Guest `virtio_gpu` initialization timed out waiting for a capability set. |
| Lucent startup | The running desktop selected CPU `llvmpipe` Vulkan. A working guest Venus device was not demonstrated. |
| Driver-only probe | The controller's forced virtio-ICD `vulkaninfo` probe followed its hardware-adapter gate. That gate failed first, so the probe **was not executed**. |
| Recovery | Graceful shutdown did not finish; the guest kernel recorded a blocked virtio GPU/DRM worker. Recovery revalidated the VM and restored its original graphics definition. Subsequent graphical verification passed with software Vulkan, advancing live-background submissions and preserved settings, source media and wallpaper. |

The recovered executable was the trial's identified release. This is a dated
result, not a new test of every later binary. The trial did not measure hardware
RSS, frame presentation rate, animation smoothness or hardware decoding.

## What explains the initialization failure

The failure is established; its exact failing system call is not. The captured
candidate retained libvirt's `spawn=deny` seccomp setting. In the
[QEMU 11.1.1 GPU implementation](https://github.com/qemu/qemu/blob/v11.1.1/hw/display/virtio-gpu-virgl.c),
Venus requests a separate render server. The
[virglrenderer 1.3.0 proxy server](https://gitlab.freedesktop.org/virgl/virglrenderer/-/blob/1.3.0/src/proxy/proxy_server.c)
falls back to `fork`/`exec` when its caller supplies no server file descriptor.
That combination makes process-spawning denial a strong explanation, but no
captured `fork` errno or controlled sandbox comparison proved it as the sole
cause. Missing Venus build support was ruled out by the installed package's
build configuration, server executable and dispatch implementation.

This distinction matters: changing Lucent's adapter preference or forcing a guest
ICD cannot repair a host renderer that failed to initialize. The failed trial
should not be repeated unchanged as a performance test.

## A hardware path still needs implementation and acceptance

Keep the host's global sandbox policy. Libvirt documents
[its seccomp boundary](https://libvirt.org/kbase/qemu-passthrough-security.html)
and provides no per-VM switch for that policy. Changing a global configuration
or merely pointing `RENDER_SERVER_EXEC_PATH` at another executable does not
establish a working isolated renderer; the latter still uses the spawning path.

An external renderer is a possible integration direction: start a separately
owned renderer, pass an explicitly owned connection to QEMU, and wire
virglrenderer's existing server-FD callback. The tested QEMU GPU does not supply
that callback. Its stock vhost-user GPU helper also does not enable Venus in
[the pinned implementation](https://github.com/qemu/qemu/blob/v11.1.1/contrib/vhost-user-gpu/virgl.c).
Consequently this is engineering work, not a supported drop-in XML fix or an
accepted hardware configuration. It requires its own reviewed lifecycle,
resource limits, device access and recovery, followed by actual guest driver,
native presentation and matched workload measurements.

## Recovery applies to a complete release

Graphics recovery and shell-release recovery affect different state. A graphics
trial must capture the current domain definition and its identity before any
change, restore that exact definition only while the guest is confirmed stopped,
and verify the original output and desktop after a normal login. The historical
`vm/enable-vulkan.py` helper and its first backup are not a substitute for a
fresh, identified transaction. Do not assume an old backup matches current VM
resources or configuration.

Lucent is now a supervised shell with separate desktop, CLI, menu, authorization,
lock and login clients. Restoring a release means restoring the identified,
compatible binary and helper set, including its supervisor/readiness contract,
with the affected services quiescent. Preserve settings, media, theme exports
and the verified secure recovery path, then check startup beyond its readiness
deadline. Copying the old prototype card executable over the desktop is not a
release rollback.

[Install and recovery](install-and-rollback.md) explains integration and explicit
stock-shell restoration. [Current readiness](current-readiness.md) records the
remaining resource and presentation targets. Software-Vulkan functional tests,
host offscreen GPU checks and visible changing pixels retain their individual
scopes; none proves guest hardware acceleration or sustained 60 FPS.
