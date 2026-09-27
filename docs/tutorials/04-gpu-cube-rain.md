# Tutorial 4: GPU cube rain

You will drop 2,000 cubes simulated on the GPU, get an event back from the
GPU when each one falls past a line, and show a counter on screen. This is
the pattern for debris, swarms, and crowds: the GPU owns the bodies, and the
CPU hears only about the moments it cares about.

You need a project made as in [Getting started](../getting-started.md). This
page calls it `Cube Rain`. It needs a Vulkan device: GPU bodies do not move
in headless runs.

## 1. The code

Replace `src/main.rs`:

```rust
use std::sync::atomic::{AtomicUsize, Ordering};

use rusting_engine::prelude::*;

/// Drops that fell past Y = -20 so far.
static FALLEN: AtomicUsize = AtomicUsize::new(0);

fn update(scene: &mut GameScene<'_>, time: &FrameTime) {
    scene.once("spawn_rain", |scene| {
        let drop = CubeSpawn::new().class("rain");
        for index in 0..2_000 {
            let x = (index % 50) as f32 - 25.0;
            let z = (index / 50) as f32 - 40.0;
            let y = 10.0 + (index % 7) as f32 * 2.0;
            scene.spawn_cube(
                format!("Drop {index}"),
                Transform::new([x, y, z]),
                &drop,
            );
        }
        let count =
            scene.apply_gpu_physics_to_class("rain", &GpuBodySettings::default());
        println!("{count} drops on the GPU");

        scene.watch_gpu_class(
            "rain",
            GpuPhysicsRule::new(
                "drop_fell",
                GpuCondition::position_y().less_than(-20.0),
            )
            .mode(GpuEventMode::OnEnter)
            .payload(GpuEventPayload::Position),
        );
    });

    let events = scene.gpu_events("drop_fell");
    let fallen = FALLEN.fetch_add(events.len(), Ordering::Relaxed) + events.len();
    if let Some(first) = events.first() {
        println!("{:?} fell past Y = -20 at {:?}", first.physics_id, first.payload);
    }

    rusting_engine::egui::Window::new("Cube rain").show(&scene.ui(), |ui| {
        ui.label(format!("{:.0} FPS", 1.0 / time.delta_seconds().max(1e-6)));
        ui.label(format!("{fallen} of 2000 drops fell past Y = -20"));
    });
}

rusting_game!(update);
```

Run it with **Play** in the editor or `rusting run "Cube Rain"`. The cubes
fall, and the counter climbs as they pass Y = -20.

## 2. What each part does

**Classes.** `CubeSpawn::new().class("rain")` tags every spawned cube with
the class `rain`. A class names a group of objects so one call can act on
all of them.

**Moving bodies to the GPU.** `apply_gpu_physics_to_class` gives every
object in the class a GPU body and returns how many it changed. From then
on, the Vulkan compute solver moves them, not the CPU. `GpuBodySettings`
chooses how:

| Field | Default | Meaning |
| --- | --- | --- |
| `solver` | `Full` | `Full`, `Simplified`, `NoCollision`, `Space` (mutual gravity), or `Custom` |
| `custom_shader` | `None` | project-relative compute shader when `solver` is `Custom` |
| `rigid_body` | `RigidBody::default()` | mass, starting velocity, gravity |
| `collider` | `Collider::default()` | shape, friction, restitution |
| `collision_layers` | all | which groups collide |

You can set the same thing in a scene: set an object's
`physics_body.simulation` to `Gpu`. See [Core concepts](../concepts.md#physics-cpu-and-gpu).

**Rules instead of readback.** The CPU never downloads the 2,000
transforms. `watch_gpu_class` attaches a rule to every body in the class.
The GPU tests the rule each tick and writes an event only when it matches:

- `GpuCondition` picks the test: `position_y()`, `velocity_y()`,
  `colliding()`, `sleeping()`, `timer_elapsed(seconds)`. Compare values with
  `less_than`, `greater_than`, or `inside`, and combine conditions with
  `and`, `or`, and `inverted`.
- `GpuEventMode::OnEnter` fires once when the condition becomes true, not
  on every tick while it stays true. Add `.cooldown(seconds)` to limit how
  often a rule can fire again.
- `GpuEventPayload` chooses the four numbers sent with the event:
  `Position`, `Velocity`, `AngularVelocity`, `Contact`, or `Custom`.

**Reading events.** `scene.gpu_events("drop_fell")` returns this frame's
events for that rule. Each `GpuPhysicsEvent` has the `entity`, its
`physics_id`, the `tick` it happened on, and the `payload` (`[x, y, z, 0]`
for `Position`). Events reach the CPU one to three frames after the tick
that caused them, because the CPU does not wait for the GPU.

**UI.** `scene.ui()` returns the frame's
[egui](https://docs.rs/egui) context; `rusting_engine::egui` is the matching
egui version. `update` is a plain function with no state of its own, so the
running total lives in a `static`. For real game state, use a plugin and a
resource, as in [Tutorial 3](03-gameplay-plugin.md).

## 3. Watch one object

To follow one body instead of a class, use `watch_gpu_object`:

```rust
scene.watch_gpu_object(
    "Drop 0",
    GpuPhysicsRule::new("first_drop_landed", GpuCondition::colliding())
        .mode(GpuEventMode::OnEnter),
);
```

Add a floor for it to land on: a cube scaled to `[60, 1, 60]` at
`[0, -2, -20]` whose `physics_body.simulation` is `Static`. Static bodies
collide with both the CPU and the GPU solver. With the floor in place the
drops stop above Y = -20, so `drop_fell` no longer fires.

`scene.set_linear_velocity(name, [x, y, z])` sets a GPU body's velocity
from code, for example to launch a drop upward.

## 4. Testing GPU games

Headless runs (`rusting run --ticks`, `rusting test`) have no renderer, so
GPU bodies stay where they started and GPU rules never fire. Test the CPU
side headless, such as spawning and counting the class:

```bash
rusting run "Cube Rain" --ticks 30
```

The run exits with code 0 once the spawn code has worked. Check GPU
behaviour by playing the game, or with the engine's GPU tests
(`cargo test --features gpu-tests`), which run the solvers on a real or
software (lavapipe) Vulkan device.

## Going further

- `src/examples/hybrid_10k.rs` in the engine repository runs 10,000 GPU
  cubes beside CPU bodies. Cook `testGame` first
  (`rusting cook testGame`), then start it with
  `cargo run --release --example hybrid_10k`.
- `testGame/src/main.rs` uses the `Space` solver for an orbiting cloud.
- [Determinism](../determinism.md) explains why CPU code that reacts to GPU
  events does not replay exactly yet.
