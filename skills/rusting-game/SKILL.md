---
name: rusting-game
description: Build, test and polish a game on RustingEngine from the command line. Use when asked to create or change a Rusting game project, its scenes (.rscene), its game code (rusting_game!, GameScene) or its scenario tests.
---

# Making games with RustingEngine

A Rusting game is a folder with `project.json`, JSON scenes, a small Rust
`update` function and JSON scenario tests. You can build a whole game
without the editor: every step below is a `rusting` command, and every
command takes `--json`.

Thirteen sample games were built this way. Read the closest one before
starting with `rusting docs show sample/<name>` (its README and
`src/main.rs`): `hammer_run` (obstacle course), `sky_hop` (platformer),
`ember_arena` and `night_vault` (third person), `target_range` and
`tower_topple` (first person, shooting), `putt_course` (physics ball, mouse
aim), `core_defense` (top-down, mouse), `lantern_grid` (click puzzle),
`crate_keeper` (grid puzzle), `snake_trail` and `brick_bounce` (2D),
`forever_bear_booth` (horror: camera screens, 3D sound, waypoints).

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
   scene JSON (a directional light's brightness is `illuminance`, not
   `intensity`). `rusting schema --json-schema` prints JSON Schema for
   scene, patch, scenario, `project.json` and `.rmeta` files. `rusting validate` reports a key the scene format does not
   have as `SCENE_UNKNOWN_FIELD`, and `rusting fix` renames a close
   misspelling in place.
3. Change the scene with a patch file, never by hand:
   `rusting scene patch scenes/main.rscene patch.json [--dry-run]`.
   For large layouts, write a short script that generates `patch.json`.
4. Write game code in `src/main.rs`. `rusting check` builds it and
   validates the scene.
5. Write scenarios in `tests/` and run `rusting test` until they pass.
6. Look at your frames. Add `capture` steps to scenarios and open the PNGs.
   A test that passes on a black screen, or with the camera inside a wall,
   is not a finished game.
7. Polish. Run the loop in `rusting docs show guide/look-and-feel`:
   capture, look, critique against its checklist (silhouette, palette,
   warm key and cool fill, materials, shapes, HUD margins, feedback), fix
   the worst item, repeat. Start from `rusting preset apply`.
8. `rusting run` opens the window for a human to play.

Every error carries a code such as `SCENE_CONFLICT`. `rusting explain CODE`
prints what it means, how to fix it and an example; `rusting explain` lists
every code. `rusting docs search <words>` and `rusting docs show <id> [--budget N]` read
the manual, tutorials and every command offline, matched to this binary;
`rusting docs --brief` is a one-screen overview. A diagnostic with a `fix` is certain; `rusting fix --dry-run` lists
them and `rusting fix` applies them.

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
  inside `entity` attaches it to an object that exists or was created
  earlier in the same patch), `upsert` (like `create`, but replaces the entity with the same
  `id` or `name` and keeps its id, so a patch can run twice; `"merge": true` keeps the fields you leave out), `set` (`id`, JSON-pointer `path`, `value`), `delete`
  (`"missing_ok": true` skips an entity that is not there),
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
  example to keep a physics pile identical every run. `turn_speed`
  (radians per second, default 0 = off) turns the body's non-camera
  children, the visible rig, toward the walking direction.
  `mouse_look: false` stops a left click from capturing the mouse (for a
  second player or a UI). The controller owns the transform of its direct
  camera children; `camera_offset` moves the camera in the body's frame
  (`[0.6, 0, 0]` over the right shoulder).
  `rusting docs show api/PlayerController` lists every field.
- Split screen: give two active cameras a `viewport` (`[x, y, width,
  height]` fractions); `scene.set_camera(name, active, viewport)` changes
  one from code. A `rusting.hud` with `"camera": "<name>"` anchors to that
  camera's viewport. `rusting docs show guide/cameras`.
