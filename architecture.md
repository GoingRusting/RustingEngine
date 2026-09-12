# RustingEngine Architecture

This document records boundaries that must remain stable as the roadmap is implemented. New features should fit these boundaries instead of reaching across layers. If a boundary must change, update this document and add a migration plan before changing public APIs.

## Dependency direction

```text
runtime components and schedules
            ↓
typed CPU assets and handles
            ↓
RenderExtract snapshot
            ↓
GPU preparation cache
            ↓
render passes and presentation
            ↓
editor composition
```

Dependencies and data flow move downward. Rendering and editor code may observe runtime state through extraction, but must not become canonical owners of scene entities, transforms, materials, or asset identity.

### Crate layers

Each layer may depend only on layers below it. Crates sharing a layer are siblings and must not depend on each other.

```text
 0  rusting-math      deterministic scalar/vector math, fixed-point, seeded RNG streams
 1  rusting-core      ECS components, schedules, time, input, hierarchy, events
 2  rusting-assets    typed handles, cache, importers, serialization, hot reload
 3  rusting-physics   CPU physics, GPU compute simulation, synchronization, queries
 4  rusting-terrain   volumetric chunks, fracture, structural load, Scar persistence
 5  rusting-nav       navigation volumes, flow fields, crowd agents
 6  rusting-render    Vulkan context, extraction, frame graph, materials, profiling
    rusting-net       authority, wire protocol, replication, interest mechanism
    rusting-audio     device management, mixing, buses, spatialization
 7  rusting-gameplay  teams, match state, abilities as forces, vision, bots
 8  rusting-editor    egui panels, viewport, inspector, gizmos, play controls
 9  rusting-engine    plugins, application facade, compatibility API
10  game projects     vertical slice, Sundering
```

`rusting-math` sits below everything because both the CPU solver and the generated shader math must use one implementation. Splitting it later means rewriting every solver.

Game-specific content — heroes, ability definitions, map layouts — belongs in the game project at layer 10, not in `rusting-gameplay`. Layer 7 owns the machinery; layer 10 owns the content.

## Frame flow

```text
winit input
    ↓
FixedUpdate → Update → PostUpdate hierarchy propagation
    ↓
RenderExtract
    ↓
prepare changed asset revisions and dirty instance ranges
    ↓
acquire → 3D scene pass → egui overlay → present
```

The current editor renders the scene directly into the swapchain. The planned offscreen viewport changes only the render target passed to `SceneRenderer`; it must not change ECS, assets, extraction, or GPU preparation.

## Authoring and release build flow

```text
game/src/main.rs → Cargo release build ─┐
                                       ├─ runtime-only game executable
editor-authored .rscene → scene cooker ┘
```

- Gameplay systems are ordinary native Rust code in a separate Cargo game project.
- Common operations use a concise typed scene API; this is a wrapper over the same ECS world, not a second runtime or language.
- Game projects may use crates.io, path, git, or workspace dependencies like any other Cargo application.
- The editor edits project Rust source but does not translate it into a private language or store code in scene files.
- The editor saves component values, hierarchy, transforms, cameras, and asset references as data.
- Source `.rscene` files are reviewable JSON. Cooked `.rscene.bin` files use the same versioned schema in a compact startup format.
- Every saved entity has a persistent `SceneId`; runtime Bevy entity IDs are never stored.
- Game plugins explicitly register the custom Rust components allowed in scenes.
- Game project files stay under a project root such as `testGame/`; the editor never writes into engine `src/`.
- The game crate depends on the engine with the `window` feature and without the `editor` feature, so egui is absent from release runtime builds.

## Stable ownership decisions

### ECS is canonical

- Bevy `Entity` is the runtime identity.
- `Transform`, hierarchy, `MeshRenderer`, cameras, and lights live in the gameplay world.
- ECS owns physics identity and authored settings. For a GPU-owned body, its newest runtime transform may remain on the GPU while ECS stores the last synchronized value and its source tick.
- Render batches and GPU buffers are derived caches and may be discarded or rebuilt.
- Editor selection stores an `Entity`; it never stores a render-batch index.

### Asset identity is typed and generational

- Gameplay and editor code use `Handle<T>`.
- A stale generation never resolves after slot reuse.
- CPU assets are stored in `AssetServer`.
- Each mutable asset access advances its revision.
- GPU preparation caches key by typed handle and source revision.
- Submitted command buffers retain GPU resources; explicit retirement remains frame/fence based.

### Extraction is the only gameplay-to-render bridge

