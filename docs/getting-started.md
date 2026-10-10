# Getting started

This page takes you from a fresh checkout to a running game. It takes about
15 minutes, most of it the first compile.

## What you need

- **Rust**, stable toolchain, from <https://rustup.rs>.
- **A Vulkan driver.** Any current NVIDIA, AMD, or Intel driver on Linux or
  Windows works. Check with `vulkaninfo --summary`.
- **CMake, Python 3, and a C++ compiler**, the first time you build. The
  shader compiler (shaderc) is built from source unless a system copy is
  installed (`shaderc` on Arch, `libshaderc-dev` on Debian/Ubuntu).

Linux and Windows are the supported platforms.

## Check your machine

From the engine checkout:

```bash
cargo run --bin rusting -- doctor
```

`doctor` reports the engine version, every Vulkan device it finds, and
whether `cargo`, `rustc`, and `glslc` are on the path. A missing Vulkan device does not stop the other commands:
you can still create, build, test, and export projects headless.

## Open the editor

```bash
./scripts/run_editor.sh
```

The Project Manager opens first.

1. Press `New Project...`.
2. Choose a parent folder, type a name such as `Hello Cube`, keep the
   `Empty 3D` template, and press `Create Project`. The editor makes a new folder and never overwrites an
   existing one.
3. The new project opens with a blue cube and a camera.

Press **Play** in the top bar. The editor saves the scene, cooks it, compiles
the game's Rust code, and starts the game in its own window. The first build
takes a few minutes; later builds take seconds. Choose `Debug` beside Play
for fast builds while you work, and `Release` to measure performance.

Scene View controls follow Blender:

| Action | Input |
| --- | --- |
| Select, add to selection | Click, `Shift`-click |
| Move, rotate, scale the selection | `G`, `R`, `S`, then `X`, `Y`, or `Z` to lock an axis; click to confirm, right-click to cancel |
| Focus the selection | `F` |
| Fly | Hold the right mouse button, then `W` `A` `S` `D`, `Space` up, `Ctrl` down, `Shift` faster |
| Orbit, pan, zoom | Middle mouse button, `Shift` + middle mouse button, wheel |
| Undo, redo | `Ctrl+Z`, `Ctrl+Shift+Z` |
| Save scene | `Ctrl+S` |
| Rename, delete | `F2`, `Delete` |

To try the scene without compiling, press **Preview** instead: it runs
inside the editor, and `Stop Preview` puts the scene back as it was.

See the [editor guide](../editor_gui.md) for areas, the hierarchy, the
inspector, asset import, and changing these keys.

## Or use the command line

Everything the editor does to a project, the `rusting` CLI does too. This is
the same project made without the editor:

```bash
cargo build --bin rusting
./target/debug/rusting new ~/Code/games "Hello Cube"
./target/debug/rusting run ~/Code/games/"Hello Cube"
```

`run` opens the game window. Add `--ticks 120` to run 120 simulation ticks
with no window and exit, which works on machines with no display.

## What is in a project

```text
Hello Cube/
  Cargo.toml         a normal Cargo package that depends on rusting_engine
  project.json       name, main scene, binary name, determinism level
  src/main.rs        your game code
  scenes/main.rscene the scene, as readable JSON
  assets/            imported textures, meshes, sounds
  shaders/           custom GPU physics shaders
  build/             cooked scene data (generated; do not edit)
```

A game is an ordinary Rust program. You can add any crate to its
`Cargo.toml` and debug it with your usual tools. The game loads the cooked
scene from `build/`, so cook first (`rusting cook`, or Play once); after
that, plain `cargo run` in the project folder works too. After you edit the scene,
cook again: `cargo run` and the built binary print a warning when the
cooked scene is older than the scene file. `rusting run` and `rusting test`
cook for you.

## Next

Continue with [Tutorial 1: Hello cube](tutorials/01-hello-cube.md), or read
[Core concepts](concepts.md) first.