- `rusting.particle_emitter` is the full particle effect: shape, random
  `[min, max]` ranges, gravity/drag/wind/turbulence, size and color over
  life, additive glow, velocity streaks, one instanced draw per emitter.
  `scene.particles(name, ParticleCommand::Stop)` controls it from code.
  `rusting effect apply <scene> fire --at X,Y,Z` (or `--on OBJECT`) adds a
  preset: dust_motes, falling_leaves, snow, rain, sparks, smoke, fire,
  embers, fireflies, magic_sparkle, confetti.
  `rusting docs show guide/effects`.
- `rusting.burst_emitter` with `rate` (particles per second) emits every
  fixed step with no trigger; `area` (half extents of a world-aligned box)
  spreads the start points and `stretch` makes each particle that many
  times taller. Rain: `rate: 300, area: [10, 0, 10], speed: 0,
  gravity: 20, stretch: 8, on_collision: false`, on an entity above the
  player.
- Round things: give a dynamic body `"collider": {"shape": "ConvexMesh"}`
  and a `Cylinder` (or any) mesh; it collides as the mesh's convex hull, so
  a barrel on its side rolls. There is no cylinder shape of its own.
- Keyframe animation: `rusting.animation` holds named clips; each track
  keys `Position`, `Rotation` (radians), `Scale`, `Color`, `Emissive`,
  `Visible` or a numeric `Field` of another component, on the object or a
  child path such as `Arm/Hand`, with `Step`, `Linear` or `Smooth`
  interpolation and `Once`, `Loop` or `PingPong` repeat. Clip `events`
  markers reach game code through `scene.animation_events()`. Game code
  calls `play_animation`, `crossfade`, `stop_animation`, `is_playing` and
  `set_animation_speed`. `transitions` (from, to, parameter test, fade)
  make a state machine driven by `set_animation_parameter`. Clips with
  `blend` points are 1D or 2D blend spaces; `layers` play clips on top
  (override or additive, masked to child paths); `root_motion` turns a
  root bone's stride into object motion (`take_root_motion` for game code).
  `rusting scene retarget <scene> <from> <clip> <to>` copies a clip onto
  another skeleton (bones matched by name or the `humanoid` map).
  `rusting.ik` (`LookAt`, `TwoBone`, `Foot` or `Chain`) makes a
  joint look at or reach a target object. `rusting.ragdoll` makes a
  character go limp on a hit or `set_ragdoll(name, true)` and blend back
  to its animation afterwards; with `muscle` (Hz) above 0 it is an active
  ragdoll that follows its clips physically and staggers when pushed
  (good for animatronics, zombies and hit reactions). See
  `rusting docs show guide/animation`.
  `scene add-model` keeps a glTF's node animations as clips on the new
  object (the first autoplays).
- Skinned glTF models keep their skin: `scene add-model` adds
  `rusting.skin` (joint paths and inverse bind matrices; weights live in a
  `.rskin` file next to the mesh), and the model's clips move the joints.
  Blend shapes become `rusting.morph` weights, which clips key with a
  `Field` track on `/weights`. Both run on the CPU, so keep to a few
  characters. Without a rigged
  model, build a character from child entities (torso, limbs) and key
  their rotations in a clip, or use `rusting.tween` for simple loops.
- Textures: `mesh_renderer.material.Inline.base_color_texture` (and the
  other slots) is a plain path string relative to the scene file, such as
  `"../assets/textures/crate.png"`. `uv_scale: [8, 4]` repeats the texture
  across each face instead of stretching it; `uv_offset` shifts it. The
  first row of an image is the top of each cube side face.
- Look: `rusting preset apply scenes/main.rscene <preset>` sets sky, sun,
  ambient, fog, bloom, tone mapping and color grading at once (`daylight`,
  `golden_hour`, `night`, `flat_toy`, `dark_interior`,
  `bright_stylized`); `rusting preset list --json` gives each preset's
  five-color palette, so take object colors from it. Tune with
  `rusting.background`, `rusting.sky_light`, `rusting.fog`,
  `rusting.bloom`, `rusting.tone_mapping`, `rusting.color_grading`
  (`contrast`, `saturation`, `shadows`/`highlights` tints, `vignette`),
  `rusting.environment_map`, `directional_light`, `point_light`,
  `spot_light`. Put `rusting.post_volume` (`extents`, `blend`,
  `priority`) on an object with its own fog or grading to use them only
  in that area, such as a foggy hall next to a warm office.
