# Agent guide

This is a Rusting game project. You can build the whole game from the command
line: scenes are JSON files, game code is plain Rust, and every `rusting`
command takes `--json` for output a program can read.

## Files

- `project.json`: project manifest; `main_scene` is the scene the game loads.
- `scenes/main.rscene`: the scene, as JSON. Edit it with `rusting scene patch`
  rather than by hand, so every change is validated before it is written.
- `src/main.rs`: game code. `update` runs once per frame.
- `assets/`: imported models, textures and sounds.
- `tests/`: scenario files for `rusting test` (create the folder when needed).

## Workflow

1. `rusting schema --json` lists every scene section and component with its
   default, fields, units and an example. Read it before writing scene JSON.
2. `rusting scene query scenes/main.rscene --json` lists the entities with
   their IDs. Patches address entities by ID or by unique name.
3. `rusting scene patch scenes/main.rscene patch.json [--dry-run]` applies a
   batch of operations: `create`, `set`, `remove`, `reparent`, `duplicate`,
   `delete` and `set_scene` (scene-level fields). Built-in sections and
   components may be partial; missing fields take their defaults.
4. `rusting check` builds the game code and validates the scene. Project
   commands default to the current folder.
5. `rusting test` runs every scenario in `tests/`, and
   `rusting test tests/name.json` runs one. A scenario presses inputs on given
   ticks, sets values, and checks values; `"exists": false` checks that an
   object is gone, and `tolerance` applies to every number in a position or
   other array. Use `set` to place the player or
   fill a counter before a check, `within` for "eventually by tick N" and
   `until` for "holds every tick through N". Both take an absolute tick,
   not a count. A `pointer` step puts the mouse cursor at a point of the
   view, given as fractions: `{"tick": 1, "pointer": [0.5, 0.5]}` is the
   center.
6. `rusting capture scenes/main.rscene shot.png --tick 60` renders a frame so
   you can look at the result. Scenario files can capture frames too.
7. `rusting run` opens the game window. `--ticks N` runs it headless and
   saves the end state to `build/final.rscene`; inspect it with
   `rusting scene query build/final.rscene --json`. Game output (`eprintln!`)
   is in the `--json` result under `game.stderr`.

## Scene basics

- +Y is up and -Z is forward. Units are metres, radians and seconds.
- A mesh is scaled by `transform.scale`, and so is its collider: a unit box
  collider on an entity scaled `[6, 1, 6]` is a 6 x 1 x 6 slab.
- Physics needs `physics_body`, `rigid_body` (`Fixed`, `Dynamic` or
  `Kinematic`) and `collider`. A `sensor` collider only reports touches.
- Gameplay components need no code: `rusting.counter`, `rusting.pickup`,
  `rusting.hud` (text with `{counter}` placeholders), `rusting.tween`,
  `rusting.sound_cue`, `rusting.burst_emitter`, `rusting.player_controller`,
  `rusting.joint`. Components with `requires` wait for that counter to reach
  its target.
- Quads are one-sided: a sprite turned more than 90 degrees about Y
  disappears. Animate 2D sprites with a `Scale` tween instead.
- Player and platformer controllers ride moving platforms (kinematic bodies
  moved by `rusting.tween` or game code).
- The player controller is kinematic: moving bodies do not push it. Handle
  hazards in game code with `touching`, as below.
- Input actions for the player controller: `player.forward`, `player.back`,
  `player.left`, `player.right`, `player.jump`, `player.sprint`.
- Add your own actions with `rusting.input_action`, for example
  `{"action": "fire", "inputs": ["MouseLeft", "KeyF"]}`. Key names are winit
  `KeyCode` names. Scenarios press the action by name.

## Game code

```rust
use rusting_engine::prelude::*;

fn update(scene: &mut GameScene<'_>, time: &FrameTime) {
    // Objects are found by their scene name.
    let hit = scene.touching("Player").iter().any(|name| name == "Spikes");
    let mut player = scene.object("Player");
    if hit || player.position()[1] < -5.0 {
        player.set_position([0.0, 1.0, 0.0]);
    }
}

rusting_game!(update);
```

`GameScene` finds objects by name (`object`, `try_object`), moves them
(`set_position`, `move_by`, `set_rotation`, `set_scale`; `position`,
`rotation` and `scale` read them back), reports contacts (`touching`), reads
and changes counters (`counter`, or the shorthands `counter_value`,
`set_counter`, `add_to_counter` and `counter_complete`), reads input actions
(`pressed` for this frame, `held`), casts rays (`raycast`, and `aim` along the
active camera; `camera_ray` gives that camera's position and forward
direction, `pointer_ray` the ray through the mouse cursor), launches bodies
(`set_body_kind`, `set_linear_velocity`, `set_angular_velocity`; each wakes a
sleeping body, and a body made `Kinematic` or `Fixed` stops) and reads their
velocity (`linear_velocity`, `angular_velocity`), stops a body completely
(`reset_body`), turns a player controller (`set_look`), removes objects
(`despawn`), shows and
hides them and their HUD text (`set_visible`), recolors one object without
touching others that share its material (`color`, `set_color`,
`set_emissive`), reloads the starting scene for a new round (`restart`;
physics after it repeats the first run exactly), reads an object's starting
transform, color and body kind (`initial`), saves and puts back the whole
scene mid-game (`snapshot`, `restore`), hashes the state of one class to
compare rounds (`state_hash`), reads
and writes `rusting.tile_map` cells under a world position (`tile`,
`set_tile`), fires sound cues and burst emitters (`trigger`), spawns shapes
(`spawn_cube`, `spawn_sphere`), copies a hidden template object with its
children (`spawn_copy`; a copied child is named `"<copy>/<child>"`), lists the
objects in a class (`in_class`), runs setup once per round (`once`; `restart`
runs it again), draws random numbers that repeat for a scenario's seed
(`random`), switches to another scene file such as a next level
(`load_scene`, with a path relative to the project folder) and draws UI (`ui`,
an egui context). `world()` gives the ECS world for anything else.
`cargo doc --open` documents the full API.
