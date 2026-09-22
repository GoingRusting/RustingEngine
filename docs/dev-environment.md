# Verifiable development environment

## Software Vulkan device (lavapipe)

Mesa's lavapipe is the supported software Vulkan implementation for running
GPU-touching code and tests on a machine with no display and no discrete GPU.

- Package: `vulkan-swrast` on Arch, `mesa-vulkan-drivers` on Debian/Ubuntu.
- Invocation: set `VK_ICD_FILENAMES` to the lavapipe ICD json before running.
  The file name is distro-dependent — on this machine (Arch,
  `vulkan-swrast` package) it is
  `/usr/share/vulkan/icd.d/lvp_icd.json` (not `lvp_icd.x86_64.json`; check
  `ls /usr/share/vulkan/icd.d/` for the exact name on your machine).
- Verified: `VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json vulkaninfo --summary`
  reports a device with `vendorID = 0x10005` (Mesa's lavapipe vendor ID).

## Device requirements

`rendering::init_vulkan` (`src/rendering/mod.rs`) requests only:

- Instance extensions: the platform surface extensions from
  `Surface::required_extensions`, plus `ext_debug_utils` /
  `ext_validation_features` when the Khronos validation layer is present
  (debug builds only).
- Device extensions: `khr_swapchain`. No other device extensions.
- Device features: none (`Features::default()`/empty) — no explicit feature
  struct is requested anywhere in device creation.

Lavapipe supports `khr_swapchain` and presents to a real window via the X11
or Wayland surface extensions like any other ICD, so the engine's existing
windowed startup path runs unmodified under it — no headless-specific code
exists yet (see the "Headless mode" and `RUSTING_VULKAN_DEVICE` items in
`roadmap.md` Milestone 0.5, which are not implemented).