- Shapes: `BuiltinPrimitive` takes `Cube`, `Sphere`, `Cylinder`, `Cone`,
  `Capsule` (diameter 1, height 2), `RoundedCube` (unit box, edges rounded
  by 0.1), `Torus`, `Plane`, `Quad` and a few solids. Use `RoundedCube` and
  `Capsule` for props and characters the player looks at; `guide/look-and-feel`
  has a ready character patch.
- Screens: `rusting.camera_screen` (`camera` name, `size` [w, h]) shows
  another camera's view on a mesh, for CCTV monitors and rear-view screens.
  Give each screen its own material with some `emissive`. See
  `guide/cameras`.
- Models: free CC0 glTF models (Kenney, Quaternius, Poly Pizza) look far
  better than primitives. `rusting asset import . model.glb --to models
  --license CC0-1.0 --author NAME`, then `rusting scene add-model
  scenes/main.rscene assets/models/model.glb --name "Tree 1"` places it as
  one object with its materials. See `guide/look-and-feel`.

Player actions are `player.forward`, `player.back`, `player.left`,
`player.right`, `player.jump`, `player.sprint`, `player.crouch` (set
`crouch_height` to the crouched body height). The player controller is
kinematic: moving bodies do not push it, so handle hazards in code with
`touching` (it includes the floor the player stands on and the wall or body
it pushes). A player teleported a little into a floor or platform is lifted
onto its top.

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

Game state that is more than a few counters can live in your own
component, with no extra crates in `Cargo.toml`:

```rust
#[derive(Component, Clone, Default, Serialize, Deserialize)]
#[serde(crate = "rusting_engine::serde")]
struct Night {
    power: f32,
}

rusting_engine::reflect! {
    struct Night {
        power: f32 { unit: "W", doc: "power left" },
    }
}

rusting_game!(update, components: [Night => "game.night"]);
```

Scenes save it as `game.night`, scenarios read and set it as
`/components/game.night/power`, and code reads it through
`scene.world().query::<&mut Night>()`. `tick: my_tick, components: [...]`
works too.

`update` runs once per rendered frame. The main `GameScene` calls:

