---
name: rusting-game
description: Build, test and polish a game on RustingEngine from the command line. Use when asked to create or change a Rusting game project, its scenes (.rscene), its game code (rusting_game!, GameScene) or its scenario tests.
---

# Making games with RustingEngine

A Rusting game is a folder with `project.json`, JSON scenes, a small Rust
`update` function and JSON scenario tests. You can build a whole game
without the editor: every step below is a `rusting` command, and every
command takes `--json`.

Twelve sample games in `samples/` were built this way. Read the closest one
before starting: `sky_hop` (platformer), `ember_arena` and `night_vault`
(third person), `target_range` and `tower_topple` (first person, shooting),
`putt_course` (physics ball, mouse aim), `core_defense` (top-down, mouse),
`lantern_grid` (click puzzle), `crate_keeper` (grid puzzle), `snake_trail`
and `brick_bounce` (2D).

## Loop

0. Put the CLI on `PATH` once: `cargo install --path <engine checkout>
   --bin rusting`, or call `<engine checkout>/target/debug/rusting`.
1. `rusting new <parent> <name> --template <t>`. Templates: `first-person`,
   `third-person`, `2d` (platformer), `sandbox` (physics pile), `3d`,
   `starter`. The template gives a floor, a player with a camera, lights and
   an `AGENTS.md` with the project's rules. Read that `AGENTS.md`. Its
   `Cargo.toml` builds the engine optimized even in debug builds; keep
   that profile, or a busy physics scene drops to a few frames a second.
2. `rusting schema --json` lists every scene section and component with
   field names, defaults and units. Check field names there before writing
   scene JSON: a misspelled field in a partial component is dropped and
   takes its default without an error (a directional light's brightness is
   `illuminance`, not `intensity`).
3. Change the scene with a patch file, never by hand:
   `rusting scene patch scenes/main.rscene patch.json [--dry-run]`.
   For large layouts, write a short script that generates `patch.json`.
4. Write game code in `src/main.rs`. `rusting check` builds it and
   validates the scene.
5. Write scenarios in `tests/` and run `rusting test` until they pass.
6. Look at your frames. Add `capture` steps to scenarios and open the PNGs.
   A test that passes on a black screen, or with the camera inside a wall,
   is not a finished game.
7. `rusting run` opens the window for a human to play.

## Scenes

- +Y is up, -Z is forward. Metres, radians, seconds. At yaw 0 an object
  faces -Z, so facing is `(-sin yaw, -cos yaw)` and the yaw toward
  `(dx, dz)` is `atan2(-dx, -dz)`.
- Objects are found by unique `name`. Give every object game code touches a
  clear name ("Guard A", "Key 1") and group similar ones with `classes`.
- `transform.scale` scales the mesh and its collider together: a unit box
  collider on an object scaled `[6, 2, 0.6]` is a 6 x 2 x 0.6 wall.
- Physics needs `physics_body` (`{"simulation": "Cpu"}`), `rigid_body`
  (`Fixed`, `Dynamic` or `Kinematic`) and `collider`. A `"sensor": true`
  collider only reports touches (pickups, doors, goals, hazards).
- Patch operations: `create` (an `entity` object; `"parent": "<name>"`
  attaches it), `set` (`id`, JSON-pointer `path`, `value`), `delete`,
  `remove`, `reparent`, `duplicate`, `set_scene`.

Create example (a sensor pickup that spins):

```json
{"operations": [
  {"op": "create", "entity": {
    "name": "Key 1", "classes": ["key"],
    "transform": {"position": [-10, 0.8, 8], "rotation": [0, 0, 0], "scale": [0.35, 0.35, 0.35]},
    "mesh_renderer": {"mesh": {"BuiltinPrimitive": "Cube"},
      "material": {"Inline": {"base_color": [1, 0.8, 0.3, 1], "emissive": [3, 2.2, 0.6]}}},
    "physics_body": {"simulation": "Cpu"}, "rigid_body": {"kind": "Fixed"},
    "collider": {"shape": {"Box": {"half_extents": [0.5, 0.5, 0.5]}}, "sensor": true},
    "components": {
      "rusting.pickup": {"counter": "keys"},
      "rusting.tween": {"property": "Rotation", "from": [0, 0, 0], "to": [0, 6.2831853, 0],
        "duration": 2.0, "easing": "Linear", "repeat": "Loop"}}}}
]}
```

### Gameplay without code

Prefer these components over game code when they fit:

- `rusting.counter` `{"name", "value", "target"}`: score, lives, keys,
  win and lose flags. A flag is a counter with target 1.
- `rusting.pickup` `{"counter"}`: the player touching it adds 1 to that
  counter.
