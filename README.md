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

The source scene is saved to `assets/scenes/main.rscene`. The default cooked
output is `assets/scenes/main.rscene.bin`.
