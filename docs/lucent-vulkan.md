# Vulkan in the Lucent test VM

The Lucent test VM currently runs the coherent `8e6d074b` release through guest
**AMD RADV RENOIR** Vulkan using the retained AMD DRM native-context candidate.
The earlier `a7f41ecc` hardware-path liveness trial remains recorded below.
The final release's guarded deployment and uninstrumented 120-second resource
observation preserve the selected media and user/security state. Observed RSS is
217.28 MiB mean / 221.92 MiB sampled peak, but host swap/I/O-pressure eligibility
fails, so the 300 MiB acceptance result remains unset. Sustained FPS, animation
pacing and hardware video decoding are not established.
[Current readiness](current-readiness.md) separates these evidence scopes.

Earlier Venus and AMD attempts recovered to Mesa lavapipe, reported as
`llvmpipe` with `device_type: Cpu`; those dated results remain below. The selected
renderer is observable in `lucent-cli inspect` and Lucent's startup log. Using the
Vulkan backend alone does not establish hardware acceleration.

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

## AMD native-context trial on 2026-10-19

The earlier bounded AMD trial used DRM native context rather than Venus. It kept
one virtio GPU, EGL, 3 GiB RAM, four vCPUs, shared memfd backing, blob resources
and a 512 MiB host-visible aperture. The candidate enabled
`drm_native_context=true` with `venus=false`; the host's global sandbox and
existing security arguments stayed unchanged. The guest's matching RADV package
was an owned, reversible addition.

The RADV-only driver probe ran first and passed: Mesa 26.2.2 enumerated
**AMD Radeon Graphics (RADV RENOIR)** as an integrated GPU. Candidate boot, native
login, Lucent's hardware-adapter gate and initial rendered shell frames also
passed for the identified `a7f41ecc` release. This is actual guest hardware-path
initialization evidence, beyond the earlier host-only offscreen checks.

The next preservation check rejected the candidate because the home Btrfs
filesystem's observed `st_dev` changed from 56 to 57. It stopped before the
planned three-second liveness gate; the initial frames do not establish stable
hardware presentation. The original failed preservation result remains recorded.

Independent recovery verified the original software graphics semantics, all 963
original package versions, removal of the owned RADV addition, and exact settings,
media, sources and other non-device identity fields. Across 189 captured
identities, only the acknowledged device remap remained. The baseline did not
record a mount UUID, so this is a qualified device-remap recovery proof rather
than a claim that every raw identity field remained equal. The original
`a7f41ecc` desktop ran again on software Vulkan at the end of that recovery.

### Later mount-anchored liveness trial

A subsequent bounded trial at 2026-10-19 20:11 UTC uses controller
`a413558a19b59b72111892739d4c4c1067e3804b72e8b160d5f9a59aac968fb6`
and guest worker
`532a548359d2c16e4334fab940189d33b6b29429311a1fb2f227e17347e156d4`.
It records filesystem UUID and Btrfs subvolume anchors across reboot before
comparing persistent identities. This corrects the guard's unstable kernel
`st_dev` assumption; the original rejected trial is not relabeled passed.

RADV-only enumeration reports **AMD Radeon Graphics (RADV RENOIR)** with Mesa
26.2.2 and Vulkan. Secure login, Lucent's actual hardware-adapter gate and native
frame-counter advancement over three seconds pass at source
`a7f41eccdcd8eca51e8e5ee878d8f489d8d4bde5`, desktop SHA256
`2fb52754e75c34d9d73fff26d72a240f3e9a8d8672563bb6e0e404e9427a4f78`.
The output remains Virtual-1, 1920×1080, scale1, 60 Hz. User/source/settings/media,
critical file and monitor preservation passes with the anchored mount authority.
The matching owned RADV addition and native-context candidate are retained.
The private receipt is `reports/local/daily-amd-native-radv-liveness.json`.

These scoped observations establish startup and frame advancement on the named
older binary. They do not measure distinct video-source frames, compositor
presentation intervals, sustained FPS, animation pacing, RSS, GPU memory or
hardware decoding. The software-Vulkan resource measurements retain their
original scope. The later `8e6d074b` deployment and resource observation below
remain separate from this older-binary trial.

### Coherent release and normal resource observation

The guarded `8e6d074b` deployment verifies all six managed user executables and
the root greeter, changes only the required compatibility helper, preserves the
existing user/security configuration and verifies rendered supervision beyond
its startup deadline. Acceptance executables are staged separately. Greetd and
the graphics configuration are not restarted or replaced by this deployment.

On the same retained RADV adapter, the uninstrumented normal-video phase collects
120.000492 seconds and 480 RSS samples at 0.25-second spacing: **217.28 MiB mean /
221.92 MiB sampled peak**, 112.25% of one logical CPU core and zero process swap.
The original H.264/yuv420p 3840×2160 60/1-FPS video remains selected with source-rate
playback on one 1920×1080 scale-1 60-Hz output. Full source/settings/media/export,
critical-file, process and output preservation checks pass within the interval.

The host records 674 swap-in pages, zero swap-out pages and an I/O full-stall
fraction of 0.5081, with at least 4152.29 MiB available memory. These exceed the
reviewed eligibility bounds: the report retains `eligible=false`, reasons
`host_swap_activity` and `host_full_stall_pressure`, and
`normal_uninstrumented_rss_target_met=null`. It does not accept the 300 MiB target
or automatically retry. Presentation/PTS tracing is off, so this phase supplies
no video-delivery or compositor-FPS evidence. It is not a matched comparison
with the historical 384.82/403.88 MiB software-Vulkan cohort.

The private receipts are
`reports/local/daily-text-8e-deployment-33ba0219a8a4a8a4c924f896.json` and
`.cache/final-cohort-measurement-v2/measurement-8e-radv-1792442023-resources.json`.
Native text/application/clipboard/tray/multi-output and physical-device acceptance,
instrumented presentation and hardware decoding remain separate pending work.

## A separate external-renderer integration remains unimplemented

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
scopes. The later AMD trial additionally establishes guest hardware-path startup
and three-second native frame advancement on `a7f41ecc`. The later `8e6d074b`
release is deployed and its normal resource interval is collected but ineligible;
sustained presentation, 60 FPS and resource-target acceptance remain unverified.
