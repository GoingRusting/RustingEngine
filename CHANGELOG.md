# Changelog

## [Unreleased]

Engine features for the horror game FOREVER BEAR.

### Added

- Agent benchmark task `crypt-wait`: a feature task on the roguelike template, a wait action that spends a turn.
- `RUSTING_BENCH_TASK=<folder>` limits the agent benchmark's slow validation test to one task.
- Agent benchmark task `lock-new-game`: a new-game task from the empty template, a four-digit code lock.
- Agent benchmark task `arena-knockback`: a bug-fix task on the top-down template, where a hit throws the enemy through the player.
- Agent benchmark task `duel-redraw`: a feature task on the card-game template, a once-per-duel mulligan.
- Agent benchmark task `swarm-diagonal-speed`: a bug-fix task on the twin-stick template, where diagonal moves are too fast.
- Agent benchmark task `puzzle-undo`: a feature task on the puzzle template, a one-step undo.
- Agent benchmark task `tower-reach`: a bug-fix task on the tower-defense template, where towers ignore the Z distance to their target.
- Agent benchmark task `racing-lap-times`: a feature task on the racing template, with last and best lap times.
- `rusting capture --game` builds the project and captures its game with game code run up to `--tick`.
- A running game shows fps, frame time and p95 in its top-right corner while `RUSTING_PERF` is set; F3 toggles that report on and off.
- Agent benchmark task `reaction-new-game`: a new-game task from the empty template, a reaction-time game with false starts.
- Agent benchmark task `platformer-goal`: a feature task on the 2D platformer template, where touching the Goal clears the level once.
- Agent benchmark task `crypt-diagonal`: a bug-fix task on the roguelike template, where diagonal enemies strike.
- Agent benchmark task `swarm-bomb`: a fourth feature task, a once-a-round bomb in the twin-stick template.
- Agent benchmark task `tapper-new-game`: a third new-game task, a ten-second tapping game with a countdown.
- Agent benchmark task `rhythm-spark-leak`: a second performance task, where hit sparks are hidden instead of despawned.
- `rusting lint` checks literal object names in every `GameScene` call that takes one, and warns `LINT_MISSING_EVENT` on a literal `gpu_events` name that no GPU physics rule in code or a scene names.
- `rusting lint` warns `LINT_SILENT_GOAL` on a counter with a target that nothing reacts to: no HUD element requires or shows it, no pickup requires it, and game code never names it.
- `rusting lint` warns `LINT_COLOR_ONLY_STATUS` when game code changes a HUD element's colour with `set_hud` but never its text, so the state it shows depends on telling colours apart. Brightness or alpha changes on one hue and counter readouts are not flagged.
- `rusting capture` warns `CAPTURE_TEXT_CONTRAST` when plain HUD text is hard to read against the scene behind it (WCAG AA, judged on the rendered frame). Captures at tick 0 now include the HUD: plain HUD text is painted directly instead of in an egui area, which hid it for its first frame, and no longer blocks the pointer. The starter and racing templates use dark HUD text that passes the check.
- `LINT_MISSING_FILE` also checks literal `spawn_prefab` paths.
- `rusting lint` warns `LINT_MISSING_CLASS` on a literal `in_class` name nothing puts objects in, with close class names.
- Agent benchmark task `arena-cooldown`: a feature request on the top-down template.
- Agent benchmark task `puzzle-box-stack`: a bug fix on the puzzle template.
- Agent benchmark task `walker-new-game`: a new game from the empty template.
- Agent benchmark task `racing-wide-road`: a bug fix on the racing template.
- Agent benchmark task `tower-sell`: a feature request on the tower-defense template.
- `rusting lint` warns `LINT_MISSING_FILE` on a literal `load_scene` path or `play_sound` clip with no file there.
- `rusting asset generate <root> mesh "barrel 4"` builds a seeded low-poly CC0 glTF prop: crate, barrel, rock, tree or gem.
- `rusting revert` undoes `scene split` and `scene join`: both now journal the old form they delete.
- `rusting asset generate <root> texture "bricks 3"` makes a tileable placeholder texture with no generator hook: six presets (grid, checker, bricks, planks, tiles, noise) in one shared palette, the seed picking the colour, imported as CC0 with the preset and seed in its `.rmeta` notes.
- `rusting asset generate <root> sprite "star 2"` makes a placeholder sprite the same way: one of six outlined shapes (circle, square, triangle, diamond, star, heart) on a clear 128 px background, for a `Quad` with `alpha_mode` `Blend`.
- `benchmarks/agent/run.py --agent '<command>'` runs the benchmark tasks against any agent CLI and writes a JSON report: hidden-scenario pass rate, wall time, output size, and the agent's commands and corrective builds. `RUSTING_COMMAND_LOG=<file>` makes every `rusting` command append a JSON line with its arguments, result, diagnostic codes and duration.
- `benchmarks/agent/` holds the first agent benchmark tasks (a new game, two bug fixes, a feature and a performance fix): a one-paragraph request, a seeded starting project and hidden acceptance scenarios each. `cargo test --test agent_benchmark -- --ignored` proves every task's hidden scenarios fail on the seeded project and pass with a reference solution.
- Any `rusting` command takes `--offline`, and `RUSTING_OFFLINE=1` sets it for a session: builds pass `--offline` to cargo, so they use only crates already downloaded and never reach the network.
- `rusting scene split` turns a scene into folder form, a `.rscene` directory with `scene.json` and one `entities/<id>.json` per entity, so edits to different objects do not conflict in git; `rusting scene join` turns it back. The runtime, the editor and every scene command read and write both forms, and a patch rewrites only the entity files it changed.
- `rusting lint` warns `LINT_MISSING_OBJECT` when game code names an object, as in `scene.flash("Stairz")`, that no scene or prefab has, and `LINT_MISSING_ACTION` when it reads an input action nothing defines, as in `scene.pressed("jmup")`; both list close names.
- `rusting.sound_cue` takes a `caption`, such as `[glass breaks]`, shown for 2 s each time the cue fires.
- `--confine DIR` (or `RUSTING_CONFINE=DIR`) refuses with `OUTSIDE_CONFINE` a run whose working folder or path arguments resolve outside DIR, links and `..` included, and file writes outside it. A `--confine` flag can narrow `RUSTING_CONFINE` but never widen it.
- `--read-only` (or `RUSTING_READ_ONLY=1`) lets only commands that write no file run, plus `scene patch` and `fix` with `--dry-run`; others fail with `READ_ONLY`. `rusting schema` lists the allowed commands under `read_only`.
- `rusting lint` warns `LINT_TEXT_CONTRAST` when HUD button text falls below WCAG contrast (4.5:1, or 3:1 from 24 px) against the button fill.
- `rusting lint` warns `LINT_TEXT_OVERFLOW` when HUD text, measured in the HUD's font, runs past the edge of a 1280 x 720 view.
- `rusting lint` warns `LINT_GOAL_UNREACHABLE` for a sensor (pickup, goal, trigger) at the player's height that no walk or jump from the player's start reaches past fixed walls. Walls named as strings in game code count as doors.
- `rusting systems [root]` also lists the game code functions of a project (stage `Game`, with file and line) and the components each reads and writes, from bevy system parameters and `GameScene` calls, so `rusting systems --writes Health` answers for game components too.
- Operation journal: each CLI or daemon command that writes project files is recorded in `.rusting/journal.jsonl`; `rusting log` lists operations and `rusting revert <op>` undoes one unless a later operation changed the same file (`REVERT_CONFLICT`). `add scenario`, `add system`, recipes and the AGENTS.md refresh now write atomically and respect leases.
- `rusting provenance [root]`: a portable record of engine crates, a game-code hash, asset hashes with their `.rmeta` provenance, generator hooks, scene hashes, and scenario seeds and hashes.
- `rusting lease claim|release|list`: scoped leases for parallel agents. A scene or prefab write to a path another agent leases fails with `LEASE_HELD` naming the holder; agents name themselves with `RUSTING_AGENT` or `--as`.
- A test pins stable scene saves: entities sorted by ID, fields and components in a fixed order, so save-load-save is byte-identical and one edit changes one line.
- `rusting merge <base> <ours> <theirs> [--output PATH]`: a three-way scene merge by entity ID and field, for use as a git merge driver. Conflicts are `SCENE_MERGE_CONFLICT` errors naming the entity and field. `rusting new` writes `.gitattributes` with `*.rscene merge=rusting-scene`; enable it with `git config merge.rusting-scene.driver "rusting merge %O %A %B"`.
- `rusting impact <file-or-component>` lists the scenes, entities, code lines and scenarios a change to an asset, prefab or component would touch.
- `rusting systems [--reads TYPE | --writes TYPE]` lists the engine's ECS systems by stage with the components and resources each reads and writes, so an agent can ask who writes `Transform`. `App::into_system_access` returns the same data.
- `rusting project summary [--budget N]`: a token-budgeted overview of a project's scenes and prefabs (entities, components, assets), game code files (functions taking `GameScene`, literal asset paths) and component use counts. `CODE_MISSING_ASSET` now also checks `spawn_prefab("...")` paths.
- `scene.spawn_prefab(path, name, transform)` places a prefab scene file from game code in one call (surveyor F4).
- `perf.stages_ms_mean` in scenario results and the `RUSTING_PERF` line split tick CPU time into `fixed`, `update`, `post_update` and `extract` (unclaimed F8).
- `perf.cpu_ms_mean` and the `mean_cpu_ms` scenario budget measure CPU time per tick, which other processes on the machine slow far less than wall time (surveyor F20).
- Transform propagation recomputes only the entities that moved and their children, so static props cost nothing per tick (unclaimed F8).
- `scene.on_screen(point)` and `scene.raycast_visible` answer "is this in view and not behind a wall" without mirroring the camera in game code or hitting hidden templates (surveyor F18).
- Scenario `explore.goals` takes `[x, y, z]` points, so a list of points walks a scripted route (surveyor F17).
- `scene.set_text_in_font` and `text_texture_in_font` draw text in a TTF or OTF font from the game (unclaimed F11).
- `scene.seed()` reads the run seed (unclaimed F5).
- `perf.render.lights` counts the lights a frame uploaded and `budgets.max_lights` limits it (unclaimed F6).
- `perf.environment` reports `cpus` and `load_average`, and a timing budget failure on a busy machine says to rerun it alone (surveyor F20).
- `rusting docs search resolution` (or `render size`, `1080p`) finds `capture_size`, the headless render size (surveyor F12).
- `scene.edit_texture(handle, |texture| ..)` draws into a created texture while the game runs; `TextureAsset`, `TextureColorSpace` and `TextureSampler` are in the prelude (surveyor F14).
- `scene.spawn_copy_at_root(template, name, transform)` spawns a copy with no parent and a full transform (surveyor F4, F6).
- `scene.set_player(name, |pc| ..)` edits a player controller's speeds and other settings from game code (surveyor F9, unclaimed F7).
- `rusting test --json` reports `perf.entities_max`, the most live entities after any tick, and `budgets.max_entities` limits it (surveyor F8, F13).
- `rusting run --bench` reports `cpu_p50_ms`, `gpu_p50_ms` and `bound` (`"cpu"` or `"gpu"`), and guide/look-and-feel says which cuts help which side; LOD only helps a GPU-bound frame (FOREVER BEAR F49).
- `rusting test <project> <scenario> --exe <game>` runs a scenario against an already built game, such as an export, from the game's own folder without cooking or building (FOREVER BEAR F48).
- `rusting scene inspect` lists every entity's id and name under `entities`, so `--fields` and `--limit` apply to it; `scene map` help says an empty `maps` list means no tile map.
- Scene patch `set`, `remove`, `reparent` and `delete` accept `name` for the entity as well as `id`, and the project AGENTS.md shows an example (surveyor F1, unclaimed F1).
- `scene.window_focused()` is false while the game window has lost focus (alt-tab), so a game can pause itself, and the scenario step `{"focus": false}` simulates it (FOREVER BEAR F46).
- `rusting new --template twin-stick` creates Swarm, a twin-stick shooter with game code: move with WASD or the left stick, aim and fire with the arrows or the right stick, and defeat a wave of twelve enemies. Its `tests/wave.json` plays the first wave.
- `rusting new --template tower-defense` creates Outpost, a tower defense game with code: build towers on four pads with keys 1 to 4, earn gold from defeated enemies, and hold the base against a wave of ten. Its `tests/defend.json` plays the wave.
- `rusting new --template roguelike` creates Crypt, a turn-based roguelike with code: each of three floors is laid out from the run's seed, walking into an enemy defeats it, and enemies next to you strike. Its `tests/descend.json` plays a run on seed 1.
- `rusting new --template card-game` creates Duel, a card battle with code: play Strike, Guard and Heal cards from a seeded deck with keys 1 to 3 or the card buttons, against a foe whose next attack shows in advance. Its `tests/duel.json` plays a duel on seed 1.
- `rusting new --template rhythm` creates Beat, a rhythm game with code: hit notes in three lanes with D, F and J as they cross the line, scored PERFECT or GOOD by timing. Its `tests/song.json` plays the song.
- `rusting lint` warns `LINT_NO_ENDING` when a scene keeps counters but nothing can end a round: no counter has a target and game code never calls `counter_complete`, `load_scene` or `quit`.
- The agent skill lists the pitfall catalog's entries by symptom, generated from `docs/pitfalls.md`.
- The agent skill lists every scene component with its one-line summary, generated from the schema and checked by the same test.
- The API table in the agent skill (`SKILL.md`) and in a new project's `AGENTS.md` is generated from the API index and lists every `GameScene` call with its parameters; a test fails when it goes stale, and `RUSTING_UPDATE_GOLDEN=1` rewrites it.
- New 2d, first-person, third-person and sandbox projects ship a passing scenario (`tests/run.json`, `tests/walk.json` or `tests/drop.json`), so `rusting test` works on every template.
- `GameScene::add_class(name, class)` puts an object, such as a copy spawned from a classless template, in a class.
- `rusting new --template racing` creates Circuit, a racing game with game code: drive three laps through the checkpoints in order; off the road the car is slow. Its `tests/lap.json` drives a lap.
- `rusting new --template top-down` creates Arena, a top-down action game with game code: move, attack the enemies that chase you, and defeat all three before taking three hits. Its `tests/fight.json` plays a winning round.
- `rusting new --template puzzle` creates Box Push, a grid puzzle with game code: push every box onto a goal. Its `tests/solve.json` solves the level.
- `rusting docs show cookbook/<name>`: tested snippets for common tasks, starting with an enemy that follows the player and a level select.
- The agent skill now lists every `GameScene` method, and a test fails when a guide misses one or names a method that does not exist.
- `rusting docs show guide/pitfalls`: a catalog of mistakes games have made, each with the diagnostic code that catches it and the fix.
- `rusting lint` warns `LINT_TEXT_SMALL` for HUD text under 14 px and `LINT_TEXT_OFFSCREEN` for HUD text anchored outside a 1280 x 720 view.
- `rusting lint` warns `LINT_GOAL_INSIDE` when a pickup, goal or other sensor starts with its centre inside solid geometry, where the player can never reach it.
- `rusting lint` warns `LINT_CAMERA_INSIDE` for a camera inside a capsule collider too, and no longer for a camera inside any player body.
- `scene.dash(name, velocity, seconds)` dashes a player or platformer controller, and `rusting recipe apply dash` adds a dash on Q with a cooldown, completing the recipe list.
- `scene.playing_sounds()` lists the sounds started and not yet ended with clip, bus and paused state, and `scene.pause_sounds(bus)` / `resume_sounds(bus)` pause and resume one bus or (with `None`) every sound. The `pause_menu` recipe pauses sounds with the game.
- Sound pitch and speed: play a sound at any rate, change it with a fade, and see it in the playing list
- Pause, resume and seek playing sounds; the playing list shows where each sound is
- Moving 3D sounds: move a sound or attach it to an object, and pick which object listens
- Bus effects without new libraries: low-pass, reverb and distortion on any volume group, with fades
- Voice limits and priorities: a thousand sounds at once stay within the limit, the lowest-priority and then quietest sounds give way, and the dropped count is reported
- Captions tied to sounds, with a settings toggle and text size
- Walls muffle sounds: a physics ray between listener and sound lowers the volume and the high frequencies
- Long music and ambience files stream from disk instead of loading whole
- Sounds can play backwards
- Game tests can compare the sound mix with a stored reference file
- Camera screens can update every few frames, switch off, and skip themselves when out of view
- CRT and VHS look for screens: scanlines, film grain, color bleed, a rolling noise band and wobble, plus film grain and color fringing for the whole picture; grain repeats exactly for the same tick
- Text on 3D objects: draw any text into a texture for signs, labels and monitor overlays
- Waypoint graphs: shortest path and nearest point for monsters that patrol
- Steam achievement and stat calls that do nothing yet, so games can call them today (see below)
- Render benchmark options for many instanced objects and camera screens
- Sample game `forever_bear_booth`: a night-shift booth with six CRT monitors, a shelf of pitched bear voices and a mascot that walks the aisles
- `BusEffect`, `Caption`, `WaypointGraph` and `AssetServer` are in the prelude
- `rusting check` warns `SCENE_COLLIDER_WITHOUT_BODY` when a collider has no `physics_body`, because physics, raycasts and `aim` skip such a collider
- A scenario fails when no tick finishes for 60 seconds, for example after a deadlock, and names the last finished tick, so `rusting test` no longer hangs (`RUSTING_TEST_STALL_SECS` changes the limit)
- The game binary warns when its cooked scene is older than the scene file, so `cargo run` after a scene edit no longer plays the old level silently
- `camera_screen` `exposure`: brighten or darken one monitor's feed without changing the scene's lights or the player's view
- The `dark_interior` preset's moonlight is bright enough to see (30000 lux instead of 3000), and the docs explain how lux, point light intensity and ambient intensity compare
- Game code can draw text onto an object (`scene.set_text`) and put any material on one (`set_material`, `create_texture`); `docs search` snippets show the line that best matches the query
- `asset reimport` by path registers a file under `assets/` that has no `.rmeta` yet, such as a model added by `scene add-model`, instead of failing with ASSET_NOT_FOUND
- The docs list every built-in primitive's size and axis (`docs/look-and-feel.md` "Mesh kit" and the `mesh` schema entry)
- `rusting schema` and `rusting explain PATCH_JSON` show a full `create` patch with a parent, built-in sections and a component
- Scenario `expect_screen` and pick checks are much faster on large scenes: a check on a 3,895-entity scene takes about 0.3 s instead of 15-20 s
- Game code can zoom a camera: `scene.set_camera_fov(name, radians)` and `scene.camera_fov(name)`
- `rusting.player_controller` has `pitch_limits` and `yaw_limits` for seated and turret views
- `rusting check` reports a CLI built before the latest engine source edit once a day per project instead of on every run
- The `restart` and `load_scene` docs say that playing sounds carry on and how to stop them first
- Scenario `greater_than` and `less_than` compare an array's length, so `/playing` can be counted
- `audio:/playing` shows each sound's `[left, right]` `gain`, and the audio docs give the pan law and warn that a named listener does not turn with the camera
- The camera docs explain clicking with `aim` while mouse look holds the cursor
- The camera docs say where a player controller's eye sits in first and third person
- `rusting schema NAME` prints one component, operation or section of the catalog
- `rusting docs show scenario` lists every scenario file field and step kind on one page
- The scenario and camera docs show turning a player's view with `set` on the controller's `yaw` and `pitch`
- The `audio:` scenario probe reports each bus's own `level` and `peak` under `/buses/<bus>`, measured after the bus's effects and volume
- `RUSTING_PERF=1` adds the p50, p95, p99 and largest frame time of each second to its `[rusting] perf` line
- `project.json` takes `"window": [width, height]`, the window size a game asks for when it runs from the project folder
- `perf.render.cameras` in a test report also lists each camera screen drawn that frame, with its GPU time, draws and triangles
- `rusting run --bench FRAMES` measures that many windowed frames after a 60-frame warm-up, closes the game, and reports mean, p50, p95, p99 and max frame time
- `set_color` and `set_emissive` no longer scan every material or keep one material per eased value; easing 900 objects a tick drops from about 0.5 ms to 0.35 µs a call
- A scenario check on a counter nobody created yet reads it as 0, as game code does, instead of failing with "no entity"
- A CLI usage error names the flag the command does not take and prints that command's usage
- `rusting test --keep-going` runs every step after a failed check, and the failure message lists every failed step
- The audio guide says which sound loses when a bus is full: priority within that bus only, then the quietest after distance and occlusion, ties replacing the oldest
- `rusting test` reports `perf.render.gpu_ms_p50`, `gpu_ms_p95`, `gpu_ms_max` and `gpu_frames` over every offscreen frame, for GPU timing without a window
- `rusting_game!(update, tick: tick)` adds a function called once per fixed tick; `scene.pressed` inside it sees presses made on frames that ran no tick (`run_game_with_tick` for custom scene paths)
- `GameScene::restart` docs say scene counters reset, `set_counter` counters are dropped and `fixed_tick` keeps counting, with an example that carries a value across; `set_paused` docs say `fixed_tick` and `elapsed` stop while `frame` counts on
- Ray hits on ragdoll bodies report the bone's name, and `raycast_skipping` skips them by the bone's classes (new `RagdollPart` component links a body to its bone)
- An active ragdoll holds each unkeyed bone field (position, rotation, scale) to its starting pose, so a clip that keys only the hips' position no longer leaves them turned
- `GameScene::reset_ragdoll(name)` drops a ragdoll's bodies and restores its animated pose, for a teleport without trailing limbs; blending back from limp also restores unkeyed bone fields one by one
- `rusting docs show api/AnimationEvent` lists the event's fields; `guide/animation` names them too
- `guide/look-and-feel` no longer claims skinned models import as a still pose
- `scene add-model` suffixes repeated or taken node names (`slice 2`) instead of failing, and a taken root name says to pass `--name`
- `GameScene::set_exposure` and `GameScene::set_mouse_look` set exposure and free or recapture the cursor without `world()`
- `scene add-model` help and `guide/look-and-feel` say a model's license and author live in the `.rmeta` from `asset import`
- `GameScene::set_field(name, path, value)` sets any scene field by the scenario `set` JSON pointer, including registered components such as `rusting.fog`
- `guide/menus-and-ui` explains why `Color32::from_white_alpha(8)` is grey 50 and how to draw a faint overlay
- `rusting capture --at X,Y,Z` with `--look-at X,Y,Z` or `--look YAW,PITCH` shoots from any point without a camera in the scene.
- A scenario with captures, and `rusting capture --tick N`, render only the few ticks before each image instead of the whole run, so long runs with a late capture are no longer slow throughout. Scenes with GPU bodies still render every tick.
- `rusting docs show api/WaypointGraph` documents the waypoint graph's fields and methods.
- `--stderr` on `rusting run` and `rusting test` streams game output while the game runs.
- New `guide/lighting` docs page: shadow support per light type, the light cap per quality and which lights are dropped first. Scenario perf reports include `dropped_lights`.
- `rusting.player_controller` gains `crouch_height` and `crouch_multiplier`, and a `player.crouch` action bound to C, left Ctrl and pad East. Crouching is off by default (`crouch_height` 0). `PLAYER_ACTIONS` now has seven entries.
- Scenario `audio:` checks read a clip that never played as 0 plays at `/clips/<clip>`.
- `asset import --to assets/sounds` now means `assets/sounds` instead of `assets/assets/sounds`.
- New docs pages `api/GpuBodySettings`, `api/GpuConditionShader` and `api/MaterialAsset`, and a note on friction and jamming in dense GPU piles.
- Scenario checks read one entity instead of capturing the whole scene each tick, which speeds up long runs of big scenes. `perf.wall_ms_mean` gives the whole run per tick.
- `rusting_game!(update, components: [Night => "game.night"])` registers a game's own scene components, and the prelude re-exports `Component`, `Serialize` and `Deserialize`, so game state can leave counters without adding crates.
- `spot_light` gains `shadows` (default false): a shadowed spot light stops at walls, so a flashlight no longer lights the far side of a door. One light per frame casts shadows, and a shadowed directional light wins.
- New `rusting.post_volume` component: fog and color grading on its object apply only inside its box, blending over `blend` metres, so a foggy hall and a warm office can share a scene.
- A game whose cooked scene was made by a `rusting` CLI from another engine build now warns and loads the source scene instead of failing with a bincode decode error (dev projects only).
- The look-and-feel guide documents mesh level of detail (`.rlod` files beside a mesh).
- Scenarios and `rusting capture` with GPU bodies no longer draw a frame every tick: ticks without an image step only advance GPU physics, with the same state hashes, so long GPU runs finish sooner.
- The lighting guide explains light brightness units, the art presets' values and which way directional and spot lights point.
- GPU contact-grid overflow counts appear in `rusting test --json` perf and in the `RenderCapacityDiagnostics` resource during a game run; the concepts manual explains why crowded cells slow every GPU body.
- The concepts manual has a "Many bodies" section: triangles per sphere `subdivisions` value, the cost of `state_hash` and where to read CPU and GPU time.
- A settled GPU pile stays asleep: sleeping bodies ignore contact pushes below half a millimetre, so a deep pile no longer creeps, spreads and wakes up again.
- `rusting docs show api/BusEffect` lists the bus effects and their fields, and `api/GameScene::press_tick` names the scenario `at` field.
- `expect_pixels` takes `differs_from` with `difference_min` or `difference_max` to compare a region with an earlier capture.
- Cylinder and cone caps show a texture as a disc, a Plane shows the whole texture, and the mesh kit docs say how each primitive maps textures and that children inherit their parent's scale.
- Invalid-object patch errors name the operation at fault, and a short color says it needs 3 or 4 numbers.
- `scene.raycast` is about 20% faster without skip classes, and the concepts guide gives the cost per ray.
- `rusting asset reimport` accepts a path relative to `assets/`, as `asset import --to` names it.
- With `keep_going`, a failed scenario's game error says how many more checks failed, and docs/concepts.md documents `RUSTING_KEEP_GOING`.
- The sides of `Cylinder`, `Cone` and `Capsule` show a texture's full height with its top row at the top, as `Sphere` does. A cylinder side showed only the middle fifth of the image before; a textured capsule was upside down and now appears flipped compared with earlier builds.
- A scenario `click` with `index` counts copies of a text drawn within 4 px of each other, such as a drop shadow, as one place.
- The `budgets` schema text no longer says draw and triangle limits need a capture step; it and docs/concepts.md say when `perf.render` is filled.
- docs/determinism.md explains that an unrelated scene edit can move a chaotic physics result, because bodies solve in `Entity` order.
- `GameScene::counter_or(name, default)` reads a counter that may not exist yet without the missing-counter warning.
- The missing-counter warning lists close counter names ("did you mean `money`?") when the name looks like a typo.
- `set_color`, `trigger`, `spawn_copy` and the other name-taking setters that return nothing warn once, with the nearest object names, when the name does not exist.
- docs/concepts.md "Build times" states the 3 s edit-to-diagnostic budget and shows how to share one `CARGO_TARGET_DIR` across games so the engine builds once.
- `rusting test <project>` saves its results to `build/test-results.json`, and the editor's Agent area shows them in a new Results tab.
- The editor's Agent area can pause agent edits; an outside scene write that arrives while paused or while the scene has unsaved edits waits as a pending row with Accept (undoable) and Reject. Accept applies only the revision shown; a newer write replaces the pending row.
- The Agent area's pending row lists each touched entity with its changed fields and its own Accept, which applies that entity alone behind one Undo snapshot.
- The Agent area's pending diff compares the outside write with the scene file as last loaded or saved, so unsaved editor edits no longer show as agent changes.
- The Agent area's pending row lists entities an outside write removes, each with an Accept that deletes it and its descendants behind one Undo snapshot.
- `rusting test <project>` saves each scenario's tick time, draws and triangles in `build/test-results.json`, and the Agent area's Results tab shows them.
- `rusting test --without COMPONENT` runs a scenario with that component left out of every scene and passes only when the scenario then fails, or fails with `SCENARIO_TOO_WEAK`.
- Scenario reports carry `coverage`: the entities and scene sections a run changed, added or removed, the sections it never touched, the actions pressed and trace events by kind.
- `rusting bisect <project> <other-root>` runs two copies of a game headless and names the first tick and entity whose state hashes differ; divergent entities now pair by scene ID, also in `rusting determinism`.
- `rusting fuzz <project> <scenario>` runs a scenario over many seeds with random action presses and writes the first failing seed to `build/fuzz/seed-N.json` as a ready scenario; scenarios accept a `fuzz` section and reports list `fuzz_steps`.
- `rusting run --record FILE.scenario.json` saves the windowed session as a scenario: press, release and tap steps for the named actions the player held, so a human can hand an agent a bug as a test.
- Scenarios take an `explore` section: an explorer bot walks the `PlayerController` to each goal (every sensor by default), jumps when stuck, and fails naming the goals it could not reach; the report lists them as `explore`.
- `rusting lint [project]` checks the main scene for a player body outside 0.5 to 3 m tall, zero scale axes, and lights that can never light anything, as `LINT_*` warnings.
- `rusting lint` warns with `LINT_CAMERA_INSIDE` when a camera starts inside a solid box or sphere collider.
- Spawning an object under a taken name still panics, but the message now says names are unique and points at the usual cause: the scene file or its template already has an object of that name (playplace friction 3).
- `rusting new --template empty` creates a project whose scene holds only a camera, so game code can spawn objects under any name (playplace friction 3).
- `rusting inspect --tick N` reports `feel` for player and platformer controllers: speeds, jump apex height and time, air time and jump distance, compared with genre ranges.
- `rusting.camera_shake` and `scene.add_trauma(name, amount)` shake a camera's drawn view by trauma squared, fading by `decay` per second without moving its `Transform`.
- `rusting.squash` and `scene.squash(name, amount)` squash or stretch an object on a damped spring that keeps its volume and returns to the rest scale.
- `rusting lint` warns `LINT_LIGHT_BUDGET` for each visible light past the scene quality's light budget (Eco 16, Balanced 32, High and Auto 64), which the renderer drops.
- `GameScene::count_gpu_bodies_in_box(class, min, max)` counts the GPU bodies of an object class inside a box, from their latest `request_gpu_class_snapshot` mirrors.
- `rusting asset generate <root> sfx "coin 7"` synthesizes a seeded sfxr-style sound (jump, coin, hit, explosion, laser, powerup or blip) and imports it as a CC0 WAV, with no hook to set up.
- `rusting.spawn_grid` copies an object and its children onto a grid when the game starts, so a scene can hold hundreds of balls without game code.
- `GameScene::gpu_command(name, command)` moves, pushes or reads a GPU body from game code, and `restart` now puts every GPU body back at its scene-file pose.
- Scenarios read `"entity": "class:ball in -5,0,-5 5,10,5"` for a class's member count, the bounds of its GPU bodies and how many lie inside the box.
- `rusting.flash` and `scene.flash(name)` tint an object and its children for a moment on a hit, without touching the shared material.
- `scene.hit_stop(seconds)` freezes a windowed game for up to a second of real time on a heavy hit; fixed ticks then carry on unchanged, so simulation results and headless runs are not affected.
- Clip paths `sfx:<preset> [seed]` (for example `sfx:coin 7` in `play_sound` or `rusting.sound_cue`) play a built-in synthesized sound with no file, and validation no longer reports them as missing assets.
- `rusting lint` reports `LINT_COLLIDER_MISMATCH` when a solid collider is more than twice or less than half the size of its entity's built-in mesh on any axis.
- `rusting recipe list` and `rusting recipe apply <root> <recipe>` write a gameplay recipe into a project: its source as `src/<recipe>.rs`, its objects into the main scene as one patch, and a passing scenario as `tests/<recipe>.json`. The recipes are `checkpoints`, `health` (hazards cost a point, a short safe window follows, 0 restarts the round), `double_jump` and `inventory` (items in class `item` count into `inventory_<kind>` counters; a key opens an object in class `locked`), `day_timer` (a `day` counter that goes up every 30 seconds, with a HUD clock), `pause_menu` (Escape pauses and shows Resume and Quit HUD buttons), `wave_spawner` (each wave copies a hidden enemy template, more each time, once the last wave is gone) and `turret` (shoots the nearest visible enemy in range on a cooldown).
- `GameScene::clicked()` names the HUD buttons clicked since the last frame, so game code reads them without the event queue.
- `rusting run --seed N` starts the game's random streams from N instead of 0.
- `air_jumps` on `rusting.player_controller` and `rusting.platformer_controller` (default 0) allows that many extra jumps before landing; 1 is a double jump.

