# RustingEngine Roadmap

RustingEngine is currently a functional Vulkan renderer prototype. The goal of this roadmap is to evolve it into a playable Windows/Linux game engine with an integrated egui editor, stable runtime architecture, and a self-written hybrid physics system that can divide work between the CPU and GPU without making GPU state invisible to gameplay.

This roadmap now covers two products: the general-purpose engine (Milestones 0-7) and *Sundering*, a deterministic, networked, destructible competitive game built on it (Milestones 8-15).

This document is the implementation source of truth. Tasks should be completed in dependency order, kept behind compiling intermediate states, and verified against the acceptance gates at the end of each milestone.

Long-lived ownership boundaries and dependency rules are recorded in [`architecture.md`](architecture.md). Roadmap work must preserve those boundaries or document a migration before changing them.

## Product target

The first major release should provide:

- A real application runtime with ECS entities, components, resources, schedules, events, input, and fixed updates.
- A Vulkan renderer with explicit frames in flight, forward PBR, shadows, transparency, HDR tone mapping, culling, LOD, and profiling.
- Typed, deduplicated assets with glTF import, scene serialization, and hot reload.
- Self-written hybrid physics with per-object CPU/GPU allocation, asynchronous GPU events, selective state readback, and custom compute shaders.
- An in-engine egui editor with a viewport, hierarchy, inspector, asset browser, console, profiler, gizmos, and play controls.
- A representative vertical-slice game and a repeatable 1080p performance benchmark.

Primary platforms are Windows and Linux desktop. Native Rust systems are the gameplay API, and games remain normal Cargo projects that can use external libraries. A versioned custom physics-compute ABI is part of the hybrid milestone; Lua/WASM, deferred rendering, and an unrestricted custom render-shader ABI remain deferred.

## Second product target: Sundering

Milestones 0-7 build an engine. Milestones 8-15 build what *Sundering* needs on top of it: a 5v5 competitive game whose terrain is made of millions of simulated chunks that come apart permanently. Its design document lives at `~/Sundering/README.md`.

Sundering demands four things a general-purpose engine does not:

- **Determinism.** Competitive play requires the simulation to produce identical results for every participant. This constrains the physics solver, the GPU reduction strategy, and the ordering of every simulation operation.
- **Authoritative networking.** A server owns the simulation; clients must stay consistent with it under real latency and loss.
- **Destructible terrain as simulation state.** Terrain is not authored geometry. It is a first-class simulated entity that feeds collision, navigation, and vision.
- **Navigation over geometry that changes every tick.** Nav meshes assume a static world. Sundering has no static world.

**Determinism cannot be retrofitted.** Milestone 8 defines the full requirements and their acceptance gate, but three of its ordering rules must be respected while Milestone 5 is implemented, or that work will need rewriting. They are restated at the top of Milestone 5.

Milestones 8-15 depend on Milestone 5 being complete. Milestone 8 must precede Milestone 9, because the determinism result decides which networking model is available.

## Architectural direction

The project should become a Cargo workspace with one-way dependencies:

```text
rusting-math       deterministic math, fixed-point, seeded RNG streams
      ↑
rusting-core       ECS components, schedules, time, input, hierarchy, events
      ↑
rusting-assets     typed handles, cache, importers, serialization, hot reload
      ↑
rusting-physics    CPU physics, GPU compute simulation, synchronization, queries
      ↑
rusting-terrain    volumetric chunks, fracture, structural load, Scar persistence
      ↑
rusting-nav        navigation volumes, flow fields, crowd agents
      ↑
rusting-render     Vulkan context, extraction, frame graph, materials, profiling
rusting-net        authority, wire protocol, replication, interest mechanism
rusting-audio      device management, mixing, buses, spatialization
      ↑
rusting-gameplay   teams, match state, abilities as forces, vision, bots
      ↑
rusting-editor     egui panels, viewport, inspector, gizmos, play controls
      ↑
rusting-engine     plugins, application facade, compatibility API
      ↑
game projects      vertical slice, Sundering
```