| Need | Call |
| --- | --- |
| find, move, turn, scale | `object(name)`, `try_object`, `set_position`, `move_by`, `set_rotation`, `set_scale`, `position`, `rotation`, `scale`, `reparent(name, Some(parent))` (keeps the local transform; `None` detaches) |
| contacts | `touching(name)` (names of everything touching it, sensors included) |
| counters | `counter_value`, `set_counter`, `add_to_counter`, `counter_complete` |
| input | `pressed(action)` this frame, `held(action)`, `stick(Stick::Left)` (gamepad tilt; bind `PadSouth`, `PadDpadUp`, `PadLeftStickUp`... in `rusting.input_action`; the player controller already binds the left stick, d-pad, South to jump and the right stick to look) |
| rays | `raycast(origin, dir, max)` (sensors included; a ray starting inside a collider passes it), `raycast_skipping(origin, dir, max, &["glass"])`, `aim`, `camera_ray`, `pointer_ray` (through the mouse) |
| cameras | `set_active_camera(name)` (the only active one), `set_camera(name, active, Some([x, y, w, h]))` (split screen), `basis(name)` (world `[right, up, forward]`; forward is -Z, rotation applies X, then Y, then Z) |
| physics | `set_body_kind` (to `Kinematic` or `Fixed` stops the body), `set_linear_velocity`, `linear_velocity`, `set_angular_velocity`, `angular_velocity`, `reset_body` (zero velocities, forget contacts and sleep, wake) |
| player | `set_look(name, yaw, pitch)`, `player(name)` (a copy of its controller: `grounded`, `floor`, `wall` it pushes against, `velocity` of the last step; its `Transform` rotation Y equals `yaw`, so or set `turn_speed` to face the rig along `velocity`) |
| create, remove | `spawn_cube`, `spawn_sphere`, `spawn_copy` (hidden template + children), `despawn` |
| sound | `play_sound("sfx/hit.wav", volume)`, `play_sound_looped`, `play_sound_with(clip, Sound { pan, bus, at_tick, position, .. })`, `set_sound_volume(id, v, fade)`, `set_bus_volume(bus, v, fade)`, `stop_sound(id)`, `stop_all_sounds`, `set_master_volume`, `sounds_requested()`, `BeatClock`, `press_tick(action)` (fractional tick of a press); clip paths are under `assets/`; see `guide/audio` |
| files | `load_text("levels/1.txt")` reads a text file under `assets/`; `rusting validate` fails on a literal asset path in game code that is not a file |
| look | `set_hud(name, \|hud\| ..)` (text, color, size of a `rusting.hud`, shown the same tick), `set_visible`, `color`, `set_color`, `set_emissive` (per object), `set_light(name, color, intensity, range)` (point or spot light; `None` keeps a value) |
| animation | `play_animation(name, clip)`, `crossfade(name, clip, secs)`, `stop_animation`, `is_playing(name, clip)`, `set_animation_speed`, `set_animation_parameter(name, param, value)` (state machine input), `animation_events()` (clip markers), `take_root_motion(name)`, `set_ragdoll(name, limp)`, `set_ragdoll_muscle(name, hz)`, `is_limp(name)`; see `guide/animation` |
| rounds, levels | `restart`, `once(key, setup)`, `load_scene("scenes/level_2.rscene")`, `initial(name)` (starting transform, color, body kind), `snapshot()` / `restore(&snapshot)`, `state_hash(class)` |
| menus, saves | `ui()` (egui), `set_paused`, `paused`, `quit`, `save_data(key, text)`, `load_data`, `delete_data`, `counters()`, `keys_pressed()`, `rebind(action, &[key])`, `cursor()`, `viewport_size()`; see `guide/menus-and-ui` |
| video settings | `set_render_scale(0.25..=2.0)`, `render_scale`, `set_pixelated(true)` (nearest upscale for a chunky low scale), `set_vsync`, `set_max_fps(Option<u32>)`, `set_fullscreen`, `fullscreen`, `set_window_size([w, h])` |
| other | `tile`, `set_tile`, `trigger`, `in_class`, `name_of(entity)`, `has_class(entity, class)`, `binding(action)` (keys of a scene `rusting.input_action`), `random(stream)`, `ui()` (egui), `world()` (raw ECS) |

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
  must repeat. GPU bodies reach game code a few frames late and are not
  covered by `restart` determinism; use them for large decorative piles.
  `restart` puts every GPU body back at its scene-file pose. Move or push
  one from code with `scene.gpu_command("Ball#7",
  GpuBodyCommand::Teleport(Transform::new([0.0, 5.0, 0.0])))` (also
  `SetVelocity`, `Impulse`, `Force`). Their
  `Transform` keeps the spawn pose: read the GPU pose with
  `gpu_state(name)` (set `GpuBodySettings { sync:
  PhysicsSyncMode::SelectedState, .. }`), and in scenarios under
  `/gpu_state/position`.
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
`rusting add scenario <name>` writes one that fails until you fill in a real check;
`rusting add system <name>` also appends a documented stub function to
`src/main.rs` (call it from `update`).

