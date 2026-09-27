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

## Shader compiler for the determinism check

`--features gpu-tests` also runs `simulation_shaders_are_precise_and_never_relaxed`,
which compiles the physics shaders with `glslc` and scans the SPIR-V (see
`docs/determinism.md`). Install `shaderc` on Arch, `glslc` on
Debian/Ubuntu, or the Vulkan SDK; `glslc --version` must work.

## Device requirements

Windows open through `vulkano_util::VulkanoContext`, configured by
`rendering::vulkano_config` (`src/rendering/mod.rs`). It requests only:

- Instance extensions: the platform surface extensions, plus
  `ext_debug_utils` / `ext_validation_features` when the Khronos validation
  layer is present (debug builds, or any build with the `validation`
  feature).
- Device extensions: `khr_swapchain`.
- Device features: `sampler_anisotropy` when the device supports it.

Lavapipe supports `khr_swapchain` and presents to a real window via the X11
or Wayland surface extensions like any other ICD, so the engine's existing
windowed startup path runs unmodified under it — no headless-specific code
exists yet (see the "Headless mode" and `RUSTING_VULKAN_DEVICE` items in
`roadmap.md` Milestone 0.5, which are not implemented).