### Fixed
- A caption stays up for its whole time when its clip is shorter; it used to vanish when the clip ended.
- HUD buttons work from the keyboard and gamepad: Tab or the first d-pad press used to focus the HUD's own area (or a caption) instead of a button, so Enter or South clicked nothing. HUD buttons now take the focus in reading order, top to bottom and then left to right, instead of in scene-id order.
- The runtime HUD always uses egui's dark theme. It used to follow the desktop theme, so a light desktop gave buttons a light fill under white text.
- A patch that puts `{"$asset": path}` in an inline material's texture slot now says to write a plain path string, and `rusting schema --json` explains that the `asset_types` form differs from inline scene materials.
- The lighting and look-and-feel guides state the light budget per quality level (64 High, 32 Balanced, 16 Eco) and which level `Auto` picks.
- `rusting preset apply` puts ambient, sky, tone mapping, grading and background on a new `Environment` entity instead of the sun, and `--only environment` applies them without touching the sun.
 A scenario keeps its whole audio mix only when it has `audio_out` or `audio_reference`, so a long soak no longer grows by 384 KB per second of game time (FOREVER BEAR F47).
- An exported game whose cooked scene was made by a different engine build loads its source scene from `scenes/` with a warning, as a dev project does; with no source, the error names the cooked file and says to recook it with a CLI built from the game's engine (FOREVER BEAR F45).
- `rusting export` copies `project.json` into the export, so the game finds `assets/` (and its window size) from its own folder instead of looking next to `build/` (FOREVER BEAR F44).
- A headless `rusting run --ticks` clears the input edges after each update, as the windowed loop does, so a key game code presses is "just pressed" for one update, not every update after (FOREVER BEAR F43).
- `rusting run --replay` no longer reports a divergence at tick 9 on every windowed recording; the state hash no longer depends on entity ids that only the windowed runtime shifts.
- Scenario screenshots and goldens now blend the runtime UI in gamma space, as the game window does; before, a translucent egui fill looked about half as dark in screenshots. Goldens with translucent UI may need regenerating.
- A paused sound's caption no longer stays on screen; it hides until `resume_sound`.