```json
{"name": "Taking every key and reaching the door wins", "ticks": 130, "steps": [
  {"tick": 1, "set": {"entity": "Player", "path": "/transform/position", "value": [-10, 1, 8]}},
  {"tick": 2, "within": 15, "expect": {"counter": "keys", "equals": 1}},
  {"tick": 20, "press": "player.forward"},
  {"tick": 22, "until": 59, "expect": {"entity": "Escaped Counter", "path": "/components/rusting.counter/value", "equals": 0}},
  {"tick": 60, "set": {"counter": "keys", "value": 3}},
  {"tick": 61, "within": 70, "expect": {"entity": "Escaped Counter", "path": "/components/rusting.counter/value", "equals": 1}},
  {"tick": 80, "capture": "shots/escaped.png"},
  {"tick": 90, "release": "player.forward"},
  {"tick": 100, "press": "restart"},
  {"tick": 101, "release": "restart"}
]}
```

- Steps: `press`, `release`, `tap` (action names; `"at": 0.5` presses
  partway through the tick, read by `press_tick`), `click` (an on-screen
  text label: egui buttons, HUD buttons; or `{"text" | "starts_with",
  "index"}`, `index` counting in reading order), `expect_file` (`{"path",
  "exists", "contains"}` in the user data folder, checked even after a
  quit), `restart`, `expect_quit`,
  `left_stick` / `right_stick` (`[x, y]` gamepad tilt), `set`, `expect`,
  `expect_events`, `expect_screen` (`on_screen`, `occluded`, `inside`,
  `min_share`, optional `camera`), `expect_pixels` (`region` fractions,
  `mean_min`/`mean_max` `[r, g, b]` 0..255, `stddev_min`/`stddev_max`,
  optional `camera` viewport), `capture` (path relative to the scenario file's folder: `"shots/a.png"` from `tests/`, not `"tests/shots/a.png"`; or
  `{"path", "camera", "hud": false, "golden": "golden/a.png", "tolerance": 1.0}`,
  and `rusting test --update-golden` writes the golden images), `pointer`
  (`[x, y]` as fractions of the view, `[0.5, 0.5]` is the center), `log`
  (`{"entity", "path"}`; records the value, with `until` on every tick,
  and never fails: use it instead of `eprintln!` to debug a scenario;
  plain `rusting test` prints each logged value as `tick N: ...`). A
  failed scenario also prints the last 20 lines of the game's stderr.
- Each scenario runs with an empty user data folder; top-level `files`
  copies save fixtures into it. Menus, saves and rebinding:
  `rusting docs show guide/menus-and-ui`.
- Game code sees `time.fixed_tick == N` on scenario tick N.
- `"entity": "audio:"` reads the sounds game code and sound cues asked for:
  `/requested` (all plays), `/clips/<clip>` (plays of one clip; write `/`
  in the clip path as `~1`: `/clips/sfx~1hit.wav`; a clip never played reads 0), `/level` (`[l, r]` RMS
  of the offline mix), `/peak` (`[l, r]` max per tick), `/clipped` (samples at full scale so far) and `/playing` (clip, volume, pan, bus, tick of each
  sound still playing). Top-level `"audio_out": "mix.wav"` writes the mix.
  Headless runs have no audio device, so this is how a scenario checks
  sound.
- `rusting serve` keeps one process open: write JSON-RPC lines (`{"jsonrpc":"2.0","id":1,"method":"scene query","params":[...]}`) to stdin, read one
  envelope per line from stdout; `shutdown` ends it.
- `rusting debug` runs the game paused and answers one JSON line per command: `{"id":1,"cmd":"step","ticks":5}`, `get`/`set` (`entity`, `path`, `value`), `press`/`release` (`action`), `capture` (`path`), `tick`, `quit`.
- `rusting mcp [root]` exposes every command as an MCP tool (`scene_query`, args as `{"args":[...]}`); read-only tools carry `readOnlyHint`; paths with `..` or absolute are refused.
- `rusting test --json` also gives each scenario a `perf` report (mean, p95
  and max tick milliseconds; `wall_ms_mean` is the whole run per tick,
  checks included; `render` has draws, triangles and `gpu_ms` of
  the last rendered frame and per viewport camera under `cameras`; and the
  environment). A render budget renders even without a capture step. A scenario `budgets` object (`max_tick_ms`,
  `mean_tick_ms`, `p95_tick_ms`, `max_draws`, `max_triangles`) fails the run
  when a limit is exceeded; timings differ per machine and debug/release.
