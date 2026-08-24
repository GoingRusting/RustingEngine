# RustingEngine Roadmap

RustingEngine is currently a functional Vulkan renderer prototype. The goal of this roadmap is to evolve it into a playable Windows/Linux game engine with an integrated egui editor, stable runtime architecture, CPU-authoritative gameplay physics, and optional high-volume GPU effects.

This document is the implementation source of truth. Tasks should be completed in dependency order, kept behind compiling intermediate states, and verified against the acceptance gates at the end of each milestone.

Long-lived ownership boundaries and dependency rules are recorded in [`architecture.md`](architecture.md). Roadmap work must preserve those boundaries or document a migration before changing them.

## Product target

The first major release should provide:

- A real application runtime with ECS entities, components, resources, schedules, events, input, and fixed updates.
- A Vulkan renderer with explicit frames in flight, forward PBR, shadows, transparency, HDR tone mapping, culling, LOD, and profiling.
- Typed, deduplicated assets with glTF import, scene serialization, and hot reload.
- Rapier-authoritative gameplay physics plus an explicitly separate GPU effects simulation tier.
- An in-engine egui editor with a viewport, hierarchy, inspector, asset browser, console, profiler, gizmos, and play controls.
- A representative vertical-slice game and a repeatable 1080p performance benchmark.

Primary platforms are Windows and Linux desktop. Rust systems are the initial gameplay API. Lua/WASM scripting, deferred rendering, and a stable custom-shader ABI are deferred until after the first vertical slice.

## Architectural direction

The project should become a Cargo workspace with one-way dependencies:

```text
rusting-core       ECS components, schedules, time, input, hierarchy, events
      ↑
rusting-assets     typed handles, cache, importers, serialization, hot reload
      ↑
rusting-physics    Rapier integration and optional GPU-effect simulation
      ↑
rusting-render     Vulkan context, extraction, frame graph, materials, profiling
      ↑
rusting-editor     egui panels, viewport, inspector, gizmos, play controls
      ↑
rusting-engine     plugins, application facade, compatibility API
      ↑
vertical-slice    integration, gameplay, visual, and performance target
```

Dependency cycles between runtime, renderer, physics, assets, and editor are not allowed. ECS entities are canonical state. Render batches, physics arrays, indirect buffers, and other GPU representations are derived data.

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

- [ ] Move compatibility-facade window and surface creation into `ApplicationHandler::resumed`, removing the last deprecated winit call.
- [ ] Replace public initialization and asset-loading panics with typed errors and `Result` APIs.
- [ ] Add Vulkan debug names and scoped command-buffer labels.
- [ ] Validate rendering, resizing, minimizing, restoring, and shutdown on Linux and Windows.
- [ ] Replace fixed grid limits with explicit capacity tracking and overflow reporting. Never silently omit bodies.
- [ ] Add generated/reflected layout checks for storage buffers, uniforms, vertex data, and push constants.

### Exit gate

- Debug validation produces no errors during startup, rendering, resize, minimize/restore, and shutdown.
- Linux and Windows smoke tests render at least 1,000 frames.
- No known CPU/GPU layout mismatch remains.
- Formatting, strict clippy, tests, shader compilation, and all-target checking pass in CI.

## Milestone 1: Workspace and runtime foundation

Goal: create the engine runtime that owns canonical scene state and system execution.

### Workspace

- [ ] Convert the package into a Cargo workspace using the architectural dependency direction above.
- [ ] Move reusable public data types into `rusting-core` without Vulkan dependencies.
- [ ] Keep compatibility re-exports in `rusting-engine` during migration.
- [ ] Add feature flags for editor, validation, experimental GPU physics, and optional importers.

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
- [x] `GpuEffectBody` marker with explicit non-authoritative semantics.
- [x] `FrameTime`, `TimeControl`, `RenderSettings`, `PhysicsSettings`, and `QualityProfile` resources.
- [ ] Input action mapping with keyboard, mouse, and gamepad-ready abstractions.

### Runtime ownership