`rusting-render`, `rusting-net`, and `rusting-audio` are siblings at one level and must not depend on each other. Full layer definitions are in [`architecture.md`](architecture.md).

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
- [x] `GpuEffectBody` marker for bodies currently assigned to GPU simulation.
- [x] `FrameTime`, `TimeControl`, `RenderSettings`, `PhysicsSettings`, and `QualityProfile` resources.
- [x] Input action mapping with keyboard, mouse, and gamepad-ready abstractions.

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

- [x] Deduplicate glTF meshes, images, and materials by synthesized asset path (samplers still unhandled).
- [x] Import base-color textures with sRGB formats.
- [x] Import normal, metallic-roughness, occlusion, and emissive textures with correct linear/sRGB treatment.
- [ ] Import sampler filtering and wrapping modes.
- [ ] Generate or import tangents.
- [ ] Support alpha opaque, mask, and blend modes.
- [ ] Import node hierarchy and cameras/lights where available.
- [ ] Add skins and animation after the static scene path is stable.
- [ ] Preserve glTF materials unless the caller explicitly supplies an override.

### Scene serialization

- [x] Define versioned, human-readable `.rscene` files with Serde plus a compact cooked binary form.
- [x] Assign stable UUIDs to serialized scene objects.
- [x] Store asset paths/UUIDs rather than runtime handles or ECS entity IDs.
- [x] Add an allowlisted component serialization registry.
- [ ] Serialize hierarchy, transforms, renderers, lights, physics, and editor metadata.
- [x] Reject unsupported scene versions with a clear structured error.
- [x] Add explicit migration for legacy unversioned project and text-scene files.
- [x] Add scene save, load, additive load, and unload operations.

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

Milestone 7 and Milestone 15 have deliberately different hardware targets. The engine vertical slice must run on low-end integrated graphics. Sundering must not: continuous destruction with thousands of debris bodies and GPU navigation has a hardware floor, and pretending otherwise would distort the engine's quality profiles. Neither target is allowed to weaken the other.

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

Goal: make the simulation produce bit-identical results from the same inputs on every supported device, and make any divergence detectable and locatable.

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
- If bit-identical cross-vendor results prove unachievable, that result is documented with evidence, and Milestone 9 proceeds with server-authoritative networking without rollback.

## Milestone 9: Networked authority

Goal: run the authoritative simulation on a server and keep many clients consistent with it under real network conditions.

The networking model depends on the Milestone 8 result. Bit-identical determinism permits lockstep and rollback. Without it, the server simulates and clients render, predicting only their own hero.

### Server

- [ ] Headless server binary running the fixed-tick simulation with no renderer or window.
- [ ] Tick-scheduled loop with drift correction, overload detection, and reported tick-budget usage.
- [ ] Client lifecycle: join, ready, in-match, drop, reconnect.
- [ ] Server-side input validation and rate limiting. The server never trusts a client-reported position, velocity, or terrain state.

### Transport and protocol

- [ ] Unreliable-ordered channel for state and a reliable channel for match events, over UDP.
- [ ] Versioned wire protocol with explicit compatibility rejection at connect time.
- [ ] Clock synchronization and per-client tick-offset estimation.
- [ ] Input buffering with configurable delay and jitter absorption.

### State replication

- [ ] Snapshot encoding with delta compression against the last client-acknowledged snapshot.
- [ ] Quantize replicated values with documented precision per field.
- [ ] Interest management: replicate only what a client's team can currently see, using the vision system from Milestone 13.
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

## Milestone 10: Destructible terrain and Scar

Goal: make terrain a first-class simulated, modifiable, and permanently scarred entity rather than authored static geometry.

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

## Milestone 11: Dynamic navigation and crowds

Goal: let thousands of agents move sensibly over geometry that changes every tick.

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

## Milestone 12: Runtime presentation

Goal: supply the non-simulation systems a shipped game needs, while guaranteeing none of them can influence simulation state.

### Animation

- [ ] Skeletal animation with GPU skinning.
- [ ] glTF skin and animation import, completing the Milestone 2 deferral.
- [ ] Animation state machine with blending, layers, and events.
- [ ] Root-motion policy that never feeds back into deterministic simulation.
- [ ] Ragdoll handoff between animation and physics.