- `rusting test --json` also gives each scenario a `trace`: inputs, `set` steps and
  collisions in tick order (first 1000), to see what happened without a capture.
- A scenario stops at its first failed check; `"keep_going": true` in the
  scenario runs on and reports every failure. `rusting test --json` lists
  the `log` lines of each scenario; `rusting run --ticks N --json` reports
  `timings.headless_ms_per_tick`.
- `rusting inspect --tick N [--entity NAME]` runs the game to tick N and prints each
  entity's state after that tick, without writing a scenario.
  It runs without scenario input; to see a value during a scenario, add a
  `log` step.
- `rusting determinism . --scenario tests/run.json` replays a scenario in a
  debug and then a release build and compares the state hash of every
  tick: a scripted full run is the replay test. `rusting run --record
  run.json` saves a played session (it stops at `--timeout`); `rusting run --replay run.json` plays it
  back headless and fails at the first tick that differs.
- `"annotate": true` in a scenario makes each `capture` also write `<name>.annotated.png`
  (a box and a 4-digit short ID over every mesh entity) and `<name>.annotated.json`
  (short ID to entity name, full ID and pixel box). Boxes come from mesh bounds and ignore occlusion.
- `"contact_sheet": "shots/sheet.png"` in a scenario also writes one image with every `capture`
  frame in a grid, each labelled with its tick: one image shows motion.
- `rusting capture` renders the scene file without running game code;
  for a state the code creates, add a `capture` step to a scenario.
  `--at X,Y,Z --look-at X,Y,Z` (or `--look YAW,PITCH`) shoots from any
  point without adding a camera to the scene.
- `expect` takes `equals`, `not_equals`, `greater_than`, `less_than` or
  `exists`, plus `tolerance` for numbers and arrays. It does not take
  `value`.
- `{"counter": "keys"}` in `expect`, `set`, `log` and `invariants` stands for
  the entity and path of the counter named `keys`, so a test does not need
  the counter object's name. `set_counter` and `add_to_counter` create a
  counter that does not exist yet, so plain game state (a clock, a power
  level) needs no scene object; reading a missing counter warns once on
  stderr and returns 0.
- `within` and `until` are absolute ticks, not durations. `within: 70`
  from tick 61 means "true on some tick from 61 to 70"; `until: 59` means
  "true on every tick from 22 to 59". A `set` on a tick applies before that
  tick's checks, so end an `until` before the tick that changes the value.
- `pressed(action)` is true only on the tick an action goes down, so a
  held `press` gives one edge. Press again after a `release`, or use
  `tap`, which presses and releases before the next tick.
- Bodies are solved in entity order, so adding or removing unrelated
  entities can change where a toppling pile ends up, although each run of
  the same scene repeats exactly. Assert robust values (distance moved,
  "fell over") rather than one exact final height.
- Order inside one tick: `set`, `press`, `release`, `tap` and `pointer` first, then
  the game's update and physics, then `expect`, `log`, `capture` and the
  invariants. To check a value before a `set` changes it, check one tick
  earlier.
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
- Level of detail: a `.rlod` file beside a `.rmesh` (same name) lists
  coarser meshes and the distance or screen size where each takes over;
  every object drawing that mesh uses it (`guide/look-and-feel`).
- Materials rougher than 0.5 skip screen-space reflections. If no surface
  needs a mirror-like look, call `scene.set_reflections(false)` at startup:
  it removes the scene copy, mip chain and depth pyramid passes.

## Before calling a game done

- `rusting check` and `rusting test` pass.
- You opened every capture and it shows what its scenario claims.
- The frames pass the `guide/look-and-feel` checklist.
- The project `README.md` says how to play, what each file does and what
  each scenario proves.
- If the engine was missing something, report the gap (what you needed,
  what you wrote instead) rather than hiding a workaround in game code.
