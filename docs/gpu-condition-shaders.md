# GPU condition shaders and custom solvers

GPU bodies (`physics_body.simulation = "Gpu"`) run in a compute shader. Two
hooks let a game add its own GLSL to that shader:

| Hook | Runs | Use it for |
| --- | --- | --- |
| `GpuConditionShader` (game code) | after the built-in step, on every GPU body, every fixed tick | force fields, swarm steering, region tracking, custom events. Built-in collision still runs. |
| `physics_body.solver = "Custom"` + `custom_shader` (scene) | instead of the built-in integration, only for bodies that select the file | a whole different motion model. Those bodies skip gravity, collider and body contacts. |

Both are compiled at runtime with `glslc` (Vulkan SDK or shaderc; set
`RUSTING_GLSLC` to another path). A shader that fails to compile is skipped,
its error is printed once, and `SceneRenderer::condition_shader_errors`
keeps it. Compiled shaders are cached by source text, so keep `glsl` the
same and change `params` instead.

## Condition shaders from game code

```rust
use rusting_engine::prelude::*;

const FAN: &str = r#"
void condition(inout PhysicsState body) {
    vec4 fan = condition_params.values[0]; // xyz = position, w = strength
    if (fan.w <= 0.0 || body.properties.z != 1.0) return; // dynamic only
    vec3 to_body = body.model[3].xyz - fan.xyz;
    float distance = length(to_body);
    if (distance < 4.0) {
        body.velocity.xyz += normalize(to_body) * fan.w * pc.dt / max(distance, 0.5);
    }
    if (body.model[3].y < -2.0 && body.custom_values.y == 0.0) {
        body.custom_values.y = 1.0;
        emit_event(body, EVENTS[0], 1u, vec4(body.model[3].xyz, 0.0));
    }
}
"#;

fn update(scene: &mut GameScene<'_>) {
    let strength = if scene.counter_value("fan_on") > 0 { 30.0 } else { 0.0 };
    scene.set_gpu_condition_shaders(vec![GpuConditionShader {
        events: vec!["fell_out".into()],
        glsl: FAN.into(),
        params: vec![[0.0, 1.0, 0.0, strength]],
        events_per_body: 1,
    }]);
    for event in scene.gpu_events("fell_out") {
        // event.payload is the vec4 passed to emit_event.
    }
}
```

- `glsl` must define `void condition(inout PhysicsState body)`. Whatever it
  writes to `body` is stored, so it can steer position and velocity.
- `events[i]` is registered by name; GLSL sees its id as `EVENTS[i]`. Read
  them in Rust with `scene.gpu_events("name")`.
- `params` reach GLSL as `condition_params.values[i]` (`vec4`). Update them
  every frame; no recompile. Reading past
  `condition_params.values.length()` is undefined.
- `events_per_body` is how many events one body may emit per tick from this
  shader (0 counts as 1). The frame's event buffer holds that many per body,
  tick and shader, up to 262144 events per frame in all. Events past the end
  are dropped, counted, printed as `GPU physics event buffer overflowed by at
  least N events` and sent as `GpuPhysicsEventsLost`. When one body has two
  things to report, raise `events_per_body` or pack both into one payload.
- Shaders run in list order, after the built-in step, once per fixed tick.

## The ABI

The full source is `src/shaders/physics_abi.glsl`; it is included before
your code. Pin it with `#if RUSTING_PHYSICS_ABI_VERSION != 1` / `#error` /
`#endif`.

```glsl
struct PhysicsState {
    mat4 model;            // model[3].xyz is the position
    vec4 velocity;         // xyz m/s; w = 1 when it touched something last step
    vec4 angular_velocity; // xyz rad/s; w = ticks at rest (built-in sleep), read only
    vec4 properties;       // x mass, y gravity scale, z kind (0 fixed, 1 dynamic, 2 kinematic), w solver id
    vec4 custom_values;    // x = solver mode, do not change; y, z, w are yours
    uvec4 metadata;        // x PhysicsId slot, y generation, z/w rule range
};

// Push constants, read as pc.<name>:
// dt, elapsed (seconds at the end of this tick), body_count, event_capacity,
// tick_low, tick_high, gravity_x/y/z, command_count, collider_count,
// grid_cell_size, command_first.

void emit_event(PhysicsState body, uint event_id, uint payload_kind, vec4 payload);
```

`custom_values.x` selects the built-in solver mode (0 Full, 1 Simplified,
2 NoCollision, 3 Custom, 4 Space). Leave it alone, and note that
`GpuBodyCommand::SetCustomValues` overwrites all four values, x included.
`custom_values.y`, `.z` and `.w` start at 0 and keep what you write, so they
are per-body memory across ticks (a region index, a "reported" flag).

Sleep: a dynamic body that touches something and stays slower than
0.05 m/s and 0.05 rad/s for 20 ticks in a row sleeps. The GPU solver
does not rotate bodies. While a body touches something, its spin shrinks
by `5 * dt` of itself each tick (8% at 60 Hz), so a spun body sleeps about
a second after it lands. Its velocity is zeroed and it keeps
its position until a contact or collider pushes it, a command reaches it,
or it loses all contact. `angular_velocity.w` counts those ticks (20 means
asleep); `GpuCondition::sleeping()` is true from then on. A condition shader
that gives a sleeping body speed wakes it on the next tick; one that only
moves its position should also set `angular_velocity.w = 0.0`.

`payload_kind` reaches Rust unchanged in `GpuPhysicsEvent::payload_kind`;
the built-in rules use 1 position, 2 velocity, 3 angular velocity, 4 contact,
5 custom values.

Determinism: write only to `body` (one body per invocation) and emit events.
Event order inside a tick is not fixed on the GPU; the CPU sorts delivered
events by tick, `PhysicsId` and event id. Take randomness from a hash of
`body.metadata.x` and `pc.tick_low`, never from time or a global counter.

## Custom solvers

Set `physics_body.solver` to `"Custom"` and `custom_shader` to a
project-relative `.glsl` file that defines
`void solve(inout PhysicsState body)`. It runs only for bodies that chose
that file (`SOLVER_ID` is defined to the file's id) and replaces gravity,
integration, collider collision and body contacts for them. Commands
(teleport, impulse) and watch rules still apply. `condition_params` is empty
for a solver file.
