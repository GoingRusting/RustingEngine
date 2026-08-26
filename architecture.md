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

## Near-term architecture sequence

1. Replace the bootstrap egui renderer with an engine-owned painter while preserving the current editor API.
2. Add explicit two/three-frame contexts and per-frame transient resources.
3. Move scene rendering to an offscreen viewport image registered with egui.
4. Upload only extraction dirty ranges into growable instance buffers.
5. Add typed editor widgets and animation assets over the scene component registry.
6. Add stable physics IDs, GPU event readback, CPU command upload, and selective state synchronization before expanding the self-written solvers.
