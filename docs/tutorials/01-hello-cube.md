# Tutorial 1: Hello cube

You will make the default cube spin and bob up and down, then check the
motion with an automated test. You need a project made as in
[Getting started](../getting-started.md); this page calls it `Hello Cube`.

## 1. Look at the starting code

Open `src/main.rs` (in the editor, switch an area to `Code Editor`):

```rust
use rusting_engine::prelude::*;

fn update(_scene: &mut GameScene<'_>, _time: &FrameTime) {
    // Add game behaviour here.
}

rusting_game!(update);
```

`rusting_game!` writes `main` for you. It loads the cooked scene and calls
`update` once per frame with:

- `scene`: access to the objects in the scene, by name.
- `time`: the frame's `FrameTime` (`delta_seconds()`, `elapsed_seconds()`,
  `fixed_tick`, ...).

## 2. Spin and bob the cube

Replace the file with:

```rust
use rusting_engine::prelude::*;

fn update(scene: &mut GameScene<'_>, time: &FrameTime) {
    let t = time.elapsed_seconds();
    scene
        .object("Cube")
        .rotate_y(1.5 * time.delta_seconds())
        .set_position([0.0, (t * 2.0).sin(), 0.0]);
}

rusting_game!(update);
```

- `scene.object("Cube")` finds the object named `Cube`. It panics with a
  clear message if there is none; use `scene.try_object(name)` when the
  object may be missing.
- `rotate_y` adds an angle in radians. Multiplying by `delta_seconds()` makes
  the speed 1.5 radians per second on any frame rate.
- `set_position` sets the position directly. Other methods: `move_by`,
  `move_x`/`y`/`z`, `set_rotation`, `rotate_by`, `rotate_x`/`z`, `set_scale`,
  and `position()` to read it. They chain.

## 3. Run it

In the editor, press **Play**. From a terminal:

```bash
rusting run "Hello Cube"
```

(Run the CLI from the engine checkout as `./target/debug/rusting`, or put it
on your `PATH`.)

The cube spins and moves up and down once every π seconds.

## 4. Add objects in the editor

1. Press the add button (`+`, "Add object") at the top of the Hierarchy,
   open `Meshes`, and choose `Sphere`.
2. Rename it to `Moon` (double-click the row, or `F2`).
3. With the Moon selected, hover the Scene View, press `G` then `X`, and
   move the mouse to slide it along X. Left-click to confirm; right-click or
   `Escape` cancels.
4. Save the scene with `Ctrl+S`.

Now add one line to `update` to make it orbit:

```rust
    scene.object("Moon").set_position([3.0 * t.cos(), 0.0, 3.0 * t.sin()]);
```

Every edit in the editor goes through Undo (`Ctrl+Z`).

## 5. Spawn objects from code

For objects made by code, spawn them once with `scene.once`, which runs its
closure the first time only:

```rust
    scene.once("make_ring", |scene| {
        let cube = CubeSpawn::new();
        for index in 0..12 {
            let angle = index as f32 / 12.0 * std::f32::consts::TAU;
            scene.spawn_cube(
                format!("Ring {index}"),
                Transform::new([5.0 * angle.cos(), 0.0, 5.0 * angle.sin()]),
                &cube,
            );
        }
    });
```

Spawned names must be unique, like any object name. `spawn_sphere`,
`create_material`, `spawn_cube_with_material`, and
`set_background_color` work the same way.

## 6. Test it without a window

Save this as `bob.scenario.json` in the project folder:

```json
{
  "name": "the cube bobs and spins",
  "seed": 1,
  "ticks": 30,
  "steps": [
    {"tick": 30, "expect": {"entity": "Cube", "path": "/transform/position/1", "greater_than": 0.5}},
    {"tick": 30, "expect": {"entity": "Cube", "path": "/transform/rotation/1", "greater_than": 0.5}}
  ]
}
```

```bash
rusting test "Hello Cube" "Hello Cube/bob.scenario.json"
```

```text
Scenario "the cube bobs and spins": passed after 30 ticks, seed 1
```

After 30 ticks (half a second) the cube is at `sin(1.0) ≈ 0.84` and has
turned 0.75 radians. A scenario runs one tick per update, so the numbers are
the same on every machine. Break the code (remove `rotate_y`) and the test
fails with the tick, the step, and the value it saw.

## What you learned

- `rusting_game!` and the per-frame `update` function.
- Finding objects by name and changing their transforms.
- Spawning objects once from code.
- Scenario tests.

The short API has no input access. For keyboard control, ECS systems, and
your own components, see [Tutorial 3](03-gameplay-plugin.md). Next,
[Tutorial 2](02-coin-run-cli.md) builds a whole game from built-in pieces.