- The new screen and text goldens failed on the software renderer (lavapipe); they now pass there and on the RTX 3060
- Each camera screen ran the GPU physics again; screens now reuse the frame's physics, so six screens cost about 3 ms instead of 113 ms
- Parallel `rusting run`s of one project no longer fail at random with `SCENE_IO` "No such file or directory" while cooking the scene; atomic writes no longer share one temporary file name.

### Changed

- `rusting_core::schedule::CpuFrameTimings` has new `update` and `post_update` fields. Migration: a struct literal needs `..Default::default()`. The `RUSTING_PERF` line says `CPU fixed` where it said `CPU physics`.
- A `CLI_OUTDATED` warning about newer engine source lists the engine commits made since the CLI was built (surveyor F3).
- `rusting test` with one scenario leaves `scenario.state_hashes` and `scenario.gpu_state_hashes` out of its result. Migration: pass `--full`, or read build/scenario-report.json (unclaimed F10).
- A scenario `capture` whose `camera` is the camera the game already shows alone keeps the HUD; before, any named camera dropped it (unclaimed F12).
- `Explore::goals` is `Vec<ExploreGoal>`. Migration: `"Coin".into()` still builds a name goal; match `ExploreGoal::Entity(name)` where the code read the string.
- A scenario `set` of `/transform/rotation` on an entity with a player controller sets its `yaw` and `pitch`; before, the controller overwrote it on the next tick (unclaimed F9).
- `resource_state_hash` takes `&mut World` instead of `&World`; pass the world mutably.
- Commands that write project files now create `.rusting/` (operation journal, content blobs, leases) in the project. It holds its own `.gitignore`, so git ignores it with no project change.