- [ ] Split the monolithic loop into `WindowRunner`, `Renderer`, `PhysicsWorld`, `AssetServer`, and optional `EditorState`.
- [ ] Keep the window, ECS world, and rendering submission main-thread owned.
- [ ] Remove unnecessary `Arc<Mutex<_>>` usage from camera and scene state.
- [ ] Use worker tasks only for safe asset decoding and preparation.
- [ ] Execute animations as ECS systems instead of passive data.

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
- [ ] Add worker-backed asynchronous loading states.
- [x] Add default/fallback mesh, material, and texture assets.

### Import pipeline

- [ ] Deduplicate glTF meshes, images, samplers, and materials.
- [ ] Import base-color textures with sRGB formats.
- [ ] Import normal, metallic-roughness, occlusion, and emissive textures with correct linear/sRGB treatment.
- [ ] Import sampler filtering and wrapping modes.
- [ ] Generate or import tangents.
- [ ] Support alpha opaque, mask, and blend modes.
- [ ] Import node hierarchy and cameras/lights where available.
- [ ] Add skins and animation after the static scene path is stable.
- [ ] Preserve glTF materials unless the caller explicitly supplies an override.

### Scene serialization

- [ ] Define versioned `.rscene` files using Serde and RON.
- [ ] Assign stable UUIDs to serialized scene objects.
- [ ] Store asset paths/UUIDs rather than runtime handles or ECS entity IDs.
- [ ] Add an allowlisted component serialization registry.
- [ ] Serialize hierarchy, transforms, renderers, lights, physics, and editor metadata.
- [ ] Add schema migration hooks and clear errors for unsupported versions.
- [ ] Add scene save, load, additive load, and unload operations.

### Hot reload

- [ ] Watch source assets and scenes for changes.
- [ ] Decode changed assets in worker tasks.
- [ ] Swap prepared resources at safe frame boundaries.
- [ ] Keep old GPU resources alive until every referencing frame completes.
- [ ] Surface reload success and failure in the editor console.

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
- [ ] Grow GPU buffers based on demand and device memory budgets.
- [ ] Add explicit errors or fallback paths for allocation/capacity failures.

### Frames in flight

- [ ] Create two or three explicit `FrameContext` objects.
- [ ] Give each context its own fence, command allocator/pool state, uniform allocations, indirect buffers, visible lists, and transient descriptors.
- [ ] Wait only when reusing a frame context whose fence has not completed.
- [ ] Chain acquire, transfer/compute, render, and present without CPU `wait()` calls in the normal frame path.
- [ ] Retain every submitted future/fence until completion.
- [ ] Add deferred GPU-resource destruction queues per frame context.
- [ ] Clear culling counters and indirect commands using queued GPU operations.
- [ ] Remove host writes to buffers that may still be in GPU use.

### Capabilities and fallback policy

- [ ] Define a low-end Vulkan baseline suitable for Intel UHD 620-class hardware.
- [ ] Detect optional capabilities and expose them through renderer capabilities.
- [ ] Provide fallbacks for bindless descriptors, indirect-count drawing, and other advanced features.
- [ ] Implement `QualityProfile::{Auto, Eco, Balanced, High}`.
- [ ] Remove shader variants as user-facing performance controls; choose implementation variants automatically.

### Exit gate

- No normal-frame CPU fence waits remain.
- Validation reports no synchronization or resource-lifetime errors.
- Two/three frames in flight can run for 10,000 frames without UBO, indirect-buffer, or descriptor races.
- Culling enabled and disabled produce equivalent visible geometry.

## Milestone 4: Rendering baseline

Goal: deliver a coherent, production-shaped forward renderer for the vertical slice.

### Pass schedule

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

- [ ] CPU or GPU frustum culling selected by capability/profile.
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

Goal: make gameplay physics reliable and preserve GPU simulation for deliberately non-authoritative effects.

### Authoritative Rapier physics

