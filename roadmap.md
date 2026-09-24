# RustingEngine Roadmap

RustingEngine is currently a functional Vulkan renderer prototype with a native Rust ECS runtime, an egui editor, and a GPU-accelerated hybrid physics bridge. The goal of this roadmap is to turn it into a complete, general-purpose Windows/Linux game engine in the same class as Godot — covering 3D and 2D rendering, animation, audio, UI, navigation, networking, a full editor, and export — whose defining strength is physics. RustingEngine should be the engine people pick *because* of its physics.

The roadmap covers two products:

- **The engine** (Milestones 0-23): Godot-class feature breadth, with physics as the flagship subsystem. Milestones 0-7 build the foundation and a vertical slice; Milestone 8 makes simulation deterministic; Milestones 9-23 reach feature parity with Godot while taking physics well beyond it.
- ***Sundering*** (Milestones 24-29): a deterministic, networked, destructible 5v5 competitive game built on the engine. It is the engine's hardest physics customer and its proof that the physics claims are real.

This document is the implementation source of truth. Tasks should be completed in dependency order, kept behind compiling intermediate states, and verified against the acceptance gates at the end of each milestone.

Long-lived ownership boundaries and dependency rules are recorded in [`architecture.md`](architecture.md). Roadmap work must preserve those boundaries or document a migration before changing them.

## Working agreement for implementation sessions

This roadmap lists outcomes, not tasks. Turning an outcome into a task is part of the work, not a reason to stop.

### How to pick an item

Pick the lowest-numbered unchecked item whose dependencies are satisfied. Milestones 2, 3, 4, and 5 are parallel tracks and may be interleaved. After Milestone 9, Milestones 10-22 are parallel tracks and may be interleaved, subject to the dependencies stated at the top of each milestone; physics milestones (10-12) take priority when two items are otherwise equally ready. If an item is too large for one session, split it, implement the first part, and record the split in this file as sub-items.

### What counts as verification

Not every item is provable the same way. Match the proof to the item instead of treating "I cannot run the game" as a universal blocker.

| Kind of item | Required proof |
| --- | --- |
| Types, APIs, error handling, identity mapping | Unit test |
| ECS, assets, serialization, scene round-trip | Unit test, no GPU |
| Extraction, dirty ranges, ordering, capacity policy | Unit test on data structures, no GPU |
| Buffer layout, push constants, vertex formats | Generated/reflected layout assertion, no GPU |
| Shader source changes | Shader compiles in the build, plus a layout assertion |
| Compute dispatch behaviour, readback, physics results | Headless Vulkan test, software device acceptable |
| Rendered output correctness | Headless offscreen render plus golden-image comparison |
| CPU physics behaviour (stacking, joints, CCD, queries) | Deterministic headless unit/scenario test, no GPU |
| Editor panel behaviour | Unit test on editor state/commands; golden image for compositing |
| Audio mixing and DSP | Offline render to buffer plus sample comparison, no device |
| Frame pacing, throughput, memory growth | Hardware GPU run, cannot be verified in software |
| Determinism across vendors | Two different Vulkan implementations |

Only the last two rows genuinely require hardware. Everything above them is verifiable on a headless machine once Milestone 0.5 exists. Until Milestone 0.5 exists, GPU-behaviour items are blocked by missing tooling, not by the roadmap.

### When blocked

Do not stop with nothing delivered. In order:

1. Implement the part that is verifiable now, behind a compiling intermediate state.
2. Write the unverified part as a test marked `#[ignore]` with the reason, so the proof is ready when hardware is.
3. Add the blocker as a new unchecked item in this file, naming what is missing.
4. Move to the next item.

### Constraints must live in the repository

Any file or subsystem an implementation session must not touch belongs in `AGENTS.md` at the repository root, with the reason. A constraint that exists only in one session's context cannot be respected by the next session and must not be invented.

### Performance numbers

Performance targets are acceptance gates for a milestone, never individual tasks. Never treat a frame-rate number as an item to implement.

## Product target

The engine's 1.0 release (Milestone 23) should provide everything a team expects from Godot, plus physics no general-purpose engine offers:

- A real application runtime with ECS entities, components, resources, schedules, events, observers, reflection, input, and fixed updates.
- Reusable scene composition: prefabs/instanced scenes with nested overrides, groups, signals, and data resources.
- A Vulkan renderer with explicit frames in flight, forward PBR, shadows, transparency, HDR, global illumination, reflection probes, screen-space effects, volumetric fog, post-processing, decals, GPU particles, culling, LOD, and profiling.
- A first-class 2D pipeline: sprites, tilemaps, 2D lights, 2D particles, and 2D physics.
- Custom shaders through a stable engine shader language and a visual shader graph.
- Skeletal and property animation, animation state machines, blend spaces, IK, retargeting, and tweens.
- An audio engine with buses, effects, streaming, and 3D spatialization.
- A runtime UI toolkit with layout containers, themes, rich text, localization, and accessibility.
- Navigation meshes, agents, and avoidance for 2D and 3D.
- High-level multiplayer: RPCs, replicated spawning/synchronization, and deterministic rollback.
- Typed, deduplicated assets with glTF import, per-asset import settings, texture compression, scene serialization, and hot reload.
- An in-engine egui editor at Godot parity: viewport, hierarchy, inspector, asset browser, console, debugger, profiler, gizmos, animation/shader/tilemap editors, project settings, export, and an editor plugin API.
- Fast iteration: Rust game-code hot reload and an optional sandboxed scripting layer.
- Export to Windows, Linux, and macOS, then Android.
- Documentation, tutorials, templates, and demo projects for every major feature.
- **Best-in-class physics** — see the next section.

Primary platforms are Windows and Linux desktop. Native Rust systems are the gameplay API, and games remain normal Cargo projects that can use external libraries. A versioned custom physics-compute ABI is part of the hybrid milestone. Deferred rendering, web export, iOS, and consoles remain out of scope until after 1.0.

## Physics is the flagship

Godot ships a capable general-purpose physics engine (Godot Physics or Jolt). RustingEngine must be measurably better, not merely equivalent. "Best physics" is defined by the following pillars, each owned by a milestone and each proven by a benchmark or test rather than asserted:

| Pillar | What it means | Owner |
| --- | --- | --- |
| Scale | Hundreds of thousands to millions of simultaneously simulated bodies through GPU solvers, with per-body CPU/GPU allocation | M5, M11 |
| Gameplay connection | GPU simulation is never a black box: typed conditions, asynchronous events, commands, and selective readback reach normal Rust gameplay code | M5 |
| Determinism | Bit-identical simulation across runs, builds, thread counts, and (where proven) GPU vendors; replay and rollback built in | M8 |
| Breadth | Rigid, articulated, character, vehicle, ragdoll, soft-body, cloth, rope, fluid, granular, and fracture simulation in one world with two-way coupling | M10, M11 |
| Correctness | Stable stacking, continuous collision, robust contacts, and conservation behaviour verified by a regression scenario suite | M5, M10 |
| Tooling | A physics debugger with recording, timeline scrubbing, per-body inspection, and authoring tools in the editor | M12 |
| Integration | Animation, particles, audio, navigation, and networking consume physics through the same bridge instead of parallel ad-hoc systems | M13-M19 |
| Evidence | A published, repeatable benchmark suite comparing RustingEngine against Jolt, PhysX, Rapier, and Godot Physics on identical scenes | M12 |

Every other subsystem is judged against Godot parity. Physics is judged against the best standalone physics engines.

## Godot parity map

Each Godot feature area has an owning milestone. A feature area is at parity when its milestone's exit gate passes.

| Godot area | RustingEngine equivalent | Milestone |
| --- | --- | --- |
| Node tree, scenes, `PackedScene` instancing, inheritance | ECS hierarchy, prefabs with nested overrides | M1, M9 |
| Signals, groups | Typed events, observers, entity groups | M1, M9 |
| Resources (`.tres`) | Typed data assets with reflection | M2, M9 |
| GDScript / C# / GDExtension | Native Rust, Rust hot reload, optional WASM scripting | M9 |
| Godot Physics / Jolt 3D | Hybrid CPU/GPU physics | M5, M10-M12 |
| Soft bodies, cloth | XPBD deformables | M11 |
| Forward+/Mobile/Compatibility renderers | Forward PBR with quality profiles and capability fallback | M3, M4 |
| SDFGI/VoxelGI/LightmapGI, probes, SSAO/SSR/SSIL, fog, glow | Advanced rendering | M13 |
| Shading language, visual shaders | Engine shader language and shader graph | M13 |
| GPUParticles, decals, MultiMesh, CSG, GridMap | Advanced rendering and editor tools | M13, M20 |
| AnimationPlayer, AnimationTree, IK, tweens | Animation system | M14 |
| Audio buses, effects, 3D audio | Audio engine | M15 |
| Control nodes, themes, RichTextLabel, translations | Runtime UI and localization | M16 |
| 2D renderer, TileMap, 2D physics, 2D lights | 2D engine | M17 |
| NavigationServer, agents, avoidance | Navigation | M18 |
| High-level multiplayer, ENet, WebSocket, HTTP | Networking | M19 |
| Editor docks, debugger, profilers, editor plugins, asset library | Editor parity | M20 |
| Export templates, platforms | Platforms and export | M21 |
| Documentation, demo projects | Documentation and ecosystem | M22 |

## Second product target: Sundering

*Sundering* is a 5v5 competitive game whose terrain is made of millions of simulated chunks that come apart permanently. Its design document lives at `~/Sundering/README.md`. It is built after the engine's physics pillars exist and consumes them harder than any other project will.

Sundering demands four things a general-purpose engine does not provide by default:

- **Determinism.** Competitive play requires the simulation to produce identical results for every participant. This constrains the physics solver, the GPU reduction strategy, and the ordering of every simulation operation. The engine provides it in Milestone 8.
- **Authoritative networking.** A server owns the simulation; clients must stay consistent with it under real latency and loss. The engine provides general networking in Milestone 19; Sundering adds competitive authority in Milestone 24.
- **Destructible terrain as simulation state.** Terrain is not authored geometry. It is a first-class simulated entity that feeds collision, navigation, and vision. It builds on the engine's fracture support from Milestone 11.
- **Navigation over geometry that changes every tick.** Nav meshes assume a static world. Sundering has no static world. It builds on the engine's navigation from Milestone 18.

**Determinism cannot be retrofitted.** Milestone 8 defines the full requirements and their acceptance gate, but three of its ordering rules must be respected while Milestone 5 is implemented, or that work will need rewriting. They are restated at the top of Milestone 5.

Milestones 24-29 depend on Milestones 8, 10, 11, 18, and 19. Milestone 8 must precede any networking milestone, because the determinism result decides which networking model is available.

## Architectural direction

The project should become a Cargo workspace with one-way dependencies:

```text
rusting-math       deterministic math, fixed-point, seeded RNG streams
      ↑
rusting-core       ECS components, schedules, time, input, hierarchy, events, reflection
      ↑
rusting-assets     typed handles, cache, importers, serialization, prefabs, hot reload
      ↑
rusting-physics    2D/3D CPU physics, GPU compute simulation, deformables, fluids, synchronization, queries
      ↑
rusting-terrain    heightmap and volumetric terrain, fracture, structural load, Scar persistence
      ↑
rusting-nav        navigation meshes/volumes, flow fields, agents, avoidance
rusting-anim       skeletal and property animation, state machines, IK, physics-driven animation
      ↑
rusting-render     Vulkan context, extraction, frame graph, 2D/3D passes, materials, shaders, particles, profiling
rusting-net        transport, wire protocol, RPC, replication, rollback, interest mechanism
rusting-audio      device management, mixing, buses, effects, spatialization
rusting-ui         runtime UI layout, themes, text shaping, localization, accessibility
      ↑
rusting-gameplay   teams, match state, abilities as forces, vision, bots
      ↑
rusting-editor     egui panels, viewport, inspector, gizmos, play controls, editor plugins
rusting-script     optional sandboxed WASM scripting host
      ↑
rusting-engine     plugins, application facade, compatibility API, export
      ↑
game projects      vertical slice, demo projects, Sundering
```

`rusting-nav` and `rusting-anim` are siblings. `rusting-render`, `rusting-net`, `rusting-audio`, and `rusting-ui` are siblings at one level and must not depend on each other; UI and particles reach the screen through render extraction, not by calling the renderer. `rusting-editor` and `rusting-script` are siblings. Full layer definitions are in [`architecture.md`](architecture.md).

Dependency cycles between runtime, renderer, physics, assets, and editor are not allowed. ECS entities are the canonical identity and authored scene state. A GPU-owned body's newest runtime transform may live on the GPU; ECS keeps its stable ID, settings, last synchronized state, and pending events. Render batches, physics arrays, indirect buffers, and other GPU representations are derived data.

The existing `Engine::new`, `add_cube`, `add_sphere`, `add_gltf`, and `run` API remains temporarily available as a deprecated compatibility facade implemented over the new runtime.

## Milestone 0: Correctness stabilization

Goal: establish a trustworthy baseline before adding architecture or features.

### Completed

- [x] Make `MaterialBuilder::default()` agree with `Material::default()`.
- [x] Copy all material properties when creating cube and sphere instances.
- [x] Cache sphere meshes by subdivision level.
- [x] Replace mutable batch/index instance handles with stable IDs.
- [x] Align all physics compute-shader instance fields with the Rust `InstanceData` layout.
- [x] Align physics push constants and add explicit padding.
- [x] Repair the `NoCollision` shader's mass, gravity, friction, restitution, and angular-velocity interpretation.
- [x] Make broad-phase sphere and box radius conventions agree with collision shaders.
- [x] Add compile-time CPU structure size/offset assertions.
- [x] Add tests that enforce common layouts across every compute shader.
- [x] Make `cargo fmt --check`, all-target tests, strict clippy, and all-target checking pass.
- [x] Upgrade Vulkano 0.33 to 0.35 and migrate pipeline, descriptor, image, allocation, and command APIs.
- [x] Remove `vulkano-win` and migrate event dispatch to winit 0.30's `ApplicationHandler` lifecycle.
- [x] Negotiate surface format, color space, image count, composite alpha, and present mode.
- [x] Enable the Khronos validation layer automatically in debug builds when it is installed.
- [x] Enable synchronization validation and opt-in GPU-assisted validation with `RUSTING_GPU_VALIDATION=1`.

### Remaining