### Performance

Measured with `render_bench` on an RTX 3060, 1920x1080, balanced quality, 600 frames. Times are mean and 95th percentile frame time.

- Six 320x180 camera screens, with the base scene's 1,000 GPU bodies: 134.91 / 151.07 ms before, 24.86 / 26.48 ms after (the base scene alone is 22.18 / 24.54 ms, most of it GPU physics).
- Six 320x180 camera screens without bodies: 5.24 / 6.11 ms (scene alone 3.06 / 3.60 ms).
- Instanced bears, without bodies: 5,000 at 7.40 / 8.20 ms; 5,000 with 500 moved per frame at 8.81 / 9.36 ms; 10,000 with 500 moved at 12.37 / 13.38 ms.
- 5,000 bears, 500 moved per frame and six screens: 16.47 / 18.96 ms; with screens updating every other frame, 12.37 / 13.47 ms.
- The same bear counts with 1,000 GPU bodies: 25.55 / 27.59 ms (5,000), 30.77 / 33.16 ms (10,000 with 500 moved).
- GPU physics: a body too big for one contact-grid cell (the benchmark's ground) tested every other body on a single GPU thread, four times a step, and stalled the whole physics pass. Each such body now gets a workgroup of 256 threads. The benchmark's physics pass fell from 20.83 ms to 1.4-1.8 ms of GPU time, and the base scene with 1,000 bodies from 22.18 ms to 5.63-6.27 ms mean frame time. Body positions and velocities match the old code bit for bit on the RTX 3060 and on lavapipe.

### Needs owner approval

- `steamworks` crate, for the real Steam backend behind the new `steam` feature.
- `cpal` as a direct dependency, for microphone level input (today it is only pulled in by kira).
- A video decoder crate, for video on textures.

### Not done

- Microphone input and video on textures wait on the approvals above.
- The Windows build was checked under Wine only: it builds, opens a window, renders on the GPU and opens the sound device. Gamepads, the save folder and real Windows hardware are not checked yet.

---

## [2.0.3] - 2026-10-05

### Added

- Animation: keyframe clips that move, turn, scale, recolor or hide objects, with smooth blending between clips
- Timeline in the editor: play, scrub and edit keys, with a record mode and full undo
- Animated and skinned glTF models: bones, blend shapes and their clips now import and play
- Animation state machine and blend spaces: switch and mix walk, run and other clips from game code
- Animation layers: play a clip on top of another, like waving while walking
- Root motion: walk clips move the character instead of sliding in place
- Copy animations between differently built skeletons
- IK: heads look at targets, hands and feet reach for things, feet stay on slopes and stairs
- Ragdolls: characters go limp when hit hard and get back up, or stay physical and stagger when pushed
- Particles: a new emitter with eleven ready presets (dust, leaves, snow, rain, sparks, smoke, fire, embers, fireflies, sparkle, confetti)
- Particle editor in the Inspector with a live preview
- Color grading and vignette, plus two new art presets: dark interior and bright stylized
- Camera screens: show what another camera sees on a mesh, like CCTV monitors
- Rounded cubes and capsules, and a ready-made simple character
- Add a downloaded glTF model to a scene with one command
- Skeletons show in the editor; click a bone to select it
- New guides: Animation, Effects and Look and feel

### Changed

- Cylinders and cones look smooth

### Fixed

- Spheres were drawn inside out and looked badly lit
- Adding a model to a scene no longer fails in some cases

---

## [2.0.2] - 2026-10-03

### Added

- Much better sound: panning, 3D sounds that follow objects, fades, music ducking and separate volume groups
- Menus: buttons, settings screens and pause menus made with egui, usable with mouse, keyboard or gamepad
- Gamepad support for moving, looking and menus
- Saves: games can save and load files, pause and quit
- Rebindable keys
- Split screen and picture-in-picture, with HUD for each player's view
- Video settings: resolution scale, pixelated look, vsync, FPS cap, fullscreen and window size
- Rain and other particles that run on their own, no code needed
- Player can push crates, turn to face where it walks and use an over-the-shoulder camera
- More control over GPU physics from game code: see where bodies are, feed values to your own physics shaders and combine checks
- Rhythm helpers: turn a tempo into game ticks and know exactly when a key was pressed
- Game tests can click menu buttons, check screenshots and colors, listen to the sound mix, check save files and restart the game
- Record a play session and replay it to check nothing changed
- Same game, same result: easier checks for GPU physics and for whole test runs
- New guides: Audio, Menus and UI, Cameras and custom GPU physics shaders

### Changed

- Tests show more useful output when something fails
- Scene edits from the command line can run twice safely and update only some fields
- Projects get warned when their agent guide is out of date, and `rusting fix` updates it

### Fixed

- Piles of GPU balls now settle and sleep instead of shaking forever, and no longer squeeze through wall corners
- Spinning GPU bodies come to rest after landing
- Barrels and other round objects roll properly
- Player no longer falls through a platform it starts slightly inside
- HUD text no longer fades in or slides off the screen edge
- Clearer error messages and fuller docs pages
- The command-line tool no longer crashes when its output is cut short
- No more extra full engine rebuild between check and run

---

## [2.0.1] - 2026-10-01

### Added

- Sound! Games can now play, loop and stop sounds and change the volume
- AI agents can now work with the engine directly: run commands, attach to a running game, step it and look inside
- New Agent panel in the editor that shows what an agent changed in your scene, and you can still undo it
- Much stronger game tests: check what is on screen, catch broken values every tick, measure performance and get screenshots with labels
- New helper commands: compare two scenes, see a scene as a map, jump the game to any moment and inspect it, create a test or system in one command
- Built-in offline docs and clear explanations for every error, with hints on how to fix common code mistakes

---

## [2.0.0] - 2026-09-31

### Added

- Water for seas, lakes and rivers with waves and currents, objects float in it
- Small splashing fluids drawn as one smooth surface
- Glass that bends and blurs what is behind it
- Reflections on smooth surfaces, sky images for lighting and reflection probes
- Fog, bloom (glow) and ambient occlusion (soft shadows in corners)
- Editor got a big visual refresh: new Add Component picker with descriptions, new Project Settings page, nicer Hierarchy, Assets and Console
- Tile Painter for drawing levels on a grid, with rectangle, line and fill tools
- Blender-like camera views on Numpad, orthographic view and snap settings
- Every shortcut can have a second key
- Physics joints (hinges, sliders, springs and more), joints that can break and robot-like linked bodies
- Scenes inside scenes (like prefabs), scene variants and overrides
- Reload Code while the game is running, without restarting the scene
- Optional scripting with WebAssembly
- New project templates: first-person, third-person and physics sandbox
- Many new sample games: platformer, brick breaker, snake, mini golf, stealth, shooter and more
- A lot of new simple functions for game code (input, raycasts, counters, colors, random, level loading and more)
- Players ride moving platforms, and the third-person camera no longer goes through walls
- Materials have names and textures can repeat on long floors

### Changed

- Rendering is much faster: fewer draw calls and higher FPS on Wayland
- Transparent materials now work as clear glass
- When the game can't keep up, it slows down instead of freezing

### Fixed

- A lot of physics fixes: fast objects, players on ledges, sleeping bodies, exact replays
- Textures no longer shimmer in the distance
- Textures on cube sides were upside down. If you flipped your images to fix it, flip them back
- Hidden objects and lights no longer show or light the scene
- HUD text no longer jumps to a new line when it gets longer
- Game no longer crashes when closing on Wayland

---

## [1.4.0] - 2026-09-27

### Added

- New `rusting` command-line tool: create, build, run, test and export games without opening the editor
- Edit scenes, take screenshots and write game tests from the command line
- Same game, same result every time: replays, fixed random seeds and checks that physics runs the same
- Play the scene right inside the editor with Pause and Step, no build needed
- Profiler panel with CPU and GPU graphs, plus Render Settings and Physics panels
- Better Scene View camera (orbit, pan, zoom, focus with F) and gizmo snapping
- Image and glTF import with reimport and import settings
- First-person player controller
- Game-feel kit: smooth animations, sounds and particles on events, pickups, score counters and in-game HUD with buttons
- Simple 2D: sprites, tile maps and a 2D starter scene
- Starter game template and a full sample level
- Choose which GPU to use
- Getting-started guide, concepts guide and four tutorials

### Changed

- Inspector now shows proper fields, drop-downs and pickers for every component instead of raw JSON
- Selected objects stay outlined even behind other objects
- GPU physics no longer drops objects when there are too many
- Old renderer code removed and editor guide rewritten

### Fixed

- Physics no longer skips or mixes up steps when the game lags
- Particle bursts no longer all look the same
- Scenes with renamed or old fields no longer lose data silently

---

## [1.3.0] - 2026-09-26

### Added

- Real Console panel with filters and Clear button
- Image previews in Assets, and images can be dragged onto objects
- Editor text size setting, saved automatically
- Fully customizable keyboard shortcuts for almost every editor action
- Engine can pick CPU or GPU physics for each object by itself
- Much better physics: real rotation, rolling spheres, stable box stacks, friction and bounce
- GPU objects now collide with each other, with CPU objects and with the level
- Accurate mesh colliders for level geometry
- Basic character movement that slides along walls and detects floor
- Raycasts and shape casts for checking what is in the way
- Custom GPU physics rules with your own shaders
- Fast objects no longer fly through thin walls
- Windows export from Linux and macOS
- MSAA anti-aliasing and sharper textures at an angle
- Detailed performance stats in the editor

### Changed

- Assets panel is now a file tree like in Godot, models can be double-clicked or dragged into the scene
- glTF import brings the whole model with meshes, materials, cameras and lights
- Dragging many selected objects in Hierarchy moves them all together
- Menus and popups now show correctly over the Scene View
- Physics modes renamed to simply CPU and GPU

### Fixed

- Stripes and triangles on lit surfaces (shadow acne)
- Camera movement no longer clicks or types into editor UI
- GPU stacks no longer slowly sink into each other
- Big editor slowdown when selecting huge meshes
- Wrong lighting on some glTF models
- GPU physics now gives the same result every run

---

## [1.2.0] - 2026-09-25

### Added

- Added a much more complete PBR renderer.
- Materials now support base color, normal maps, metallic/roughness, ambient occlusion and emissive textures.
- Added transparent and cutout materials.
- Added HDR rendering and tone mapping.
- Added environment lighting and support for multiple point lights.
- Added directional light shadows with quality settings.
- Added automatic visibility culling to improve performance in large scenes.
- The engine can choose between CPU and GPU culling depending on scene size and hardware.
- Added occlusion culling so objects hidden behind other objects can be skipped.
- Added LOD support for using simpler models at long distances.
- Added a clearer rendering pipeline with separate rendering stages.
- Added scene Quality and Culling settings to the editor.
- Added a full Material section to the Inspector.
- Materials can now be edited directly from the editor.
- Added Blender-style camera and light icons inside the Scene View.
- Cameras and lights can now be selected directly from their viewport icons.
- Added editable render bounds for controlling object culling.
- Added culling statistics to the editor.
- The editor now remembers the camera position and selected objects for every scene.
- Asset reload success and errors are now shown in the Console.

### Changed

- Material shader settings were simplified into PBR and Unlit material types.
- Scenes now save their rendering quality and culling settings.
- Older scenes still load correctly.
- The editor rendering system was reworked to give the engine more control over the UI.
- File dialogs now run separately, so the editor no longer looks frozen while they are open.

### Known issues

- Image files manually added as normal, metallic or occlusion textures may use the wrong color space. Textures imported through glTF work correctly.

---

## [1.1.3] - 2026-09-24

### Added

- Added asynchronous asset loading.
- Assets can now load without freezing the main engine thread.
- Added automatic asset hot reload when project files change.
- Broken asset reloads keep the previous working version instead of breaking the scene.
- glTF import now supports full model hierarchies.
- glTF cameras and lights are now imported.
- Added better glTF material and texture support.
- Added tangent generation for models that need it.
- Added transparent glTF materials.
- Scene files now save hierarchy, transforms, renderers, lights, physics and editor data.
- Renderer resource management was heavily improved for smoother frame rendering.
- Added automatic GPU buffer growth for larger scenes.
- Added GPU capability detection and rendering quality profiles.
- Major editor redesign inspired by Blender and Godot.
- Added a new dark editor theme.
- Added a better Inspector with easier editing of positions, rotations, colors and custom components.
- Added Hierarchy search.
- Added object type icons, visibility controls, inline rename, multi-selection and context menus.
- Added adjustable editor UI scale.
- Added Blender-style Scene View orbit, pan, zoom and focus controls.
- Added new editor design and roadmap documentation.

### Changed

- Core engine systems such as time, input, hierarchy and events were moved into the shared core module.
- glTF support can now be disabled when it is not needed.
- Engine camera and scene handling was simplified internally.
- Engine architecture and roadmap were rewritten around the new editor and physics direction.

---

## [1.1.2] - 2026-09-15

### Added

- Added Vulkan GPU selection with `RUSTING_VULKAN_DEVICE`.
- Engine startup now shows which GPU is being used.
- Added clearer errors when a requested GPU cannot be found.
- Added headless Vulkan support for rendering and compute without opening a window.
- Added offscreen rendering support.
- Added GPU image and buffer readback tools.
- Added headless game execution for automated tests and servers.
- Added optional GPU tests.
- Added reusable GPU testing tools.
- Added automatic rendered-image comparison tests.
- Added better Vulkan debugging information.
- Started moving shared engine types into the new `rusting-core` crate.

### Changed

- Loading broken or missing glTF models now returns a proper error instead of crashing.
- Loading broken or missing textures now returns a proper error instead of crashing.
- GPU layout tests now detect shader changes automatically instead of relying on manually written values.

---

## [1.1.1] - 2026-09-12

### Added

- Added gameplay click events for selecting objects with the mouse.
- Added collision events.
- Added runtime keyboard, mouse and cursor input.
- Added named input actions, making gameplay controls easier to manage.
- Input system is prepared for future gamepad support.
- Added shared object picking for gameplay.
- Added scene unloading without needing to immediately load another scene.
- Improved glTF material import.
- glTF models now correctly import base color, normal, metallic/roughness, occlusion and emissive textures.
- glTF textures now use the correct color space.

---

## [1.1.0] - 2026-08-31

### Added

- Added new shapes and light to "Add Object"
- Now creating multiple light sources is possible

### Reworked

- Massive GUI update
- Custom icons
- Smaller everything to get more space
- Floating buttons on scene view
- Moved scene view setting to dropdown
- Recreated "Add Object", now its a modal window with categories
- Now every Object in Hierarchy is draggable to drag to other object and make it child of it
- And few other little changes

### Removed

- Frame count from Header

---

## [1.0.2] - 2026-08-30

### Added

- Interactive axis arrows for every transform, so you can move/scale/rotate object just by holding and moving mouse like in blender and other game engines
- Also shortcuts for transform, "G" for Move, "S" for scale and "R" for rotate.
- Axes choice, e.g. When you use Move, Scale or Rotate you can press "X" to use Transformation only for "X" axis, also you can press "Y" to transform only in X and Y axes in same time, no translate will allied to "Z".
- Added transform cancelation of current transform on "Secondary button"(Mostly right click) and "Escape"
- Some GUI buttons for Transform modes

### Changed

- Now fly mode active just by holding "Secondary button"(Mostly right click)

---

## [1.0.1] - 2026-08-27

### Added

- Scene view overlay
- Bound box for selected object in Scene view
- Axis arrows for selected object
- Shortcut system
- Move camera to selected object on "F" in Scene view
- FPS like camera fly on "Num 0" in Scene view

---

## [1.0.0] - 2026-08-26

### Added

- Added Project Manager with project creation, folder picker and recent projects
- Added scene New, Open, Save As, object creation, duplicate, rename, delete and parenting
- Added scene dirty state, safe confirmation and Undo/Redo
- Added Assets browser with file import, texture loading and glTF mesh assignment
- Added real Cargo Check, release Build and compiler output in Code Editor
- Added portable game export with executable, cooked scene, assets and license
- Added Linux and Windows CI and automatic GitHub release archives
- Added public contribution guidelines and a reproducible 10,000-body benchmark summary
- Added reusable responsive GUI elements with CSS-like style values
- Added modern editor theme with reusable hover, active, border, and shadow styles
- Added Debug and Release choices beside the editor Play button
- Added a reusable CSS-style ComboBox with a toolbar-aligned preset
- Added compact File, Edit, and View menus while keeping Play directly visible
- Added generation-checked physics IDs shared by ECS and future GPU readback
- Added typed Rust GPU-condition builders with comparisons, ranges, boolean logic, timers, collisions, sleeping state and custom values
- Added serializable GPU physics watch rules, event modes, payload selection and cooldown settings
- Added a fixed 48-byte GPU physics event ABI with safe routing back to live ECS entities
- Connected GPU Dynamic bodies to native-game compute gravity and GPU-owned render transforms
- Connected compiled GPU conditions to asynchronous fence-polled Rust gameplay events
- Added `GameScene::watch_gpu_object` and `GameScene::gpu_events` for concise game code
- Added multi-class object tagging and `GameScene::watch_gpu_class` so shared GPU rules affect only explicitly selected object classes
- Added unique scene-name validation for reliable single-object lookup
- Added `GameScene::once`, reusable cube spawning, and class-based GPU physics assignment for procedural 10,000-body scenes
- Added a directly runnable `hybrid_10k` native game example
- Added visible vertical and horizontal scroll areas to Code, Inspector, Hierarchy, Project, Console, and Assets panels
- Moved Cargo compiler and native game output from Code Editor into the Console panel so source editing keeps its full height
- Added migration support for version 1 cooked scenes created before GPU watch rules
- Added cached procedural `SphereSpawn` and `GameScene::spawn_sphere` for native Rust games
- Reduced high-instance runtime overhead by using ECS change detection for physics IDs, revision-based render extraction, cached swapchain frame resources, and fixed-tick-only GPU event readback

### Fixed

- Shaderc source builds now configure correctly with CMake 4 on GitHub's Windows runners
- Inspector values no longer leak from the previous object into a newly selected object
- Native ECS rendering now batches equal mesh/material objects into indexed instanced draws instead of recording one Vulkan draw per object
- The 10,000-body example aggregates event logs instead of printing thousands of terminal lines per frame
- Editor areas showing the same panel now use separate Egui IDs
- Custom ComboBoxes now keep separate popup IDs in repeated dock areas
- New Project now requires an explicitly selected parent folder
- Project switching now clears stale code and uses project-relative source and scene paths
- Project and scene save actions now have separate, unambiguous controls
- Toolbar dropdowns now open wider styled panels with aligned full-width actions
- Replaced editor icon-font symbols with portable text to prevent missing-glyph squares
- Grouped Hierarchy object creation into one styled Add Object popup
- Hierarchy now renders a real parent-first indented tree instead of grouping rows only by depth
- Hierarchy Rename, Duplicate, and Delete actions now live in each object's right-click menu
- Rename now edits the selected tree row inline with Apply, Cancel, Enter, and Escape controls
- Split the large editor module into view, dock, project, test, and GUI files
- Dock selection now follows clicks anywhere inside an area
- Dock content now keeps safe spacing from borders and neighboring areas
- Cargo Output now streams native game stdout, stderr, panics, and exit status
- Editor Play now cooks, compiles, and runs the real Rust game instead of only changing preview mode
- Scene loading now validates object IDs and hierarchy before replacing current scene
- Saved and cooked asset paths are now portable between project and export folders
- Old unversioned projects and scenes are now migrated safely

---

## [0.1.47] - 2026-08-25

### Added

- More settings in gui like: Game/Scene/Code modes
- Adding native Rust game projects that can be edited and built from GUI
- Adding simple Rust scene API to move and edit objects without ECS boilerplate
- Adding Blender-style editor areas that can be split, resized and changed to another panel type

### Fixed

- Gui design a bit improved

---

## [0.1.46] - 2026-08-24

### Added

- More settings and features in Editor GUI like FPS limit controls
- Little custom compiler from GUI scene to optimized no GUI game/simulation

### Fixed

- Rewriting architecture a bit to prepare for scaling

---

## [0.1.45] - 2026-08-24

### Added

- Added first version of ECS runtime with schedules, hierarchy and stable entity identities
- Added typed asset handles, render extraction and revision-aware GPU mesh cache
- Added first version of egui editor with hierarchy, inspector, camera settings, play controls and live 3D Vulkan viewport
- Added roadmap and architecture documentation for future engine implementation
- Added more tests for runtime, assets, GPU layouts and perspective projection

---

## [0.1.44] - 2026-08-24

### Added

- A lot of tests

### Fixed

- A lot of different fixes to improve stability before big implementation

---

## [0.1.43] - 2026-04-11

### Added

- Textures can be applied on engine build in shapes

### Fixed

- Now textures can be reused to save VRAM

---

## [0.1.42] - 2026-04-11

### Added

- A lot of docs for better user experience( description of functions/values on hover )

### Fixed

- Culling is now working much better, without bugs.
- Fixing multi gltf model import, now each texture and model render good even if there are 10k gltf models. But each texture is separate, so you cant reuse texture without vram loss, I will fix it so fast as possible.

---

## [0.1.41] - 2026-04-6

### Added

- Added gltf models import, already with Materials, Textures and everything that needed

---

## [0.1.4] - 2026-04-5

### Added

- Better code, now no warnings( before was like 60 )
- Added new very heavy fragment shader with noise and other things
- Culling toggle on 'C'

### Fixed

- Culling is working well and give insane performance boost on big scenes where fragment/vertex shader is heavy. But it uses object center, so some object might disappear earlier as needed. I will fix it in next patch

---

## [0.1.32] - 2026-04-5

### Added

- Added culling( when mesh is not in view => dont render ), but its beta, so its working bad

---

## [0.1.31] - 2026-04-4

### Added

- Just made code cleaner and only fixed some problems in shaders

### Fixed

- Grid collision shader

---

## [0.1.3] - 2026-04-3

### Added

- Optimizing physic shaders and fragment shaders render. Collision check was **O(n²)**, now it splitted on grid, so its **O(n*k) + O(n*j)** where k is objects count in cell and j is big objects count.
- adding more physic settings like **_friction_**, **_gravity direction_** and **_bounciness_**.

### Fixed

- Object collapse on stacking.

---

## [0.1.1] - 2026-04-1

### Added

- Collision types as enum (Sphere, Box).
- Optional apply one physic/visual shader for all object on scene

### Fixed

- Better performance( main loop refactoring )

---

## [0.1.0] - 2026-03-31

### Added

- Initial release!
- Different fragment shader support.
- Different physic shader support.
- Physics engine with per-object collision types (Box/Sphere).
- Same speed as pure Vulkano+winit