- [ ] Add `rapier3d` and a `PhysicsWorld` resource.
- [ ] Synchronize ECS rigid bodies/colliders into Rapier using stable entity mappings.
- [ ] Write authoritative dynamic transforms back to ECS after fixed updates.
- [ ] Support fixed, dynamic, and kinematic rigid bodies.
- [ ] Support boxes, spheres, capsules, convex meshes, and static triangle meshes.
- [ ] Add triggers/sensors and collision events.
- [ ] Add collision groups and query filters.
- [ ] Add raycasts, shape casts, overlap queries, and editor picking queries.
- [ ] Add character movement and joints required by the vertical slice.
- [ ] Add sleeping and continuous collision detection settings.
- [ ] Support snapshots needed for play/stop restoration.

### GPU effects tier

- [ ] Introduce `SimulationClass::{Gameplay, GpuEffect, Static}`.
- [ ] Document and enforce one-way ownership: gameplay/static transforms may be uploaded, but GPU contacts never synchronously drive gameplay state.
- [ ] Use GPU simulation for particles, debris, crowds, or other query-free bodies.
- [ ] Replace fixed hash capacities with device-budgeted growable buffers.
- [ ] Track grid cell overflow and total overflow counters.
- [ ] Implement a tested fallback when a grid cell or table overflows.
- [ ] Ensure bodies are never silently omitted.

### Experimental custom solver

- [ ] Move the current solver behind an `experimental-gpu-physics` feature.
- [ ] Add a true spatial broad phase before claiming sub-quadratic collision complexity.
- [ ] Add stable contact generation, iterative solving, sleeping, and continuous collision detection.
- [ ] Compare deterministic scenarios and tolerances against Rapier.

### Exit gate

- Tests cover triggers, raycasts, stacking, tunneling, collision layers, and fixed-step independence.
- GPU grid overflow is observable and its fallback preserves every body.
- Gameplay queries never depend on unread GPU results.
- Rapier and GPU-effect ownership boundaries are visible in components and editor UI.

## Milestone 6: Integrated egui editor

Goal: allow scenes to be built, inspected, saved, and played without leaving the engine.

The editor should use `egui` and `egui-winit`. Rendering should go through an engine-owned Vulkan painter/integration layer so renderer compatibility and resource lifetime remain under project control.

### Editor shell

- [x] Add a feature-gated editor plugin that can be excluded from runtime builds.
- [x] Build the initial egui shell with toolbar, hierarchy, transform inspector, viewport placeholder, and structured console.
- [x] Connect play, pause, single-step, and stop controls to runtime time control.
- [x] Show typed asset inventory, mesh/material handles, and live render-extraction dirty ranges in editor panels.
- [x] Add a runnable Vulkan editor window with winit input, resize handling, DPI-aware egui rendering, and presentation.
- [x] Render extracted ECS meshes as a depth-tested Vulkan scene beneath the editor UI.
- [x] Add revision-aware GPU mesh preparation so asset mutation invalidates cached buffers without changing handles.
- [x] Drive the scene viewport and camera aspect ratio from the DPI-aware central-panel pixel rectangle.
- [x] Expose camera FOV, clipping planes, priority, and active state in the inspector.
- [ ] Add a dockable main layout and persistent panel arrangement.
- [ ] Replace the bootstrap Vulkan egui integration with an engine-owned texture/mesh upload path and render pass.
- [ ] Route keyboard and mouse focus correctly between viewport navigation and UI.
- [ ] Add DPI scaling, font configuration, and theme persistence.

### Core panels

- [ ] Scene viewport rendered to an editor texture.
- [ ] Entity hierarchy with filtering, selection, reparenting, and drag/drop.
- [ ] Component inspector driven by an allowlisted reflection/editor registry.
- [ ] Asset browser with folders, thumbnails, filtering, and drag/drop assignment.
- [ ] Console with structured logs, filtering, warnings, and asset/validation errors.
- [ ] Profiler with CPU spans, GPU pass timings, counters, and memory usage.
- [ ] Render and physics settings panels.

### Scene editing