- `rusting.hud` `{"anchor", "offset", "font_size", "text", "requires"}`:
  text with `{counter}` placeholders. With `requires` it shows only once
  that counter reaches its target, which makes win and lose screens free.
- `rusting.input_action` `{"action", "inputs"}`: named actions such as
  `{"action": "restart", "inputs": ["KeyR"]}`. Key names are winit
  `KeyCode` names; mouse is `MouseLeft`.
- `rusting.tween`, `rusting.sound_cue`, `rusting.burst_emitter`,
  `rusting.player_controller`, `rusting.joint`, `rusting.tile_map`.
- `rusting.fluid_block` makes a particle fluid: `count_x/y/z` particles of
  `spacing` meters resting in a box of `container_half_extents` centered on
  the entity. Dynamic bodies with sphere colliders float and sink in it.
  It is CPU only and one entity per particle, so keep it to a few thousand
  particles.
- `rusting.player_controller` walks up ground no steeper than
  `max_slope` (radians, default 45°) and steps onto ledges up to
  `max_step_height` (default 0.3 m); anything taller is a wall. Set
  `push_bodies: false` so the player never moves dynamic bodies, for
  example to keep a physics pile identical every run.
- Textures: `mesh_renderer.material.Inline.base_color_texture` (and the
  other slots) is a plain path string relative to the scene file, such as
  `"../assets/textures/crate.png"`. `uv_scale: [8, 4]` repeats the texture
  across each face instead of stretching it; `uv_offset` shifts it. The
  first row of an image is the top of each cube side face.
- Look: `rusting.background`, `rusting.sky_light`, `rusting.fog`,
  `rusting.bloom`, `rusting.tone_mapping`, `rusting.environment_map`,
  `directional_light`, `point_light`, `spot_light`.

Player actions are `player.forward`, `player.back`, `player.left`,
`player.right`, `player.jump`, `player.sprint`. The player controller is
kinematic: moving bodies do not push it, so handle hazards in code with
`touching`.

## Game code

```rust
use rusting_engine::prelude::*;

fn update(scene: &mut GameScene<'_>, time: &FrameTime) {
    if scene.pressed("restart") {
        scene.restart();
        return;
    }
    if scene.counter_complete("won") || scene.counter_complete("lost") {
        return; // the round is over; the HUD shows the result
    }
    let at_goal = scene.touching("Player").iter().any(|name| name == "Goal");
    if at_goal && scene.counter_complete("keys") {
        scene.set_counter("won", 1);
    }
}

rusting_game!(update);
```

`update` runs once per rendered frame. The main `GameScene` calls:

| Need | Call |
| --- | --- |
| find, move, turn, scale | `object(name)`, `try_object`, `set_position`, `move_by`, `set_rotation`, `set_scale`, `position`, `rotation`, `scale` |
| contacts | `touching(name)` (names of everything touching it, sensors included) |
| counters | `counter_value`, `set_counter`, `add_to_counter`, `counter_complete` |
| input | `pressed(action)` this frame, `held(action)` |
| rays | `raycast(origin, dir, max)` (sensors included), `aim`, `camera_ray`, `pointer_ray` (through the mouse) |
| physics | `set_body_kind` (to `Kinematic` or `Fixed` stops the body), `set_linear_velocity`, `linear_velocity`, `set_angular_velocity`, `angular_velocity`, `reset_body` (zero velocities, forget contacts and sleep, wake) |
| player | `set_look(name, yaw, pitch)` |
| create, remove | `spawn_cube`, `spawn_sphere`, `spawn_copy` (hidden template + children), `despawn` |
| look | `set_visible`, `color`, `set_color`, `set_emissive` (per object) |
| rounds, levels | `restart`, `once(key, setup)`, `load_scene("scenes/level_2.rscene")`, `initial(name)` (starting transform, color, body kind), `snapshot()` / `restore(&snapshot)`, `state_hash(class)` |
| other | `tile`, `set_tile`, `trigger`, `in_class`, `random(stream)`, `ui()` (egui), `world()` (raw ECS) |

### Rules that keep tests reliable

