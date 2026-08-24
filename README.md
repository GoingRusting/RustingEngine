# RustingEngine

RustingEngine is a Vulkan 3D engine focused on one problem: running very large
physics-heavy scenes without reducing the game to a slideshow.

The engine uses hybrid physics calculation. Gameplay-critical simulation can
stay on the CPU, while high-volume physics workloads can run in parallel on the
GPU through Vulkan compute shaders. Rendering uses instanced batches, indirect
drawing, and optional GPU frustum culling so thousands of similar objects do
not become thousands of expensive CPU draw calls.

The target workload is a scene such as 10,000 cubes stacked and colliding while
still rendering at extremely high frame rates. Specialized stress scenes can
reach thousands of frames per second on suitable hardware, where a conventional
CPU-bound engine may fall into single-digit FPS. Exact performance depends on
the selected physics solver, scene complexity, and GPU.

RustingEngine also includes a visual editor, a serializable scene format, and a
separate cooked runtime. A scene can be assembled in the editor and then run
without shipping the editor or its GUI dependencies.

## Main features

- Vulkan rendering and compute with low-level control over the GPU
- CPU and GPU physics paths selectable for different workloads
- Instanced and indirect rendering for large object counts
- Optional GPU frustum culling
- Visual scene editor with save and load support
- Cooked gameplay scripts with scene lifecycle functions
- Cooked game runtime without editor code

## Running

Install Rust and make sure that a Vulkan-compatible graphics driver is available.

Run the editor GUI:

```bash
./scripts/run_editor.sh
```

Run the simulation without the editor:

```bash
cargo run --bin user_main
```

The editor can also be started directly through Cargo:

```bash
cargo run --bin editor
```

The editor has three workspaces. **Scene** uses an editor-only camera for
authoring, **Game** shows the active game camera, and **Code** opens and saves
project gameplay scripts, Rust, or GLSL files. Play mode snapshots the authored scene; pressing
Stop restores it instead of keeping runtime changes.

Select an entity and use the **Physics** inspector to choose `Static`,
`Gameplay (CPU)`, or `GPU Dynamic`. GPU bodies can use the full, simplified,
no-collision, or custom compute profile. Static bodies are not included in a
per-frame compute dispatch. A custom shader can be opened directly in the Code
workspace and saved inside the game project.

Game files are kept outside the engine source:

```text
testGame/
  project.json
  scenes/main.rscene
  scripts/main.rscript
  build/main.rscene.bin
```

Gameplay scripts can bind scene objects and update their ECS transforms:

```text
let orange = scene.get_object("Orange Cube");

onSceneUpdate() {
    orange.x = 5;
    orange.rotation.y += 1.5 * delta;
}
```

The cooker validates scripts and embeds compiled instructions in the cooked
scene. The release runtime does not need `.rscript` source files or the editor.

## Building an editor scene

1. Run the editor, change the scene and click **Save Scene**.
2. Cook the human-readable scene into compact runtime data:

```bash
./scripts/cook_scene.sh
```

3. Run the cooked scene without egui or editor code:

```bash
./scripts/run_game.sh
```

The source scene is saved to `testGame/scenes/main.rscene`. The default cooked
output is `testGame/build/main.rscene.bin`.