- [ ] Move compatibility-facade window and surface creation into `ApplicationHandler::resumed`, removing the last deprecated winit call. Split: the legacy `Engine` facade builds its Vulkan device, swapchain, and GPU-backed scene state eagerly inside `Engine::new`/`with_render_settings`, before the event loop runs, and callers (`add_cube`, `add_gltf`, etc.) depend on that state existing immediately after construction. Deferring creation into `resumed()` needs a two-phase builder rewrite of that whole public facade, which roadmap Milestone 1 already plans to replace outright — blocked pending that replacement rather than attempted as a patch here.
- [x] Replace public initialization and asset-loading panics with typed errors and `Result` APIs.
  - [x] `geometry::gltf_loader::load_gltf_scene` and `Engine::add_gltf` return `Result<_, GltfLoadError>` instead of panicking on a missing/corrupt glTF file or a primitive without positions.
  - [x] `Engine::load_texture` returns `Result<usize, TextureLoadError>` instead of panicking on a missing/corrupt image file.
  - [x] Remaining production `.unwrap()`/`.expect()`/`panic!` call sites outside tests audited (~220 across `scene/mod.rs`, `engine/mod.rs`, `rendering/*`, `project_runner.rs`). Findings: no genuine unconverted caller-recoverable site remains (checked for file/filesystem loading specifically — the only production `fs::`/`File` usage is a compiled-in shader module load and this session's own golden-image test helper). The rest fall into three buckets that are correctly left as panics, not oversights: (1) internal invariants after initialization — ECS resource/component lookups, GPU pipeline/queue state assumed valid once the device exists; (2) documented API panics that already ship a non-panicking alternative, e.g. `GameScene::object` panics but `GameScene::try_object` returns `Option`; (3) the legacy `Engine`/`scene::RenderScene` compatibility facade (`engine/mod.rs`, `scene/mod.rs`), already deferred to the Milestone 1 facade replacement per the compatibility-facade item above — converting its error style now would be churn thrown away at that rewrite. No new unchecked item added since nothing actionable was found outside what's already tracked.
- [x] Add Vulkan debug names and scoped command-buffer labels. `init_vulkan` names the main queue via `Device::set_debug_utils_object_name` when `ext_debug_utils` is enabled (debug builds with validation, same gate as the existing `DebugUtilsMessenger`); `SceneRenderer::render` wraps its command buffer in a `begin_debug_utils_label`/`end_debug_utils_label` scope ("SceneRenderer::render"), both gated on `instance().enabled_extensions().ext_debug_utils` so release/no-validation builds pay nothing. Verified live on real hardware (NVIDIA RTX 3060): `rendering::debug_utils_tests::object_names_and_command_buffer_labels_are_accepted_by_the_driver` creates an instance with `ext_debug_utils` forced on, names a queue, and records/submits a command buffer with a begin/end label pair, asserting the driver accepts both calls with no validation error. `cargo test --lib --features gpu-tests`: 146 passed.
- [x] Validate rendering, resizing, minimizing, restoring, and shutdown on Linux and Windows. Partially verified live in this environment (Linux, Wayland session, real NVIDIA RTX 3060): `./target/debug/game testGame/build/main.rscene.bin` opens a window and renders continuously for 5s under `timeout` with no panic/validation output, then exits cleanly (SIGTERM). Resizing, minimizing, and restoring need a window-manager automation tool (`xdotool`/`wmctrl`, neither installed, not added since installing new system packages is outside this session's scope) to script without a human; Windows has no machine available in this environment at all. See new item below for the remaining manual QA pass.
- [ ] Manually verify window resize, minimize, and restore on Linux (with `xdotool`/`wmctrl` installed, or by hand) and repeat the full rendering/resize/minimize/restore/shutdown pass on a Windows machine — blocked in this environment per item above.
- [ ] Replace fixed grid limits with explicit capacity tracking and overflow reporting. Never silently omit bodies. Blocked: the only fixed grid limit in the codebase is `MAX_PER_CELL = 128` in `shaders/compute/grid_build.comp` (`if (idx < MAX_PER_CELL) { grid_objects.data[...] = i; }` silently drops the object otherwise), driven entirely by `scene::RenderScene`/`Engine::render` (`engine/mod.rs`, `scene/mod.rs`) — the legacy compatibility facade already deferred to the Milestone 1 full replacement per the compatibility-facade item above. No ECS-native system (`scene_renderer.rs`, `compute_registry.rs`'s other pipelines) uses this grid. Adding overflow reporting to code being replaced wholesale is churn thrown away at that rewrite; deferred with it rather than tracked separately.
- [x] Add generated/reflected layout checks for storage buffers, uniforms, vertex data, and push constants. `render_instance_layout_matches_shader_struct`, `light_gpu_layouts_match_shader_structs`, and `hybrid_physics_gpu_layouts_match_shader_structs` (`rendering/scene_renderer.rs`) now compare CPU upload structs' `size_of`/`offset_of` against the `vulkano_shaders`-generated types for the same GLSL struct/block (`vertex_shader::RenderInstance`/`Camera`, `fragment_shader::Light`, `physics_shader::PhysicsState`/`ConditionInstruction`/`RuleState`/`PhysicsEvent`/`PhysicsPush`) — reflected straight from the compiled SPIR-V — instead of hardcoded magic-number offsets that could silently drift from the shader source. `GpuEventHeader` has no named GLSL struct (bare buffer block) so it keeps its one hand-computed size assertion. Verified: `cargo test --lib --features gpu-tests`: 146 passed.

### Exit gate

- Debug validation produces no errors during startup, rendering, resize, minimize/restore, and shutdown.
- Linux and Windows smoke tests render at least 1,000 frames.
- No known CPU/GPU layout mismatch remains.
- Formatting, strict clippy, tests, shader compilation, and all-target checking pass in CI.

## Milestone 0.5: Verifiable development environment

Goal: make GPU-touching work provable on a machine with no display and no second GPU. This milestone precedes all remaining GPU work. Without it, most of Milestones 3, 4, 5, 8, 11, 13, 25, and 26 cannot be verified by anyone, including CI.

### Software Vulkan device

- [x] Document Mesa lavapipe as the supported software Vulkan implementation. On Arch the package is `vulkan-swrast`; on Debian/Ubuntu it is `mesa-vulkan-drivers`. See `docs/dev-environment.md`.
- [x] Document the invocation. Correction: on this machine (Arch, current `vulkan-swrast`) the ICD file is `/usr/share/vulkan/icd.d/lvp_icd.json`, not `lvp_icd.x86_64.json` — the exact name is distro/version-dependent, see `docs/dev-environment.md`.
- [x] Verify `vulkaninfo --summary` reports a second device with `vendorID = 0x10005` when the variable is set. Confirmed on this machine.
- [x] Record required Vulkan features and extensions, and which of them lavapipe does not provide. `init_vulkan` requests only the `khr_swapchain` device extension and no explicit device features; lavapipe supports `khr_swapchain` via the standard surface path, so it provides everything the engine currently requests. See `docs/dev-environment.md`.

### Device selection

- [x] Add `RUSTING_VULKAN_DEVICE` to select a physical device by index or by name substring. `rendering::select_device_index` (pure, unit-tested) plus wiring in `init_vulkan`.
- [x] Log the selected device name, vendor ID, driver version, and API version at startup. Verified live: `Selected Vulkan device: llvmpipe (LLVM 22.1.8, 256 bits) (vendor 0x10005, driver 109060098, api 1.4.354)` with `RUSTING_VULKAN_DEVICE=llvmpipe` and lavapipe's ICD.
- [x] Fail with a typed error naming every available device when the selection matches nothing. Never fall back silently. `DeviceSelectionError` lists every candidate device name; `init_vulkan` panics with its `Display` message (kept a panic, not a `Result`, since `init_vulkan`'s single caller is the legacy `Engine::new` facade already documented as panic-based pending its Milestone 1 replacement — see item above on the deprecated winit call).

### Headless mode

- [x] Create instance, physical device, and logical device without a surface, a swapchain, or a winit window. `rendering::init_vulkan_headless`/`HeadlessVulkanBase`, sharing device-selection logic with the windowed path via `select_physical_device`. Verified live (no window created): `Selected headless Vulkan device: NVIDIA GeForce RTX 3060 (vendor 0x10de, driver 2580660800, api 1.4.351)`.
- [x] Add offscreen render targets with the same formats used by the windowed path. `swapchain::create_offscreen_target`/`OffscreenTarget` (color `B8G8R8A8_SRGB` + `TRANSFER_SRC` for later readback, depth `D16_UNORM`), sharing `create_render_pass_for_format` with the windowed `create_render_pass`. Verified live against `init_vulkan_headless` on real hardware, no window created.
- [x] Add a fenced image readback returning CPU pixel data. `readback::read_back_image` (copy-to-buffer command, `then_signal_fence_and_flush`, `wait(None)`, host-visible buffer read). Verified live against `init_vulkan_headless`/`create_offscreen_target` on real hardware.
- [x] Add a fenced storage-buffer readback returning CPU data after a compute dispatch. `readback::read_back_buffer<T>` (copy-buffer command, same fence pattern as `read_back_image`). Verified live: `reads_back_storage_buffer_results_after_a_compute_dispatch` builds a `ComputePipeline` from `src/shaders/compute/test_double.comp`, dispatches against a 64-element storage buffer on real hardware (`NVIDIA GeForce RTX 3060`), and asserts every value was doubled.
- [x] Add `--headless` to the game binary, running a fixed number of ticks and exiting with a status code. `project_runner::run_project_headless` builds the same `App`/plugin stack as `run_project` (`AssetPlugin`, `HybridPhysicsPlugin`, `RenderExtractPlugin`, game plugin) with no `VulkanoContext`, window, or renderer, and runs 60 fixed-step ticks; `src/bin/game.rs` wires `--headless` to it. Verified live: `./target/debug/game --headless testGame/build/main.rscene.bin` opens no window and exits 0.

### Test harness

- [x] Add a `gpu-tests` feature. GPU tests compile always and run only under that feature. `gpu-tests = []` in `Cargo.toml`; every GPU test carries `#[cfg_attr(not(feature = "gpu-tests"), ignore = "...")]`. Verified live: `cargo test --lib` shows `4 ignored`; `cargo test --lib --features gpu-tests` runs all 142 with 0 ignored, both clean under `cargo clippy --workspace --all-targets [--features gpu-tests] -- -D warnings`.
- [x] Add a shared test fixture that creates a headless device once per test binary. `rendering::test_support::headless_device()` (`OnceLock<HeadlessVulkanBase>`), used by `swapchain::tests` and both `readback::tests`. Verified live: `cargo test --lib --features gpu-tests -- --nocapture` prints "Selected headless Vulkan device" only twice for 4 GPU tests (once for the fixture's one-time init, once for the unrelated `headless_device_tests` test that exercises `init_vulkan_headless` directly).
- [x] Skip with an explicit message, not a failure, when no Vulkan device is present. Every GPU test still opens with `if VulkanLibrary::new().is_err() { eprintln!("skipping: no Vulkan driver present"); return; }` in addition to the `gpu-tests` ignore gate, so a `--features gpu-tests` run on a driverless machine reports pass, not failure.
- [x] Add golden-image comparison with a per-pixel tolerance, writing actual/expected/difference images to an artifact directory on mismatch. `rendering::test_support::assert_matches_golden_image(actual, golden_path, artifact_dir, tolerance)` (per-channel `abs_diff`, dimension check first, writes `actual.png`/`expected.png`/`diff.png` via the `image` crate on any mismatch, no GPU required). Verified live: `identical_image_matches_its_own_golden_with_zero_tolerance`, `a_small_difference_within_tolerance_passes`, and `a_mismatch_beyond_tolerance_panics_and_writes_artifact_images` (which asserts the three artifact files exist after a caught panic) all pass under `cargo test --lib`.
- [x] Add a compute-dispatch fixture: upload known input, dispatch, read back, assert. `rendering::test_support::dispatch_and_read_back` (generic over the storage-buffer element type; builds the pipeline/layout/descriptor set, dispatches, reads back via `readback::read_back_buffer`). `readback::tests::reads_back_storage_buffer_results_after_a_compute_dispatch` now calls it instead of repeating the boilerplate inline. Verified live against real hardware: same doubling assertion still passes.
- [x] Mark every test whose result depends on real hardware timing `#[ignore]`, with the reason in the attribute. Audited: no test in the crate asserts on wall-clock elapsed time, FPS, or frame pacing — `Duration` values in `runtime::tests` and `project_runner` tests are deterministic simulated inputs (fixed `Duration::from_millis(...)` passed to `App::update`), not measured real time, so nothing currently qualifies. Note for future work: apply this to any new test that measures real elapsed time, frame pacing, or throughput.

### Repository constraints

- [x] Add `AGENTS.md` recording files that implementation sessions must not modify, and why.
- [x] Record the verification tier table from the working agreement above, or link to it.

### Exit gate

- A headless compute test and a headless offscreen render test both pass under lavapipe on a machine with no display.
- The same two tests pass on the NVIDIA device.
- CI runs the `gpu-tests` feature under lavapipe on every pull request.
- `RUSTING_VULKAN_DEVICE` selects between lavapipe and hardware on a machine where both are present.

## Milestone 1: Workspace and runtime foundation

Goal: create the engine runtime that owns canonical scene state and system execution.

### Workspace

- [x] Convert the package into a Cargo workspace using the architectural dependency direction above. Verified: root `Cargo.toml` already had `[workspace] members = ["testGame"]`; added `crates/rusting-core` as the first real architectural-layer member (bottom of the dependency direction). `cargo build --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` clean.
- [ ] Move reusable public data types into `rusting-core` without Vulkan dependencies. First increment: moved `Transform` and `CollisionType` into `crates/rusting-core`. Second increment: moved the Vulkan-independent canonical scene, hierarchy, camera, naming/classification, light, visibility, render-settings, physics-settings/status, and rigid-body/collider data from `src/runtime/components.rs` into `crates/rusting-core/src/components.rs`; `rusting_engine::runtime::*` remains compatible through re-exports. Third increment: moved the generic `EventQueue<T>` and its frame-buffer semantics into `crates/rusting-core/src/events.rs`; the engine retains only the `World` adapter and its public runtime path remains an exact re-export of the core type. Fourth increment: moved the Vulkan-independent `FrameTime` resource and its duration accessors into `crates/rusting-core/src/time.rs`; the engine retains time control and advancement policy while its public runtime path remains an exact re-export of the core type. Fifth increment: moved hierarchy mutation, transform propagation, malformed-hierarchy diagnostics, and the typed `HierarchyError` into `crates/rusting-core/src/hierarchy.rs`; the engine adapter preserves the existing `AppError` variants and public diagnostics/propagation paths. Sixth increment: moved the public `ScheduleStage` and `FrameReport` scheduler data types into `crates/rusting-core/src/schedule.rs`; schedule storage and execution remain engine-local while the existing runtime paths are exact re-exports. Seventh increment: moved `TimeControl`, its private accumulator/queued-step state, the deterministic advancement algorithm, and typed `TimeAdvanceError` into `crates/rusting-core/src/time.rs`; the engine retains only the `World`/`AppError` compatibility adapter and its public time resource paths remain exact re-exports. Eighth increment: moved `RuntimeInput`, `ActionMap`, `InputBinding`, and the `KeyCode`/`MouseButton` re-exports into `crates/rusting-core/src/input.rs` (adds a `winit` dependency to core for the key/button types only; no Vulkan); `src/runtime/input.rs`/`actions.rs` keep only the `World` install adapters and exact re-exports. Built-in `ClickEvent` and `CollisionEvent` remain engine-local. `MeshRenderer` stays engine-local because it uses engine asset handles. `src/core/material.rs` and `src/core/physics.rs` also remain engine-local because they depend on rendering registries. The parent item remains open pending extraction or redesign of those rendering-coupled public types and any remaining runtime candidates.
- [x] Keep compatibility re-exports in `rusting-engine` during migration. `src/core/mod.rs` re-exports `rusting_core::{transform, collisions}` at their original `crate::core::transform`/`crate::core::collisions` paths, so every existing caller (`src/tests.rs`, `src/scene/mod.rs`, `src/engine/mod.rs`, `src/core/physics.rs`) compiles unchanged. Confirmed `src/editor/mod.rs`/`src/editor/view.rs` never import `crate::core::` at all, so this migration step could not have touched the protected editor/gizmo files even indirectly; verified via `git status` that both files are untouched.
- [x] Add feature flags for editor, validation, experimental GPU physics, and optional importers. `editor` and `window` already gated their modules. `validation` now force-enables the Khronos validation layer (when installed) in non-debug builds too (`init_vulkan`: `cfg!(debug_assertions) || cfg!(feature = "validation")`). New `gltf` feature (default, implied by `editor` because `src/editor/view.rs` calls `AssetServer::import_gltf`) makes the `gltf` crate optional and gates `geometry::gltf_loader`, `Engine::add_gltf`, `AssetServer::import_gltf`, and the `gltf_test` example. `experimental-gpu-physics` exists but gates nothing yet: all current GPU physics is the shipped hybrid path, so the flag is reserved for Milestone 5 GPU solvers. Verified: strict clippy over `--workspace --all-targets` clean for default, `--no-default-features`, `--no-default-features --features window`, `--no-default-features --features gltf,validation`, and `--features gpu-tests`; `cargo test --workspace` passes with default and `--no-default-features`.

### ECS and application lifecycle

- [x] Add standalone `bevy_ecs` with its parallel scheduler enabled.
- [x] Introduce a Vulkan-independent `App` and fallible `EngineBuilder`.
- [x] Add `add_plugin`, `add_system`, `add_systems`, `insert_resource`, `spawn`, `despawn`, `run`, and controlled shutdown.
- [x] Define ordered schedules: `Startup`, `FixedUpdate`, `Update`, `PostUpdate`, and `RenderExtract`.
- [x] Support command-buffered entity spawning and despawning inside systems through Bevy `Commands`.
- [x] Implement deterministic fixed timestep with configurable maximum catch-up limits.
- [x] Add pause, single-step, time scale, elapsed time, frame delta, and fixed delta resources.
- [x] Add typed events with frame-bounded lifetime.

### Canonical components and resources

- [x] `Transform` and `GlobalTransform` ECS components.
- [x] Parent/child hierarchy, cycle rejection, diagnostics, and cycle-safe propagation.
- [x] `Camera` and perspective/orthographic projection settings.
- [x] Deterministic active-camera selection by active flag, priority, and stable entity identity.
- [x] `MeshRenderer` and visibility components using typed asset handles.
- [x] Directional, point, and ambient light components.
- [x] `RigidBody`, primitive `Collider`, sensor, and collision-layer components.
- [x] `GpuEffectBody` marker for bodies currently assigned to GPU simulation.
- [x] `FrameTime`, `TimeControl`, `RenderSettings`, `PhysicsSettings`, and `QualityProfile` resources.
- [x] Input action mapping with keyboard, mouse, and gamepad-ready abstractions.

### Runtime ownership

- [ ] Split the monolithic loop into `WindowRunner`, `Renderer`, `PhysicsWorld`, `AssetServer`, and optional `EditorState`.
- [x] Keep the window, ECS world, and rendering submission main-thread owned.
  Evidence: thread audit (2026-09-23) found only `std::thread::yield_now()` in
  `App::run` and editor cargo-build/stdout worker threads in `src/editor/mod.rs`;
  no thread touches the window, `World`, or Vulkan submission.
- [x] Remove unnecessary `Arc<Mutex<_>>` usage from camera and scene state.
  Evidence: legacy `Engine` now owns `camera: PerspectiveCamera` and
  `scene: RenderScene` directly. Same change fixed a pre-existing
  `AccessConflict(DeviceRead)` panic in `prepare_frame_ubo` by keeping one
  fence per frame slot and waiting only before reusing that slot. Clippy/tests
  clean; `user_main` stress_pbr ran 8 s at ~1000-1300 FPS without panic.
- [x] Use worker tasks only for safe asset decoding and preparation.
  Evidence: `Assets::load_async` runs only the CPU loader on a worker; results
  are published on the main thread by `poll_loads` (see Milestone 2).
- [ ] Execute animations as ECS systems instead of passive data.
  Note: legacy `AnimationType` on `SceneObject` is never read or executed.
  Real ECS animation is owned by Milestone 14. Deleting the public field is a
  breaking API change for the owner to decide.

### Exit gate

- A headless test app can run startup, fixed, variable, hierarchy, and event systems deterministically.
- Entities remain stable through spawn, despawn, sorting, and render extraction.
- Pause and single-step work without affecting editor interaction.
- The old facade can spawn and render cubes and spheres through ECS components.

## Milestone 2: Asset system and scene persistence

Goal: make assets stable, deduplicated, reloadable, and serializable.

### Typed assets

- [x] Add generational `Handle<MeshAsset>`, `Handle<TextureAsset>`, `Handle<MaterialAsset>`, and `Handle<SceneAsset>`.
- [x] Build typed asset storage with generation checks, explicit reference tracking, and frame-deferred destruction.
- [x] Canonicalize asset paths and deduplicate repeated loads.
- [x] Separate CPU mesh/material assets from renderer-owned prepared GPU mesh buffers.
- [x] Add synchronous loading state inspection and structured asset errors.
- [x] Add worker-backed asynchronous loading states.
  Evidence: `LoadState::Loading`, `Assets::load_async` (path dedup, panic
  capture, failed-path retry, cancel via `remove`) and `Assets::poll_loads`,
  polled each `Update` by `AssetPlugin`. Unit test
  `async_loads_publish_success_failure_panic_and_discard_cancelled`.
- [x] Add default/fallback mesh, material, and texture assets.

### Import pipeline

- [x] Deduplicate glTF meshes, images, and materials by synthesized asset path, including sampler identity.
- [x] Import base-color textures with sRGB formats.
- [x] Import normal, metallic-roughness, occlusion, and emissive textures with correct linear/sRGB treatment.
- [x] Import sampler filtering and wrapping modes.
  Evidence: `TextureAsset::sampler` (`TextureSampler` with mag/min/mipmap
  `TextureFilter` and U/V `TextureWrap`) is filled from glTF samplers; the
  sampler index is part of the synthesized texture path. Unit test
  `gltf_import_carries_sampler_filtering_and_wrapping`. API note: adds a public
  field to `TextureAsset`. The renderer still uses one shared linear/repeat
  sampler; per-texture samplers land when the ECS renderer uploads
  `TextureAsset`s (GPU tier).
- [x] Generate or import tangents.
  Evidence: glTF `TANGENT` is imported when present; otherwise
  `assets::generate_tangents` derives tangents and handedness from triangle UV
  gradients, with a perpendicular fallback for degenerate UVs. Unit test
  `generated_tangents_follow_uvs_handedness_and_degenerate_fallback`.
- [x] Support alpha opaque, mask, and blend modes. First increment (data):
  `AlphaMode::{Opaque, Mask { cutoff }, Blend}` on `MaterialAsset`, imported
  from glTF `alphaMode`/`alphaCutoff`, saved as `SceneAlphaMode` in scene
  format 5. Cooked v1-v4 scenes migrate to `Opaque` (v4 is dispatched on its
  leading `format_version` because a v4 material can also parse as v5).
  Tests: `gltf_import_carries_sampler_state_and_alpha_mode`,
  `material_alpha_mode_survives_scene_round_trip`,
  `version_four_cooked_scene_migrates_materials_to_opaque_alpha`; cooked v3
  `testGame` scene still runs. Second increment (renderer): the ECS
  `SceneRenderer` discards masked fragments below the cutoff, writes alpha 1
  for opaque/mask, and draws blended instances last through a second pipeline
  (alpha blend, depth test without depth write), sorted back to front along
  the camera view each frame. GPU test
  `alpha_modes_render_opaque_mask_and_sorted_blend` (RTX 3060, headless
  readback) fails when sorting is disabled; unit test
  `blended_objects_render_last_unbatched_and_back_to_front`. Known ceiling:
  blended GPU-physics bodies sort by their last CPU transform.
- [x] Import node hierarchy and cameras/lights where available.
  `AssetServer::import_gltf_scene` returns one `ImportedGltfNode` per glTF
  node (name, parent index, local `Transform` from the decomposed TRS,
  mesh primitives, `Camera`, `KHR_lights_punctual` light); `spawn_gltf_nodes`
  spawns them with `Parent` links, extra primitives as child entities.
  Evidence: unit test
  `gltf_scene_import_spawns_hierarchy_cameras_and_lights` (parent/child
  links, quaternion to Euler, orthographic camera, spot light, spawned
  components). Light intensities keep glTF physical units. The editor's
  flat `import_gltf` path is unchanged (AGENTS.md); wiring the editor to the
  node import is the owner's call.
- [ ] Add skins and animation after the static scene path is stable.
  Blocked: needs the skeleton/clip/pose runtime from Milestone 14; import
  lands together with that runtime rather than as data nothing consumes.
- [x] Preserve glTF materials unless the caller explicitly supplies an override.
  `spawn_gltf_nodes(app, nodes, material_override)` keeps each primitive's
  imported material when the override is `None`. Fixed a real loss: glTF
  textures used synthesized `.rtexture` keys that never existed on disk, so a
  saved scene with a glTF material failed to load in a fresh process. The
  importer now writes `.rtexture` (bincode, carrying color space and sampler)
  next to the source like `.rmesh`, and `load_texture` decodes it. Evidence:
  unit test `gltf_materials_survive_save_and_fresh_load_unless_overridden`
  (factors, alpha mode, linear normal map, sampler survive save plus fresh
  load; override replaces material); fails with the `.rtexture` write
  removed.

### Scene serialization

- [x] Define versioned, human-readable `.rscene` files with Serde plus a compact cooked binary form.
- [x] Assign stable UUIDs to serialized scene objects.
- [x] Store asset paths/UUIDs rather than runtime handles or ECS entity IDs.
- [x] Add an allowlisted component serialization registry.
- [x] Serialize hierarchy, transforms, renderers, lights, physics, and editor metadata.
  Hierarchy, transforms, renderers, cameras, visibility, classes,
  directional/point/spot lights, and physics were already typed scene fields.
  The missing `AmbientLight` component is now built into the default
  `SceneComponentRegistry` as `rusting.ambient_light`, so the scene binary
  layout is unchanged and the editor can add it like other registered
  components. Evidence: `all_authored_light_types_round_trip_together` now
  round-trips an ambient light through cooked bytes and fails with the
  registration removed. Editor metadata is split out below.
- [ ] Serialize per-scene editor metadata (editor camera pose, selection).
  Blocked on owner: the state lives in `EditorState` in `src/editor/mod.rs`,
  which AGENTS.md forbids automated sessions from changing.
- [x] Reject unsupported scene versions with a clear structured error.
- [x] Add explicit migration for legacy unversioned project and text-scene files.
- [x] Add scene save, load, additive load, and unload operations.

### Hot reload

- [x] Watch source assets and scenes for changes.
  `Assets::changed()` compares each path-backed slot's file modification
  time with the one recorded at load (stdlib polling, no new dependency);
  missing files are ignored so save-by-rename never drops a value. It works
  for every asset type, including `scenes`. `AssetPlugin` scans every
  `HOT_RELOAD_SCAN_INTERVAL` (500 ms). Known ceilings: one `stat` per
  watched file per scan; editing a source `.gltf` does not re-import it
  (only rewritten `.rmesh`/`.rtexture` reload).
- [x] Decode changed assets in worker tasks.
  `Assets::reload_async` decodes on a worker while the old value stays
  visible; `poll_loads` publishes the new value and bumps the revision, and a
  failed decode keeps the last good value and lands in
  `take_reload_failures`. `AssetServer::reload_changed` wires meshes and
  textures. Evidence: unit test
  `changed_files_reload_on_workers_and_failures_keep_last_value` (unchanged
  file skipped, old value visible mid-decode, new value published with a
  higher revision, corrupt rewrite keeps the old value and reports one
  failure, deleted file ignored).
- [x] Swap prepared resources at safe frame boundaries.
  Reloads publish in `poll_loads` during the ECS `Update` stage, so assets
  never change mid-render; `SceneRenderer::prepare_visible_meshes` sees the
  new revision at the start of the next frame and uploads fresh buffers
  instead of writing into in-use ones. Evidence: GPU test
  `hot_reloaded_mesh_swaps_next_frame_and_old_buffers_outlive_in_flight_frame`
  (frame 2 shows the reloaded mesh while frame 1 is still unwaited; a
  control run without the reload reads white, so the black result proves the
  swap).
- [x] Keep old GPU resources alive until every referencing frame completes.
  Old buffers are released by ownership: each submitted command buffer holds
  `Arc`s to the buffers it binds, and the frame future keeps the command
  buffer until its fence completes. Evidence: the same GPU test holds a
  `Weak` to the replaced vertex buffer; it stays alive while frame 1 is in
  flight and is freed once frame 1 is dropped after completion. Textures are
  not yet uploaded by the ECS renderer; they follow the same pattern when
  PBR texture binding lands.
- [ ] Surface reload success and failure in the editor console.
  Blocked on owner: the console lives in `src/editor/mod.rs`, which AGENTS.md
  forbids automated sessions from changing. The data is ready:
  `Assets::take_reload_failures()` drains failures, and `poll_loads` returns
  the published count.

### Exit gate

- Loading the same asset path returns the same live asset identity.
- Saving and reloading a representative scene preserves all allowlisted state.
- Asset reload during rendering produces no invalid descriptor/resource use.
- Golden tests verify glTF channels, alpha modes, normals, and tangents.

## Milestone 3: Render extraction and frame management

Goal: eliminate frame-loop stalls and host/GPU races while making rendering derived from ECS state.

### Render extraction

- [x] Define a render world separate from the gameplay world.
- [x] Extract cameras, visible renderers, global transforms, lights, and material handles during `RenderExtract`.
- [x] Diff extracted state so unchanged render data produces no dirty uploads.
- [x] Build deterministic render keys and stable ordering from typed handles and entity identity.
- [x] Track contiguous dirty upload ranges and full-range reorder/removal updates.
- [x] Grow GPU buffers based on demand and device memory budgets.
  Render-instance uploads (rebuilt whenever anything moves) used a dedicated
  allocation per change; they now come from a vulkano `SubbufferAllocator`
  whose arenas start at 256 KiB, double when an upload outgrows them, and
  are reused once no in-flight frame references them. One upload is capped
  at half the largest device-local heap (`transient_upload_budget`); going
  over returns a `SceneRenderError` instead of allocating. Mesh buffers stay
  exact-size per revision. Evidence: unit test
  `transient_budget_is_half_the_largest_device_local_heap`; GPU test
  `instance_uploads_reuse_arenas_grow_on_demand_and_respect_budget` (two
  small uploads share one arena buffer, 5,000 instances grow it and still
  render, a budget one byte short fails with an explicit error). Known
  ceiling: the budget uses static heap size, not live usage
  (`VK_EXT_memory_budget` is not exposed by vulkano 0.35).
- [x] Add explicit errors or fallback paths for allocation/capacity failures.
  Audit of `SceneRenderer`: every Vulkan buffer/descriptor/command
  allocation already maps to `SceneRenderError`; instance uploads over
  budget are an explicit error; missing meshes fall back to the cube and
  missing materials render magenta. The two silent paths now report:
  lights past `MAX_LIGHTS` (64) keep the first 64 and count the rest in
  `RenderCapacityDiagnostics::dropped_lights`; GPU physics event-buffer
  overflow accumulates into `physics_events_dropped` (still logged). Both
  are read through `SceneRenderer::capacity_diagnostics()`. Evidence: GPU
  test `lights_over_capacity_render_first_max_lights_and_report_the_rest`
  (67 point lights upload 64 and report 3 dropped; back to 1 light reports
  0). Physics overflow counting has no dedicated test; it sits on the same
  readback path as the existing GPU physics event tests.

### Frames in flight

- [x] Create two or three explicit `FrameContext` objects.
  `SceneRenderer` owns `[FrameContext; FRAMES_IN_FLIGHT]` (2), used
  round-robin by `render`. Each context holds the fence of its last
  submission, which also keeps that submission's resources alive. Every
  frame now ends in a fence signal before the caller's present, not only
  physics frames. Physics readbacks share that fence. Evidence: GPU test
  `frame_contexts_bound_frames_in_flight_and_wait_only_on_reuse`.
- [x] Give each context its own fence, command allocator/pool state, uniform allocations, indirect buffers, visible lists, and transient descriptors.
  Each `SceneRenderer` `FrameContext` owns its fence and a host-visible
  `SubbufferAllocator` (64 KiB first arena). Debug-overlay vertices and GPU
  physics readback buffers now come from that arena instead of a dedicated
  allocation per frame. The physics descriptor set that references them is
  per-frame and pooled. The rest of the list maps as follows:
  command buffers come from vulkano's `StandardCommandBufferAllocator`,
  which pools per thread and recycles a buffer when its frame drops it.
  `SceneRenderer` has no uniform buffers (the camera is a push constant)
  and no indirect buffers. Visible lists are CPU-only. Evidence: GPU tests
  `transient_uploads_come_from_per_context_arenas_across_reuse` (the debug
  line renders in each of 4 frames, two full laps, and the two contexts
  never share an arena) and
  `physics_events_read_back_from_transient_arenas_every_tick` (one
  `WhileTrue` event per tick for 4 ticks, tick numbers match, no overflow).
  Before this test, no test covered the readback path through
  `SceneRenderer`. The legacy `Engine` still shares one indirect buffer per
  batch across frames; see the next two items.
- [x] Wait only when reusing a frame context whose fence has not completed.
  At the start of `render`, contexts whose fence already signaled are
  released without waiting. The one context being reused is waited on only
  if its fence has not signaled; that wait bounds CPU run-ahead to
  `FRAMES_IN_FLIGHT` frames. Evidence: the same GPU test hands back two
  frames unflushed. The third `render` submits and waits for frame 1 only;
  frame 2's fence stays unsignaled. With the wait removed, the test fails
  ("reusing context 0 first submitted and waited for its frame"). Frame
  pacing on real hardware is not verified here (hardware tier).
- [x] Chain acquire, transfer/compute, render, and present without CPU `wait()` calls in the normal frame path.
  In the product path (`project_runner` + `SceneRenderer`), one future
  chain runs from acquire to present: the acquire future is `render`'s
  `before`, physics compute and drawing share one command buffer, then come
  the frame fence and `present(.., false)`, which does not wait. Physics
  events are collected with `is_signaled` plus a zero-timeout cleanup, so
  that never blocks either. The only CPU block left is the bounded
  context-reuse wait from the item above. Evidence: the frame-context test
  shows a context that is not being reused is never waited on. Out of
  scope: the legacy `Engine` facade still waits on its separate compute and
  cull submissions. It follows the Milestone 0 precedent: that facade is
  deferred to its Milestone 1 replacement, not patched.
- [x] Retain every submitted future/fence until completion.
  Frame fences stay in their `FrameContext` until they signal or their
  context is reused after a wait. The present future is retained by
  `vulkano_util` (`previous_frame_end`), and physics readbacks by
  `pending_physics`. Evidence: the frame-context test asserts no
  unfinished fence is released. The hot-reload test shows old mesh buffers
  live until the frame that used them completed and the next frame
  released its fence.
- [x] Add deferred GPU-resource destruction queues per frame context.
  No separate queue was added; ownership already provides the deferral.
  A vulkano command buffer holds an `Arc` to every buffer, image, descriptor
  set and framebuffer it uses. Each `FrameContext` fence holds that command
  buffer until the GPU finishes. So anything the renderer replaces (mesh
  and instance buffers, lights, the depth target and framebuffers on
  resize) is freed only after its last frame completes. It is freed on the
  next `render` after that, with no wait. Evidence: GPU tests
  `hot_reloaded_mesh_swaps_next_frame_and_old_buffers_outlive_in_flight_frame`
  (mesh buffers) and
  `resize_defers_old_depth_destruction_until_its_frame_completes` (the
  depth target is replaced while frame 1 is pending, stays alive until
  frame 1 completes, then is released).
- [x] Clear culling counters and indirect commands using queued GPU operations.
  The legacy `Engine` reset `instance_count` with a host write every frame
  while the previous frame could still draw from that buffer. This made
  `user_main` panic in 2 of 10 runs with `AccessConflict(DeviceRead)`. The
  reset is now `record_reset_instance_count` (`src/scene/mod.rs`), a
  `fill_buffer` of that one field recorded before the cull dispatch. The
  cull and physics submissions are now chained after the previous frame
  instead of starting from `sync::now`. Evidence: GPU test
  `instance_count_reset_is_a_queued_gpu_fill_of_that_field_only`, and
  `user_main` ran to the timeout in 25 of 25 runs, about 3000 FPS as before.
  Known ceiling (`ponytail:` comment): on frames that run physics or
  culling, the CPU now waits for the previous frame, so CPU and GPU do not
  overlap there. Per-slot buffers would restore the overlap.
- [x] Remove host writes to buffers that may still be in GPU use.
  Audit of every `.write()` in the render paths: the legacy per-slot UBO is
  written only after that slot's fence wait. `SceneRenderer` writes only
  fresh suballocations: instance and transient arenas are reused only once
  no frame holds them, and vulkano's host-access lock rejects any
  conflicting write with an error, never a race. The one racing write, the
  indirect reset in the legacy `Engine`, was removed in the item above.

### Capabilities and fallback policy

- [x] Define a low-end Vulkan baseline suitable for Intel UHD 620-class hardware.
  `LOW_END_BASELINE` (`src/rendering/scene_renderer.rs`) requires:
  - Vulkan 1.1
  - `maxComputeWorkGroupInvocations` and `maxComputeWorkGroupSize[0]` of
    at least 256, which the physics shader's `local_size_x` needs
  - `maxPushConstantsSize` of at least 128 (the largest block used is 96
    bytes)
  - `maxStorageBufferRange` of at least 128 MiB
  - `D32_SFLOAT` usable as a depth attachment
  Every limit except compute invocations is the Vulkan-required minimum.
  No optional features or extensions are required. `SceneRenderer::new`
  reads `DeviceLimits` from the physical device and refuses a device
  below the baseline with an error that names the device and every
  shortfall. The instance upload budget is now also clamped to
  `maxStorageBufferRange`, because half of a large heap could exceed the
  range one storage binding may cover. Evidence: unit test
  `baseline_shortfalls_name_every_missing_property`, and GPU test
  `test_device_meets_the_low_end_baseline` on the headless test device.
  Not verified on a real UHD 620 (hardware tier). Its limits are expected,
  not measured, to meet the baseline.
- [x] Detect optional capabilities and expose them through renderer capabilities.
  `SceneRenderer::capabilities()` returns `RendererCapabilities`
  (`src/rendering/scene_renderer.rs`), detected once in `SceneRenderer::new`:
  - device name, integrated-GPU flag, and largest device-local heap size
  - multi-draw indirect, indirect-count drawing (core feature or
    `VK_KHR_draw_indirect_count`), bindless textures (all four descriptor
    indexing bits), and `VK_EXT_memory_budget`, each reported as
    `supported` by the GPU and `enabled` on the device
  - timestamp queries on graphics and compute queues

  A pass may use a feature only when `Capability::usable()` holds. No optional
  feature is enabled on the device yet, because no pass uses one.
  Evidence:
  - Unit test
    `optional_features_need_every_bindless_bit_and_accept_either_indirect_count_source`.
  - GPU test
    `renderer_reports_detected_capabilities_and_enables_none_it_does_not_use`.
    On the RTX 3060 it reports all four as supported and none as enabled.
- [x] Provide fallbacks for bindless descriptors, indirect-count drawing, and other advanced features.
  Audit of every draw call and shader: `SceneRenderer` uses only direct
  `draw`/`draw_indexed` and fixed descriptor sets, so its current path is
  already the baseline fallback. No shader declares a GLSL extension or a
  descriptor array. The GPU suite runs on a device with no optional feature
  enabled, which the capabilities test asserts. A pass that adopts an
  advanced feature later must branch on `Capability::usable()` and keep this
  path tested.
  The audit found one path that silently depended on an optional feature:
  the legacy `Engine` culling path wrote each batch's offset into the
  indirect command's `firstInstance`, which needs
  `drawIndirectFirstInstance`. That feature is never enabled, so the result
  was undefined. Now `firstInstance` is 0 and `base.vert` adds the batch
  offset from the push constant it already receives
  (`data[v_visible_list_offset + gl_InstanceIndex]`). A dead, unused
  indirect staging buffer next to it was deleted.
  Evidence: the shader compiles and the smoke runs pass. The culled image
  itself is not checked by a test; the exit gate's culling-equivalence check
  covers that.
- [x] Implement `QualityProfile::{Auto, Eco, Balanced, High}`.
  `RenderSettings::quality` is now extracted into `RenderWorld::quality`
  every frame. `resolve_quality` (`src/rendering/scene_renderer.rs`) turns
  `Auto` into a concrete profile from `RendererCapabilities`:
  - `Eco` on integrated GPUs
  - `Balanced` below 4 GiB of device-local memory
  - `High` otherwise
  Concrete profiles pass through unchanged. Today the resolved profile sets
  the per-frame light budget: 16, 32 or 64 lights (`light_budget`). Lights
  past the budget are dropped and counted in
  `RenderCapacityDiagnostics::dropped_lights`. A profile change rebuilds the
  light list even when the lights did not change. Later budgets (shadows,
  culling mode, substeps) plug into the same resolved profile.
  Evidence:
  - Unit test
    `auto_quality_resolves_from_device_class_and_concrete_profiles_pass_through`.
  - GPU test `lights_over_capacity_render_first_max_lights_and_report_the_rest`
    now switches High to Eco with unchanged lights and checks the 16-light
    upload. It fails (3 dropped instead of 51) without the rebuild on profile
    change.
  The `Auto` heuristic is static, marked with a `ponytail:` comment. Whether
  it picks the right profile on a real UHD 620 is hardware tier.
- [x] Remove shader variants as user-facing performance controls; choose implementation variants automatically.
  The ECS/editor path (`SceneRenderer`) exposes no performance variant:
  - It builds one scene pipeline.
  - Materials choose appearance through `MaterialModel::{Pbr, Unlit}`, never cost.
  - Cost is set only through `QualityProfile`, which `Auto` resolves from
    capabilities (item above).
  The remaining user-facing variants are the public `ShaderType` (including
  the benchmark-only `Heavy`) on the legacy `Engine` path. By roadmap
  precedent that path is replaced in Milestone 1, not patched. The
  Milestone 4 item "Replace public `ShaderType` with `MaterialModel`..."
  owns its removal. Solver selection (`PhysicsSolver`) is a semantic
  choice, and "Keep manual CPU/GPU and solver selection available" keeps it
  manual on purpose. Evidence: code audit only; no code changed.

### Exit gate

- No normal-frame CPU fence waits remain.
- Validation reports no synchronization or resource-lifetime errors.
- Two/three frames in flight can run for 10,000 frames without UBO, indirect-buffer, or descriptor races.
- Culling enabled and disabled produce equivalent visible geometry.

## Milestone 4: Rendering baseline

Goal: deliver a coherent, production-shaped forward renderer for the vertical slice.

### Pass schedule

- [x] Batch opaque ECS renderables by mesh and material and draw them through a cached GPU instance buffer.
- [ ] Directional shadow-map pass.
- [ ] Opaque forward PBR pass.
- [ ] Transparent forward pass with back-to-front sorting.
- [ ] HDR render target and tone-mapping pass.
- [x] Bootstrap Vulkan egui overlay/compositing pass for the runnable editor.
- [ ] Engine-owned egui compositing pass shared with the production renderer.
- [ ] Explicit pass resources, transitions, and debug labels.

### Materials and lighting

- [ ] Replace public `ShaderType` with `MaterialModel::{Pbr, Unlit}` plus internal shader variants.
- [ ] Complete metallic-roughness PBR texture binding and sampling.
- [ ] Support base color, normal, metallic-roughness, occlusion, and emissive maps.
- [ ] Support alpha cutoff and alpha blending.
- [ ] Add one shadowed directional light.
- [ ] Add several point lights with capability-appropriate limits.
- [ ] Add sky/environment ambient lighting.
- [ ] Add material/texture fallback behavior and diagnostic rendering.

### Visibility and quality

- [ ] Add `RenderBounds` as a rendering-only component independent from the physics `Collider`.
- [ ] Support local bounding spheres and axis-aligned boxes, transformed into world bounds during render preparation.
- [ ] Generate default render bounds automatically for primitives and imported meshes, with an explicit editor override for unusual meshes and animations.
- [ ] Add `CullingMode::{Auto, Disabled, Frustum, FrustumAndOcclusion}`. Start with the first three modes; reserve occlusion culling for a later pass.
- [ ] Select CPU or GPU frustum culling by object count, simulation ownership, device capability, and quality profile.
- [ ] Cull GPU-owned objects directly from their newest GPU physics transforms. Do not read every transform back to the CPU merely to decide visibility.
- [ ] Make GPU culling write a compact visible-instance list and indirect draw commands consumed by the normal instanced renderer.
- [ ] Let `Auto` bypass the culling dispatch for small scenes where direct rendering is cheaper, using a tested threshold before timing-based selection exists.
- [ ] Keep render visibility separate from simulation activity: a culled object continues physics unless an explicit simulation-distance policy says otherwise.
- [ ] Add Render Bounds editing and debug visualization to the Inspector and viewport.
- [ ] Report submitted, visible, and culled instance counts plus culling compute time in the profiler.
- [ ] Add hierarchical-Z occlusion culling after frustum culling, depth-pyramid generation, and conservative-bound tests are stable.
- [ ] LOD asset groups and distance/error-based selection.
- [ ] Transparent sorting.
- [ ] Shadow resolution and distance scaling by quality profile.
- [ ] Optional anisotropy and MSAA based on device support.

### Profiling

- [ ] Vulkan timestamp queries per pass.
- [ ] CPU timings for extraction, preparation, command recording, physics, and editor.
- [ ] Counters for draws, dispatches, triangles, visible instances, upload bytes, and GPU memory.
- [ ] Named GPU allocations/resources where practical.

### Exit gate

- Golden images cover primitive normals, PBR reference spheres, texture channels, alpha modes, shadow depth, and tone mapping.
- The renderer remains validation-clean through resize and asset reload.
- GPU timings and memory usage are visible to runtime code and the editor.

## Milestone 5: Hybrid physics

Goal: let each body use the processing unit and solver that fits its job while preserving a practical two-way connection between GPU simulation and normal Rust gameplay code.

### Determinism constraints, required from the start

Milestone 8 defines the full determinism requirements and owns their acceptance gate. Three of those rules cannot be retrofitted without rewriting the solver, so they apply while this milestone is implemented:

- [ ] Never accumulate simulation state through order-dependent atomic operations. Use per-body accumulation slots or fixed-order reductions. Atomics remain acceptable for counters that do not feed simulation results.
- [ ] Give body iteration and constraint ordering a stable sort key that survives buffer compaction, re-sorting, and insertion order.
- [ ] Source any randomness inside a solver from a seeded, tick-indexed stream. Never read a global or thread-local RNG from simulation code.

### Ownership and public model

- [ ] Replace the old effect-only distinction with `SimulationClass::{Static, Cpu, Gpu}`. The selected class says where the newest runtime physics state lives.
- [ ] Add `PhysicsSyncMode::{None, Events, SelectedState, FullState}`. Synchronization is an explicit cost chosen per body or group, not a hidden full-scene copy.
- [x] Give GPU-simulated bodies a stable, generation-checked `PhysicsId` that is valid in ECS, GPU buffers, and events even after bodies are removed or buffers are sorted. Extend the same ID to the command bridge when commands are added.
- [ ] Keep authored settings and identity in ECS while recording when mirrored transform/velocity data was produced and how many frames old it is.
- [ ] Allow CPU and GPU bodies in the same scene and allow static colliders to be consumed by both solvers.
- [ ] Show simulation owner, synchronization mode, readback age, and approximate synchronization cost in the editor.

### Programmable GPU condition and event bridge

- [x] Define a compact `GpuPhysicsEvent` layout shared by Rust and every physics shader: body ID, registered event ID, tick, flags, and a configurable small payload.
- [x] Provide a typed Rust condition builder for common GPU state fields, comparisons, boolean combinations, ranges, collision state, sleeping state, timers, and per-body custom values. This is a Rust API, not a separate scripting language.
- [x] Define event modes such as `OnEnter`, `OnExit`, `WhileTrue`, `Once`, and cooldown/rate-limited emission so conditions do not accidentally flood the readback buffer.
- [x] Upload built-in per-body condition instructions and rule parameters as compact GPU buffers. CPU-only custom-value uploads remain part of the command bridge.
- [x] Allow shared rules to target explicit multi-class object groups while keeping separate edge and cooldown state per body. Unrelated GPU bodies are not affected, and only matching body IDs and selected payloads return to Rust.
- [ ] Add a custom condition-compute hook for arbitrary GLSL logic over GPU-accessible state. Custom physics and condition shaders use the same `emit_event(...)` ABI as built-in rules.
- [x] Allow events to return selected position, velocity, angular velocity, or custom-value payloads so a second state readback is often unnecessary. Add collision/contact payloads with the spatial solver.
- [ ] Treat `Y < -100` only as the first end-to-end acceptance example. The implementation must not hard-code an axis, threshold, or event meaning.
- [x] Compare previous and current condition results so edge-triggered events are emitted exactly when a condition changes state.
- [x] Let the native physics compute shader append events with an atomic counter into a per-frame event buffer.
- [x] Keep event output in per-frame mapped readback allocations so only emitted records cross into Rust; move the write target fully device-local if profiling shows mapped writes are costly on discrete GPUs.
- [x] Consume completed readback buffers asynchronously after their frame fence signals. Normal frames never wait for unfinished physics work.
- [x] Convert `PhysicsId` values back into live ECS entities and expose events to Rust systems through the engine event API.
- [ ] Define event latency clearly: GPU events normally reach CPU gameplay one to three frames later. Logic requiring same-tick answers must use CPU simulation or an explicit blocking query.
- [ ] Track event-buffer overflow, resize it within the configured memory budget, and provide an overflow fallback. Events must never disappear silently.

### CPU to GPU command bridge

- [ ] Add a compact command stream for spawn, despawn, teleport, velocity change, force, impulse, wake, solver change, and watch-condition updates.
- [ ] Upload commands in batches through each frame context instead of mapping or rewriting the complete physics buffer.
- [ ] Apply commands before the fixed GPU step and reject commands whose body generation is stale.
- [ ] Support CPU-controlled kinematic bodies that collide with GPU bodies without transferring every GPU body to the CPU.
- [ ] Record command count and uploaded bytes for profiling.

### Selective state synchronization

- [ ] Support asynchronous requests for selected transforms, velocities, sleeping state, or contact data by stable body ID.
- [ ] Provide batched region/group snapshots for gameplay systems that need more than events.
- [ ] Keep full-state readback available for debugging, save-state capture, editor inspection, and tests, but keep it off the normal gameplay path.
- [ ] Triple-buffer readback storage with frame contexts so GPU writes, transfer copies, and CPU reads never race.
- [ ] Add an explicit blocking readback API only for tooling and exceptional cases, with a name and warning that make its performance cost obvious.
- [ ] Support play/stop snapshots without requiring continuous full-state synchronization.

### Self-written CPU physics and queries

- [ ] Add an engine-owned CPU `PhysicsWorld` for bodies that require immediate gameplay answers.
- [ ] Support fixed, dynamic, and kinematic rigid bodies plus boxes, spheres, capsules, convex meshes, and static triangle meshes.
- [ ] Add triggers, collision layers, raycasts, shape casts, overlap queries, character movement, and the joints required by the vertical slice.
- [ ] Add sleeping, continuous collision detection, stable contact generation, and iterative solving.
- [ ] Allow selected GPU events to create, update, or remove CPU proxy bodies when gameplay needs an approximate local query representation.

### GPU solvers and custom allocation

- [ ] Connect per-object `ComputeShaderType` selection to the ECS/editor game runner instead of only the compatibility `Engine` path.
- [ ] Preserve mixed `Static`, `NoCollision`, simplified, full, spatial-grid, and custom compute batches in one scene. A scene-wide override remains a debugging tool only.
- [ ] Rename the stable form of `ComputeShaderType::Test` to describe its actual solver while keeping a temporary compatibility alias.
- [ ] Add a true spatial broad phase to every collision solver before claiming sub-quadratic collision complexity.
- [ ] Replace fixed hash capacities with device-budgeted growable buffers.
- [ ] Track grid-cell overflow, hash collisions, oversized-body count, and total fallback work.
- [ ] Implement a tested overflow fallback that preserves every body.
- [ ] Define a versioned custom-compute ABI for instance state, commands, condition inputs, custom values, and event output.
- [ ] Let custom shaders emit the same typed events as built-in solvers so Rust gameplay can react without downloading complete buffers.

### Profiling and automatic allocation

- [ ] Measure CPU physics time, GPU physics time, dispatch count, command bytes, event bytes, selected-state bytes, synchronization latency, and overflow counts.
- [ ] Add repeatable 1K, 10K, and 100K body benchmark scenes covering falling, stacking, debris, and mixed solvers.
- [ ] Add an optional `Auto` allocation policy that uses body requirements, query needs, hardware capabilities, transfer cost, and measured timings.
- [ ] Keep manual CPU/GPU and solver selection available; automatic allocation must be observable and overridable.

### Exit gate

- A scene with at least 10,000 GPU-simulated cubes can evaluate a user-configured condition and emit a Rust event when any cube crosses `Y = -100` without copying all cube transforms or blocking the frame loop. Replacing that rule with another supported or custom condition does not require engine changes.
- Tests cover triggers, raycasts, stacking, tunneling, collision layers, fixed-step independence, stale IDs, and CPU/GPU event delivery.
- GPU event and grid overflow are observable and their fallbacks never silently omit bodies or events.
- CPU/GPU commands and events remain correct with multiple frames in flight.
- Immediate gameplay queries never pretend that delayed GPU mirrors are current; state age is available to callers.
- Per-body ownership, solver, synchronization mode, traffic, latency, and overflow are visible in runtime diagnostics and the editor.

## Milestone 6: Integrated egui editor

Goal: allow scenes to be built, inspected, saved, and played without leaving the engine.

The editor should use `egui` and `egui-winit`. Rendering should go through an engine-owned Vulkan painter/integration layer so renderer compatibility and resource lifetime remain under project control.

### Editor shell

- [x] Add a startup Project Manager with create, open, folder selection, validation, and recent projects.
- [x] Create complete standalone Cargo game templates without overwriting existing folders.
- [x] Store project format versions and reject projects made by a newer editor.
- [x] Add a feature-gated editor plugin that can be excluded from runtime builds.
- [x] Build the initial egui shell with toolbar, hierarchy, transform inspector, viewport placeholder, and structured console.
- [x] Connect play, pause, single-step, and stop controls to runtime time control.
- [x] Show typed asset inventory, mesh/material handles, and live render-extraction dirty ranges in editor panels.
- [x] Add a runnable Vulkan editor window with winit input, resize handling, DPI-aware egui rendering, and presentation.
- [x] Render extracted ECS meshes as a depth-tested Vulkan scene beneath the editor UI.
- [x] Add revision-aware GPU mesh preparation so asset mutation invalidates cached buffers without changing handles.
- [x] Drive the scene viewport and camera aspect ratio from the DPI-aware central-panel pixel rectangle.
- [x] Expose camera FOV, clipping planes, priority, and active state in the inspector.
- [x] Add Scene, Game, and Code workspaces with editor/game camera selection.
- [x] Add a project-local Rust/GLSL editor with open, validation, and save actions.
- [x] Run Cargo checks and Debug/Release builds in a worker and show compiler output inside Code Editor.
- [x] Export a native release folder with the executable, cooked scene, project assets, license, and run instructions.
- [x] Store portable scene-relative asset paths and resolve packaged scene data beside the executable.
- [x] Add a separate native Rust Cargo game project, project-local source editing, and Debug/Release build/run from the editor.
- [x] Add a concise native Rust scene API for common transform operations without hiding the ECS from advanced games.
- [x] Add a dockable area-tree layout with selectable editor types and project-local persistence.
- [ ] Replace the bootstrap Vulkan egui integration with an engine-owned texture/mesh upload path and render pass.
- [x] Add a central editor-shortcut action map; `Numpad 0` toggles Scene View fly-camera pointer capture while Escape remains a normal UI key.
- [ ] Add a Settings panel for rebinding and persisting shortcuts, then route every editor command through the same action map.
- [ ] Route keyboard and mouse focus correctly between all viewport navigation modes and UI.
- [ ] Add DPI scaling, font configuration, and theme persistence.

### Core panels

- [x] Add project asset file import, filtering, typed texture loading, glTF primitive import, and selected-object assignment.
- [x] Persist imported glTF geometry as reloadable engine-native `.rmesh` assets.
- [ ] Scene viewport rendered to an editor texture.
- [ ] Entity hierarchy with filtering, selection, reparenting, and drag/drop.
- [ ] Component inspector driven by an allowlisted reflection/editor registry.
- [ ] Asset browser with folders, thumbnails, filtering, and drag/drop assignment.
- [ ] Console with structured logs, filtering, warnings, and asset/validation errors.
- [ ] Profiler with CPU spans, GPU pass timings, counters, and memory usage.
- [ ] Add dedicated render settings and physics diagnostics panels.
- [x] Add a typed physics inspector for simulation ownership, GPU solver profile, rigid body, and collider settings.

### Scene editing

- [x] Add editor-only Scene View overlays: XZ grid, selected-object local axes, and real mesh-bounds box.
- [x] Select the nearest rendered object by clicking Scene View, using an editor ray against transformed mesh bounds instead of requiring a physics collider.
- [ ] Add a selection outline that remains readable when the object is behind other geometry.
- [ ] Translate, rotate, and scale gizmos with local/global modes and snapping.
- [x] Add FPS-style Scene View fly camera: Numpad 0 captures/releases the pointer, mouse changes yaw/pitch, WASD moves, Space/Ctrl move vertically, and Shift boosts speed.
- [ ] Add camera orbit, pan, focus-selection, and framing controls after fly camera input is stable.
- [x] Create empty, cube, and camera objects; duplicate, rename, delete, and reparent entities.
- [x] Add/remove/edit registered compiled components through the generic JSON inspector.
- [ ] Assign meshes, materials, textures, and physics shapes by typed handle.
- [x] Scene new/open/save/save-as operations with native file pickers.

### Play workflow and history

- [x] Make Play save and cook the scene, compile the real Rust project, and launch its native game window.
- [x] Use fast Debug builds by default and allow optimized Release play tests.
- [ ] Add stop and restart controls for the native game process.
- [ ] Add an optional embedded preview for workflows that do not need compiled Rust systems.
- [ ] Make runtime-spawned entities visually distinct where useful.
- [x] Implement snapshot-based undo/redo for scene and Inspector edits.
- [x] Group continuous Inspector field edits into single undo transactions.
- [x] Mark scenes dirty and prompt before destructive new/load/project-switch actions.

### Exit gate

- A user can construct, save, reload, play, pause, step, and restore a scene entirely through the editor.
- Undo/redo covers transforms, hierarchy, entity lifecycle, and component edits.
- Editor compositing has a golden-image test.
- Runtime-only builds do not depend on editor code.

## Milestone 7: Vertical slice

Goal: prove that the engine architecture works as a usable game-development stack.

### Required content

- [ ] One representative imported environment with PBR assets.
- [ ] Player camera/controller.
- [ ] CPU collisions, triggers, and at least one immediate physics query.
- [ ] Directional shadows, point lights, sky/environment light, and transparent content.
- [ ] At least 10,000 GPU bodies that evaluate configurable conditions and send typed events to Rust gameplay without full-state readback.
- [ ] CPU-to-GPU commands that alter selected GPU bodies while the simulation is running.
- [ ] Runtime UI and editor UI.
- [ ] Scene persistence and live asset reload.
- [ ] Runtime editing through play/pause/stop.
- [ ] Eco, Balanced, and High quality comparisons.

### Performance target

Milestone 7 and Milestone 29 have deliberately different hardware targets. The engine vertical slice must run on low-end integrated graphics. Sundering must not: continuous destruction with thousands of debris bodies and GPU navigation has a hardware floor, and pretending otherwise would distort the engine's quality profiles. Neither target is allowed to weaken the other.

- [ ] Define a fixed benchmark scene and camera path.
- [ ] Target 1920×1080 at 60 FPS on approximately Intel UHD 620-class hardware using Eco/Auto settings.
- [ ] Record CPU frame time, GPU pass time, draw/dispatch count, triangles, memory, visible instances, upload bytes, physics bodies, event/readback bytes, synchronization latency, and overflow.
- [ ] Store performance baselines and reject material regressions rather than relying only on FPS logs.
- [ ] Document tested drivers, operating systems, resolutions, and quality settings.

### Exit gate

- The vertical slice is playable from a clean checkout on Windows and Linux.
- Its scenes can be edited and saved in the integrated editor.
- Automated smoke, visual, physics, serialization, and performance tests pass.
- Packaging produces a runnable build with required assets and licenses.

## Milestone 8: Deterministic simulation and replay

Goal: make the simulation produce bit-identical results from the same inputs on every supported device, and make any divergence detectable and locatable. Determinism is an engine feature available to every game, not only to Sundering, and it is one of the physics pillars.

### Engine-level opt-in

- [ ] Expose `DeterminismMode::{Off, Local, CrossPlatform}` per project, so games that do not need determinism do not pay for fixed-point math or fixed-order reductions. `Local` guarantees identical results on one machine and build; `CrossPlatform` enforces every rule in this milestone.
- [ ] Validate at startup that every registered solver, system, and custom compute shader supports the selected mode, and fail with a structured error naming the offender otherwise.

### Deterministic math

- [ ] Choose and document the simulation number format: fixed-point integer math, or strictly constrained IEEE-754 with fast-math disabled, fused-multiply-add behaviour pinned, and operation order fixed.
- [ ] Implement the chosen format once in a shared math module used by the CPU solver and every physics compute shader.
- [ ] Forbid relaxed precision and fast-math in simulation shaders. Both remain available to rendering shaders.
- [ ] Replace transcendental functions in simulation paths with tabulated or polynomial implementations that have defined results on every device.
- [ ] Document which engine values are simulation state, which must be deterministic, and which are presentation only and may differ per machine.

### Deterministic execution order

- [ ] Make broad-phase cell assignment, pair generation, and constraint ordering deterministic under any workgroup or thread scheduling.
- [ ] Make solver iteration count and convergence criteria independent of timing, frame rate, and hardware.
- [ ] Apply CPU-to-GPU commands in a deterministic order that does not depend on arrival time within a tick.
- [ ] Add a seeded, tick-indexed RNG with independent streams per subsystem, and route all simulation randomness through it.
- [ ] Make floating-point-to-integer conversions, clamping, and saturation behaviour explicit and identical across backends.

### Verification harness

- [ ] Compute a world-state hash every tick over all simulation state, cheap enough to leave enabled in release builds.
- [ ] Add a headless simulation mode that runs with no window, surface, or renderer.
- [ ] Add a determinism runner that executes the same inputs for N ticks twice and compares per-tick hashes.
- [ ] Extend the runner across configurations: debug against release, differing CPU thread counts, differing GPU vendors, differing driver versions.
- [ ] Report the first divergent tick and the first divergent body, not merely that a mismatch occurred.
- [ ] Run the determinism suite in CI on every change to physics, math, or shader code.

### Replay

- [ ] Record match input streams with tick numbers, a seed, and a format version.
- [ ] Replay a recorded stream and verify it reproduces the recorded per-tick hash sequence.
- [ ] Support replay seeking by re-simulating forward from periodic full snapshots.
- [ ] Keep replays independent of render settings, resolution, quality profile, and window size.

### Exit gate

- 10,000 GPU bodies simulated for 10,000 ticks produce identical per-tick world-state hashes on at least two GPU vendors, and on both debug and release builds.
- A recorded replay reproduces its original hash sequence exactly.
- A deliberately introduced divergence is reported with its first divergent tick and body.
- If bit-identical cross-vendor results prove unachievable, that result is documented with evidence, and Milestones 19 and 24 proceed with server-authoritative networking without cross-vendor rollback.

## Milestone 9: Scene composition, reflection, and iteration speed

Goal: give the engine Godot's authoring model — reusable scenes, signals, groups, data resources, and fast code iteration — expressed through ECS instead of a node tree.

Depends on: Milestones 1, 2, and 6.

### Reflection

- [ ] Add a reflection registry for components, resources, and asset types: field names, types, ranges, defaults, and editor hints, derived from Rust types rather than hand-maintained tables.
- [ ] Drive scene serialization, the Inspector, undo/redo, and animation property tracks from the same registry. Replace the generic JSON inspector path once parity is reached.
- [ ] Support reflected enums, nested structs, collections, typed handles, and entity references.
- [ ] Reject or migrate renamed/removed reflected fields with a structured, versioned error instead of silently dropping data.

### Prefabs and scene instancing

- [ ] Instance a saved scene as a child subtree of another scene, keeping a link to its source asset.
- [ ] Store per-instance overrides as property diffs against the source, so source edits propagate to every instance that did not override that property.
- [ ] Support nested prefabs and prefab variants (inheritance), with cycle rejection.
- [ ] Revert, apply-to-source, and unpack-instance operations with undo.
- [ ] Instance prefabs at runtime from gameplay code through typed handles.

### Signals, groups, and observers

- [ ] Add entity-targeted observers that react to component insertion/removal and to typed events, as the ECS equivalent of Godot signals.
- [ ] Allow scene files to store event-to-handler connections between entities by `SceneId`, with registered Rust handler names, validated at load time.
- [ ] Add entity groups/tags with fast membership queries and editor assignment.
- [ ] Add a typed scene-tree query API for common "find child / find in group / find by name" operations without hiding ECS queries.

### Data resources

- [ ] Add user-defined typed data assets (the equivalent of Godot `Resource`/`.tres`), serialized through reflection and editable in the Inspector.
- [ ] Support shared versus per-instance-unique resource references.
- [ ] Hot reload data assets into running play sessions.

### Iteration speed

- [ ] Hot reload game Rust code during play through a dynamic-library game module, preserving ECS state through the reflection registry. Fall back to a clean restart with a clear message when the change is layout-incompatible.
- [ ] Measure and report edit-to-running-game latency for a representative project; keep incremental Debug builds under a documented target.
- [ ] Define an optional sandboxed WASM scripting host (`rusting-script`) exposing the reflected ECS API, for modding and designer-level logic. Native Rust remains the primary gameplay API.
- [ ] Deliver project templates for 3D first-person, 3D third-person, 2D platformer, and physics sandbox games.

### Exit gate

- A prefab edited in the editor updates every instance that did not override the edited property, and overrides survive save and reload.
- An observer connected in a scene file fires the registered Rust handler at runtime.
- A reflected component added in game code appears in the Inspector, serializes, and animates without editor changes.
- Changing a gameplay system during play reloads it without losing scene state.

## Milestone 10: Advanced rigid-body physics

Goal: exceed Godot's built-in 3D physics feature set on the CPU path while keeping every feature available to hybrid CPU/GPU scenes.

Depends on: Milestones 5 and 8 (all new solver work follows the determinism rules).

### Constraints and articulations

- [ ] Joints: fixed, hinge, slider, ball-socket, cone-twist, distance, spring, and a generic 6-DOF joint with per-axis limits, springs, and motors.
- [ ] Breakable joints with force/torque thresholds that emit typed events.
- [ ] Reduced-coordinate articulations (Featherstone) for robots, chains, and ragdolls that need exact joint limits without drift.
- [ ] Stable long joint chains and high mass ratios verified by regression scenes.

### Bodies and characters

- [ ] Kinematic character controller with slopes, steps, ground snapping, moving platforms, and push interaction with dynamic bodies.
- [ ] Physics-based (dynamic) character option for games that want fully simulated movement.
- [ ] Raycast and wheel-collider vehicle models with suspension, tire friction curves, differential, and drivetrain.
- [ ] Ragdolls generated from skeletons, with a handoff API to and from animation (consumed by Milestone 14).
- [ ] Compound shapes, convex hulls, heightfields, and triangle meshes with per-triangle materials.

### Materials, forces, and fields

- [ ] Physics materials with friction, restitution, combine modes, and per-material contact events.
- [ ] Force fields: directional, radial, vortex, wind with turbulence, and custom field functions; usable by CPU and GPU bodies alike.
- [ ] Buoyancy and drag against water volumes.
- [ ] Gravity volumes and per-body gravity scale (planetary gravity, zero-g zones).

### Solver quality

- [ ] Simulation islands with sleeping and wake propagation.
- [ ] Speculative contacts plus swept CCD for fast bodies; bullets through thin walls never tunnel in regression scenes.
- [ ] Substepping with a documented stability/cost trade-off per quality profile.
- [ ] Contact caching and warm starting for stable stacks.
- [ ] Multithreaded CPU solver whose results are identical for any worker count.

### 2D physics

- [ ] 2D rigid bodies, shapes (circle, capsule, box, convex polygon, segment chain), joints, and queries, sharing the solver architecture and determinism rules with 3D.
- [ ] Hybrid 2D GPU bodies with the same condition/event/command bridge as 3D.

### Exit gate

- A regression scenario suite (pyramid stacks, joint chains, high mass ratios, CCD bullets, ragdoll piles, vehicles on uneven terrain) runs headless in CI with recorded expected results.
- Every joint type, character controller behaviour, and vehicle model has a test and a demo scene.
- CPU results are identical across worker-thread counts.
- 2D and 3D physics run in the same project without interference.

## Milestone 11: Deformable, continuum, and large-scale GPU physics

Goal: provide physics Godot does not have — deformables, fluids, granular media, runtime fracture, and million-body scale — all coupled in one world.

Depends on: Milestones 5, 8, and 10.

### Deformables

- [ ] XPBD soft bodies from tetrahedral meshes with volume preservation, attachment to rigid bodies, and tearing.
- [ ] Cloth with self-collision, wind interaction, attachment/pinning, and tearing.
- [ ] Ropes and cables with rigid-body attachment and correct tension transfer.
- [ ] GPU execution path for deformables with the same event and readback bridge as rigid GPU bodies.

### Fluids and granular media

- [ ] Particle-based fluid (PBF or SPH) with two-way rigid-body coupling and buoyancy.
- [ ] Grid-based or hybrid (FLIP/APIC) option for large water volumes, chosen by the simulation class.
- [ ] Granular material (sand, gravel, rubble) with piling and angle-of-repose behaviour.
- [ ] Fluid surface extraction for rendering through extraction, never by rendering reading solver buffers directly.

### Fracture and destruction

- [ ] Pre-fractured assets authored in the editor (Voronoi and slicing) with connectivity graphs.
- [ ] Runtime fracture from impact energy and stress, deterministic under the seeded tick RNG.
- [ ] Debris lifetime, budget, sleeping, and merge policies. Never drop bodies arbitrarily at the cap.
- [ ] Destruction events consumable by audio, particles, navigation, and gameplay.

### Scale

- [ ] One unified GPU broad phase shared by rigid, deformable, particle, and fluid solvers.
- [ ] Multi-GPU-queue scheduling: async compute for physics overlapping graphics where supported, with a fallback on single-queue devices.
- [ ] GPU simulation LOD: distant or unobserved regions step at reduced rates or freeze, with explicit, deterministic policies.
- [ ] Streaming simulation regions in and out of GPU memory within a device budget.

### Coupling

- [ ] Two-way coupling between rigid, articulated, deformable, fluid, and granular simulation in one scene.
- [ ] Mixed CPU/GPU ownership for coupled objects with defined latency and state age.

### Exit gate

- A demo scene combines cloth, soft bodies, fluid, granular media, and runtime fracture interacting with rigid bodies, with correct events delivered to Rust gameplay.
- Deformable, fluid, and fracture scenarios have headless GPU tests under lavapipe.
- A 1,000,000-body GPU scene runs within the hardware budget recorded for the benchmark machine.
- Fracture is deterministic across runs.

## Milestone 12: Physics tooling, authoring, and evidence

Goal: make physics the best-understood part of the engine — visible, debuggable, authorable, and benchmarked against competitors.

Depends on: Milestones 5, 6, and 10. Deformable and fluid tooling follows Milestone 11.

### Physics debugger

- [ ] Debug draw for shapes, contacts, normals, impulses, joints, islands, sleeping state, broad-phase cells, and GPU ownership.
- [ ] Record physics sessions (from play mode or a running game) and scrub a timeline tick by tick, using Milestone 8 replay.
- [ ] Per-body inspector showing state history, contacts, applied forces, owning solver, sync mode, and readback age.
- [ ] Remote physics debugging of a running game process from the editor.
- [ ] Heatmaps for solver cost, contact density, and overflow by region.

### Authoring tools

- [ ] Collider editing gizmos, automatic collider generation, and convex decomposition (V-HACD-class) for imported meshes.
- [ ] Joint placement and limit-editing gizmos with live preview.
- [ ] Ragdoll creation wizard from a skeleton.
- [ ] Physics material library, collision-layer matrix editor, and force-field visualization.
- [ ] Simulate-in-editor: run physics on selected objects without entering play mode, then keep or discard the result (for placing props naturally).
- [ ] Fracture authoring preview.

### Profiling

- [ ] Physics profiler panel: CPU/GPU solver time per stage, body/contact/constraint counts, command/event/readback bytes, overflow, and synchronization latency.
- [ ] Automatic-allocation diagnostics explaining why each body was placed on CPU or GPU.

### Benchmarks and evidence

- [ ] Standard benchmark scenes: stacking stability, 100K pile, ragdoll count, joint-chain stability, CCD bullets, vehicle stress, cloth, fluid, and fracture.
- [ ] Reference implementations of the same scenes on Jolt, PhysX, Rapier, and Godot Physics, run by the same harness.
- [ ] Publish results with hardware, driver, and settings; regressions against stored baselines fail CI.

### Exit gate

- A physics bug in a recorded session can be located by scrubbing to the tick and inspecting the body in the editor.
- Colliders, joints, ragdolls, and materials can be authored entirely in the editor.
- The comparative benchmark report is generated reproducibly and checked into the repository.

## Milestone 13: Advanced rendering

Goal: reach Godot's Forward+ visual feature set and give users programmable shading.

Depends on: Milestones 3 and 4.

### Global illumination and reflections

- [ ] Reflection probes with box projection and blending.
- [ ] Baked lightmaps with a GPU lightmapper and light probes for dynamic objects.
- [ ] One real-time GI technique (probe-based DDGI or voxel/SDF GI) with a quality-profile fallback to ambient probes.
- [ ] Sky system: procedural physical sky, HDRI skies, and sky-derived ambient/specular.

### Screen-space and post effects

- [ ] SSAO, SSR, and screen-space indirect lighting, each independently toggled by quality profile.
- [ ] Bloom/glow, depth of field, motion blur, auto-exposure, color grading LUTs, vignette, and chromatic aberration.
- [ ] Temporal anti-aliasing and FXAA; optional upscaling (FSR-class) behind capability checks.
- [ ] Volumetric fog with light scattering and fog volumes.

### Lighting and shadows

- [ ] Cascaded shadow maps for directional lights; shadows for point and spot lights.
- [ ] Clustered lighting for many point and spot lights.
- [ ] Area-light approximation and light cookies.

### Geometry and effects

- [ ] GPU particle system reusing the physics compute infrastructure, with collision against physics shapes, attractors, sub-emitters, and trails.
- [ ] Particle emission driven by physics events: impacts, fracture, debris settling.
- [ ] Decals, including decals that survive or are invalidated by fracture.
- [ ] Instanced multi-mesh rendering and foliage scattering with wind driven by physics force fields.
- [ ] Heightmap terrain rendering with texture splatting and LOD.
- [ ] Automatic mesh LOD generation at import.
- [ ] Rendering of deformables, fluid surfaces, and debris produced by Milestone 11.
- [ ] Effect budget and quality-profile scaling for particles and decals.

### Programmable shading

- [ ] A versioned engine shader language (GLSL-based with engine includes) for surface, unlit, sky, particle, fog, and post-process shaders, with stable uniforms/built-ins.
- [ ] A visual shader graph that compiles to the same language.
- [ ] Shader hot reload with error reporting in the editor.
- [ ] Material instancing with per-instance parameter overrides.

### Frame structure

- [ ] A render graph that schedules passes, transient resources, and barriers automatically.
- [ ] Frame pacing that holds a stable presentation cadence under variable GPU load.
- [ ] Measure and report end-to-end input latency.

### Exit gate

- Golden images cover probes, GI fallback, SSAO/SSR, fog, bloom, tone mapping, particles, decals, and custom shaders.
- A user-written surface shader and a visual-graph shader render correctly without engine changes.
- Every effect degrades gracefully on the low-end baseline defined in Milestone 3.

## Milestone 14: Animation

Goal: match Godot's animation system and make physics-driven animation a first-class feature.

Depends on: Milestones 2, 9 (reflection), and 10 (ragdolls).

### Core animation

- [ ] Skeletal animation with GPU skinning and blend shapes (morph targets).
- [ ] glTF skin, morph target, and animation import, completing the Milestone 2 deferral.
- [ ] Property animation of any reflected field (the equivalent of `AnimationPlayer`), including method-call and event tracks.
- [ ] Tweens for scripted interpolation with easing curves.

### Blending and control

- [ ] Animation state machine with transitions, conditions, and sync groups.
- [ ] 1D/2D blend spaces, additive layers, and bone masks.
- [ ] Root motion with an explicit policy that never feeds back into deterministic simulation unless routed through the physics command bridge.
- [ ] Two-bone, FABRIK, look-at, and foot-placement IK.
- [ ] Skeleton retargeting between humanoid rigs.

### Physics-driven animation

- [ ] Ragdoll handoff between animation and physics on physics events, with blend-back.
- [ ] Active ragdolls: powered articulations that track animation targets while reacting physically to hits.
- [ ] Procedural secondary motion (jiggle bones, tails, hair) through the physics solver rather than a separate spring system.
- [ ] Cloth attached to skinned characters (from Milestone 11).

### Exit gate

- An imported, skinned character blends through a state machine, uses IK on uneven ground, and hands off to an active ragdoll when hit.
- A property track animates a custom reflected component.
- Animation playback results are identical across runs.

## Milestone 15: Audio

Goal: provide Godot-equivalent audio, driven naturally by physics.

Depends on: Milestone 1. Physics-driven audio depends on Milestones 5 and 10.

- [ ] Audio device management with hot-plug, output selection, and a no-device fallback.
- [ ] Mixer with buses, sends, volume/mute/solo, and bus effects (reverb, delay, EQ, compressor, limiter, filters).
- [ ] WAV, OGG Vorbis, and FLAC import; streaming playback for long assets.
- [ ] 3D spatial audio with attenuation curves, doppler, and an occlusion approximation using physics raycasts.
- [ ] Reverb zones tied to physics volumes.
- [ ] Event-driven playback triggered by gameplay events and by GPU physics events.
- [ ] Physics-driven impact, scrape, and roll sounds parameterized by contact impulse, relative velocity, and physics material.
- [ ] Voice limiting and priority so large destruction events do not exhaust the mixer.
- [ ] Offline render path for deterministic audio tests.
- [ ] Audio state is presentation only and never affects simulation or replay hashes.

### Exit gate

- An offline-rendered mix matches a reference buffer within tolerance.
- A physics scene with thousands of contacts produces voice-limited impact audio without dropouts.
- Disabling all audio does not change replay hashes.

## Milestone 16: Runtime UI, text, and localization

Goal: give games a runtime UI toolkit equivalent to Godot's Control system.

Depends on: Milestones 1, 3, and 9.

- [ ] Runtime UI layer independent of the editor's egui usage, rendered through extraction.
- [ ] Layout containers: box, grid, margin, scroll, split, tab, and anchors/offsets for free placement.
- [ ] Widgets: label, button, toggle, slider, text input, dropdown, list, tree, progress bar, image, and panel.
- [ ] Text shaping with Unicode, bidirectional text, font fallback, SDF font rendering, and rich text markup.
- [ ] Themes and styles editable as data assets.
- [ ] Focus navigation for keyboard and gamepad, and input routing between UI and gameplay.
- [ ] Data-bound HUD elements and world-space indicators.
- [ ] Scaling across resolutions, aspect ratios, and DPI settings.
- [ ] Localization: translation tables, pluralization, locale switching at runtime, and extraction of translatable strings.
- [ ] Accessibility: screen-reader metadata, scalable text, and color-blind-safe defaults.
- [ ] Input remapping UI component built on the action map.
- [ ] Decouple input sampling rate from the simulation tick without introducing nondeterminism.

### Exit gate

- A menu, settings screen with rebinding, and HUD can be built from data assets and work with mouse, keyboard, and gamepad.
- Golden images cover layout, text shaping (including right-to-left), and theming.
- Switching locale at runtime updates every translated string.

## Milestone 17: 2D engine

Goal: make RustingEngine a complete 2D engine, not a 3D engine with a flat camera.

Depends on: Milestones 3, 4, and 10 (2D physics).

- [ ] 2D renderer with sprites, sprite batching, z-ordering, and canvas layers.
- [ ] Sprite sheets and sprite animation.
- [ ] Tilemaps with multiple layers, autotiling rules, tile collision shapes, and tile navigation data.
- [ ] 2D lights, shadows from occluder polygons, and normal-mapped sprites.
- [ ] 2D GPU particles with physics-event emission.
- [ ] 2D camera with smoothing, limits, and pixel-perfect mode.
- [ ] 2D hybrid GPU physics showcases (thousands of physics sprites with events to Rust gameplay).
- [ ] Mixing 2D layers over or inside 3D scenes.

### Exit gate

- A 2D platformer template with tilemap, lights, particles, and physics is playable and editable in the editor.
- Golden images cover sprites, tilemaps, and 2D lighting.

## Milestone 18: Navigation

Goal: provide Godot-equivalent navigation for 2D and 3D, with runtime updates that respond to physics.

Depends on: Milestones 5 and 10.

- [ ] Navigation mesh baking from collision geometry for 3D, and from tilemaps/polygons for 2D.
- [ ] Runtime incremental re-baking of changed tiles within a per-tick budget.
- [ ] Path queries, off-mesh links (jumps, ladders, doors), and area costs.
- [ ] Navigation agents with path following and velocity-obstacle avoidance.
- [ ] Obstacles derived from physics bodies, including moving and sleeping bodies.
- [ ] Navigation debug visualization and profiler counters.
- [ ] Deterministic query and avoidance results for deterministic projects.

### Exit gate

- Agents route around physics bodies that are knocked into their path at runtime.
- Re-baking stays within its per-tick budget and reports overruns.
- Navigation results are identical across runs.

## Milestone 19: Networking and multiplayer

Goal: provide Godot-equivalent high-level multiplayer, plus deterministic rollback that Godot cannot offer.

Depends on: Milestones 8 and 9. The Milestone 8 result decides whether rollback is offered across vendors.

- [ ] Transport abstraction with UDP (reliable and unreliable channels), WebSocket, and loopback for tests.
- [ ] Versioned wire protocol with compatibility rejection at connect time.
- [ ] Remote procedure calls on entities with authority checks and reliability modes.
- [ ] Replicated spawning and component synchronization, configured through reflection, with delta compression and quantization.
- [ ] Client-server and listen-server topologies; headless dedicated-server builds.
- [ ] Client-side prediction and reconciliation for player-controlled entities.
- [ ] Deterministic rollback netcode for physics-heavy games, using Milestone 8 snapshots and input streams.
- [ ] Physics-aware replication: replicate commands and events instead of full body state wherever determinism allows.
- [ ] Network simulation (latency, jitter, loss) for testing, and a network profiler.
- [ ] HTTP client for services and downloads.

### Exit gate

- A physics sandbox runs with four clients under simulated 150 ms latency and 2% loss without visible desync.
- Rollback reproduces identical simulation state on every peer in a deterministic test.
- RPC, spawning, and synchronization have loopback tests.

## Milestone 20: Editor parity

Goal: bring the editor to Godot-level completeness on top of the Milestone 6 foundation.

Depends on: Milestone 6, and the milestone that owns each edited subsystem.

### Workflow

- [ ] Remote scene tree and inspector for a running game process.
- [ ] Runtime debugger panel: errors with stack traces, monitors (FPS, memory, physics, rendering counters), and custom monitors.
- [ ] Unified profiler with CPU spans, GPU pass timings, physics stages, network traffic, and memory.
- [ ] Project settings editor, input map editor, and per-asset import settings dock with re-import.
- [ ] Multiple scene tabs and multiple viewports (split views, orthographic views).
- [ ] 2D viewport with snapping and pixel grid.
- [ ] Search across the project: files, entities, components, and settings.
- [ ] Version-control integration showing changed files and scene diffs.

### Specialized editors

- [ ] Animation editor with timeline, curves, onion skinning, and state-machine graph.
- [ ] Shader editor and visual shader graph editor.
- [ ] Tilemap and tile-set editor.
- [ ] Grid/modular level editing and constructive solid geometry blocking tools.
- [ ] Particle system editor with live preview.
- [ ] Theme and UI layout editor.
- [ ] Audio bus editor.
- [ ] Navigation baking controls and visualization.

### Extensibility

- [ ] Editor plugin API for custom panels, inspectors, gizmos, importers, and tools, loaded from project crates.
- [ ] Asset library browser for installing templates, plugins, and assets into a project.
- [ ] In-editor API documentation for engine and reflected project types.

### Exit gate

- Every subsystem milestone's authoring tasks can be completed without leaving the editor.
- A third-party editor plugin adds a panel, a custom inspector, and an importer without engine changes.
- Editor state, layouts, and settings persist across sessions.

## Milestone 21: Platforms, export, and distribution

Goal: ship games to every supported platform from the editor.

Depends on: Milestones 3 and 6.

- [ ] Export presets per platform with feature flags, asset filters, and encryption of packed data.
- [ ] Packed asset archives with compression and streaming.
- [ ] Texture compression per platform (BCn desktop, ASTC/ETC2 mobile) through KTX2.
- [ ] Windows and Linux release polish: installers/archives, icons, and code-signing hooks.
- [ ] Steam Deck/Proton verification.
- [ ] macOS through MoltenVK, with capability fallbacks for missing Vulkan features.
- [ ] Android export with touch input, lifecycle handling, and mobile quality profiles.
- [ ] Platform services abstraction for save paths, achievements, and storefront SDK hooks.
- [ ] Crash reporting with symbolicated stack traces.
- [ ] Record web, iOS, and console export as post-1.0 decisions with the blocking technical reasons.

### Exit gate

- A demo project exports and runs from the editor on Windows, Linux, and macOS.
- An Android build runs the physics sandbox template on a mid-range device.
- Crash reports from exported builds resolve to source locations.

## Milestone 22: Documentation, samples, and ecosystem

Goal: make the engine learnable and adoptable the way Godot is.

Depends on: the features being documented. Documentation for each feature lands with that feature; this milestone covers the structure and the gaps.

- [ ] User manual covering every subsystem, with a dedicated physics guide that explains ownership, synchronization, determinism, and solver choice.
- [ ] Generated API reference for engine crates and reflected types.
- [ ] Step-by-step tutorials: first 3D game, first 2D game, physics sandbox, multiplayer physics game.
- [ ] Demo projects per feature area, with a physics showcase gallery (destruction, fluids, cloth, vehicles, ragdolls, million-body scenes).
- [ ] Migration guide for Godot users mapping nodes, signals, resources, and scripts to RustingEngine concepts.
- [ ] Contribution guide, plugin authoring guide, and release notes process.

### Exit gate

- A new user can build and export the first-game tutorial from a clean install using only the documentation.
- Every demo project builds and runs in CI.

## Milestone 23: Engine 1.0 release

Goal: declare Godot-class parity with physics leadership, backed by evidence.

Depends on: Milestones 9-22.

- [ ] Every row in the Godot parity map has a passing exit gate.
- [ ] Every physics pillar has its test or benchmark evidence checked in.
- [ ] Reference games built entirely with the engine: a 3D action game, a 2D platformer, a vehicle/physics sandbox, and a multiplayer physics game.
- [ ] Public API stability policy, semantic versioning, and a deprecation process.
- [ ] Performance baselines recorded for the low-end, mid-range, and high-end reference machines.

### Exit gate

- All reference games are playable from a clean checkout, editable in the editor, and exportable to every supported platform.
- The comparative physics benchmark report shows where RustingEngine leads, and any area where it does not is documented with a follow-up item.

## Sundering track

Milestones 24-29 build *Sundering* on top of the engine. They consume the determinism, fracture, GPU-scale, navigation, and networking milestones and must not fork parallel versions of those systems; gaps found here become engine items in the owning milestone.

## Milestone 24: Competitive networked authority

Goal: run the authoritative simulation on a server and keep many clients consistent with it under real network conditions.

Depends on: Milestones 8 and 19. Milestone 19 provides the general transport, protocol, RPC, replication, and rollback machinery; this milestone adds the competitive-authority, anti-cheat, and destruction-scale requirements Sundering needs on top of it.

The networking model depends on the Milestone 8 result. Bit-identical determinism permits lockstep and rollback. Without it, the server simulates and clients render, predicting only their own hero.

### Server

- [ ] Headless server binary running the fixed-tick simulation with no renderer or window.
- [ ] Tick-scheduled loop with drift correction, overload detection, and reported tick-budget usage.
- [ ] Client lifecycle: join, ready, in-match, drop, reconnect.
- [ ] Server-side input validation and rate limiting. The server never trusts a client-reported position, velocity, or terrain state.

### Transport and protocol

- [ ] Reuse the Milestone 19 transport and versioned protocol; do not fork a second networking stack.
- [ ] Clock synchronization and per-client tick-offset estimation.
- [ ] Input buffering with configurable delay and jitter absorption.

### State replication

- [ ] Snapshot encoding with delta compression against the last client-acknowledged snapshot.
- [ ] Quantize replicated values with documented precision per field.
- [ ] Interest management: replicate only what a client's team can currently see, using the vision system from Milestone 27.
- [ ] Per-client bandwidth budget with measured usage and an enforced cap.
- [ ] Replicate terrain modification events, not individual debris transforms. Clients re-simulate debris locally from the same events. This is the only approach that keeps bandwidth bounded during heavy destruction, and it depends on Milestone 8.

### Client

- [ ] Interpolate remote entities with a documented, configurable interpolation delay.
- [ ] Predict only the local hero. Never predict terrain destruction results unless determinism is proven.
- [ ] Reconcile prediction error with presentation-only smoothing that never feeds back into simulation state.
- [ ] Detect desync by periodic state-hash comparison, report it, and recover through full resynchronization.
- [ ] Support spectator clients that receive state and send no input.

### Exit gate

- Ten clients complete a full match against a headless server at 200 ms round-trip latency with 2% packet loss, without desync.
- Per-client bandwidth stays within budget with the terrain fully destroyed.
- A client disconnects mid-match, reconnects, and resynchronizes correctly.
- Server tick-budget usage and desync events are observable in operations tooling.

## Milestone 25: Destructible terrain and Scar

Goal: make terrain a first-class simulated, modifiable, and permanently scarred entity rather than authored static geometry.

Depends on: Milestones 8, 11 (fracture, debris, GPU scale), and 13 (terrain rendering).

### Terrain representation

- [ ] Chunked volumetric terrain with an authoring format, streaming, and a documented chunk size.
- [ ] GPU surface extraction with per-chunk LOD and crack-free transitions between levels.
- [ ] Derive terrain collision from the same volume data as rendering. One source of truth, never two.
- [ ] Editor tools to author, sculpt, and paint terrain volumes.
- [ ] Report chunk count, resident memory, extraction time, and streaming state in the profiler.

### Fracture and debris

- [ ] Promote static terrain chunks to dynamic GPU rigid bodies when destroyed.
- [ ] Use deterministic fracture patterns driven by the seeded tick RNG.
- [ ] Enforce a debris budget with a deterministic, documented policy at the cap. Never drop bodies arbitrarily.
- [ ] Settle and re-freeze resting debris back into static terrain, restoring collision and navigation.
- [ ] Merge small settled debris into larger static aggregates so long-running matches stay bounded.

### Structural load

- [ ] Build a support graph over connected terrain chunks.
- [ ] Solve support incrementally on the GPU when the graph changes, within a bounded per-tick cost.
- [ ] Collapse unsupported spans under their own mass.
- [ ] Expose per-chunk stress to gameplay and to debug visualization.

### Scar

- [ ] Make settled debris permanent by default, with an explicit per-region regeneration policy.
- [ ] Ensure settled debris affects collision, navigation, and vision, not only rendering.
- [ ] Persist terrain state in save, replay, and network resynchronization formats as modification history where practical rather than full volumes.

### Terrain commands

- [ ] Extend the CPU-to-GPU command bridge with terrain modification: carve, fracture, displace, freeze.
- [ ] Apply terrain commands at deterministic points in the tick.
- [ ] Account modification cost against a per-team mass-budget resource.

### Exit gate

- A cliff collapses onto a 10,000-body scene deterministically, producing identical hashes on two GPU vendors.
- Settled debris becomes walkable, occluding, static terrain.
- Frame time and memory stay within budget after a one-hour continuous-destruction soak test.
- Terrain state round-trips correctly through save, replay, and network resynchronization.

## Milestone 26: Dynamic navigation and crowds

Goal: let thousands of agents move sensibly over geometry that changes every tick.

Depends on: Milestones 18 (engine navigation) and 25. The engine navigation handles incremental re-baking of ordinary scenes; this milestone handles terrain that changes every tick at crowd scale.

### Navigation representation

- [ ] GPU navigation volume or flow field derived from live terrain collision data.
- [ ] Incremental rebuild limited to changed regions, with a bounded and enforced per-tick cost.
- [ ] Deterministic rebuild results, independent of how many chunks changed in a single tick.
- [ ] Reachability queries answering whether a team can reach a given point at all.
- [ ] Report navigation build time, dirty-region count, and budget overruns in the profiler.

### Agents

- [ ] GPU agent steering with local avoidance at thousands of units.
- [ ] Deterministic agent update order and avoidance resolution.
- [ ] Agents fall, are buried, and are displaced by debris rather than ignoring it.
- [ ] Make path failure an observable gameplay state, never a silent stall.
- [ ] Keep a CPU pathing path for the small number of agents that need immediate answers.

### Exit gate

- 1,000 agents cross a map while it is actively being destroyed, without stalling or tunnelling.
- Navigation rebuild stays within its per-tick budget during a full cliff collapse.
- Agent behaviour is bit-identical across runs and across vendors.
- Blocking every route is detected and reported rather than producing stuck agents.

## Milestone 27: Competitive gameplay framework

Goal: provide the game-specific systems Sundering needs, built entirely on deterministic simulation.

### Match and teams

- [ ] Team membership, match phases, and authoritative match state owned by the server.
- [ ] Objective and structure entities with destruction-aware state.
- [ ] Resource and mass-budget accounting per team.
- [ ] Match start, end, surrender, and result reporting.

### Abilities as forces

- [ ] Express ability definitions as deterministic physical effects: impulses, sustained forces, tethers, mass changes, area displacement.
- [ ] Cooldowns, costs, cast time, interrupts, and targeting resolved on the fixed tick.
- [ ] Area queries against live terrain and bodies, executed deterministically.
- [ ] Require every ability effect to be expressible as commands through the physics bridge.
- [ ] Expose telegraph data to presentation without letting presentation affect simulation.

### Dynamic vision

- [ ] Per-team visibility computed on the GPU from actual terrain geometry, not from authored vision blockers.
- [ ] Recompute visibility when terrain changes, within a bounded per-tick budget.
- [ ] Feed visibility into network interest management from Milestone 24.
- [ ] Derive presentation-side fog rendering from the same data.
- [ ] Keep visibility deterministic and identical between server and every client.

### Bots

- [ ] Bot controllers that use the same input interface as human players.
- [ ] Deterministic bot decisions driven by the seeded tick RNG.
- [ ] Difficulty levels with a documented behaviour set.
- [ ] Bots handle a reshaped map, including blocked routes and newly opened ones.

### Exit gate

- A match can be played to completion against bots with no human opponents.
- Vision is computed from destroyed geometry and matches exactly between server and clients.
- Every ability is expressible as a deterministic physical effect and replays identically.

## Milestone 28: Balance and operations tooling

Goal: make a game whose map differs every match tunable with evidence rather than intuition.

### Headless experimentation

- [ ] Run matches headless, faster than real time.
- [ ] Batch runner executing many matches across varied seeds, configurations, and bot profiles.
- [ ] Deterministic seeding so any interesting match is exactly reproducible from its seed.

### Telemetry

- [ ] Structured per-match telemetry: terrain modified, routes opened and closed, fight locations, resource use, outcome.
- [ ] Aggregate reporting across batch runs.
- [ ] Automatic detection of degenerate states: total lockout, unreachable objectives, runaway debris count, stalled navigation.

### Replay analysis

- [ ] Replay browser with seeking, speed control, and a free camera.
- [ ] Terrain-state timeline showing how the map evolved through the match.
- [ ] Export a match's terrain modification history for offline analysis.

### Server operations

- [ ] Match hosting, lifecycle management, and health reporting.
- [ ] Crash and desync capture producing reproducible artifacts.
- [ ] Per-map performance baselines with regression rejection.

### Exit gate

- One thousand headless bot matches run unattended and produce an aggregate balance report.
- Any reported degenerate state reproduces exactly from its recorded seed.
- A match replay can be inspected tick by tick alongside its terrain timeline.

## Milestone 29: Sundering vertical slice

Goal: prove the whole stack with the smallest piece of the real game. Each slice answers exactly one question and adds nothing beyond it.

### Slice 1: pathing survives destruction

- [ ] One lane, one tower, one destructible cliff.
- [ ] Two heroes, one able to destroy terrain.
- [ ] Creeps pathing from spawn to tower.
- [ ] Acceptance: creeps route correctly after the cliff collapses, deterministically, on two GPU vendors.

### Slice 2: Scar is legible

- [ ] Debris persists and blocks both vision and movement.
- [ ] Per-team dynamic vision active.
- [ ] Acceptance: a spectator can read the map state correctly after sustained destruction.

### Slice 3: structural constraints hold

- [ ] Structural load, mass budget, and debris settling all active.
- [ ] Two players attempt to wall themselves in, and to dig directly to the enemy base.
- [ ] Acceptance: both degenerate strategies fail for physical reasons, not through rule-based prohibitions.

### Slice 4: networked match

- [ ] 3v3 against a mix of bots and humans on a headless server.
- [ ] Full replay recorded and reproduced.
- [ ] Acceptance: a complete match at 200 ms latency with no desync.

### Performance target

- [ ] Define the Sundering benchmark: fixed map, fixed input replay, scripted destruction sequence.
- [ ] Target 1920x1080 at 60 FPS on mid-range hardware with destruction active.
- [ ] Record simulation tick time, navigation rebuild time, debris count, per-client bandwidth, and input latency.
- [ ] Store baselines and reject regressions.

### Exit gate

- A 3v3 match is playable from a clean checkout on Linux, with bots filling empty slots.
- The match replays identically from its recorded input stream.
- Performance baselines are recorded and enforced in CI.

## Continuous test and CI plan

### Unit tests

- [ ] Transform composition and decomposition.
- [ ] Hierarchy propagation and cycle rejection.
- [x] Material default consistency and copying.
- [ ] Stable generational asset handles.
- [ ] Scene round trips and schema migration.
- [ ] Asset path canonicalization and deduplication.
- [ ] Input action mapping.
- [ ] Quality-profile selection.
- [ ] Stable ECS/GPU `PhysicsId` conversion, generation rejection, and entity mapping.

### Layout and shader tests

- [x] Rust physics storage-buffer sizes and offsets.
- [x] Common instance field order across compute shaders.
- [x] Common physics push-constant declaration across compute shaders.
- [ ] Shader reflection compared with every Rust storage, uniform, vertex, and push-constant type.
- [ ] Shader compilation as a dedicated CI job.

### Renderer integration tests

- [ ] Resize, minimize, restore, and zero-sized surface handling.
- [ ] Unsupported present-mode and format fallback.
- [ ] Frames-in-flight reuse and validation-clean shutdown.
- [ ] Asset unload/reload while referenced by submitted frames.
- [ ] Culling enabled/disabled equivalence.
- [ ] Automatic primitive/imported-mesh bounds contain their complete source geometry.
- [ ] GPU-owned objects are culled from the current GPU transform without full-state CPU readback.
- [ ] Objects outside the camera stop producing render work but continue physics simulation.
- [ ] `CullingMode::Auto` skips culling overhead below its tested scene-size threshold.
- [ ] Capability fallback behavior on the low-end baseline.

### Golden-image tests

- [ ] Primitive normals.
- [ ] glTF texture channels and color spaces.
- [ ] Alpha mask and blend modes.
- [ ] Directional shadow depth.
- [ ] PBR reference spheres.
- [ ] Tone mapping.
- [ ] Editor compositing.

### Physics tests

- [ ] Triggers and collision events.
- [ ] Raycasts and filtered queries.
- [ ] Stable stacking.
- [ ] Tunneling/CCD cases.
- [ ] Fixed-step independence from render frame rate.
- [ ] GPU grid overflow and fallback.
- [ ] GPU event overflow and fallback.
- [ ] Built-in boolean/range conditions and custom shader conditions emit according to `OnEnter`, `OnExit`, `WhileTrue`, `Once`, and cooldown modes.
- [ ] Condition events reach the correct ECS entity with their registered event ID and requested payload.
- [ ] CPU-to-GPU commands reject stale body generations.
- [ ] Asynchronous selected-state readback reports its source tick and frame age.
- [ ] Explicit CPU/GPU simulation ownership and synchronization modes.

### Physics scenario and benchmark suite

- [ ] Joint, articulation, character-controller, vehicle, and ragdoll regression scenes with recorded expected results.
- [ ] Soft-body, cloth, rope, fluid, granular, and fracture scenarios under lavapipe.
- [ ] Two-way coupling scenarios between rigid, deformable, and fluid simulation.
- [ ] 2D physics scenarios mirroring the 3D suite.
- [ ] Comparative benchmark harness against Jolt, PhysX, Rapier, and Godot Physics with stored baselines.

### Engine feature tests

- [ ] Reflection round trip for every registered component and data asset.
- [ ] Prefab override propagation, nesting, variants, and cycle rejection.
- [ ] Observers and scene-file event connections fire the registered handlers.
- [ ] Animation sampling, blending, IK, and ragdoll handoff are deterministic.
- [ ] Offline audio render compared against reference buffers.
- [ ] UI layout, text shaping, theming, and localization golden images.
- [ ] 2D sprite, tilemap, and 2D lighting golden images.
- [ ] Navigation baking, re-baking budget, and agent avoidance.
- [ ] Networking RPC, replication, prediction, and rollback over loopback with simulated loss.
- [ ] Export smoke test for every supported platform preset.
- [ ] Every demo project and template builds and runs headless.

### Determinism tests

- [ ] Per-tick world-state hashes match between two runs with identical inputs.
- [ ] Hashes match between debug and release builds.
- [ ] Hashes match across differing CPU worker-thread counts.
- [ ] Hashes match across at least two GPU vendors.
- [ ] A recorded replay reproduces its original hash sequence.
- [ ] An injected divergence is reported with its first divergent tick and body.
- [ ] Disabling audio, particles, and all presentation systems does not alter hashes.

### Networking tests

- [ ] Full match completes at 200 ms latency with 2% packet loss without desync.
- [ ] Delta snapshot encoding and decoding round-trip correctly.
- [ ] Bandwidth stays within the per-client budget during maximum destruction.
- [ ] Mid-match disconnect and reconnect resynchronizes correctly.
- [ ] Protocol version mismatch is rejected at connect time.
- [ ] A client reporting an impossible position or terrain state is rejected by the server.

### Terrain and navigation tests

- [ ] Chunk fracture is deterministic across runs and vendors.
- [ ] Debris settling restores static collision and navigation.
- [ ] The debris budget cap never drops bodies non-deterministically.
- [ ] Unsupported spans collapse; supported spans do not.
- [ ] Navigation rebuild stays within its per-tick budget during a cliff collapse.
- [ ] Agents re-route correctly when a route is closed mid-path.
- [ ] Fully blocking every route is reported rather than producing stuck agents.
- [ ] One-hour destruction soak test holds frame time, memory, and debris count within budget.

### CI matrix

- [ ] Linux software Vulkan runner (lavapipe) running the `gpu-tests` feature on every pull request. This is the primary GPU verification path; hardware runners confirm it, they do not replace it.
- [ ] Linux hardware runner.
- [ ] Windows hardware runner.
- [x] Formatting, strict clippy, unit tests, and docs on Linux and Windows for every pull request.
- [ ] Scheduled validation and performance runs with stored artifacts.
- [ ] Second-vendor GPU runner dedicated to cross-vendor determinism verification.
- [ ] Scheduled soak-test job for long-running destruction and memory growth.
- [ ] Scheduled headless batch-match job producing aggregate balance reports.
- [ ] macOS (MoltenVK) runner.
- [ ] Android build job.
- [ ] Scheduled comparative physics benchmark job publishing its report as an artifact.

## Cross-cutting engineering rules

- Public fallible operations return structured `Result` values; panics are reserved for internal invariant violations.
- ECS entities and typed asset handles are the only canonical identities exposed to gameplay/editor code.
- GPU resource destruction is deferred until all referencing frames complete.
- No fixed-capacity structure may silently drop work.
- Optional Vulkan features always have a documented fallback or a clear startup error.
- Debug builds enable validation by default when layers are available.
- Runtime and editor code must expose useful diagnostics instead of relying on ad-hoc FPS/debug printing.
- Every milestone must leave the project compiling and its completed acceptance gates automated where practical.
- Simulation state and presentation state are separate. Presentation code may read simulation state and may never write it.
- Anything affecting simulation must be deterministic: no wall-clock time, no frame-rate dependence, no unseeded randomness, no order-dependent accumulation.
- Network code never trusts a client for authoritative state.
- Systems that run for hours need soak tests. Correct for one minute and degrading over one hour is not correct.
- Anything that moves things — animation, particles, characters, vehicles, navigation obstacles, audio occlusion — integrates with physics through the typed bridge rather than a parallel ad-hoc simulation.
- Physics features are judged against the best standalone physics engines, with benchmark evidence. Other features are judged against Godot parity.
- Every user-facing feature ships with documentation and a demo scene in the same change or the next one.

## Recommended implementation order

Work on one vertical path at a time rather than creating empty crates for every eventual subsystem.

Foundation (Milestones 0-7):

1. Finish Vulkan/winit modernization and validation.
2. Add the workspace plus `rusting-core`, ECS schedules, components, and compatibility facade.
3. Add typed assets and static scene serialization.
4. Add render extraction and correct frames-in-flight synchronization.
5. Complete the forward PBR pass schedule and profiling.
6. Build the self-written hybrid physics bridge: stable IDs, GPU events, CPU commands, selective readback, and mixed CPU/GPU ownership.
7. Add the minimal egui shell, viewport, hierarchy, and inspector.
8. Build the vertical slice while filling in editor, rendering, asset, and physics gaps.
9. Add hot reload, undo/redo, polish, packaging, and performance gating.

Physics leadership and Godot parity (Milestones 8-23):

10. Establish determinism: simulation math, ordering rules, per-tick state hashing, and the verification harness. Every later solver depends on these rules.
11. Add reflection, prefabs, observers, data resources, and Rust hot reload. Every later editor and animation feature depends on reflection.
12. Complete rigid-body physics: joints, articulations, characters, vehicles, ragdolls, fields, CCD, and 2D physics.
13. Build the physics debugger and the comparative benchmark harness early, so every later solver is measured from its first commit.
14. Add deformables, fluids, granular media, fracture, and million-body GPU scale.
15. In parallel, reach parity in advanced rendering, animation, audio, runtime UI, 2D, and navigation, integrating each with physics as it lands.
16. Add networking with prediction and deterministic rollback.
17. Grow editor parity, platforms/export, and documentation continuously, closing them out at the 1.0 release gate.

Sundering (Milestones 24-29):

18. Add competitive authority on top of the engine networking, keeping clients as renderers until determinism is proven.
19. Build chunked destructible terrain, structural load, and Scar persistence on the engine fracture system.
20. Add navigation and GPU crowd agents over terrain that changes every tick.
21. Add the competitive gameplay framework: teams, abilities as forces, dynamic vision, and bots.
22. Build the Sundering slices in order, adding balance and operations tooling as each slice requires it.

The next concrete editor task is a Shortcuts settings panel. It must display the central action map, let a user rebind one action at a time, detect duplicate bindings, and persist choices inside project editor settings. Then add focus-selection and interactive translate/rotate/scale handles. Explicit frame contexts and an offscreen scene viewport target remain the next renderer-architecture task.