- [ ] Selection outlines and editor-only overlays.
- [ ] Translate, rotate, and scale gizmos with local/global modes and snapping.
- [ ] Camera orbit, pan, fly, focus-selection, and framing controls.
- [ ] Create, duplicate, rename, delete, and reparent entities.
- [ ] Add/remove/edit supported components.
- [ ] Assign meshes, materials, textures, and physics shapes by typed handle.
- [ ] Scene new/open/save/save-as operations.

### Play workflow and history

- [ ] Edit, play, pause, single-step, and stop states.
- [ ] Snapshot the edit scene before play and restore it on stop.
- [ ] Make runtime-spawned entities visually distinct where useful.
- [ ] Implement command-based undo/redo for scene edits.
- [ ] Group continuous gizmo/field edits into single undo transactions.
- [ ] Mark scenes dirty and prompt before destructive close/load actions.

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
- [ ] Authoritative collisions, triggers, and at least one physics query.
- [ ] Directional shadows, point lights, sky/environment light, and transparent content.
- [ ] GPU debris, particles, or crowds demonstrating the effects tier.
- [ ] Runtime UI and editor UI.
- [ ] Scene persistence and live asset reload.
- [ ] Runtime editing through play/pause/stop.
- [ ] Eco, Balanced, and High quality comparisons.

### Performance target

- [ ] Define a fixed benchmark scene and camera path.
- [ ] Target 1920×1080 at 60 FPS on approximately Intel UHD 620-class hardware using Eco/Auto settings.
- [ ] Record CPU frame time, GPU pass time, draw/dispatch count, triangles, memory, visible instances, upload bytes, physics bodies, and grid overflow.
- [ ] Store performance baselines and reject material regressions rather than relying only on FPS logs.
- [ ] Document tested drivers, operating systems, resolutions, and quality settings.

### Exit gate

- The vertical slice is playable from a clean checkout on Windows and Linux.
- Its scenes can be edited and saved in the integrated editor.
- Automated smoke, visual, physics, serialization, and performance tests pass.
- Packaging produces a runnable build with required assets and licenses.

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
- [ ] ECS/Rapier conversion and entity mapping.

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
- [ ] Explicit CPU/GPU simulation ownership boundaries.

### CI matrix

- [ ] Linux software Vulkan runner for deterministic smoke tests where supported.
- [ ] Linux hardware runner.
- [ ] Windows hardware runner.
- [ ] Formatting, strict clippy, unit tests, docs, and shader compilation on every pull request.
- [ ] Scheduled validation and performance runs with stored artifacts.

## Cross-cutting engineering rules

- Public fallible operations return structured `Result` values; panics are reserved for internal invariant violations.
- ECS entities and typed asset handles are the only canonical identities exposed to gameplay/editor code.
- GPU resource destruction is deferred until all referencing frames complete.
- No fixed-capacity structure may silently drop work.
- Optional Vulkan features always have a documented fallback or a clear startup error.
- Debug builds enable validation by default when layers are available.
- Runtime and editor code must expose useful diagnostics instead of relying on ad-hoc FPS/debug printing.
- Every milestone must leave the project compiling and its completed acceptance gates automated where practical.

## Recommended implementation order

Work on one vertical path at a time rather than creating empty crates for every eventual subsystem:

1. Finish Vulkan/winit modernization and validation.
2. Add the workspace plus `rusting-core`, ECS schedules, components, and compatibility facade.
3. Add typed assets and static scene serialization.
4. Add render extraction and correct frames-in-flight synchronization.
5. Complete the forward PBR pass schedule and profiling.
6. Integrate Rapier and formalize GPU-effect ownership.
7. Add the minimal egui shell, viewport, hierarchy, and inspector.
8. Build the vertical slice while filling in editor, rendering, asset, and physics gaps.
9. Add hot reload, undo/redo, polish, packaging, and performance gating.

The next concrete task is explicit frame contexts and an offscreen scene viewport target. `SceneRenderer` already accepts an arbitrary image target, so this should not require changes to ECS, typed assets, render extraction, or editor selection.