- `RenderWorld` is a renderer-facing snapshot.
- Extraction selects the active camera and visible renderables deterministically.
- Stable ordering uses asset handles plus entity identity.
- Changes produce dirty ranges; removals or reorderings may intentionally dirty the full affected range.
- Render passes must not query or mutate gameplay ECS directly.

### Physics choices are semantic scene data

- `SimulationClass` selects static, CPU, or GPU ownership; ownership can differ between bodies in one scene.
- `PhysicsSyncMode` selects no readback, typed events, selected state, or full state. Readback cost and latency are never hidden.
- A stable generation-checked `PhysicsId`, not a GPU array index, connects ECS bodies, GPU commands, readback state, and events.
- Static colliders are prepared once and excluded from dynamic compute dispatches.
- GPU solver profiles map to built-in compute pipelines during physics preparation.
- Custom shader paths are project source references, never GPU pipeline or descriptor indexes.
- CPU-to-GPU changes travel through batched commands. GPU-to-CPU changes travel through asynchronous events or requested snapshots.
- GPU conditions may be assembled through the typed Rust builder or implemented by custom compute code. Both emit through one versioned event ABI; arbitrary Rust closures do not execute on the GPU.
- A condition can only read GPU state and values explicitly uploaded by the CPU. Events may carry selected state back without mirroring the complete physics world.
- Normal gameplay must not wait for GPU readback. Same-tick queries use CPU bodies; GPU mirrors always expose the tick that produced them.
- The editor writes physics components into `.rscene`; backend buffers and dispatch groups remain derived data.

### Simulation state and presentation state are separate

This is the boundary that makes deterministic networked play possible, and the easiest one to break by accident.

- **Simulation state** is anything that contributes to the per-tick world-state hash: transforms, velocities, terrain volumes, navigation results, visibility, match state, ability state.
- **Presentation state** is everything else: interpolation, prediction smoothing, camera shake, audio voices, particles, decals, UI animation.
- Presentation systems may read simulation state. They may never write it, including through corrections that look harmless.
- Prediction error smoothing is presentation. It adjusts what is drawn, never what is simulated.
- The enforcing test is mechanical: disabling audio, particles, and all presentation systems must not change a replay's hash sequence.

### Determinism has exactly one owner

- `rusting-math` owns the simulation number format and the seeded RNG. Nothing else defines simulation arithmetic.
- Simulation code may not call standard-library transcendental functions, read a wall clock, read frame delta, or use an unseeded or thread-local RNG.
- Physics and terrain compute shaders include the same math module and compile with fast-math and relaxed precision disabled. Rendering shaders are unaffected.
- Every container iterated by simulation code has a defined, stable order that survives compaction, reuse, and re-sorting.
- Simulation state must never be accumulated through order-dependent atomic operations. Atomics remain fine for counters that do not feed results.
- The per-tick world-state hash is part of the engine, not a test utility, and stays enabled in release builds.

### Authority belongs to the server

- The server's ECS world is canonical. A client's ECS world is a replica.
- A client may hold presentation-only components the server does not have. It may not hold authoritative simulation state the server does not have.
- Clients predict only their own hero's input. No client predicts terrain destruction unless cross-vendor determinism is proven.
- The server validates all input and never accepts a client-reported position, velocity, or terrain state.
- Desync is detected by comparing state hashes, reported explicitly, and recovered by full resynchronization. It is never silently corrected.

### Terrain is canonical simulation state

- The terrain volume plus its ordered modification history is the source of truth.
- Collision geometry, render meshes, navigation data, and visibility are all derived from it and may be discarded and rebuilt at any time.
- Terrain modifications arrive as commands applied at a deterministic point in the tick, through the same bridge as physics commands.
- Scar is not a separate list of permanent bodies. Settled debris re-enters the terrain volume and becomes ordinary static terrain.
- Terrain replicates as modification events. Individual debris transforms are never replicated.

### Navigation is derived, never authored

- Navigation data is built from terrain collision, not from a designer-authored mesh. There is no authored navigation asset to fall out of sync.
- Rebuilds are incremental and budgeted. The budget is a hard cap: overruns are reported and deferred, never exceeded by running longer.
- Rebuild results are deterministic regardless of how many regions were dirtied, or in what order.
- Unreachable destinations are an observable gameplay state, not a stalled agent.

### Vision is simulation, not a rendering effect