### Audio

- [ ] Audio device management, mixer, buses, and volume control.
- [ ] 3D spatial audio with attenuation and an occlusion approximation.
- [ ] Event-driven playback triggered by gameplay events and by GPU physics events.
- [ ] Voice limiting and priority so large destruction events do not exhaust the mixer.
- [ ] Audio state is presentation only and never affects simulation or replay hashes.

### Visual effects

- [ ] GPU particle system reusing the existing physics compute infrastructure.
- [ ] Emission driven by physics events: impacts, fracture, debris settling.
- [ ] Decals that either survive terrain modification or are invalidated with it.
- [ ] Effect budget and quality-profile scaling.

### Runtime UI

- [ ] Runtime UI layer independent of the editor's egui usage.
- [ ] Data-bound HUD elements, world-space indicators, and input routing.
- [ ] Scaling across resolutions and DPI settings.

### Input and frame pacing

- [ ] Measure and report end-to-end input latency.
- [ ] Decouple input sampling rate from the simulation tick without introducing nondeterminism.
- [ ] Frame pacing that holds a stable presentation cadence under variable GPU load.

### Exit gate

- Disabling all audio and visual effects does not change replay hashes.
- Animated, skinned characters render and hand off to ragdoll on physics events.
- End-to-end input latency is measured and reported.

## Milestone 13: Competitive gameplay framework

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
- [ ] Feed visibility into network interest management from Milestone 9.
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

## Milestone 14: Balance and operations tooling

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

## Milestone 15: Sundering vertical slice

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

- [ ] Linux software Vulkan runner for deterministic smoke tests where supported.
- [ ] Linux hardware runner.
- [ ] Windows hardware runner.
- [x] Formatting, strict clippy, unit tests, and docs on Linux and Windows for every pull request.
- [ ] Scheduled validation and performance runs with stored artifacts.
- [ ] Second-vendor GPU runner dedicated to cross-vendor determinism verification.
- [ ] Scheduled soak-test job for long-running destruction and memory growth.
- [ ] Scheduled headless batch-match job producing aggregate balance reports.

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

## Recommended implementation order

Work on one vertical path at a time rather than creating empty crates for every eventual subsystem:

1. Finish Vulkan/winit modernization and validation.
2. Add the workspace plus `rusting-core`, ECS schedules, components, and compatibility facade.
3. Add typed assets and static scene serialization.
4. Add render extraction and correct frames-in-flight synchronization.
5. Complete the forward PBR pass schedule and profiling.
6. Build the self-written hybrid physics bridge: stable IDs, GPU events, CPU commands, selective readback, and mixed CPU/GPU ownership.
7. Add the minimal egui shell, viewport, hierarchy, and inspector.
8. Build the vertical slice while filling in editor, rendering, asset, and physics gaps.
9. Add hot reload, undo/redo, polish, packaging, and performance gating.

Then the Sundering track, which depends on the hybrid physics of step 6:

10. Establish determinism: simulation math, ordering rules, per-tick state hashing, headless mode, and the cross-vendor verification harness. This result decides the networking model, so it comes first.
11. Add replay recording and playback on top of the state hash.
12. Add the headless server, wire protocol, and replication. Keep clients as renderers until determinism is proven.
13. Build chunked destructible terrain, fracture, structural load, and Scar persistence.
14. Add dynamic navigation and GPU crowd agents over changing geometry.
15. Fill in runtime presentation: animation, audio, particles, runtime UI, and frame pacing.
16. Add the competitive gameplay framework: teams, abilities as forces, dynamic vision, and bots.
17. Build the Sundering slices in order, adding balance and operations tooling as each slice requires it.

The next concrete editor task is a Shortcuts settings panel. It must display the central action map, let a user rebind one action at a time, detect duplicate bindings, and persist choices inside project editor settings. Then add focus-selection and interactive translate/rotate/scale handles. Explicit frame contexts and an offscreen scene viewport target remain the next renderer-architecture task.
