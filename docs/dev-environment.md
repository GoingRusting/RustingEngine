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

## Iteration speed

The engine's target for a representative project is **3 seconds or less**
from a Rust edit to the first frame of the running game, as an incremental
Debug build. The representative project is the starter template
(`rusting new <dir> "Coin Run" --template starter`).
`tests/iteration_latency.rs` checks the target and prints a report:

```sh
cargo test --test iteration_latency -- --ignored --nocapture
```

The test runs `rusting run --ticks 1 --json` and reads `data.timings`. Each
timing starts when the command starts and ends at the game's first frame, so
it covers cooking, the cargo build and game startup. The test reports the
median of three samples. Set `RUSTING_LATENCY_REPORT=<file>` to also write
the report as JSON.

Measured on 2026-09-28 (Linux, AMD Ryzen 5 7600X, NVIDIA RTX 3060, headless,
shared `target/cli-games` build directory):

| Change | Build | Edit to first frame |
| --- | --- | --- |
| First build of a new project | 40.2 s | 40.3 s |
| No change | 100 ms | 129 ms |
| Edit `src/main.rs` | 824 ms | 853 ms |

In the editor, **Reload Code** measured 1.35 s from the click to the first frame of the
restarted game, with its window, for a generated 3D project. The editor
Console prints this time after each Play and each reload.
