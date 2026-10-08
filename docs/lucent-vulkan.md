# Vulkan in the Lucent test VM

The delivered demo uses wgpu's Vulkan backend with Mesa lavapipe inside the guest (`vulkan-swrast`). This is **software rendering**. The guest's original virtio/virgl graphics device remains in place for Hyprland. Vulkan transparency and the new shader animations are tested in this configuration; hardware acceleration is not claimed.

Install guest dependencies, then check the driver in a graphical terminal:

```sh
sudo pacman -S --needed vulkan-swrast vulkan-tools
vulkaninfo --summary
~/.local/bin/lucent-desktop
```

On a physical Arch desktop, install the appropriate Vulkan driver for its GPU instead. Lucent logs its selected adapter and `backend: Vulkan`; a `device_type: Cpu` adapter is software rendering. The renderer never silently falls back to OpenGL.

## Venus attempt and rollback

The host has Radeon Vulkan support. The attempted VM configuration added shared memfd backing and a 1 GiB host-visible aperture with `blob=true,venus=true`, as described by [QEMU's VirtIO GPU documentation](https://www.qemu.org/docs/master/system/devices/virtio/virtio-gpu.html). Guest `vulkan-virtio` was installed. QEMU 11.1.1/virglrenderer 1.3.0 failed to initialize Venus: libvirt starts QEMU with `spawn=deny`, while this renderer starts a separate helper process. The host Vulkan driver itself successfully enumerated its GPU.

The experiment did not change host security policy. The guest stalled during shutdown after the failed graphics initialization, so it was power-cycled and its saved original XML restored. Software Vulkan provides the working demo. Hardware Vulkan needs a separately reviewed renderer/process-isolation setup; do not disable the host's global sandbox to run this demo.

The optional `vm/enable-vulkan.py --experimental-venus` documents and reproduces the device change for hosts with a compatible setup. Plain invocation refuses to change the guest. It requires the Lucent guest to be stopped and saves the previous definition in ignored `.local-vms/lucent/before-venus.xml`. It does not alter sandbox policy. Restore it with:

```sh
python3 vm/lab.py shutdown lucent
python3 vm/enable-vulkan.py --restore
python3 vm/lab.py start lucent
```

A copy of the previous executable is retained inside this prepared guest as `~/.local/bin/lucent-desktop-before-drag`. Close the demo, then copy it over `~/.local/bin/lucent-desktop` to restore the older fixed card. Stock Omarchy and its locker are unaffected by either prototype.