- Time gameplay with `time.fixed_tick`, never with frame counts or wall
  time. Make moving things a pure function of the tick where you can (a
  guard's patrol position = f(tick)); scenarios then repeat exactly.
- Randomness comes only from `scene.random(stream)`: it repeats for a
  scenario's `seed` and differs per tick and stream. Never use `rand` or
  the system clock.
- Keep round state in counters, not in Rust statics: `restart` and
  `load_scene` reset the scene, and statics would survive.
- `restart` reloads the current scene and runs `once` blocks again.
  Physics after `restart`, `load_scene` or `restore` repeats bit for bit
  what it did the first time, so a loop or replay game can simply
  restart. `snapshot()` saves the scene mid-game and `restore` puts it back
  with velocities, sleep and solver state; the clock carries on, so time
  loop logic by ticks since the round began.
- Use CPU bodies (`"simulation": "Cpu"`) for anything gameplay reads or
  must repeat. GPU bodies reach game code a few frames late, cannot be
  reset or moved from code after setup, and are not covered by
  `restart` determinism; use them for large decorative piles.
  `load_scene` switches to another scene file, which then becomes the one
  `restart` returns to. Counters live in the scene, so carry a score to the
  next level by reading it before `load_scene` and setting it after.
- After `restart` or `load_scene`, return from `update`: objects you looked
  up earlier in the frame are gone.
- Game code adds no dependencies beyond `rusting_engine`.

## Multiple levels

Create a level by copying `scenes/main.rscene` to `scenes/level_2.rscene`
and patching the copy. Then, in code:

```rust
if scene.counter_complete("keys") && at_exit {
    let score = scene.counter_value("score");
    scene.load_scene("scenes/level_2.rscene").expect("level 2 loads");
    scene.set_counter("score", score);
    return;
}
```

`rusting export` ships only the main scene today, so exported builds cannot
load other levels yet; `rusting run` and `rusting test` can.

## Scenarios

A scenario is a JSON file in `tests/`. `rusting test` runs them all.

```json
{"name": "Taking every key and reaching the door wins", "ticks": 130, "steps": [
  {"tick": 1, "set": {"entity": "Player", "path": "/transform/position", "value": [-10, 1, 8]}},
  {"tick": 2, "within": 15, "expect": {"entity": "Keys Counter", "path": "/components/rusting.counter/value", "equals": 1}},
  {"tick": 20, "press": "player.forward"},
  {"tick": 22, "until": 59, "expect": {"entity": "Escaped Counter", "path": "/components/rusting.counter/value", "equals": 0}},
  {"tick": 60, "set": {"entity": "Keys Counter", "path": "/components/rusting.counter/value", "value": 3}},
  {"tick": 61, "within": 70, "expect": {"entity": "Escaped Counter", "path": "/components/rusting.counter/value", "equals": 1}},
  {"tick": 80, "capture": "shots/escaped.png"},
  {"tick": 90, "release": "player.forward"},
  {"tick": 100, "press": "restart"},
  {"tick": 101, "release": "restart"}
]}
```

- Steps: `press`, `release` (action names), `set`, `expect`,
  `expect_events`, `capture` (path relative to the scenario), `pointer`
  (`[x, y]` as fractions of the view, `[0.5, 0.5]` is the center), `log`
  (`{"entity", "path"}`; records the value, with `until` on every tick,
  and never fails: use it instead of `eprintln!` to debug a scenario).
- A scenario stops at its first failed check; `"keep_going": true` in the
  scenario runs on and reports every failure. `rusting test --json` lists
  the `log` lines of each scenario; `rusting run --ticks N --json` reports
  `timings.headless_ms_per_tick`.
- `rusting capture` renders the scene file without running game code;
  for a state the code creates, add a `capture` step to a scenario.
- `expect` takes `equals`, `greater_than`, `less_than` or `exists`, plus
  `tolerance` for numbers and arrays. It does not take `value`.
- `within` and `until` are absolute ticks, not durations. `within: 70`
  from tick 61 means "true on some tick from 61 to 70"; `until: 59` means
  "true on every tick from 22 to 59". A `set` on a tick applies before that
  tick's checks, so end an `until` before the tick that changes the value.
- A check on tick N sees the state after tick N's update. If the game acts
  on the first tick (a guard sees you at once), do not expect the starting
  value on tick 1.
- HUD objects have no `/visible` value; check the counter the HUD
  `requires` and look at a capture instead.
- Write at least: one scenario for the main success path, one for failure,
  and one that restarts. Test the rule, not the tuning: set the player next
  to the thing under test instead of walking there for 600 ticks.
- Add `tests/shots` to the project's `.gitignore`.

## Performance

- Run the release binary with `RUSTING_PERF=1` to see fps, update and
  render time, GPU pass times and draw counts once a second on stderr.
  Close other GPU programs first: a second game instance halves the numbers.
- Objects with the same mesh and textures share one draw whatever their
  colors, so give many props one texture set instead of many textured ones.
- Materials rougher than 0.5 skip screen-space reflections. If no surface
  needs a mirror-like look, call `scene.set_reflections(false)` at startup:
  it removes the scene copy, mip chain and depth pyramid passes.

## Before calling a game done

- `rusting check` and `rusting test` pass.
- You opened every capture and it shows what its scenario claims.
- The project `README.md` says how to play, what each file does and what
  each scenario proves.
- If the engine was missing something, report the gap (what you needed,
  what you wrote instead) rather than hiding a workaround in game code.