- Per-team visibility is computed from actual terrain geometry, on the tick, deterministically.
- The server and every client compute identical visibility. A mismatch is a desync, not a cosmetic difference.
- Visibility feeds network interest management and gameplay rules. Fog rendering consumes the same data through extraction and is presentation only.

### Networking owns mechanism, gameplay owns policy

- `rusting-net` owns transport, protocol versioning, snapshot encoding, delta compression, and the interest-management mechanism.
- `rusting-gameplay` supplies interest policy — what a given team may currently see — through a trait that `rusting-net` defines.
- `rusting-net` must not depend on `rusting-gameplay`. This is what keeps the replication layer reusable and testable without a match running.

### Audio and effects are consumers only

- Audio and particles are triggered by gameplay and physics events, never by polling simulation state in a way that writes back.
- Voice limiting, effect budgets, and quality scaling are presentation policy and must produce identical simulation results whether they are applied or not.

### Renderer APIs are target-oriented

- `SceneRenderer` receives an image target, extent, `RenderWorld`, and `AssetServer`.
- It does not own a window, event loop, scene, or editor.
- Swapchain and future chaining belong to the window runner.
- An offscreen viewport, game window, thumbnail renderer, and golden-image test should reuse the same scene renderer.

### Editor is a plugin and consumer

- Runtime-only builds compile without editor dependencies.
- The editor edits canonical ECS components and typed handles.
- Egui input and compositing are owned by the window/editor integration.
- Play-mode snapshots and undo/redo operate on serialized/registered ECS state plus an explicit GPU-state snapshot when live GPU simulation must be restored.

## Rules for future work

1. Do not expose GPU buffer indexes, descriptor indexes, or batch positions as public identities.
2. Do not let rendering write transforms back into gameplay ECS. Physics may update ECS only through the typed CPU/GPU synchronization bridge.
3. Do not load or decode files inside a render pass.
4. Do not block the normal frame path with `GpuFuture::wait`; retain fences/futures until frame-context reuse.
5. Do not silently drop work when a buffer or grid reaches capacity.
6. Keep fallible window, asset, and renderer operations as structured `Result` APIs.
7. Add a focused test whenever a new cache, identity mapping, or ownership boundary is introduced.
8. Do not introduce wall-clock time, frame-rate dependence, or unseeded randomness into simulation code.
9. Do not accumulate simulation state through order-dependent atomic operations.
10. Do not replicate derived data across the network. Replicate the commands that produce it.
11. Do not make a client authoritative over any simulation state.
12. Do not author navigation data by hand. Derive it from terrain collision.
13. Do not let presentation systems write simulation state, including through smoothing or correction.
14. Give every system with an unbounded per-tick cost a budget, a reported measurement, and a defined behaviour at the cap.

## Near-term architecture sequence

1. Replace the bootstrap egui renderer with an engine-owned painter while preserving the current editor API.
2. Add explicit two/three-frame contexts and per-frame transient resources.
3. Move scene rendering to an offscreen viewport image registered with egui.
4. Upload only extraction dirty ranges into growable instance buffers.
5. Add typed editor widgets and animation assets over the scene component registry.
6. Add stable physics IDs, GPU event readback, CPU command upload, and selective state synchronization before expanding the self-written solvers.

## Sundering architecture sequence

This sequence begins once the hybrid physics bridge above is complete. It corresponds to roadmap Milestones 8-15.

1. Extract `rusting-math` and move both the CPU solver and every physics shader onto it. Add the per-tick world-state hash and headless mode.
2. Prove or disprove cross-vendor determinism. Record the result in this document, because it decides the networking model for everything that follows.
3. Add replay recording and playback on top of the hash, then use replays as the primary debugging tool for everything after this point.
4. Add `rusting-net` with a headless server, treating clients as pure renderers until determinism is proven.
5. Add `rusting-terrain` as canonical simulation state, with collision and render meshes derived from one volume.
6. Add `rusting-nav` deriving navigation from terrain collision, with a hard per-tick budget.
7. Add `rusting-audio` and the presentation systems, verifying that disabling them does not alter replay hashes.
8. Add `rusting-gameplay` with vision, and wire vision into network interest management through the trait boundary.

## Recorded decisions

Decisions that later work must not silently reverse. Add to this list rather than changing a boundary above.

- *(pending)* Simulation number format: fixed-point or constrained IEEE-754. Decided by Milestone 8.
- *(pending)* Networking model: lockstep with rollback, or server-authoritative without. Decided by the Milestone 8 determinism result.
- *(pending)* Terrain chunk size and volume representation. Decided by Milestone 10.
