# Core concepts

A short tour of the ideas the tutorials rely on.

## Projects

A project is a Cargo package plus a `project.json` manifest. The manifest
names the main scene, the cooked scene the game loads, the binary name, and
the determinism level. The editor and the `rusting` CLI read and write the
same files, so you can switch between them at any time.

## Scenes

A scene (`scenes/main.rscene`) is JSON: a list of entities, each with a
persistent ID (a UUID), a name, an optional parent, and components. Built-in
components have their own fields (`transform`, `mesh_renderer`, `camera`,
`rigid_body`, `collider`, lights, ...). Everything else, built-in gameplay
components such as `rusting.pickup` and your own, lives under `components`
by registered name.

- Entity names must be unique in a scene. Game code finds objects by name,
  tools address them by ID.
- `rusting scene query` prints entities, `rusting scene patch` changes them
  as one atomic batch, and the editor edits them with undo.
- `rusting schema` lists every component and field, with defaults, units,
  and valid ranges.
- Registered components are described with `reflect!`. The description
  drives saving, the Inspector's widgets, the schema, and field paths
  (`set_registered_component_field`). A saved field the type does not have
  fails the load; a renamed or removed field needs a `FieldMigration`.

**Cooking** turns the JSON scene into a compact binary
(`build/main.rscene.bin`) that the game loads. Play, `rusting run`, `test`,
and `export` cook for you.

## Game code

A game's `main.rs` picks one of two entry points:

- **`rusting_game!(update)`**: one function called every frame with a
  `GameScene`. Find objects by name, move and rotate them, spawn cubes and
  spheres, turn on GPU physics for a class, and read GPU events. Good for
  small games and procedural setup. See [Tutorial 1](tutorials/01-hello-cube.md).
- **A `Plugin` passed to `run_project`**: normal ECS systems (the engine uses
  `bevy_ecs`) with queries, resources, input actions, and your own
  components saved in scenes. See [Tutorial 3](tutorials/03-gameplay-plugin.md).

Both run the same engine: the same scene loader, physics, renderer, and
built-in gameplay systems.

## Frames and fixed ticks

The engine separates **frames** from **fixed ticks**:

- A frame is one rendered image. Frame length varies with the machine.
- A fixed tick is one simulation step of exactly `fixed_delta` (1/60 s by
  default). Each frame runs as many ticks as real time requires, up to 8.

Systems run in stages: `Startup`, `FixedUpdate` (once per tick), `Update`
(once per frame), `PostUpdate`, and `RenderExtract`. Physics and anything
that must replay identically runs in `FixedUpdate` and reads
`FrameTime::fixed_delta`. Visual effects and per-frame input handling run in
`Update` and read `FrameTime::delta`.

`FrameTime` also holds `elapsed` game time, `fixed_tick` (ticks completed),
and `frame`. `TimeControl` pauses, steps, and scales time.

## Physics: CPU and GPU

Every body has a `physics_body.simulation` class:

| Class | Where it runs | Use it for |
| --- | --- | --- |
| `Cpu` (default) | CPU solver | the player, doors, anything gameplay reads every frame |
| `Gpu` | Vulkan compute | debris, swarms, crowds, thousands of bodies |
| `Static` | nowhere, never moves | floors and walls; both solvers collide with it |
| `None` | nowhere | visual-only objects |

CPU bodies are ordinary ECS components: read and write them directly. GPU
bodies stay on the GPU. The CPU does not download their transforms every
frame. Instead you attach **rules** (`GpuPhysicsRule`: "position Y below
-20", "colliding", "timer elapsed") to an object or a whole **class**, and the
GPU sends back an event only when a rule matches. See
[Tutorial 4](tutorials/04-gpu-cube-rain.md).

`rigid_body.kind` is `Dynamic` (moved by forces), `Kinematic` (moved by your
code or a controller), or `Fixed`.

## Input

Game code reads **actions**, not keys. The `ActionMap` resource binds action
names to keys and mouse buttons; `RuntimeInput` holds this frame's raw
state. The engine binds `player.forward`, `player.back`, `player.left`,
`player.right` (WASD and arrows), `player.jump` (Space), and
`player.sprint` (Shift). Scenario tests press and release actions by name,
so a game that uses actions is testable without a keyboard.

## Tests you can run without a screen

- `rusting run <project> --ticks N` runs the game headless for N ticks.
- `rusting test <project> <scenario.json>` presses actions at fixed ticks and
  checks scene values, collision events, and screenshots.
- `rusting determinism <project>` checks that debug, release, and one-CPU
  builds produce the same simulation, tick by tick.

GPU bodies do not move in headless runs, because no renderer runs their
compute shaders. A headless run warns when the loaded scene has GPU bodies;
bodies that game code moves to the GPU later are not counted.

## Determinism

`determinism` in `project.json` is `Off`, `Local` (reproduces on one
machine), or `CrossPlatform`. The CPU simulation is deterministic today, and
recorded replays reproduce exactly. See [Determinism](determinism.md).
