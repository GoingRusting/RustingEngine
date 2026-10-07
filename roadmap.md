# RustingEngine Roadmap

RustingEngine is currently a functional Vulkan renderer prototype with a native Rust ECS runtime, an egui editor, and a GPU-accelerated hybrid physics bridge. The goal of this roadmap is to turn it into a complete, general-purpose Windows/Linux game engine in the same class as Godot — covering 3D and 2D rendering, animation, audio, UI, navigation, networking, a full editor, and export — whose defining strength is physics. RustingEngine should be the engine people pick *because* of its physics, and the best engine to build games with an LLM coding agent.

The roadmap covers these tracks:

- **The engine** (Milestones 0-23): Godot-class feature breadth, with physics as the flagship subsystem. Milestones 0-7 build the foundation and a vertical slice; Milestone 8 makes simulation deterministic; Milestones 9-23 reach feature parity with Godot while taking physics well beyond it.
- **LLM-native development** (Milestones L1-L10): the engine as the best place for an LLM agent to write, run, observe, test, and ship a game, proven by a published agent benchmark.
- **Engine expansion** (Milestones 30-35): gameplay framework, large worlds, cinematics, procedural content, modding, and XR, which a general-purpose engine needs beyond the Godot parity map.
- ***Sundering*** (Milestones 24-29): a deterministic, networked, destructible 5v5 competitive game built on the engine. It is the engine's hardest physics customer and its proof that the physics claims are real.

This document is the implementation source of truth. Tasks should be completed in dependency order, kept behind compiling intermediate states, and verified against the acceptance gates at the end of each milestone.

Long-lived ownership boundaries and dependency rules are recorded in [`architecture.md`](architecture.md). Roadmap work must preserve those boundaries or document a migration before changing them.

## Working agreement for implementation sessions

This roadmap lists outcomes, not tasks. Turning an outcome into a task is part of the work, not a reason to stop.

### How to pick an item

Pick the lowest-numbered unchecked item whose dependencies are satisfied. Milestones 2, 3, 4, and 5 are parallel tracks and may be interleaved. After Milestone 9, Milestones 10-22 are parallel tracks and may be interleaved, subject to the dependencies stated at the top of each milestone; physics milestones (10-12) take priority when two items are otherwise equally ready. The LLM-native milestones (L1-L10) and the engine expansion milestones (30-35) are parallel tracks too; while the owner's current focus is agent-built games, LLM-native items take priority over every other track. If an item is too large for one session, split it, implement the first part, and record the split in this file as sub-items.

### What counts as verification

Not every item is provable the same way. Match the proof to the item instead of treating "I cannot run the game" as a universal blocker.

| Kind of item | Required proof |
| --- | --- |
| Types, APIs, error handling, identity mapping | Unit test |
| ECS, assets, serialization, scene round-trip | Unit test, no GPU |
| Extraction, dirty ranges, ordering, capacity policy | Unit test on data structures, no GPU |
| Buffer layout, push constants, vertex formats | Generated/reflected layout assertion, no GPU |
| Shader source changes | Shader compiles in the build, plus a layout assertion |
| Compute dispatch behaviour, readback, physics results | Headless Vulkan test, software device acceptable |
| Rendered output correctness | Headless offscreen render plus golden-image comparison |
| CPU physics behaviour (stacking, joints, CCD, queries) | Deterministic headless unit/scenario test, no GPU |
| Editor panel behaviour | Unit test on editor state/commands; golden image for compositing |
| Audio mixing and DSP | Offline render to buffer plus sample comparison, no device |
| Frame pacing, throughput, memory growth | Hardware GPU run, cannot be verified in software |
| Determinism across vendors | Two different Vulkan implementations |

Only the last two rows genuinely require hardware. Everything above them is verifiable on a headless machine once Milestone 0.5 exists. Until Milestone 0.5 exists, GPU-behaviour items are blocked by missing tooling, not by the roadmap.

### When blocked

Do not stop with nothing delivered. In order:

1. Implement the part that is verifiable now, behind a compiling intermediate state.
2. Write the unverified part as a test marked `#[ignore]` with the reason, so the proof is ready when hardware is.
3. Add the blocker as a new unchecked item in this file, naming what is missing.
4. Move to the next item.

### Constraints must live in the repository

Any file or subsystem an implementation session must not touch belongs in `AGENTS.md` at the repository root, with the reason. A constraint that exists only in one session's context cannot be respected by the next session and must not be invented.

### Performance numbers

Performance targets are acceptance gates for a milestone, never individual tasks. Never treat a frame-rate number as an item to implement.

## Product target

The engine's 1.0 release (Milestone 23) should provide everything a team expects from Godot, plus physics no general-purpose engine offers:

- A real application runtime with ECS entities, components, resources, schedules, events, observers, reflection, input, and fixed updates.
- Reusable scene composition: prefabs/instanced scenes with nested overrides, groups, signals, and data resources.
- A Vulkan renderer with explicit frames in flight, forward PBR, shadows, transparency, HDR, global illumination, reflection probes, screen-space effects, volumetric fog, post-processing, decals, GPU particles, culling, LOD, and profiling.
- A first-class 2D pipeline: sprites, tilemaps, 2D lights, 2D particles, and 2D physics.
- Custom shaders through a stable engine shader language and a visual shader graph.
- Skeletal and property animation, animation state machines, blend spaces, IK, retargeting, and tweens.
- An audio engine with buses, effects, streaming, and 3D spatialization.
- A runtime UI toolkit with layout containers, themes, rich text, localization, and accessibility.
- Navigation meshes, agents, and avoidance for 2D and 3D.
- High-level multiplayer: RPCs, replicated spawning/synchronization, and deterministic rollback.
- Typed, deduplicated assets with glTF import, per-asset import settings, texture compression, scene serialization, and hot reload.
- An in-engine egui editor at Godot parity: viewport, hierarchy, inspector, asset browser, console, debugger, profiler, gizmos, animation/shader/tilemap editors, project settings, export, and an editor plugin API.
- Fast iteration: Rust game-code hot reload and an optional sandboxed scripting layer.
- Export to Windows, Linux, and macOS, then Android.
- Documentation, tutorials, templates, and demo projects for every major feature.
- **Best-in-class physics** — see the next section.
- **Best-in-class LLM development** — a versioned, budgeted, machine-readable engine surface; a generation-friendly Rust API; model-oriented observation, verification, and quality lints; a warm daemon and MCP adapter; and a published agent benchmark. See the LLM-native track.
- A gameplay framework (saves, state machines, behaviour trees, dialogue, inventory), terrain and streaming large worlds, cinematics, and procedural content. See the engine expansion track.

Primary platforms are Windows and Linux desktop. Native Rust systems are the gameplay API, and games remain normal Cargo projects that can use external libraries. A versioned custom physics-compute ABI is part of the hybrid milestone. Deferred rendering, web export, iOS, and consoles remain out of scope until after 1.0.

## Physics is the flagship

Godot ships a capable general-purpose physics engine (Godot Physics or Jolt). RustingEngine must be measurably better, not merely equivalent. "Best physics" is defined by the following pillars, each owned by a milestone and each proven by a benchmark or test rather than asserted:

| Pillar | What it means | Owner |
| --- | --- | --- |
| Scale | Hundreds of thousands to millions of simultaneously simulated bodies through GPU solvers, with per-body CPU/GPU allocation | M5, M11 |
| Gameplay connection | GPU simulation is never a black box: typed conditions, asynchronous events, commands, and selective readback reach normal Rust gameplay code | M5 |
| Determinism | Bit-identical simulation across runs, builds, thread counts, and (where proven) GPU vendors; replay and rollback built in | M8 |
| Breadth | Rigid, articulated, character, vehicle, ragdoll, soft-body, cloth, rope, fluid, granular, and fracture simulation in one world with two-way coupling | M10, M11 |
| Correctness | Stable stacking, continuous collision, robust contacts, and conservation behaviour verified by a regression scenario suite | M5, M10 |
| Tooling | A physics debugger with recording, timeline scrubbing, per-body inspection, and authoring tools in the editor | M12 |
| Integration | Animation, particles, audio, navigation, and networking consume physics through the same bridge instead of parallel ad-hoc systems | M13-M19 |
| Evidence | A published, repeatable benchmark suite comparing RustingEngine against Jolt, PhysX, Rapier, and Godot Physics on identical scenes | M12 |

Every other subsystem is judged against Godot parity. Physics is judged against the best standalone physics engines.

## Godot parity map

Each Godot feature area has an owning milestone. A feature area is at parity when its milestone's exit gate passes.

| Godot area | RustingEngine equivalent | Milestone |
| --- | --- | --- |
| Node tree, scenes, `PackedScene` instancing, inheritance | ECS hierarchy, prefabs with nested overrides | M1, M9 |
| Signals, groups | Typed events, observers, entity groups | M1, M9 |
| Resources (`.tres`) | Typed data assets with reflection | M2, M9 |
| GDScript / C# / GDExtension | Native Rust, Rust hot reload, optional WASM scripting | M9 |
| Godot Physics / Jolt 3D | Hybrid CPU/GPU physics | M5, M10-M12 |
| Soft bodies, cloth | XPBD deformables | M11 |
| Forward+/Mobile/Compatibility renderers | Forward PBR with quality profiles and capability fallback | M3, M4 |
| SDFGI/VoxelGI/LightmapGI, probes, SSAO/SSR/SSIL, fog, glow | Advanced rendering | M13 |
| Shading language, visual shaders | Engine shader language and shader graph | M13 |
| GPUParticles, decals, MultiMesh, CSG, GridMap | Advanced rendering and editor tools | M13, M20 |
| AnimationPlayer, AnimationTree, IK, tweens | Animation system | M14 |
| Audio buses, effects, 3D audio | Audio engine | M15 |
| Control nodes, themes, RichTextLabel, translations | Runtime UI and localization | M16 |
| 2D renderer, TileMap, 2D physics, 2D lights | 2D engine | M17 |
| NavigationServer, agents, avoidance | Navigation | M18 |
| High-level multiplayer, ENet, WebSocket, HTTP | Networking | M19 |
| Editor docks, debugger, profilers, editor plugins, asset library | Editor parity | M20 |
| Export templates, platforms | Platforms and export | M21 |
| Documentation, demo projects | Documentation and ecosystem | M22 |

## Second product target: Sundering

*Sundering* is a 5v5 competitive game whose terrain is made of millions of simulated chunks that come apart permanently. Its design document lives at `~/Sundering/README.md`. It is built after the engine's physics pillars exist and consumes them harder than any other project will.

Sundering demands four things a general-purpose engine does not provide by default:

- **Determinism.** Competitive play requires the simulation to produce identical results for every participant. This constrains the physics solver, the GPU reduction strategy, and the ordering of every simulation operation. The engine provides it in Milestone 8.
- **Authoritative networking.** A server owns the simulation; clients must stay consistent with it under real latency and loss. The engine provides general networking in Milestone 19; Sundering adds competitive authority in Milestone 24.
- **Destructible terrain as simulation state.** Terrain is not authored geometry. It is a first-class simulated entity that feeds collision, navigation, and vision. It builds on the engine's fracture support from Milestone 11.
- **Navigation over geometry that changes every tick.** Nav meshes assume a static world. Sundering has no static world. It builds on the engine's navigation from Milestone 18.

**Determinism cannot be retrofitted.** Milestone 8 defines the full requirements and their acceptance gate, but three of its ordering rules must be respected while Milestone 5 is implemented, or that work will need rewriting. They are restated at the top of Milestone 5.

Milestones 24-29 depend on Milestones 8, 10, 11, 18, and 19. Milestone 8 must precede any networking milestone, because the determinism result decides which networking model is available.

## Architectural direction

The project should become a Cargo workspace with one-way dependencies:

```text
rusting-math       deterministic math, fixed-point, seeded RNG streams
      ↑
rusting-core       ECS components, schedules, time, input, hierarchy, events, reflection
      ↑
rusting-assets     typed handles, cache, importers, serialization, prefabs, hot reload
      ↑
rusting-physics    2D/3D CPU physics, GPU compute simulation, deformables, fluids, synchronization, queries
      ↑
rusting-terrain    heightmap and volumetric terrain, fracture, structural load, Scar persistence
      ↑
rusting-nav        navigation meshes/volumes, flow fields, agents, avoidance
rusting-anim       skeletal and property animation, state machines, IK, physics-driven animation
      ↑
rusting-render     Vulkan context, extraction, frame graph, 2D/3D passes, materials, shaders, particles, profiling
rusting-net        transport, wire protocol, RPC, replication, rollback, interest mechanism
rusting-audio      device management, mixing, buses, effects, spatialization
rusting-ui         runtime UI layout, themes, text shaping, localization, accessibility
      ↑
rusting-gameplay   teams, match state, abilities as forces, vision, bots
      ↑
rusting-editor     egui panels, viewport, inspector, gizmos, play controls, editor plugins
rusting-script     optional sandboxed WASM scripting host
      ↑
rusting-engine     plugins, application facade, compatibility API, export
      ↑
game projects      vertical slice, demo projects, Sundering
```

`rusting-nav` and `rusting-anim` are siblings. `rusting-render`, `rusting-net`, `rusting-audio`, and `rusting-ui` are siblings at one level and must not depend on each other; UI and particles reach the screen through render extraction, not by calling the renderer. `rusting-editor` and `rusting-script` are siblings. Full layer definitions are in [`architecture.md`](architecture.md).

Dependency cycles between runtime, renderer, physics, assets, and editor are not allowed. ECS entities are the canonical identity and authored scene state. A GPU-owned body's newest runtime transform may live on the GPU; ECS keeps its stable ID, settings, last synchronized state, and pending events. Render batches, physics arrays, indirect buffers, and other GPU representations are derived data.

The existing `Engine::new`, `add_cube`, `add_sphere`, `add_gltf`, and `run` API remains temporarily available as a deprecated compatibility facade implemented over the new runtime.

## Current focus: agent-built games and the editor

Owner direction (2026-09): the editor, example games, and a workflow where an
agent builds a whole game end to end with the `rusting` CLI and project files
alone, with no MCP server or glue scripts. Deep physics items are paused.

Method: build a small game with the CLI only, log every gap, fix the gaps,
keep the game as a sample, then repeat with another genre. Editor work comes
from the `docs/editor-overhaul.md` backlog, and the CLI and the editor should
share one undoable command layer.

- [x] Game 1, Hammer Run (`samples/hammer_run`). Evidence: `rusting check`
  and its three scenarios pass from the repository; captures show the course,
  the HUD and a collected gem; `hammer_hits` fails with the respawn code
  removed.
- [x] Gap: `scene patch` rejected partial built-in sections. Missing fields
  now take their defaults. Evidence: `scene_patch::tests::created_sections_take_defaults_for_missing_fields`.
- [x] Gap: patches needed UUIDs, so an agent had to query first. `id` and
  `parent` take unique names. Evidence: `scene_patch::tests::operations_address_entities_by_unique_name`.
- [x] Gap: new projects had no guide for an agent. `rusting new` writes
  `AGENTS.md`. Evidence: `project::tests` create-project test.
- [x] Gap: scenarios could only check exact ticks or "holds through".
  `within` passes on the first tick a check holds. Evidence:
  `scenario::tests::within_passes_on_the_first_tick_the_check_holds`.
- [x] Gap: game code could not see contacts or the ECS world.
  `GameScene::touching` and `GameScene::world`. Evidence: Hammer Run's
  `hammer_hits` scenario.
- [x] Gap: CLI commands needed an explicit project root; they default to
  `.`. Evidence: `project_root_defaults_to_the_current_folder` in
  `src/bin/rusting.rs`.
- [x] Gap: `scene patch` could not change scene-level fields. `set_scene`
  does. Evidence: `scene_patch::tests::set_scene_changes_scene_fields_but_not_entities`.
- [x] Gap: a scenario could not set up state. `set` steps write a transform
  or component value before a tick. Evidence:
  `scenario::tests::set_steps_move_entities_before_the_tick_runs`, and a
  Hammer Run scenario that sets the `Goal Count` counter and the player's
  position.
- [x] Gap: `GameScene` had no counter helper. `GameScene::counter(name)`.
  Evidence: `project_runner::tests::game_code_reads_and_sets_counters_by_name`.
  Other components stay behind `world()`.
- [ ] Gap (deferred with deep physics): a kinematic player is never pushed
  by dynamic bodies, so hazards need game code. Pushing it out of overlaps
  was tried: the solver treats the kinematic player as infinite mass, so a
  motorised sweeper stalls against it and only nudges it a few centimetres.
  Real knockback needs the player as a finite-mass body in the solver. The
  agent guide documents the `touching` pattern instead.
- [x] Gap: `run --ticks` reported nothing about the end state. It saves the
  scene to `build/final.rscene` for `scene query`; game `eprintln!` output is
  in the `--json` result. Evidence: `rusting run samples/hammer_run --ticks
  120` then `rusting scene query samples/hammer_run/build/final.rscene`.
- [x] Game 2, Target Range (`samples/target_range`), a first-person
  shooting gallery. Evidence: `rusting test samples/target_range
  samples/target_range/tests` passes all three scenarios; a capture shows
  the targets, crosshair and HUD.
- [x] Gap: games had no custom input. `rusting.input_action` binds a named
  action to keys and mouse buttons from the scene; `GameScene::pressed` and
  `held` read it. Evidence:
  `project_runner::tests::scene_input_actions_bind_and_report_presses`.
- [x] Gap: game code could not ray cast or remove objects.
  `GameScene::raycast`, `aim` (along the active camera), `despawn`,
  `set_visible` and `trigger`. Evidence:
  `project_runner::tests::aim_hits_what_the_camera_faces_and_despawn_removes_it`.
- [x] Gap: a scenario could not check that an object is gone. Expectations
  take `"exists": false`. Evidence:
  `scenario::tests::exists_checks_entities_and_paths` and Target Range's
  `first_shot`.
- [x] Gap: `rusting test` ran one file per call. With a folder, or no
  argument (`tests/`), it runs every scenario and lists each result.
  Evidence: `rusting test` in `samples/target_range`.
- [x] Gap: `rusting new` wrote no `.gitignore`, so `target/` and `build/`
  showed up in git. Evidence: `project::tests` create-project test.
- [x] Game 3, Sky Hop (`samples/sky_hop`), a 2D platformer. Evidence:
  `rusting test` in `samples/sky_hop` passes all four scenarios, including
  `clear_level`, which finishes the level with timed inputs only; a capture
  shows the tile map, coin and HUD.
- [x] Gap: character controllers did not ride moving platforms and stuck to
  ceilings while rising. Both now follow the floor body's motion and stop
  rising at a ceiling. Evidence:
  `runtime::tests::controllers_ride_moving_platforms_and_stop_at_ceilings`
  and Sky Hop's `ride_lift`.
- [x] Gap: the editor showed no tile map tiles or background color until
  Play, so Sky Hop's level was blank while editing. Edit mode now runs
  `build_tile_maps` and `apply_scene_background`; a clicked tile selects its
  map, and tiles stay out of the Hierarchy and the saved scene. Evidence:
  `editor::tests::edit_mode_builds_tile_maps_without_listing_or_saving_tiles`.
- [x] Gap: tile maps could only be edited as text rows. The Inspector's
  Tile Painter paints and erases cells from the Scene View, one Undo step
  per stroke. Evidence:
  `editor::picking::tests::scene_view_points_paint_the_tile_map_cell_under_them`
  and `runtime::tests::tile_map_cells_are_found_by_position_and_painted_with_padding`.
- [x] Tile Painter tools, as in Godot: Rectangle fills the cells between
  the pressed and released cells, Fill flood fills the joined cells that
  match the clicked one, and the cell under the pointer (or the dragged
  rectangle) is outlined in the Scene View. Each is one Undo step.
  Evidence: `runtime::tests::tile_map_rectangles_and_flood_fills_paint_regions`,
  `editor::overlay::tests::tile_rect_lines_outline_the_cells_between_two_corners`
  and `editor::picking::tests::scene_view_points_paint_the_tile_map_cell_under_them`.
  The Scene View input path has no automated test; try it by hand.
- [x] Tile Painter Line tool paints Bresenham's line from the pressed cell
  to the released one, with each cell outlined while dragging, and Fill
  no longer grows the grid when clicked outside the used area. Evidence:
  `runtime::tests::tile_map_rectangles_and_flood_fills_paint_regions`
  (line cells, `fill_line`, fill outside the grid). The Scene View input
  path has no automated test; try it by hand.
- [x] Tile Painter palette: the brush is a row of swatches, one per tile in
  its color with its character on top, then Erase and Off, as in Godot's
  TileSet panel. Evidence:
  `editor::inspector::tests::the_tile_palette_shows_each_tile_color_and_picks_a_brush`.
- [x] Tile Painter tool shortcuts, Godot's keys: D Paint, R Rectangle, L
  Line, B Fill. They apply only while a brush is active on a selected tile
  map in Edit mode, where they win over the Scene View keys (R rotates
  otherwise), and they are rebindable in their own Keyboard Shortcuts
  section. Evidence:
  `editor::shortcuts::tests::tile_tool_keys_apply_only_while_a_brush_paints_a_tile_map`.
- [x] Game 4, Ember Arena (`samples/ember_arena`), a third-person arena
  survival game lit by fog, bloom, ambient occlusion and point lights.
  Evidence: `rusting test` in `samples/ember_arena` passes all three
  scenarios (`ember_burns`, `pulse_quenches`, `game_over`); captures in
  `tests/shots/` show the fogged arena, gate glow and ember lights.
- [x] Gap: game code could not spawn copies of a scene object, so enemies
  had to be pre-placed. `GameScene::spawn_copy` copies a template object and
  its children (children are named `"<copy>/<child>"` so names stay unique),
  and `GameScene::in_class` lists the objects in a class. Evidence:
  `project_runner::tests::spawn_copy_clones_the_template_tree_and_in_class_lists_it`.
- [x] Gap: a patch that created a registered component with some fields
  missing failed with "missing field". Registered components now take
  defaults like built-in sections. Evidence:
  `scene_patch::tests::created_sections_take_defaults_for_missing_fields`.
- [x] Gap: lights on hidden objects still lit the scene, so a hidden
  template's glow showed. Light extraction skips objects hidden by
  themselves or a parent, as meshes already did. Evidence:
  `runtime::render_world::tests::lights_under_hidden_objects_are_not_extracted`.
- [x] Game 5, Tower Topple (`samples/tower_topple`), a first-person game:
  thrown balls knock two towers and a pyramid off their stands, with ten
  balls per round. Evidence: `rusting test` in `samples/tower_topple`
  passes all three scenarios (`topple_pyramid`, `win_and_restart`,
  `out_of_balls`); `tests/shots/` shows the throw and the win text.
- [x] Gap: game code could not launch a projectile: there was no camera
  ray to aim along, a template copy could not switch from kinematic to
  dynamic, and a velocity set on a sleeping body was lost.
  `GameScene::camera_ray`, `GameScene::set_body_kind`, and
  `set_linear_velocity` waking the body cover it. Evidence:
  `project_runner::tests::a_kinematic_template_copy_launches_along_the_camera_ray`.
- [x] Gap: a round could not be restarted without resetting every object
  by hand. `GameScene::restart` reloads the starting scene and reruns
  `once` setup. Evidence:
  `project_runner::tests::restart_puts_the_starting_scene_back_and_reruns_setup`.
- [x] Gap: hidden HUD elements were still drawn, so a win message could
  not be hidden by game code. Evidence:
  `runtime::tests::hidden_hud_elements_are_not_drawn`.
- [x] Gap: a thrown ball passed through the pyramid. The continuous
  collision sweep cast only the ball's center, which slipped through the
  seam between two blocks, and it stopped the ball dead on a dynamic
  target instead of pushing it. The sweep now casts against colliders
  grown by the ball's radius and leaves the momentum to the contact solve.
  Evidence:
  `runtime::tests::a_fast_ball_between_two_dynamic_boxes_pushes_both_back`
  (fails on the old sweep) and `fast_cpu_bodies_do_not_tunnel_through_thin_walls`.
- [x] Gap: HUD text wrapped when it grew ("14 / 14" broke onto two lines).
  HUD labels now break only at `\n`. Evidence: `tests/shots/win.png` in
  `samples/tower_topple`.
- [x] Game 6, Crate Keeper (`samples/crate_keeper`), a top-down
  crate-pushing puzzle on a tile map grid. Evidence: `rusting test` in
  `samples/crate_keeper` passes all three scenarios (`solve`, `blocked`,
  `restart`); `tests/shots/solved.png` shows every crate on a gold square
  and the win text.
- [x] Gap: game code could not read the tile map, so grid games had to
  duplicate the level layout in code. `GameScene::tile` reads and
  `GameScene::set_tile` writes the cell under a world position. Evidence:
  `project_runner::tests::game_code_reads_and_writes_tile_map_cells_by_world_position`.
- [x] Gap: scenario `tolerance` applied only to single numbers, so a
  position check failed on `0.10000000149` against `0.1`. It now applies to
  every number in an array or object. Evidence:
  `scenario::tests::tolerance_applies_to_every_number_in_a_vector_or_object`.
- [x] Game 7, Brick Bounce (`samples/brick_bounce`), a 2D brick breaker
  with a dynamic ball bouncing off fixed walls and bricks and a kinematic
  paddle. Evidence: `rusting test` in `samples/brick_bounce` passes all
  four scenarios (`first_hit`, `lose_ball`, `paddle_limits`,
  `win_and_restart`); `tests/shots/playing.png` shows a broken brick and
  the returning ball. The ball's z stays 0 through bounces.
- [x] Gap: game code could set a body's velocity but not read it, so
  constant-speed balls needed `world()` queries. `GameScene::linear_velocity`
  reads it. Evidence:
  `project_runner::tests::a_kinematic_template_copy_launches_along_the_camera_ray`.
- [x] Game 8, Core Defense (`samples/core_defense`), a top-down turret
  shooter aimed with the mouse cursor. Evidence: `rusting test` in
  `samples/core_defense` passes all four scenarios (`aimed_shot`,
  `missed_shot`, `core_falls`, `defended`); `tests/shots/firing.png` shows a
  bolt flying at the first drone.
- [x] Gap: game code had no ray through the mouse cursor, and scenarios
  could not move the cursor, so mouse aiming could not be written or tested.
  `GameScene::pointer_ray` returns it, and the `pointer` scenario step places
  the cursor at a fraction of the view. Evidence:
  `project_runner::tests::pointer_ray_goes_through_the_cursor`,
  `scenario::tests::pointer_steps_place_the_cursor_in_the_capture_sized_view`.
- [x] Gap: `GameObject` could set rotation and scale but not read them back.
  `GameObject::rotation` and `GameObject::scale` read them. Evidence: Core
  Defense bolts fly along their stored yaw.
- [x] Game 9, Putt Course (`samples/putt_course`), a mini golf hole with a
  dynamic ball rolling on physics, a charged putt aimed with the mouse, a
  kinematic sweeper and a sensor cup. Evidence: `rusting test` in
  `samples/putt_course` passes all four scenarios (`first_putt`,
  `overcharge`, `sink`, `too_fast`); `tests/shots/aiming.png` shows the aim
  arrow at half power and `tests/shots/sunk.png` the win text after one
  stroke.
- [x] Gap: six of eight games defined their own `value`, `add` and
  `complete` counter helpers, and a helper that borrows the scene cannot be
  nested in another scene call. `GameScene::counter_value`,
  `add_to_counter` and `counter_complete` do it in one call. Evidence:
  `project_runner::tests::game_code_reads_and_sets_counters_by_name`.
- [x] Gap: game code had no short way to draw seeded random numbers; it
  had to read `RandomSeed` and `FrameTime` through `world()`, and earlier
  samples used fixed lists instead. `GameScene::random(stream)` draws from
  the seed, the fixed tick and the stream. Evidence:
  `project_runner::tests::game_randomness_repeats_for_a_seed_and_varies_by_tick_and_stream`.
- [x] Game 10, Lantern Grid (`samples/lantern_grid`), a lights-out puzzle
  played with mouse clicks on a top-down orthographic board. Evidence:
  `rusting test samples/lantern_grid` passes both scenarios (`center`,
  `solve`); `tests/shots/scrambled.png` shows the 11 lit lanterns of the
  fixed scramble and `tests/shots/solved.png` the full board with the win
  text after three moves.
- [x] Gap: game code could not recolor one object. Scene materials are
  shared by value, so editing one through `world()` recolored every object
  with the same color. `GameScene::color`, `set_color` and `set_emissive`
  give the object its own material. Evidence:
  `project_runner::tests::game_code_recolors_one_object_without_touching_shared_materials`.
- [x] Gap: `rusting test samples/game` read the project folder as a
  scenario and failed on `project.json`, and a scenario path was resolved
  from the current folder rather than the project. A lone project argument
  now runs its `tests/` folder, and scenario paths are looked up in the
  project first. Evidence:
  `rusting::tests::project_root_defaults_to_the_current_folder`;
  `rusting test samples/lantern_grid` from the repository root.
- [x] Game 11, Snake Trail (`samples/snake_trail`), a snake game on the
  2D template: grid steps on fixed ticks, walls read from a
  `rusting.tile_map`, and a tail grown with `spawn_copy`. Evidence:
  `rusting test samples/snake_trail` passes all three scenarios (`eat`,
  `crash`, `win`); `tests/shots/grown.png` shows the four-cell snake after
  the first apple and `tests/shots/won.png` the win text.
- [x] Gap: game code had no way to set a counter to a value; Putt Course,
  Lantern Grid and Snake Trail each wrote the same borrow-and-assign
  helper. `GameScene::set_counter` does it. Evidence:
  `project_runner::tests::game_code_reads_and_sets_counters_by_name`.
- [x] Game 12, Night Vault (`samples/night_vault`), a third-person stealth
  game at night: guards patrol on fixed-tick paths with spot lights, see
  the player through a view cone and a line-of-sight raycast, and raise an
  alert meter; crates and hedges block their view. Evidence:
  `rusting test samples/night_vault` passes all three scenarios (`spotted`,
  `hidden`, `heist`); `tests/shots/spotted.png` shows the lose text with a
  guard's lamp on the floor and `tests/shots/escaped.png` the win text at
  the vault door.
- [x] Gap: the third-person camera went through walls behind the player
  and showed only the sky. A camera-sized sphere cast from the orbit
  center now stops the camera in front of the first solid collider,
  ignoring the player's own body. Evidence:
  `runtime::tests::third_person_camera_stops_in_front_of_a_wall_behind_the_body`;
  the Night Vault captures with the player backed against a hedge.
- [x] Gap: game code could only restart the scene it started with, so a
  game had no levels, menus or separate end screens.
  `GameScene::load_scene(path)` replaces the scene with another project
  scene file, which `restart` then returns to. Evidence:
  `project_runner::tests::load_scene_switches_to_another_project_scene`
  (a missing file leaves the scene as it was; `once` runs again; restart
  returns to the loaded level). `rusting export` still ships only the main
  scene.
- [x] `skills/rusting-game/SKILL.md`: a skill file for LLM agents that
  build games on the engine, with the CLI loop, scene and patch format,
  the `GameScene` API, determinism rules for game code, the scenario format
  and its pitfalls, and a done checklist, taken from the twelve sample
  games.
- [ ] Next: the LLM-native track. Milestone L1 diagnostic codes, `rusting explain`, diagnostic locations and `rusting fix` are done; every L1 item is done (its exit gate needs a fresh-agent trial, which is not verifiable here); continue with L2, then L3 annotated captures and the event trace, since those remove the most guesswork seen in the twelve samples.
- [x] Gap: `rusting export` should copy every scene a game loads, not only
  the main one. Export now copies the project's `scenes/` folder; checked in `export_package_contains_executable_scene_assets_and_readme`.
- [x] Gap: a misspelled field in a partial scene component is dropped and
  takes its default without a diagnostic (Night Vault's moon used
  `intensity` instead of `illuminance` and lit the night at full sun).
  `rusting scene patch` and `rusting check` should report unknown fields.
  Done for `scene patch`: unit tests `a_misspelled_field_in_a_section_is_an_error` and `a_full_material_and_light_pass_the_field_check`; full check green. `rusting check` reads scenes strictly through serde already.
- [x] Gap (Same Shift): game projects built the engine unoptimized, so a
  busy CPU physics scene took 20 to 35 ms per tick and the fixed-step
  catch-up spiraled to near 0 FPS. The `rusting new` template and every
  sample `Cargo.toml` now build dependencies with `opt-level = 3`. When a
  frame hits `max_fixed_steps` and its steps take longer than the time they
  simulate, the next frames run one step each (slow motion instead of a
  freeze) until steps are fast again, with one warning that names the
  likely cause. The clamp happens before the replay recorder, so replays
  step the same way. `rusting run --ticks N --json` reports
  `timings.headless_ms_per_tick`. Evidence:
  `runtime::tests::fixed_steps_slower_than_real_time_fall_back_to_slow_motion`.
- [x] Gap (Same Shift): `restart` and `load_scene` did not repeat physics,
  because the CPU solver ordered bodies by `Entity`, and a reload hands out
  entities in a different order. Bodies now sort by `SpawnOrder`, the
  object's rank by scene ID within its document; runtime spawns take the
  next number. Joints and articulations index bodies by map and sort by
  body order. Evidence:
  `project_runner::tests::restart_repeats_a_physics_pile_exactly` (a
  12-box pile saved in reverse ID order matches after `restart` with
  tolerance 0; fails with the old entity sort).
- [x] Gap (Same Shift): game code had no snapshot, body reset, starting
  state, look direction or class hash. Added `GameScene::snapshot`,
  `restore` (keeps velocities, sleep and solver caches, renamed to the new
  entities), `reset_body`, `set_angular_velocity`, `angular_velocity`,
  `initial`, `set_look` and `state_hash(class)`; `set_body_kind` to
  `Kinematic` or `Fixed` now zeroes velocities. The prelude exports
  `Entity`, `World`, `Name`, `RigidBody`, `PlayerController` and
  `PhysicsWorld`. Evidence:
  `project_runner::tests::body_calls_stop_spin_and_reset_bodies_and_set_look_turns_the_player`
  and `restart_repeats_a_physics_pile_exactly` (restored mid-fall run
  matches the run that went on; fails without the solver-state rename).
- [x] Gap (Same Shift): sleeping bodies floated when their support was
  removed or moved. Sleepers near a body that gameplay removed, moved or
  changed wake on the next step. Evidence:
  `runtime::tests::sleeping_cpu_bodies_fall_when_their_support_is_removed_or_moved`.
- [x] Gap (Same Shift): scenarios had no way to record a value. A `log`
  step records `entity.path` (every tick with `until`) and never fails;
  `rusting test --json` lists the lines. Evidence:
  `scenario::tests::log_steps_record_a_value_every_tick_and_never_fail`.
- [ ] Gap (Same Shift): GPU bodies reach game code 1 to 3 frames late and
  cannot be reset or teleported after setup. Document their feature set
  and let game code move one to a pose with zero velocity.
- [ ] Gap (Same Shift): a camera that renders into a texture or a screen
  rectangle, for monitors and split screen.
- [x] Gap: `RenderSettings::render_scale` is read by the window and headless
  capture: the scene renders into an offscreen image of the scaled size and a
  linear blit stretches it over the target (`rendering::render_scale`). See
  Night Market F10.
- [ ] Gap: `rusting capture` does not run game code.
- [x] Gap: a scenario stops at its first failed check. `"keep_going": true` in the scenario file reports every failure; test `keep_going_reports_every_failed_check`.
- [ ] Gap: a windowed `rusting run` shows no on-screen FPS or frame time.
  `RUSTING_PERF=1` prints fps, update/render split, CPU phases, GPU pass
  times and draw counts to stderr once a second and shows fps and frame
  time in the window title. An in-game overlay is still open.
- [x] Perf (Same Shift, RTX 3060, 1080p, clean GPU): 355 to about 1000 fps.
  Swapchain image count 2 to 4 (Wayland throttled Immediate with 2), a
  `reflections` render setting that skips the scene copy, mip chain and depth
  pyramid, and batching by mesh and texture group (97 to 18 draws, 0.56 to
  0.22 ms recording). Evidence: `RUSTING_PERF=1` lines in CHANGELOG, test
  `materials_that_differ_only_in_color_share_a_batch`,
  `the_reflections_setting_reaches_the_render_world`; full check green with
  and without `--features gpu-tests`. Note: a second game instance on the
  GPU halves every number; close it before measuring.
- [x] Gap (Same Shift): a scenario `expect` with tolerance 0 failed on
  every printed spelling of an f32 value, because serde_json parses decimal
  text only to within an f64 ulp. A value that is an exact f32 now also
  matches any number that rounds to the same f32. Evidence:
  `scenario::tests::a_printed_f32_matches_itself_with_tolerance_zero`.
- [x] Gap (Same Shift): cube side faces showed images upside down. Cube
  UVs follow glTF: v = 0 is the image's first row, at the top of each side
  face and the -Z edge of the top and bottom faces (spheres already did).
  Evidence:
  `assets::tests::cube_side_faces_show_the_first_image_row_at_the_top`.
- [x] Gap (Same Shift): textures could not repeat. Materials have
  `uv_scale` and `uv_offset`, saved in scenes (default `[1, 1]` and
  `[0, 0]`), reflected for the Inspector and applied in the vertex shader.
  Evidence: `rendering::scene_renderer::tests::render_instance_layout_matches_shader_struct`
  checks the new `uv_transform` field against the shader struct.
- [x] Gap (Same Shift): `rusting schema` called scene texture slots a
  "project path"; they are plain strings relative to the scene file, not
  `{"$asset": ...}`. The `mesh_renderer` entry now says so and lists
  `uv_scale` and `uv_offset`.
- [x] Gap (Same Shift): the player controller climbed boxes on the round
  bottom of its capsule and could not be kept from pushing dynamic bodies.
  `PlayerController` has `max_slope` (default 45°), `max_step_height`
  (default 0.3 m) and `push_bodies` (default true), and
  `PhysicsWorld::move_character_on_foot` walks with the first two. With
  `push_bodies` off the solver drops the player's contacts with dynamic
  bodies, which still block its movement. Evidence:
  `runtime::tests::players_step_onto_low_ledges_but_not_high_ones_and_can_leave_crates_alone`.


Goal: establish a trustworthy baseline before adding architecture or features.

### Completed

- [x] Make `MaterialBuilder::default()` agree with `Material::default()`.
- [x] Copy all material properties when creating cube and sphere instances.
- [x] Cache sphere meshes by subdivision level.
- [x] Replace mutable batch/index instance handles with stable IDs.
- [x] Align all physics compute-shader instance fields with the Rust `InstanceData` layout.
- [x] Align physics push constants and add explicit padding.
- [x] Repair the `NoCollision` shader's mass, gravity, friction, restitution, and angular-velocity interpretation.
- [x] Make broad-phase sphere and box radius conventions agree with collision shaders.
- [x] Add compile-time CPU structure size/offset assertions.
- [x] Add tests that enforce common layouts across every compute shader.
- [x] Make `cargo fmt --check`, all-target tests, strict clippy, and all-target checking pass.
- [x] Upgrade Vulkano 0.33 to 0.35 and migrate pipeline, descriptor, image, allocation, and command APIs.
- [x] Remove `vulkano-win` and migrate event dispatch to winit 0.30's `ApplicationHandler` lifecycle.
- [x] Negotiate surface format, color space, image count, composite alpha, and present mode.
- [x] Enable the Khronos validation layer automatically in debug builds when it is installed.
- [x] Enable synchronization validation and opt-in GPU-assisted validation with `RUSTING_GPU_VALIDATION=1`.

### Remaining

- [x] Move compatibility-facade window and surface creation into `ApplicationHandler::resumed`, removing the last deprecated winit call. Evidence: `Engine` now builds an ECS `App` and `run()` hands it to `project_runner::run_windowed`, whose `WindowRunner` creates the window in `resumed()`. `rendering::init_vulkan` (the eager window and the last `#[allow(deprecated)] create_window`) is deleted; its validation-layer setup moved to `rendering::vulkano_config`, and `RUSTING_VULKAN_DEVICE` now applies to every window. Verified: `RUSTING_VULKAN_DEVICE=llvmpipe ./target/debug/user_main` prints `Using device llvmpipe ...`; an unknown selector panics with `matches no available Vulkan device; available devices: [NVIDIA GeForce RTX 3060, llvmpipe ...]`; the 8-second `user_main` and `editor` runs print no validation warnings or errors. Limit: the main queue no longer gets a debug name on windowed runs because `vulkano_util` creates the queues.
- [x] Replace public initialization and asset-loading panics with typed errors and `Result` APIs.
  - [x] `geometry::gltf_loader::load_gltf_scene` returns `Result<_, GltfLoadError>` and `Engine::add_gltf` returns `Result<(), AssetError>` instead of panicking on a missing/corrupt glTF file or a primitive without positions.
  - [x] `Engine::load_texture` returns `Result<usize, AssetError>` instead of panicking on a missing/corrupt image file.
  - [x] Remaining production `.unwrap()`/`.expect()`/`panic!` call sites outside tests audited (~220 across `scene/mod.rs`, `engine/mod.rs`, `rendering/*`, `project_runner.rs`). Findings: no genuine unconverted caller-recoverable site remains (checked for file/filesystem loading specifically — the only production `fs::`/`File` usage is a compiled-in shader module load and this session's own golden-image test helper). The rest fall into three buckets that are correctly left as panics, not oversights: (1) internal invariants after initialization — ECS resource/component lookups, GPU pipeline/queue state assumed valid once the device exists; (2) documented API panics that already ship a non-panicking alternative, e.g. `GameScene::object` panics but `GameScene::try_object` returns `Option`; (3) the legacy `Engine`/`scene::RenderScene` compatibility facade (`engine/mod.rs`, `scene/mod.rs`), already deferred to the Milestone 1 facade replacement per the compatibility-facade item above — converting its error style now would be churn thrown away at that rewrite. No new unchecked item added since nothing actionable was found outside what's already tracked.
- [x] Add Vulkan debug names and scoped command-buffer labels. `init_vulkan` named the main queue via `Device::set_debug_utils_object_name` when `ext_debug_utils` was enabled (removed with `init_vulkan` in Milestone 0's resumed-window item); `SceneRenderer::render` wraps its command buffer in a `begin_debug_utils_label`/`end_debug_utils_label` scope ("SceneRenderer::render"), both gated on `instance().enabled_extensions().ext_debug_utils` so release/no-validation builds pay nothing. Verified live on real hardware (NVIDIA RTX 3060): `rendering::debug_utils_tests::object_names_and_command_buffer_labels_are_accepted_by_the_driver` creates an instance with `ext_debug_utils` forced on, names a queue, and records/submits a command buffer with a begin/end label pair, asserting the driver accepts both calls with no validation error. `cargo test --lib --features gpu-tests`: 146 passed.
- [x] Validate rendering, resizing, minimizing, restoring, and shutdown on Linux and Windows. Partially verified live in this environment (Linux, Wayland session, real NVIDIA RTX 3060): `./target/debug/game testGame/build/main.rscene.bin` opens a window and renders continuously for 5s under `timeout` with no panic/validation output, then exits cleanly (SIGTERM). Resizing, minimizing, and restoring need a window-manager automation tool (`xdotool`/`wmctrl`, neither installed, not added since installing new system packages is outside this session's scope) to script without a human; Windows has no machine available in this environment at all. See new item below for the remaining manual QA pass.
- [ ] Manually verify window resize, minimize, and restore on Linux (with `xdotool`/`wmctrl` installed, or by hand) and repeat the full rendering/resize/minimize/restore/shutdown pass on a Windows machine — blocked in this environment per item above.
- [x] Replace fixed grid limits with explicit capacity tracking and overflow reporting. Never silently omit bodies. Evidence: the rebuilt `Engine` maps `GridCollision` and `FullPhysics` to `PhysicsSolver::Full`, so every runner now uses the ECS contact grid. That grid grows with the body count, reports overflow in `RenderCapacityDiagnostics`, and keeps every body through its fallback list (Milestone 5 overflow items; `gpu_bodies_collide_with_each_other_through_grid_and_fallback`). The old `grid_build.comp` with `MAX_PER_CELL = 128` is still compiled, but only the legacy `RenderScene` code and its tests use it; that code is deleted by the Milestone 1 cleanup item.
- [x] Add generated/reflected layout checks for storage buffers, uniforms, vertex data, and push constants. `render_instance_layout_matches_shader_struct`, `light_gpu_layouts_match_shader_structs`, and `hybrid_physics_gpu_layouts_match_shader_structs` (`rendering/scene_renderer.rs`) now compare CPU upload structs' `size_of`/`offset_of` against the `vulkano_shaders`-generated types for the same GLSL struct/block (`vertex_shader::RenderInstance`/`Camera`, `fragment_shader::Light`, `physics_shader::PhysicsState`/`ConditionInstruction`/`RuleState`/`PhysicsEvent`/`PhysicsPush`) — reflected straight from the compiled SPIR-V — instead of hardcoded magic-number offsets that could silently drift from the shader source. `GpuEventHeader` has no named GLSL struct (bare buffer block) so it keeps its one hand-computed size assertion. Verified: `cargo test --lib --features gpu-tests`: 146 passed.

### Exit gate

- Debug validation produces no errors during startup, rendering, resize, minimize/restore, and shutdown.
- Linux and Windows smoke tests render at least 1,000 frames.
- No known CPU/GPU layout mismatch remains.
- Formatting, strict clippy, tests, shader compilation, and all-target checking pass in CI.

## Milestone 0.5: Verifiable development environment

Goal: make GPU-touching work provable on a machine with no display and no second GPU. This milestone precedes all remaining GPU work. Without it, most of Milestones 3, 4, 5, 8, 11, 13, 25, and 26 cannot be verified by anyone, including CI.

### Software Vulkan device

- [x] Document Mesa lavapipe as the supported software Vulkan implementation. On Arch the package is `vulkan-swrast`; on Debian/Ubuntu it is `mesa-vulkan-drivers`. See `docs/dev-environment.md`.
- [x] Document the invocation. Correction: on this machine (Arch, current `vulkan-swrast`) the ICD file is `/usr/share/vulkan/icd.d/lvp_icd.json`, not `lvp_icd.x86_64.json` — the exact name is distro/version-dependent, see `docs/dev-environment.md`.
- [x] Verify `vulkaninfo --summary` reports a second device with `vendorID = 0x10005` when the variable is set. Confirmed on this machine.
- [x] Record required Vulkan features and extensions, and which of them lavapipe does not provide. `init_vulkan` requests only the `khr_swapchain` device extension and no explicit device features; lavapipe supports `khr_swapchain` via the standard surface path, so it provides everything the engine currently requests. See `docs/dev-environment.md`.

### Device selection

- [x] Add `RUSTING_VULKAN_DEVICE` to select a physical device by index or by name substring. `rendering::select_device_index` (pure, unit-tested) plus wiring in `init_vulkan_headless` and `vulkano_config` (every window).
- [x] Log the selected device name, vendor ID, driver version, and API version at startup. Verified live: `Selected Vulkan device: llvmpipe (LLVM 22.1.8, 256 bits) (vendor 0x10005, driver 109060098, api 1.4.354)` with `RUSTING_VULKAN_DEVICE=llvmpipe` and lavapipe's ICD.
- [x] Fail with a typed error naming every available device when the selection matches nothing. Never fall back silently. `DeviceSelectionError` lists every candidate device name; `vulkano_config` and `init_vulkan_headless` panic with its `Display` message, because `VulkanoContext` itself panics on a failed device pick.

### Headless mode

- [x] Create instance, physical device, and logical device without a surface, a swapchain, or a winit window. `rendering::init_vulkan_headless`/`HeadlessVulkanBase`, sharing device-selection logic with the windowed path via `select_physical_device`. Verified live (no window created): `Selected headless Vulkan device: NVIDIA GeForce RTX 3060 (vendor 0x10de, driver 2580660800, api 1.4.351)`.
- [x] Add offscreen render targets with the same formats used by the windowed path. `swapchain::create_offscreen_target`/`OffscreenTarget` (color `B8G8R8A8_SRGB` + `TRANSFER_SRC` for later readback, depth `D16_UNORM`), sharing `create_render_pass_for_format` with the windowed `create_render_pass`. Verified live against `init_vulkan_headless` on real hardware, no window created.
- [x] Add a fenced image readback returning CPU pixel data. `readback::read_back_image` (copy-to-buffer command, `then_signal_fence_and_flush`, `wait(None)`, host-visible buffer read). Verified live against `init_vulkan_headless`/`create_offscreen_target` on real hardware.
- [x] Add a fenced storage-buffer readback returning CPU data after a compute dispatch. `readback::read_back_buffer<T>` (copy-buffer command, same fence pattern as `read_back_image`). Verified live: `reads_back_storage_buffer_results_after_a_compute_dispatch` builds a `ComputePipeline` from `src/shaders/compute/test_double.comp`, dispatches against a 64-element storage buffer on real hardware (`NVIDIA GeForce RTX 3060`), and asserts every value was doubled.
- [x] Add `--headless` to the game binary, running a fixed number of ticks and exiting with a status code. `project_runner::run_project_headless` builds the same `App`/plugin stack as `run_project` (`AssetPlugin`, `HybridPhysicsPlugin`, `RenderExtractPlugin`, game plugin) with no `VulkanoContext`, window, or renderer, and runs 60 fixed-step ticks; `src/bin/game.rs` wires `--headless` to it. Verified live: `./target/debug/game --headless testGame/build/main.rscene.bin` opens no window and exits 0.

### Test harness

- [x] Add a `gpu-tests` feature. GPU tests compile always and run only under that feature. `gpu-tests = []` in `Cargo.toml`; every GPU test carries `#[cfg_attr(not(feature = "gpu-tests"), ignore = "...")]`. Verified live: `cargo test --lib` shows `4 ignored`; `cargo test --lib --features gpu-tests` runs all 142 with 0 ignored, both clean under `cargo clippy --workspace --all-targets [--features gpu-tests] -- -D warnings`.
- [x] Add a shared test fixture that creates a headless device once per test binary. `rendering::test_support::headless_device()` (`OnceLock<HeadlessVulkanBase>`), used by `swapchain::tests` and both `readback::tests`. Verified live: `cargo test --lib --features gpu-tests -- --nocapture` prints "Selected headless Vulkan device" only twice for 4 GPU tests (once for the fixture's one-time init, once for the unrelated `headless_device_tests` test that exercises `init_vulkan_headless` directly).
- [x] Skip with an explicit message, not a failure, when no Vulkan device is present. Every GPU test still opens with `if VulkanLibrary::new().is_err() { eprintln!("skipping: no Vulkan driver present"); return; }` in addition to the `gpu-tests` ignore gate, so a `--features gpu-tests` run on a driverless machine reports pass, not failure.
- [x] Add golden-image comparison with a per-pixel tolerance, writing actual/expected/difference images to an artifact directory on mismatch. `rendering::test_support::assert_matches_golden_image(actual, golden_path, artifact_dir, tolerance)` (per-channel `abs_diff`, dimension check first, writes `actual.png`/`expected.png`/`diff.png` via the `image` crate on any mismatch, no GPU required). Verified live: `identical_image_matches_its_own_golden_with_zero_tolerance`, `a_small_difference_within_tolerance_passes`, and `a_mismatch_beyond_tolerance_panics_and_writes_artifact_images` (which asserts the three artifact files exist after a caught panic) all pass under `cargo test --lib`.
- [x] Add a compute-dispatch fixture: upload known input, dispatch, read back, assert. `rendering::test_support::dispatch_and_read_back` (generic over the storage-buffer element type; builds the pipeline/layout/descriptor set, dispatches, reads back via `readback::read_back_buffer`). `readback::tests::reads_back_storage_buffer_results_after_a_compute_dispatch` now calls it instead of repeating the boilerplate inline. Verified live against real hardware: same doubling assertion still passes.
- [x] Mark every test whose result depends on real hardware timing `#[ignore]`, with the reason in the attribute. Audited: no test in the crate asserts on wall-clock elapsed time, FPS, or frame pacing — `Duration` values in `runtime::tests` and `project_runner` tests are deterministic simulated inputs (fixed `Duration::from_millis(...)` passed to `App::update`), not measured real time, so nothing currently qualifies. Note for future work: apply this to any new test that measures real elapsed time, frame pacing, or throughput.

### Repository constraints

- [x] Add `AGENTS.md` recording files that implementation sessions must not modify, and why.
- [x] Record the verification tier table from the working agreement above, or link to it.

### Exit gate

- A headless compute test and a headless offscreen render test both pass under lavapipe on a machine with no display.
- The same two tests pass on the NVIDIA device.
- CI runs the `gpu-tests` feature under lavapipe on every pull request.
- `RUSTING_VULKAN_DEVICE` selects between lavapipe and hardware on a machine where both are present.

## Milestone 1: Workspace and runtime foundation

Goal: create the engine runtime that owns canonical scene state and system execution.

### Workspace

- [x] Convert the package into a Cargo workspace using the architectural dependency direction above. Verified: root `Cargo.toml` already had `[workspace] members = ["testGame"]`; added `crates/rusting-core` as the first real architectural-layer member (bottom of the dependency direction). `cargo build --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` clean.
- [x] Move reusable public data types into `rusting-core` without Vulkan dependencies. First increment: moved `Transform` and `CollisionType` into `crates/rusting-core`. Second increment: moved the Vulkan-independent canonical scene, hierarchy, camera, naming/classification, light, visibility, render-settings, physics-settings/status, and rigid-body/collider data from `src/runtime/components.rs` into `crates/rusting-core/src/components.rs`; `rusting_engine::runtime::*` remains compatible through re-exports. Third increment: moved the generic `EventQueue<T>` and its frame-buffer semantics into `crates/rusting-core/src/events.rs`; the engine retains only the `World` adapter and its public runtime path remains an exact re-export of the core type. Fourth increment: moved the Vulkan-independent `FrameTime` resource and its duration accessors into `crates/rusting-core/src/time.rs`; the engine retains time control and advancement policy while its public runtime path remains an exact re-export of the core type. Fifth increment: moved hierarchy mutation, transform propagation, malformed-hierarchy diagnostics, and the typed `HierarchyError` into `crates/rusting-core/src/hierarchy.rs`; the engine adapter preserves the existing `AppError` variants and public diagnostics/propagation paths. Sixth increment: moved the public `ScheduleStage` and `FrameReport` scheduler data types into `crates/rusting-core/src/schedule.rs`; schedule storage and execution remain engine-local while the existing runtime paths are exact re-exports. Seventh increment: moved `TimeControl`, its private accumulator/queued-step state, the deterministic advancement algorithm, and typed `TimeAdvanceError` into `crates/rusting-core/src/time.rs`; the engine retains only the `World`/`AppError` compatibility adapter and its public time resource paths remain exact re-exports. Eighth increment: moved `RuntimeInput`, `ActionMap`, `InputBinding`, and the `KeyCode`/`MouseButton` re-exports into `crates/rusting-core/src/input.rs` (adds a `winit` dependency to core for the key/button types only; no Vulkan); `src/runtime/input.rs`/`actions.rs` keep only the `World` install adapters and exact re-exports. Built-in `CollisionEvent` remains engine-local with the CPU physics implementation. `MeshRenderer` stays engine-local because it uses engine asset handles. `src/core/material.rs` and `src/core/physics.rs` also remain engine-local because they depend on rendering registries. Audit complete: remaining public runtime types are domain-owned rather than core data: `MeshRenderer` and scene documents depend on assets, physics events and GPU IDs belong to the planned physics crate, render snapshots and picking belong to the planned render crate, and `App`/`Plugin`/`EngineBuilder` remain engine application logic. `cargo tree -p rusting-core` shows no Vulkan, egui, image, or glTF dependency.
  - [x] Ninth increment: moved `CpuFrameTimings` to `rusting-core::schedule` as a Vulkan-independent ECS resource and preserved `rusting_engine::runtime::CpuFrameTimings` as an exact re-export. Evidence: core resource test passes; `cargo fmt --all --check`, strict workspace clippy (default, `--no-default-features`, and `--features gpu-tests`), `cargo test --workspace`, and `cargo test --workspace --features gpu-tests -- --test-threads=1` pass. The parallel GPU suite crashed with SIGSEGV in the test process; the serial run passed all 312 engine library tests.
  - [x] Tenth increment: moved the Vulkan-independent `AppError` enum and its error formatting into `rusting-core::app`, preserving `rusting_engine::runtime::AppError` as an exact re-export. Evidence: `app::tests::errors_preserve_entity_and_plugin_context` passes; formatting, strict workspace clippy in all three configurations, default workspace tests, and serial `gpu-tests` all pass.
  - [x] Eleventh increment: moved the asset-independent `ClickEvent` data type into `rusting-core::input` while keeping gameplay picking and event routing in the engine; `rusting_engine::runtime::ClickEvent` remains an exact re-export. Evidence: `runtime::tests::clicking_a_rendered_cube_fires_a_click_event` passes and exercises event delivery through the old path.
- [x] Keep compatibility re-exports in `rusting-engine` during migration. `src/core/mod.rs` re-exports `rusting_core::{transform, collisions}` at their original `crate::core::transform`/`crate::core::collisions` paths, so every existing caller (`src/tests.rs`, `src/scene/mod.rs`, `src/engine/mod.rs`, `src/core/physics.rs`) compiles unchanged. Confirmed `src/editor/mod.rs`/`src/editor/view.rs` never import `crate::core::` at all, so this migration step could not have touched the protected editor/gizmo files even indirectly; verified via `git status` that both files are untouched.
- [x] Add feature flags for editor, validation, experimental GPU physics, and optional importers. `editor` and `window` already gated their modules. `validation` now force-enables the Khronos validation layer (when installed) in non-debug builds too (now in `rendering::validation_instance_info`, used by `vulkano_config`: `cfg!(debug_assertions) || cfg!(feature = "validation")`). New `gltf` feature (default, implied by `editor` because `src/editor/view.rs` calls `AssetServer::import_gltf`) makes the `gltf` crate optional and gates `geometry::gltf_loader`, `Engine::add_gltf`, `AssetServer::import_gltf`, and the `gltf_test` example. `experimental-gpu-physics` exists but gates nothing yet: all current GPU physics is the shipped hybrid path, so the flag is reserved for Milestone 5 GPU solvers. Verified: strict clippy over `--workspace --all-targets` clean for default, `--no-default-features`, `--no-default-features --features window`, `--no-default-features --features gltf,validation`, and `--features gpu-tests`; `cargo test --workspace` passes with default and `--no-default-features`.

### ECS and application lifecycle

- [x] Add standalone `bevy_ecs` with its parallel scheduler enabled.
- [x] Introduce a Vulkan-independent `App` and fallible `EngineBuilder`.
- [x] Add `add_plugin`, `add_system`, `add_systems`, `insert_resource`, `spawn`, `despawn`, `run`, and controlled shutdown.
- [x] Define ordered schedules: `Startup`, `FixedUpdate`, `Update`, `PostUpdate`, and `RenderExtract`.
- [x] Support command-buffered entity spawning and despawning inside systems through Bevy `Commands`.
- [x] Implement deterministic fixed timestep with configurable maximum catch-up limits.
- [x] Add pause, single-step, time scale, elapsed time, frame delta, and fixed delta resources.
- [x] Add typed events with frame-bounded lifetime.

### Canonical components and resources

- [x] `Transform` and `GlobalTransform` ECS components.
- [x] Parent/child hierarchy, cycle rejection, diagnostics, and cycle-safe propagation.
- [x] `Camera` and perspective/orthographic projection settings.
- [x] Deterministic active-camera selection by active flag, priority, and stable entity identity.
- [x] `MeshRenderer` and visibility components using typed asset handles.
- [x] Directional, point, and ambient light components.
- [x] `RigidBody`, primitive `Collider`, sensor, and collision-layer components.
- [x] `GpuEffectBody` marker for bodies currently assigned to GPU simulation. Removed in Milestone 5: it only mirrored `PhysicsBody::uses_gpu`, which `SimulationClass::Gpu` now states directly.
- [x] `FrameTime`, `TimeControl`, `RenderSettings`, `PhysicsSettings`, and `QualityProfile` resources.
- [x] Input action mapping with keyboard, mouse, and gamepad-ready abstractions.

### Runtime ownership

- [ ] Split the monolithic loop into `WindowRunner`, `Renderer`, `PhysicsWorld`, `AssetServer`, and optional `EditorState`.
  - [x] Windowed and headless project runners now use one `load_project_runtime` function to install assets, hybrid physics, render extraction, the game plugin, and the cooked scene in the same order. GPU availability remains a windowed runner setting. Evidence: default workspace tests and serial `gpu-tests` pass; the full window/renderer ownership split remains open.
  - [x] Extracted `WindowRunner` from `ProjectApplication` in `src/project_runner.rs`. It owns the Vulkan context, windows, scene renderer, VSync state, resize, frame pacing, swapchain acquisition, rendering, and presentation; `ProjectApplication` keeps the ECS runtime and coordinates input, GPU result delivery, and updates. Evidence: `cargo check --workspace`, `cargo run --bin game -- --headless testGame/build/main.rscene.bin`, formatting, strict workspace clippy (default, no default features, GPU tests), default workspace tests, and serial GPU tests pass. The legacy `Engine` facade and editor window loops still need the same separation.
  - [x] Extracted `EditorWindowRunner` from `EditorApplication` in `src/bin/editor.rs`. It owns the Vulkan context, window/swapchain, scene renderer, offscreen scene target, egui painter state, VSync state, and frame pacer, and initializes them on `resumed`. `EditorApplication` keeps scene ECS, input, and update coordination. Evidence: `cargo check --workspace --bin editor`, formatting, strict workspace clippy in all three configurations, default workspace tests, and serial GPU tests pass. Editor draw/present delegation and a live GUI smoke run remain open in `docs/editor-overhaul.md`.
  - [x] Rebuilt the legacy `Engine` facade over `App` + ECS + the shared `WindowRunner`. `add_cube`/`add_sphere`/`add_gltf` spawn `MeshRenderer`, `PhysicsBody`, `RigidBody`, and `Collider` entities; legacy compute shaders map to `SimulationClass`/`PhysicsSolver`; `set_light` becomes a `DirectionalLight` plus `AmbientLight`; the fly camera is an `Update` system that uses the new `RuntimeInput::mouse_motion` and cursor capture. Breaking changes: `PerspectiveCamera::new` drops the aspect ratio and `update` no longer returns a view matrix; `gltf_cache` holds imported nodes; the C-key culling toggle is gone; `run()` needs the `window` feature. Evidence: `engine::tests` (component mapping, scene overrides, camera and light directions, and the GPU render test below); `user_main` (10k PBR cubes) ran 8 s with no errors. Not smoke-run: `native_texture` and `gltf_test` need the untracked `testModels/` directory, which is missing here.
  - [x] Delete the legacy `scene::RenderScene`, `ShaderRegistry`, `ComputeShaderRegistry` grid path, and `geometry::gltf_loader` now that no runner uses them. Removed the retired scene, registry, pipeline, render, and glTF loader modules and disconnected the legacy shader module. Kept the public `ComputeShaderType` and physics profile mapping in `rendering::compute_profile` for the `Engine` facade, with a `rendering::compute_registry` compatibility re-export that contains no pipeline registry. Replaced `src/tests.rs` with its still relevant material tests; the old registry GPU test and layout checks exercised only the retired path, while `SceneRenderer` has active GPU rendering, physics, and shader layout coverage. Evidence: `cargo fmt --all --check`, strict workspace clippy (default, `--no-default-features`, and `--features gpu-tests`), `cargo test --workspace`, and serial `cargo test --workspace --features gpu-tests -- --test-threads=1` all pass; `rg` finds no Rust references to the removed scene, shader-registry, or glTF loader modules.
  - [x] Moved the editor scene draw, egui paint, and present sequence into `EditorWindowRunner::draw`; `EditorApplication::window_event` now only updates the ECS, builds the GUI, extracts, and delegates drawing. Evidence: formatting, strict workspace clippy in all three configurations, default workspace tests, and serial GPU tests pass; `timeout 8 cargo run --bin editor` on Wayland opened the editor and ran with no error output until killed. Interactive resize and window-close checks remain open in `docs/editor-overhaul.md`.
- [x] Keep the window, ECS world, and rendering submission main-thread owned.
  Evidence: thread audit (2026-09-23) found only `std::thread::yield_now()` in
  `App::run` and editor cargo-build/stdout worker threads in `src/editor/mod.rs`;
  no thread touches the window, `World`, or Vulkan submission.
- [x] Remove unnecessary `Arc<Mutex<_>>` usage from camera and scene state.
  Evidence: legacy `Engine` now owns `camera: PerspectiveCamera` and
  `scene: RenderScene` directly. Same change fixed a pre-existing
  `AccessConflict(DeviceRead)` panic in `prepare_frame_ubo` by keeping one
  fence per frame slot and waiting only before reusing that slot. Clippy/tests
  clean; `user_main` stress_pbr ran 8 s at ~1000-1300 FPS without panic.
- [x] Use worker tasks only for safe asset decoding and preparation.
  Evidence: `Assets::load_async` runs only the CPU loader on a worker; results
  are published on the main thread by `poll_loads` (see Milestone 2).
- [ ] Execute animations as ECS systems instead of passive data.
  The unused legacy `SceneObject::animation` field was removed with
  `RenderScene`. Real ECS animation remains part of Milestone 14; it needs
  clip sampling, pose application, and a fixed/update schedule integration.

### Exit gate

- A headless test app can run startup, fixed, variable, hierarchy, and event systems deterministically.
- Entities remain stable through spawn, despawn, sorting, and render extraction.
- Pause and single-step work without affecting editor interaction.
- The old facade can spawn and render cubes and spheres through ECS components. Met: GPU test `engine::tests::engine_cubes_and_spheres_render_through_ecs` builds an `Engine` with a red cube and a green sphere, runs one `App` update, renders the extracted `RenderWorld` offscreen, and finds both colors (llvmpipe and RTX 3060 via `--features gpu-tests`).

## Milestone 2: Asset system and scene persistence

Goal: make assets stable, deduplicated, reloadable, and serializable.

### Typed assets

- [x] Add generational `Handle<MeshAsset>`, `Handle<TextureAsset>`, `Handle<MaterialAsset>`, and `Handle<SceneAsset>`.
- [x] Build typed asset storage with generation checks, explicit reference tracking, and frame-deferred destruction.
- [x] Canonicalize asset paths and deduplicate repeated loads.
- [x] Separate CPU mesh/material assets from renderer-owned prepared GPU mesh buffers.
- [x] Add synchronous loading state inspection and structured asset errors.
- [x] Add worker-backed asynchronous loading states.
  Evidence: `LoadState::Loading`, `Assets::load_async` (path dedup, panic
  capture, failed-path retry, cancel via `remove`) and `Assets::poll_loads`,
  polled each `Update` by `AssetPlugin`. Unit test
  `async_loads_publish_success_failure_panic_and_discard_cancelled`.
- [x] Add default/fallback mesh, material, and texture assets.

### Import pipeline

- [x] Deduplicate glTF meshes, images, and materials by synthesized asset path, including sampler identity.
- [x] Import base-color textures with sRGB formats.
- [x] Import normal, metallic-roughness, occlusion, and emissive textures with correct linear/sRGB treatment.
- [x] Import sampler filtering and wrapping modes.
  Evidence: `TextureAsset::sampler` (`TextureSampler` with mag/min/mipmap
  `TextureFilter` and U/V `TextureWrap`) is filled from glTF samplers; the
  sampler index is part of the synthesized texture path. Unit test
  `gltf_import_carries_sampler_filtering_and_wrapping`. API note: adds a public
  field to `TextureAsset`. The renderer still uses one shared linear/repeat
  sampler; per-texture samplers land when the ECS renderer uploads
  `TextureAsset`s (GPU tier).
- [x] Generate or import tangents.
  Evidence: glTF `TANGENT` is imported when present; otherwise
  `assets::generate_tangents` derives tangents and handedness from triangle UV
  gradients, with a perpendicular fallback for degenerate UVs. Unit test
  `generated_tangents_follow_uvs_handedness_and_degenerate_fallback`.
- [x] Support alpha opaque, mask, and blend modes. First increment (data):
  `AlphaMode::{Opaque, Mask { cutoff }, Blend}` on `MaterialAsset`, imported
  from glTF `alphaMode`/`alphaCutoff`, saved as `SceneAlphaMode` in scene
  format 5. Cooked v1-v4 scenes migrate to `Opaque` (v4 is dispatched on its
  leading `format_version` because a v4 material can also parse as v5).
  Tests: `gltf_import_carries_sampler_state_and_alpha_mode`,
  `material_alpha_mode_survives_scene_round_trip`,
  `version_four_cooked_scene_migrates_materials_to_opaque_alpha`; cooked v3
  `testGame` scene still runs. Second increment (renderer): the ECS
  `SceneRenderer` discards masked fragments below the cutoff, writes alpha 1
  for opaque/mask, and draws blended instances last through a second pipeline
  (alpha blend, depth test without depth write), sorted back to front along
  the camera view each frame. GPU test
  `alpha_modes_render_opaque_mask_and_sorted_blend` (RTX 3060, headless
  readback) fails when sorting is disabled; unit test
  `blended_objects_render_last_unbatched_and_back_to_front`. Known ceiling:
  blended GPU-physics bodies sort by their last CPU transform.
- [x] Import node hierarchy and cameras/lights where available.
  `AssetServer::import_gltf_scene` returns one `ImportedGltfNode` per glTF
  node (name, parent index, local `Transform` from the decomposed TRS,
  mesh primitives, `Camera`, `KHR_lights_punctual` light); `spawn_gltf_nodes`
  spawns them with `Parent` links, extra primitives as child entities.
  Evidence: unit test
  `gltf_scene_import_spawns_hierarchy_cameras_and_lights` (parent/child
  links, quaternion to Euler, orthographic camera, spot light, spawned
  components). Light intensities keep glTF physical units. The editor's
  flat `import_gltf` path is unchanged (AGENTS.md); wiring the editor to the
  node import is the owner's call.
- [x] Sample imported UVs and material textures in the ECS renderer.
  Found in the `issues.md` audit. `SceneVertex` now carries UVs, and the
  fragment shader multiplies the material color by the base-color texture
  (set 1, one descriptor set per texture, white when a material has none or
  its texture is still loading). Uploads go through a staging buffer copied
  in the frame's own command buffer, with no CPU wait. sRGB textures use an
  `_SRGB` format; each glTF sampler (filter, wrap) gets a cached `Sampler`.
  A changed texture revision re-uploads, and removed textures leave the
  cache like meshes. Evidence: GPU tests
  `base_color_texture_tints_the_surface_and_reloads_next_frame` (fails with
  sampling disabled in the shader) and `material_without_texture_samples_white`.
  Known ceiling: no mip chain yet. The other maps landed with the Milestone 4
  PBR pass.
- [ ] Add skins and animation after the static scene path is stable.
  Blocked: needs the skeleton/clip/pose runtime from Milestone 14; import
  lands together with that runtime rather than as data nothing consumes.
- [x] Preserve glTF materials unless the caller explicitly supplies an override.
  `spawn_gltf_nodes(app, nodes, material_override)` keeps each primitive's
  imported material when the override is `None`. Fixed a real loss: glTF
  textures used synthesized `.rtexture` keys that never existed on disk, so a
  saved scene with a glTF material failed to load in a fresh process. The
  importer now writes `.rtexture` (bincode, carrying color space and sampler)
  next to the source like `.rmesh`, and `load_texture` decodes it. Evidence:
  unit test `gltf_materials_survive_save_and_fresh_load_unless_overridden`
  (factors, alpha mode, linear normal map, sampler survive save plus fresh
  load; override replaces material); fails with the `.rtexture` write
  removed.

### Scene serialization

- [x] Define versioned, human-readable `.rscene` files with Serde plus a compact cooked binary form.
- [x] Assign stable UUIDs to serialized scene objects.
- [x] Store asset paths/UUIDs rather than runtime handles or ECS entity IDs.
- [x] Add an allowlisted component serialization registry.
- [x] Serialize hierarchy, transforms, renderers, lights, physics, and editor metadata.
  Hierarchy, transforms, renderers, cameras, visibility, classes,
  directional/point/spot lights, and physics were already typed scene fields.
  The missing `AmbientLight` component is now built into the default
  `SceneComponentRegistry` as `rusting.ambient_light`, so the scene binary
  layout is unchanged and the editor can add it like other registered
  components. Evidence: `all_authored_light_types_round_trip_together` now
  round-trips an ambient light through cooked bytes and fails with the
  registration removed. Editor metadata is split out below.
- [x] Serialize per-scene editor metadata (editor camera pose, selection).
  `save_editor_scene` writes the editor camera position, rotation, orbit
  distance, and selection (by `SceneId`, active object first) to a
  `<scene>.editor.json` sidecar, like Godot's per-scene edit state, so the
  game's scene file stays clean. `load_editor_scene` restores them after a
  Replace load; a missing or corrupt sidecar only clears the selection. Every
  editor save and open path, and the startup restore, uses the pair.
  Evidence: unit test
  `editor_scene_restores_camera_pose_and_selection_after_reload`.
- [x] Reject unsupported scene versions with a clear structured error.
- [x] Add explicit migration for legacy unversioned project and text-scene files.
- [x] Add scene save, load, additive load, and unload operations.

### Hot reload

- [x] Watch source assets and scenes for changes.
  `Assets::changed()` compares each path-backed slot's file modification
  time with the one recorded at load (stdlib polling, no new dependency);
  missing files are ignored so save-by-rename never drops a value. It works
  for every asset type, including `scenes`. `AssetPlugin` scans every
  `HOT_RELOAD_SCAN_INTERVAL` (500 ms). Known ceilings: one `stat` per
  watched file per scan; editing a source `.gltf` does not re-import it
  (only rewritten `.rmesh`/`.rtexture` reload).
- [x] Decode changed assets in worker tasks.
  `Assets::reload_async` decodes on a worker while the old value stays
  visible; `poll_loads` publishes the new value and bumps the revision, and a
  failed decode keeps the last good value and lands in
  `take_reload_failures`. `AssetServer::reload_changed` wires meshes and
  textures. Evidence: unit test
  `changed_files_reload_on_workers_and_failures_keep_last_value` (unchanged
  file skipped, old value visible mid-decode, new value published with a
  higher revision, corrupt rewrite keeps the old value and reports one
  failure, deleted file ignored).
- [x] Swap prepared resources at safe frame boundaries.
  Reloads publish in `poll_loads` during the ECS `Update` stage, so assets
  never change mid-render; `SceneRenderer::prepare_visible_meshes` sees the
  new revision at the start of the next frame and uploads fresh buffers
  instead of writing into in-use ones. Evidence: GPU test
  `hot_reloaded_mesh_swaps_next_frame_and_old_buffers_outlive_in_flight_frame`
  (frame 2 shows the reloaded mesh while frame 1 is still unwaited; a
  control run without the reload reads white, so the black result proves the
  swap).
- [x] Keep old GPU resources alive until every referencing frame completes.
  Old buffers are released by ownership: each submitted command buffer holds
  `Arc`s to the buffers it binds, and the frame future keeps the command
  buffer until its fence completes. Evidence: the same GPU test holds a
  `Weak` to the replaced vertex buffer; it stays alive while frame 1 is in
  flight and is freed once frame 1 is dropped after completion. Textures
  follow the same pattern: each frame's command buffer holds the descriptor
  set, and with it the image, it binds.
- [x] Surface reload success and failure in the editor console.
  `poll_loads` records the path of each published hot reload;
  `AssetServer::take_reloaded()` drains them, and the editor logs "Hot
  reloaded <path>" at Info and each failure at Error. Evidence: unit test
  `changed_files_reload_on_workers_and_failures_keep_last_value` (one path
  after a good reload, none after a failed one).

### Exit gate

- Loading the same asset path returns the same live asset identity.
- Saving and reloading a representative scene preserves all allowlisted state.
- Asset reload during rendering produces no invalid descriptor/resource use.
- Golden tests verify glTF channels, alpha modes, normals, and tangents.

## Milestone 3: Render extraction and frame management

Goal: eliminate frame-loop stalls and host/GPU races while making rendering derived from ECS state.

### Render extraction

- [x] Define a render world separate from the gameplay world.
- [x] Extract cameras, visible renderers, global transforms, lights, and material handles during `RenderExtract`.
- [x] Diff extracted state so unchanged render data produces no dirty uploads.
- [x] Build deterministic render keys and stable ordering from typed handles and entity identity.
- [x] Track contiguous dirty upload ranges and full-range reorder/removal updates.
- [x] Grow GPU buffers based on demand and device memory budgets.
  Render-instance uploads (rebuilt whenever anything moves) used a dedicated
  allocation per change; they now come from a vulkano `SubbufferAllocator`
  whose arenas start at 256 KiB, double when an upload outgrows them, and
  are reused once no in-flight frame references them. One upload is capped
  at half the largest device-local heap (`transient_upload_budget`); going
  over returns a `SceneRenderError` instead of allocating. Mesh buffers stay
  exact-size per revision. Evidence: unit test
  `transient_budget_is_half_the_largest_device_local_heap`; GPU test
  `instance_uploads_reuse_arenas_grow_on_demand_and_respect_budget` (two
  small uploads share one arena buffer, 5,000 instances grow it and still
  render, a budget one byte short fails with an explicit error). Known
  ceiling: the budget uses static heap size, not live usage
  (`VK_EXT_memory_budget` is not exposed by vulkano 0.35).
- [x] Add explicit errors or fallback paths for allocation/capacity failures.
  Audit of `SceneRenderer`: every Vulkan buffer/descriptor/command
  allocation already maps to `SceneRenderError`; instance uploads over
  budget are an explicit error; missing meshes fall back to the cube and
  missing materials render magenta. The two silent paths now report:
  lights past `MAX_LIGHTS` (64) keep the first 64 and count the rest in
  `RenderCapacityDiagnostics::dropped_lights`; GPU physics event-buffer
  overflow accumulates into `physics_events_dropped` (still logged). Both
  are read through `SceneRenderer::capacity_diagnostics()`. Evidence: GPU
  test `lights_over_capacity_render_first_max_lights_and_report_the_rest`
  (67 point lights upload 64 and report 3 dropped; back to 1 light reports
  0). Physics overflow counting has no dedicated test; it sits on the same
  readback path as the existing GPU physics event tests.

### Frames in flight

- [x] Create two or three explicit `FrameContext` objects.
  `SceneRenderer` owns `[FrameContext; FRAMES_IN_FLIGHT]` (2), used
  round-robin by `render`. Each context holds the fence of its last
  submission, which also keeps that submission's resources alive. Every
  frame now ends in a fence signal before the caller's present, not only
  physics frames. Physics readbacks share that fence. Evidence: GPU test
  `frame_contexts_bound_frames_in_flight_and_wait_only_on_reuse`.
- [x] Give each context its own fence, command allocator/pool state, uniform allocations, indirect buffers, visible lists, and transient descriptors.
  Each `SceneRenderer` `FrameContext` owns its fence and a host-visible
  `SubbufferAllocator` (64 KiB first arena). Debug-overlay vertices and GPU
  physics readback buffers now come from that arena instead of a dedicated
  allocation per frame. The physics descriptor set that references them is
  per-frame and pooled. The rest of the list maps as follows:
  command buffers come from vulkano's `StandardCommandBufferAllocator`,
  which pools per thread and recycles a buffer when its frame drops it.
  `SceneRenderer` has no uniform buffers (the camera is a push constant)
  and no indirect buffers. Visible lists are CPU-only. Evidence: GPU tests
  `transient_uploads_come_from_per_context_arenas_across_reuse` (the debug
  line renders in each of 4 frames, two full laps, and the two contexts
  never share an arena) and
  `physics_events_read_back_from_transient_arenas_every_tick` (one
  `WhileTrue` event per tick for 4 ticks, tick numbers match, no overflow).
  Before this test, no test covered the readback path through
  `SceneRenderer`. The legacy `Engine` still shares one indirect buffer per
  batch across frames; see the next two items.
- [x] Wait only when reusing a frame context whose fence has not completed.
  At the start of `render`, contexts whose fence already signaled are
  released without waiting. The one context being reused is waited on only
  if its fence has not signaled; that wait bounds CPU run-ahead to
  `FRAMES_IN_FLIGHT` frames. Evidence: the same GPU test hands back two
  frames unflushed. The third `render` submits and waits for frame 1 only;
  frame 2's fence stays unsignaled. With the wait removed, the test fails
  ("reusing context 0 first submitted and waited for its frame"). Frame
  pacing on real hardware is not verified here (hardware tier).
- [x] Chain acquire, transfer/compute, render, and present without CPU `wait()` calls in the normal frame path.
  In the product path (`project_runner` + `SceneRenderer`), one future
  chain runs from acquire to present: the acquire future is `render`'s
  `before`, physics compute and drawing share one command buffer, then come
  the frame fence and `present(.., false)`, which does not wait. Physics
  events are collected with `is_signaled` plus a zero-timeout cleanup, so
  that never blocks either. The only CPU block left is the bounded
  context-reuse wait from the item above. Evidence: the frame-context test
  shows a context that is not being reused is never waited on. Out of
  scope: the legacy `Engine` facade still waits on its separate compute and
  cull submissions. It follows the Milestone 0 precedent: that facade is
  deferred to its Milestone 1 replacement, not patched.
- [x] Retain every submitted future/fence until completion.
  Frame fences stay in their `FrameContext` until they signal or their
  context is reused after a wait. The present future is retained by
  `vulkano_util` (`previous_frame_end`), and physics readbacks by
  `pending_physics`. Evidence: the frame-context test asserts no
  unfinished fence is released. The hot-reload test shows old mesh buffers
  live until the frame that used them completed and the next frame
  released its fence.
- [x] Add deferred GPU-resource destruction queues per frame context.
  No separate queue was added; ownership already provides the deferral.
  A vulkano command buffer holds an `Arc` to every buffer, image, descriptor
  set and framebuffer it uses. Each `FrameContext` fence holds that command
  buffer until the GPU finishes. So anything the renderer replaces (mesh
  and instance buffers, lights, the depth target and framebuffers on
  resize) is freed only after its last frame completes. It is freed on the
  next `render` after that, with no wait. Evidence: GPU tests
  `hot_reloaded_mesh_swaps_next_frame_and_old_buffers_outlive_in_flight_frame`
  (mesh buffers) and
  `resize_defers_old_depth_destruction_until_its_frame_completes` (the
  depth target is replaced while frame 1 is pending, stays alive until
  frame 1 completes, then is released).
- [x] Clear culling counters and indirect commands using queued GPU operations.
  The legacy `Engine` reset `instance_count` with a host write every frame
  while the previous frame could still draw from that buffer. This made
  `user_main` panic in 2 of 10 runs with `AccessConflict(DeviceRead)`. The
  reset is now `record_reset_instance_count` (`src/scene/mod.rs`), a
  `fill_buffer` of that one field recorded before the cull dispatch. The
  cull and physics submissions are now chained after the previous frame
  instead of starting from `sync::now`. Evidence: GPU test
  `instance_count_reset_is_a_queued_gpu_fill_of_that_field_only`, and
  `user_main` ran to the timeout in 25 of 25 runs, about 3000 FPS as before.
  Known ceiling (`ponytail:` comment): on frames that run physics or
  culling, the CPU now waits for the previous frame, so CPU and GPU do not
  overlap there. Per-slot buffers would restore the overlap.
- [x] Remove host writes to buffers that may still be in GPU use.
  Audit of every `.write()` in the render paths: the legacy per-slot UBO is
  written only after that slot's fence wait. `SceneRenderer` writes only
  fresh suballocations: instance and transient arenas are reused only once
  no frame holds them, and vulkano's host-access lock rejects any
  conflicting write with an error, never a race. The one racing write, the
  indirect reset in the legacy `Engine`, was removed in the item above.

### Capabilities and fallback policy

- [x] Define a low-end Vulkan baseline suitable for Intel UHD 620-class hardware.
  `LOW_END_BASELINE` (`src/rendering/scene_renderer.rs`) requires:
  - Vulkan 1.1
  - `maxComputeWorkGroupInvocations` and `maxComputeWorkGroupSize[0]` of
    at least 256, which the physics shader's `local_size_x` needs
  - `maxPushConstantsSize` of at least 128 (the largest block used is 96
    bytes)
  - `maxStorageBufferRange` of at least 128 MiB
  - `D32_SFLOAT` usable as a depth attachment
  Every limit except compute invocations is the Vulkan-required minimum.
  No optional features or extensions are required. `SceneRenderer::new`
  reads `DeviceLimits` from the physical device and refuses a device
  below the baseline with an error that names the device and every
  shortfall. The instance upload budget is now also clamped to
  `maxStorageBufferRange`, because half of a large heap could exceed the
  range one storage binding may cover. Evidence: unit test
  `baseline_shortfalls_name_every_missing_property`, and GPU test
  `test_device_meets_the_low_end_baseline` on the headless test device.
  Not verified on a real UHD 620 (hardware tier). Its limits are expected,
  not measured, to meet the baseline.
- [x] Detect optional capabilities and expose them through renderer capabilities.
  `SceneRenderer::capabilities()` returns `RendererCapabilities`
  (`src/rendering/scene_renderer.rs`), detected once in `SceneRenderer::new`:
  - device name, integrated-GPU flag, and largest device-local heap size
  - multi-draw indirect, indirect-count drawing (core feature or
    `VK_KHR_draw_indirect_count`), bindless textures (all four descriptor
    indexing bits), and `VK_EXT_memory_budget`, each reported as
    `supported` by the GPU and `enabled` on the device
  - timestamp queries on graphics and compute queues

  A pass may use a feature only when `Capability::usable()` holds. No optional
  feature is enabled on the device yet, because no pass uses one.
  Evidence:
  - Unit test
    `optional_features_need_every_bindless_bit_and_accept_either_indirect_count_source`.
  - GPU test
    `renderer_reports_detected_capabilities_and_enables_none_it_does_not_use`.
    On the RTX 3060 it reports all four as supported and none as enabled.
- [x] Provide fallbacks for bindless descriptors, indirect-count drawing, and other advanced features.
  Audit of every draw call and shader: `SceneRenderer` uses only direct
  `draw`/`draw_indexed` and fixed descriptor sets, so its current path is
  already the baseline fallback. No shader declares a GLSL extension or a
  descriptor array. The GPU suite runs on a device with no optional feature
  enabled, which the capabilities test asserts. A pass that adopts an
  advanced feature later must branch on `Capability::usable()` and keep this
  path tested.
  The audit found one path that silently depended on an optional feature:
  the legacy `Engine` culling path wrote each batch's offset into the
  indirect command's `firstInstance`, which needs
  `drawIndirectFirstInstance`. That feature is never enabled, so the result
  was undefined. Now `firstInstance` is 0 and `base.vert` adds the batch
  offset from the push constant it already receives
  (`data[v_visible_list_offset + gl_InstanceIndex]`). A dead, unused
  indirect staging buffer next to it was deleted.
  Evidence: the shader compiles and the smoke runs pass. The culled image
  itself is not checked by a test; the exit gate's culling-equivalence check
  covers that.
- [x] Implement `QualityProfile::{Auto, Eco, Balanced, High}`.
  `RenderSettings::quality` is now extracted into `RenderWorld::quality`
  every frame. `resolve_quality` (`src/rendering/scene_renderer.rs`) turns
  `Auto` into a concrete profile from `RendererCapabilities`:
  - `Eco` on integrated GPUs
  - `Balanced` below 4 GiB of device-local memory
  - `High` otherwise
  Concrete profiles pass through unchanged. Today the resolved profile sets
  the per-frame light budget: 16, 32 or 64 lights (`light_budget`). Lights
  past the budget are dropped and counted in
  `RenderCapacityDiagnostics::dropped_lights`. A profile change rebuilds the
  light list even when the lights did not change. Later budgets (shadows,
  culling mode, substeps) plug into the same resolved profile.
  Evidence:
  - Unit test
    `auto_quality_resolves_from_device_class_and_concrete_profiles_pass_through`.
  - GPU test `lights_over_capacity_render_first_max_lights_and_report_the_rest`
    now switches High to Eco with unchanged lights and checks the 16-light
    upload. It fails (3 dropped instead of 51) without the rebuild on profile
    change.
  The `Auto` heuristic is static, marked with a `ponytail:` comment. Whether
  it picks the right profile on a real UHD 620 is hardware tier.
  The editor's Project area shows the resolved profile, marked "(Auto)"
  when it was picked automatically, and counts loaded LOD groups next to
  meshes. Evidence:
  `editor::tests::project_area_shows_lod_groups_and_the_resolved_auto_quality`.
- [x] Remove shader variants as user-facing performance controls; choose implementation variants automatically.
  The ECS/editor path (`SceneRenderer`) exposes no performance variant:
  - It builds one scene pipeline.
  - Materials choose appearance through `MaterialModel::{Pbr, Unlit}`, never cost.
  - Cost is set only through `QualityProfile`, which `Auto` resolves from
    capabilities (item above).
  The legacy `Engine` path's public `ShaderType` (including the
  benchmark-only `Heavy`) was removed by the Milestone 4 item "Replace
  public `ShaderType` with `MaterialModel`...". Solver selection (`PhysicsSolver`) is a semantic
  choice, and "Keep manual CPU/GPU and solver selection available" keeps it
  manual on purpose. Evidence: code audit only; no code changed.

### Exit gate

- No normal-frame CPU fence waits remain.
- Validation reports no synchronization or resource-lifetime errors.
- Two/three frames in flight can run for 10,000 frames without UBO, indirect-buffer, or descriptor races.
- Culling enabled and disabled produce equivalent visible geometry.

## Milestone 4: Rendering baseline

Goal: deliver a coherent, production-shaped forward renderer for the vertical slice.

### Pass schedule

- [x] Batch opaque ECS renderables by mesh and material and draw them through a cached GPU instance buffer.
- [x] Directional shadow-map pass.
  `SceneRenderer` records a depth-only pass into a 2048x2048 `D32_SFLOAT`
  shadow map before the main pass, drawing opaque/mask batches with
  `cast_shadows` (others collapse to a zero-area triangle in the shadow
  vertex shader). It shares the main pipeline layout and set 0, uses
  slope-scaled depth bias, and is always cleared so the main pass samples
  initialized depth. The orthographic light box covers `SHADOW_DISTANCE`
  (50 m) in front of the camera. Evidence: GPU test
  `directional_light_shadow_darkens_receivers_only_when_enabled` (off-view
  caster darkens the floor; non-caster, non-receiver, and `shadows: false`
  stay lit); fails with r=253 when the shadow factor is forced to 1.
  Known ceilings: one cascade, no texel snapping (edges shimmer as the
  camera moves), masked casters cast fully opaque, blended casters none.
- [x] Opaque forward PBR pass.
  The ECS fragment shader is metallic-roughness Cook-Torrance (GGX, Smith-
  Schlick, Schlick Fresnel) for every light kind, plus emissive. Light units
  are unchanged: a white Lambert surface facing a unit light still reflects
  1. `MaterialModel::Unlit` outputs the base color. Built-in primitives now
  get real tangents from `generate_tangents` instead of placeholders.
  Ambient became a sky/ground hemisphere with the sky/environment item.
  Evidence: GPU test `pbr_maps_and_unlit_model_shape_the_lit_color` (unlit
  ignores light; PBR without light is black; emissive factor and map; the
  occlusion map darkens ambient; a rough metal is darker than a dielectric
  and a metallic-roughness map with blue 0 restores the dielectric; a normal
  map tilted toward a grazing light brightens the surface). Disabling the
  normal, metallic-roughness, occlusion, or emissive sample in the shader
  each fails it.
- [x] Transparent forward pass with back-to-front sorting.
  Already delivered by Milestone 2 "Support alpha opaque, mask, and blend
  modes": second pipeline, per-frame back-to-front sort. Evidence: GPU test
  `alpha_modes_render_opaque_mask_and_sorted_blend` (fails with sorting
  disabled), unit test `blended_objects_render_last_unbatched_and_back_to_front`.
- [x] HDR render target and tone-mapping pass. The scene pass lights into an
  `R16G16B16A16_SFLOAT` target (subpass 0); subpass 1 reads it as an input
  attachment and maps it into the output with a `ToneMapping` component
  (`Linear` clip by default like Godot, `Reinhard`, `Aces`, plus exposure;
  saved as `rusting.tone_mapping`). Debug views and editor lines skip the
  curve. Evidence: GPU test `hdr_values_above_one_survive_until_tone_mapping`
  (fails with an 8-bit target, with the Reinhard branch removed, and with the
  debug-view bypass removed); scene-file round trip includes the component;
  all earlier renderer GPU tests pass unchanged.
- [x] Bootstrap Vulkan egui overlay/compositing pass for the runnable editor.
- [x] Engine-owned egui compositing pass shared with the production renderer.
  `rendering::egui_painter::EguiPainter` draws egui meshes in its own
  load/store render pass over any target, including the `SceneRenderer`
  output; `egui_winit_vulkano` is removed. It is behind the `editor` feature
  because that feature owns the egui dependency. Evidence: GPU tests
  `meshes_blend_over_the_target_in_points_and_respect_clip_rects` and
  `texture_patches_update_one_region_and_freed_textures_stop_drawing` (they
  fail with the sRGB output conversion removed, with the clip scissor
  widened to the full target, and with partial updates written at offset 0);
  the editor ran 8 s on Linux/Wayland and drew its UI with no validation
  panic.
- [x] Explicit pass resources, transitions, and debug labels.
  `rendering::frame_passes` declares the frame's passes in order (Uploads,
  Physics, Shadow, Scene, ToneMap, DebugOverlay), the resources each one
  reads and writes, and the image layout it needs. `layout_transitions()`
  derives the three layout changes (material textures and the shadow map to
  shader-read before Scene, HDR color to shader-read before ToneMap). The
  shadow render pass takes its final layout from the table, so the render
  pass does that transition; vulkano still inserts the remaining barriers
  until the Milestone 13 render graph. Each pass is recorded under its own
  debug-utils label inside the frame's `SceneRenderer::render` label, and
  `SceneRenderer::last_frame_passes()` reports the recorded order. The
  headless test instance now enables `ext_debug_utils` when the driver has
  it, so every GPU test records the labels. Evidence: unit tests in
  `frame_passes` (every read follows a write, unique labels, exact
  transition list); GPU test
  `frames_record_the_declared_passes_with_their_layouts` (pass order, shadow
  final layout, HDR input layout; fails with the shadow final layout
  removed); the 38 `scene_renderer` GPU tests under
  `VK_LAYER_KHRONOS_validation` on an RTX 3060 report no validation error,
  while a removed Shadow label begin reports
  `VUID-vkCmdEndDebugUtilsLabelEXT-commandBuffer-01912`.

### Materials and lighting

- [x] Replace public `ShaderType` with `MaterialModel::{Pbr, Unlit}` plus internal shader variants.
  The legacy `Material` now carries `model: MaterialModel` (builder
  `.model(...)`), and `Engine::set_scene_shader` takes a `MaterialModel`.
  `ShaderType` is no longer re-exported from the crate root; it stays in
  `rendering::shader_registry` as the legacy renderer's internal variant
  enum, mapped by `From<MaterialModel>`. The `Emissive`, `NormalDebug`, and
  `Heavy` variants are no longer selectable from the public API; the Pbr
  variant already renders emissive. Evidence: unit tests
  `material_builder_sets_model`, `material_builder_model_can_be_overridden`,
  and `instance_material_copy_preserves_every_property` (an Unlit material
  maps to the Unlit variant); doc tests on `Material` and `MaterialBuilder::model`.
- [x] Complete metallic-roughness PBR texture binding and sampling.
  Set 1 is now one cached descriptor set per material holding its five maps
  (white where absent), rebuilt only when a map is re-uploaded. Metallic and
  roughness factors, emissive, and the model travel in the instance buffer,
  which is already rebuilt on material revision changes. Evidence: as above.
- [x] Support base color, normal, metallic-roughness, occlusion, and emissive maps.
  Normal maps use the vertex tangent and handedness (glTF convention);
  metallic-roughness reads blue and green, occlusion red. Evidence: as above.
- [x] Support alpha cutoff and alpha blending.
  Already delivered by Milestone 2 "Support alpha opaque, mask, and blend
  modes": second pipeline, per-frame back-to-front sort. Evidence: GPU test
  `alpha_modes_render_opaque_mask_and_sorted_blend` (fails with sorting
  disabled), unit test `blended_objects_render_last_unbatched_and_back_to_front`.
  Later: blending is premultiplied. Diffuse and emissive scale by alpha,
  specular and environment reflections do not, so glass at alpha 0 still
  mirrors its surroundings. Known ceiling: no refraction or colored tint of
  what is behind; that needs a copy of the opaque color (Milestone 13
  "Refractive glass"). Evidence: GPU test
  `clear_glass_shows_what_is_behind_it_and_reflects_the_environment` (fails
  with the old `SrcAlpha` color factor: no red reflection).
- [x] Add one shadowed directional light.
  The first uploaded `DirectionalLight` with `shadows: true` is shadowed;
  its index reaches the fragment shader in `light_info.y`, and receivers
  (`receive_shadows`) apply 3x3 PCF through a comparison sampler. Evidence:
  same GPU test as the shadow-map pass.
- [x] Add several point lights with capability-appropriate limits.
  Point (and spot) lights share the light storage buffer with directional
  lights and fade to zero at `range` (squared linear fade). The per-frame
  budget comes from the resolved `QualityProfile`: Eco 16, Balanced 32,
  High 64 (`MAX_LIGHTS`), with `Auto` resolved from device capabilities.
  Lights past the budget are dropped and counted in
  `RenderCapacityDiagnostics::dropped_lights`. Known ceiling: every
  fragment loops over every uploaded light; no clustered culling yet.
  Evidence: GPU test `several_point_lights_add_up_and_fade_out_at_their_range`
  (a second light adds its color, a light past its range adds nothing, two
  equal lights are brighter than one); forcing the range fade to 1 or
  capping the shader loop at one light each fails it. GPU test
  `lights_over_capacity_render_first_max_lights_and_report_the_rest` and
  unit test for the ordered profile budgets cover the limits.
- [x] Add sky/environment ambient lighting.
  New `SkyLight { sky_color, ground_color, intensity }` component
  (registered as `rusting.sky_light`, so the editor can add it and scenes
  round-trip it) adds a hemisphere on top of any `AmbientLight`: +Y faces see
  the sky, -Y faces the ground. Diffuse samples it along the normal and
  specular along the reflection with roughness-aware Fresnel, so metals are
  no longer black without direct light. The 0.12 gray fallback applies only
  when neither component exists. The camera push constant grew to 128 bytes
  (`ground_ambient`), the guaranteed minimum. Known ceiling: two colors, no
  cubemap or HDRI; the sky system milestone brings image-based lighting.
  Evidence: GPU test `sky_light_lights_up_faces_with_sky_and_down_faces_with_ground`
  (top face red sky, bottom face blue ground, smooth metal reflects the sky,
  gray fallback without either light); ignoring the normal's Y, using the
  normal's Z, or dropping the environment specular each fails it. Unit test
  `all_authored_light_types_round_trip_together` round-trips a sky light.
  `base_color_texture_tints_the_surface_and_reloads_next_frame` now uses an
  unlit material, since a lit dielectric correctly reflects 4% of the
  environment.
- [x] Add material/texture fallback behavior and diagnostic rendering.
  Missing meshes draw the fallback cube and missing materials draw
  magenta (both existed). New: a referenced map that is missing or failed
  to upload binds a per-slot stand-in (magenta base color, flat normal,
  white for the rest) instead of white, so a broken base color map is
  visible and a broken normal map no longer tilts the shading.
  `RenderCapacityDiagnostics` now counts `missing_meshes`,
  `missing_materials`, and `missing_textures`. `SceneRenderOptions` gained
  `debug_view: SceneDebugView::{Lit, Unshaded, Normals}` (shader reads it
  from `light_info.z`); the editor's viewport settings popup has a
  "Viewport shading" section, and only the Scene workspace uses it.
  Evidence: GPU tests `missing_assets_draw_visible_fallbacks_and_are_counted`
  (a removed or malformed base color map draws magenta; a missing normal
  map under a grazing light matches no normal map; a removed material and
  mesh draw a magenta cube; all counters) and
  `debug_views_show_unshaded_base_color_and_normals`. Replacing the magenta
  or flat-normal stand-in with white, or disabling the normals view, each
  fails them. Unit test `scene_view_shading_applies_only_in_the_scene_workspace`.

### Visibility and quality

- [x] Add `RenderBounds` as a rendering-only component independent from the physics `Collider`.
  `rusting_core::components::RenderBounds` (default: the unit cube's box),
  saved as `rusting.render_bounds`. Evidence: scene-file round-trip test
  includes it.
- [x] Support local bounding spheres and axis-aligned boxes, transformed into world bounds during render preparation.
  `RenderBounds::{Sphere, Aabb}` are local; `RenderBounds::transformed`
  scales a sphere's radius by the largest axis scale and turns a box into
  the axis-aligned box of its transformed corners (Arvo). Extraction carries
  the local override in `ExtractedRenderable::bounds`; `SceneRenderer`
  transforms it (or the mesh box) into world bounds when it prepares
  instances. Evidence: unit tests
  `render_bounds_stay_conservative_under_transforms` (rotated, non-uniformly
  scaled, translated sphere and box; fails without the per-axis min/max) and
  `render_bounds_overrides_are_extracted_and_track_edits` (fails when the
  `RenderBounds` change tick is not tracked).
- [x] Generate default render bounds automatically for primitives and imported meshes, with an explicit editor override for unusual meshes and animations.
  An object without `RenderBounds` gets the box around the vertices of the
  mesh the renderer actually uploaded (`PreparedMesh::bounds`), so built-in
  primitives, imported glTF meshes, the fallback mesh shown while a mesh
  loads, and reloaded meshes are all covered. The prepared instances record
  the mesh revisions their bounds came from and rebuild when one changes,
  without a scene change. A `RenderBounds` component overrides the mesh box.
  Inspector editing of that override is the later "Render Bounds editing"
  item. Evidence: GPU test
  `culling_follows_the_camera_and_mesh_edits_without_scene_changes` (a mesh
  edit alone un-culls an object; fails when the mesh revisions are not
  checked).
- [x] Add `CullingMode::{Auto, Disabled, Frustum, FrustumAndOcclusion}`. Start with the first three modes; reserve occlusion culling for a later pass.
  `RenderSettings::culling` (default `Auto`) is extracted into
  `RenderWorld::culling`. When culling is on, `SceneRenderer` tests each
  instance's world bounds against the Gribb-Hartmann planes of the camera
  clip matrix on the CPU. It compacts the survivors into a visible list:
  each batch stays contiguous and draws once, and each kept blended instance
  gets its own slot. The main and blend vertex shaders read the instance
  index from that list as a per-instance vertex attribute. The list is
  cached until the camera clip matrix, the culling mode, or the prepared
  instances change, so a still camera costs nothing. The shadow pass still
  draws every caster from the instance buffer directly.
  `FrustumAndOcclusion` culls by frustum for now. How `Auto` picks a path
  is described in the selection and small-scene items below. The CPU path
  never culls GPU-physics bodies, because their CPU transform is stale.
  Frames with GPU bodies take the GPU path, which culls them. Evidence:
  - Unit tests: `frustum_test_keeps_bounds_that_touch_the_view_and_drops_the_rest`
    (sphere and box on each side of the side, near, and far planes) and
    `compaction_keeps_batches_contiguous_and_slots_blended_instances`.
  - GPU tests: `culling_follows_the_camera_and_mesh_edits_without_scene_changes`
    (the kept instance of a partly culled batch draws, a mesh edit
    un-culls, a camera move re-culls);
    `directional_light_shadow_darkens_receivers_only_when_enabled` (an
    off-view caster is culled and still casts, `Disabled` culls nothing,
    an override outside the view removes the floor from the image); and
    `gpu_bodies_are_not_culled_by_their_stale_cpu_transform`.
  - Mutations that fail these tests: the shader indexing with
    `gl_InstanceIndex`, the cache ignoring mesh revisions, the cache
    ignoring the camera, drawing whole batches, culling GPU bodies,
    dropping plane normalization for spheres, and the OpenGL near plane
    (`w + z`).
  - The validation layer reports 0 errors across 42 `scene_renderer` tests.
- [x] Select CPU or GPU frustum culling by object count, simulation ownership, device capability, and quality profile.
  `select_culling_path` (`src/rendering/scene_renderer.rs`) returns a
  `CullingPath` each frame:
  - `Disabled` gives `Direct`: every instance is drawn.
  - Any rendered GPU-owned body gives `Gpu`, because only the GPU has its
    newest transform.
  - Otherwise `Gpu` from `GPU_CULL_MIN_INSTANCES` (4096) instances, or four
    times that on integrated GPUs and with the resolved `Eco` profile.
  - Below the threshold, `Cpu`.

  The threshold is untimed and marked with a `ponytail:` comment; the
  profiler item replaces it with measured cull times.
  `SceneRenderer::last_culling_path()` reports the choice.
  `last_frame_culled()` is now `Option<usize>`: `None` on the GPU path,
  whose count stays on the GPU.
  Evidence: unit test
  `culling_path_follows_mode_ownership_count_device_and_quality`, which
  covers the threshold edge, GPU ownership, integrated GPUs, `Eco`, and
  `Disabled`.
- [x] Cull GPU-owned objects directly from their newest GPU physics transforms. Do not read every transform back to the CPU merely to decide visibility.
  The `cull_shader` compute pass (`FramePass::Culling`, after Physics)
  takes each instance's model from the physics state when it is a GPU
  body, and from the instance buffer otherwise. It transforms the local
  bounding sphere and tests the six clip planes. Nothing is read back to
  the CPU.
  The GPU tests a sphere around the mesh or `RenderBounds` box, so it keeps
  some instances that the CPU box test drops (`ponytail:` comment).
  Evidence: GPU test
  `gpu_culling_counts_visible_instances_from_gpu_transforms_into_indirect_draws`.
  A GPU body has its stale CPU transform in the view center and its GPU
  state at x = -3. The cull pass counts only the static cube in that
  batch. With the camera moved to x = -3, it counts only the body, and the
  body draws at its GPU position.
  `gpu_bodies_are_not_culled_by_their_stale_cpu_transform` now runs on the
  GPU path and still draws the body.
- [x] Make GPU culling write a compact visible-instance list and indirect draw commands consumed by the normal instanced renderer.
  Each GPU-culled frame allocates fresh buffers:
  - an indirect command per opaque batch and per blended instance, written
    by the host with `instanceCount` 0
  - a visible list sized like the instance buffer

  The pass `atomicAdd`s each visible instance into its draw's count and
  writes the instance index into that draw's range of the list. The main
  and blend pipelines are unchanged. They read `instance_index` from the
  list as a per-instance vertex attribute.
  Each draw binds the list sliced at its group's start and calls
  `draw_indexed_indirect` with `firstInstance` 0. This avoids
  `drawIndirectFirstInstance`, which is not enabled. The CPU path now draws
  through the same slices.
  Evidence:
  - The GPU test above reads back the commands after the fence: `[1, 1, 1]`
    (one opaque batch, two blended draws), then `[1, 0, 0]` after the camera
    moves. The frame's passes include `Culling`.
  - Unit test `cull_gpu_layouts_match_shader_structs` checks the struct
    sizes against the std430 and push-constant layouts, and the box-to-sphere
    radius.
  - Mutation checks that fail the tests: the shader ignoring the physics
    model, the shader skipping the plane test, all blended instances
    counting into one draw, and selection ignoring GPU ownership.
  - Not caught by a test: drawing the full batch range directly instead of
    indirectly, and ignoring the list slot base. Both leave the tested pixels
    unchanged.
  - The validation layer reports 0 errors across 45 `scene_renderer` tests.
- [x] Let `Auto` bypass the culling dispatch for small scenes where direct rendering is cheaper, using a tested threshold before timing-based selection exists.
  `select_culling_path` returns `Direct` for `Auto` below
  `AUTO_DIRECT_MAX_INSTANCES` (64) instances, even when there are GPU
  bodies: drawing everything is correct whoever owns the transform. An
  explicit `Frustum` still culls small scenes. The threshold is untimed
  (`ponytail:` comment); the profiler item replaces it with the measured
  break-even. The `SlabScene` GPU tests set `Frustum`, because every test
  scene is below the threshold.
  Evidence:
  - Unit test `culling_path_follows_mode_ownership_count_device_and_quality`
    covers 63 instances (`Direct`, with or without GPU bodies), 64
    (`Cpu`), and `Frustum` at 10 instances (`Cpu`, or `Gpu` with a GPU
    body).
  - GPU test `gpu_bodies_are_not_culled_by_their_stale_cpu_transform`
    switches to `Auto`. The frame takes `Direct` with no `Culling` pass,
    culls nothing, and still draws the body.
  - Mutation checks that fail the tests: removing the bypass, and `<=`
    in place of `<` at the threshold.
- [x] Keep render visibility separate from simulation activity: a culled object continues physics unless an explicit simulation-distance policy says otherwise.
  Culling results live only inside `SceneRenderer`: the visible list, the
  indirect counts, and `last_frame_culled`. No simulation code reads them.
  A search for `CullingPath`, `last_frame_culled`, and `Visibility` outside
  the renderer finds only extraction, the scene file, and the editor. The
  GPU physics dispatch runs over every body in the physics buffer before
  the cull pass, and the CPU physics systems do not query `Visibility`.
  No simulation-distance policy exists yet.
  Evidence: GPU test `culled_gpu_bodies_keep_simulating`. A GPU body sits
  at y = 5, above the ±1 view. On each of 4 ticks its draw count is 0
  (culled), and its `position_y < 5` rule fires, so gravity keeps moving
  it. With gravity removed, the test fails.
- [x] Add Render Bounds editing and debug visualization to the Inspector and viewport.
  The Mesh Renderer section has a "Render Bounds" choice: Mesh (no
  override), Box (Min/Max), or Sphere (Center/Radius). A new Box or Sphere
  starts from the mesh box, so the volume does not jump. Edits use the
  same Inspector snapshot path as every other field, so Undo covers them;
  choosing Mesh removes the component. With "Selected Bounds" on, the
  viewport outline of a selected object with an override shows the
  world-space volume that culling tests: the world box around the
  transformed corners, or three great circles of the scaled sphere.
  Without an override it still shows the mesh box.
  Evidence: unit tests `render_bounds_kinds_start_from_the_mesh_box`,
  `render_bounds_outline_uses_the_culled_world_volume` (box corners and
  sphere radius after translation and non-uniform scale), and
  `selected_outline_shows_the_render_bounds_override` (12 mesh-box lines
  become 96 sphere lines of radius 3). fmt and clippy are clean in all
  three configurations. Tests: 190+49+15, and 226+49+15 with `gpu-tests`.
  Not covered by a test: driving the egui combo box through a real frame.
  Later: a selected override shows a handle on each box face (or at the
  sphere's radius on each local axis). Dragging one moves that face along
  the object's local axis, stopping at the opposite face, or sets the
  radius; each drag is one Undo step. "Fit to Mesh" wraps the override
  around the mesh box again. Evidence: unit test
  `dragging_render_bounds_handles_moves_one_face_in_local_space` (a 2x
  X-scaled box's +X face dragged 2 m moves local max X by 1, min stays,
  face clamping and sphere radius).
- [x] Report submitted, visible, and culled instance counts plus culling compute time in the profiler.
  `SceneRenderer::culling_stats()` returns `CullingStats`: the path,
  submitted, visible, and culled counts, and the culling time. The
  `Direct` and `Cpu` paths time the CPU visibility pass. The `Gpu` path
  writes two timestamps around the cull dispatch and sums the indirect
  commands' instance counts. Both are read back once that frame's fence
  signals, without waiting, so the numbers lag a frame or two. The time is
  `None` when the queue has no timestamp support. The editor binary stores
  the stats as a resource each frame. The editor stats line then shows
  "Culling Gpu | 4 submitted, 3 visible, 1 culled | 0.012 ms". There is no
  dedicated profiler panel yet; that is backlog work in
  `docs/editor-overhaul.md`. The `GPU_CULL_MIN_INSTANCES` and
  `AUTO_DIRECT_MAX_INSTANCES` thresholds are still untimed. Tuning them
  needs these numbers from real hardware.
  Evidence: GPU test
  `gpu_culling_counts_visible_instances_from_gpu_transforms_into_indirect_draws`
  checks the read-back counts (4/3/1, then 1 visible and 3 culled) and that
  the time is `Some`. It fails when `culling_stats` skips the readback, and
  when the end timestamp goes to the wrong query. The CPU-path counts
  (2/1/1) and time are checked in the shadow-caster culling GPU test. Unit
  test `culling_stats_label_shows_counts_and_time` covers the label. The
  validation layer reports 0 errors across the 46 `scene_renderer` tests.
  fmt and clippy are clean in all three configurations. Tests: 191+49+15,
  and 227+49+15 with `gpu-tests`.
- [x] Add hierarchical-Z occlusion culling after frustum culling, depth-pyramid generation, and conservative-bound tests are stable.
  `CullingMode::FrustumAndOcclusion` now selects `CullingPath::GpuOcclusion`,
  a two-phase GPU cull. The early phase draws the opaque instances that
  passed the frustum and were visible last frame. A compute pass then
  builds a farthest-depth pyramid from that depth. The late phase tests
  every frustum-visible instance against the pyramid. It writes this
  frame's visibility history and draws only the instances the early phase
  skipped, in a late render pass that loads HDR and depth. Blended
  instances skip the history and go through the late phase. The occlusion
  test projects the corners of each bounding sphere's box and treats a box
  that crosses the near plane as visible. It then picks the mip where the
  rectangle covers at most two texels per axis and compares the nearest
  box depth with the farthest pyramid depth. The frame schedule gains the
  `DepthPyramid`, `OcclusionCulling`, and `LateScene` passes. The culling
  time sums both cull dispatches.
  Evidence: GPU test
  `occlusion_culls_hidden_objects_and_draws_revealed_ones_the_same_frame`
  hides a cube behind a wall. It checks the early and late draw counts
  over three frames ([0,2], [2,0], [1,0]) and the stats (2 submitted, 1
  visible, 1 culled). After the camera moves past the wall, the cube draws
  in the late phase of the same frame and the center pixel is red. GPU test
  `depth_pyramid_mips_hold_the_farthest_depth_below_them` reads back every
  mip of a 7×5 pyramid and compares it with a CPU max reduction, including
  the odd last row and column. Mutations the tests catch: `occluded()`
  always false, the early phase ignoring history, the late phase drawing
  early instances again, dropping the odd-edge fold, last texel instead of
  max, and the copy writing cleared depth. The validation layer, with
  synchronization validation on, reports 0 errors and no hazards across
  the 48 `scene_renderer` tests. fmt and clippy are clean in all three
  configurations. Tests: 191+49+15, and 229+49+15 with `gpu-tests`.
  Known limits: mip 0 of the pyramid is full resolution, a render-world
  instance rebuild resets the visibility history (one frame of extra
  late-phase draws), and the stats do not split frustum-culled from
  occluded counts.
- [x] LOD asset groups and distance/error-based selection.
  `LodGroupAsset` lists mesh levels finest first. Each level has a
  hand-over value. The `Distance` metric hands over at a world distance.
  The `ScreenSize` metric hands over when the bounding sphere covers less
  than a fraction of the view height, which tracks projected error across
  fields of view. `load_mesh` also loads a `.rlod` JSON file next to the
  mesh. Its level paths are relative to that file. A group applies to
  every renderable whose mesh is its finest level. The renderer expands
  each such renderable into one instance per level and gives each one a
  `[start, end)` range of a LOD value that grows with distance. The CPU
  cull and the GPU cull shader keep the instance whose range holds the
  value, so exactly one level draws, and nothing draws past the last
  level. Scenes with LOD groups skip the `Direct` path, and `Disabled`
  culling still selects levels. Only the finest level casts shadows.
  Evidence: GPU test
  `lod_groups_draw_the_level_for_the_camera_distance_on_every_path` moves
  an orthographic camera through level 0, level 1, and past the group on
  the `Cpu` (frustum and `Disabled`) and `GpuOcclusion` paths. It checks
  the drawn mesh per batch, the center pixel, the stats (2 submitted, 1
  visible, 1 culled), and that only level 0 casts shadows. Unit test
  `lod_value_grows_with_distance_like_group_ranges` checks level choice for
  perspective and orthographic cameras and `world_sphere`. Unit test
  `lod_group_beside_a_mesh_loads_with_it_and_rejects_bad_hand_overs` covers
  `.rlod` loading, deduplication, and the rejection of empty groups and of
  hand-overs that do not grow. Mutations the tests catch: the shader
  ignoring the range, the CPU cull ignoring the range, every level casting
  shadows, and path selection ignoring LOD groups. The validation layer,
  with synchronization validation on, reports 0 errors across the 50
  `scene_renderer` tests. fmt and clippy are clean in all three
  configurations. Tests: 193+49+15, and 232+49+15 with `gpu-tests`.
  Known limits: level changes pop with no cross-fade or hysteresis, the
  shadow pass draws level 0 at every range, `.rlod` files do not hot
  reload, and glTF `MSFT_lod` is not imported. Editor authoring is in the
  `docs/editor-overhaul.md` backlog.
- [x] Transparent sorting.
  Already delivered by Milestone 2 "Support alpha opaque, mask, and blend
  modes": second pipeline, per-frame back-to-front sort. Evidence: GPU test
  `alpha_modes_render_opaque_mask_and_sorted_blend` (fails with sorting
  disabled), unit test `blended_objects_render_last_unbatched_and_back_to_front`.
- [x] Shadow resolution and distance scaling by quality profile.
  `shadow_settings` maps the resolved profile to the directional shadow
  map size and the view distance it covers: `Eco` 1024 texels over 30
  units, `Balanced` 2048 over 50, and `High` 4096 over 80. The renderer
  recreates the shadow map and its framebuffer when the size changes.
  In-flight frames keep the old map alive. The shadow viewport and the
  light-space box follow the new values.
  Evidence: GPU test
  `directional_light_shadow_darkens_receivers_only_when_enabled` moves the
  camera 40 units from a shadowed floor. It checks that `Eco` leaves the
  floor lit (outside its distance) and that `Balanced` and `High` shadow it.
  It also checks the map size of each profile, including a switch back to
  `Eco`. Mutations the test catches: a fixed shadow distance, no map
  recreation, and a fixed viewport size. The validation layer, with
  synchronization validation on, reports 0 errors across the 50
  `scene_renderer` tests. fmt and clippy are clean in all three
  configurations. Tests: 193+49+15, and 232+49+15 with `gpu-tests`.
  Known limits: still one cascade with no texel snapping, and the sizes
  are fixed per profile rather than scaled with the output resolution.
- [x] Optional anisotropic filtering based on device support.
  Every device path (`init_vulkan`, `init_vulkan_headless`, and
  `rendering::vulkano_config` for the editor and exported games) enables
  `samplerAnisotropy` when the device has it. `vulkano_config` probes the
  device `VulkanoContext` will pick first, because `VulkanoContext` panics on
  a requested feature the device lacks. Texture samplers with linear
  minification filter at up to 16x, capped by the device limit; nearest
  filtering stays unfiltered. `RendererCapabilities::sampler_anisotropy`
  reports it. Evidence: unit test
  `anisotropy_needs_the_feature_and_linear_filtering_and_caps_at_16`, and
  GPU test `devices_enable_anisotropy_when_the_driver_supports_it` (fails
  when the headless device does not enable the feature, checked on an RTX
  3060). The editor starts without a panic with the feature on.
  Known limit: textures still have no mip chain, so distant textures alias.
- [x] Optional MSAA based on device support. The scene subpass renders
  into multisampled HDR and depth targets that resolve into the existing
  single-sample ones, so tone mapping, editor lines, and the depth pyramid
  are unchanged. `RendererCapabilities::msaa_samples` is 4 or 2 when the
  device can multisample both targets and resolve depth (Vulkan 1.2 or
  `VK_KHR_depth_stencil_resolve`), else 1. The MSAA passes and pipelines are
  built once at startup and share the single-sample pipeline layouts; each
  frame picks them unless the resolved quality is `Eco`, and the
  multisampled targets are freed when a frame goes back to 1x. Evidence:
  unit test
  `msaa_takes_the_largest_shared_count_up_to_four_with_a_depth_resolve`,
  and GPU test `msaa_smooths_edges_except_on_eco` (a slanted edge has only
  two shades on `Eco` and blended shades on `High`), plus the full GPU
  suite, all passing on an RTX 3060 and on llvmpipe. The NVIDIA driver lost
  the device (Xid 69) when the occlusion early pass ended its empty tone
  mapping subpass with the multisampled pipeline still bound; binding the
  tone mapping pipeline there fixes it.
  Known limits: depth resolves from sample 0, so the occlusion pyramid can
  be slightly non-conservative at silhouettes; 8x is not offered; no
  per-frame toggle other than the quality profile.

### Profiling

- [x] Vulkan timestamp queries per pass.
  Every `FramePass` writes a start and end timestamp (queries `2p` and
  `2p + 1` of a per-frame-context pool, reset at the start of the command
  buffer) through the new `PassRecorder`, which also does the debug labels.
  Once the frame's fence signals, `SceneRenderer::gpu_pass_times` returns
  `GpuPassTimes`, the time of each pass in recording order. The editor
  stats line shows the total and each pass. `CullingStats::time` now comes
  from the Culling, DepthPyramid, and OcclusionCulling pass times instead of
  its own timestamp pairs. Evidence: GPU test
  `frames_record_the_declared_passes_with_their_layouts` checks that every
  recorded pass, including those inside the render pass, has a time and that
  the total is above zero; `gpu_culling_counts_visible_instances_...` still
  checks the cull time. Both pass on an RTX 3060 and llvmpipe, and with the
  Khronos validation layer. Unit test
  `gpu_pass_times_label_shows_total_then_each_pass`.
  Known limits: times arrive one or two frames late; pass timestamps use
  `AllCommands`, so overlap between passes counts toward the earlier one;
  timestamp wrap-around reads as zero for one frame.
- [x] CPU timings for extraction, preparation, command recording, physics, and editor.
  `CpuFrameTimings` is a resource: `App::update` fills `physics` (all
  FixedUpdate steps) and `extraction` (RenderExtract);
  `SceneRenderer::write_cpu_timings` fills `preparation` (fence wait to
  command buffer start) and `recording` (builder creation to build); the
  editor fills `editor` (egui run and tessellation) and adds its second
  `extract_render_world` to `extraction`. The editor stats line shows all
  five. Evidence: unit tests `update_times_physics_and_extraction` and
  `cpu_timings_label_shows_each_part`; GPU test
  `frames_record_the_declared_passes_with_their_layouts` checks that
  preparation and recording are above zero on an RTX 3060 and llvmpipe.
  Known limits: `physics` covers every FixedUpdate system, not only physics;
  the fence wait and the egui paint are not counted.
- [x] Counters for draws, dispatches, triangles, visible instances, upload bytes, and GPU memory.
  `SceneRenderer::render_counters` returns the `RenderCounters` resource.
  Draws, dispatches, and direct-draw triangles are counted as commands are
  recorded; indirect draws add the triangles read back from their GPU-culled
  commands. Visible instances come from `CullingStats`. Upload bytes add up
  every host-written buffer of the frame: meshes, texture staging, instance
  and visibility lists, lights, cull commands, physics state, and debug
  lines. GPU memory sums the device memory blocks in the renderer's
  allocator pools. The editor stats line shows all six. Evidence: GPU test
  `frames_record_the_declared_passes_with_their_layouts` checks exact draw,
  dispatch, and triangle counts plus nonzero upload and memory;
  `gpu_culling_counts_visible_instances_...` checks that the triangle count
  includes the read-back indirect triangles and that 3 instances are
  visible. Both pass on an RTX 3060 and llvmpipe, and with the Khronos
  validation layer. Unit test `render_counters_label_shows_each_counter`.
  Known limits: indirect triangles arrive a frame or two late; GPU memory
  leaves out vulkano's dedicated allocations (large images) and other
  allocators such as the egui painter's.
- [x] Named GPU allocations/resources where practical.
  `name_object` sets a `VK_EXT_debug_utils` object name whenever the
  instance enables the extension. Named: the scene HDR and depth targets
  (with "(MSAA)" on the multisampled ones), the shadow map, the depth
  pyramid, every texture image and mesh vertex/index buffer (by asset path,
  or `#<handle key>` for assets built in code), the lights and GPU physics
  buffers, every render pass and pipeline, and the timestamp query pools.
  Evidence: headless GPU tests enable `ext_debug_utils` whenever the driver
  has it, and `name_object` has a `debug_assert` on the driver's result, so
  all 249 tests (RTX 3060 and llvmpipe) name every object without a
  rejection. The Khronos validation layer reports nothing on the
  `scene_renderer` tests. Unit test
  `asset_names_use_the_path_or_the_handle_key`.
  Known limits: suballocator arenas (instance and visibility lists, cull
  commands, per-frame uploads) stay unnamed. The windowed editor enables
  `ext_debug_utils` only with validation (debug builds with the layer
  installed, or `--features validation`). Names in RenderDoc or Nsight are
  not checked here.

### Exit gate

- Golden images cover primitive normals, PBR reference spheres, texture channels, alpha modes, shadow depth, and tone mapping.
- The renderer remains validation-clean through resize and asset reload.
- GPU timings and memory usage are visible to runtime code and the editor.

## Milestone 5: Hybrid physics

Goal: let each body use the processing unit and solver that fits its job while preserving a practical two-way connection between GPU simulation and normal Rust gameplay code.

### Determinism constraints, required from the start

Milestone 8 defines the full determinism requirements and owns their acceptance gate. Three of those rules cannot be retrofitted without rewriting the solver, so they apply while this milestone is implemented:

- [x] Never accumulate simulation state through order-dependent atomic operations. Use per-body accumulation slots or fixed-order reductions. Atomics remain acceptable for counters that do not feed simulation results. Audit: the ECS physics shader integrates each body alone and uses atomics only for the event counter and overflow count; `cull.comp` uses one for the visible list. `basic.comp` solved grid contacts in atomic cell-slot order, and each contact changes `vel`/`pos` for the next, so results changed with the grid build's thread order. It now visits each cell in ascending body index. Verified: `compute_registry::tests::grid_contacts_do_not_depend_on_cell_insertion_order` runs one step with the cell listed forward and reversed and requires bit-identical output; it failed before the fix and passes on llvmpipe and the RTX 3060. Known limit: a cell over 128 bodies still drops an arbitrary set; overflow handling is a later item. Rules recorded in `AGENTS.md` "Physics determinism".
- [x] Give body iteration and constraint ordering a stable sort key that survives buffer compaction, re-sorting, and insertion order. `basic.comp` orders contacts by body index (see above); `space.comp` already loops bodies in index order. GPU events reach Rust in atomic append order, so `route_gpu_physics_events` now sorts them by tick, `PhysicsId` slot and generation, event ID and flags; the slot is the stable key that survives buffer rebuilds. Verified: `runtime::tests::gpu_events_are_delivered_in_tick_and_body_order`.
- [x] Source any randomness inside a solver from a seeded, tick-indexed stream. Never read a global or thread-local RNG from simulation code. Audit: no solver, physics shader or runtime physics code uses randomness today (`grep` for `rand`, `random`, `thread_rng` finds no use; the `rand` dependency in `Cargo.toml` is declared but unused). The rule is recorded in `AGENTS.md` so the first solver that needs randomness follows it.

### Ownership and public model

- [x] Replace the old effect-only distinction with `SimulationClass::{Static, Cpu, Gpu}`. The selected class says where the newest runtime physics state lives. `Gameplay` is now `Cpu` and `GpuDynamic` is now `Gpu`; `None` stays for bodies no solver touches. Old scene files load through `#[serde(alias)]`, and old Rust code compiles through deprecated associated constants. The `GpuEffectBody` marker is removed: the editor and the game runner inserted it exactly when `uses_gpu()` was true, and nothing read it. The Inspector names the classes "CPU" and "GPU". Verified: `scene_file::tests::scenes_saved_with_old_simulation_names_still_load` and the existing physics routing, extraction and scene round-trip tests.
- [x] Add `PhysicsSyncMode::{None, Events, SelectedState, FullState}`. Synchronization is an explicit cost chosen per body or group, not a hidden full-scene copy. Evidence: per-body `PhysicsSyncMode` component (default `Events`, saved as `rusting.physics_sync`); `None` drops watch rules at extraction, only `SelectedState`/`FullState` bodies get a readback copy. `runtime::tests::sync_mode_controls_rules_and_state_mirror` and GPU test `scene_renderer::tests::only_state_synchronized_bodies_read_back_their_state` (llvmpipe and RTX 3060). Group-level selection is left to classes/prefabs setting the component.
- [x] Give GPU-simulated bodies a stable, generation-checked `PhysicsId` that is valid in ECS, GPU buffers, and events even after bodies are removed or buffers are sorted. Extend the same ID to the command bridge when commands are added.
- [x] Keep authored settings and identity in ECS while recording when mirrored transform/velocity data was produced and how many frames old it is. Evidence: `apply_gpu_state_samples` writes a `GpuStateMirror { tick, transform, velocities, custom_values }` and never touches the authored `Transform`; `GpuStateMirror::age_ticks` gives the age; stale generations are counted and older samples never rewind the mirror (`sync_mode_controls_rules_and_state_mirror`).
- [x] Allow CPU and GPU bodies in the same scene and allow static colliders to be consumed by both solvers. Evidence: the CPU solver skips GPU bodies (`runtime::tests::cpu_physics_is_deterministic_and_ignores_gpu_bodies`) and uses `Static` colliders. Extraction sends every non-sensor CPU/static collider (`PhysicsWorld::gpu_colliders`) and each GPU body's own shape to the native shader, which pushes GPU bodies out of them with friction, restitution and collision layers (`runtime::tests::gpu_bodies_extract_their_collider_and_see_cpu_colliders`, GPU test `scene_renderer::tests::gpu_bodies_rest_on_cpu_colliders_unless_filtered_or_no_collision`, llvmpipe and RTX 3060). Limit: GPU bodies do not collide with each other yet.
- [x] Show simulation owner, synchronization mode, readback age, and approximate synchronization cost in the editor. Evidence: the Inspector Physics section shows the owner (`Simulation`: CPU/GPU/Static), and for GPU bodies a `Sync` choice (undoable, stored as `rusting.physics_sync`), `Sync Cost` (bytes per tick from `PhysicsSyncMode::STATE_READBACK_BYTES`, compile-time checked against the GPU body layout), and `Last Readback` tick and age from `GpuStateMirror`. `editor::tests::inspector_shows_gpu_sync_mode_and_readback_age`. The editor preview does not simulate yet, so the age updates only where the mirror is applied (native Play).

### Programmable GPU condition and event bridge

- [x] Define a compact `GpuPhysicsEvent` layout shared by Rust and every physics shader: body ID, registered event ID, tick, flags, and a configurable small payload.
- [x] Provide a typed Rust condition builder for common GPU state fields, comparisons, boolean combinations, ranges, collision state, sleeping state, timers, and per-body custom values. This is a Rust API, not a separate scripting language.
- [x] Define event modes such as `OnEnter`, `OnExit`, `WhileTrue`, `Once`, and cooldown/rate-limited emission so conditions do not accidentally flood the readback buffer.
- [x] Upload built-in per-body condition instructions and rule parameters as compact GPU buffers. CPU-only custom-value uploads remain part of the command bridge.
- [x] Allow shared rules to target explicit multi-class object groups while keeping separate edge and cooldown state per body. Unrelated GPU bodies are not affected, and only matching body IDs and selected payloads return to Rust.
- [x] Add a custom condition-compute hook for arbitrary GLSL logic over GPU-accessible state. Custom physics and condition shaders use the same `emit_event(...)` ABI as built-in rules. Evidence: `GpuConditionShaders` holds `GpuConditionShader { events, glsl }`; the GLSL defines `condition(inout PhysicsState body)` and runs over every GPU body after each fixed step. It includes the same `src/shaders/physics_abi.glsl` as the built-in shader (body layout, push constants, `emit_event(body, event_id, payload_kind, payload)`), and `EVENTS[i]` holds the registered IDs, so its events arrive as ordinary `GpuPhysicsEvent`s. The renderer compiles sources at runtime with the system `glslc` (no new crate; `RUSTING_GLSLC` overrides the path), caches them by source, and skips a shader that fails to compile, keeping the message in `SceneRenderer::condition_shader_errors`. GPU test `scene_renderer::tests::custom_condition_shaders_emit_events_and_skip_broken_sources` (llvmpipe and RTX 3060); `runtime::tests::condition_shaders_reach_the_render_world_with_registered_events`. Needs `glslc` installed at runtime; precompiled SPIR-V input is not supported yet.
- [x] Allow events to return selected position, velocity, angular velocity, or custom-value payloads so a second state readback is often unnecessary. Add collision/contact payloads with the spatial solver.
- [x] Treat `Y < -100` only as the first end-to-end acceptance example. The implementation must not hard-code an axis, threshold, or event meaning. Evidence: `-100` appears only in tests and examples; the shader reads any `GpuStateField` by code and compares with the uploaded threshold. GPU test `scene_renderer::tests::conditions_use_any_field_and_see_custom_value_commands` fires an `OnEnter` rule on `PositionX > 10 || Custom(2) >= 3`, once from the start pose and once after a `SetCustomValues` command (llvmpipe and RTX 3060).
- [x] Compare previous and current condition results so edge-triggered events are emitted exactly when a condition changes state.
- [x] Let the native physics compute shader append events with an atomic counter into a per-frame event buffer.
- [x] Keep event output in per-frame mapped readback allocations so only emitted records cross into Rust; move the write target fully device-local if profiling shows mapped writes are costly on discrete GPUs.
- [x] Consume completed readback buffers asynchronously after their frame fence signals. Normal frames never wait for unfinished physics work.
- [x] Convert `PhysicsId` values back into live ECS entities and expose events to Rust systems through the engine event API.
- [x] Define event latency clearly: GPU events normally reach CPU gameplay one to three frames later. Logic requiring same-tick answers must use CPU simulation or an explicit blocking query. Evidence: `runtime::hybrid_physics` module docs, section "Latency" (events and `GpuStateMirror` carry the tick they describe; same-tick logic uses `SimulationClass::Cpu`). The blocking query is the open "explicit blocking readback API" item below.
- [x] Track event-buffer overflow, resize it within the configured memory budget, and provide an overflow fallback. Events must never disappear silently. Evidence: the per-frame event buffer is sized rules × fixed steps in the frame (each rule fires at most once per tick), so it cannot overflow below the `MAX_PHYSICS_EVENTS` budget (12 MiB). Past the budget, lost events are counted in `RenderCapacityDiagnostics::physics_events_dropped`, logged, and sent to gameplay as `GpuPhysicsEventsLost` so it can resynchronize from body state. GPU test `scene_renderer::tests::event_buffer_fits_every_step_and_reports_losses_past_budget` (three ticks in one frame deliver all 300 events; with a 60-event budget, 40 are reported lost). The budget is a constant, not derived from device memory yet.

### CPU to GPU command bridge

- [x] Add a compact command stream for spawn, despawn, teleport, velocity change, force, impulse, wake, solver change, and watch-condition updates. Evidence: `GpuPhysicsCommands::push(PhysicsId, GpuBodyCommand)` with `Teleport`, `SetVelocity`, `Impulse`, `Force` (one tick), and `SetCustomValues`, 80 bytes each. Spawn, despawn, solver, and watch changes go through the ECS components: extraction rebuilds the tables and `carry_regions` keeps surviving bodies' simulated state, so they need no command. Wake is left out until bodies can sleep. GPU test `scene_renderer::tests::commands_apply_once_before_the_step_and_reject_stale_bodies` (llvmpipe and RTX 3060) and `runtime::tests::gpu_commands_reach_the_render_world_once_per_batch`.
- [x] Upload commands in batches through each frame context instead of mapping or rewriting the complete physics buffer. Evidence: one batch per frame in the frame context's transient allocator, bound as binding 5 of the physics shader; the state buffer is never rewritten for a command. GPU test `scene_renderer::tests::commands_apply_once_before_the_step_and_reject_stale_bodies` (llvmpipe and RTX 3060) and `runtime::tests::gpu_commands_reach_the_render_world_once_per_batch`.
- [x] Apply commands before the fixed GPU step and reject commands whose body generation is stale. Evidence: the shader applies each body's commands (binary search in the index-sorted batch, submission order kept) before integrating on the frame's first step only; stale or unknown bodies are rejected on the CPU and counted in `RenderCapacityDiagnostics::physics_commands_rejected`, and the shader rechecks the generation. Commands wait while no fixed step runs and a batch is applied once. GPU test `scene_renderer::tests::commands_apply_once_before_the_step_and_reject_stale_bodies` (llvmpipe and RTX 3060) and `runtime::tests::gpu_commands_reach_the_render_world_once_per_batch`.
- [x] Support CPU-controlled kinematic bodies that collide with GPU bodies without transferring every GPU body to the CPU. Evidence: kinematic CPU colliders are uploaded with their velocity each frame, and the GPU contact uses the relative velocity, so a moving wall carries GPU bodies without any GPU state coming back. GPU test `scene_renderer::tests::kinematic_cpu_colliders_push_gpu_bodies` (llvmpipe and RTX 3060). Limit: one-way; GPU bodies do not push CPU bodies back.
- [x] Record command count and uploaded bytes for profiling. Evidence: `RenderCounters::physics_commands` and `physics_command_bytes`, asserted in GPU test `scene_renderer::tests::commands_apply_once_before_the_step_and_reject_stale_bodies` (llvmpipe.

### Selective state synchronization

- [x] Support asynchronous requests for selected transforms, velocities, sleeping state, or contact data by stable body ID. Evidence: `GpuPhysicsCommands::push(id, GpuBodyCommand::ReadState)` reads that body's full state back once after the next step, arriving as a `GpuStateMirror` like per-tick sync; stale IDs are rejected. Sleeping and contact data do not exist on the GPU path yet. GPU test `scene_renderer::tests::one_shot_state_reads_return_requested_bodies_once` (llvmpipe and RTX 3060).
- [x] Provide batched region/group snapshots for gameplay systems that need more than events. Evidence: `request_gpu_class_snapshot(world, class)` queues one `ReadState` per GPU body in an object class; the whole group reads back in one frame and arrives as `GpuStateMirror`s with the same tick. `runtime::tests::class_snapshots_request_one_read_per_member`; delivery is covered by GPU test `scene_renderer::tests::one_shot_state_reads_return_requested_bodies_once`. Regions are selected on the GPU instead: a `WhileTrue` rule with `inside` position conditions and a position payload, or a `GpuConditionShader`, returns only the bodies inside.
- [x] Keep full-state readback available for debugging, save-state capture, editor inspection, and tests, but keep it off the normal gameplay path. Evidence: `GpuPhysicsCommands::read_all_states` is a one-shot flag (default off) that copies every body once; the per-tick path copies only `SelectedState`/`FullState` bodies. GPU test `scene_renderer::tests::one_shot_state_reads_return_requested_bodies_once` (llvmpipe and RTX 3060).
- [x] Triple-buffer readback storage with frame contexts so GPU writes, transfer copies, and CPU reads never race. Evidence: event and state readback slices come from the per-frame-context transient allocator and are read only after that context's fence signals (`collect_physics_readbacks`); `only_state_synchronized_bodies_read_back_their_state` runs `2 * FRAMES_IN_FLIGHT` ticks and checks every sample is from its own tick.
- [x] Add an explicit blocking readback API only for tooling and exceptional cases, with a name and warning that make its performance cost obvious. Evidence: `SceneRenderer::block_until_physics_readbacks_complete`, documented as slow and not for gameplay; GPU test `scene_renderer::tests::one_shot_state_reads_return_requested_bodies_once` (llvmpipe and RTX 3060) uses it instead of waiting on the frame.
- [x] Support play/stop snapshots without requiring continuous full-state synchronization. Evidence: the authored ECS scene is the play snapshot, since GPU state only reaches `GpuStateMirror` and never the authored `Transform`. Stop sets `GpuPhysicsCommands::reset_to_authored`, which restarts every GPU body from its authored components with rule state cleared, and reads nothing back. A mid-play state captured once with `read_all_states` resumes through `GpuPhysicsCommands::restore` in the same batch as the reset. GPU test `scene_renderer::tests::reset_restarts_from_authored_state_and_restore_resumes_a_snapshot` (llvmpipe and RTX 3060); `runtime::tests::gpu_commands_reach_the_render_world_once_per_batch`.

### Self-written CPU physics and queries

- [x] Add an engine-owned CPU `PhysicsWorld` for bodies that require immediate gameplay answers. Evidence: `runtime::cpu_physics` runs in every `App`'s `FixedUpdate` for `SimulationClass::Cpu` bodies, with `Static` bodies as colliders. It integrates gravity and velocity, resolves contacts with 8 sequential-impulse iterations (restitution, friction, position correction), writes `Transform` and `RigidBody::linear_velocity` back on the same tick, and keeps a `PhysicsWorld` resource with `contacts()`, `raycast`, and `overlap_sphere`. `CollisionEvent` now comes from real contacts instead of bounding spheres. GPU bodies are ignored. Tests: `runtime::tests::cpu_boxes_fall_and_rest_in_a_stack_on_static_ground`, `cpu_restitution_bounces_and_sensors_only_report`, `cpu_physics_is_deterministic_and_ignores_gpu_bodies`, and the pair and ray tests in `runtime::cpu_physics::tests`. Limits: contacts apply no torque yet and child bodies are treated as fixed colliders. The pair search is now a sort-and-sweep broad phase (see the GPU solvers section).
- [x] Support fixed, dynamic, and kinematic rigid bodies plus boxes, spheres, capsules, convex meshes, and static triangle meshes. Evidence: fixed, dynamic, and kinematic bodies with box (oriented, SAT), sphere, and capsule colliders work (see `PhysicsWorld` above). `ColliderShape::ConvexMesh` uses the convex hull of the entity's `MeshRenderer` mesh and `ColliderShape::TriangleMesh` its triangles (static only; a body with it never moves), both scaled by the transform and cached per mesh revision. Pairs with a mesh collider use GJK on the shape cores plus EPA for overlaps; rays hit hull faces from outside and triangle meshes from either side; resting mesh contacts get up to four points. The Inspector offers both shapes. Tests: `runtime::cpu_physics::tests::mesh_colliders_match_primitives_and_report_normals_from_a_to_b`, `rays_hit_hulls_from_outside_and_triangle_meshes_from_either_side`, and `runtime::tests::cpu_convex_mesh_lands_flat_on_a_triangle_mesh_floor`. Limits: each triangle of a mesh is tested (no BVH yet), GPU bodies see a mesh collider as its bounding box, and `shape_cast`/`move_character` take primitive shapes only.
- [x] Add triggers, collision layers, raycasts, shape casts, overlap queries, character movement, and the joints required by the vertical slice. Progress: triggers (`Collider::sensor` reports without a response), `PhysicsWorld::raycast`, and `PhysicsWorld::overlap_sphere` are done. `CollisionLayers` filter contacts and query masks (`runtime::tests::cpu_collision_layers_filter_pairs_and_queries`). `PhysicsWorld::shape_cast` sweeps an unrotated box, sphere or capsule and stops just before the first solid collider, and `PhysicsWorld::move_character` moves a character shape with collide-and-slide and reports `grounded` (`runtime::tests::cpu_shape_casts_stop_before_colliders_and_characters_slide`). Joints: the Milestone 7 vertical slice lists none, so none are required here; the full joint set is its own later item ("Joints: fixed, hinge, slider, ...").
- [x] Add sleeping, continuous collision detection, stable contact generation, and iterative solving. Progress: iterative solving is done (sequential impulses with accumulated clamping; a three-box stack rests within 3 cm). Sleeping is done: still bodies get `Sleeping` after `SLEEP_STEPS` and wake on touch or edit (`runtime::tests::cpu_bodies_sleep_when_still_and_wake_on_touch_or_edit`). CCD for fast bodies uses a center-ray sweep (`runtime::tests::fast_cpu_bodies_do_not_tunnel_through_thin_walls`, which fails without the sweep). Contact manifolds are done: two boxes touching face to face get up to four points (incident face clipped to the reference face), and the solver applies impulses with angular response, per-point Coulomb friction, and warm starting from the last step (`runtime::tests::cpu_tilted_box_tips_over_onto_a_face`, `runtime::tests::cpu_sphere_sliding_on_the_ground_starts_rolling`; the stack test now also checks tilt below 0.01 rad).
- [x] Allow selected GPU events to create, update, or remove CPU proxy bodies when gameplay needs an approximate local query representation. Evidence: `GpuQueryProxy` on a GPU body names a `place_on` event (with a `Position` payload) and an optional `remove_on` event; `sync_gpu_query_proxies` keeps one `Static` proxy entity marked `GpuProxyOf`, removes it with the body or the component, and GPU bodies skip proxies. Test `gpu_events_place_move_and_remove_a_cpu_query_proxy` routes raw events and raycasts the proxy after it is placed, moved and removed. Limit: the proxy pose is as old as the event.

### GPU solvers and custom allocation

- [x] Connect per-object `ComputeShaderType` selection to the ECS/editor game runner instead of only the compatibility `Engine` path. Evidence: the ECS path selects per body through `PhysicsBody::solver` (the Inspector's GPU Solver), which the shared native shader reads from `custom_values.x`: Full, Simplified, NoCollision (skips collider contacts) and Space (point attractor). `Custom` now works too: the body's hook file defines `void solve(inout PhysicsState body)`, extraction reads each distinct file once (`RenderWorld::gpu_solver_shaders`), and the renderer compiles it like a condition shader that runs only for bodies whose `properties.w` holds that file's `custom_solver_id`; the built-in step leaves their motion alone. The Inspector's Open in Code Editor starts a missing file from a template. GPU test `scene_renderer::tests::custom_solvers_move_only_their_own_bodies` (llvmpipe and RTX 3060) and `runtime::tests::custom_solver_files_reach_the_render_world_once_per_path`. Limits: Simplified behaves like Full, and solver files reload only when GPU bodies are re-extracted.
- [x] Preserve mixed `Static`, `NoCollision`, simplified, full, spatial-grid, and custom compute batches in one scene. A scene-wide override remains a debugging tool only. Evidence: in the ECS path Static colliders and NoCollision, Full, Simplified, Space and Custom GPU bodies mix in one scene (`gpu_bodies_rest_on_cpu_colliders_unless_filtered_or_no_collision`, `custom_solvers_move_only_their_own_bodies`), and Full, Simplified and Space bodies now also collide with each other through a spatial-hash grid (`src/shaders/compute/physics_contacts.comp`) while NoCollision and Custom bodies skip it. GPU test `scene_renderer::tests::gpu_bodies_collide_with_each_other_through_grid_and_fallback` (llvmpipe).
- [x] Rename the stable form of `ComputeShaderType::Test` to describe its actual solver while keeping a temporary compatibility alias. Evidence: the variant is now `ComputeShaderType::GridCollision` (the spatial-hash collision solver in `basic.comp`); `ComputeShaderType::Test` remains as a deprecated associated constant that still works in expressions and patterns (`compute_registry::tests::old_test_name_still_selects_the_grid_solver`). Examples use the new name.
- [x] Add a true spatial broad phase to every built-in collision solver. Broad phases prune separated bodies, but dense overlapping scenes can still require quadratic contact work.
  - [x] Make the CPU `PhysicsWorld` sweep the axis with the greatest center spread and reject pairs separated on either other axis before the exact shape test. Pairs return in stable body order. Evidence: `broad_phase_finds_the_same_pairs_as_testing_every_pair` matches brute force on 200 scattered bodies; `broad_phase_prunes_a_tall_column_without_changing_pair_order` reduces a 200-body X-overlapping column to its one actual candidate pair.
  - [x] GPU body-to-body contacts use a spatial hash with a cell size derived from body sizes (`contact_grid_cell_size`), checking 27 neighbouring cells plus the fallback list. The legacy shader path is no longer compiled.
  - [x] Give the native GPU shader a spatial broad phase for CPU/static colliders. The renderer builds a stackless bounding-volume tree from each frame's collider snapshot, in stable collider order, and uploads it beside the colliders. The shader traverses from each GPU body's current pose, skipping separated subtrees; after a contact changes the pose it restarts at the root after the last processed collider, preserving the old list-order semantics. A CPU test checks 128 separated colliders yield one candidate and fewer than 20 visited nodes; the headless GPU tests `gpu_bodies_rest_on_cpu_colliders_unless_filtered_or_no_collision` and `gpu_collider_tree_rechecks_later_branches_after_a_push` check late collider discovery, layers/solver filtering, and correction into a previously separated branch. The `ColliderNode` upload has a shader-reflected layout assertion. The tree is rebuilt each physics frame so kinematic colliders move with their CPU poses.
- [x] Replace fixed hash capacities with device-budgeted growable buffers. The active `PhysicsContactGrid` is device-local, sized from the body count (two hash cells per body), and grows by doubling. Its hash is capped at a sixteenth of the largest device-local heap (`contact_grid_hash_stays_inside_its_memory_budget`); bodies that do not fit spill into a fallback list that preserves all bodies. GPU test `gpu_bodies_collide_with_each_other_through_grid_and_fallback` reruns with a zero budget (one hash cell) and gets bit-identical results, with overflow reported. The fixed-capacity legacy `basic.comp` path was removed from compilation in Milestone 1. Verified with default and serial `gpu-tests` workspace suites.
- [x] Track grid-cell overflow, hash collisions, oversized-body count, and total fallback work. Evidence: the contact passes add them to `contacts` in the event header, and `RenderCapacityDiagnostics` reports `physics_grid_overflow`, `physics_oversized_bodies`, `physics_grid_hash_collisions` and `physics_fallback_tests` for the latest completed physics frame. `gpu_bodies_collide_with_each_other_through_grid_and_fallback` asserts overflow, oversized and fallback counts; `hybrid_physics_gpu_layouts_match_shader_structs` checks the header layout. Limit: the legacy `GridCollision` path is not tracked.
- [x] Implement a tested overflow fallback that preserves every body. Evidence: in the ECS contact grid, a body whose cell is full or which is bigger than one cell goes to a fallback list that every body tests, and oversized bodies test every body, so no contact is skipped. `gpu_bodies_collide_with_each_other_through_grid_and_fallback` crowds twelve overlapping pebbles into one eight-slot cell and puts a small sphere on an oversized one: every pebble separates, the small sphere rests on top, and a second run matches bit for bit. Limit: the legacy `GridCollision` shader still drops bodies past `MAX_PER_CELL`.
- [x] Define a versioned custom-compute ABI for instance state, commands, condition inputs, custom values, and event output. Evidence: `src/shaders/physics_abi.glsl` is included by the built-in shader and every custom condition shader. It holds the `PhysicsState` layout (with `custom_values`), the `BodyCommand` layout and `COMMAND_*` kinds, the `PhysicsEvent` layout, bindings 0/3/4, the push constants, and `emit_event`, and it defines `RUSTING_PHYSICS_ABI_VERSION` so a shader can `#error` on a mismatch. Rust exposes the same number as `runtime::GPU_PHYSICS_ABI_VERSION`. GPU test `scene_renderer::tests::hybrid_physics_gpu_layouts_match_shader_structs` checks every Rust mirror against the reflected shader structs and the version against the GLSL define. Built-in condition instructions and rule state stay private to the built-in shader; custom shaders receive their inputs as `custom_values` and push constants.
- [x] Let custom shaders emit the same typed events as built-in solvers so Rust gameplay can react without downloading complete buffers. Evidence: custom condition shaders call the shared `emit_event` with IDs from `EVENTS[i]`, registered by name in `GpuEventRegistry`, and the events arrive as ordinary `GpuPhysicsEvent`s. GPU test `scene_renderer::tests::custom_condition_shaders_emit_events_and_skip_broken_sources` (llvmpipe and RTX 3060).

### Profiling and automatic allocation

- [x] Measure CPU physics time, GPU physics time, dispatch count, command bytes, event bytes, selected-state bytes, synchronization latency, and overflow counts. Evidence: CPU physics time is `CpuFrameTimings::physics` and GPU physics time the `FramePass::Physics` timestamp pair; `RenderCounters` adds `physics_dispatches`, `physics_commands`/`physics_command_bytes`, `physics_event_bytes`, `physics_state_bytes` and `physics_readback_latency_frames`, and `RenderCapacityDiagnostics` holds event loss and contact-grid overflow, oversized, hash-collision and fallback counts. The editor's Stats panel shows them on a "GPU Physics" line (`editor::view::gizmo_tests::physics_counters_label_shows_traffic_and_fallbacks`); `gpu_bodies_collide_with_each_other_through_grid_and_fallback` checks the dispatch, event-byte and latency counters on a real frame. Limit: latency is counted in rendered frames, not milliseconds.
- [x] Add repeatable 1K, 10K, and 100K body benchmark scenes covering falling, stacking, debris, and mixed solvers. Evidence: `runtime::PhysicsBenchmark` (`Falling`, `Stacking`, `Debris`, `Mixed`) builds any body count as a pure function with no RNG; `runtime::physics_benchmark::tests` checks 1K/10K/100K counts, identical output across calls, no starting overlaps, and the Mixed solver interleave. `cargo run --release --example physics_bench -- <scene> <count>` runs one in the game window and prints ms/frame. GPU test `scene_renderer::tests::benchmark_scenes_run_on_the_gpu_and_repeat_exactly` (1K bodies, 90 ticks, headless Vulkan) requires colliding bodies to stay on the ground, ten-cube towers to stand, and Debris and Mixed to repeat bit for bit. It found two bugs, both fixed: towers sank through each other (the contact pass now uses shock propagation: a supported body below counts as immovable), and crowded cells broke determinism (contact sums are now fixed-point integers, so atomic list order cannot change them). Limit: 10K/100K frame times are real-hardware numbers and are not recorded here.
- [x] Add an optional `Auto` allocation policy that uses body requirements, query needs, hardware capabilities, transfer cost, and measured timings. Evidence: the `runtime::AutoSimulation` component (scene key `rusting.auto_simulation`) opts a body in; `allocate_auto_simulation` runs in PostUpdate before GPU IDs are assigned. Requirements come first: non-dynamic bodies, no GPU backend, custom solvers (GPU), kinematic bodies, sensor and mesh colliders, and sync modes that read state back every tick (query needs and transfer cost) all force a class. Flexible bodies go to the GPU when there are at least `AutoAllocationPolicy::gpu_min_bodies` of them or when the last `CpuFrameTimings::physics` exceeded `cpu_physics_budget`, else to the CPU. Test `runtime::tests::auto_simulation_picks_cpu_or_gpu_and_stays_overridable` covers every reason. Limit: decisions are sticky; a decided body never migrates, and transfer cost is judged by sync mode, not by measured bytes.
- [x] Keep manual CPU/GPU and solver selection available; automatic allocation must be observable and overridable. Evidence: bodies without `AutoSimulation` keep their manual `PhysicsBody::simulation` and solver; the decision and its `AllocationReason` are public on the component, and clearing it makes the body decide again (runtime test above). The Inspector's Physics section has an "Auto CPU/GPU" checkbox (undoable component add/remove) that shows the decision in place of the manual choice (`editor::tests::inspector_shows_the_auto_simulation_decision`).

### Exit gate

- A scene with at least 10,000 GPU-simulated cubes can evaluate a user-configured condition and emit a Rust event when any cube crosses `Y = -100` without copying all cube transforms or blocking the frame loop. Replacing that rule with another supported or custom condition does not require engine changes.
- Tests cover triggers, raycasts, stacking, tunneling, collision layers, fixed-step independence, stale IDs, and CPU/GPU event delivery.
- GPU event and grid overflow are observable and their fallbacks never silently omit bodies or events.
- CPU/GPU commands and events remain correct with multiple frames in flight.
- Immediate gameplay queries never pretend that delayed GPU mirrors are current; state age is available to callers.
- Per-body ownership, solver, synchronization mode, traffic, latency, and overflow are visible in runtime diagnostics and the editor.

## Milestone 6: Integrated egui editor

Goal: allow scenes to be built, inspected, saved, and played without leaving the engine.

The editor should use `egui` and `egui-winit`. Rendering should go through an engine-owned Vulkan painter/integration layer so renderer compatibility and resource lifetime remain under project control.

### Editor shell

- [x] Add a startup Project Manager with create, open, folder selection, validation, and recent projects.
- [x] Create complete standalone Cargo game templates without overwriting existing folders.
- [x] Store project format versions and reject projects made by a newer editor.
- [x] Add a feature-gated editor plugin that can be excluded from runtime builds.
- [x] Build the initial egui shell with toolbar, hierarchy, transform inspector, viewport placeholder, and structured console.
- [x] Connect play, pause, single-step, and stop controls to runtime time control.
- [x] Show typed asset inventory, mesh/material handles, and live render-extraction dirty ranges in editor panels.
- [x] Add a runnable Vulkan editor window with winit input, resize handling, DPI-aware egui rendering, and presentation.
- [x] Render extracted ECS meshes as a depth-tested Vulkan scene beneath the editor UI.
- [x] Add revision-aware GPU mesh preparation so asset mutation invalidates cached buffers without changing handles.
- [x] Drive the scene viewport and camera aspect ratio from the DPI-aware central-panel pixel rectangle.
- [x] Expose camera FOV, clipping planes, priority, and active state in the inspector.
- [x] Add Scene, Game, and Code workspaces with editor/game camera selection.
- [x] Add a project-local Rust/GLSL editor with open, validation, and save actions.
- [x] Run Cargo checks and Debug/Release builds in a worker and show compiler output inside Code Editor.
- [x] Export a native release folder with the executable, cooked scene, project assets, license, and run instructions.
- [x] Store portable scene-relative asset paths and resolve packaged scene data beside the executable.
- [x] Add a separate native Rust Cargo game project, project-local source editing, and Debug/Release build/run from the editor.
- [x] Add a concise native Rust scene API for common transform operations without hiding the ECS from advanced games.
- [x] Add a dockable area-tree layout with selectable editor types and project-local persistence.
- [x] Replace the bootstrap Vulkan egui integration with an engine-owned texture/mesh upload path and render pass. Evidence: see Milestone 4 "Engine-owned egui compositing pass" (`EguiPainter` GPU tests, editor smoke run).
- [x] Add a central editor-shortcut action map; `Numpad 0` toggles Scene View fly-camera pointer capture while Escape remains a normal UI key.
- [x] Add a Settings panel for rebinding and persisting shortcuts, then route every editor command through the same action map. Evidence: the "Keyboard Shortcuts" area type lists every action by context (Editor, Scene View, Transform, Fly Camera); clicking a key waits for the next key press, Escape cancels, and a key taken from another action in the same context leaves that action unbound. The map is saved to `editor_shortcuts.json` in the user config folder and loaded over the defaults at startup. Undo, Redo, Save scene, Delete selection and Rename are now `EditorAction`s (Ctrl+Z, Ctrl+Shift+Z, Ctrl+S, Delete, F2) that the window-event handler queues and the view runs like the menu entries; the Hierarchy's hard-coded Delete/X/F2 keys are gone. Tests: `editor::shortcuts::tests::{editor_actions_need_their_exact_modifiers, captured_key_binds_the_waiting_action_and_escape_cancels, saved_shortcuts_load_over_the_defaults}`, `editor::tests::{queued_shortcuts_delete_and_undo_like_the_menu, shortcuts_area_lists_every_action_with_its_key}`; the editor starts cleanly. Each action can also take a second key (right-click removes it); Redo also answers Ctrl+Y. A file saved before second keys existed keeps the default ones (test `second_keys_trigger_actions_and_move_between_actions`). Limits: at most two keys per action, and X does not delete by default because it picks the X axis in the Scene View; widget-local keys (Enter/Escape in the rename field, Escape to cancel a gizmo drag) stay fixed. Follow-ups are in `docs/editor-overhaul.md`.
- [x] Route keyboard and mouse focus correctly between all viewport navigation modes and UI. Evidence: the Scene View takes pointer events only when egui's topmost layer under the cursor is the viewport (`EditorViewport::hovered`, test `editor::shortcuts::tests::covered_viewport_does_not_take_pointer_events`), and keys only when no text field wants them. Losing window focus ends fly and orbit navigation (`release_editor_navigation`). Pointer capture falls back from `Locked` to `Confined` on Windows and X11. While fly or orbit navigation is active, the editor now keeps presses, pointer motion, the wheel and text away from egui, so they cannot click, scroll or type into UI under the hidden cursor; releases still pass, so egui never keeps a button held (`ui_receives_during_navigation`, test `navigation_keeps_presses_and_motion_away_from_the_ui`). The view also drops text-field focus while navigating. Editor commands run only when no text field has focus and no navigation or transform is active. Limit: the grab fallback and focus loss need a real window to verify by hand.
- [x] Add DPI scaling, font configuration, and theme persistence.
  Evidence: OS display scale reaches egui through egui-winit (initial
  `scale_factor` plus `ScaleFactorChanged`); `EditorPreferences` gained
  `font_scale` beside `ui_scale`, View menu has TEXT SIZE choices, and both
  persist in `editor_preferences.json` without dropping each other
  (`editor_preferences_round_trip_and_fall_back_to_defaults`,
  `font_scale_multiplies_every_default_text_size`). Limits: one built-in
  palette, so there is no theme choice to persist yet; widgets that build
  their own `FontId` keep a fixed size.

### Core panels

- [x] Add project asset file import, filtering, typed texture loading, glTF primitive import, and selected-object assignment.
- [x] Persist imported glTF geometry as reloadable engine-native `.rmesh` assets.
- [x] Scene viewport rendered to an editor texture.
  Evidence: the editor renders the live 3D view into an offscreen image
  sized to the view (`scene_view_target` in `src/bin/editor.rs`) and egui
  draws it as `SCENE_VIEW_TEXTURE` through
  `EguiPainter::set_native_texture`
  (`native_textures_show_an_image_rendered_elsewhere`, gpu-tests;
  `scene_area_draws_the_offscreen_view_image_over_its_viewport`). Editor
  smoke run is clean. Limits: one live view as before; with no 3D area the
  scene still renders into the window under the panels, so GPU physics keeps
  running.
- [x] Entity hierarchy with filtering, selection, reparenting, and drag/drop.
  Evidence: search keeps matches and their ancestors
  (`search_keeps_matches_and_their_ancestors_only`), Ctrl/Shift
  multi-select with a primary object
  (`clicks_select_toggle_and_extend_with_a_primary_object`), drag/drop
  rejects cycles (`drag_parenting_rejects_self_and_descendant_cycles`), and
  dragging a selected row reparents the whole selection in one undoable
  scene edit that keeps world positions and nested children
  (`reparenting_a_selection_keeps_world_positions_and_nested_children`).
  Limits: no sibling reordering; after a reparent only the first moved
  object stays selected.
- [x] Component inspector driven by an allowlisted reflection/editor registry.
  Evidence: only components in the `SceneComponentRegistry` allowlist reach
  the Inspector; `InspectorRegistry::register::<T>(name, draw)` gives one a
  typed section, and the rest are edited as JSON fields (serde is the
  reflection). Typed edits go through the registry and the Inspector undo
  snapshot (`registered_inspectors_draw_and_edit_their_component`). Limits:
  built-in components (Transform, lights, physics) keep their hand-written
  sections instead of registry entries.
  Reflected fields show their `doc` hint under the label in the label's
  tooltip, as Godot shows property descriptions. Evidence:
  `editor::inspector::widgets::tests::a_described_row_shows_its_doc_in_the_label_tooltip`.
  A field drawn as a foldable group (a nested struct, list or map) shows
  its doc in the group header's tooltip. Evidence:
  `editor::inspector::reflected::tests::a_described_section_shows_its_doc_on_the_header`.
  A string-keyed map field adds an entry from a key field and removes one
  with its button; an empty or existing key adds nothing. Evidence:
  `editor::inspector::reflected::tests::map_entries_can_be_added_by_key_and_removed`.
- [x] Asset browser with folders, thumbnails, filtering, and drag/drop assignment.
  Evidence: the Assets area is a foldable FileSystem tree with a filter
  (`rows_put_folders_first_hide_caches_and_fold`). Image rows show a decoded
  preview, two new decodes per frame, with a larger one on hover
  (`image_rows_decode_a_few_thumbnails_per_frame`). Models drag into the
  Scene View; images drag onto a Hierarchy object (base color) or an
  Inspector texture slot, as one Undo step each
  (`dropping_an_image_on_hierarchy_rows_and_texture_slots_assigns_it`).
  Limits: list view only (no grid), previews decode on the UI thread and do
  not refresh when the file changes, and dropped models land at the origin.
- [x] Console with structured logs, filtering, warnings, and asset/validation errors.
  Evidence: `ConsoleEntry` carries a level, a source ("Assets", "Build",
  "Scene", "Code") and a repeat count; the Console area has Info/Warnings/
  Errors toggles with counts, a text filter, and Clear. Asset request
  results, hot reload failures, and scene load/save/validation status lines
  are logged (`console_groups_repeats_filters_and_records_status_lines`).
  Limits: status lines get their level from keywords, the log keeps the
  last 2000 entries, and engine code outside the editor has no log sink yet.
- [x] Profiler with CPU spans, GPU pass timings, counters, and memory usage. The dockable Profiler area shows a 180-render-frame CPU/GPU history graph, five measured CPU spans, every available completed GPU pass timestamp, draw/dispatch/triangle and culling counts, upload bytes, GPU allocator-pool memory, and physics overflow/event loss. Pause and Clear operate on the bounded history. The editor records one sample after publishing renderer diagnostics. Evidence: `history_is_bounded_and_pause_freezes_the_visible_frame`, `profiler_area_shows_timing_and_memory_sections`, and the full workspace and serial GPU test suites. GPU times can lag CPU frames; the allocator-pool figure excludes dedicated allocations.
- [x] Add dedicated render settings and physics diagnostics panels. The area switcher now opens Render Settings (scene quality/culling with snapshot undo, display pacing, resolved device profile, culling and missing-asset counts) and Physics Diagnostics (backend status, body/tick counts, GPU work/readback, overflow and rejected-event counts). Evidence: `panels_expose_missing_assets_and_physics_overflow`, `scene_render_settings_edits_are_undoable_scene_changes`, and the full workspace checks.
- [x] Add a typed physics inspector for simulation ownership, GPU solver profile, rigid body, and collider settings.

### Scene editing

- [x] Add editor-only Scene View overlays: XZ grid, selected-object local axes, and real mesh-bounds box.
- [x] Select the nearest rendered object by clicking Scene View, using an editor ray against transformed mesh bounds instead of requiring a physics collider.
- [x] Add a selection outline that remains readable when the object is behind other geometry. Selected mesh bounds and camera/light wire shapes now use the depth-independent debug pass with thicker lines; grid and unselected helpers remain depth tested. Evidence: `selection_outline_ignores_depth_without_changing_other_helpers` and the full workspace checks, including serial GPU tests. This is a wire outline of the authored bounds, not a pixel silhouette.
- [x] Translate, rotate, and scale gizmos with local/global modes and snapping. The Scene View toolbar now toggles world or object axes and snaps move to 1 unit, rotation to 15°, and scale to 0.1 factor steps. Global scale projects the chosen world axes onto local scale axes because `Transform` has no shear representation. Each drag still takes one snapshot undo step. The menu beside Snap sets the move, rotation and scale increments. Evidence: `gizmo_snap_quantizes_move_scale_and_rotation`, `global_gizmo_axes_ignore_object_rotation`, `global_rotation_uses_parent_space_axis`, `global_scale_on_rotated_object_uses_matching_local_axis`, and the full workspace checks.
- [x] Add FPS-style Scene View fly camera: Numpad 0 captures/releases the pointer, mouse changes yaw/pitch, WASD moves, Space/Ctrl move vertically, and Shift boosts speed.
- [x] Add camera orbit, pan, focus-selection, and framing controls after fly camera input is stable. Middle drag orbits around the view pivot, Shift+middle drag pans, the wheel dollies, and F or the Scene View Frame button centers and fits the selected mesh or object. Evidence: `orbit_keeps_the_pivot_fixed_and_dolly_moves_toward_it`, `focus_places_selected_object_in_front_of_camera`, and the full workspace checks. Home frames every rendered object, and Numpad 1, 3 and 7 turn to the front, right and top views around the same pivot. Evidence: `numpad_views_keep_the_pivot_and_frame_all_sees_every_mesh`. Ctrl turns to the opposite views, Numpad 9 looks at the pivot from the other side of the current view (evidence: the same test, which checks right to left and a tilted view to its mirror), and Numpad 5 (or VIEW > Orthographic in the viewport menu) toggles an orthographic view sized to the orbit pivot. Evidence: `orthographic_view_matches_the_pivot_size_and_follows_the_dolly`.
- [x] Create empty, cube, and camera objects; duplicate, rename, delete, and reparent entities.
- [x] Add/remove/edit registered compiled components through the generic JSON inspector.
- [x] Assign meshes, materials, textures, and physics shapes by typed handle. Mesh Renderer now lists saveable `Handle<MeshAsset>` and loaded `Handle<MaterialAsset>` choices with snapshot undo; the Material section already picks `Handle<TextureAsset>` values, and the typed ColliderShape picker selects primitive or mesh collider shapes (mesh shapes use the renderer's typed mesh handle). Evidence: `mesh_picker_excludes_transient_unsaved_handles`, `typed_asset_assignment_snapshots_once_and_rejects_unsaved_meshes`, and the full workspace checks. Physics shapes are enum variants, not standalone shape assets.
- [x] Scene new/open/save/save-as operations with native file pickers.

### Play workflow and history

- [x] Make Play save and cook the scene, compile the real Rust project, and launch its native game window.
- [x] Use fast Debug builds by default and allow optimized Release play tests.
- [x] Add stop and restart controls for the native game process. Stop already cancels Cargo or the child game; Restart is now available in the toolbar and Code area, waits for worker exit, then saves, cooks, builds, and launches again. A later Stop cancels a queued restart. Evidence: `restart_waits_for_worker_exit_and_stop_cancels_pending_restart` and the full workspace checks.
- [x] Add an optional embedded preview for workflows that do not need compiled Rust systems. Preview resumes the editor's ECS runtime in the Game area without a build; Pause, Resume, and Step control fixed time. Stop restores the authored scene, removes preview-spawned entities, and restores Undo/Redo history and the dirty flag. Evidence: `embedded_preview_restores_authored_scene_after_simulation` and the full workspace checks. Project Rust binaries still require native Play.
- [x] Make runtime-spawned entities visually distinct where useful. Entities created after embedded preview starts carry a `[Runtime]` badge in Hierarchy and a preview-only warning in Inspector; they disappear when preview stops. Evidence: `only_entities_created_during_preview_get_runtime_badges`, `embedded_preview_restores_authored_scene_after_simulation`, and the full workspace checks.
- [x] Implement snapshot-based undo/redo for scene and Inspector edits.
- [x] Group continuous Inspector field edits into single undo transactions.
- [x] Mark scenes dirty and prompt before destructive new/load/project-switch actions.

### Exit gate

- A user can construct, save, reload, play, pause, step, and restore a scene entirely through the editor.
- Undo/redo covers transforms, hierarchy, entity lifecycle, and component edits.
- Editor compositing has a golden-image test.
- Runtime-only builds do not depend on editor code.

## Milestone 7: Vertical slice

Goal: prove that the engine architecture works as a usable game-development stack.

### Required content

- [x] One representative imported environment with PBR assets. `samples/vertical_slice/environment.gltf` is a reproducible courtyard fixture with 11 nodes, three PBR materials, and embedded base-color and metallic/roughness maps; the normal glTF importer creates typed cooked meshes and textures for it. Evidence: `pbr_environment_imports_geometry_materials_and_maps`. The sample README records the import path and asset provenance.
- [x] Player camera/controller. `PlayerController` (`src/runtime/player.rs`, built into every `App` and saved as `rusting.player_controller`) walks a body entity with `PhysicsWorld::move_character` each fixed step, as its `Collider` shape or a default 1.8 m capsule, with gravity, jump, and sprint from the `player.*` actions (default WASD/arrows, Space, Shift). Mouse look yaws the body and pitches its `Camera` children; left click captures the cursor and Escape releases it; pitch clamps to ±89°. Evidence: `player_controller_falls_walks_jumps_and_stops_at_walls`, `player_look_needs_captured_cursor_and_clamps_pitch`, `player_controller_settings_round_trip_through_scene_registry`, and the full workspace checks including serial GPU tests. Limits: no step climbing or slope limit beyond sliding, collider scale is ignored, and a ceiling does not cancel upward speed.
- [x] CPU collisions, triggers, and at least one immediate physics query. A player body with a kinematic CPU capsule collider is stopped by a fixed wall, reported by a sensor it walks through (`CollisionEvent` with `sensor`), and the wall is found by an immediate `PhysicsWorld::raycast` on the same tick. Evidence: `player_controller_falls_walks_jumps_and_stops_at_walls`, plus the existing `cpu_restitution_bounces_and_sensors_only_report`, `cpu_collision_layers_filter_pairs_and_queries`, and `cpu_shape_casts_stop_before_colliders_and_characters_slide`. Sensors report every touching step; there are no separate enter/exit events.
- [x] Directional shadows, point lights, sky/environment light, and transparent content. One headless frame combines a shadowed directional light, a point light, a `SkyLight`, and an alpha-blended pane; each light uses its own color channel, and turning each feature off removes only its own contribution while the floor stays visible through the pane. Evidence: GPU test `vertical_slice_lighting_combines_shadow_point_sky_and_blend` (llvmpipe), on top of the per-feature tests from Milestone 3. The scene is test slabs, not the imported courtyard; shadows remain one directional map without cascades.
- [x] At least 10,000 GPU bodies that evaluate configurable conditions and send typed events to Rust gameplay without full-state readback. Evidence: GPU test `ten_thousand_gpu_bodies_report_condition_events_without_state_readback` (llvmpipe) runs 10,000 falling `Events`-sync bodies for 30 ticks with an `OnEnter` `position_y < 0` rule: exactly the 5,000 bodies that cross report one event each with their crossing position, nothing is dropped, and `physics_state_bytes` stays 0 while `physics_event_bytes` counts the event traffic. Delivery to typed `GpuPhysicsEvent`s on live entities is covered by `raw_gpu_events_reach_the_live_ecs_entity` and `gpu_events_are_delivered_in_tick_and_body_order`; custom condition shaders by `custom_condition_shaders_emit_events_and_skip_broken_sources`. `cargo run --example hybrid_10k` shows the same rule in a window. Frame time at 10,000 bodies is a hardware number and is not recorded here.
- [x] CPU-to-GPU commands that alter selected GPU bodies while the simulation is running. Evidence: GPU test `commands_alter_only_selected_bodies_mid_simulation` (llvmpipe) lets four bodies fall for 10 ticks, then sends an `Impulse` to one and `SetVelocity` to another: only those two change on tick 11, the others keep plain gravity, and all four keep simulating from the new state. `commands_apply_once_before_the_step_and_reject_stale_bodies` covers teleport, force, custom values, once-only application, and stale `PhysicsId` rejection; `gpu_commands_reach_the_render_world_once_per_batch` covers the ECS `GpuPhysicsCommands` path.
- [x] Runtime UI and editor UI. Runtime UI: the new `ui` feature (egui and egui-winit, already dependencies; `editor` enables it) gives every `App` a `RuntimeUi` resource. `App::update` opens one egui pass around `Update` and `PostUpdate`, so gameplay systems draw HUDs and menus with `ui.context()` (or `GameScene::ui()`), and headless runs behave the same. The game window runner feeds window input to that pass, keeps presses the UI uses away from `RuntimeInput`, and paints the result over the scene with the shared `EguiPainter`. Editor UI is the Milestone 6 egui editor. Evidence: `runtime_ui_systems_draw_and_receive_clicks_each_update` (an ECS system draws a label and button, and one press and release reaches it as one click), GPU test `runtime_ui_drawn_by_an_ecs_system_paints_over_the_game_view` (llvmpipe), and a 10 s windowed run of `cargo run --example hybrid_10k`, whose HUD now shows FPS and fallen cubes, with no error output (RTX 3060, Hyprland). The HUD was not checked by eye. Embedded editor Preview does not show `RuntimeUi` yet; that is logged in `docs/editor-overhaul.md`. The Milestone 16 UI toolkit (layout containers, themes, localization) is still to come.
- [x] Scene persistence and live asset reload. Evidence: integration test `vertical_slice_scene_persists_and_reloads_assets_live` imports the courtyard glTF, adds a player body with `PlayerController` and a child camera plus a directional light, saves the scene, and loads it into a fresh `App` with `AssetPlugin`: every name, the controller settings, the camera parent link, the light, and the mesh handles come back. It then rewrites the floor's cooked `.rmesh` file, and the running app's hot-reload scan publishes the new mesh with a higher revision on a later `update`, with no restart. The Milestone 3 hot-reload items cover worker decoding, failed reloads, and GPU resource lifetime. Editing the source `.gltf` still does not re-import it.
- [x] Runtime editing through play/pause/stop. In the embedded preview, a dynamic CPU body falls while playing, holds still while paused, takes a transform edit made through the editor's snapshot-undo path, advances one fixed step from the edited pose on Step, keeps falling from there on Resume, and returns to its authored pose on Stop with the preview's undo entries dropped. Pause and Resume share `set_embedded_preview_paused`. The test found a bug: bevy 0.19 stores resources and observers as entities, so Stop despawned the ones that systems created during the preview and panicked on a stale command. Stop now skips `IsResource` and `Observer` entities. Evidence: `preview_edits_while_paused_drive_the_simulation_until_stop` (panicked before the fix), `embedded_preview_restores_authored_scene_after_simulation`, and `only_entities_created_during_preview_get_runtime_badges`. Native Play runs a separate process and has no live editing; that is the Milestone 20 remote inspector.
- [x] Eco, Balanced, and High quality comparisons. GPU test `quality_profiles_compare_on_one_scene` renders one 32x32 scene (a floor under a red shadowed sun and 40 green point lights) at each profile and prints the comparison. On llvmpipe:

  | Profile | Lights kept (dropped) | Shadow map | MSAA | Mean red | Mean green |
  |---|---|---|---|---|---|
  | Eco | 16 (25) | 1024 px | 1x | 130.0 | 27.9 |
  | Balanced | 32 (9) | 2048 px | 4x | 130.0 | 56.2 |
  | High | 41 (0) | 4096 px | 4x | 130.0 | 69.5 |

  The sun counts toward the light budget. The test checks that the sun looks the same on every profile, that each higher profile keeps more point lights and lights more of the floor, and the shadow map size and MSAA sample count for each profile (MSAA follows the device's sample count). Shadow distance follows `shadow_settings` (30, 50 and 80 m); it is not rendered at range in a test. Per-feature tests: `msaa_smooths_edges_except_on_eco`, `lights_over_capacity_render_first_max_lights_and_report_the_rest`, `culling_path_follows_mode_ownership_count_device_and_quality`, and `auto_quality_resolves_from_device_class_and_concrete_profiles_pass_through`. Frame-time cost per profile is a hardware number and is not recorded here.

### Performance target

Milestone 7 and Milestone 29 have deliberately different hardware targets. The engine vertical slice must run on low-end integrated graphics. Sundering must not: continuous destruction with thousands of debris bodies and GPU navigation has a hardware floor, and pretending otherwise would distort the engine's quality profiles. Neither target is allowed to weaken the other.

- [x] Define a fixed benchmark scene and camera path. Evidence: `spawn_render_benchmark` and `render_benchmark_camera` (`src/runtime/render_benchmark.rs`) build a fixed scene. It has a ground, 40 pillars with spheres in five PBR materials, 1,000 `PhysicsBenchmark::Mixed` GPU bodies with landing events, a shadowed sun, and 24 point lights. The camera path orbits once in 600 frames with two dives, from 40 m to 16 m. Unit tests `camera_path_is_closed_repeatable_and_faces_the_scene` and `scene_spawns_the_same_fixed_content_every_time` pass.
- [ ] Target 1920×1080 at 60 FPS on approximately Intel UHD 620-class hardware using Eco/Auto settings. Not verifiable here: this machine has no integrated GPU. For reference, the benchmark at 1080p Eco reaches a frame-time p95 of 3.0 ms on an RTX 3060 and 62.6 ms on llvmpipe (`benchmarks/README.md`). Run `cargo run --release --example render_bench -- eco` on UHD 620-class hardware to close this item.
- [x] Record CPU frame time, GPU pass time, draw/dispatch count, triangles, memory, visible instances, upload bytes, physics bodies, event/readback bytes, synchronization latency, and overflow. Evidence: `run_render_benchmark` (`src/rendering/benchmark.rs`) records each of these into a JSON `RenderBenchmarkReport`. The report gives frame, CPU and GPU times as mean, p95 and max, and mean GPU time per pass. The GPU test `benchmark_records_every_metric_along_the_camera_path` checks that every counter is populated on a 30-frame run, including the 9 lights Eco drops. The editor profiler shows the same counters live.
- [x] Store performance baselines and reject material regressions rather than relying only on FPS logs. Evidence: `render_bench <quality> <baseline.json>` writes a missing baseline, or compares against an existing one and exits 1 on any regression. `RenderBenchmarkReport::regressions` applies the rules. Work counters may grow at most 10% when extent and quality match. Times are compared only on the same device and driver, and may grow at most 10% and 0.5 ms. Any new overflow is a regression. Unit test `regressions_flag_material_growth_only` covers these rules. `benchmarks/rtx3060-linux-eco.json` is stored, and two later runs reported no regressions against it. CI enforcement waits for the scheduled performance job in the CI matrix.
- [x] Document tested drivers, operating systems, resolutions, and quality settings. Evidence: `benchmarks/README.md` lists the RTX 3060 (NVIDIA 615.71.09) at every profile and llvmpipe (Mesa 26.2.3) on Eco. All runs were 1080p on CachyOS Linux 7.2.8. The README also names the configurations not yet tested: UHD 620, Windows, AMD, and presented frame pacing.

### Fast game creation and agent feedback loop

A small browser game can feel more polished during development because code changes are quick to preview, input can be automated, and the resulting frame is easy to inspect. RustingEngine needs an equivalent **edit → run → observe → revise** loop for native games. This is a workflow and game-feel target, not a requirement to add an AI model to the engine. The detailed backlog is in [`missingFeatures.md`](missingFeatures.md); the items below are the Milestone 7 acceptance path. Later milestones can deepen each subsystem without postponing its first usable slice.

- [x] Provide the first window-free `rusting` CLI slice: `doctor`, `new`, project/scene inspection, stable ID/name/class/component queries, `validate`, and `cook`, using shared project and scene library operations. Evidence: `tests/cli.rs` covers `testGame`, generated projects, malformed input, missing assets, query ordering, JSON errors, and exit codes; `cargo test --workspace` passed, and `cargo test --workspace --features gpu-tests -- --test-threads=1` passed.
- [x] Complete the scriptable project loop with `check`, `run`, and `export` commands. Reuse the editor's build/play/export operations; return the same versioned result envelope and verify the exported game from an isolated folder. See Milestone 21.
  - Verified: `rusting check`, `rusting run [--release] [--ticks N] [--timeout S]` and `rusting export <root> <parent> [--target T]` live in `src/cli.rs` and return the `schema_version` 1 envelope. Export uses the editor's `export_built_game`, now in `src/project.rs`, and the editor export test still passes. `run --ticks N` sets `RUSTING_HEADLESS_TICKS`, which `run_project` reads to call `run_project_headless`. A native export is copied to a fresh temporary folder and must run 60 headless ticks with exit code 0; a cross-target export is not run and carries an `EXPORT_NOT_VERIFIED` warning. `generated_project_checks_runs_and_exports_a_verified_game` (ignored, real cargo builds; run with `cargo test --test cli -- --ignored`) passed: check, a 30-tick run with exit 0, a verified export containing the cooked scene, and a corrupted scene that makes `run` exit 1. `run_and_export_reject_malformed_flags_as_usage_errors` covers flag errors with exit 2.
- [x] Add a public offscreen `capture` command that renders a specified camera and tick to PNG, reports camera and render metadata, and maps picked pixels to persistent scene IDs. Keep CPU-only validation available when Vulkan is absent. See Milestones 4 and 20.
  - Verified: `rusting capture <scene> <out.png> [--camera ID|NAME] [--tick N] [--size WxH] [--pick X,Y]...` (`capture_scene` in `src/cli.rs`) loads the scene, renders every tick up to N offscreen so GPU physics advances, and writes the last frame as a PNG. It reports camera ID, name, projection, transform and world matrix; device, driver, requested and resolved quality, draws, triangles, visible instances and dropped lights; and, per pick, the persistent ID, name, hit distance, world position and rendered RGBA. Picks use CPU rays against mesh bounds (the same method as gameplay clicks), so a rendered ID buffer is still missing for exact silhouettes. `try_init_vulkan_headless` returns an error instead of panicking, and without a device the command still reports the camera and picks with a `VULKAN_UNAVAILABLE` error. Game code is not run. Tests: `capture_without_vulkan_still_reports_camera_and_picks_per_tick` (the device selector is forced to miss; a falling CPU-physics cube is under the pixel at tick 0 and gone at tick 60; also covers an unknown camera and an out-of-range pick) and `capture_renders_a_png_and_maps_pixels_to_scene_ids` (`--features gpu-tests`; a 320x180 PNG whose pixel matches the reported pick color, and camera selection by name and by ID).
- [x] Add input-driven `test` scenarios: named actions at fixed ticks, assertions on reflected game state and events, optional captures, reproducible seeds, and a compact failure trace with the first failing tick. This is the first slice of the broader replay work in Milestone 8.
  - Verified: `src/scenario.rs` defines the JSON `Scenario` (`name`, `seed`, `ticks`, `capture_size`, `steps`) with `press`/`release` of named `ActionMap` actions, `expect` (a JSON pointer into the entity's reflected scene form, with registered components parsed; `equals` with `tolerance`, `greater_than`, `less_than`, and `until` to hold a check across ticks), `expect_events` (collision counts, optionally per entity), and `capture` (offscreen PNG through the shared `HeadlessCapture`, skipped without Vulkan). `run_scenario` sets the new `RandomSeed` resource (splitmix64 indexed by fixed tick and stream, in `rusting-core`), advances exactly one fixed step per tick, stops at the first failure, and reports `first_failure` with tick, step index, message and the observed value. `run_project` runs a scenario when `RUSTING_TEST_SCENARIO` is set and writes the report to `RUSTING_TEST_REPORT`; `rusting test <root> <scenario.json> [--release] [--timeout S]` builds the game, runs it, and returns `SCENARIO_FAILED` with `tick T step S: ...`. Tests: `scenario::tests` (actions change game state at the pressed tick, a falling cube and collision events pass, an out-of-range step and an unbound action fail, an `until` check reports its first failing tick and stops the run there, and the same seed reproduces a seeded game value while another seed changes it) and `generated_game_passes_and_fails_scenarios_with_a_tick_trace` (ignored, real cargo build; passed with a capture step and a failing run that exits 1). Only CPU collision events are countable so far; GPU physics and click events are left to Milestone 8 replay work. Full check passed: fmt, clippy in all three configurations, `cargo test --workspace` (lib 266 passed), and `--features gpu-tests -- --test-threads=1` (lib 328 passed).
- [x] Publish a generated capability and scene/component schema catalog. Give each operation defaults, examples, units, valid ranges, and GPU/readback cost; test examples against the parser and editor reflection registry. See Milestone 9.
  - Verified: `rusting schema [--json]` prints `rusting_engine::schema::catalog()` (`src/schema.rs`, catalog version 1). It lists the 13 CLI operations from one `OPERATIONS` table that now also builds `rusting --help`, each with usage, summary, flag defaults, an example, and GPU/readback cost; all 16 scene entity sections and all 7 registered components, each with summary, GPU cost, default, example, and per-field unit, range and notes; the scenario format; and the 144-byte physics-sync readback size. Defaults are not hand-written: they are the scene form of each component's `Default`, saved through `scene_document` and the component registry. Tests: `every_saved_field_is_documented_and_every_section_is_listed` (the entity keys the scene writer saves equal the documented sections, the catalog's components equal the `SceneComponentRegistry`, every saved leaf field is documented and every documented field exists; removing one field's docs makes it fail), `examples_load_through_the_scene_parser_and_registry_unchanged` (all examples form one scene that parses, loads, and saves back identically, and each component example restores through `set_registered_component`; this caught that `BuiltinPrimitive: Cube` saves as `BuiltinCube`), `catalog_lists_every_operation_with_defaults_and_cost`, and `schema_catalog_examples_parse_and_help_lists_every_operation` in `tests/cli.rs` (every example runs without a usage error and every usage appears in `--help`). The editor's typed inspectors are not part of the catalog; they draw the same registered components. Full check passed: fmt, clippy in all three configurations, `cargo test --workspace` (lib 269 passed), and `--features gpu-tests -- --test-threads=1` (lib 331 passed).
- [x] Support atomic scene patch batches with persistent IDs, dry-run diffs, revision checks, validation, and editor snapshot undo. An agent and a person editing the same open scene must see conflicts instead of silently replacing each other's work. See Milestones 6 and 9.
  - Verified: `rusting scene patch <scene> <patch.json> [--dry-run]` uses `src/scene_patch.rs`. Operations `create`, `set`, `remove` (with optional per-field `expected`), `reparent`, `duplicate` (one entity) and `delete` (with descendants) address entities by persistent ID and fields by JSON pointer into the scene form with registered components parsed; `/id` cannot change. The patched document must deserialize as a `SceneDocument`, pass `validate_scene_structure`, and restore every built-in component through the registry; unknown components are written with a `PATCH_UNVALIDATED_COMPONENT` warning. Nothing is written on any error, and the file is re-read before the atomic write. The result carries `revision_before`/`revision_after` (FNV-1a 64 of the file bytes, `scene_revision`), created and deleted IDs, and per-leaf `{id, name, path, before, after}` changes. `scene inspect` and `scene query` report `revision`; a stale `expected_revision` or `expected` value gives `SCENE_CONFLICT`. The editor records the revision on load, project open and save (`SceneFileRevision`). Each frame in Edit mode, a changed file reloads behind a `remember_scene_before_edit` snapshot when the scene is clean, keeping the selection; with unsaved edits it keeps them and reports the conflict, and `save_editor_scene` returns `SceneIoError::Conflict` once before it overwrites. Tests: `scene_patch::tests` (a six-operation batch and its diff; eight failing batches, including a failure after an earlier operation succeeded, a stale revision, a field conflict, duplicate names, a cycle, a bad type, a bad built-in component value and an `/id` change, all leave the file byte-identical; a dry run does not write), `scene_patch_dry_runs_writes_and_reports_revision_conflicts` in `tests/cli.rs`, and `outside_scene_patch_reloads_under_undo_or_conflicts_with_unsaved_edits` in the editor tests. The editor watch compares the file time first, so a writer on a file system with a coarse clock can be missed until the next change; there is no merge UI yet. Full check passed: fmt, clippy in all three configurations, `cargo test --workspace` (lib 272 passed), and `--features gpu-tests -- --test-threads=1` (lib 334 passed).
- [x] Add a fast preview/restart path for gameplay edits, with measured save-to-first-playable-frame time and clear diagnostics when Rust, shaders, or assets fail to reload. State-preserving Rust reload can follow in Milestone 9; a reliable clean restart is the initial gate.
  - Verified: the clean restart path is the editor's existing Restart (save, stop the game, wait for the worker to confirm the exit, rebuild, relaunch; `restart_waits_for_worker_exit_and_stop_cancels_pending_restart`). A game now prints `FIRST_FRAME_MARKER` (`[rusting] first playable frame after N ms`, to standard error) once, after its first presented frame or first headless tick, counted from `run_project`. The editor's `EditorBuildState` times Play/Restart from the save to that line and writes `First playable frame X s after save (build Y s, game startup N ms)` to the Console. `rusting run` returns `timings` (`build_ms`, `game_first_frame_ms`, `command_to_first_frame_ms`). `crate::project::reload_diagnostics` splits `cargo --message-format short` output into per-file Rust errors and shader errors (`vulkano_shaders` compiles shaders at build time, so they are build errors that name a shader file) and reads `hot reload failed:` lines from the game; the asset server already keeps the last good asset after a failed reload. The editor shows one Console error per Rust or shader error and one per failed asset, and the CLI returns `RUST_BUILD_ERROR`, `SHADER_BUILD_ERROR` and `ASSET_RELOAD_FAILED` diagnostics with the file. Tests: `reload_output_is_split_into_file_diagnostics_and_first_frame_time`, `play_reports_first_frame_time_and_per_file_reload_errors`, and `generated_project_checks_runs_and_exports_a_verified_game` (ignored, real cargo builds; passed with first-frame timings and a Rust error that names `src/main.rs`). Measured here with `rusting run --ticks 1` on a generated project (debug, headless, shared target folder): cold build 3228 ms to first frame, no change 96 ms, after touching `src/main.rs` 638 ms, with game startup 7–8 ms each time. Windowed first-frame time includes Vulkan and window setup and needs a real display to measure. State-preserving Rust reload stays in Milestone 9. Full check passed: fmt, clippy in all three configurations, `cargo test --workspace` (lib 274 passed), and `--features gpu-tests -- --test-threads=1` (lib 336 passed).
- [x] Deliver a small game-feel kit: one reusable camera/controller recipe, input actions, tween/easing, event-triggered sound, burst effects, and a minimal runtime HUD with text and buttons. Expose these through ordinary scene data and Rust APIs so a generated game is inspectable and editable. Expand them in Milestones 9 and 14–17.
  - Verified: the camera/controller recipe is the existing `rusting.player_controller` (`PlayerController` plus a child `Camera`), and input actions are the existing `ActionMap` with the `player.*` defaults. New in `src/runtime/game_feel.rs`, each a registered scene component with a Rust API and a schema catalog entry: `rusting.tween` (`Tween`: Position, Rotation or Scale from `from` to `to` over `duration` after `delay`, eight `Easing` curves, Once/Loop/PingPong), `rusting.sound_cue` (`SoundCue` sends a `SoundEvent{entity, clip, volume}` when its body starts touching another collider or after `trigger()`), `rusting.burst_emitter` (`BurstEmitter` spawns `count` `BurstParticle` entities that copy the emitter's `MeshRenderer`, fly out, fall and shrink, then despawn), and `rusting.hud` (`HudElement` text or button at an anchor, drawn through `RuntimeUi`; a click sends `HudButtonPressed`). Tweens, contact triggers and particles run on the fixed step after CPU physics, and burst directions come from `RandomSeed` indexed by the fixed tick and the emitter's `SceneId`. The engine has no audio output: playing the clip needs an audio crate, which is a new dependency that needs the owner's approval, so a game plays `SoundEvent` itself for now. Tests: `easing_curves_start_at_zero_and_end_at_one`, `tweens_play_once_loop_and_ping_pong_on_the_fixed_step`, `landing_fires_one_sound_and_a_seeded_burst_that_expires` (one sound on landing, five particles within launch speed, identical velocities on a second run, none left after their lifetime, manual triggers), `hud_draws_scene_text_and_reports_button_clicks` (egui pointer press and release over the button), and the schema tests, whose examples load through the scene parser and registry. Full check passed: fmt, clippy in all three configurations, `cargo test --workspace` (lib 278 passed), and `--features gpu-tests -- --test-threads=1` (lib 340 passed).
- [x] Provide a quick 2D creation path for games that do not need 3D: sprites, a simple tile grid, orthographic camera, screen-space UI, and a playable 2D starter scene. Keep this a first slice of Milestone 17 rather than requiring its entire renderer and physics scope.
  - Verified: 2D games live in the XY plane with the existing renderer and CPU physics. Sprites are `MeshRenderer`s with the new `PrimitiveShape::Quad` (upright, facing +Z, UV origin top-left) and an unlit material; the camera is the existing orthographic `Camera`; screen-space UI is `rusting.hud`. New in `src/runtime/two_d.rs`, both registered scene components with schema entries: `rusting.tile_map` (`TileMap`: text rows, one `TileKind{color, texture, solid}` per character; `build_tile_maps` spawns one unsaved quad per tile and one fixed box collider per run of solid tiles in a row, and respawns on change or removal) and `rusting.platformer_controller` (`PlatformerController`: run, jump and gravity through `move_character` on the fixed step, with a 0.1 s jump buffer and ledge grace). `rusting new <parent> <name> --template 2d` writes a side-view platformer: a tile level, a player with an orange sprite and a following orthographic camera, a sensor goal with a burst emitter, and a HUD hint. Generated projects now enable the `ui` feature so the HUD draws. Tests: `tile_maps_spawn_merged_colliders_and_rebuild_or_clean_up`, `platformer_runs_jumps_and_lands_on_tiles`, `the_2d_template_is_a_playable_side_view_level` (the generated scene loads, the player rests on the ground, seven merged colliders, and running with a jump clears the two-tile step and lands more than 3 m right), and the schema tests. `rusting capture` of the generated scene at tick 60 on an RTX 3060 showed the ground, steps, platform and player sprite. The editor does not build tile maps in Edit mode yet (backlog in `docs/editor-overhaul.md`). Full check passed: fmt, clippy in all three configurations, `cargo test --workspace` (lib 281 passed), and `--features gpu-tests -- --test-threads=1` (lib 343 passed).
- [x] Make assets easy to replace and polish: import images and glTF with stable IDs, settings, dependency reporting, reimport, and source/license provenance. Provide starter art-direction presets for lighting, camera, typography, and color that remain editable rather than baking a style into the engine. See Milestones 2, 13, and 16.
  - Verified: `src/asset_import.rs` imports png/jpeg/bmp/tga/gltf/glb files into `<project>/assets/<folder>/` after loading them as the runtime would (image decode, `AssetServer::import_gltf`), copies a glTF's external buffers and images (URIs that leave its folder are rejected), and writes `<file>.rmeta` with a new UUID, `ImportSettings` (`max_size` downscales images), dependencies, an FNV-1a content hash, and `AssetProvenance` (original, author, license, url, generator, notes). `reimport_asset` finds the asset by ID or path, replaces it from `--from`, the recorded original, or itself, and keeps the ID; given provenance fields and settings replace the recorded ones. `list_assets` reports each asset with the project scenes that reference it, and flags missing files and dependencies, invalid or duplicate metadata, files changed since import, files without `.rmeta`, and missing licenses; `validate` now fails on the error-level ones. CLI: `rusting asset import|reimport|list`, in `--help` and the schema catalog. `src/art_direction.rs` has four presets (`daylight`, `golden_hour`, `night`, `flat_toy`) that `rusting preset apply <scene> <name> [--dry-run]` writes as one revision-checked scene patch: the sun (a `Sun` entity is created when the scene has none), ambient and sky light, tone mapping, the new `rusting.background` component (`SceneBackground`, copied into `RenderSettings` each frame), perspective camera field of view, and HUD font size plus the new `HudElement::color`. Reapplying edits the same entities, and the editor sees the change as an outside patch under undo. Tests: `images_import_with_settings_provenance_and_a_stable_id_on_reimport`, `gltf_import_copies_and_reports_dependencies`, `presets_patch_lighting_camera_text_and_background_and_reapply_in_place`, `every_preset_has_a_unique_name_and_sane_values`, `assets_import_list_and_reimport_with_a_stable_id` and `art_presets_list_and_apply_as_scene_patches` in `tests/cli.rs`, and the schema tests. `rusting capture` of the 3D template on an RTX 3060 before and after each preset showed distinct backgrounds, light, and field of view. Editor import dialog, Replace action, and preset picker are in the `docs/editor-overhaul.md` backlog. Full check passed: fmt, clippy in all three configurations, `cargo test --workspace` (lib 285 passed), and `--features gpu-tests -- --test-threads=1` (lib 347 passed).
- [x Expose optional, model-agnostic asset-generator hooks that accept generated images, models, and audio through the normal importer, then validate file type, dimensions/geometry, dependencies, and license/provenance metadata. Give the editor and CLI the same preview and replace operation; game projects must remain usable without an AI service.
  - Verified: `ProjectManifest.generators` (optional, omitted from `project.json` when empty) maps a hook name to `GeneratorHook{command, license, settings}`. `asset_import::generate_asset` runs the command from the project root without a shell, with `RUSTING_PROMPT` and an empty `RUSTING_OUTPUT_DIR`, reads the last stdout line as JSON (`file` plus any `AssetProvenance` field), and passes the file to `import_asset`, or to `reimport_asset` for `--replace`, so it gets the same file type, image size, glTF geometry and dependency, and metadata checks. It records `original: "generator:<hook>"`, the hook as generator, and the prompt as notes. Output without a license from the hook or its config is rejected (`GENERATOR_FAILED`), as are failed runs and unparsable output; unknown hooks give `GENERATOR_UNKNOWN`. The importer now accepts WAV (sample rate and length are read from the `fmt ` and `data` chunks) and Ogg (magic only; a `ponytail:` note defers length to Milestone 15). `import_asset` and `reimport_asset` take `dry_run`, which runs every check on a staged copy in the temp folder and returns the report with `dry_run: true`. CLI: `rusting asset generate <project> <hook> <prompt> [--to F | --replace ASSET] [--dry-run]`, plus `--dry-run` on `asset import` and `asset reimport`, in `--help` and the schema catalog. Editor: an imported file's right-click menu has Replace…, which picks a file of the same type, shows the dry-run preview (size, referencing scenes, license, copied dependencies, or the blocking error) in a modal, and calls the same `reimport_asset` on confirm; the asset server's hot reload picks up the file. The Assets tree hides `.rmeta` sidecars. Nothing at runtime depends on a hook. A Generate dialog in the editor is in the `docs/editor-overhaul.md` backlog. Tests: `audio_imports_and_dry_runs_write_nothing`, `generator_hooks_import_and_replace_through_the_importer` (a `sh` hook: preview, import with hook settings, replace keeping the ID, and unlicensed, silent, failing and unknown hooks), `generator_hooks_preview_and_import_through_the_cli` in `tests/cli.rs`, `replacing_an_imported_asset_previews_then_writes_on_confirm` (the editor's dialog result, preview modal, and Replace click), and the schema tests. Full check passed: fmt, clippy in all three configurations, `cargo test --workspace` (lib 288 passed), and `--features gpu-tests -- --test-threads=1` (lib 350 passed).
- [x] Ship one small, complete starter game and a repeatable acceptance exercise: from a one-paragraph request, create, modify, run, visually inspect, test, and export it using public tools. Record time to first playable frame, manual interventions, failed edits, and whether the requested mechanics and presentation are visible. Keep the CLI, editor, and optional MCP adapter on the same service layer; MCP is an adapter after these operations work locally.
  - Verified: `rusting new --template starter` creates Coin Run (tile map, platformer, five `rusting.pickup` coins, `rusting.counter` HUD with `{name}` placeholders, flag gated by `requires`, "You win!" HUD). `samples/starter_game/request.md` is the one-paragraph request and `win.scenario.json` plays it to the win. Tests: `pickups_count_hide_and_unlock_in_scene_id_order`, `hud_text_fills_counter_placeholders`, `the_starter_template_wins_its_sample_scenario`, plus the ignored `tests/starter_game.rs` exercise (new, preset, query, patch dry-run and apply, validate, run, capture with pick, test, export). `HeadlessCapture` now paints the runtime HUD, checked in the captured win frame. `benchmarks/starter-game-rtx3060-linux.json`: game first frame 20 ms, command to first frame 4.9 s warm (22.3 s cold build), 0 failed edits, 0 manual interventions, 2 corrective edits (background, tone mapping after `golden_hour`), mechanics visible, first coin picked on screen, win frame captured. The MCP adapter stays deferred (missingFeatures.md section 9); the CLI and editor share the library calls. Full check: fmt, clippy default / no-default-features / gpu-tests, `cargo test --workspace` (lib 291), gpu-tests (lib 353).

The fast-feedback bar is informed by [Vite's hot module replacement](https://vite.dev/guide/features) and [Playwright's action, screenshot, and error traces](https://playwright.dev/docs/trace-viewer). These are workflow references, not dependencies or a plan to turn the native engine into a web stack.

### Exit gate

- The vertical slice is playable from a clean checkout on Windows and Linux.
- Its scenes can be edited and saved in the integrated editor.
- Automated smoke, visual, physics, serialization, and performance tests pass.
- Packaging produces a runnable build with required assets and licenses.
- A fresh project can be created, edited, run, captured, tested with named input, and exported from public commands; a human can inspect and undo an agent's scene changes.
- The starter game has visible UI, audio/feedback, and a coherent editable visual style, verified by a capture and a scenario rather than by compilation alone.

## Milestone 8: Deterministic simulation and replay

Goal: make the simulation produce bit-identical results from the same inputs on every supported device, and make any divergence detectable and locatable. Determinism is an engine feature available to every game, not only to Sundering, and it is one of the physics pillars.

### Engine-level opt-in

- [x] Expose `DeterminismMode::{Off, Local, CrossPlatform}` per project, so games that do not need determinism do not pay for fixed-point math or fixed-order reductions. `Local` guarantees identical results on one machine and build; `CrossPlatform` enforces every rule in this milestone.
  - Verified: `determinism` in `project.json` (`Off` default, omitted when Off). `cook_scene` copies the nearest `project.json` value into the cooked scene (scene format 7, new last field `simulation`; cooked v6 is dispatched on its version so its render settings survive), and a replacing load inserts the `DeterminismMode` resource. `Off` runs no check and adds no work. Tests: `cook_copies_project_determinism_and_replacing_loads_insert_it`, `version_six_cooked_scenes_keep_render_settings`. Full check: fmt, clippy in all three configurations, `cargo test --workspace` (lib 295, cli 13), gpu-tests (lib 357).
- [x] Validate at startup that every registered solver, system, and custom compute shader supports the selected mode, and fail with a structured error naming the offender otherwise.
  - Verified: `check_determinism` runs in `load_project_runtime` (windowed, headless, and scenario runs) and returns a `DeterminismError { mode, offenders: [{part, supports}] }` sorted by part name. Parts: `cpu_physics` and each `gpu_solver:*` support `Local` (body-ordered, no order-dependent atomics, float math not pinned yet); a custom shader declares its level with a `// rusting: determinism = <mode>` line and otherwise supports `Off`; game systems declare theirs in the `DeterminismSupport` resource (a second declaration keeps the weaker mode). Undeclared game systems are not detected automatically. `rusting validate` reports one `DETERMINISM_UNSUPPORTED` error per part with the first body's scene location. Tests: `determinism_check_names_every_part_below_the_project_mode`, `shader_pragmas_declare_determinism`, CLI `validate_names_parts_below_the_project_determinism_mode`. Same full check as above.

### Deterministic math

- [x] Choose and document the simulation number format: fixed-point integer math, or strictly constrained IEEE-754 with fast-math disabled, fused-multiply-add behaviour pinned, and operation order fixed.
  - Verified: `docs/determinism.md` chooses constrained IEEE-754 `f32` over fixed-point (keeps the existing solvers; no `shaderInt64` need), lists the correctly rounded operations allowed as hardware ops, the ones replaced by shared routines (division, `sqrt`, `inversesqrt`, `dot`/`cross`/`length`/`normalize`, transcendentals), and the forbidden ones (FMA contraction, fast-math and relaxed precision, reassociation, NaN, subnormals; `CrossPlatform` needs Vulkan float controls for denormal preservation and RTE rounding). It records that the solvers meet `Local` only today. Documentation item; no code change.
- [x] Implement the chosen format once in a shared math module used by the CPU solver and every physics compute shader.
  - Verified: `src/shaders/sim_math.glsl` (`sim_recip`, `sim_div`, `sim_rsqrt`, `sim_sqrt`, `sim_dot`, `sim_length`, `sim_normalize`, `sim_mul`, `sim_mul_transposed`; bit-trick seed, three Newton steps, then the best of the result and its two neighbours, all `precise`) replaces every GLSL division, `sqrt`, `dot`, `length`, `normalize`, and matrix-vector product in the live physics shaders (`physics.comp`, `physics_contacts.comp`, `physics_shapes.glsl`). `runtime::sim_math` is the bit-exact Rust reference. The CPU solver keeps Rust's native `/` and `sqrt`, which IEEE-754 already defines exactly on every supported CPU (documented in `docs/determinism.md`); its transcendental calls move to the module in the transcendental item. Tests: GPU `shader_sim_math_matches_the_rust_reference_bit_for_bit` (64 cases, including one that differs under FMA contraction; removing `precise` makes it fail on lavapipe), unit `round_values_are_exact_and_the_rest_stay_within_two_ulp`; `commands_apply_once_before_the_step_and_reject_stale_bodies` still gets exact `3.0` from `3 / 1`. The routines always run, even with `Off`; they add a few ALU operations per contact; the GPU physics cost has not been re-measured on hardware. Full check: fmt, clippy in three configurations, `cargo test --workspace` (lib 296), gpu-tests (lib 359).
- [x] Forbid relaxed precision and fast-math in simulation shaders. Both remain available to rendering shaders.
  - Verified: every float operation in `physics.comp`, `physics_contacts.comp` (both passes), and `physics_shapes.glsl` is `precise`, with `precise` locals for values that only feed comparisons; `round` became `roundEven`. `simulation_shaders_are_precise_and_never_relaxed` (gpu-tests, needs `glslc`, documented in `docs/dev-environment.md`) compiles those shaders and `sim_math_test.comp` and fails on any float arithmetic without `NoContraction`, any `RelaxedPrecision` decoration, or any GLSL.std.450 call outside the exact set. `simulation_violations_flag_fusable_relaxed_and_inexact_code` feeds the checker hand-written SPIR-V with each violation. Removing the `radius_squared` `precise` makes the shader test fail. Rendering shaders are not checked. Full check: fmt, clippy in three configurations, `cargo test --workspace` (lib 297), gpu-tests (lib 361).
- [x] Replace transcendental functions in simulation paths with tabulated or polynomial implementations that have defined results on every device.
  - Verified: `runtime::sim_math` gains `sin_cos`, `atan2`, and `asin` (Cephes polynomials using only `+`, `-`, `*`, `/`, `sqrt`, and the exact `%`), plus `rotation_from_euler`, `euler_from_rotation`, `rotation_from_scaled_axis`, and `transform_matrix`. The CPU solver uses them for loading body rotations, integrating angular velocity, and writing rotations back; GPU bodies upload `transform_matrix`. The GPU physics shaders call no transcendental function, and `simulation_shaders_are_precise_and_never_relaxed` rejects any GLSL.std.450 call outside the exact set. Tests: `transcendentals_stay_close_to_f64_and_keep_their_bits` (8001 points; within 4e-7 of `f64` and `sin_cos` for `|x| <= 8192`; a checksum of every result bit is pinned and passes in debug and release), `rotation_helpers_match_nalgebra` (within 1e-6 of nalgebra; Euler round trip within 1e-5). Existing CPU physics tests pass unchanged. Not done: the checksum has only run on x86-64 here; ECS readback, player movement, and tween easing moved to `sim_math` in the simulation-state item below. Full check: fmt, clippy in three configurations, `cargo test --workspace` (lib 299), gpu-tests (lib 363).
- [x] Document which engine values are simulation state, which must be deterministic, and which are presentation only and may differ per machine.
  - Verified: `docs/determinism.md` "Simulation state" lists the components, resources, GPU buffers, events, and input that must be deterministic, the presentation-only values, the rule that presentation never writes simulation state, and the known gap (GPU events, proxies, and mirrors arrive one to three frames late). Fixes found while listing: the GPU clock `pc.elapsed` was the frame-timed game clock and is now `fixed_tick * fixed_delta` (GPU test `gpu_timers_count_simulated_ticks_not_the_frame_clock` sets the frame clock to 1000 s and sees the 0.04 s timer fire only from tick 3; it fails on the old code); teleport commands and GPU readback now convert with `sim_math::transform_matrix` and `transform_from_matrix` (round trip checked in `rotation_helpers_match_nalgebra`); player movement and tween easing use `sim_math::sin_cos` and explicit products instead of `powi`. Full check: fmt, clippy in three configurations, `cargo test --workspace` (lib 299), gpu-tests (lib 364).

### Deterministic execution order

- [x] Make broad-phase cell assignment, pair generation, and constraint ordering deterministic under any workgroup or thread scheduling.
  - Verified: found and fixed thread-dependent system order. `FixedUpdate` used bevy's multi-threaded executor, and five engine systems that write `Transform` (player and platformer movement, bursts, burst particles, tweens) had no order against CPU physics or each other. `engine_fixed_update_systems_have_one_order` builds the schedule with ambiguity detection set to error; it failed with 12 conflicts before the fix. The engine's fixed systems are now one chain, and `FixedUpdate` runs on the single-threaded executor, so unordered game systems follow the topological sort instead of thread timing. Already deterministic and now documented in `docs/determinism.md` "Execution order": the CPU solver sorts bodies by `Entity` and pairs into body order (`broad_phase_finds_the_same_pairs_as_testing_every_pair`, `broad_phase_prunes_a_tall_column_without_changing_pair_order`); GPU bodies upload by slot and colliders by entity; GPU contact cells and fallback fill in atomic order but sum in integers, and `gpu_bodies_collide_with_each_other_through_grid_and_fallback` gets the same bits from two runs and from a run with no hash memory. The chain updates existing burst particles before spawning new ones (`landing_fires_one_sound_and_a_seeded_burst_that_expires` caught the other order). Not covered: which events a GPU event-buffer overflow drops (counted and logged). Full check: fmt, clippy in three configurations, `cargo test --workspace` (lib 300), gpu-tests (lib 365).
- [x] Make solver iteration count and convergence criteria independent of timing, frame rate, and hardware.
  - Verified: already true, now documented in `docs/determinism.md` "Execution order": the CPU solver runs a fixed `SOLVER_ITERATIONS` (8) with no convergence exit, sleep counts `SLEEP_STEPS` ticks, the GPU runs one contact pass and one step per tick, and every fixed system (CPU physics, player, platformer, tweens, burst particles) reads `fixed_delta`, never the frame delta. New test `simulation_bits_depend_on_ticks_not_frame_pacing` runs a spinning three-box stack for 120 ticks at one tick per frame and in uneven frames (3, 0.5, 0.5, 0, 5 ticks) and compares every `Transform` and `RigidBody` bit; making CPU physics read the frame delta makes it fail. Not covered: `player_look` and `platformer_jump` sample input per frame (Replay items). Full check: fmt, clippy in three configurations, `cargo test --workspace` (lib 301), gpu-tests (lib 366).
- [x] Apply CPU-to-GPU commands in a deterministic order that does not depend on arrival time within a tick.
  - Verified: found and fixed three frame-dependent paths. (1) `time::advance` added all of a frame's steps to `FrameTime::fixed_tick` before `FixedUpdate` ran, so every step in a multi-tick frame saw the same tick (and tick-seeded bursts repeated); `App::update` now sets each step's tick. (2) The renderer applied a frame's whole command batch before its first GPU step; `GpuPhysicsCommands::apply_ticks` now records the GPU tick each command applies before (stamped after every fixed step and at extraction), the renderer sorts uploads by step and body, and the new push constant `command_first` gives each step its own range. (3) Ticks past `MAX_PHYSICS_STEPS_PER_FRAME` were dropped; they now wait for the next frame. Tests: `fixed_steps_see_their_own_tick_and_stamp_gpu_commands_for_the_next` (a three-tick frame sees ticks 0, 1, 2; stamps are 1, 2, 3 for fixed-step commands and 4 for an `Update` command), GPU `commands_apply_on_their_tick_however_frames_batch_ticks` (commands for ticks 3 and 6 give the same bits with one tick per frame, frames of three, and a ten-tick frame carrying two over; it fails when commands apply at the frame's first step). Documented in `docs/determinism.md` "Execution order". Full check: fmt, clippy in three configurations, `cargo test --workspace` (lib 302), gpu-tests (lib 368).
- [x] Add a seeded, tick-indexed RNG with independent streams per subsystem, and route all simulation randomness through it.
  - Verified: `RandomSeed::value(tick, stream)` (seeded splitmix64, already present) gains `RandomSeed::stream(subsystem, key)`, which hashes the subsystem name (FNV-1a) into the stream so subsystems that pick the same keys stay independent. Bursts, the only engine simulation randomness, now draw from `"bursts"` keyed by `SceneId`, or by entity when there is none; before, every emitter without a `SceneId` shared stream 0 and fired identical bursts. The previous item made the tick index correct in multi-tick frames. Audit: no simulation code reads `rand` (unused dependency), a thread-local generator, `Uuid::new_v4`, or `HashMap` iteration order, and the GPU physics shaders draw no random values. Tests: `subsystem_streams_are_independent_and_fixed` (value pinned), `emitters_without_scene_ids_draw_their_own_bursts` (fails with the old shared stream), `landing_fires_one_sound_and_a_seeded_burst_that_expires` (same seed and tick repeat). Documented in `docs/determinism.md` "Execution order". Full check: fmt, clippy in three configurations, `cargo test --workspace` (core 54, lib 303), gpu-tests (lib 369).
- [x] Make floating-point-to-integer conversions, clamping, and saturation behaviour explicit and identical across backends.
  Verified: `sim_to_int` in `sim_math.glsl` mirrors Rust `as i32` (`sim_math::to_int`: truncate, saturate, NaN to 0); the contact grid's `cell_of` and `to_fixed` use it; `shader_sim_math_matches_the_rust_reference_bit_for_bit` covers NaN, infinities, `±2^31`, `±3e9`, and halves; rules in `docs/determinism.md`. Full check green (lib 303, gpu 369).

### Verification harness

- [x] Compute a world-state hash every tick over all simulation state, cheap enough to leave enabled in release builds.
  Verified: `world_state_hash` runs after every fixed step into `StateHashes` (last 1024 ticks); `state_hashes_cover_every_tick_and_only_simulation_state` (pacing-independent, one-ULP change detected from its tick on, GPU readback `Transform` ignored; fails when readback copies are hashed); release cost about 0.4 ms per tick for 10,000 CPU bodies. CPU-visible state only; GPU buffers are the next item. Full check green (lib 304, gpu 370).
- [x] Include GPU body buffers in the world-state hash: hash each body's `PhysicsState` on the GPU, reduce in slot order, and read it back tagged with its tick.
  Verified: `physics_hash.comp` runs after every GPU tick and sums per-body hashes (state and rules) with integer atomics, so the order cannot change the result (an integer sum, not a slot-order fold); results reach `StateHashes::gpu` tagged with their tick; `commands_apply_on_their_tick_however_frames_batch_ticks` asserts one hash per tick, the same bits for 1/3/8+carry-over batching, and a changed hash from the tick of a one-ULP command change. Full check green (lib 304, gpu 370).
- [x] Add a headless simulation mode that runs with no window, surface, or renderer.
  Verified: `simulate_project_headless` (behind `run_project_headless`, `RUSTING_HEADLESS_TICKS`, and `rusting run --ticks`) builds only the ECS and asset stack, steps one `fixed_delta` per update, warns that GPU bodies stay still, and returns the `App`; `headless_simulation_runs_exact_ticks_from_a_cooked_scene` saves, cooks, and runs a scene for exactly 30 ticks with 30 recorded hashes. Full check green (lib 306, gpu 372).
- [x] Add a determinism runner that executes the same inputs for N ticks twice and compares per-tick hashes.
  Verified: `compare_runs` steps two apps in lockstep and compares every tick's hash; `compare_runs_reports_the_first_divergent_tick_and_body` passes identical runs, finds a velocity change on tick 6, and a seed change on tick 1. In-process only; separate processes come with the configurations item. Full check green (lib 306, gpu 372).
- [ ] Extend the runner across configurations: debug against release, differing CPU thread counts, differing GPU vendors, differing driver versions.
  Partial: `rusting determinism <project> [--ticks N]` builds debug and release, runs each headless in its own process with `RUSTING_STATE_HASH_OUT`, reruns release pinned to one CPU with `taskset` (one bevy and rayon thread), and compares every tick. Starter template: debug, release, and one-CPU runs give the same 120 hashes; a debug-only `1e-4` nudge of `Player` on tick 30 fails with `DETERMINISM_DIVERGED`, tick 31, `Player` and its `SceneId`. `first_divergent_tick` checked in `compare_runs_reports_the_first_divergent_tick_and_body`. Open: GPU vendors and driver versions, which need GPU bodies in the headless hash and real hardware. Full check green (lib 306, gpu 372).
- [x] Report the first divergent tick and the first divergent body, not merely that a mismatch occurred.
  Verified: in process, `compare_runs` reports tick, entity, `Name`, and `SceneId` (test above). Across processes, `rusting determinism` reruns both configurations up to the divergent tick and compares their per-entity hashes (`first_divergent_entity`); the injected nudge above names tick 31 and `Player`. Full check green (lib 306, gpu 372).
- [x] Run the determinism suite in CI on every change to physics, math, or shader code.
  Verified locally: `.github/workflows/determinism.yml` runs on pushes and pull requests touching `src/runtime`, `src/shaders`, `src/rendering`, `src/project_runner.rs`, `crates/rusting-core`, or `Cargo.lock`. It installs lavapipe and `glslc`, runs `cargo test --workspace --features gpu-tests` on lavapipe (bit-for-bit shader math, `precise` scan, batched GPU hashes), then `rusting determinism` for 600 ticks on a fresh starter project. Both steps run here as written under bash: lavapipe (llvmpipe, Mesa 26.2.3) gpu-tests lib 372 passed; the determinism step exits 0. Not yet run on GitHub runners. Full check green (lib 306, gpu 372).

### Replay

- [x] Record match input streams with tick numbers, a seed, and a format version.
  - Verified: `src/runtime/replay.rs` defines the JSON `Replay` (`format_version` 1, `seed` from `RandomSeed`, `start_tick`, per-frame `tick`, `delta_nanos` and `RuntimeInput` stored only when it changes, plus every tick's hash). `App::start_recording`/`finish_recording` hold the recorder outside the World, since a bevy resource allocates an entity and would shift entity order and hashes. `RuntimeInput` now uses `BTreeSet` and derives serde (winit `serde` feature). A game run with `RUSTING_REPLAY_OUT` writes its replay on exit (windowed path not exercised here, no display test). Test: `replays_reproduce_recorded_hashes_and_find_changed_input` records 90 uneven frames with held and pressed keys and stores under 20 inputs. Full check green (lib 307, gpu 373).
- [x] Replay a recorded stream and verify it reproduces the recorded per-tick hash sequence.
  - Verified: `play_replay` checks the format version and start tick, sets the seed, feeds each frame's delta and input, and returns the first divergent tick. The same test round-trips the replay through JSON and reproduces all 140 hashes; removing one jump press reports the tick after that frame (the press is buffered in `Update`), and version 0 is rejected. `RUSTING_REPLAY_PLAY` plays a replay headless and fails with the divergent tick. Full check green (lib 307, gpu 373).
- [x] Support replay seeking by re-simulating forward from periodic full snapshots.
  - Verified: `src/runtime/snapshot.rs` adds `App::snapshot`/`restore`: every entity (resources included) with its registered components, plus the entity allocator's state, including bevy's hidden local free list, read by allocating and freeing, then rebuilt. Unregistered types fail with their names (bevy_ecs `debug` feature). `ReplaySeeker` snapshots every N ticks and seeks by restoring the nearest snapshot into a new app and re-simulating, still checking each tick's hash. Tests: `snapshots_restore_entity_ids_and_state_into_a_new_app` (particles reuse ids at raised generations; the restored app matches ids, allocator and 52 frames of hashes), `snapshots_name_unregistered_types`, and `replays_of_the_starter_template_seek_through_snapshots` (seeks 240, 10, 190, 60, 240 with 4 snapshots, checks the counters, and reports a tampered hash at tick 170). Full check green (lib 311, gpu 377).
- [x] Keep replays independent of render settings, resolution, quality profile, and window size.
  - Verified by construction: playback runs headless with no renderer, and the viewport size that click picking reads is part of the recorded `RuntimeInput`, so the playback window never enters the simulation. GPU physics bodies are not in the world-state hash yet, so replays cover CPU simulation only. Full check green (lib 307, gpu 373).

### Exit gate

- 10,000 GPU bodies simulated for 10,000 ticks produce identical per-tick world-state hashes on at least two GPU vendors, and on both debug and release builds.
- A recorded replay reproduces its original hash sequence exactly.
- A deliberately introduced divergence is reported with its first divergent tick and body.
- If bit-identical cross-vendor results prove unachievable, that result is documented with evidence, and Milestones 19 and 24 proceed with server-authoritative networking without cross-vendor rollback.

## Milestone 9: Scene composition, reflection, and iteration speed

Goal: give the engine Godot's authoring model — reusable scenes, signals, groups, data resources, and fast code iteration — expressed through ECS instead of a node tree.

Depends on: Milestones 1, 2, and 6.

### Reflection

- [x] Add a reflection registry for components, resources, and asset types: field names, types, ranges, defaults, and editor hints, derived from Rust types rather than hand-maintained tables.
  Verified: `src/reflect/` adds the `Reflect` trait and the `reflect!` macro (our own, no new crate). The macro destructures every field and matches every variant, so a description that misses or misnames a field does not compile. Hints are `unit`, `min`, `max`, `doc`, and `color`; defaults come from `Default`. Components register through `register_scene_component` (which now requires `Reflect`); resources and asset types register in `TypeRegistry` (`rusting.render_settings`, `physics_settings`, `random_seed`, `determinism`, and the `rusting.material` asset). All 16 built-in components are described in `src/reflect/builtin.rs`, and `rusting schema` (catalog version 2) derives component fields, resources, and asset types from them instead of the old hand-written table. Tests: `the_macro_describes_structs_newtypes_and_enums`, `builtin_types_are_described_and_listed`, `every_saved_field_is_documented_and_every_section_is_listed` (checks pitch range `-1.55..1.55`, `/tiles/*/solid`, `/alpha_mode/Mask/cutoff`). Full check: fmt, clippy in three configurations, `cargo test --workspace` (core 54, lib 317, doc 16), gpu-tests (lib 383).
- [ ] Drive scene serialization, the Inspector, undo/redo, and animation property tracks from the same registry. Replace the generic JSON inspector path once parity is reached.
  Partial: scene save and load run through the registration's `TypeInfo` (walk, migrate, entity and handle conversion); the Inspector's generic JSON editor (`inspector/json.rs`) is replaced by `inspector/reflected.rs`, which draws typed widgets (numbers with ranges and units that do not clamp saved values on draw, linear and sRGB colors, enums, options, lists with add/remove, and drop-downs of scene objects and loaded assets for references, so a reference is never a half-typed ID) and applies edits through the existing snapshot undo path. `registered_component_field` and `set_registered_component_field` read and write one field by JSON pointer with type checks; this is the path API animation property tracks will use, but animation tracks themselves do not exist yet (Milestone 14 owns them), so this item stays open. Map key add/rename, doc tooltips, and an asset picker for assets not yet loaded are in the `docs/editor-overhaul.md` backlog. Tests: `field_paths_read_and_write_checked_values`, `field_names_become_captions`, `references_start_at_the_first_object_or_stay_empty`, `drawing_without_input_leaves_the_value_unchanged`, `registered_inspectors_draw_and_edit_their_component`. Full check: fmt, clippy in three configurations, `cargo test --workspace` (core 54, lib 317, doc 16), gpu-tests (lib 383).
- [x] Support reflected enums, nested structs, collections, typed handles, and entity references.
  Verified: `TypeInfo` covers unit, struct, and tuple enum variants, nested structs, arrays, `Vec`, `Option`, string-keyed `BTreeMap`/`HashMap`, `Handle<T>` for texture and mesh assets, and `Entity`. An `Entity` saves as the target's `SceneId` and is remapped on load; a handle saves as `{"$asset": path}` and loads the asset. A reference to an entity that is gone or has no `SceneId` saves as `null`; `null` or an ID no object has loads as `None` in an `Option<Entity>` and as `Entity::PLACEHOLDER` otherwise, so deleting a referenced object, a despawned target, or a preview-only object never blocks saving, undo, or loading. Handle assets load in `preload_component_assets` before the open scene is replaced, so a missing one leaves it untouched. Tests: `entity_references_and_handles_save_as_ids_and_asset_paths`, `the_macro_describes_structs_newtypes_and_enums`. Full check: fmt, clippy in three configurations, `cargo test --workspace` (core 54, lib 317, doc 16), gpu-tests (lib 383).
- [x] Reject or migrate renamed/removed reflected fields with a structured, versioned error instead of silently dropping data.
  Verified: unknown fields and variants fail the load with `ReflectError { component, saved_version, current_version, path, problem }` (CLI code `SCENE_COMPONENT_FIELD`); the message suggests a migration. `App::migrate_scene_component` takes `FieldMigration::rename` or `remove`; each raises the component version, saves write `"$version"`, older saves run the missed migrations, and newer ones fail with `NewerVersion`. Tests: `unknown_fields_and_variants_are_rejected_with_their_path`, `migrations_rename_and_remove_fields_and_newer_versions_fail`. Documented in tutorial 3 and `docs/concepts.md`. Full check: fmt, clippy in three configurations, `cargo test --workspace` (core 54, lib 317, doc 16), gpu-tests (lib 383).

### Prefabs and scene instancing

- [x] Instance a saved scene as a child subtree of another scene, keeping a link to its source asset.
  Verified: the built-in `rusting.scene_instance` component (`SceneInstance { source }`, in `src/runtime/scene_instance.rs`) links an object to a scene file, saved relative to the scene that holds it. `load_scene_document` adds the source scene's objects under that object before validation, with IDs from `member_id(root, source_id)` (FNV-1a, stable across loads and platforms) and entity references inside their components pointed at the copies. Added objects carry `InstanceMember` and the `rusting.instance_member` marker, so text saves drop them and keep only the link, while editor undo snapshots and cooked scenes keep them without reading the source again; a cooked game ships without the source scenes. A missing source fails the load with `SceneIoError::Instance` and leaves the open scene untouched. The editor's Assets panel instances a scene by double-click or `Instance in Scene`, through snapshot undo, and refuses the open scene. Changing `source` takes effect on the next load. Tests: `instances_add_the_source_scene_under_the_root_with_stable_ids`, `saves_keep_the_link_and_snapshots_keep_the_members_once`, `cooked_scenes_load_without_the_source_scene`, `instances_nest_and_a_scene_cannot_contain_itself`, `a_missing_source_fails_the_load_and_keeps_the_open_scene`, `an_empty_source_places_nothing`, `scenes_instance_as_one_linked_root_but_not_into_themselves`. Full check: fmt, clippy in three configurations, `cargo test --workspace` (core 54, lib 324, doc 16), gpu-tests (lib 390).
- [x] Store per-instance overrides as property diffs against the source, so source edits propagate to every instance that did not override that property.
  Verified: a text save (`fold_instance_members`) expands each instance's source again and stores, on the root under `rusting.instance_overrides`, a sparse JSON diff per member keyed by source object ID: only changed fields (component fields separately), `{"$removed": true}` for a removed component or map entry, and `null` for a deleted object. Loading merges the diffs into the source as it is now, so unchanged fields follow source edits; deleting an object removes its source children too. Objects the user adds under an instance's objects save normally, because member IDs are stable; if a later source drops their parent they move to the top of the scene instead of failing the load. Asset paths inside diffs are saved relative like every other path. Expanded roots carry `rusting.instance_expanded` with the source they came from, so snapshots never expand twice (also when every member was deleted), and a changed link saves no stale diffs. Tests: `overrides_are_saved_and_source_edits_reach_the_rest`, `a_changed_link_saves_no_overrides_for_the_old_members`, `diffs_merge_back_to_the_current_value`, `override_asset_paths_are_converted`. Full check: fmt, clippy in three configurations, `cargo test --workspace` (core 54, lib 328, doc 16), gpu-tests (lib 394).
- [x] Support nested prefabs and prefab variants (inheritance), with cycle rejection.
  Verified: nested instances expand recursively and a scene that contains itself, directly or through other scenes, fails with `SceneIoError::InstanceCycle` (`instances_nest_and_a_scene_cannot_contain_itself`). A variant is an ordinary scene whose one object instances the base scene, so it needs no new file format: its own edits save as overrides on that object, and a scene that places the variant saves its overrides against the expanded variant. Overrides therefore stack base, then variant, then level, and a base edit reaches every field neither layer changed. `save_scene_variant(base, path)` writes one, and first expands it once, so a missing base or a base that already contains `path` fails without writing. The editor's Assets menu for a `.rscene` file has New Variant, which writes `<name>_variant.rscene` (or `_variant_2`, ...) next to it and selects it; it does not change the open scene, so it adds no undo step. Structure note: the variant's objects sit under its one root, so a placed variant is one level deeper than a placed base. Tests: `variants_inherit_base_edits_under_their_own_overrides` (base, variant and level each change a different field; the base edit shows where no layer overrode it, and variants of themselves or of a scene built from them are rejected with the base unchanged) and `scenes_instance_as_one_linked_root_but_not_into_themselves` (New Variant names and content). Full check: fmt, clippy in three configurations, `cargo test --workspace` (core 54, lib 329, doc 16), gpu-tests (lib 395).
- [x] Revert, apply-to-source, and unpack-instance operations with undo.
  Verified: `edit_instance(document, id, InstanceEdit)` in `src/runtime/scene_instance.rs` works on a document of the open scene, and `id` may be the instance root or any object it placed. `RevertObject` replaces one placed object with its source values. `Revert` replaces all of them, which brings deleted ones back and keeps objects added under them. `ApplyToSource` maps the placed objects back to their source IDs (object references inside components too), restores the member markers of instances inside the source, folds those into overrides, validates and expands the result once (so an added instance that contains the source is rejected before writing), and writes the source file. Before writing, it saves the rest of the open scene's overrides against the old source and then expands again, so other instances pick up the change and keep their own overrides, and the applied instance keeps none. `UnpackCompletely` drops the link and every member marker, including those of instances inside it, and numbers names that now clash. Right-clicking an instance root or a placed object in the Hierarchy shows an INSTANCE section with the four actions. They run as `EntityRequest::Instance` behind the usual undo snapshot and keep the selection. Undo restores the open scene, but it does not restore a source file that Apply to Source wrote. Tests: `revert_resets_one_object_or_the_whole_instance`, `apply_writes_the_source_and_other_instances_keep_their_overrides` (including a source made of nested instances, which stay linked), `unpack_turns_an_instance_into_ordinary_objects` (the scene loads after the source is deleted), and `scenes_instance_as_one_linked_root_but_not_into_themselves` (the editor actions reload and keep the object). Full check: fmt, clippy in three configurations, `cargo test --workspace` (core 54, lib 332, doc 16), gpu-tests (lib 398).
- [x] Instance prefabs at runtime from gameplay code through typed handles.
  Verified: `AssetServer::load_prefab(path)` reads a `.rscene` file once into a `Prefab` asset in `AssetServer::prefabs` and returns a `Handle<Prefab>`; loading the same path again returns the same handle. `spawn_prefab(world, handle, transform)` in `src/runtime/scene_instance.rs` adds an instance root named `"<file stem> #<n>"` at `transform` with the prefab's objects under it, through the same `load_scene_document` path as scene loading (additive), so object references inside the prefab point at the new copies. Root IDs come from a per-world placement counter (`member_id(nil, n)`), so a replay that places the same prefabs in the same order gets the same IDs, and member IDs follow from the root as in saved scenes. A saved scene keeps each runtime instance as a link like any other. Systems call it through `commands.queue(move |world: &mut World| spawn_prefab(world, handle, transform).map(drop))`; a stale handle fails with `SceneIoError::MissingPrefab`. Limits: a changed prefab file is not reloaded while the game runs, runtime prefabs must live under `assets/` to be exported, and each placement scans for the root by `SceneId`. Test: `gameplay_code_places_loaded_prefabs_with_repeatable_ids` (one load per path, placement at the given transform, a second placement queued from commands after the file was renamed away, references pointing at each placement's own objects, identical IDs on replay, links kept on save, missing handle error). Full check: fmt, clippy in three configurations, `cargo test --workspace` (core 54, lib 333, doc 16), gpu-tests (lib 399).

### Signals, groups, and observers

- [x] Add entity-targeted observers that react to component insertion/removal and to typed events, as the ECS equivalent of Godot signals.
  Verified: `src/runtime/signals.rs`. `App::add_signal_handler(name, system)` registers a named Rust system whose input says the signal it answers: `In<Signal<E>>` for a typed bevy `EntityEvent` (sent with `commands.trigger`), `In<Signal<Added<T>>>` or `In<Signal<Removed<T>>>` for component `T` (a despawn counts as a removal). The emitting entity carries a `Connections` component (handler name plus receiver entity; `App::connect` checks both), and one bevy observer per signal type queues the matching handlers in connection order; `Signal` carries source, target and event. Bevy's per-entity `observe` is not used, because its observer entities hold boxed systems that a snapshot cannot copy; `Connections` is plain data, so snapshots and replays keep it, and `App::restore` mutes signals so writing components back runs no handler. Handlers must be registered during setup; a handler that re-emits its own signal does not run inside itself. `SignalError` converts to `AppError` for `Plugin::build`. Tests: `connected_handlers_answer_events_and_component_changes` (typed event through commands, two handlers in order, unconnected entity silent, add and despawn), `snapshots_keep_connections_and_restores_fire_nothing` (fails if the mute is removed), `connections_name_registered_handlers_and_live_entities`. Documented in `docs/concepts.md`. Full check: fmt, clippy in three configurations, `cargo test --workspace` (core 54, lib 336, doc 16), gpu-tests (lib 402).
- [x] Allow scene files to store event-to-handler connections between entities by `SceneId`, with registered Rust handler names, validated at load time.
  Verified: `Connections { list: [Connection { handler, target }] }` is a reflected built-in scene component, `rusting.connections`; the reflection saves `target` as the object's scene ID and loads it back to the entity, so the Inspector, scene patches, `rusting schema`, prefab ID remapping and cooked scenes handle it like any other object reference. `load_scene_document` checks every connection of the objects it spawned, in object-ID order, before replacing the open scene: an unregistered handler or a target that is not in the world fails with `SceneIoError::Connection { object, problem }` and leaves the open scene as it was. Tools that load scenes without the game's code (`keep_unregistered`: the editor, `rusting capture`) skip the check. Test: `scenes_save_connections_by_object_id_and_check_them_on_load` (save writes the target's ID, a new app loads it and the handler runs with the loaded entities, unknown handler and dangling target fail with the object ID and keep the scene, tools mode loads). The schema catalog lists the component with an example (`every_saved_field_is_documented_and_every_section_is_listed`). Full check: fmt, clippy in three configurations, `cargo test --workspace` (core 54, lib 337, doc 16), gpu-tests (lib 403).
- [x] Add entity groups/tags with fast membership queries and editor assignment.
  Verified: groups and tags are the existing object classes (`ObjectClasses`), which scenes already save per object and the Inspector's Classes section already assigns with undo. New in `src/runtime/classes.rs`: the `ClassIndex` resource maps each class name to its members in ascending entity-index order, kept current by `on_insert` and `on_discard` hooks on `ObjectClasses`, so inserts, replacements, removals, despawns and scene loads (including the editor's undo, which reloads the scene) update it. Snapshots skip the index and `App::restore` rebuilds it, because a restore overwrites components in place, past the hooks. Read it with `Res<ClassIndex>` (`members`, `contains`) or `App::class_members`. Editing `ObjectClasses` through `Mut` bypasses the index; this is documented. Test: `class_index_follows_insert_replace_remove_despawn_and_restore`. Documented in `docs/concepts.md`. Full check: fmt, clippy in three configurations, `cargo test --workspace` (core 54, lib 338, doc 16), gpu-tests (lib 404).
- [x] Add a typed scene-tree query API for common "find child / find in group / find by name" operations without hiding ECS queries.
  Verified: `src/runtime/scene_tree.rs`. `SceneTree` is a bevy `SystemParam` built from plain queries over `Name`, `Parent` and `Children` plus `Res<ClassIndex>`. Its methods return only entities, so component access stays in the system's own queries. Methods: `find_by_name` (the lowest entity index among objects with the name, so storage order does not change the answer; scans every named object), `child`, `find_path` (Godot-style `Arm/Hand` with `..`), `parent`, `children`, `descendants` (depth first, in hierarchy order), `name`, `in_class` and `is_in_class`. Test: `scene_tree_finds_children_paths_names_and_classes` (duplicate names, missing child, `..` path, empty path, descendants order, class members); the doc example compiles as a doctest. Documented in `docs/concepts.md`. Full check: fmt, clippy in three configurations, `cargo test --workspace` (core 54, lib 339, doc 17), gpu-tests (lib 405).

### Data resources

- [x] Add user-defined typed data assets (the equivalent of Godot `Resource`/`.tres`), serialized through reflection and editable in the Inspector. Evidence: `DataAsset`, `App::register_data_asset` and `AssetServer::load_data`/`save_data`/`reload_data` in `src/assets/data.rs` store `.rdata` JSON through the reflection walker. Unit test `data_assets_load_share_save_and_reload_through_handles` covers shared handles, nested references, relative paths, scene save and load of a `Handle` field, and reload. `bad_data_files_fail_with_the_reason` covers a wrong type, an unknown field and a reference cycle. The editor's Assets `New` menu and the Inspector's data asset view are covered by `data_assets_are_created_edited_and_reloaded_from_the_editor`. `cargo fmt --all --check`, clippy with `-D warnings` in all three configurations, `cargo test --workspace` (lib 346) and `cargo test --workspace --features gpu-tests -- --test-threads=1` (lib 412) passed. Undo for data asset edits and a picker for unloaded files are in the `docs/editor-overhaul.md` backlog.
- [x] Support shared versus per-instance-unique resource references. Evidence: a data asset handle saves as `{"$asset": path}` when it points at a file and is shared, or as `{"$data": value}` when its value has no file, and each `$data` loads as its own copy (`handle_to_scene`/`handle_from_scene` in `src/reflect/mod.rs`, used by scenes and `.rdata` files). `AssetServer::duplicate_data` makes a unique copy in game code, and the Inspector's `Unique` choice makes one from the current file and edits it in place. Unit test `unique_data_assets_are_private_copies_saved_in_place` covers duplicate independence, a unique value inside a `.rdata` file, a scene with one shared and two unique references loading as one file handle and two distinct copies, and a file reload that leaves the copies alone. Full check passed: fmt, clippy with `-D warnings` in all three configurations, `cargo test --workspace` (lib 347), and gpu-tests (lib 413). Saving a unique value out as a file is in the `docs/editor-overhaul.md` backlog.
- [x] Hot reload data assets into running play sessions. Evidence: `AssetPlugin`'s `poll_asset_loads` scan (every `HOT_RELOAD_SCAN_INTERVAL`, 500 ms) now also calls `DataAssetTypes::reload_changed`. It uses the same file-time check as meshes and textures to find changed `.rdata` files and reads each one again into its existing handle, which raises the asset's revision. Unique copies are left alone. Reloads and failures go through `AssetServer::take_reloaded` and `take_reload_failures`, which the editor Console and `project_runner` (`hot reload failed:`) already report. Unit test `changed_data_files_reload_into_a_running_game` covers a rewrite picked up by `App::update`, a raised revision, the reported path, an untouched unique copy, and a broken rewrite that keeps the last value and reports one failure. Full check passed: fmt, clippy with `-D warnings` in all three configurations, `cargo test --workspace` (lib 348), and gpu-tests (lib 414). Decoding runs on the main thread (`ponytail:` note; data files are small JSON).

### Iteration speed

- [x] Hot reload game Rust code during play, preserving ECS state through the reflection registry. Fall back to a clean restart with a clear message when the change is layout-incompatible. The owner chose a state-keeping restart instead of a dynamic-library game module (no new dependency, no ABI risk; costs one window and Vulkan start per reload). Evidence: the editor's `Reload Code` (`EditorBuildState::request_code_reload`) sends `save-state` on the game's standard input. The game writes a `scene_document` of every object plus its finished `once` keys to `build/code_reload_state.json` (`CODE_RELOAD_STATE_ENV`) and exits. The editor then rebuilds and starts it again, and `run_project` restores that state over the cooked scene with startup systems skipped. `load_scene_document` is transactional, so a saved component that no longer fits its type leaves the fresh scene and prints `code reload restarted clean:` with the reason, which the Console shows as a warning. A failed build keeps the saved state for the next Play, and a game that does not save within 10 s is killed and started clean. Unit test `code_reload_keeps_the_scene_and_runs_the_new_code` runs one game build, saves, then restores into a build with a changed update system: the position from the old code is kept and the new code moves the object on, a runtime-added component keeps its value, and neither the `once` block nor the startup system runs again. A changed component type starts clean with the error naming the component. Editor tests `streamed_game_gets_the_save_command_on_standard_input` and `code_reload_saves_a_running_game_and_resumes_after_a_failed_build` cover the process and state wiring. A manual end-to-end check used a generated project on Wayland with an RTX 3060. The old code moved a spawned cube along x for 1 s, and after the save and rebuild the new code moved it along y from the same x (4.26). The first frame came back 1.35 s after the reload started, and both processes exited 0. This run found and fixed an exit crash in the egui clipboard on Wayland. Full check passed: fmt, clippy with `-D warnings` in all three configurations, `cargo test --workspace` (lib 351), and gpu-tests (lib 417). Automatic reload on save and keeping resources are in the `docs/editor-overhaul.md` backlog.
- [x] Measure and report edit-to-running-game latency for a representative project; keep incremental Debug builds under a documented target. Evidence: `tests/iteration_latency.rs` (ignored; `cargo test --test iteration_latency -- --ignored --nocapture`) creates the starter template project, runs `rusting run --ticks 1 --json` and reports the median of three samples of `build_ms` and `command_to_first_frame_ms` for no change and for an edit to `src/main.rs`. It fails when the Rust edit takes more than the documented 3000 ms target. The target and results are in `docs/dev-environment.md` "Iteration speed". Run on 2026-09-28 (Ryzen 5 7600X, headless, shared `target/cli-games`): first build 40259 ms, no change 129 ms, Rust edit 853 ms (build 824 ms); test passed. Windowed Reload Code measured 1.35 s to the first frame (previous item).
- [x] Define an optional sandboxed WASM scripting host (`rusting-script`) exposing the reflected ECS API, for modding and designer-level logic. Native Rust remains the primary gameplay API. Evidence: new workspace crate `crates/rusting-script` (the owner approved the `wasmi` 2.0 dependency, with its `deterministic` feature). Games opt in with `ScriptPlugin`, which registers the `rusting.script` component (a `.wasm` or `.wat` module path) and runs each script's `start()` once and `update(delta_seconds)` every frame, in entity order. Host functions in module `rusting` (`log`, `entity`, `find`, `action`, `get`, `set`) read and write fields by component name and JSON pointer through `registered_component_field` and `set_registered_component_field`, so values are checked against the reflected type; `transform` addresses the object's transform. Sandbox: no WASI imports, 10 million fuel per call, 16 MiB memory, and a trap, limit or load error stops only that script with a `[rusting] script <path> stopped:` log line. Unit tests in `crates/rusting-script/src/tests.rs`: `scripts_read_and_write_reflected_fields` (start moves the object, update copies another object's field through JSON), `bad_requests_return_a_status_instead_of_stopping_the_script` (unknown component and wrong kind return -3, a missing entity -1, and a changed module path reloads), `sandbox_limits_stop_only_the_offending_script` (an endless loop, a 64 MiB memory, a non-Wasm `.wasm`, and a missing file each stop while another script keeps running), and `removed_scripts_are_dropped`. The host API, limits and examples are in `docs/scripting.md`. The Rust guest example there was not compiled, because the `wasm32-unknown-unknown` target is not installed on this machine. Full check passed: fmt, clippy with `-D warnings` in all three configurations, `cargo test --workspace` (lib 351, rusting-script 4), and gpu-tests (lib 417).
- [x] Deliver project templates for 3D first-person, 3D third-person, 2D platformer, and physics sandbox games. Evidence: `ProjectTemplate` gains `FirstPerson3d` (`first-person`), `ThirdPerson3d` (`third-person`) and `PhysicsSandbox` (`sandbox`) next to the existing `2d` platformer, with `ALL`, `name` and `label`; `rusting new --template` takes every name and the editor's Create Project form has a Template picker (`ProjectManagerState::template`). The player templates build a lit floor with fixed crates and a kinematic capsule `rusting.player_controller` with a child camera; third person adds a visible body and the new `PlayerController::camera_distance`/`camera_height`, which orbit the camera behind the body. The sandbox drops nine dynamic boxes and two balls onto a walled floor under a fixed camera. Unit tests: `third_person_camera_orbits_behind_the_body`; in `src/project.rs`, `template_names_round_trip`, `the_player_templates_stand_walk_and_frame_the_camera` (both player templates land grounded at y 0.9, walk forward with W, and the camera sits at eye height or behind and above), and `the_sandbox_template_settles_its_pile_on_the_floor` (after 20 s every box is still and every body is above the floor and inside the walls); the existing 2D template test still passes. Real GPU (RTX 3060) `rusting capture --tick 120` of each new template showed the expected frames, and `rusting run --ticks 60` of a generated third-person project built and exited 0 (first frame 15 ms). Full check passed: fmt, clippy with `-D warnings` in all three configurations, `cargo test --workspace` (lib 355, rusting-script 4), and gpu-tests (lib 421).

### Exit gate

- A prefab edited in the editor updates every instance that did not override the edited property, and overrides survive save and reload.
- An observer connected in a scene file fires the registered Rust handler at runtime.
- A reflected component added in game code appears in the Inspector, serializes, and animates without editor changes.
- Changing a gameplay system during play reloads it without losing scene state.

## Milestone 10: Advanced rigid-body physics

Goal: exceed Godot's built-in 3D physics feature set on the CPU path while keeping every feature available to hybrid CPU/GPU scenes.

Depends on: Milestones 5 and 8 (all new solver work follows the determinism rules).

### Constraints and articulations

- [x] Joints: fixed, hinge, slider, ball-socket, cone-twist, distance, spring, and a generic 6-DOF joint with per-axis limits, springs, and motors. Evidence: the `Joint` component (`rusting.joint`, `src/runtime/joints.rs`) links a CPU body to another CPU body or to the world. Every `JointKind` is a preset of six axes (locked, free, or limited, each with an optional soft spring and a force-capped motor), plus a cone limit and an anchor-distance limit or spring. The CPU solver builds one velocity row per constraint in joint entity order, warm-starts them from the last step (`PhysicsWorld` state, hashed in entity order), and solves them before contacts with 4 passes per iteration. Jointed pairs skip contacts unless `collide_connected` is set, and a moving or motor-driven side wakes the other. The component is reflected, listed in `rusting schema`, snapshot-registered, and saves `target` as a scene ID. Tests: `runtime::tests::hinge_pendulum_keeps_its_pivot_and_swings_in_its_plane`, `hinge_limits_stop_the_swing_and_motors_drive_it`, `sliders_move_along_their_axis_only`, `fixed_joints_hold_a_cantilever_in_place`, `ropes_cap_the_distance_and_springs_settle_at_their_stretch`, `cone_twist_joints_keep_the_swing_inside_the_cone`, `joints_link_bodies_without_contacts_and_round_trip_through_scenes` (also checks determinism), and `runtime::cpu_physics::joints::tests::swing_twist_splits_rotations_about_each_axis`. Full check passed: lib 363 tests, with `gpu-tests` 429. Limits: CPU bodies only (GPU solvers have no joints), and the anchor rows are iterated rather than solved as one 3×3 block.
- [x] Breakable joints with force/torque thresholds that emit typed events. Evidence: `Joint::break_force` (N) and `Joint::break_torque` (N·m), 0 meaning unbreakable, are reflected and listed in `rusting schema`. After the velocity solve, each breakable joint's load is its accumulated linear-row impulse (force) and purely angular-row impulse (torque) over the step length. A joint past either limit has its `Joint` component removed and sends a typed `JointBroken { joint, target, force, torque }` through `EventQueue<JointBroken>`, in joint entity order. The queue is registered by `App::new` and captured by snapshots. Test: `runtime::tests::joints_break_past_their_force_or_torque_and_send_an_event` covers a hanging ball whose joint breaks at 1 N and reports its weight, then falls; the same joint at twice its weight holds; and a 1 m cantilever breaks below its gravity torque and holds above it. Full check passed: lib 364 tests, with `gpu-tests` 430. Limit: the joint breaks after the step that overloaded it, so that step's impulse is still applied.
- [x] Reduced-coordinate articulations (Featherstone) for robots, chains, and ragdolls that need exact joint limits without drift. Evidence: `rusting.articulation` (`Articulation`, reflected, in `rusting schema`, captured by snapshots) on a root body turns the `Fixed`, `Hinge`, `Slider`, and `BallSocket` joints of its tree (breadth-first, joint entity order) into joint coordinates; other joints stay maximal and couple through the tree. `src/runtime/articulation.rs` reads the coordinates from the link poses each step and places every link by forward kinematics, builds the joint-space mass matrix from each link's Jacobian, and applies gravity and velocity-product forces from a recursive pass, taken at the midpoint speeds so fast chains do not gain energy. Contacts and joint rows on a link use `Gᵀ H⁻¹ G` as effective mass and move the tree through `H⁻¹`; hinge and slider limits, springs, and motors are joint-space rows, and each coordinate is clamped to its limit after integration. A dynamic root floats with six coordinates. Tests: `runtime::tests::articulated_chains_swing_without_their_joints_drifting` (8-link ball-socket and hinge chains, every link exactly 0.5 m from its parent within 1e-4 on every step for 4 s, energy never above 106% of the start, the chain stays in its plane); `articulated_hinge_limits_hold_exactly_and_motors_drive_them` (the angle never passes its −0.5 rad limit by more than 1e-4, where the maximal joint allows 0.05; a motor reaches 2 rad/s); `floating_articulations_land_rest_and_round_trip_through_scenes` (a floating torso with two ball-socket links lands on the ground with its links exactly in place, is bit-identical across runs, and the component round-trips through a scene). Full check passed: lib 367 tests, with `gpu-tests` 433. Limits: dense `H` (O(n³) per step; articulated-body algorithm for trees past ~30 links), no cone limits on ball sockets in a tree, reduced joints do not break, articulations never sleep, and a whipping chain tip overshoots its energy by up to about 5%.
- [ ] Stable long joint chains and high mass ratios verified by regression scenes.

### Bodies and characters

- [ ] Kinematic character controller with slopes, steps, ground snapping, moving platforms, and push interaction with dynamic bodies.
- [ ] Physics-based (dynamic) character option for games that want fully simulated movement.
- [ ] Raycast and wheel-collider vehicle models with suspension, tire friction curves, differential, and drivetrain.
- [ ] Ragdolls generated from skeletons, with a handoff API to and from animation (consumed by Milestone 14).
- [ ] Compound shapes, convex hulls, heightfields, and triangle meshes with per-triangle materials.

### Materials, forces, and fields

- [ ] Physics materials with friction, restitution, combine modes, and per-material contact events.
- [ ] Force fields: directional, radial, vortex, wind with turbulence, and custom field functions; usable by CPU and GPU bodies alike.
- [ ] Buoyancy and drag against water volumes.
- [ ] Gravity volumes and per-body gravity scale (planetary gravity, zero-g zones).

### Solver quality

- [ ] Simulation islands with sleeping and wake propagation.
- [ ] Speculative contacts plus swept CCD for fast bodies; bullets through thin walls never tunnel in regression scenes.
- [ ] Substepping with a documented stability/cost trade-off per quality profile.
- [ ] Contact caching and warm starting for stable stacks.
- [ ] Multithreaded CPU solver whose results are identical for any worker count.

### 2D physics

- [ ] 2D rigid bodies, shapes (circle, capsule, box, convex polygon, segment chain), joints, and queries, sharing the solver architecture and determinism rules with 3D.
- [ ] Hybrid 2D GPU bodies with the same condition/event/command bridge as 3D.

### Exit gate

- A regression scenario suite (pyramid stacks, joint chains, high mass ratios, CCD bullets, ragdoll piles, vehicles on uneven terrain) runs headless in CI with recorded expected results.
- Every joint type, character controller behaviour, and vehicle model has a test and a demo scene.
- CPU results are identical across worker-thread counts.
- 2D and 3D physics run in the same project without interference.

## Milestone 11: Deformable, continuum, and large-scale GPU physics

Goal: provide physics Godot does not have — deformables, fluids, granular media, runtime fracture, and million-body scale — all coupled in one world.

Depends on: Milestones 5, 8, and 10.

### Deformables

- [ ] XPBD soft bodies from tetrahedral meshes with volume preservation, attachment to rigid bodies, and tearing.
- [ ] Cloth with self-collision, wind interaction, attachment/pinning, and tearing.
- [ ] Ropes and cables with rigid-body attachment and correct tension transfer.
- [ ] GPU execution path for deformables with the same event and readback bridge as rigid GPU bodies.

### Fluids and granular media

- [ ] Particle-based fluid (PBF or SPH) with two-way rigid-body coupling and buoyancy.
  - [x] Step 1, the solver core: `runtime::fluid` (`Fluid`, `FluidSettings`) is a CPU position based fluid with a hash-grid neighbor search, sorted neighbor lists, Jacobi passes into separate buffers, XSPH viscosity, a box container and `state_hash`. No atomics, no randomness. Tests: `the_same_start_gives_the_same_bits` (30 steps, equal hashes), `a_column_falls_settles_and_stays_inside_the_container` (768 particles, 240 steps, all finite and inside the box, top below 1 m, speed below 3 m/s), `a_settled_pool_keeps_its_density_near_rest` (mean density ratio in 0.5 to 1.6). Full check passed. Limits: the pool settles about 1.6 times denser than rest (walls have no ghost particles and only over-density is pushed apart); artificial pressure is off (`SCORR`, its scale was wrong for these units); results are deterministic for one particle order but not invariant under reordering the array (float sum order). Open: scene format, rigid-body coupling, buoyancy, rendering, GPU port.
  - [x] Step 2, the ECS component: `FluidVolume { settings, fluid }` steps in the `FixedUpdate` chain after CPU physics (`step_fluids`). Test `runtime::tests::fluid_volumes_step_with_the_fixed_tick` (64 particles fall 0.3 m or more in 30 ticks). Full check passed. Limits: runtime only, made by game code; not reflected, not saved in scenes, not in snapshots.
  - [x] Step 3, drawing: `FluidVolume::visual` (a `MeshRenderer`) makes `sync_fluid_visuals` keep one entity per particle, moved each fixed tick, and despawn them when the visual is removed. Test `runtime::tests::fluid_volumes_with_a_visual_own_one_entity_per_particle` (8 particles give 8 renderers; removing the visual gives 0). Full check passed. Limits: one entity per particle, so a few thousand particles is the practical ceiling until fluids get an instanced draw path or a surface mesh; the particle entities carry a `FluidParticle` marker, and the editor Hierarchy and Scene View picking skip them (tests `editor::tests::fluid_particles_are_not_listed_in_the_hierarchy`, and the marker count in the visual test; full check passed with all three feature sets).
  - [x] Step 4, rigid-body coupling and buoyancy: `couple_fluids` runs before `step_fluids`. A dynamic body with a sphere collider gets an upward force of rest density times the sphere volume its overlapping particles fill times gravity, plus drag, and particles inside the sphere are pushed out to its surface. Bodies and volumes go in entity order. Test `runtime::tests::a_light_ball_floats_in_a_fluid_and_a_heavy_one_sinks` (a 7 kg ball of 0.15 m radius floats at y 0.25 in a pool about 0.3 m deep; a 50 kg ball rests on the floor at y 0.145). Full check passed. Box colliders work the same way (rotation and scale respected; particles leave through the nearest face). Test `runtime::tests::a_light_box_floats_in_a_fluid_and_a_heavy_one_sinks` (5 kg floats above a 100 kg box resting on the floor); full check passed. Capsules too (test `a_light_capsule_floats_in_a_fluid_and_a_heavy_one_sinks`; the capsule is a core segment along its local Y with a radius). Limits: sphere, box and capsule colliders only (meshes are skipped); every body checks every particle (O(bodies x particles)); the force is analytic, so the fluid does not push the body sideways and the body's momentum reaches the fluid only through displacement; a body far lighter than the water it displaces is launched hard (that is the physics, but the explicit step is stiff).
  - [x] Step 5, the scene component: `rusting.fluid_block` (`FluidBlock`: spacing, `count_x/y/z`, `container_half_extents`, iterations, viscosity, `visible`) is reflected, listed in `rusting schema`, saved in scenes and registered for snapshots. `spawn_fluid_volumes` turns each block into a `FluidVolume` (box centered on the entity, particles on its floor, spheres with the fallback material when visible). Test `runtime::tests::fluid_blocks_round_trip_through_scenes_and_become_volumes` (a 3x2x4 block round-trips through a scene document and becomes 24 particles in a box whose floor is at y 1.5). Full check passed. Limits: the particles themselves are not in scenes or snapshots, the editor shows no fluid preview until play. (Snapshots now do keep the fluid: see Step 6.)
  - [x] Step 6, review fixes. `couple_fluids` visits bodies and volumes in `SpawnOrder` (entity order only for objects without one), and returns at once with no volume. `FluidParticle(owner)` names the volume that made a particle: removing a volume despawns its particles, and a copied volume makes its own. `FluidVolume` and `FluidParticle` are registered for snapshots, so a restore resumes the same fluid. Cooked scenes are format 8 (`SCENE_FORMAT_VERSION`); v7, v6 and v5 files load through legacy material structs. Tests: `despawning_a_fluid_volume_removes_its_particle_entities`, `a_copied_fluid_volume_gets_its_own_particle_entities`, `fluids_snapshot_and_restore_to_the_same_future`, `version_seven_cooked_scenes_read_materials_without_the_new_fields`. Also fixed from the same review: swapchain image count clamps to the surface maximum (`probe_swapchain_images`); the CCD sweep adds its pad only for grown shapes; `slide` measures step height at the contact position; a `push_bodies: false` player keeps its contacts (crates rest on it, `a_crate_dropped_on_a_player_that_pushes_nothing_rests_on_it`) but shoves nothing; the slow-motion clamp scales with `time_scale` and replay skips it (`update_exact`); `set_scene` refuses the root path; `schema::defaults` is cached and its `mesh_renderer` default comes from `MaterialAsset::default()`; whole numbers compare exactly in scenario `equals` (`whole_numbers_above_the_f32_range_match_exactly`); headless ms per tick excludes scene load; reflection probes and lights sort by entity index; `pointer` listed under scenario steps. Full check: fmt, clippy in all three configurations, `cargo test --workspace` (lib 445, cli 13), gpu-tests (lib 520, cli 13).
  - [x] Material names. `MaterialAsset::name` (reflected, scene-saved; cooked format 9, format 8 loads through `LegacySceneDocumentV8`; glTF import keeps the glTF name). Mesh Renderer combos show names or "Material N"/"Mesh N". Tests: `version_eight_cooked_scenes_read_materials_without_a_name`, `material_names_survive_the_scene_form`. Full check in the same run as the Project Settings item.
  - [x] Fluid surface. `fluid_surface::surface_mesh` splats particles on a grid in index order and extracts the surface by marching tetrahedra (outward faces from the density gradient); `sync_fluid_surfaces` keeps one entity and one mesh per volume and rewrites the mesh each fixed tick (the renderer re-uploads on the asset revision). Tests: `a_block_of_particles_gets_one_closed_outward_surface`, `the_surface_is_reproducible_and_empty_fluid_still_draws`, `a_fluid_surface_is_one_entity_whose_mesh_follows_the_particles`. How it looks on screen is not judged here. Deferred: GPU surface pass, screen-space fluid, shared vertices.
  - [x] Water body and opacity 0. `rusting.water` (`runtime::water`): `sync_water` rewrites a wave grid mesh each fixed tick, `float_in_water` gives buoyancy, drag and a current to dynamic bodies. Tests: `waves_are_reproducible_and_bounded`, `the_mesh_grid_matches_the_rectangle`, `a_water_body_draws_one_surface_floats_a_light_ball_and_carries_it`. The scene shader discards Blend fragments with alpha 0 and no transmission; gpu test `a_blend_material_at_opacity_zero_draws_nothing` (the clear-glass test now uses alpha 0.02). Deferred: rotated water, wave torque on boxes, shore foam, river splines, water material look (no screenshot taken).
  - [x] Editor redesign, part 1. The Project panel is a Project Settings page (`src/editor/project_settings.rs`) built from new `gui_elements::kit` widgets: category rail (Overview, Rendering, Display, Export, Diagnostics), cards, setting rows, switch, segmented control, stat tiles, CPU frame bar. The Console has a framed toolbar, striped rows and level stripes. Opacity below 1 turns an Opaque material into Blend. Add Component is a searchable grouped picker (`inspector/add_component.rs`); Inspector sections are bordered cards. Test `project_settings::tests::every_category_draws_its_cards`; the two editor text tests were updated for the new layout. Full check passed: fmt, clippy in all three configurations, `cargo test --workspace` and with gpu-tests. Not judged by eye: no screenshot was taken, so spacing and colors need a look in the editor.
- [ ] Grid-based or hybrid (FLIP/APIC) option for large water volumes, chosen by the simulation class.
- [ ] Granular material (sand, gravel, rubble) with piling and angle-of-repose behaviour.
- [ ] Fluid surface extraction for rendering through extraction, never by rendering reading solver buffers directly.

### Fracture and destruction

- [ ] Pre-fractured assets authored in the editor (Voronoi and slicing) with connectivity graphs.
- [ ] Runtime fracture from impact energy and stress, deterministic under the seeded tick RNG.
- [ ] Debris lifetime, budget, sleeping, and merge policies. Never drop bodies arbitrarily at the cap.
- [ ] Destruction events consumable by audio, particles, navigation, and gameplay.

### Scale

- [ ] One unified GPU broad phase shared by rigid, deformable, particle, and fluid solvers.
- [ ] Multi-GPU-queue scheduling: async compute for physics overlapping graphics where supported, with a fallback on single-queue devices.
- [ ] GPU simulation LOD: distant or unobserved regions step at reduced rates or freeze, with explicit, deterministic policies.
- [ ] Streaming simulation regions in and out of GPU memory within a device budget.

### Coupling

- [ ] Two-way coupling between rigid, articulated, deformable, fluid, and granular simulation in one scene.
- [ ] Mixed CPU/GPU ownership for coupled objects with defined latency and state age.

### Exit gate

- A demo scene combines cloth, soft bodies, fluid, granular media, and runtime fracture interacting with rigid bodies, with correct events delivered to Rust gameplay.
- Deformable, fluid, and fracture scenarios have headless GPU tests under lavapipe.
- A 1,000,000-body GPU scene runs within the hardware budget recorded for the benchmark machine.
- Fracture is deterministic across runs.

## Milestone 12: Physics tooling, authoring, and evidence

Goal: make physics the best-understood part of the engine — visible, debuggable, authorable, and benchmarked against competitors.

Depends on: Milestones 5, 6, and 10. Deformable and fluid tooling follows Milestone 11.

### Physics debugger

- [ ] Debug draw for shapes, contacts, normals, impulses, joints, islands, sleeping state, broad-phase cells, and GPU ownership.
- [ ] Record physics sessions (from play mode or a running game) and scrub a timeline tick by tick, using Milestone 8 replay.
- [ ] Per-body inspector showing state history, contacts, applied forces, owning solver, sync mode, and readback age.
- [ ] Remote physics debugging of a running game process from the editor.
- [ ] Heatmaps for solver cost, contact density, and overflow by region.

### Authoring tools

- [ ] Collider editing gizmos, automatic collider generation, and convex decomposition (V-HACD-class) for imported meshes.
- [ ] Joint placement and limit-editing gizmos with live preview.
- [ ] Ragdoll creation wizard from a skeleton.
- [ ] Physics material library, collision-layer matrix editor, and force-field visualization.
- [ ] Simulate-in-editor: run physics on selected objects without entering play mode, then keep or discard the result (for placing props naturally).
- [ ] Fracture authoring preview.

### Profiling

- [ ] Physics profiler panel: CPU/GPU solver time per stage, body/contact/constraint counts, command/event/readback bytes, overflow, and synchronization latency.
- [ ] Automatic-allocation diagnostics explaining why each body was placed on CPU or GPU.

### Benchmarks and evidence

- [ ] Standard benchmark scenes: stacking stability, 100K pile, ragdoll count, joint-chain stability, CCD bullets, vehicle stress, cloth, fluid, and fracture.
- [ ] Reference implementations of the same scenes on Jolt, PhysX, Rapier, and Godot Physics, run by the same harness.
- [ ] Publish results with hardware, driver, and settings; regressions against stored baselines fail CI.

### Exit gate

- A physics bug in a recorded session can be located by scrubbing to the tick and inspecting the body in the editor.
- Colliders, joints, ragdolls, and materials can be authored entirely in the editor.
- The comparative benchmark report is generated reproducibly and checked into the repository.

## Milestone 13: Advanced rendering

Goal: reach Godot's Forward+ visual feature set and give users programmable shading.

Depends on: Milestones 3 and 4.

### Global illumination and reflections

- [x] Reflection probes with box projection and blending.
  `rusting.reflection_probe` holds the half size of a world-aligned box
  around its object. When a probe is added or changes, a temporary renderer
  draws six 90-degree faces from its center, so the main renderer's caches,
  GPU physics and occlusion history stay untouched. The faces are copied into
  one mipmapped array image with six layers per probe. Up to four probes are
  used (`MAX_REFLECTION_PROBES`). The fragment shader's `surroundings` casts
  the reflection direction to the box wall, looks up the face toward that
  point, fades each probe in over the outer tenth of its box, and blends the
  result over the environment. Rough surfaces use lower mips. Limits
  (`ponytail:` in code): probes are static captures, faces are tone mapped
  with Linear so captured light clamps at 1, and moving a probe rebuilds the
  capture pipelines.
  Evidence: `reflection_probe_shows_the_scene_behind_the_camera` renders a
  mirror floor under an unlit red ceiling that covers the -X half and is
  behind the camera. With no probe, both sides are dark. With a probe, the
  -X side is red and the +X side is not. Flipping the face lookup's x axis
  fails the test. Full check: fmt, clippy three ways, `cargo test
  --workspace` (474 passed, 76 ignored), and GPU tests (546 passed).
  Editor: a selected probe shows a handle at each box face; dragging one
  sets that axis of `extents` (the box stays centered), as one Undo step.
  Evidence: unit test
  `dragging_scene_handles_resizes_a_probe_and_thins_fog`.
- [x] Refractive glass: `transmission`, `ior` and `thickness` on materials, sampling a copy of the opaque scene color so glass bends and tints what is behind it. The scene render pass must split after the opaque draws in every variant (MSAA, occlusion culling).
  Materials carry `transmission`, `ior` and `thickness`. A frame with a
  transmissive draw ends the scene pass after the opaque draws, copies HDR
  into a mipmapped `scene_color` image (`FramePass::SceneColor`), and draws
  the blended list in the late pass (`FramePass::Transparent`). The shader
  refracts the view ray once at the entry face, offsets it by `thickness`,
  and samples the copy at mip `roughness * last_mip`, tinted by base color.
  Limits: no exit-face refraction, and blended objects behind glass are not
  in the copy. glTF `KHR_materials_transmission` import needs a new `gltf`
  crate feature and is not done. Evidence: GPU test
  `transmissive_glass_tints_the_refracted_backdrop` under frustum and
  occlusion culling (fails with the refraction path turned off: 255 255
  255); `transitions_list_every_layout_change_between_passes`.
- [ ] Baked lightmaps with a GPU lightmapper and light probes for dynamic objects.
- [ ] One real-time GI technique (probe-based DDGI or voxel/SDF GI) with a quality-profile fallback to ambient probes.
- [ ] Sky system: procedural physical sky, HDRI skies, and sky-derived ambient/specular.
  Partial: `EnvironmentMap { texture, intensity }` (`rusting.environment_map`)
  loads an equirectangular image and replaces the `SkyLight` hemisphere as the
  ambient source. Diffuse samples the second-smallest mip along the normal;
  specular samples mip `roughness * last_mip` along the reflection. Textures
  now get a linear-blit mip chain, and samplers now allow every mip (the old
  default clamped the LOD to 0). Known ceilings: 8-bit images only, so no
  `.hdr`/EXR and no light brighter than `intensity`; box-filtered mips instead
  of GGX prefiltering; the map is not drawn as the background. Evidence: GPU
  test `smooth_metal_reflects_the_environment_map` (a smooth metal slab
  reflects the red upper half from above and the blue lower half from below;
  fully rough, it shows a blend of both). It failed before the pole clamp
  (a repeating sampler blended the top row with the bottom one) and before
  the mip chain (rough metal stayed pure red). Unit test
  `environment_maps_resolve_their_texture_and_reach_the_render_world`.

### Screen-space and post effects

- [ ] SSAO, SSR, and screen-space indirect lighting, each independently toggled by quality profile.
  Partial (SSR): frames with a PBR batch under roughness 0.5
  (`SSR_MAX_ROUGHNESS`) split like refraction frames. The `SceneColor` pass
  also copies opaque depth into depth pyramid mip 0; the `Transparent` pass
  redraws the glossy batches with a reflection overlay pipeline (fragment
  specialization `PASS = 2`, `LessOrEqual` depth, additive blend) that
  marches the reflected ray through that depth and adds the difference
  between the traced color and the environment reflection. Blended and
  transmissive surfaces trace inline. The weight fades at the screen edge,
  at the end of the march, and toward the roughness limit. `Eco` skips it.
  Limits: linear march, no Hi-Z; glossy surfaces do not reflect each other
  or blended objects. Evidence: GPU test
  `mirror_floor_reflects_the_scene_in_screen_space` (red wall in the floor
  on High, plain gray hemisphere on Eco); pass table
  `transitions_list_every_layout_change_between_passes`.
  Partial (SSAO): `rusting.ambient_occlusion` (radius, intensity) turns it
  on. A `DepthPrepass` pass draws the early-culled (else GPU-culled)
  batches into scene depth with alpha-mask discard; the `AmbientOcclusion`
  compute pass traces 16 rotated hemisphere samples per pixel against that
  depth, then a 4x4 depth-aware blur writes an R32F factor that set 2
  binding 6 multiplies into ambient, sky and environment light. The main
  pass then loads the prepassed depth. Off on `Eco`, in probe captures and
  in non-Lit views. Limits: objects that come into view on an
  occlusion-culling frame miss the prepass for one frame; no temporal
  accumulation. Evidence: GPU tests
  `ambient_occlusion_darkens_surfaces_next_to_occluders` (floor next to a
  box darker than open floor, unchanged with the component removed) and
  `atmosphere_effects_combine_with_every_scene_path` (frustum and
  occlusion culling, MSAA off and 4x, opaque and glossy); pass table
  `transitions_list_every_layout_change_between_passes`. Screen-space
  indirect lighting is not started.
- [ ] Bloom/glow, depth of field, motion blur, auto-exposure, color grading LUTs, vignette, and chromatic aberration.
  Partial (bloom): `rusting.bloom` (intensity, threshold, spread). After the
  transparent pass, the `Bloom` compute pass prefilters the HDR target to
  half resolution (13-tap with Karis weights against fireflies, soft knee
  at the threshold), downsamples it through up to 7 mips, and upsamples
  back with a 3x3 tent mixed by spread. Tone mapping adds mip 0 times the
  intensity before the curve. Frames with bloom split the main pass like
  refraction frames. Off in probe captures and non-Lit views. Evidence: GPU
  tests `bloom_spreads_bright_light_into_its_surroundings` (dark pixels
  beside an emissive quad brighten only with bloom on) and
  `atmosphere_effects_combine_with_every_scene_path`. The other effects
  are not started.
- [ ] Temporal anti-aliasing and FXAA; optional upscaling (FSR-class) behind capability checks.
- [ ] Volumetric fog with light scattering and fog volumes.
  Partial (height fog): `rusting.fog` (color, density, height,
  height_falloff, sun_scatter, sky_affect). `src/shaders/fog.glsl`
  integrates exponential height fog along the camera ray in closed form,
  with in-scattering toward the shadow-casting (or first) directional
  light. Opaque, blended and transmissive surfaces apply it in the
  fragment shader; a full-screen sky pass at the far plane fades the
  background. Evidence: GPU test `fog_hides_distant_surfaces_and_the_sky`
  (distant red wall and the sky turn fog-colored, near surfaces keep their
  color) and `atmosphere_effects_combine_with_every_scene_path`; unit tests
  `atmosphere_settings_come_from_the_lowest_entity` and
  `atmosphere_settings_go_on_one_environment_object`. Not started: froxel
  volumetrics, light shafts through shadows, local fog volumes.
  Editor: the Scene view's viewport menu has Scene Effects toggles that
  leave fog, bloom or ambient occlusion out of the Scene view only (Play
  and games draw them all), and a selected Fog draws squares at its
  full-density height and its 1/e height. Dragging the first square sets
  `height`; dragging the second sets `height_falloff`; each drag is one
  Undo step. Evidence: unit tests
  `scene_view_shading_and_effects_apply_only_in_the_scene_workspace`,
  `fog_height_squares_mark_full_density_and_the_thinned_height` and
  `fog_squares_are_grabbed_and_dragged_to_a_height`; the fog,
  bloom and ambient occlusion GPU tests render each effect switched off.

### Lighting and shadows

- [ ] Cascaded shadow maps for directional lights; shadows for point and spot lights.
- [ ] Clustered lighting for many point and spot lights.
- [ ] Area-light approximation and light cookies.

### Geometry and effects

- [ ] GPU particle system reusing the physics compute infrastructure, with collision against physics shapes, attractors, sub-emitters, and trails.
- [ ] Particle emission driven by physics events: impacts, fracture, debris settling.
- [ ] Decals, including decals that survive or are invalidated by fracture.
- [ ] Instanced multi-mesh rendering and foliage scattering with wind driven by physics force fields.
- [ ] Heightmap terrain rendering with texture splatting and LOD.
- [ ] Automatic mesh LOD generation at import.
- [ ] Rendering of deformables, fluid surfaces, and debris produced by Milestone 11.
- [ ] Effect budget and quality-profile scaling for particles and decals.

### Programmable shading

- [ ] A versioned engine shader language (GLSL-based with engine includes) for surface, unlit, sky, particle, fog, and post-process shaders, with stable uniforms/built-ins.
- [ ] A visual shader graph that compiles to the same language.
- [ ] Shader hot reload with error reporting in the editor.
- [ ] Material instancing with per-instance parameter overrides.

### Frame structure

- [ ] A render graph that schedules passes, transient resources, and barriers automatically.
- [ ] Frame pacing that holds a stable presentation cadence under variable GPU load.
- [ ] Measure and report end-to-end input latency.

### Exit gate

- Golden images cover probes, GI fallback, SSAO/SSR, fog, bloom, tone mapping, particles, decals, and custom shaders.
- A user-written surface shader and a visual-graph shader render correctly without engine changes.
- Every effect degrades gracefully on the low-end baseline defined in Milestone 3.

## Milestone 14: Animation

Goal: match Godot's animation system and make physics-driven animation a first-class feature.

Depends on: Milestones 2, 9 (reflection), and 10 (ragdolls).

### Core animation

- [ ] Skeletal animation with GPU skinning and blend shapes (morph targets).
- [x] glTF skin, morph target, and animation import, completing the Milestone 2 deferral. Done in the Effects and animation track, items 6 and 7 (node clips, `rusting.skin` with `.rskin`, `rusting.morph` with `.rmorph` and weight channels). Tests: `add_model_keeps_gltf_skin_and_bends_the_mesh`, `add_model_keeps_gltf_morph_weights_and_reshapes_every_part`, `every_primitive_of_a_skinned_node_gets_the_skin`.
- [ ] Property animation of any reflected field (the equivalent of `AnimationPlayer`), including method-call and event tracks.
- [ ] Tweens for scripted interpolation with easing curves.

### Blending and control

- [~] Animation state machine with transitions, conditions, and sync groups.
  - Done (2026-10-04): `rusting.animation` gains `parameters` (name to number) and `transitions` (`AnimationTransition { from, to, parameter, compare: Above|Below|Equal, value, at_end, fade }`). Each tick without a command, `next_state` takes the first transition in list order out of the playing clip (empty `from` = any clip except `to`) whose test holds and crossfades. `at_end` waits for one full play; an ended `Once` clip can leave, a stopped one cannot. Game code: `GameScene::set_animation_parameter`; a Field track on `/parameters/<name>` keys them. Reflected, so the Inspector edits both lists. Schema example and summary, `docs/animation.md` "State machine", SKILL.md, project AGENTS template.
  - Test: `runtime::animation::tests::transitions_follow_parameters_and_clip_ends` (idle to run on speed, any to jump on Equal, jump back to idle at its end, then to run again, run to idle when speed drops).
  - Full check: fmt and clippy ×3 clean. `cargo test --workspace` passes (566 lib tests); with gpu-tests 651 lib, 26 CLI, 5 vertical slice, 17 doc tests pass.
  - Not done: sync groups; trigger parameters that reset after use (set back to 0 from game code for now); a state-machine graph editor (see Milestone 15 animation editor item).
- [x] 1D/2D blend spaces, additive layers, and bone masks.
  - Done (2026-10-04): 1D blend spaces. An `AnimationClip` with `blend: Vec<BlendPoint { clip, at }>` and `blend_parameter` mixes the two point clips around the parameter value, each stretched to the blend clip's length (1 s when 0) so cycles stay in step; outside the range the end clip plays alone. Its own tracks and markers play on top. It is an ordinary clip, so play, crossfade and transitions treat it as a state. Crossfades now share the same `mix` helper. The Timeline preview passes the whole `Animation` so it shows the mix. Reflected (Inspector), schema example and summary, `docs/animation.md` "Blend spaces".
  - Test: `runtime::animation::tests::blend_spaces_mix_neighbors_in_step` (half walk/half run at the midpoint, run stretched to 1 s, clamped at both ends, unsorted points).
  - Full check: fmt and clippy ×3 clean. `cargo test --workspace` passes (567 lib tests); with gpu-tests 652 lib, 26 CLI, 5 vertical slice, 17 doc tests pass.
  - Done (2026-10-04): 2D blend spaces. `blend_parameter_y` set on a blend clip makes it 2D; each `BlendPoint` also has `at_y`. Weights come from gradient band interpolation (`band_weights`): a point's weight is the minimum over every other point of `1 - (p - a)·(b - a)/|b - a|²`, clamped to 0..1, then all are normalized, so a point plays alone on its spot and past the outer points the nearest edge clips play; points need not form a grid. The 1D and 2D paths now share one weighted mix loop (running average via `mix`, zero weights skipped). Reflected, schema example and summary, `docs/animation.md` "Blend spaces". Test: `two_d_blend_spaces_play_points_alone_and_mix_between` (each point alone on its spot, half way between idle and run with strafe at 0, past run on its axis, an interior mix). Full check: fmt, clippy in three configurations, `cargo test --workspace` 574 lib, 25 CLI, 5 vertical slice, 17 doc; gpu-tests (`--test-threads=1`) 659 lib, 26 CLI, 5 vertical slice, 17 doc.
  - Done (2026-10-04): layers and bone masks. `Animation::layers: Vec<AnimationLayer { clip, weight, weight_parameter, additive, mask }>` play clips over the state machine, each on its own clock from scene start (seconds kept per layer in `AnimationPlayer`), in list order. Override layers lerp matching tracks by weight and write tracks the state lacks at full value. Additive layers add the clip's change from its own first frame onto tracks the state keys: positions, Euler rotations and colors add, scales multiply, `Orientation` applies `ref⁻¹·layer` (hemisphere fixed, lerped from identity by weight) in the joint's own frame. `mask` keeps writes whose target equals a path or lies under it (`Body` covers `Body/Arm`). `weight_parameter` reads the weight from a parameter so game code fades a layer with `set_animation_parameter`. Reflected (Inspector), schema example and summary, `docs/animation.md` "Layers", SKILL.md and the project AGENTS template. Test: `layers_override_masked_tracks_and_add_motion` (mask keeps the root, override at weight 0.5, additive position, additive orientation in the joint frame against the sampled key, weight parameter 0 turns it off). Full check: fmt, clippy in three configurations, `cargo test --workspace` 575 lib, 25 CLI, 5 vertical slice, 17 doc; gpu-tests (`--test-threads=1`) 660 lib, 26 CLI, 5 vertical slice, 17 doc.
  - Limits: a blend point that is itself a blend space plays only its own tracks; layer markers send no events; the Timeline preview does not show layers.
- [x] Root motion with an explicit policy that never feeds back into deterministic simulation unless routed through the physics command bridge.
  - Done (2026-10-04): `Animation::root_motion: RootMotion { Off, InPlace, Transform, Velocity }` and `root_bone` (path of the bone whose `Position` track carries the motion; empty is the object's own track). With any policy but `Off`, `extract_root_motion` pins that track's x and z at the state clip's first frame (height still plays) and measures the horizontal distance the state clip moved it this tick, across `Loop` wraps (end of cycle plus start of next) and blended with a fading clip by fade weight; the result lands in `AnimationPlayer::root_delta` (this tick) and `root_motion` (accumulated). `InPlace` only collects it: game code calls `GameScene::take_root_motion(name)` and moves the character itself, e.g. through `move_character`. `Transform` adds it to the object's position, turned by its rotation (`sim_math::rotation_from_euler`) and scaled, for objects without physics bodies. `Velocity` is the only policy that reaches the simulation, and only through the command bridge: it pushes `GpuBodyCommand::SetVelocity { linear: delta / dt (world), angular: 0 }` for the object's `PhysicsId`. Reflected (Inspector), schema example and summary, `docs/animation.md` "Root motion" and the game-code list, SKILL.md and the project AGENTS template. Test: `runtime::animation::tests::root_motion_moves_the_object_by_policy_and_holds_the_root_bone` (11 ticks of a 2 m/s loop with one wrap move a quarter-turned object 2.2 m along world -z; `InPlace` leaves it put and collects 2.2 m; the root bone keeps x and z but bobs; `Velocity` pushes one -z 2 m/s command and does not move the transform). Full check: fmt, clippy in three configurations, `cargo test --workspace` 576 lib, 25 CLI, 5 vertical slice, 17 doc; gpu-tests (`--test-threads=1`) 661 lib, 26 CLI, 5 vertical slice, 17 doc.
  - Limits: yaw (turning in place) and vertical motion are not extracted; `Velocity` zeroes angular velocity; `Transform` on an object with a CPU body is overwritten by physics; layers do not contribute root motion.
- [~] Two-bone, FABRIK, look-at, and foot-placement IK.
  - Done (2026-10-04): `rusting.ik` (`src/runtime/ik.rs`) with `LookAt` (local `forward` axis at the target, shortest arc) and `TwoBone` (analytic law-of-cosines solve of parent and grandparent, out-of-reach targets clamped, straight chains bent toward the pole or a fallback side, optional `pole` twist about root→target). `weight` nlerps local rotations; 0 skips the solve. Solved at the end of `step_animations` in scene ID then entity order, from world poses rebuilt from the `Transform` chain with `sim_math` (GlobalTransform is stale in the fixed step); trig through `sim_math::atan2`/`sqrt`, no new schedule system. Registered in scene file, reflect (Inspector entity pickers for target and pole), snapshot, schema, placement help, `docs.rs`; `docs/animation.md` "Inverse kinematics".
  - Tests: `runtime::ik::tests::two_bone_reaches_the_target_bent_or_straight` (3 targets × bent/straight chain, within 1e-3), `two_bone_bends_toward_the_pole_and_clamps_far_targets`, `look_at_points_forward_at_the_target_with_weight`.
  - Full check: fmt and clippy ×3 clean. `cargo test --workspace` passes (570 lib tests); with gpu-tests 655 lib, 26 CLI, 5 vertical slice, 17 doc tests pass (a first gpu-tests run had 6 scene_renderer tests fail inside vulkano from GPU contention; they pass alone and on rerun).
  - Done (2026-10-04, foot placement): `IkKind::Foot` with `reach` (default 0.5 m). It raycasts the CPU `PhysicsWorld` (last step's colliders) straight down from `reach` above the character's floor (nearest ancestor with `Animation`, else the top ancestor), skipping colliders in the character's hierarchy, and two-bone solves to the animated foot position shifted by the ground height minus the floor height. Full check: fmt and clippy ×3 clean; `cargo test --workspace` 571 lib; gpu-tests 656 lib, 26 CLI, 5 vertical slice, 17 doc. Test: `runtime::cpu_physics::tests::foot_ik_stands_on_the_ground_and_skips_its_own_colliders` (foot over a 0.2 m step with a collider on the foot itself lands at y 0.2).
  - Done (2026-10-04, hips and slopes): `Foot` turns the foot by the shortest arc from world up to the ground normal, applied to its pre-solve world rotation, and lowers the hips (the thigh's parent, unless that is the character) by the largest drop any of its feet needs to reach (leg length × 0.999), times weight; drops are computed from ground targets found before any hip moves. New runtime-only `IkPose { before, after }` (snapshot-registered, not saved) records each joint IK writes; next tick a joint whose transform still equals `after` goes back to `before` first, so weights below 1, tilts and hip drops never pile up on joints no clip drives. Test: `runtime::cpu_physics::tests::foot_ik_lowers_the_hips_tilts_to_the_slope_and_does_not_pile_up` (0.3 rad slope 0.28 m below the floor: foot on the hit point, foot up axis on the normal within 1e-4, hips below 0.75 m, tick 1 and tick 3 poses equal); `look_at_points_forward_at_the_target_with_weight` now checks weight 0 returns the head to its rest rotation. Full check: fmt and clippy ×3 clean; `cargo test --workspace` 572 lib; gpu-tests 657 lib, 26 CLI, 5 vertical slice, 17 doc.
  - Done (2026-10-04, FABRIK): `IkKind::Chain` with `joints` (default 3). Collects the tip and up to `joints` ancestors, pins the top joint, straightens toward out-of-reach targets, otherwise runs at most 16 FABRIK passes (stops when the tip is within 1e-4 of the chain length), then turns each joint top first by the shortest arc from its current to its new child direction, blended by weight. Fixed pass count and `sim_math` normalization keep it deterministic. Test: `runtime::ik::tests::chain_bends_every_joint_to_reach_or_straightens_toward_far_targets` (5-joint chain reaches (1, 0.8, 0.3) within 1e-3 with the top joint fixed; a far target gives a straight 2 m chain). Full check: fmt and clippy ×3 clean; `cargo test --workspace` 573 lib; gpu-tests 658 lib (run with `--test-threads=1`: a parallel run hung once and then had 7 scene_renderer tests fail inside vulkano while a game held 4.8 GB of the GPU; all pass single-threaded), 26 CLI, 5 vertical slice, 17 doc.
  - Not done: pole or joint limits for `Chain`, up-vector / twist limits for LookAt, joint angle limits, an editor gizmo that draws the target and pole lines.
- [x] Skeleton retargeting between humanoid rigs.
  - Done (2026-10-04): `runtime::retarget_clip(world, from, clip, to)` (`src/runtime/retarget.rs`) and `rusting scene retarget <scene> <from> <clip> <to> [--dry-run]`, which saves the result on the target's `rusting.animation` through the scene patch path (replacing a same-name clip; error code `RETARGET_FAILED`). Bones pair by the new `Animation::humanoid` map (`HumanoidBone { bone, path }`) or by bone name lowercased with rig prefixes (`mixamorig:`) and separators dropped; target bones without a map are found breadth first. Rest poses come from the scene transforms. Each `Rotation`/`Orientation` key becomes an `Orientation` key that keeps its turn from the source rest in the rig frame (`q_t = P_t⁻¹ · P_s q_s W_s⁻¹ · W_t`), with hemisphere continuity; hips `Position` keys are turned into the target parent frame and scaled by hip height ratio; tracks on the object itself are kept. Reflected, schema example and operation, `docs/animation.md` "Retargeting", SKILL.md, AGENTS template; editor button deferred in `docs/editor-overhaul.md`. Tests: `runtime::retarget::tests::clips_move_a_differently_built_rig_alike` (rest maps to the target's quarter-turned rest; a 0.7 rad rig-y swing gives the same rig-frame swing within 1e-5; hips on a 2× taller rig move 2× as far; unknown bones and clips handled) and CLI `scene_retarget_saves_a_clip_on_the_other_rig` (dry run, write, missing clip). Full check: fmt, clippy ×3 clean; `cargo test --workspace` 577 lib, 26 CLI, 5 vertical slice, 17 doc; gpu-tests (`--test-threads=1`) 662 lib, 27 CLI, 5 vertical slice, 17 doc.
  - Limits: bone length differences other than hip height are not compensated; position and scale keys of other bones are dropped; blend spaces go point clip by point clip; no twist or swing limits.

### Physics-driven animation

- [x] Ragdoll handoff between animation and physics on physics events, with blend-back.
  - Done (2026-10-04): `rusting.ragdoll` (`Ragdoll { bones: Vec<RagdollBone>, hit_speed, recover_after, blend_time }`, `src/runtime/ragdoll.rs`; reflected, in `rusting schema`, captured by snapshots with `RagdollState`) runs inside `step_animations` after IK, so the FixedUpdate system list and replay hashes are unchanged. Going limp (game code `GameScene::set_ragdoll(name, true)`, or a non-sensor contact or continuous-collision impact on the character's own colliders, descendants or parts at `hit_speed` m/s or more) spawns one capsule CPU body per bone, as root entities, with the velocity of the previous animated pose, each jointed to the nearest ancestor bone's body by `joint` (default `ConeTwist`) with `frame` putting X along the bone; the character's collider is taken off. While limp, bone locals follow the bodies. Recovery (after `recover_after` s, or `set_ragdoll(name, false)`) moves the root's x/z under the hips, despawns the bodies, restores the collider and blends each bone (lerp/nlerp) from the limp pose to the animated pose, or to the pose before going limp for bones no clip drives, over `blend_time`. Physics additions: `Contact::speed` (closing speed before the solve) and `PhysicsWorld::impacts()` (fast bodies stopped by CCD at fixed or kinematic colliders, which never show a contact speed). Roots and bones are visited in (SceneId, entity) and depth order. `GameScene::is_limp`; `docs/animation.md` "Ragdolls", SKILL.md, AGENTS template; editor bone generator and gizmos deferred in `docs/editor-overhaul.md`. Test: `runtime::tests::ragdolls_go_limp_on_a_hit_fall_and_blend_back_deterministically` (a 12 m/s ball knocks a two-bone character down within 30 ticks; spine body jointed to hips body; collider removed; the hips body falls below 0.8 m and the bone follows; a second run gives a bit-identical bone pose; after 1 s it stands where the hips lay within 0.2 m, bodies gone, collider back, and blends to the exact rest pose within 1e-5; `command` toggles limp and blending). Full check: fmt, clippy ×3 clean; `cargo test --workspace` 578 lib, 26 CLI, 5 vertical slice, 17 doc; gpu-tests (`--test-threads=1`) 663 lib, 27 CLI, 5 vertical slice, 17 doc.
  - Limits: CPU physics only; getting up keeps height and facing (no get-up clips yet); bone bodies may touch non-adjacent bone bodies; removing `rusting.ragdoll` while limp leaves its bodies behind.
- [x] Active ragdolls: powered articulations that track animation targets while reacting physically to hits.
  - Done (2026-10-04): `Ragdoll::muscle` (Hz; reflected, schema example) makes a `rusting.ragdoll` active. Its capsule bodies spawn on the first tick (phase `RagdollPhase::Active`). Each tick `drive` (`src/runtime/ragdoll.rs`) takes the animated bone locals as targets (bones the module wrote last tick fall back to the pose before the handoff). It turns each body toward its target relative to its parent body, and holds the top body to the animated hips, with an implicit critically damped spring `ω' = (ω + k·dt·e) / (1 + c·dt + k·dt²)` (k = (2πf)², c = 2·2πf), so it is stable at any step. Driven bodies are kept awake (`PhysicsWorld::keep_awake`). Bones then follow the bodies. A hit at `hit_speed` or `set_ragdoll(name, true)` drops the muscles (`Limp`). Recovery moves the root under the hips and ramps the muscles from 0 to full over `blend_time`. Setting `muscle` to 0 while active goes limp and then back to plain animation. Game code: `GameScene::set_ragdoll_muscle`. Docs: `docs/animation.md` "Active ragdolls", schema summary, the SKILL.md animation API row, and the AGENTS template. Test: `runtime::tests::active_ragdolls_hold_the_pose_shrug_off_pushes_and_get_back_up` (10 Hz muscles hold a two-bone character upright: spine tilt < 0.05 rad, hips within 0.05 m; a 30 rad/s shove tilts the spine past 0.1 rad and it returns within 1 s; a 0.5 rad keyed spine bend is tracked within 0.08 rad; limp, it falls below 0.5 m; recovered, it stands back up within 2 s; muscle 0 goes limp, then removes the bodies and blends). Full check: fmt, clippy ×3 clean; `cargo test --workspace` 579 lib, 26 CLI, 5 vertical slice, 17 doc; gpu-tests (`--test-threads=1`) 664 lib, 27 CLI, 5 vertical slice, 17 doc.
  - Deviation and limits: the muscles drive the maximal-joint bodies' velocities, not reduced-coordinate articulation motors. Muscles push only their own body, with no reaction on the parent, and the hips hold is a "hand of god" force, so the character cannot lose balance on its own; only `hit_speed` or game code drops it. Steady sag is about 0.05 rad on a loaded 10 Hz joint. CPU physics only.
- [ ] Procedural secondary motion (jiggle bones, tails, hair) through the physics solver rather than a separate spring system.
- [ ] Cloth attached to skinned characters (from Milestone 11).

### Exit gate

- An imported, skinned character blends through a state machine, uses IK on uneven ground, and hands off to an active ragdoll when hit.
- A property track animates a custom reflected component.
- Animation playback results are identical across runs.

## Milestone 15: Audio

Goal: provide Godot-equivalent audio, driven naturally by physics.

Depends on: Milestone 1. Physics-driven audio depends on Milestones 5 and 10.

First slice built (owner approved `kira`, 2026-10-01): `src/runtime/audio.rs` (`AudioQueue`, `AudioCommand`, `SoundId`; presentation only, not in snapshots) and `src/audio_output.rs` (kira playback, feature `audio`, on with `window`). `GameScene::play_sound`, `play_sound_looped`, `stop_sound`, `stop_all_sounds`, `set_master_volume`, `sounds_requested`; `SoundEvent`s from `rusting.sound_cue` become play requests. WAV, Ogg, MP3 and FLAC decode through kira. No device means silent, with one stderr line. Real speaker output is not verified here. Sound events are routed by the runner after each update (`route_sound_events`), not by a schedule system: adding a system changed replay hashes (`replays_reproduce_recorded_hashes_and_find_changed_input` diverged at tick 1). Headless runs keep the newest 1,024 commands and count only game-code requests. Tests: `requests_get_distinct_ids_and_drain_once`, `sound_requests_are_queued_counted_and_drained`, and the cue check in `landing_fires_one_sound_and_a_seeded_burst_that_expires`. Full check passed under lavapipe: fmt, clippy in three configurations, `cargo test --workspace` and with `--features gpu-tests` (exit 0, 16 result lines each). Not done: buses, effects, spatial, streaming, hot-plug, voice limits, offline render.

- [ ] Audio device management with hot-plug, output selection, and a no-device fallback.
- [ ] Mixer with buses, sends, volume/mute/solo, and bus effects (reverb, delay, EQ, compressor, limiter, filters).
- [ ] WAV, OGG Vorbis, and FLAC import; streaming playback for long assets.
- [ ] 3D spatial audio with attenuation curves, doppler, and an occlusion approximation using physics raycasts.
- [ ] Reverb zones tied to physics volumes.
- [ ] Event-driven playback triggered by gameplay events and by GPU physics events.
- [ ] Physics-driven impact, scrape, and roll sounds parameterized by contact impulse, relative velocity, and physics material.
- [ ] Voice limiting and priority so large destruction events do not exhaust the mixer.
- [ ] Offline render path for deterministic audio tests.
- [ ] Audio state is presentation only and never affects simulation or replay hashes.

### Exit gate

- An offline-rendered mix matches a reference buffer within tolerance.
- A physics scene with thousands of contacts produces voice-limited impact audio without dropouts.
- Disabling all audio does not change replay hashes.

## Milestone 16: Runtime UI, text, and localization

Goal: give games a runtime UI toolkit equivalent to Godot's Control system.

Depends on: Milestones 1, 3, and 9.

- [ ] Runtime UI layer independent of the editor's egui usage, rendered through extraction.
- [ ] Layout containers: box, grid, margin, scroll, split, tab, and anchors/offsets for free placement.
- [ ] Widgets: label, button, toggle, slider, text input, dropdown, list, tree, progress bar, image, and panel.
- [ ] Text shaping with Unicode, bidirectional text, font fallback, SDF font rendering, and rich text markup.
- [ ] Themes and styles editable as data assets.
- [ ] Focus navigation for keyboard and gamepad, and input routing between UI and gameplay.
- [ ] Data-bound HUD elements and world-space indicators.
- [ ] Scaling across resolutions, aspect ratios, and DPI settings.
- [ ] Localization: translation tables, pluralization, locale switching at runtime, and extraction of translatable strings.
- [ ] Accessibility: screen-reader metadata, scalable text, and color-blind-safe defaults.
- [ ] Input remapping UI component built on the action map.
- [ ] Decouple input sampling rate from the simulation tick without introducing nondeterminism.

### Exit gate

- A menu, settings screen with rebinding, and HUD can be built from data assets and work with mouse, keyboard, and gamepad.
- Golden images cover layout, text shaping (including right-to-left), and theming.
- Switching locale at runtime updates every translated string.

## Milestone 17: 2D engine

Goal: make RustingEngine a complete 2D engine, not a 3D engine with a flat camera.

Depends on: Milestones 3, 4, and 10 (2D physics).

- [ ] 2D renderer with sprites, sprite batching, z-ordering, and canvas layers.
- [ ] Sprite sheets and sprite animation.
- [ ] Tilemaps with multiple layers, autotiling rules, tile collision shapes, and tile navigation data.
- [ ] 2D lights, shadows from occluder polygons, and normal-mapped sprites.
- [ ] 2D GPU particles with physics-event emission.
- [ ] 2D camera with smoothing, limits, and pixel-perfect mode.
- [ ] 2D hybrid GPU physics showcases (thousands of physics sprites with events to Rust gameplay).
- [ ] Mixing 2D layers over or inside 3D scenes.

### Exit gate

- A 2D platformer template with tilemap, lights, particles, and physics is playable and editable in the editor.
- Golden images cover sprites, tilemaps, and 2D lighting.

## Milestone 18: Navigation

Goal: provide Godot-equivalent navigation for 2D and 3D, with runtime updates that respond to physics.

Depends on: Milestones 5 and 10.

- [ ] Navigation mesh baking from collision geometry for 3D, and from tilemaps/polygons for 2D.
- [ ] Runtime incremental re-baking of changed tiles within a per-tick budget.
- [ ] Path queries, off-mesh links (jumps, ladders, doors), and area costs.
- [ ] Navigation agents with path following and velocity-obstacle avoidance.
- [ ] Obstacles derived from physics bodies, including moving and sleeping bodies.
- [ ] Navigation debug visualization and profiler counters.
- [ ] Deterministic query and avoidance results for deterministic projects.

### Exit gate

- Agents route around physics bodies that are knocked into their path at runtime.
- Re-baking stays within its per-tick budget and reports overruns.
- Navigation results are identical across runs.

## Milestone 19: Networking and multiplayer

Goal: provide Godot-equivalent high-level multiplayer, plus deterministic rollback that Godot cannot offer.

Depends on: Milestones 8 and 9. The Milestone 8 result decides whether rollback is offered across vendors.

- [ ] Transport abstraction with UDP (reliable and unreliable channels), WebSocket, and loopback for tests.
- [ ] Versioned wire protocol with compatibility rejection at connect time.
- [ ] Remote procedure calls on entities with authority checks and reliability modes.
- [ ] Replicated spawning and component synchronization, configured through reflection, with delta compression and quantization.
- [ ] Client-server and listen-server topologies; headless dedicated-server builds.
- [ ] Client-side prediction and reconciliation for player-controlled entities.
- [ ] Deterministic rollback netcode for physics-heavy games, using Milestone 8 snapshots and input streams.
- [ ] Physics-aware replication: replicate commands and events instead of full body state wherever determinism allows.
- [ ] Network simulation (latency, jitter, loss) for testing, and a network profiler.
- [ ] HTTP client for services and downloads.

### Exit gate

- A physics sandbox runs with four clients under simulated 150 ms latency and 2% loss without visible desync.
- Rollback reproduces identical simulation state on every peer in a deterministic test.
- RPC, spawning, and synchronization have loopback tests.

## Milestone 20: Editor parity

Goal: bring the editor to Godot-level completeness on top of the Milestone 6 foundation.

Depends on: Milestone 6, and the milestone that owns each edited subsystem.

### Workflow

- [ ] Remote scene tree and inspector for a running game process.
- [ ] Runtime debugger panel: errors with stack traces, monitors (FPS, memory, physics, rendering counters), and custom monitors.
- [ ] Unified profiler with CPU spans, GPU pass timings, physics stages, network traffic, and memory.
- [ ] Project settings editor, input map editor, and per-asset import settings dock with re-import.
- [ ] Multiple scene tabs and multiple viewports (split views, orthographic views).
- [ ] 2D viewport with snapping and pixel grid.
- [ ] Search across the project: files, entities, components, and settings.
- [ ] Version-control integration showing changed files and scene diffs.

### Specialized editors

- [ ] Animation editor with timeline, curves, onion skinning, and state-machine graph.
- [ ] Shader editor and visual shader graph editor.
- [ ] Tilemap and tile-set editor.
- [ ] Grid/modular level editing and constructive solid geometry blocking tools.
- [ ] Particle system editor with live preview.
- [ ] Theme and UI layout editor.
- [ ] Audio bus editor.
- [ ] Navigation baking controls and visualization.

### Extensibility

- [ ] Editor plugin API for custom panels, inspectors, gizmos, importers, and tools, loaded from project crates.
- [ ] Asset library browser for installing templates, plugins, and assets into a project.
- [ ] In-editor API documentation for engine and reflected project types.

### Exit gate

- Every subsystem milestone's authoring tasks can be completed without leaving the editor.
- A third-party editor plugin adds a panel, a custom inspector, and an importer without engine changes.
- Editor state, layouts, and settings persist across sessions.

## Milestone 21: Platforms, export, and distribution

Goal: ship games to every supported platform from the editor.

Depends on: Milestones 3 and 6.

- [ ] Export presets per platform with feature flags, asset filters, and encryption of packed data.
- [ ] Packed asset archives with compression and streaming.
- [ ] Texture compression per platform (BCn desktop, ASTC/ETC2 mobile) through KTX2.
- [ ] Windows and Linux release polish: installers/archives, icons, and code-signing hooks.
- [ ] Steam Deck/Proton verification.
- [ ] macOS through MoltenVK, with capability fallbacks for missing Vulkan features.
- [ ] Android export with touch input, lifecycle handling, and mobile quality profiles.
- [ ] Platform services abstraction for save paths, achievements, and storefront SDK hooks.
- [ ] Crash reporting with symbolicated stack traces.
- [ ] Record web, iOS, and console export as post-1.0 decisions with the blocking technical reasons.

### Exit gate

- A demo project exports and runs from the editor on Windows, Linux, and macOS.
- An Android build runs the physics sandbox template on a mid-range device.
- Crash reports from exported builds resolve to source locations.

## Milestone 22: Documentation, samples, and ecosystem

Goal: make the engine learnable and adoptable the way Godot is.

Depends on: the features being documented. Documentation for each feature lands with that feature; this milestone covers the structure and the gaps.

- [ ] User manual covering every subsystem, with a dedicated physics guide that explains ownership, synchronization, determinism, and solver choice.
- [ ] Generated API reference for engine crates and reflected types.
- [ ] Step-by-step tutorials: first 3D game, first 2D game, physics sandbox, multiplayer physics game.
- [ ] Demo projects per feature area, with a physics showcase gallery (destruction, fluids, cloth, vehicles, ragdolls, million-body scenes).
- [ ] Migration guide for Godot users mapping nodes, signals, resources, and scripts to RustingEngine concepts.
- [ ] Contribution guide, plugin authoring guide, and release notes process.

### Exit gate

- A new user can build and export the first-game tutorial from a clean install using only the documentation.
- Every demo project builds and runs in CI.

## Milestone 23: Engine 1.0 release

Goal: declare Godot-class parity with physics leadership, backed by evidence.

Depends on: Milestones 9-22.

- [ ] Every row in the Godot parity map has a passing exit gate.
- [ ] Every physics pillar has its test or benchmark evidence checked in.
- [ ] Reference games built entirely with the engine: a 3D action game, a 2D platformer, a vehicle/physics sandbox, and a multiplayer physics game.
- [ ] Public API stability policy, semantic versioning, and a deprecation process.
- [ ] Performance baselines recorded for the low-end, mid-range, and high-end reference machines.

### Exit gate

- All reference games are playable from a clean checkout, editable in the editor, and exportable to every supported platform.
- The comparative physics benchmark report shows where RustingEngine leads, and any area where it does not is documented with a follow-up item.

## LLM-native track

Milestones L1-L10 make RustingEngine the best engine to build games with an LLM coding agent. They run in parallel with Milestones 9-23 and have priority while the owner's current focus (agent-built games) holds.

The starting point already exists: the window-free `rusting` CLI, the `schema_version` 1 result envelope, `rusting schema`, atomic `scene patch` batches with dry runs and revision checks, input-driven scenarios, offscreen `capture` with pixel picking, the generated project `AGENTS.md`, `skills/rusting-game/SKILL.md`, and twelve sample games whose gaps were logged and fixed. `missingFeatures.md` holds the earlier backlog; its open sections are absorbed below.

### LLM-native pillars

"Best for LLM coding" is defined by the following pillars. Like the physics pillars, each is owned by a milestone and proven by a test or by the agent benchmark (Milestone L9), not asserted.

| Pillar | What it means | Owner |
| --- | --- | --- |
| Legibility | An agent learns the whole engine surface from the installed version, in budgeted text, with no web search and no guessing from training data | L1 |
| Generation-friendly API | Game code an LLM writes compiles on the first or second try: no lifetime puzzles, no nested borrows, one obvious way to do each thing | L2 |
| Observability | Every mutation has a way to see its effect as text, structured state, an annotated image, or an event trace | L3 |
| Fast loop | Edit to verified result in seconds, through a warm daemon and a no-rebuild logic lane | L4, L8 |
| Verifiability | Behaviour is proven by scenarios, invariants, fuzzing and bots, and a scenario's strength is itself checked | L5 |
| Quality floor | A generated game looks, sounds, and feels finished by default; lints catch what a model cannot see | L6 |
| Scale | Agents stay effective on a large project and several agents can work on one project at once | L10 |
| Safety | Agent actions are scoped, journaled, reviewable and reversible by a human | L4, L8 |
| Evidence | A published, repeatable agent benchmark compares RustingEngine against Godot and others on identical requests | L9 |

### Design rules for the track

- One truth, several interfaces: CLI, daemon, MCP adapter, editor, and Rust API call the same operations and validation code.
- Small outputs by default: summaries, filters, pagination, and stable references; details on demand. Every command that can print a lot takes `--limit` and `--fields`.
- Every error names the next command or edit that fixes it.
- Text formats that diff well: sorted keys, stable ordering, one value per line where it matters for merges.
- No model lock-in: projects and tools stay fully useful with no AI service, account, or network connection. Nothing in the engine calls a model.

## Milestone L1: Machine-readable engine surface

Goal: an agent can learn everything it needs about the installed engine version from the engine itself, within a stated text budget.

Depends on: Milestone 7 agent loop (done).

### Diagnostics

- [x] Stable diagnostic codes for every validation, patch, scenario, import, and build error, kept in one registry with a uniqueness test. The existing symbolic codes (`SCENE_CONFLICT`, `ASSET_NO_LICENSE`) are kept rather than renumbered: they were already stable in the JSON envelope and a model reads a name more reliably than a number. Evidence: `src/diagnostics.rs` registers all 52 codes; `diagnostics::tests::every_emitted_code_is_registered_and_every_registered_code_is_emitted` scans `src/` for code literals and fails on an unregistered code or an entry nothing emits; `codes_are_sorted_unique_and_fully_documented`.
- [x] `rusting explain <code>` prints the cause, the fix, and an example command or patch for each code; `rusting explain` lists every code; a mistyped code is a usage error that lists codes with the same prefix; every human-readable failure ends with `Run \`rusting explain CODE\` for the cause and a fix.` The project `AGENTS.md` and the skill file mention it. Evidence: `rusting::tests::explain_describes_a_code_and_suggests_similar_ones_for_a_typo`, `diagnostics::tests::json_patch_examples_parse_as_patches`; full check green (fmt, clippy default / no-default-features / gpu-tests, `cargo test --workspace` with and without gpu-tests, lib 541 with gpu-tests). Limit: command examples starting with `rusting` are not yet parsed by a test.
- [x] Each diagnostic in `--json` output carries the file, the JSON pointer or line, the entity ID and name, and a severity. `Diagnostic` has `line`, `column`, `scene_location` (JSON pointer into `file`) and `entity` (`id`, `name`), filled wherever the tools know them: scene JSON errors give line and column; duplicate IDs and names, missing parents, parent cycles and component value errors point at `/entities/N` (or `/entities/N/components/NAME`) and the object, through the shared `runtime::error_object`; patch operation errors point at `/operations/N` of the patch file; a patch that breaks a scene rule (`PATCH_INVALID`) names the object; missing assets and determinism errors name the object; build errors give the line. The human output prints the same location. Evidence: `tests/cli.rs` `scene_and_patch_errors_point_at_the_line_object_and_operation` and `missing_asset_reference_fails_validation_with_location`; `reflect::tests` asserts `ReflectError::object` on a failed scene load; full check green (lib 541 with gpu-tests). Limit: `PATCH_INVALID` for a default or an unknown scene setting has only its message, and a patch's broken object has no pointer because it is not on disk yet.
- [x] Machine-applicable fixes: when a fix is certain (a misspelled field, a missing default section, a wrong unit), the diagnostic carries a ready `scene patch` operation, and `rusting fix` applies all certain fixes with a dry-run diff first. Done for misspelled keys, the only certain case the tools detect today. `validate` now reports every scene key that loading drops as `SCENE_UNKNOWN_FIELD` (no hits on the 16 scenes in the repository); a misspelled required field (`SCENE_JSON` "missing field") is matched to the key that misspells it. A key within two edits of exactly one unset known key, written once in the file, gets `fix: {"op": "rename_key", "path", "to"}`. The fix is a rename in the file text rather than a `scene patch` operation, because a patch rewrites the whole scene and would delete the other unknown keys the user still has to look at; and a file whose required field is misspelled does not load, so a patch cannot apply to it. `rusting fix [root] [--dry-run]` applies fixes in rounds until none are left, since the next missing field shows only after the first is fixed; `--dry-run` lists the first round and writes nothing. Evidence: `tests/cli.rs` `fix_renames_misspelled_scene_keys_in_place_and_keeps_the_rest`, `scene_patch::tests::dropped_fields_suggest_only_one_close_unset_key`; full check green (lib 542 with gpu-tests). Not done: a missing section is already filled from defaults by `scene patch`; wrong units have no detector yet; registered component strings are checked on load (`SCENE_COMPONENT_FIELD`) and get no fix.

### Version-matched reference

- [x] `rusting docs` serves the manual, the Rust API index, the schema catalog, and sample snippets offline from the installed version, with `search <query>` and `show <item>`. Done for the manual, tutorials, both agent guides, every command (from the schema catalog) and every diagnostic code; pages are embedded with `include_str!`, so the text always matches the binary. `search` needs every word and ranks id and title hits first; `show` accepts `kind/name` or an unambiguous bare name and cuts at a line to fit `--budget`. Evidence: `docs::tests` (5: unique ids with title and summary, bare-name lookup, search ranking, budget cuts) and `tests/cli.rs` `docs_lists_searches_shows_and_briefs_within_a_token_budget`; full check green (lib 547 with gpu-tests). Not done: the Rust API index and sample snippets are not items yet (next two L1 items).
- [x] `rusting docs --brief [--budget N]` prints a compact `llms.txt`-style overview sized to a token budget, with pointers to the detailed entries. The default budget is 2000 tokens (four characters each); items are listed in manual, tutorial, guide, command, code order and the tail is replaced by "N more items; `rusting docs` lists them all." Evidence: `docs::tests::budget_cuts_at_a_line_and_the_brief_fits_it` checks 300, 1000 and 4000 tokens and that a huge budget lists all; the CLI test above.
- [x] A generated Rust API index for the gameplay surface: `rusting docs` lists an `api/Type::method` item for every public method of `GameScene`, `GameObject`, `CubeSpawn` and `SphereSpawn`, with signature and doc comment (units, `# Errors`), read from `project_runner.rs` at build time via `include_str!`, so it cannot drift. Evidence: `docs::tests::api_index_lists_documented_gameplay_methods` (more than 30 items, every one with a summary); full check green (lib 548 with gpu-tests; `headless_device` tests flake under parallel load and pass alone or on rerun). Not done: the prelude free items and registered components are not indexed, and examples in doc comments are not yet run (next-but-one L1 item).
- [x] JSON Schema files (draft 2020-12) for scenes, patches, scenarios, `project.json` and `.rmeta`, emitted by `rusting schema --json-schema` (`schema::json_schemas`). Scene entity sections and registered components come from the catalog tables; patch operations list all seven ops with required fields. Evidence: `schema::tests::json_schemas_describe_every_key_the_engine_writes` (top-level keys match what serde writes for scene, patch, scenario and project), `json_schema_patch_operations_match_the_parser`, `json_schema_entity_lists_every_section_and_component`, `tests/cli.rs` `schema_json_schema_prints_a_schema_per_file_kind`; full check green (lib 551 with gpu-tests). Not done: `$schema` is not written into engine files (a scene with an unknown key reports `SCENE_UNKNOWN_FIELD`, and the cooked format is binary, so it needs a format decision); component and section values are not described below the top level (the catalog lists fields); `.rmeta` keys are not cross-checked against serde.
- [x] (Partly) Every `rusting ...` command in the schema catalog, the manual, tutorials, the skill file and the project `AGENTS.md` is extracted and checked on every test run: it must name a real operation (longest match, so `scene patch` beats `scene`) and use only flags that operation's usage lists. A stale command fails the build. Evidence: `docs::tests::documented_commands_name_real_operations_and_flags` (over 40 commands; a mutation check with a misspelled command and flag in the skill file failed it with both named); full check green (lib 552 with gpu-tests). Not done: the commands are checked, not executed, because most need a project fixture; doc-comment Rust examples in the API index are not compiled; JSON patch examples in the code registry are already parsed by `diagnostics::tests::json_patch_examples_parse_as_patches`.

### Budgeted output

- [x] `--limit N`, `--fields a,b` and `--summary` on the listing commands (`project inspect`, `scene inspect`, `scene query`, `asset list`, `preset list`, `explain`; `docs` keeps its own `--budget`). They apply to every array directly under `data`: `--fields` keeps those keys of each object, `--limit` keeps the first N and reports the cut count under `data.omitted`, `--summary` replaces each array with `{"count": N}`. A bad value is a `CLI_USAGE` error. Evidence: `cli::shape_tests::shape_limits_projects_and_summarizes_listings` and `tests/cli.rs` `listing_flags_cut_output_and_every_json_result_reports_its_size`; full check green (lib 553 with gpu-tests). Limit: arrays nested deeper than `data.<key>` are not cut, and `docs search` results are not shaped because `data.matches` already has its own limit.
- [x] Each `--json` result carries `size: {data_bytes, data_tokens}` (bytes of `data`, tokens at four characters), so an agent can decide whether to ask for more. Evidence: the CLI test above compares the size of a cut and a full result. The envelope field is additive; `schema_version` stays 1.

### Exit gate

- A fresh agent with no prior RustingEngine knowledge and no network access builds a sample-sized game using only `rusting docs`, `rusting schema`, and `rusting explain`.
- Every error the engine can emit has a code, an explanation, and a tested example.

## Milestone L2: Generation-friendly gameplay API

Goal: gameplay code written by an LLM compiles and behaves as intended on the first or second attempt.

Depends on: Milestone L1 for the API index; Milestone 9 for reflection and prefabs.

- [ ] An API audit rule, enforced by a test over the public gameplay surface: no public gameplay function returns a borrow that blocks another `GameScene` call, and no gameplay type needs an explicit lifetime. Handles are `Copy` IDs. Partly done: `docs::tests::gameplay_methods_do_not_return_new_borrows` fails on any new public `GameScene`, `GameObject`, `CubeSpawn` or `SphereSpawn` method returning a borrow (the `&mut Self` chaining builder on `GameObject` is exempt), and fails when a known violation is fixed but left on the list, so the list only shrinks. Four violations remain and are listed in the test: `GameScene::object`, `try_object` (return `GameObject<'_>`), `counter` (returns `Mut<'_, Counter>`) and `world` (the ECS escape hatch). Left open: replacing `GameObject<'_>` with a `Copy` ID handle rewrites every sample and the skill file, so it needs the owner's go-ahead. Full check green (lib 554 with gpu-tests).
- [ ] One obvious way per task: overlapping helpers are merged or deprecated, and the API index marks the preferred call. Partly done: a `Preferred:` paragraph in a doc comment names the call to use instead, and `docs::tests::preferred_calls_exist_in_the_index` fails when a named call is not in the API index. Marked so far: `GameScene::counter` (prefer `counter_value`, `add_to_counter`, `set_counter`) and `GameScene::world`. Left open: nothing is merged or deprecated, because choosing the winner among `move_x`/`move_by`, `object`/`try_object` and similar pairs changes the public API and every sample, which is the owner's call. Full check green (lib 555 with gpu-tests).
- [ ] Typed, fallible lookups everywhere a generated name can be wrong (`try_object` style), with a diagnostic that lists the nearest existing names. Partly done: `GameScene::object` and `watch_gpu_object` panics now end with ``; did you mean `Crate`?`` listing up to three scene object names within a case-insensitive edit distance of half the name length (at least two). Evidence: `project_runner::tests::a_missing_object_name_panic_lists_the_nearest_names`; full check green (lib 556 with gpu-tests). Left open: the other name-taking methods (`counter`, `set_color`, `spawn_copy`'s template and so on) keep their current behaviour, most return `Option` or `bool` already and have no message to add to; the hint is a panic message, not a diagnostic code.
- [x] Engine-aware compiler hints: when `cargo` fails, `rusting check` (and `run`, `test`, `export`, which share the build step) adds one `RUST_ENGINE_HINT` diagnostic (severity `hint`, with file and line) per pattern found in rustc's output: scene borrow conflicts (E0499, E0502), unknown `GameScene` or `GameObject` methods (E0599, points at `rusting docs search`), a bad `rusting_engine` import (E0432, points at the prelude), and `f64` or `[f32; 3]` mismatches (E0308). The table is `cli::ENGINE_HINTS`; add a row per new pattern. Evidence: `cli::hint_tests::rustc_errors_get_the_engine_pattern_that_fixes_them` (line and file taken from the error, one hint per pattern, unrelated errors ignored); the code is in the registry, so `diagnostics::tests` covers explain. Full check green (lib 557 with gpu-tests). Limit: matching is on rustc's short message text, so a rustc wording change silently drops a hint; the hints are not yet tried on a real failing project in a test.
- [x] Scaffolding commands: `rusting add system <name>`, `rusting add component <name>`, and `rusting add scenario <name>` write a registered, documented stub plus a failing scenario that proves it runs.
  (Partly.) `add scenario` and `add system` are done: `add system` appends a documented `todo!` stub to `src/main.rs` and writes a failing scenario; the stub is not called until `update` calls it. `add component` is not done: it needs a way for game code to register a scene component, which is an owner decision. Evidence: `add_scaffolds_a_stub_and_a_failing_scenario` in `tests/cli.rs` (stub, duplicate refusal, bad name, scenario shape); full AGENTS.md check passes (gpu-tests needed a rerun after a known stall). The scenario being red on `rusting test` is not run here.
- [ ] (Blocked on owner: changes the public API and every sample.) Typed units at the API boundary (metres, seconds, radians, degrees) where a mix-up is a common generated bug, without making ordinary code verbose.
- [ ] (Blocked: no deprecation exists yet to migrate; `rusting fix` already applies scene key renames.) Deprecations carry machine-applicable migrations, and `rusting migrate` upgrades scenes and game code across engine versions, because models are trained on older APIs.
- [ ] Measure edit-to-diagnostic time for game code and keep it within a stated budget with a shared build cache across projects.

### Exit gate

- In the agent benchmark (Milestone L9), the median task needs at most one corrective build.
- An engine upgrade across one minor version is applied to every sample by `rusting migrate` with no hand edits.

## Milestone L3: Observability for models

Goal: an agent can see what the game did, as text or as an image built for a vision model, without guessing.

Depends on: Milestone 7 capture and scenarios; Milestone 8 replay for tick-exact inspection.

### Structured state

- [x] `rusting inspect <project> --tick N` pauses a headless run at a tick and reports selected entities, contacts, recent events, the active camera, the HUD/UI tree, and the freshness of every GPU-owned value.
  (Partly.) `rusting inspect [root] --tick N [--entity NAME]...` builds the game, runs it headless to tick N (through a generated `build/inspect-tick.json` scenario of `log` steps) and reports each entity's full scene form after that tick; with no `--entity`, every named entity of the main scene. Evidence: ran it on a generated project (Cube at tick 5 returned its transform and mesh); `inspect_tick_reports_entity_state_after_that_tick` in `tests/cli.rs` is `#[ignore]` (real cargo build, 103 s) and passes with `--ignored`; full AGENTS.md check green. Left open: contacts, recent events, the active camera, the HUD tree and GPU-value freshness; it pauses by running to the tick, not by attaching to a live game (L4 debug protocol). Unnamed entities are not listed.
- [x] A per-tick event trace (collisions, triggers, counter changes, spawns, despawns, inputs, scene loads, sounds, errors) written as JSON lines, with filters by entity, class, and event kind.
  (Partly.) Every `ScenarioReport` (so `rusting test --json`) has a `trace`: `input`, `set` and `collision` entries with tick and detail (collisions by persistent ID), in tick order, capped at 1000. Evidence: `scenario::tests::the_trace_lists_inputs_sets_and_collisions_by_tick`; full AGENTS.md check green. Left open: triggers, counter changes, spawns, despawns, scene loads, sounds and errors; JSON-lines output; filters by entity, class and kind (use `--fields` or `jq` for now).
- [x] `rusting diff <a> <b>` compares two scenes or two ticks semantically: entities added, removed, and changed, by ID and name, with field-level values.
  (Partly.) Scene files only: `rusting diff <scene-a> <scene-b>` matches entities by ID and reports added, removed and changed entities (with JSON path, before and after for each field) and scene-level field changes. Comparing two ticks needs `rusting inspect --tick`, which is not built. Evidence: `diff_reports_added_removed_and_changed_entities_by_id` in `tests/cli.rs`; full AGENTS.md check green. Limit: a rename plus ID change shows as remove plus add.
- [x] A text view of 2D, tile, and grid scenes (a character map with a legend) so a model without vision can check a layout.
  (Partly.) `rusting scene map <scene>` prints every `rusting.tile_map` as character rows plus a legend, and draws other named entities that sit over a map's cells as letters with name, ID, column and row. Evidence: `scene_map_draws_tile_rows_and_places_entities_on_them` in `tests/cli.rs`; full AGENTS.md check green. Left open: sprite-only 2D scenes without a tile map, and scene state at a tick (needs `inspect --tick`). Entities placed over a map ignore rotation and size.

### Images built for models

- [x] Annotated captures: optional overlays with entity names, short IDs, bounding boxes, collider outlines, velocity arrows, and a legend, so a model can refer to what it sees by ID.
  (Partly.) A scenario with `"annotate": true` makes every `capture` step also write `<name>.annotated.png` (a box and a 4-hex-digit short ID per mesh entity) and `<name>.annotated.json` (the legend: short ID, entity name, full ID, pixel box). Evidence: ran it on a generated project under lavapipe and opened the PNG (the Cube's box and `4033` label sit on the cube; the legend lists the same ID); `annotate::tests::annotations_box_a_visible_mesh_around_the_frame_center` (projection, no GPU) and `draw_outlines_the_box_and_writes_the_short_id`; full AGENTS.md check green. Left open: collider outlines, velocity arrows, names drawn on the image (the font has only hex digits, names are in the legend), occlusion (hidden entities still get a box), and a `rusting capture --annotate` flag for scene captures.
- [x] Contact sheets: one image with frames from several ticks in a grid, labelled by tick, so one image shows motion.
  A scenario with `"contact_sheet": "shots/sheet.png"` writes one grid image of every `capture` frame, each tile 320 px wide with its tick in a corner label (the frames are still saved on their own too). Evidence: ran a 3-capture scenario on a generated project under lavapipe and opened the PNG (tiles labelled 1, 3, 6, empty slot black); `annotate::tests::a_contact_sheet_tiles_frames_in_a_grid_with_tick_labels`; full AGENTS.md check green. The columns and tile width are fixed (square-ish grid, 320 px); a sheet is written only when at least one capture ran.
- [x] Region picking: `--pick-rect` returns every entity ID that covers a rectangle and its pixel share.
  (Partly.) `rusting capture ... --pick-rect X,Y,W,H` (repeatable) adds `pick_rects` to the result: per rectangle, every entity ID and name that is the nearest hit, with its share, plus the `none` share that hits nothing (`CameraView::pick_rect`). Evidence: `capture_without_vulkan_still_reports_camera_and_picks_per_tick` (rectangle over the cube lists it with a share above 0; sky rectangle lists nothing); full AGENTS.md check green. Open limits: shares are estimates from at most 64x64 sampled pixel centers using bounds picking (not silhouettes); only the nearest hit counts, so occluded entities are not listed.
- [x] Scenario checks on screen presence: an entity is on screen, inside a screen rectangle, covers at least a pixel share, or is occluded. These replace most golden images in agent-written tests.
  (Partly.) New scenario step `expect_screen` with `on_screen`, `occluded`, `inside` (box within fractions of the frame) and `min_share` (share of the frame the entity is the nearest hit for); works with `until`/`within`, needs no Vulkan, and failures report the box, `box_share` and `frame_share`. Evidence: `scenario::tests::screen_checks_see_a_mesh_in_front_of_a_camera_and_behind_a_wall` (visible wall passes on_screen/inside/min_share; a mesh behind the wall is occluded and not on_screen; an off-frame mesh is not on screen; wrong expectations fail with steps 3 and 4); schema and docs tests; full AGENTS.md check green. Open limits: bounds picking sampled at 64x64 (a tiny or thin visible sliver can read as occluded); `inside` uses the box clipped to the frame; only mesh entities count; not run in a real game process.

### Budgets

- [x] Performance reports in stable JSON (CPU/GPU time per phase, draws, dispatches, upload and readback bytes, asset memory), with budget assertions in scenarios and environment metadata in every result.
  (Partly.) Every scenario report in `rusting test --json` now has `perf`: `tick_ms_mean`, `tick_ms_p95`, `tick_ms_max` (wall clock per whole tick), `render` (draws, triangles, visible instances of the last captured frame, null without a capture) and `environment` (engine version, OS, arch, debug or release, device and driver when a frame rendered). A scenario `budgets` object (`max_tick_ms`, `mean_tick_ms`, `p95_tick_ms`, `max_draws`, `max_triangles`) fails the run with a `budget:` step. Evidence: `scenario::tests::perf_is_reported_and_budgets_fail_the_run`; `a_capturing_run_reports_render_counters_and_draw_budgets` (`--features gpu-tests`, lavapipe: draws above 0, device named, `max_draws: 0` fails); schema test; full AGENTS.md check green. Open: CPU/GPU time per phase, dispatches, upload and readback bytes, asset memory, GPU timings; `environment` is only in scenario results, not every CLI result; timing budgets are machine-dependent so real-hardware numbers are unverified here.

### Exit gate

- Each benchmark task in Milestone L9 can be completed by a text-only model, using text views, traces and diffs instead of images.
- A vision model identifies the entity behind any visible object in an annotated capture by ID.

## Milestone L4: Live session and editor bridge

Goal: an agent works against a warm, running engine instead of paying cold-start costs, and a human sees and controls what it does.

Depends on: Milestone L1; Milestone 6 editor and snapshot undo.

- [x] `rusting serve`: a local daemon that keeps a project loaded, the build warm, and shaders compiled, and speaks JSON-RPC over stdio. Every CLI operation is available through it with the same result envelope.
  (Partly.) `rusting serve` reads one JSON-RPC 2.0 request per line on stdin (`method` is the command words, `params` the remaining arguments as strings) and writes one response per line whose `result` is the same envelope as `--json` (including `size`, `--limit`, `--fields`, `--summary`); `shutdown` or end of input stops it; standard error codes -32700, -32600, -32601, -32602, and -32603 (a panicking operation does not kill the daemon). Evidence: `serve_answers_json_rpc_lines_with_the_cli_envelope` in `tests/cli.rs` (matches the cold CLI result, unknown method, bad params, parse error, nothing answered after `shutdown`); schema/docs tests; full AGENTS.md check green. Open: the daemon does not yet keep a project loaded, the build warm or shaders compiled (each request does the same work as the CLI, minus the process start); the 10x speed gate was not measured; operations still use their own output paths, so a long-running one blocks the daemon.
- [x] (Partly) Debug protocol over the daemon: attach to a running game, pause, step N ticks, set values, query, capture, and resume.
  Evidence: `rusting debug [root]` cooks, builds and runs the game with `RUSTING_DEBUG_SESSION=1`; `src/debug_session.rs` reads JSON lines `{"id","cmd"}` (step, get, set, press, release, capture, tick, quit) and the game stays paused between commands, so `step` is resume. Tests: `the_game_is_paused_between_commands_and_steps_exactly`, ignored real-build `debug_session_steps_and_inspects_a_running_game` (passes with `--ignored`). Full check green: fmt, clippy x3, test plain and `--features gpu-tests`.
  Open: `capture` in a session is not verified against a real game; `rusting serve` does not relay to `rusting debug`.
- [x] (Partly) MCP adapter over the same daemon (stdio, project-root scoped, read-only and mutating tools marked), carried from `missingFeatures.md` section 9. It must give the same results as the CLI.
  Evidence: `rusting mcp [root]` speaks MCP over stdio (`initialize`, `ping`, `tools/list`, `tools/call`); one tool per operation (underscored names, `serve`/`debug`/`mcp` excluded) with `readOnlyHint` from `schema::READ_ONLY`; `tools/call` runs through `serve_request`, so the text result is the `--json` envelope and `isError` mirrors `ok`. Scoped by `set_current_dir(root)`; absolute paths and `..` arguments are refused (-32602). Test: `mcp_lists_tools_and_calls_them_like_the_cli`. Full check green: fmt, clippy x3, test plain and `--features gpu-tests`.
  Open: no CLI-versus-MCP identity test over many operations (gate at the L10 "identical results" item); root scoping is argument-checked, not a sandbox, so a spawned build can still read elsewhere.
- [x] (Partly) Live editor bridge: agent edits to an open scene go through the editor's snapshot undo, show a diff, and highlight the affected entities. An external write never silently replaces unsaved GUI work; conflicts are reported.
  Evidence: the editor watches the open scene file (`reload_external_scene_change`). A clean scene reloads behind one Undo snapshot; the status message now carries "N added, N changed, N removed" (`outside_change`) and the added and changed entities become the selection (`highlight_scene_ids`). A scene with unsaved edits keeps them, reports the conflict, and Save refuses once. Test: `outside_scene_patch_reloads_under_undo_or_conflicts_with_unsaved_edits` (asserts the counts and one highlighted entity). Full check green: fmt, clippy x3, test plain and `--features gpu-tests`.
  Open: file-watch only, no socket, so the editor learns of a write on its next poll; the diff is a count, not a per-field view (the Agent panel item adds that with accept and reject); highlight replaces the user's selection.
- [ ] Editor Agent panel, in the Blender area style: the journal of agent operations, pending diffs with accept and reject, and the last scenario results. Add its design to `docs/editor-overhaul.md`.
  Progress: design written (`docs/editor-overhaul.md`, "Agent panel design"). Journal slice built: `EditorPanel::Agent` area (`src/editor/agent_panel.rs`), an `AgentJournal` resource fed by `reload_external_scene_change` (path, summary, changed IDs), newest first, click selects the entry's entities, Clear button. Verified: unit test `outside_scene_patch_reloads_under_undo_or_conflicts_with_unsaved_edits` asserts one journal entry holding the patched ID; `every_editor_panel_draws_even_when_optional_resources_are_missing` draws the new panel. 2026-10-01, lavapipe: fmt clean; clippy plain, `--no-default-features`, `--features gpu-tests` clean; `cargo test --workspace` exit 0, 16 result lines, 0 failed; same with `--features gpu-tests`. Not done: Pending tab with per-field Accept/Reject, Results tab, Pause toggle. Journal is session-only (not saved) and the panel is not checked visually. Item stays open.
- [x] (Partly) Every result reports whether a change touched disk, the editor buffer, or the running game.
  Evidence: the `--json` envelope (CLI, `serve`, `mcp`) carries `touched {disk, editor, game}` from `schema::touched`; `disk` is true only for `WRITES_PROJECT` commands that succeed and are not `--dry-run`. `rusting debug` replies carry it with `game` true after step, set, press, release, tick. Tests: `touched_reports_project_writes_but_not_reads_dry_runs_or_failures`, `the_game_is_paused_between_commands_and_steps_exactly`.
  Open: `editor` is always false from the CLI (the open editor reloads on its next poll); no per-command list of which files changed.
  Verification note: the checks recorded for the debug-protocol, MCP and editor-bridge items above counted partial output of runs that a `timeout` cut off, so they were not a full pass. Re-run on 2026-10-01 after fixing the `mcp` missing-root exit code (1, not 2): fmt, clippy default, `cargo test --workspace` and with `--features gpu-tests` all exit 0 with 16 `test result` lines and 0 failures. The real GPU driver was wedged, so both runs used lavapipe (`VK_DRIVER_FILES=/usr/share/vulkan/icd.d/lvp_icd.json`). Clippy `--no-default-features` and `--features gpu-tests` re-run clean after the change.

### Exit gate

- A patch, test, and capture round trip through the daemon is at least ten times faster than the cold CLI on the same machine (hardware gate).
- An agent and a person edit the same open scene; every conflict is reported and every agent change is undoable from the editor.

## Milestone L5: Behaviour verification toolkit

Goal: an agent can prove a game works, and prove that its proof is strong enough.

Depends on: Milestone 7 scenarios; Milestone 8 replay and state hashes.

- [x] (Partly) Declared invariants: a `rusting.invariant` component or scenario section (the player is never below the floor, no NaN positions, a counter never exceeds its target) checked every tick in `run`, `test`, and fuzz runs.
  Evidence: top-level `invariants` list in a scenario; each entry has the `expect` fields plus `finite` (no NaN or infinite number; JSON reads those back as `null`). Checked after every tick, reported as `invariant N: ...` at the first failing tick, once each with `keep_going`. A missing entity or path passes unless `exists` is set. `rusting schema scenario` and `rusting explain` list it. Test `invariants_are_checked_every_tick_and_report_the_first_bad_tick` covers holding, broken, despawned-entity, empty-invariant and null-number cases. 2026-10-01, lavapipe: fmt clean; clippy plain, `--no-default-features`, `--features gpu-tests` clean; `cargo test --workspace` exit 0, 16 result lines, 0 failed; same with `--features gpu-tests`. Open: checked in `rusting test` scenarios only, not in plain `rusting run`; no `rusting.invariant` component form; the scene document is rebuilt per invariant per tick (slow with many); fuzz runs do not exist yet, so the fuzz half of the item waits on the fuzz item.
- [ ] Seeded input fuzzing: `rusting fuzz` presses random named actions for N ticks over many seeds and reports the first seed and tick that breaks an invariant, as a ready scenario file.
- [ ] Explorer bot: a goal-seeking player that tries to reach pickups, sensors, and win conditions and reports unreachable goals and stuck states.
- [ ] Scenario strength check: `rusting test --without <system|component>` runs a scenario with a named system or component switched off and expects it to fail. This turns the manual "fails with the code removed" evidence into a command.
- [ ] Coverage: which systems, components, entities, and events a scenario exercised, so an agent sees untested mechanics.
- [ ] Record a human play session in the editor or the game window as a scenario file, so a human can hand an agent a bug as a test.
- [ ] Divergence bisect: given two builds or two scene revisions, report the first tick and entity where state hashes differ.

### Exit gate

- Every sample game has invariants, a fuzz run with no failures over 100 seeds, and scenarios that pass the strength check.
- A seeded regression inserted into a sample is found by `rusting fuzz` or the explorer bot without a hand-written scenario.

## Milestone L6: Quality floor and game feel

Goal: a game made by an agent looks, sounds, and feels finished by default, and the engine reports what a model cannot perceive.

Depends on: Milestones 15 and 16 for audio and UI breadth; the existing game-feel kit and art-direction presets.

- [ ] `rusting lint`: presentation and design checks with diagnostic codes, including HUD text contrast and off-screen text, camera inside geometry, zero-intensity or clipped lighting, collider and mesh mismatch, scale sanity (a 40 m player), pickups or goals with no reachable path, events with no audio or visual feedback, and games with no win or lose state.
- [ ] Controller feel metrics in physical units (jump apex height and time, time to top speed, stopping distance, air control), reported by `rusting inspect` and compared with documented genre ranges.
- [ ] More data-driven juice: screen shake, hit-stop, squash and stretch, camera trauma, flashes, and particle and sound presets, each a registered component with schema entries.
- [ ] A built-in placeholder asset pack: CC0 meshes, sprites, fonts, and sounds in one consistent style, so a first build does not look like grey cubes. Its provenance goes through the existing `.rmeta` records.
- [ ] A deterministic procedural sound-effect generator (sfxr style: jump, coin, hit, explosion, from parameters and a seed) with no new dependency.
- [ ] Accessibility checks from `missingFeatures.md` P2: contrast, text size, focus order, colour-only status, and missing captions, with locations and captures.

### Exit gate

- Every sample game passes `rusting lint` with no warnings.
- In a blind review of benchmark outputs, games made with the defaults are rated as more finished than the same requests built without them.

## Milestone L7: Recipes, templates, and knowledge

Goal: common game shapes start from tested, editable recipes rather than from an empty scene.

Depends on: Milestones L1 and L2.

- [ ] `rusting recipe list` and `rusting recipe apply <name>`: named mechanics (double jump, dash, health and damage, checkpoints, inventory, wave spawner, turret, day timer, pause menu) that write ordinary components, systems, and scenarios into a project.
- [ ] A template matrix for `rusting new --template`: platformer, top-down action, first-person, third-person, puzzle grid, racing, twin-stick shooter, tower defense, roguelike, card game, rhythm game, and physics sandbox. Each passes its scenarios in CI.
- [ ] The skill file and project `AGENTS.md` are generated from the schema, the API index, and a pitfall catalog, and checked for staleness in CI.
- [ ] A pitfall catalog grown from every logged gap in the "Current focus" list, each with its diagnostic code and fix.
- [ ] A task cookbook of runnable snippets ("make an enemy follow the player", "add a level select"), each tested.

### Exit gate

- Every template and recipe passes its scenarios and `rusting lint` in CI.
- Every closed gap in this roadmap either has a diagnostic that catches it or a pitfall entry.

## Milestone L8: Fast logic lane and safe execution

Goal: gameplay logic can change in under a second without a Cargo rebuild, inside a sandbox, and every agent action can be reviewed and reverted.

Depends on: Milestone 9 WASM scripting host and Rust hot reload.

- [ ] A script-first lane: gameplay systems written against the reflected ECS API in WASM (compiled from Rust) swap in a running game without restart, with the same `GameScene` surface as native code where the sandbox allows.
- [ ] A promotion path from a WASM script to native Rust with no behaviour change, checked by state hashes.
- [ ] Permission scopes for agent-facing commands: project-root confinement, a `--read-only` mode, `--confirm` for destructive operations, and no network access unless a generator hook asks for it.
- [ ] An operation journal: every mutating command records the tool, the command, the diff, and the time. `rusting log` lists it and `rusting revert <op>` undoes one operation when later operations do not depend on it.
- [ ] A portable provenance report: engine and plugin versions, asset hashes, generator metadata, seeds, and scenario versions (from `missingFeatures.md` P2).

### Exit gate

- A logic change in a running sample reaches the game in under one second (hardware gate).
- Every agent change in a benchmark run is listed in the journal and revertible.

## Milestone L9: Agent benchmark and evidence

Goal: prove the "best engine for LLM coding" claim with a repeatable benchmark, the way Milestone 12 proves the physics claim.

Depends on: Milestones L1-L5 for the tools it measures; can start earlier with the tools that exist.

- [ ] `benchmarks/agent/`: a task suite of one-paragraph game requests with hidden acceptance scenarios, covering new games, feature additions to existing projects, bug fixes in seeded broken projects, and performance fixes.
- [ ] A model-agnostic runner that drives any agent CLI through a command hook and records wall time, turns, output size, commands, failed edits, corrective builds, human interventions, and hidden-scenario pass rate.
- [ ] Baselines for the same tasks in Godot and at least one other engine, run with the same agent and budget.
- [ ] A scheduled job that runs the suite for every release and publishes a report; a regression opens an item in this file.
- [ ] Every failed task is triaged into a gap item in the owning milestone, as the sample games were.

### Exit gate

- The published report shows RustingEngine ahead of the baselines on pass rate and time, and every task where it is not has a follow-up item.

## Milestone L10: Large projects and multiple agents

Goal: agents stay effective on a project with hundreds of scenes and systems, and several agents can work on one project at once.

Depends on: Milestones L1, L3, and L4.

- [ ] `rusting project summary [--budget N]`: a budgeted overview of scenes, systems, components, assets, and their dependencies.
- [ ] A system access graph from ECS system parameters: which systems read and write which components and resources, queryable as "who writes `Health`?".
- [ ] Scene files that merge well: stable key and entity order, and an optional folder form with one file per entity or prefab.
- [ ] `rusting merge`: a semantic three-way merge driver for scenes, registered through `.gitattributes`, that reports real conflicts by entity and field.
- [ ] Scoped leases: an agent can claim scenes, prefabs, or source files through the daemon; conflicting writes are refused with the holder named.
- [ ] Impact reports: before a change to a prefab, component, or asset, list every scene and scenario it affects.

### Exit gate

- Two agents build separate features in one project at the same time, merge with no manual conflict resolution, and all scenarios pass.
- An agent completes a benchmark bug-fix task in a project with at least 100 scenes and 50 systems.

## Dogfood track: PLAYPLACE

Goal: build a real game from `researches/game2/research` (PLAYPLACE, the pick in its VERDICT.md) at `RustingGames/playplace` using only `rusting` commands, and fix every engine gap it exposes. The friction log is `RustingGames/playplace/FRICTION.md`.

- [x] Week 1 slice: pit room, 40,000 code-spawned GPU spheres, `ball_asleep` GPU rule feeding a `sleeping` counter, scenarios `w1_pool` (real hardware, 600 ticks) and `w1_pool_quick` (120 ticks).
  Evidence (2026-10-01, lavapipe because the real GPU is wedged): `rusting check` passes; `rusting test tests/w1_pool.json` before the fix failed at the sleeping check with the counter at 0; with a capture step the same game reported 14,146 sleeping balls at tick 120 (1.2 s per tick in software). Real-GPU frame time, the 60 FPS kill criterion, is NOT measured.
- [x] Fix: scenario `expect` on a game with code-spawned meshes failed with "mesh N has no asset path and cannot be saved". `reflected` now uses `scene_document_lenient`. Verification: the gpu-test `the_gpu_flag_opens_the_device_without_a_capture_step` and the playplace scenario above.
- [x] Scenario `"gpu": true` opens the headless Vulkan device without a capture step and fails with `gpu: no Vulkan device` when none opens. Documented in `rusting schema`, `docs/concepts.md` and tutorial 4. Answers PLAYPLACE research gap #1 (GPU physics does run in `rusting test`).
  Full check, 2026-10-01, lavapipe: fmt clean; clippy plain, `--no-default-features`, `--features gpu-tests` clean; `cargo test --workspace` exit 0, 16 result lines, 0 failed; same with `--features gpu-tests`.
- [ ] A `--template empty` (no leftover `Ball`/`Box`/`Wall` names), or a clearer error when code-spawned names collide with the scene.
- [ ] A bulk spawn for scenes (emitter component or a patch grid operation), so large uniform sets need no game code.
- [ ] A built-in "bodies inside this box" count for GPU classes readable from a scenario, for `escaped` and zone-mass checks.
- [ ] Week 2: noise-attraction force field as a custom GLSL solver; scenario `w2_flow`.
- [ ] Measure 40,000 spheres on real hardware (60 FPS at 1080p, RTX 3060) once the GPU driver is healthy.

## Dogfood track: 40,001

Goal: fix every engine gap that `RustingGames/forty-thousand-and-one/FRICTION.md` (F1-F29) logs.

- [x] F1, F3: `rusting new` writes `skills/rusting-game/SKILL.md`; the agent guide points at `rusting docs show sample/<name>`, not at a `samples/` folder. Verification: `tests/cli.rs` project tests.
- [x] F2: `rusting doctor --probe` opens the device and round-trips a 64-word buffer fill (15 s timeout), reporting `probe.ok` with `selected_device` or `error`. Verification: the doctor CLI test.
- [x] F4, F25, F26, F27: sound output (`play_sound`, `stop_sound`, `set_master_volume`, `sound_cue`), a scenario `audio:` entity with request counts, `sound_clip_diagnostics` in `rusting check`, one project-relative root for asset and clip paths, and the sound API in SKILL.md, the project guide and `rusting docs`. Verification: runtime and CLI unit tests.
- [x] F5: `docs/gpu-condition-shaders.md` documents the hooks, the `PhysicsState` ABI, the free `custom_values`, `emit_event`, sleep and determinism.
- [x] F6, F20: `GpuConditionShader::params` (uniform `vec4`s at binding 14, no recompile) and `events_per_body`; overflow is printed and sent as `GpuPhysicsEventsLost`. Verification: gpu-tests for condition params and event capacity.
- [x] F7, F8, F9, F12: `docs search` reports the total past its limit; `Name::as_str` and `Deref`; `GameObject::entity()`; `scene.set_active_camera(name)`; point light intensity documented in its unit. Verification: unit tests.
- [x] F10: render scale works (see Night Market F10). The post-process hook is still open: it needs the tone-map pass to read HDR from a sampled image instead of a subpass input.
- [x] F11, F13, F14: the GPU contact pass takes the largest push and velocity change per direction instead of their sum, runs 4 rounds per tick (`CONTACT_ROUNDS`), ignores contacts closer than a 2 mm margin, and dynamic bodies sleep after 20 ticks under 0.05 m/s (`angular_velocity.w`; `GpuCondition::sleeping()`). Verification: gpu-test `a_deep_gpu_ball_pit_settles_and_its_walls_hold`: 3,600 balls in a 4 x 4 m pit with sideways gravity of 8 m/s^2, 3,045 at rest and 0 escaped. The old solver gave 646 at rest and 17 escaped through wall corners.
- [x] F15, F16, F19, F22: reading a missing counter warns once, and `set_counter` / `add_to_counter` create it, so game state needs no scene entity per counter; the scenario docs give the order inside one tick; `expect` and invariants take `not_equals`. Verification: runtime and scenario unit tests.
- [x] F17, F18, F21: `preset apply --only`, EPIPE exits quietly, `scene query` returns component values as JSON. Verification: CLI tests.
- [ ] F23: 3D world-space text. Deferred; needs a text atlas in the scene renderer.
- [x] F24: `rusting determinism <root> --gpu <scenario.json>` runs the scenario in a debug and a release build and compares the GPU state hashes of every tick both runs reached (`DETERMINISM_DIVERGED` with the tick, or `DETERMINISM_NO_GPU_STATE`). Scenario reports carry `gpu_state_hashes`; a failing expect step does not stop the comparison (`scenario_passed`). Evidence (2026-10-02, RTX 3060): `rusting determinism . --gpu tests/w6_seed_a.json` on 40,001 compared 29 ticks, all equal (its pinned `night_hash` is stale after the solver change, so `scenario_passed` is false).
- [x] F28: HUD elements are placed from this frame's measured text, so a right or bottom anchor keeps its margin when a `{counter}` value grows; the offset direction is in the schema. Evidence: 40,001 `tests/w4_cctv.json` recapture shows "COUNT: 40000" ending 24 px from the right edge.
- [x] F29: the project guide says a `path` dependency follows a live engine and how to pin a `rev` or `tag`.
  Full check, 2026-10-02, RTX 3060: fmt clean; clippy plain, `--no-default-features`, `--features gpu-tests` clean; `cargo test --workspace` 0 failed; `--features gpu-tests` 573 lib tests passed, 0 failed. The grid test now counts overflow and oversized bodies once per contact round. `rusting check` on 40,001 passes.

## Dogfood track: Lantern Keeper

Goal: fix the engine gaps that `RustingGames/lantern-keeper/FRICTION.md` (F1-F26) logs.

- [ ] F1: skeletal animation and skinned meshes. Deferred (milestone-sized); SKILL.md and the project AGENTS.md now say there is none and to animate child entities from game code.
- [x] F2, F7: a walking player keeps pushing a dynamic body: `move_character` reports the `wall` it slid along and the controller raises the body's horizontal velocity along the push to the walking speed (mass is ignored). `PlayerController` carries runtime `wall` and `velocity`; `touching` includes the player's floor and wall; `scene.player(name)` returns a copy of the controller. Verification: `a_walking_player_keeps_pushing_a_crate_it_did_not_start_in` (crate past z -6 after 180 ticks, player grounded, velocity z about -4).
- [x] F3: scenario `set` reaches `/visible` and the fields of `rigid_body`, `collider`, `physics_body`, `collision_layers` and the three lights. Verification: `set_reaches_built_in_components`.
- [x] F4, F10: `rusting check` runs the debug `cargo build` that `run` and `test` reuse instead of `cargo check`, so no command compiles the engine twice. `rusting new` prints a `Next:` line saying the first build takes minutes. The cold build itself is unchanged.
- [x] F5, F23: plain `rusting test` prints each `log` step as `tick N: <value>`, pass or fail, and a failed scenario message ends with the last 20 lines of game stderr. Verification: `plain_output_lists_scenario_log_values_even_on_failure`.
- [x] F6: no new shape; a dynamic `ConvexMesh` body on the `Cylinder` mesh is the barrel. It slid instead of rolling: `mesh_manifold` picked the body with fewer vertices, so an 8-vertex floor box beat the cylinder hull and the floor's far corners became contact points. It now picks the smaller body by half-extent length. Verification: `a_barrel_with_a_convex_cylinder_collider_rolls` (fails before the fix: no spin, stuck at x 1.33).
- [x] F8: a missing counter warns once (done for 40,001). `counter_value` and `counter_complete` now take `&self` (a read-only `try_query`); the warn-once set is a process-wide static that feeds only warnings.
- [x] F9: SKILL.md puts `parent` inside `entity` of a `create` op.
- [x] F11: `PlayerController::turn_speed` (rad/s, default 0) turns the body's non-`Camera` children toward the horizontal `velocity` in a fixed-step `player_face` system after `player_move`. Verification: `a_player_with_a_turn_speed_faces_its_rig_where_it_walks` (rig yaw -90° walking +X, body yaw unchanged).
- [x] F13: `scene.reparent(name, parent)` keeps the local transform; `scene.set_light(name, color, intensity, range)` changes a point or spot light. Verification: `reparent_and_set_light_move_and_dim_a_lantern`.
- [x] F14, F19: `rusting determinism <root> --scenario <file>` replays a scenario in a debug and then a release build and compares the CPU state hash of every tick, plus GPU hashes when present (`data.divergence.state`). Scenario reports carry `state_hashes`. Each build prints a progress line to stderr. Evidence (2026-10-02): Lantern Keeper `tests/w6_replay.json` compared 520 ticks, all equal, in 66 s.
- [x] F15: `rusting run --record FILE` and `--replay FILE` expose the existing input replay (`RUSTING_REPLAY_OUT`, `RUSTING_REPLAY_PLAY`). Game-code input injection is not added; a recorded run covers the replay need.
- [x] F16: `slide` first casts down from one `bottom` above the body, so a body sunk into a walkable top by less than that is lifted onto it. Verification: `a_player_teleported_into_a_platform_stands_on_top_of_it` (fails without the cast).
- [x] F17: the scenario `log` step already exists; SKILL.md now points to it from the `inspect` note.
- [x] F18: the `counter_value` doc says counters are `i32`.
- [x] F20: `BurstEmitter` gains `rate` (particles/s, every fixed step, fractional carry in `pending`), `area` (world-aligned start box, seeded per tick from a `burst_area` stream) and `stretch` (Y scale factor). Verification: `an_emitter_with_a_rate_rains_streaks_over_its_area`.
- [x] F22: `rusting docs show api/PlayerController` is generated from the struct and field doc comments in `player.rs`; the undocumented fields got docs. Verification: `api_index_lists_documented_gameplay_methods`.
- [x] F24: scenario `tap` step presses and releases before the next tick; SKILL.md explains the `pressed` edge. Verification: the tap case in the scenario press test.
- [x] F25: a `capture` path whose first folder repeats the scenario's folder (`tests/` inside `tests/`) adds a warning with the written path.
- [x] F26: SKILL.md says bodies solve in entity order and scenarios should assert robust values.

## Dogfood track: Night Market

Goal: fix the engine gaps that `RustingGames/night-market/FRICTION.md` (F1-F16) logs.

- [x] F1: a headless egui pass (`rusting test`, `debug`, `RUSTING_HEADLESS_TICKS`) builds its `RawInput` from `RuntimeInput`: screen from the viewport, cursor, mouse button edges and key edges. Verification: `a_headless_pass_clicks_a_button_from_runtime_input` (mouse click, then Tab and Enter click the focused button).
- [x] F2, F14: new `guide/menus-and-ui` page (main menu, pause, quit, keyboard focus, saves, rebinding, menu scenarios); `GameScene::set_paused` and `paused` wrap `TimeControl`. SKILL.md and the project AGENTS.md point to it.
- [x] F3: `docs search` with no match prints "No matches for `<query>`" and the hint to use fewer words.
- [x] F4: `GameScene::cursor`, `viewport_size`, `keys_pressed` and `rebind(action, keys)`.
- [x] F5: a scenario sets the viewport to `capture_size` at tick 0. Verification: `pointer_steps_place_the_cursor_in_the_capture_sized_view`.
- [ ] F6: first build time. Unchanged; `rusting new` already prints the note.
- [x] F7: `GameScene::save_data`, `load_data`, `delete_data` in a per-game user data folder (`project::user_data_folder`, `RUSTING_USER_DATA` override; keys reject absolute paths and `..`), and `GameScene::quit` (window closes, headless run stops).
- [x] F8: scenario `restart` step and top-level `files` (copied into a per-scenario user data folder, `build/test-userdata/<scenario>/`, emptied before each run). A relaunch step is skipped: `restart` plus `files` cover save tests.
- [x] F9: `determinism --scenario` prints a verdict line with the number of ticks compared (the last 1024 the hash history keeps) and the final hash.
- [x] F10: video settings. `GameScene::set_render_scale` (0.25 to 2.0; the scene renders into an offscreen image of that size, then a linear blit stretches it over the window or capture, and UI is painted after at full resolution; skipped when the device or swapchain cannot blit), `set_vsync`, `set_max_fps`, `set_fullscreen` (borderless, current monitor) and `set_window_size`, applied by the window runner after the frame. Verification: `a_half_render_scale_still_fills_the_whole_target` (gpu-tests, lavapipe), `video_settings_write_render_settings_and_the_window_request`, `scaled_extent_rounds_clamps_and_never_reaches_zero`; a 20 s Night Market window run on an RTX 3060 at scale 0.5 with a `set_window_size` and a scale change showed no errors. Fullscreen is real-hardware only and was not run.
- [x] F11: scenario `click` step finds the topmost on-screen text with that label (from egui's output shapes) and clicks it; a miss lists the texts on screen. `rusting inspect --ui` is skipped: the failure lists the labels. Verification: `click_steps_press_egui_buttons_by_label_and_quit_ends_the_run`.
- [x] F12: a failed check names the inputs that ran on its tick. Verification: `a_failed_check_names_inputs_that_ran_on_its_tick`.
- [x] F13: gamepad input through gilrs 0.11 (owner approved; `window` feature). Every pad feeds `RuntimeInput`: `PadButton` buttons, two sticks, and stick directions as buttons (press past 0.5, release below 0.3). `InputBinding::Pad` and `Pad*` input names in `rusting.input_action`; the player controller binds the left stick, d-pad, South (jump) and LeftStick (sprint), and the right stick looks at 3 rad/s. egui menus: the d-pad or left stick focuses and moves the focus, South clicks, East is Escape. `GameScene::stick`, pad names in `keys_pressed`, scenario `left_stick` / `right_stick` steps. Verification: `pad_buttons_and_stick_directions_drive_actions`, `a_headless_pass_clicks_a_button_from_runtime_input` (d-pad then South), `pad_presses_and_stick_steps_reach_runtime_input`, `player_look_needs_captured_cursor_and_clamps_pitch` (right stick). Reading a physical pad is real-hardware only.
- [x] F15: `GameScene::counters()` lists every counter for saves. Bulk creation stays a `once` loop.
- [x] F16: the exported README.txt documents the user data folder, `RUSTING_USER_DATA`, `RUSTING_HEADLESS_TICKS` and `std::env::args()`; `lib.rs` re-exports `serde_json`. `rusting test --build <export>` is deferred.
- [x] Also: `rusting doctor --probe` prints the probe result in text output, not only in JSON.
  Full check, 2026-10-02, RTX 3060: fmt clean; clippy plain, `--no-default-features`, `--features gpu-tests` clean; `cargo test --workspace` 498 lib tests passed, 0 failed; `--features gpu-tests` 577 lib tests passed, 0 failed. All 7 Lantern Keeper scenarios pass with the engine push.

## Dogfood track: Metronome Mines

Goal: fix the engine gaps that `RustingGames/metronome-mines/FRICTION.md` (F1-F19) logs.

- [x] F1: `CLI_OUTDATED` names the newest changed engine file, how much newer it is than the CLI binary, and the command `cargo install --path <engine> --locked`.
- [x] F2, F3, F4: sounds belong to a fixed tick and start one fixed step after it (`Sound::at_tick` for a later tick), so music restarted each bar with `at_tick` stays on the beat; `BeatClock` (`tick_of`, `beat_at`, `is_beat`, `ticks_to_next`, `phase`, `beats_between`). An audio-position query is skipped: the start tick gives it. Verification: `beat_clock_lands_on_whole_ticks`, `the_offline_mix_reports_levels_pan_buses_and_scheduled_starts`.
- [x] F5: SKILL.md says game code sees `time.fixed_tick == N` on scenario tick N.
- [x] F6: `GameScene::press_tick(action)` gives the press time in fractional ticks from the window event time. Verification: `press_tick_is_the_event_time_or_the_frame_tick`.
- [x] F7: scenarios mix every sound through kira's own mixer offline (`OfflineBackend`); `audio:` reports `/level` and `/playing`, and top-level `audio_out` writes the mix as a WAV. Verification: `the_offline_mix_reports_levels_pan_buses_and_scheduled_starts`.
- [x] F8, F9: `play_sound_with(clip, Sound { pan, bus, position, .. })`, `set_sound_volume`, `set_bus_volume` with fades; positional sound pans to the camera's side and falls off as `2 / distance` past 2 m. New `guide/audio` page. Hearing it on speakers is real-hardware only and was not done.
- [x] F10, F11: the HUD draws in PostUpdate, so a capture shows the tick's own text, and it no longer fades in; `GameScene::set_hud`. Verification: `hud_text_shows_the_counters_game_code_set_on_the_same_tick`.
- [x] F12: the GPU hybrid solver damps spin by `5 * dt` per tick while a body touches something, so a spun body sleeps. Verification: `gpu_bodies_rest_on_cpu_colliders_unless_filtered_or_no_collision` (gpu-tests) now spins the box and asserts it sleeps; it fails without the damping.
- [x] F13: `GameScene::load_text`; `rusting validate` reports `CODE_MISSING_ASSET` for a literal `load_text` or `play_sound*` path that is not a file under `assets/`. Verification: `literal_asset_paths_in_game_code_must_be_files`.
- [x] F14: `GpuBodySettings::sync` and `GameScene::gpu_state(name)`; scenarios read the GPU pose under `/gpu_state`. Verification: `gpu_bodies_report_the_gpu_pose_under_gpu_state`.
- [x] F15: patch `upsert` operation replaces the entity with the same `id` or `name` and keeps its id. Verification: `a_patch_of_upserts_runs_twice_and_keeps_ids`.
- [x] F16: scenario reports copy state hashes after every tick, so `determinism --scenario` compares all ticks. Verification: `hashes_are_kept_past_the_world_history`.
- [x] F17: an unknown enum variant lists the valid ones. Verification: the reflect enum test.
- [x] F18: `GpuCondition`, `GpuFieldCondition` and `GpuStateField` are in the API docs.
- [x] F19: `rusting asset import` reports `channels` for a WAV. Verification: the WAV import test asserts `Some(1)`.
  Full check, 2026-10-03, RTX 3060: fmt clean; clippy plain, `--no-default-features`, `--features gpu-tests` clean; `cargo test --workspace` 515 lib tests passed, 0 failed; `--features gpu-tests` 595 lib tests passed, 0 failed. `rusting validate` is clean for 40,001, Lantern Keeper, Metronome Mines and Night Market.

## Dogfood track: Split Signal

Goal: fix the engine gaps that `RustingGames/split-signal/FRICTION.md` (F1-F22) logs.

- [x] F1: `Camera.viewport` (`[x, y, width, height]` fractions); viewport cameras draw over the full-window camera, lower priority first; `GameScene::set_camera(name, active, viewport)`. Verification: `viewport_cameras_split_the_frame`, `viewport_cameras_draw_over_the_full_window_one_in_priority_order` (gpu-tests).
- [x] F2, F3: `PlayerController.mouse_look` (false leaves the mouse free) and `camera_offset` (over-the-shoulder). Verification: `player_look_needs_captured_cursor_and_clamps_pitch`.
- [x] F4: patch errors for a too-short array name the operation and the field path. Verification: `a_bad_field_in_a_created_entity_is_named_by_its_path`.
- [x] F5, F6: patch `delete` takes `missing_ok`; `upsert` takes `merge: true` to keep fields left out. Verification: `delete_can_skip_a_missing_entity_and_upsert_can_merge`.
- [x] F9: render scale is set from game code (video settings) and is no longer listed as a scene resource. Verification: `video_settings_write_render_settings_and_the_window_request`, `a_half_render_scale_still_fills_the_whole_target` (gpu-tests).
- [x] F10, F11: `expect_screen` and `capture` take `camera`; `capture` takes `hud: false` and `golden` with `tolerance`; `rusting test --update-golden`; new `expect_pixels` step (region mean and luma deviation bounds); `rusting capture --no-hud`. Verification: `pixel_checks_goldens_and_named_cameras` (gpu-tests), `capture_steps_take_a_path_or_options`, `region_stats_and_golden_differences`, `screen_checks_see_a_mesh_in_front_of_a_camera_and_behind_a_wall`.
- [x] F12: the perf report has `gpu_ms` and, with viewport cameras, `render.cameras` per camera, without a capture step. Verification: `the_gpu_flag_opens_the_device_without_a_capture_step`, `a_render_budget_reports_render_counters_without_a_capture`.
- [x] F13: does not reproduce. `tools/perf_feeds.py` on 2026-10-03, RTX 3060: 2.37 ms per feed in debug, 2.21 ms with `--release`. No change.
- [x] F14, F15: rotation order, `basis(name)` and `RayHit` fields are documented; `raycast_skipping` passes through chosen classes. Verification: `basis_forward_follows_the_rotation_order`.
- [x] F16: a scenario `set` creates a missing counter, like game code. Verification: `counter_shorthand_sets_checks_and_logs_a_counter_by_name`.
- [x] F17: an unknown expect path suggests the nearest real paths. Verification: `a_missing_path_suggests_the_nearest_real_ones`.
- [x] F18: `rusting docs show` prints whole pages. Verification: `docs_lists_searches_shows_and_briefs_within_a_token_budget`.
- [x] F19: new `guide/cameras` page (viewports, split screen, HUD per camera, camera tests, limits).
- [x] F20: `rusting.hud` element `camera` anchors to that camera's viewport and hides while it is inactive. Verification: `hud_elements_follow_their_camera_viewport`.
- [x] F22: the game AGENTS.md sample uses `time.delta_seconds()`.
- [ ] F7 (render to texture), F8 (per-camera post effects), F21 (reuse builds across commands): deferred. `guide/cameras` lists F7 and F8 under Limits.
  Bug found on the way: `skip_serializing_if` on `SceneCamera.viewport` broke cooked (bincode) scenes; removed, and `source_and_compiled_scene_decode_to_same_document` now cooks a viewport camera.
  Full check, 2026-10-03, RTX 3060: fmt clean; clippy plain, `--no-default-features`, `--features gpu-tests` clean; `cargo test --workspace` 524 lib tests passed, 0 failed; `--features gpu-tests` 606 lib tests passed, 0 failed.

## Dogfood track: update pass

Goal: fix the engine friction the six-game update pass (RustingGames, 2026-10-03) logs.

- [x] Conditions combine with `&` and `|` (`GpuCondition`). Verification: `docs` and `gpu_condition` lib tests.
- [x] Game code: `name_of`, `has_class`, `binding`, `set_pixelated`. Verification: the `in_class("ember")` test in `project_runner`.
- [x] `RenderSettings.pixelated` upscales a low render scale with nearest filtering. Verification: `a_half_render_scale_still_fills_the_whole_target` checks 4x4 blocks at 0.25 (gpu-tests).
- [x] Scenario `click` takes `{text | starts_with, index}`; `index` counts in reading order. Verification: `find_text_where_picks_by_prefix_and_reading_order`.
- [x] New `expect_file` step for save data, evaluated even after the game quits. Verification: the quit test in `scenario`.
- [x] `press`/`tap` take a fractional `at`, recorded as the press tick. Verification: `a_press_at_a_fraction_records_that_press_tick`.
- [x] The `audio:` entity reports `peak` and `clipped`. Verification: the audio mix test asserts both.
- [x] `log` on a missing entity or path says so and suggests near paths; a missing `/gpu_state` names the sync modes that make it.
- [x] Patches accept RGB colors and add alpha 1. Verification: `scene_patch` tests.
- [x] `rusting asset import --dry-run` shows WAV channels. Verification: `asset_import` tests.
- [x] `RUSTING_KEEP_GOING`, and `rusting determinism --scenario` keeps going past failed checks (forty F30).
- [x] `--record` with `--ticks`, `--scenario` or `--replay`, and `--replay` with headless runs, fail with `CLI_USAGE` before the build; `--record` honours `--timeout`. Verification: `record_and_replay_mistakes_fail_before_the_build`.
- [x] `rusting test` and plain output print `log:` lines and step warnings for every scenario.
- [x] `AGENTS_OUTDATED` warns when a project's engine-written AGENTS.md differs from this engine's; `rusting fix` refreshes it and keeps `AGENTS.md.old` (metronome F24, split F28). Verification: `an_old_engine_agents_md_is_flagged_and_refreshed`.
- [x] split F23 (w6_perf slower): does not reproduce. A/B raycast bench, 3600 rays, best of runs: current 4.874/4.904/4.922 ms, HEAD 4.888/4.861/4.882 ms. Slowdown was machine load.
- [ ] Deferred: `expect_pixels` `differs_from`, counters design (night F15), skeletal animation, bulk GPU spawn, GPU body control and waking piles, render to texture, per-camera post effects, build caching, audio limiter, kill plane, `click` `right_of`, per-kind rebinding.
  Full check, 2026-10-03, RTX 3060: fmt clean; clippy plain, `--no-default-features`, `--features gpu-tests` clean; `cargo test --workspace` 528 lib tests passed, 0 failed; `--features gpu-tests` 610 lib tests passed, 0 failed (one earlier GPU run failed in the lib tests while the machine was loaded; a rerun and the full run passed).

## Sundering track

Milestones 24-29 build *Sundering* on top of the engine. They consume the determinism, fracture, GPU-scale, navigation, and networking milestones and must not fork parallel versions of those systems; gaps found here become engine items in the owning milestone.

## Milestone 24: Competitive networked authority

Goal: run the authoritative simulation on a server and keep many clients consistent with it under real network conditions.

Depends on: Milestones 8 and 19. Milestone 19 provides the general transport, protocol, RPC, replication, and rollback machinery; this milestone adds the competitive-authority, anti-cheat, and destruction-scale requirements Sundering needs on top of it.

The networking model depends on the Milestone 8 result. Bit-identical determinism permits lockstep and rollback. Without it, the server simulates and clients render, predicting only their own hero.

### Server

- [ ] Headless server binary running the fixed-tick simulation with no renderer or window.
- [ ] Tick-scheduled loop with drift correction, overload detection, and reported tick-budget usage.
- [ ] Client lifecycle: join, ready, in-match, drop, reconnect.
- [ ] Server-side input validation and rate limiting. The server never trusts a client-reported position, velocity, or terrain state.

### Transport and protocol

- [ ] Reuse the Milestone 19 transport and versioned protocol; do not fork a second networking stack.
- [ ] Clock synchronization and per-client tick-offset estimation.
- [ ] Input buffering with configurable delay and jitter absorption.

### State replication

- [ ] Snapshot encoding with delta compression against the last client-acknowledged snapshot.
- [ ] Quantize replicated values with documented precision per field.
- [ ] Interest management: replicate only what a client's team can currently see, using the vision system from Milestone 27.
- [ ] Per-client bandwidth budget with measured usage and an enforced cap.
- [ ] Replicate terrain modification events, not individual debris transforms. Clients re-simulate debris locally from the same events. This is the only approach that keeps bandwidth bounded during heavy destruction, and it depends on Milestone 8.

### Client

- [ ] Interpolate remote entities with a documented, configurable interpolation delay.
- [ ] Predict only the local hero. Never predict terrain destruction results unless determinism is proven.
- [ ] Reconcile prediction error with presentation-only smoothing that never feeds back into simulation state.
- [ ] Detect desync by periodic state-hash comparison, report it, and recover through full resynchronization.
- [ ] Support spectator clients that receive state and send no input.

### Exit gate

- Ten clients complete a full match against a headless server at 200 ms round-trip latency with 2% packet loss, without desync.
- Per-client bandwidth stays within budget with the terrain fully destroyed.
- A client disconnects mid-match, reconnects, and resynchronizes correctly.
- Server tick-budget usage and desync events are observable in operations tooling.

## Milestone 25: Destructible terrain and Scar

Goal: make terrain a first-class simulated, modifiable, and permanently scarred entity rather than authored static geometry.

Depends on: Milestones 8, 11 (fracture, debris, GPU scale), and 13 (terrain rendering).

### Terrain representation

- [ ] Chunked volumetric terrain with an authoring format, streaming, and a documented chunk size.
- [ ] GPU surface extraction with per-chunk LOD and crack-free transitions between levels.
- [ ] Derive terrain collision from the same volume data as rendering. One source of truth, never two.
- [ ] Editor tools to author, sculpt, and paint terrain volumes.
- [ ] Report chunk count, resident memory, extraction time, and streaming state in the profiler.

### Fracture and debris

- [ ] Promote static terrain chunks to dynamic GPU rigid bodies when destroyed.
- [ ] Use deterministic fracture patterns driven by the seeded tick RNG.
- [ ] Enforce a debris budget with a deterministic, documented policy at the cap. Never drop bodies arbitrarily.
- [ ] Settle and re-freeze resting debris back into static terrain, restoring collision and navigation.
- [ ] Merge small settled debris into larger static aggregates so long-running matches stay bounded.

### Structural load

- [ ] Build a support graph over connected terrain chunks.
- [ ] Solve support incrementally on the GPU when the graph changes, within a bounded per-tick cost.
- [ ] Collapse unsupported spans under their own mass.
- [ ] Expose per-chunk stress to gameplay and to debug visualization.

### Scar

- [ ] Make settled debris permanent by default, with an explicit per-region regeneration policy.
- [ ] Ensure settled debris affects collision, navigation, and vision, not only rendering.
- [ ] Persist terrain state in save, replay, and network resynchronization formats as modification history where practical rather than full volumes.

### Terrain commands

- [ ] Extend the CPU-to-GPU command bridge with terrain modification: carve, fracture, displace, freeze.
- [ ] Apply terrain commands at deterministic points in the tick.
- [ ] Account modification cost against a per-team mass-budget resource.

### Exit gate

- A cliff collapses onto a 10,000-body scene deterministically, producing identical hashes on two GPU vendors.
- Settled debris becomes walkable, occluding, static terrain.
- Frame time and memory stay within budget after a one-hour continuous-destruction soak test.
- Terrain state round-trips correctly through save, replay, and network resynchronization.

## Milestone 26: Dynamic navigation and crowds

Goal: let thousands of agents move sensibly over geometry that changes every tick.

Depends on: Milestones 18 (engine navigation) and 25. The engine navigation handles incremental re-baking of ordinary scenes; this milestone handles terrain that changes every tick at crowd scale.

### Navigation representation

- [ ] GPU navigation volume or flow field derived from live terrain collision data.
- [ ] Incremental rebuild limited to changed regions, with a bounded and enforced per-tick cost.
- [ ] Deterministic rebuild results, independent of how many chunks changed in a single tick.
- [ ] Reachability queries answering whether a team can reach a given point at all.
- [ ] Report navigation build time, dirty-region count, and budget overruns in the profiler.

### Agents

- [ ] GPU agent steering with local avoidance at thousands of units.
- [ ] Deterministic agent update order and avoidance resolution.
- [ ] Agents fall, are buried, and are displaced by debris rather than ignoring it.
- [ ] Make path failure an observable gameplay state, never a silent stall.
- [ ] Keep a CPU pathing path for the small number of agents that need immediate answers.

### Exit gate

- 1,000 agents cross a map while it is actively being destroyed, without stalling or tunnelling.
- Navigation rebuild stays within its per-tick budget during a full cliff collapse.
- Agent behaviour is bit-identical across runs and across vendors.
- Blocking every route is detected and reported rather than producing stuck agents.

## Milestone 27: Competitive gameplay framework

Goal: provide the game-specific systems Sundering needs, built entirely on deterministic simulation.

### Match and teams

- [ ] Team membership, match phases, and authoritative match state owned by the server.
- [ ] Objective and structure entities with destruction-aware state.
- [ ] Resource and mass-budget accounting per team.
- [ ] Match start, end, surrender, and result reporting.

### Abilities as forces

- [ ] Express ability definitions as deterministic physical effects: impulses, sustained forces, tethers, mass changes, area displacement.
- [ ] Cooldowns, costs, cast time, interrupts, and targeting resolved on the fixed tick.
- [ ] Area queries against live terrain and bodies, executed deterministically.
- [ ] Require every ability effect to be expressible as commands through the physics bridge.
- [ ] Expose telegraph data to presentation without letting presentation affect simulation.

### Dynamic vision

- [ ] Per-team visibility computed on the GPU from actual terrain geometry, not from authored vision blockers.
- [ ] Recompute visibility when terrain changes, within a bounded per-tick budget.
- [ ] Feed visibility into network interest management from Milestone 24.
- [ ] Derive presentation-side fog rendering from the same data.
- [ ] Keep visibility deterministic and identical between server and every client.

### Bots

- [ ] Bot controllers that use the same input interface as human players.
- [ ] Deterministic bot decisions driven by the seeded tick RNG.
- [ ] Difficulty levels with a documented behaviour set.
- [ ] Bots handle a reshaped map, including blocked routes and newly opened ones.

### Exit gate

- A match can be played to completion against bots with no human opponents.
- Vision is computed from destroyed geometry and matches exactly between server and clients.
- Every ability is expressible as a deterministic physical effect and replays identically.

## Milestone 28: Balance and operations tooling

Goal: make a game whose map differs every match tunable with evidence rather than intuition.

### Headless experimentation

- [ ] Run matches headless, faster than real time.
- [ ] Batch runner executing many matches across varied seeds, configurations, and bot profiles.
- [ ] Deterministic seeding so any interesting match is exactly reproducible from its seed.

### Telemetry

- [ ] Structured per-match telemetry: terrain modified, routes opened and closed, fight locations, resource use, outcome.
- [ ] Aggregate reporting across batch runs.
- [ ] Automatic detection of degenerate states: total lockout, unreachable objectives, runaway debris count, stalled navigation.

### Replay analysis

- [ ] Replay browser with seeking, speed control, and a free camera.
- [ ] Terrain-state timeline showing how the map evolved through the match.
- [ ] Export a match's terrain modification history for offline analysis.

### Server operations

- [ ] Match hosting, lifecycle management, and health reporting.
- [ ] Crash and desync capture producing reproducible artifacts.
- [ ] Per-map performance baselines with regression rejection.

### Exit gate

- One thousand headless bot matches run unattended and produce an aggregate balance report.
- Any reported degenerate state reproduces exactly from its recorded seed.
- A match replay can be inspected tick by tick alongside its terrain timeline.

## Milestone 29: Sundering vertical slice

Goal: prove the whole stack with the smallest piece of the real game. Each slice answers exactly one question and adds nothing beyond it.

### Slice 1: pathing survives destruction

- [ ] One lane, one tower, one destructible cliff.
- [ ] Two heroes, one able to destroy terrain.
- [ ] Creeps pathing from spawn to tower.
- [ ] Acceptance: creeps route correctly after the cliff collapses, deterministically, on two GPU vendors.

### Slice 2: Scar is legible

- [ ] Debris persists and blocks both vision and movement.
- [ ] Per-team dynamic vision active.
- [ ] Acceptance: a spectator can read the map state correctly after sustained destruction.

### Slice 3: structural constraints hold

- [ ] Structural load, mass budget, and debris settling all active.
- [ ] Two players attempt to wall themselves in, and to dig directly to the enemy base.
- [ ] Acceptance: both degenerate strategies fail for physical reasons, not through rule-based prohibitions.

### Slice 4: networked match

- [ ] 3v3 against a mix of bots and humans on a headless server.
- [ ] Full replay recorded and reproduced.
- [ ] Acceptance: a complete match at 200 ms latency with no desync.

### Performance target

- [ ] Define the Sundering benchmark: fixed map, fixed input replay, scripted destruction sequence.
- [ ] Target 1920x1080 at 60 FPS on mid-range hardware with destruction active.
- [ ] Record simulation tick time, navigation rebuild time, debris count, per-client bandwidth, and input latency.
- [ ] Store baselines and reject regressions.

### Exit gate

- A 3v3 match is playable from a clean checkout on Linux, with bots filling empty slots.
- The match replays identically from its recorded input stream.
- Performance baselines are recorded and enforced in CI.

## Engine expansion track

Milestones 30-35 cover areas a general-purpose engine needs that the Godot parity map does not own. They are parallel tracks after Milestone 9, subject to the dependencies at the top of each. Every feature here follows the LLM-native design rules: data-driven components with schema entries, a Rust API in the index, and scenario-testable behaviour. Items that need a new dependency say so and wait for the owner's approval.

## Milestone 30: Gameplay framework

Goal: the systems almost every game rebuilds are built in, inspectable, and deterministic.

Depends on: Milestones 8 and 9.

- [ ] Save games: versioned save files for selected components and resources, save slots, autosave, and migration of old saves across game versions, with a round-trip test per registered component.
- [ ] State machines as data: states, transitions, guards on counters and events, and enter and exit actions, editable in the editor and checked by scenarios.
- [ ] Behaviour trees and utility AI as data assets, with a deterministic tick order and a debugger view of the active branch.
- [ ] Health, damage, teams, and status effects as optional registered components, built on the typed event bridge.
- [ ] Inventory, items, and loot tables as data resources with seeded rolls.
- [ ] Dialogue and quest graphs as data assets, with localization keys and a runtime UI hookup.
- [ ] Timers, cooldowns, and a game clock with pause and time scale that never affect the fixed simulation step's determinism.
- [ ] Spawners and object pools with capacity reporting (no silent drops).
- [ ] Gamepad input with dead zones, rumble, and hot plug. Needs a gamepad backend dependency approved by the owner.

### Exit gate

- A sample RPG slice uses saves, dialogue, a quest, an inventory, and AI enemies with no custom framework code, and its scenarios load a save made by the previous engine version.

## Milestone 31: Terrain, large worlds, and streaming

Goal: build outdoor and open-world games, not only arenas.

Depends on: Milestones 2, 4, and 10; Milestone 13 for foliage rendering.

- [ ] General heightmap terrain with LOD, holes, splat-map materials, and physics collision; sculpt and paint tools in the editor.
- [ ] Foliage and detail scattering by rules (slope, height, mask), linked to the Milestone 13 instancing item.
- [ ] World partition: cells that load and unload by distance, with async asset loading and no hitch above a stated budget.
- [ ] Large coordinates: origin rebasing or 64-bit world positions, documented and tested at 100 km from the origin.
- [ ] Hierarchical LOD and impostors for distant cells.
- [ ] Oceans and rivers built on the existing `rusting.water` buoyancy and waves.
- [ ] Time of day and weather as scene settings driving sun, sky, fog, and wind fields.
- [ ] Splines and paths as a shared component for roads, rails, rivers, and camera tracks.

### Exit gate

- A 16 km² open-world sample streams with no loading screens, keeps frame-time spikes within budget, and stays stable at the far corner (hardware gate for timing).

## Milestone 32: Cinematics and media

Goal: tell stories in the engine with cutscenes, camera work, and video.

Depends on: Milestones 14, 15, and 16.

- [ ] A sequencer timeline asset with tracks for transforms, animation, cameras, audio, events, and properties, played deterministically by fixed tick.
- [ ] Camera rails, look-at targets, blends between cameras, and letterboxing, on the Milestone 31 splines.
- [ ] Subtitles and captions tied to audio cues and localization.
- [ ] Video playback to a texture and to the screen. Needs a decoder dependency approved by the owner.
- [ ] In-game photo mode and trailer capture (fixed-step offline rendering to an image sequence at any resolution).

### Exit gate

- A cutscene plays identically in the editor, the game, and an offline render, and a scenario can skip it and check the state after it.

## Milestone 33: Procedural content

Goal: generate levels and content from seeds, as ordinary engine data that tools and agents can inspect.

Depends on: Milestones 8 and 9.

- [ ] Seeded noise, sampling, and random streams on the Milestone 8 stream rules, with results identical across platforms.
- [ ] Generators as data assets: a graph or Rust function whose output is a normal scene or prefab, cached by seed and inputs, and regenerated on change.
- [ ] Tile and grid generators: wave function collapse, rooms and corridors, and cellular automata, for tile maps and 3D grids.
- [ ] Mesh generation helpers: extrusion along splines, lofting, and simple constructive solid geometry, with colliders.
- [ ] Validation hooks so a generated level can be checked by invariants and the Milestone L5 explorer bot for reachability.

### Exit gate

- A roguelike sample generates a new, fully reachable level per seed, and the same seed gives the same level on every machine.

## Milestone 34: Modding and user-generated content

Goal: players can extend a shipped game safely.

Depends on: Milestones 9, 21, and L8.

- [ ] Mod packages: data assets, scenes, and WASM scripts in a versioned archive with a manifest, dependencies, and a load order.
- [ ] Capability-based sandbox for mod scripts: a game declares which APIs mods may call.
- [ ] Mod conflict reporting by asset and component field.
- [ ] Reusable in-game level editor built from the editor's widgets and command layer, with undo.
- [ ] Workshop and storefront hooks through the Milestone 21 platform services abstraction.

### Exit gate

- A sample game ships a mod that adds a level, an item, and a scripted enemy, and a malicious mod script cannot read files or reach the network.

## Milestone 35: XR and additional input

Goal: record and, where justified, deliver VR and new input devices.

Depends on: Milestones 3, 4, and 21.

- [ ] OpenXR support through Vulkan: stereo rendering, head and hand tracking, and controller actions in the action map. Needs owner approval for the loader dependency; record it as a post-1.0 decision if deferred.
- [ ] Physics-based hand interaction (grabbing, throwing, pushing) built on the typed bridge.
- [ ] Touch and pen input with gestures, shared with the Milestone 21 Android work.
- [ ] Comfort options: snap turning, vignette, and seated mode.

### Exit gate

- A physics sandbox runs in a headset with stable frame timing and hand interaction (hardware gate), or the decision to defer XR past 1.0 is recorded with its blocking reasons.

## Continuous test and CI plan

### Unit tests

- [ ] Transform composition and decomposition.
- [ ] Hierarchy propagation and cycle rejection.
- [x] Material default consistency and copying.
- [ ] Stable generational asset handles.
- [ ] Scene round trips and schema migration.
- [ ] Asset path canonicalization and deduplication.
- [ ] Input action mapping.
- [ ] Quality-profile selection.
- [ ] Stable ECS/GPU `PhysicsId` conversion, generation rejection, and entity mapping.

### Layout and shader tests

- [x] Rust physics storage-buffer sizes and offsets.
- [x] Common instance field order across compute shaders.
- [x] Common physics push-constant declaration across compute shaders.
- [ ] Shader reflection compared with every Rust storage, uniform, vertex, and push-constant type.
- [ ] Shader compilation as a dedicated CI job.

### Renderer integration tests

- [ ] Resize, minimize, restore, and zero-sized surface handling.
- [ ] Unsupported present-mode and format fallback.
- [ ] Frames-in-flight reuse and validation-clean shutdown.
- [ ] Asset unload/reload while referenced by submitted frames.
- [ ] Culling enabled/disabled equivalence.
- [ ] Automatic primitive/imported-mesh bounds contain their complete source geometry.
- [ ] GPU-owned objects are culled from the current GPU transform without full-state CPU readback.
- [ ] Objects outside the camera stop producing render work but continue physics simulation.
- [ ] `CullingMode::Auto` skips culling overhead below its tested scene-size threshold.
- [ ] Capability fallback behavior on the low-end baseline.

### Golden-image tests

- [ ] Primitive normals.
- [ ] glTF texture channels and color spaces.
- [ ] Alpha mask and blend modes.
- [ ] Directional shadow depth.
- [ ] PBR reference spheres.
- [ ] Tone mapping.
- [ ] Editor compositing.

### Physics tests

- [ ] Triggers and collision events.
- [ ] Raycasts and filtered queries.
- [ ] Stable stacking.
- [ ] Tunneling/CCD cases.
- [ ] Fixed-step independence from render frame rate.
- [ ] GPU grid overflow and fallback.
- [ ] GPU event overflow and fallback.
- [ ] Built-in boolean/range conditions and custom shader conditions emit according to `OnEnter`, `OnExit`, `WhileTrue`, `Once`, and cooldown modes.
- [ ] Condition events reach the correct ECS entity with their registered event ID and requested payload.
- [ ] CPU-to-GPU commands reject stale body generations.
- [ ] Asynchronous selected-state readback reports its source tick and frame age.
- [ ] Explicit CPU/GPU simulation ownership and synchronization modes.

### Physics scenario and benchmark suite

- [ ] Joint, articulation, character-controller, vehicle, and ragdoll regression scenes with recorded expected results.
- [ ] Soft-body, cloth, rope, fluid, granular, and fracture scenarios under lavapipe.
- [ ] Two-way coupling scenarios between rigid, deformable, and fluid simulation.
- [ ] 2D physics scenarios mirroring the 3D suite.
- [ ] Comparative benchmark harness against Jolt, PhysX, Rapier, and Godot Physics with stored baselines.

### Engine feature tests

- [ ] Reflection round trip for every registered component and data asset.
- [ ] Prefab override propagation, nesting, variants, and cycle rejection.
- [ ] Observers and scene-file event connections fire the registered handlers.
- [ ] Animation sampling, blending, IK, and ragdoll handoff are deterministic.
- [ ] Offline audio render compared against reference buffers.
- [ ] UI layout, text shaping, theming, and localization golden images.
- [ ] 2D sprite, tilemap, and 2D lighting golden images.
- [ ] Navigation baking, re-baking budget, and agent avoidance.
- [ ] Networking RPC, replication, prediction, and rollback over loopback with simulated loss.
- [ ] Export smoke test for every supported platform preset.
- [ ] Every demo project and template builds and runs headless.

### Determinism tests

- [ ] Per-tick world-state hashes match between two runs with identical inputs.
- [ ] Hashes match between debug and release builds.
- [ ] Hashes match across differing CPU worker-thread counts.
- [ ] Hashes match across at least two GPU vendors.
- [ ] A recorded replay reproduces its original hash sequence.
- [ ] An injected divergence is reported with its first divergent tick and body.
- [ ] Disabling audio, particles, and all presentation systems does not alter hashes.

### Networking tests

- [ ] Full match completes at 200 ms latency with 2% packet loss without desync.
- [ ] Delta snapshot encoding and decoding round-trip correctly.
- [ ] Bandwidth stays within the per-client budget during maximum destruction.
- [ ] Mid-match disconnect and reconnect resynchronizes correctly.
- [ ] Protocol version mismatch is rejected at connect time.
- [ ] A client reporting an impossible position or terrain state is rejected by the server.

### Terrain and navigation tests

- [ ] Chunk fracture is deterministic across runs and vendors.
- [ ] Debris settling restores static collision and navigation.
- [ ] The debris budget cap never drops bodies non-deterministically.
- [ ] Unsupported spans collapse; supported spans do not.
- [ ] Navigation rebuild stays within its per-tick budget during a cliff collapse.
- [ ] Agents re-route correctly when a route is closed mid-path.
- [ ] Fully blocking every route is reported rather than producing stuck agents.
- [ ] One-hour destruction soak test holds frame time, memory, and debris count within budget.

### Agent-surface tests

- [ ] Every diagnostic code is unique, registered, and has an `explain` entry with a tested example.
- [ ] Every example in the schema catalog, API index, skill file, and generated `AGENTS.md` runs and passes.
- [ ] The generated skill file and `AGENTS.md` match the current schema and API index.
- [ ] JSON Schema files validate every scene, patch, and scenario in the repository.
- [ ] Every sample passes `rusting lint`, its invariants, a 100-seed fuzz run, and the scenario strength check.
- [ ] CLI, daemon, and MCP adapter return identical results for the same operation.

### CI matrix

- [ ] Linux software Vulkan runner (lavapipe) running the `gpu-tests` feature on every pull request. This is the primary GPU verification path; hardware runners confirm it, they do not replace it.
- [ ] Linux hardware runner.
- [ ] Windows hardware runner.
- [x] Formatting, strict clippy, unit tests, and docs on Linux and Windows for every pull request.
- [ ] Scheduled validation and performance runs with stored artifacts.
- [ ] Second-vendor GPU runner dedicated to cross-vendor determinism verification.
- [ ] Scheduled soak-test job for long-running destruction and memory growth.
- [ ] Scheduled headless batch-match job producing aggregate balance reports.
- [ ] macOS (MoltenVK) runner.
- [ ] Android build job.
- [ ] Scheduled comparative physics benchmark job publishing its report as an artifact.
- [ ] Scheduled agent benchmark job (Milestone L9) publishing its report as an artifact.

## Cross-cutting engineering rules

- Public fallible operations return structured `Result` values; panics are reserved for internal invariant violations.
- ECS entities and typed asset handles are the only canonical identities exposed to gameplay/editor code.
- GPU resource destruction is deferred until all referencing frames complete.
- No fixed-capacity structure may silently drop work.
- Optional Vulkan features always have a documented fallback or a clear startup error.
- Debug builds enable validation by default when layers are available.
- Runtime and editor code must expose useful diagnostics instead of relying on ad-hoc FPS/debug printing.
- Every milestone must leave the project compiling and its completed acceptance gates automated where practical.
- Simulation state and presentation state are separate. Presentation code may read simulation state and may never write it.
- Anything affecting simulation must be deterministic: no wall-clock time, no frame-rate dependence, no unseeded randomness, no order-dependent accumulation.
- Network code never trusts a client for authoritative state.
- Systems that run for hours need soak tests. Correct for one minute and degrading over one hour is not correct.
- Anything that moves things — animation, particles, characters, vehicles, navigation obstacles, audio occlusion — integrates with physics through the typed bridge rather than a parallel ad-hoc simulation.
- Physics features are judged against the best standalone physics engines, with benchmark evidence. Other features are judged against Godot parity.
- Every user-facing feature ships with documentation and a demo scene in the same change or the next one.
- Every user-facing feature is agent-ready when it lands: a schema entry with defaults, units and an example; a Rust API index entry; diagnostic codes for its errors; and a way to observe it in a scenario, trace, or capture.
- Every error tells the reader what to do next. An error that only says what went wrong is incomplete.
- Nothing in the engine calls an AI model. Agent support is tools, formats, and documentation that work offline and without any account.

## Recommended implementation order

Work on one vertical path at a time rather than creating empty crates for every eventual subsystem.

Foundation (Milestones 0-7):

1. Finish Vulkan/winit modernization and validation.
2. Add the workspace plus `rusting-core`, ECS schedules, components, and compatibility facade.
3. Add typed assets and static scene serialization.
4. Add render extraction and correct frames-in-flight synchronization.
5. Complete the forward PBR pass schedule and profiling.
6. Build the self-written hybrid physics bridge: stable IDs, GPU events, CPU commands, selective readback, and mixed CPU/GPU ownership.
7. Add the minimal egui shell, viewport, hierarchy, and inspector.
8. Build the vertical slice while filling in editor, rendering, asset, and physics gaps.
9. Add hot reload, undo/redo, polish, packaging, and performance gating.

Physics leadership and Godot parity (Milestones 8-23):

10. Establish determinism: simulation math, ordering rules, per-tick state hashing, and the verification harness. Every later solver depends on these rules.
11. Add reflection, prefabs, observers, data resources, and Rust hot reload. Every later editor and animation feature depends on reflection.
12. Complete rigid-body physics: joints, articulations, characters, vehicles, ragdolls, fields, CCD, and 2D physics.
13. Build the physics debugger and the comparative benchmark harness early, so every later solver is measured from its first commit.
14. Add deformables, fluids, granular media, fracture, and million-body GPU scale.
15. In parallel, reach parity in advanced rendering, animation, audio, runtime UI, 2D, and navigation, integrating each with physics as it lands.
16. Add networking with prediction and deterministic rollback.
17. Grow editor parity, platforms/export, and documentation continuously, closing them out at the 1.0 release gate.

LLM-native development (Milestones L1-L10), in parallel with the above and first while the current focus holds:

- Machine-readable surface and diagnostic codes (L1), then observability (L3) and verification (L5), since these remove guesswork from every later task.
- Start the agent benchmark (L9) as soon as L1 lands, with the tools that exist, so later milestones are measured from their first commit.
- Then the generation-friendly API (L2), the daemon and editor bridge (L4), quality lints and recipes (L6, L7), the fast logic lane (L8), and large-project support (L10).

Engine expansion (Milestones 30-35), after Milestone 9: gameplay framework (30) first, since every sample needs it; then terrain and large worlds (31), procedural content (33), cinematics (32), modding (34), and XR (35).

Sundering (Milestones 24-29):

18. Add competitive authority on top of the engine networking, keeping clients as renderers until determinism is proven.
19. Build chunked destructible terrain, structural load, and Scar persistence on the engine fracture system.
20. Add navigation and GPU crowd agents over terrain that changes every tick.
21. Add the competitive gameplay framework: teams, abilities as forces, dynamic vision, and bots.
22. Build the Sundering slices in order, adding balance and operations tooling as each slice requires it.

The next concrete editor task is a Shortcuts settings panel. It must display the central action map, let a user rebind one action at a time, detect duplicate bindings, and persist choices inside project editor settings. Then add focus-selection and interactive translate/rotate/scale handles. Explicit frame contexts and an offscreen scene viewport target remain the next renderer-architecture task.

### Review-fix pass verification

- fmt, clippy (default, `--no-default-features`, `--features rusting_engine/gpu-tests`) clean.
- `cargo test --workspace`: 457 lib tests pass; with `rusting_engine/gpu-tests`: 533 pass, 0 failed.
- New tests: fixed GPU box solid to CPU queries, switched-off fluid surface reaped, scene `InputAction` unbinds, mistyped scene setting errors, huge fluid block cap.

### Look and feel track

Goal: games made by agents look less cheap. Items A to G from the 2026-10-04 audit.

- [x] A. Mipmaps and anisotropic filtering. Already present: textures upload with a full mip chain and linear minification filters anisotropically when the device enables `samplerAnisotropy`. Only the stale comment above `texture_sampler` changed.
- [x] B. Polish loop and art checklist. `docs/look-and-feel.md` (`rusting docs guide/look-and-feel`) covers the capture, look, critique and fix loop, plus a checklist for silhouette, palette, value contrast, warm key against cool fill, materials, shapes, HUD, and motion and feedback. `skills/rusting-game/SKILL.md` gained a Polish step, and the project `AGENTS.md` template mentions the loop.
- [x] C. Color grading and palettes. `rusting.color_grading` (contrast, saturation, shadow and highlight tints, vignette) runs after tone mapping. Presets: `dark_interior` and `bright_stylized` are new, every preset applies color grading, and `preset list --json` prints each preset's five-color palette. The new component is in the schema catalog and the editor's Environment components. Tests: `hdr_values_above_one_survive_until_tone_mapping` (grading assertions, GPU) and the `art_direction` tests.
- [x] D. Mesh kit. New `RoundedCube` and `Capsule` primitives. Cylinder and cone are now smooth shaded, built on a shared lathe. The sphere was inside out and now winds outward, which also fixes lighting on ball pits and other sphere crowds. The guide has a ready humanoid patch (`Hero`), checked by capture. Tests: `primitives_wind_outward_with_unit_normals` (CPU) and `every_primitive_shows_its_outside_to_the_camera` (GPU, Normals debug view).
- [x] E. Free models. The guide lists CC0 sources (Kenney, Quaternius, Poly Pizza, Poly Haven, ambientCG), license rules, and `asset import --license/--author/--url`. New `rusting scene add-model <scene> <model> [--name N] [--dry-run]` places a glTF as one named parent object, with a child per node and the model's materials. The `MODEL_IMPORT` diagnostic covers import failures. A house model CDN is noted as planned and does not exist yet. Tests: `a_gltf_model_lands_in_a_scene_under_one_named_object` and the schema and CLI catalog tests. Checked end to end: import, add-model, validate and capture.
- [x] F. Render to texture. `rusting.camera_screen` (`camera` name, `size`) on a mesh shows that camera's view in place of the material's base color and emissive maps. `SceneRenderer::render_screens` draws each feed with its own child renderer before the frame. `render_game` (game window and captures) calls it. Screen materials batch alone. Tests: `screens_find_their_camera_by_name` (CPU) and `a_screen_material_shows_what_its_camera_sees` (GPU). Checked by a `rusting capture` of a monitor showing a second camera's side view. Limits, also listed in `docs/cameras.md`: one full child renderer per screen, no screens inside feeds, no feeds in the editor viewport, and no egui images.
- [~] G. Small distant objects and noisy crowds. Partial. The sphere winding fix made the `w4_cctv` ball pit lit correctly, and the guide warns against random colors in crowds. LOD groups (`.rlod`) already exist. Deferred: TAA and impostors.
- Full check for this track: fmt and clippy (default, `--no-default-features`, `--features gpu-tests`) clean. `cargo test --workspace` passed, lib 532. `cargo test --workspace --features rusting_engine/gpu-tests` passed: lib 616, cli 25, doctests 17. A first run of that suite was stopped after 30 minutes with no output. Every test binary then passed separately in seconds, and the full rerun took a few minutes, so the stall was not reproduced.

### Effects and animation track

Goal: games feel alive. Particles, effects and animation, each with editor GUI. Items 1 to 7 are from the 2026-10-04 brief.

- [x] 1. Particle emitter v2. `rusting.particle_emitter` (`src/runtime/particles.rs`) has these settings:
  - Emission: rate, bursts, max particles, prewarm, duration and looping.
  - Shape: point, box, sphere, cone or circle, with a size.
  - Random `[min, max]` ranges for lifetime, speed, size, rotation and spin.
  - Motion: direction and spread, gravity, drag, wind and turbulence.
  - Over life: size curve, color and alpha gradient, emissive, fade in and fade out.
  - Rendering: world or local space, billboard or velocity streak, alpha or additive blend, and a soft, disc or square sprite.

  Live particles sit in a `ParticleSystem` component next to the emitter, never one entity per particle. The renderer draws each emitter in one instanced draw. Alpha batches and their particles are sorted far to near; additive batches draw last. Random values come from `RandomSeed` keyed by the emitter's `SceneId` and spawn number and indexed by the fixed tick. Game code uses `scene.particles(name, ParticleCommand::…)`, and `scene.trigger` restarts the emitter. `rusting.burst_emitter` is unchanged. The component is registered in schema, reflect, snapshot and scene_file, and has editor placement help. Docs: `docs/effects.md` (`guide/effects`), `api/ParticleEmitter`, the look-and-feel checklist, SKILL.md and the project AGENTS template.
  - Tests: 9 simulation tests in `runtime::particles::tests` (rate and lifetime, seeding, bursts and max, looping bursts, shapes, spread and gravity, curves and fades, local space, prewarm), plus the GPU test `particles_render_with_their_color_and_size` (coverage, alpha color, additive sum).
  - Full check: fmt and clippy (default, `--no-default-features`, `--features rusting_engine/gpu-tests`) clean. `cargo test --workspace` passes with 541 lib tests; with gpu-tests, 626 lib tests and 25 CLI tests pass.
- [x] 2. Effect presets. `rusting effect list` and `rusting effect apply <scene> <effect> [--on OBJECT | --name N --at X,Y,Z] [--dry-run]` (`src/runtime/effect_presets.rs`) add these presets:
  - Ambient: dust_motes, fireflies, magic_sparkle.
  - Weather: falling_leaves, snow, rain.
  - Fire: fire, embers, smoke.
  - One-shot bursts: sparks, confetti.

  The emitter gained `start_colors`, a per-particle tint picked from a list, for confetti and mixed leaves. The new commands are in the schema catalog, with the `EFFECT_UNKNOWN` diagnostic. Docs: the preset table in `guide/effects`, SKILL.md, the project AGENTS template and look-and-feel.
  - Tuning: each preset was captured with `rusting capture` (640x360, tick 120, or tick 12 to 20 for bursts) over a night or daylight ground scene, and the images were reviewed. Fixes from that review:
    - Emissive was 5 to 8, which clipped additive colors to white. It is now 1 to 2.5, so sparks, embers and fireflies keep their hue.
    - Fireflies, embers and sparkles were too small to read at 7 m, so they are bigger.
    - Leaves are now dark square tumbling sprites, not bright discs.
    - Rain is thinner and more transparent.
    - Fire is wider and softer, with low alpha, so it reads as a flame instead of a white blob.
  - Tests: `presets_have_unique_names_and_round_trip_as_json`, `start_colors_tint_each_particle_with_one_of_them`, CLI `effect_presets_list_and_apply_as_scene_patches`.
  - Full check: fmt and clippy ×3 clean. `cargo test --workspace` passes with 543 lib tests; with gpu-tests, 628 lib tests and 26 CLI tests pass.
- [x] 3. Particle editor GUI. The Inspector draws `rusting.particle_emitter` with its own editor (`src/editor/inspector/particles.rs`), built from the shared inspector widgets and `EditorTheme` colors:
  - A preset dropdown over `EFFECT_PRESETS`, and Play, Pause, Restart and Stop buttons with the live particle count.
  - Collapsible sections: Emission (with a bursts list), Shape, Lifetime, Velocity, Size, Color and Rendering.
  - A min–max range row. Rotation and spin show in degrees.
  - A size-over-life curve plot: click to add a key, drag a dot to move it, and edit or remove keys in the rows below.
  - A color-over-life gradient bar over a checker: click to add a key with the shown color, drag a marker to move it, with color rows below.
  - A start-tint list.

  Setting edits are `ComponentEdit::Set`, so a drag or click closes into one snapshot Undo step. Transport buttons set `EditorState::particle_command`, which is applied to the live emitter and is not a scene edit. Live preview: `update_edit_preview` (called by the editor binary before each runtime update) steps emitters in Edit mode through `runtime::preview_particles`, with its own fixed-step clock capped at four steps a frame. The editor redraws continuously while a preview plays. Starting Play calls `reset_particles`, so the game's emitters start fresh. `update_particles` and the preview share one `advance` function. Deferred work (a standalone particle area, key box selection, a shape gizmo, project presets) is in `docs/editor-overhaul.md`. Docs: "In the editor" in `guide/effects`.
  - Visual check: an offscreen render of the Inspector with the fire preset was reviewed. It used a temporary capture test that was then removed.
  - Tests: `particles::tests` covers sorted key insertion that keeps the curve, presets replacing every setting, and every section and the transport drawing. In `editor::tests`, `particle_inspector_edits_are_one_undo_step_each` clicks "+ Add Burst" through egui input, checks one Undo step, then undoes. `particles_preview_while_stopped_and_transport_is_not_an_edit` checks Edit-mode preview, Pause through the Inspector with no Undo and no dirty scene, and that Play resets the preview and stops it.
  - Full check: fmt and clippy ×3 clean. `cargo test --workspace` passes with 548 lib tests; with gpu-tests, 633 lib tests and 26 CLI tests pass.
- [x] 4. Keyframe animation. `rusting.animation` (`src/runtime/animation.rs`) holds named clips, with `autoplay` and `speed`:
  - A clip has `duration` (0 uses the last key), `repeat` (Once, Loop or PingPong, the `TweenRepeat` enum), tracks and `{time, name}` event markers.
  - A track targets the entity or a child name path (`Arm/Hand`). It writes Position, Rotation, Scale, Color, Emissive, Visible, or `Field {component, path}`, which is any numeric field of a registered component through `set_registered_component_field`.
  - Interpolation is Step, Linear or Smooth (Catmull-Rom).

  An `AnimationPlayer` component is added next to each animation. It holds the current clip, the fading clip, and the material copies it owns. Color and emissive tracks give the target one material copy, then edit it in place only when the value changes, so the renderer re-uploads only on change. The exclusive system `advance_animations` runs on FixedUpdate after `advance_tweens`. It steps by `fixed_delta`, applies writes in (SceneId, entity) order, and sends `AnimationEvent{tick, entity, object, clip, name}` in that order, sorted by time within a tick. Markers fire on half-open windows; a Once clip's end is closed. Crossfade lerps tracks that match on target and property. Game API: `play_animation`, `crossfade`, `stop_animation`, `set_animation_speed`, `is_playing`, `animation_events`. `step_animations`, `apply_preview_pose` and `find_target` are public for the editor timeline. The component is registered in schema (with a round-trip example), reflect, snapshot (component, player and event queue), scene_file, placement help and `api/Animation`. Docs: new `docs/animation.md` (`guide/animation`), SKILL.md, the project AGENTS template and look-and-feel.
  - Tests: 8 in `runtime::animation::tests`: interpolation modes, repeat mapping, autoplay with Once, markers once per pass, crossfade with Stop holding the pose, a color copy with no re-upload when unchanged, child target visibility, and a bitwise-equal pose over two runs. The App test `animations_run_on_the_fixed_step_and_send_marker_events` covers the FixedUpdate chain, a Field track on `rusting.tween /duration`, and event delivery through `EventQueue`.
  - Full check: fmt and clippy ×3 clean. `cargo test --workspace` passes with 557 lib tests; with gpu-tests, 642 lib tests and 26 CLI tests pass.
- [x] 5. Editor animation timeline. `EditorPanel::Timeline` (`src/editor/timeline.rs`) is a Blender-style dope sheet for the selected object's animation, or its nearest animated parent:
  - The header has a clip picker, add, delete and rename, repeat, length (auto), Autoplay, ⏮/Play/Pause/Stop, the time, ● Record, an Insert Key menu (Location, Rotation, Scale, All), Delete Key and + Event.
  - The body has a ruler that scrubs, event markers, one row per track with delete and an interpolation cycle, diamond keys that select and drag (snapped to 1/60 s, re-sorted), and the playhead.
  - Clip edits return as `ComponentEdit::Set` on `rusting.animation`, so they coalesce into one snapshot Undo step. Add Animation is a `ComponentEdit::Add`.
  - `update_timeline` runs after edits in Edit mode while a Timeline area is shown. It poses the subtree with `apply_preview_pose` (transform and visibility only) when the clip, time or clip data changes. The authored pose is kept in the runtime resource `PreviewRestPose`, which `scene_document` writes instead of the posed values, so saves and Undo snapshots never contain a scrub. The rest pose is put back when the selection leaves the animation and before Play. Undo/Redo drop it and re-pose.
  - Record mode compares each posed transform to what the pose wrote. A changed position, rotation or scale keys only that channel at the playhead, at the object's child name path. The gizmo or Inspector edit already took the Undo snapshot, so the key undoes with it. With Record off, the edit updates the rest pose. Playback requests continuous redraw.
  - Deferred items (zoom and pan, marker drag, box select, curve editor, color and field preview, scoped Delete key) are in `docs/editor-overhaul.md`. Docs: "In the editor" in `docs/animation.md`.
  - Tests: in `editor::tests`, `timeline_adds_animation_and_keys_as_one_undo_step_each` clicks + Add Animation and Insert Key › All through egui input, checks two Undo steps, then undoes the keys. `timeline_scrub_poses_but_scene_documents_keep_the_rest_pose` checks a posed x = 1 at 0.5 s, a document x = 0, no Undo step, and restore when the selection is cleared. `timeline_record_mode_keys_transform_edits_at_the_playhead` checks one Position key [1, 3, 0] at 0.5 s, no rotation or scale tracks, and a dirty scene.
  - Full check: fmt and clippy ×3 clean. `cargo test --workspace` passes with 560 lib tests; with gpu-tests, 645 lib tests and 26 CLI tests pass.
- [x] 6. glTF node animation import. `AssetServer::import_gltf_animations(path, &nodes)` (`src/assets/mod.rs`) reads every glTF animation into an `AnimationClip` (Loop, auto length) for a root whose children are the glTF top nodes:
  - Translation and scale channels become Position and Scale tracks. Rotation becomes a track of the new `AnimationProperty::Orientation` (quaternion `[x, y, z, w]`), with the sign of each key flipped to the shortest arc from the previous key. `apply` normalizes the blended quaternion and writes Euler `Rotation`, so imported spins do not flip at ±180°.
  - STEP, LINEAR and CUBICSPLINE map to Step, Linear and Smooth. Cubic spline keys keep the value and drop the tangents. Morph target weight channels are skipped.
  - Tracks target the node name path. `scene add-model` and the editor's `add_model_to_scene` put the clips on the new root as `rusting.animation`, and the first clip autoplays. The editor maps renamed (de-duplicated) node names into the paths.
  - `add_model` now builds its Create operations with `scene_patch::entity_form`, so registered components go in as JSON objects. Before, the JSON strings failed validation; no model had a registered component until now.
  - Sample: `samples/vertical_slice/windmill.gltf` (`generate_windmill.py`, standard library only) is a tower with a Rotor (5 linear quarter-turn rotation keys) and a Flag (3 cubic spline translation keys). Docs: "Imported clips" and the Orientation row in `docs/animation.md`, the add-model step in look-and-feel, the sample README, SKILL.md, the project AGENTS template and the schema summary.
  - Tests: `add_model_keeps_gltf_node_animation_as_a_clip` (`tests/vertical_slice_environment.rs`) runs `cli::add_model` into a saved scene, reloads it, and checks autoplay "spin", a length of 4 s, tracks (`Tower/Rotor` Orientation ×5, `Tower/Flag` Position ×3), Smooth with the cubic value [0.5, 2.0, 0], and the rotor at π/2 after 1 s of fixed ticks. `editor::tests::added_model_clips_follow_renamed_nodes` adds the windmill twice and resolves the second copy's renamed rotor path.
  - Full check: fmt and clippy ×3 clean. `cargo test --workspace` passes with 561 lib tests; with gpu-tests, 646 lib tests, 26 CLI tests and 3 vertical slice tests pass.
- [~] 7. Skeletal skinning. Done on the CPU; GPU vertex skinning is not done (plan below).
  - Import: `import_gltf` reads `JOINTS_0` and `WEIGHTS_0` into `SkinWeights { joints: Vec<[u16; 4]>, weights: Vec<[f32; 4]> }`, cooked with bincode next to the mesh as `.rskin`. When the importer flat-shades a mesh with no normals, the weights are expanded per triangle corner the same way. `import_gltf_scene` reads each node's skin (joint node indices, and inverse bind matrices, identity when missing) into `ImportedGltfNode::skin`.
  - `rusting.skin` (`src/runtime/skinning.rs`) is `Skin { joints, inverse_bind }`. Each joint is a name path from the skinned object, with `..` for the parent (`find_target` now accepts `..`), so it survives saving and reparenting the model root. `gltf_node_skin` builds the paths from the current node names. `spawn_gltf_nodes_in_world` inserts it, and the editor's Add to Scene rebuilds it after de-duplicating names, like the clip paths. Nodes with no shared ancestor meet at the spawn root. Registered in scene_file, reflect, snapshot, schema (with example), placement help and `api/Skin`. The Assets panel hides `.rskin` like other cooked files.
  - Runtime: `update_skins` runs at the start of `extract_render_world`, so it covers the game and the editor (Timeline scrubs of joints included). For each skinned object it computes `inverse(object global) × joint global × inverse bind` per joint. When any matrix changed, it bends the cooked mesh on the CPU with linear blend skinning (weights normalized, normals by the inverse transpose, tangents by the linear part) into a private mesh copy held by the unsaved `SkinnedMesh` component. Render extraction draws that copy in place of `MeshRenderer.mesh`, so saves keep the cooked mesh. Later frames rewrite the copy through `Assets::get_mut`, so the renderer re-uploads it, without re-collecting renderables. A missing `.rskin` draws the mesh unbent.
  - Skeletal clips: joints are glTF nodes, so item 6 already imports their channels as Orientation, Position and Scale tracks on joint paths. No extra code.
  - Sample: `samples/vertical_slice/bending_bar.gltf` (`generate_bending_bar.py`) is a 2 m bar with three vertex rings: Root weights, a 50/50 split, and Tip weights. Its "bend" clip turns Tip a quarter turn around Z in 1 s. Docs: "Skinned meshes" in `docs/animation.md`, look-and-feel, SKILL.md, the project AGENTS template and the sample README.
  - Tests: `runtime::skinning::tests::joints_bend_vertices_and_normals_in_the_mesh_space` checks a `../Bone` path, glTF bind-space placement, weight normalization, normal rotation, and that a vertex with no weight does not move. `add_model_keeps_gltf_skin_and_bends_the_mesh` (`tests/vertical_slice_environment.rs`) runs `cli::add_model`, reloads the scene, and checks the joint paths `../Root` and `../Root/Tip`. It then plays the clip and checks that the drawn copy folds to x ≈ −1 while the cooked mesh stays within ±0.1.
  - Test `editor::tests::added_model_skin_joints_follow_renamed_nodes` adds the bar twice in the editor and checks that the second copy's renamed joint paths resolve.
  - Full check: fmt and clippy ×3 clean. `cargo test --workspace` passes with 563 lib tests. With gpu-tests, 648 lib tests, 26 CLI tests and 4 vertical slice tests pass. One earlier gpu-tests run stalled in `vertical_slice_lighting_combines_shadow_point_sky_and_blend` under parallel lavapipe load. That renderer-only test does not touch skinning. It passed alone in 0.44 s, and two later full runs passed.
  - Follow-up batch (2026-10-04), done:
    - Extra primitives of a skinned node. `insert_gltf_skins` gives every child part the node's skin with each joint path prefixed by `../`; the editor's Add to Scene calls the same function. Test: `assets::tests::every_primitive_of_a_skinned_node_gets_the_skin`.
    - Morph targets. `import_gltf` cooks a primitive's targets to `.rmorph` (positions and normals, expanded per corner on the flat-shaded path). `rusting.morph` is `Morph { weights }`, taken from the node or mesh default weights; child parts read their parent's weights. glTF `weights` channels import as a Field track on `rusting.morph` `/weights`. `update_skins` applies the weighted offsets first (normals renormalized), then skinning. Sample: `samples/vertical_slice/squash_box.gltf` (`generate_squash_box.py`), a box and hat with "squash" and "lean" shapes and a "wobble" clip. Docs: "Blend shapes" in `docs/animation.md`. Test: `add_model_keeps_gltf_morph_weights_and_reshapes_every_part` (both parts move with the weights after 60 ticks).
    - Memory. Deformed copies are tracked in `AssetServer` and freed with `Assets::remove` when the skin and morph go, the renderer mesh changes, or the entity despawns. A `SkinnedMesh` restored from a snapshot without a tracked copy is dropped and rebuilt. Test: `runtime::skinning::tests::copies_are_freed_when_the_skin_goes_or_the_entity_despawns`.
    - Editor skeleton overlay. `overlay::bone_shapes` gives each joint of a visible skin a small cross and a line from its parent joint. The Scene View draws them in front of meshes (cyan, selection colors when selected), and a click near a bone selects the joint before any mesh. Test: `editor::tests::added_model_skin_joints_follow_renamed_nodes` now also checks 4 joints for two bars and a Tip bone line. Not checked visually.
    - Full check: fmt and clippy ×3 clean. `cargo test --workspace` passes (565 lib tests). With gpu-tests: 650 lib, 26 CLI, 5 vertical slice, 17 doc tests pass.
  - Not done, in order:
    1. GPU skinning. Add `joints: [u16; 4]` and `weights: [f32; 4]` as a second vertex buffer, bound only for skinned draws. Add a joint-matrix SSBO with one slice per skinned instance; the instance carries its slice offset. Give the forward, depth-prepass, shadow and picking pipelines in `scene_renderer.rs` a skinned variant of each vertex shader. Drop the CPU copy when the GPU path is on. Verify with a gpu-tests render of `bending_bar.gltf` that compares bent pixels with the CPU path.
    2. Skinned bounds. Culling uses the bent copy's bounds today; the GPU path needs bounds per joint or a padded bind-pose box.

### FOREVER BEAR track

Goal: the engine features the horror game FOREVER BEAR needs (night-shift booth, six CCTV monitors, about 2,000 shelf bears with pitched voices, a mascot that hunts by sound). Gap list from reading `src/runtime/audio.rs`, `docs/audio.md`, `docs/cameras.md`, `docs/effects.md`, `docs/look-and-feel.md`, CHANGELOG 2.0.3 and this file on 2026-10-06. Audio stays presentation only: no audio state feeds the simulation or the replay hashes.

- [x] 1. Sound rate. `rate` on play, `set_sound_rate(id, rate, fade)`, `/playing` reports the rate. Tests: `half_rate_doubles_the_end_tick` (unit), scenario `rate_pause_resume_and_seek_show_in_playing` (rate 0.5). Docs: "Speed and pitch" in `docs/audio.md`.
- [x] 2. Moving 3D sounds and the listener. `set_sound_position`, sounds attached to an entity, an explicit listener entity. Tests: `spatialize_pans_to_the_side_and_falls_off_past_two_metres`, scenario `an_attached_sound_pans_as_its_entity_moves_and_the_listener_can_move`. Docs: "Moving sounds and the listener".
- [x] 3. Bus effects with kira 0.12 only: `set_bus_effect(bus, LowPass | Reverb | Distortion, fade)`, shown in the audio report. Tests: `bus_voice_limits_clamp_and_inactive_effects_report_off`, scenario `bus_effects_show_change_the_mix_and_leave_state_hashes_alone` (state hashes equal with and without the effects). Docs: "Bus effects".
- [x] 4. Voice limits and `priority: u8`. Scenario `a_thousand_sounds_in_one_tick_stay_within_the_voice_limit`: 1,001 requests, 32 voices, 968 dropped and 1 stolen, no panic. Docs: "Voice limits".
- [x] 5. Pause, resume and seek, with the position in `/playing`. Scenario `rate_pause_resume_and_seek_show_in_playing` pauses at 2.0 s and resumes from 2.0 s. Docs: "Pause, resume and seek".
- [x] 6. Camera screen cost. `rusting.camera_screen` gained `update_every`, `enabled` and `grading`; screens out of every view frustum keep their last image; a new feed always draws once. Root cause of the cost: every feed re-ran GPU physics; feeds now reuse the frame's prepared physics (GPU bodies in feeds lag one frame). GPU test `a_screen_material_shows_what_its_camera_sees` covers update_every 2, enabled false, off-screen, and a wall of six monitors (six on the first frame, then three per frame). Numbers (RTX 3060, `render_bench` balanced 1920x1080, 600 frames, mean / p95): six 320x180 screens 134.91 / 151.07 ms before, 24.86 / 26.48 ms after; base scene 22.18 / 24.54 ms. Docs: "Keeping screens cheap" in `docs/cameras.md`.
- [x] 7. CRT/VHS look. `ColorGrading` gained `grain`, `chromatic_aberration`, `scanlines`, `color_bleed`, `noise_band` and `distortion`, for the scene and per screen. Grain and the band are a hash of pixel and fixed tick, so the same tick gives the same image. Effects that sample neighbours copy the HDR image first. GPU test `a_screen_feed_takes_a_crt_look_with_repeatable_grain` with goldens `screen_crt_off.png` and `screen_crt_on.png`, the same tick identical and the next one different, and the scene grading repeatable. Docs: "Film and CRT/VHS effects" in `docs/look-and-feel.md`.
- [x] 8. Many instanced objects. Scaling holds at 60 fps without a new path, so the per-instance custom value was skipped. `render_bench` gained `--bears`, `--bear-updates`, `--screens`, `--screen-size`, `--screen-every`, `--no-bodies` and `--extent`. Without GPU bodies: 5,000 bears 7.40 / 8.20 ms; 5,000 with 500 moved per frame 8.81 / 9.36 ms; 10,000 with 500 moved 12.37 / 13.38 ms; 5,000 + 500 moved + six screens 16.47 / 18.96 ms, 12.37 / 13.47 ms with screens every other frame. Scenario test `five_hundred_of_five_thousand_bears_move_in_one_tick`. Docs: "Crowds of one object" in `docs/look-and-feel.md`.
- [x] 8a. The benchmark scene's 1,000 GPU bodies cost about 18.6 ms of GPU time per frame on the RTX 3060 (22.18 ms with them, 3.06 ms without). Profile the GPU physics pass. Cause: pass timestamps put 20.83 of 23.27 ms GPU time in Physics, and dropping `CONTACT_ROUNDS` to 0 left 2.6 ms. The contact grid counters were small (about 80,000 fallback tests a frame). Skipping the oversized-body loop alone brought the frame to about 4 ms. The ground body is too big for one cell, so a single GPU thread tested all 1,000 bodies one after another, in each of the four contact rounds. Fix: `physics_contacts.comp` gained `OVERSIZED_PASS`. The grid pass files oversized bodies in their own list; the contact pass writes an indirect dispatch of one workgroup per body (capped at 65,535, the rest loop). Each workgroup splits the bodies across 256 threads and merges max, min and the touch flag in shared memory. Evidence: final poses of `gpu_bodies_collide_with_each_other_through_grid_and_fallback` (a dynamic oversized sphere) and of `benchmark_scenes_run_on_the_gpu_and_repeat_exactly` (Debris, Mixed) are byte-identical to the old shader on the RTX 3060 and on llvmpipe, and the same on both devices. That test now expects `3 * CONTACT_ROUNDS + 2` dispatches. Numbers (RTX 3060, balanced 1920x1080, 600 frames, two runs, mean / p95 frame, then GPU mean): base 5.63-6.27 / 10.83-13.07 ms, GPU 3.93-4.16 ms, Physics 1.60-1.82 ms (was 22.18 / 24.54 ms, Physics 20.83 ms); no bodies 2.57 / 2.80 ms; six screens 8.31-13.14 ms, GPU 3.76-3.91 ms; 5,000 bears 8.57-10.02 ms, GPU 6.11-6.24 ms; 10,000 bears with 500 moved 16.70-19.62 ms, GPU 7.40-7.85 ms; 5,000 + 500 moved + six screens 18.26-20.25 ms, GPU 6.09-6.30 ms. With bears and screens, frame time is now mostly CPU and wait, not GPU, and it varied a lot between runs. Docs: GPU contacts in `docs/determinism.md`. The diagnostic-code scanner now skips the `OVERSIZED_PASS` shader define, as it already skipped `GRID_PASS`. Full check after the fix: fmt clean; clippy clean with default features, `--no-default-features`, `--features gpu-tests` and `--features steam`; `cargo test --workspace` 597 lib; gpu-tests serial on the RTX 3060: 684 lib, 27 CLI, 5 vertical slice, 17 doc tests.
- [x] 9. Text on a mesh. Option B: `text_texture::text_texture(text, TextStyle)` (`ui` feature) lays text out with egui's fonts and copies glyph coverage into a `TextureAsset` on the CPU. Tests: `text_draws_ink_inside_its_padding`, `longer_and_multiline_text_grows_the_texture`, GPU golden `text_reads_on_a_tilted_panel` (`text_panel_tilted.png`, checked by eye). Docs: "Text on meshes" in `docs/menus-and-ui.md`.
- [x] 10. World-space text. Covered by item 9, documented there.
- [x] 11. Captions tied to a sound, with a settings toggle and size. Scenario `captions_show_while_their_sound_plays`. Docs: "Captions".
- [x] 12. Occlusion by a physics raycast lowers volume and adds a low-pass. Scenario `a_wall_between_listener_and_sound_lowers_its_volume` (0.4 to 0.12). Docs: "Occlusion".
- [x] 13. Offline audio render already existed (`audio_out`, RMS and peak checks). New: `audio_reference {path, tolerance}` compares the mix with a stored WAV. Scenario `the_mix_matches_its_stored_reference_within_tolerance`. Docs: "Reference mixes".
- [x] 14. Files over 1 MiB stream. Test `large_files_stream_and_still_mix`. Docs: "Long files".
- [~] 15. Steam. `rusting_engine::steam` (`is_running`, `unlock_achievement`, `set_stat`) compiles in every build and does nothing; the `steam` feature is reserved. Test `calls_do_nothing_without_steam`; clippy clean with `--features steam`.
  - [ ] Blocked: the Steamworks backend needs the owner to approve the `steamworks` crate.
- [~] 16. Windows export. `cargo build --release --target x86_64-pc-windows-gnu --bins` builds. Under Wine 2026-10-06: `game.exe --headless` runs 60 ticks; `game.exe` opens a window, draws its first frame on the RTX 3060 in 1.7 s and opens the WASAPI sound device; the Windows lib test binary passes the 44 `scenario::` tests plus steam, text_texture and waypoints. `rusting.exe doctor --probe` crashed inside Wine's `vkQueueSubmit` (exit 3), while the windowed game rendered, so this looks like a Wine issue.
  - [ ] Check on real Windows: gamepad (Wine stubs Windows.Gaming.Input), the `%APPDATA%\<game>` save folder (scenarios override it with `RUSTING_USER_DATA`), sound output and the GPU probe.
- [ ] 17. Microphone level (`mic_level()`, off by default, never in replay hashes). Blocked: needs the owner to approve `cpal` as a direct dependency.
- [ ] 18. Video on a texture. Blocked: needs the owner to approve a video decoder crate.
- [x] 19. Reverse playback (not for streamed files). Test `reversed_sounds_start_from_the_end`.
- [x] 20. `WaypointGraph`: nodes, two-way edges, `nearest`, shortest `path` (Dijkstra with lowest-ID tie breaks), `disconnect`, `length`. Tests: `shortest_path_avoids_the_detour_and_ties_break_the_same_way`, `nearest_picks_the_closest_node`. Docs: "Waypoint graphs" in `docs/concepts.md`.
- [x] 21. Booth sample `samples/forever_bear_booth`, built with the `rusting` CLI: six CRT monitors on inactive store cameras, 192 shelf bears copied from a template, one bear line at a time at a random rate on a `bears` bus with reverb and a low-pass, captions, "CAM n" plates from `text_texture`, and Big Button walking a `WaypointGraph` route as a pure function of the tick with occluded, top-priority steps. Scenario `tests/night.json` passes: the route at ticks 2, 600 and 1200, 101 bear lines and 41 steps by tick 1200, the reverb on the bus, no dropped voice, a mix WAV whose RMS falls from 0.041 to 0.010 as Big Button walks away, and a booth capture checked by eye. Dogfooding found that `BusEffect`, `Caption`, `WaypointGraph` and `AssetServer` were missing from the prelude although the docs used them with the prelude alone; they are in it now.
- [x] 22. New goldens on lavapipe. `screen_crt_off` differed by up to 185 on llvmpipe: its 8x8 checker was sampled with mipmaps, and the drivers pick different mip levels for it inside the feed. The checker now samples with nearest filtering, and the same goldens pass on llvmpipe and the RTX 3060. `text_panel_tilted` differed by 37 only on glyph edges (mip choice on a minified, tilted texture); that golden now allows 48 per channel, and the test still requires over 200 lit text pixels.
- [x] 23. FOREVER BEAR F13: raycasts and `aim` silently skipped colliders without a `physics_body`. Kept the Godot rule (a shape needs a body) and made it loud: `rusting check`, `validate` and `scene` commands warn `SCENE_COLLIDER_WITHOUT_BODY` with the entity, its `/entities/N/collider` location and the `physics_body` to add; player and platformer controllers are exempt, since their collider is only their own shape. `GameScene::raycast` and "Aiming a camera" in `docs/cameras.md` state the rule. Test: `a_collider_without_a_physics_body_is_reported` (tests/cli.rs). No sample or game scene triggers it. 2026-10-06: fmt clean; clippy clean default, `--no-default-features`, `--features gpu-tests`; `cargo test --workspace` 597 lib, 27 CLI.
- [x] 24. FOREVER BEAR F20: a deadlocked game made `rusting test` wait forever with no output. A scenario run now starts a watchdog thread in the game process. When no tick finishes for 60 s (`RUSTING_TEST_STALL_SECS`, `0` turns it off), it writes a failed report naming the last finished tick and exits with code 1, so `rusting test` reports `SCENARIO_FAILED` ("no tick finished in 60 s; the last finished tick is N ..."). Progress goes through a relaxed atomic that no simulation code reads. Tests: unit `a_stalled_run_reports_its_last_finished_tick`; by hand on 2026-10-06, a copy of `forever_bear_booth` that locks a `Mutex` twice from fixed tick 30 failed in 5.2 s with `RUSTING_TEST_STALL_SECS=5` and "the last finished tick is 29". Docs: `docs/concepts.md`, "Tests you can run without a screen". Not done: no live progress line, since the CLI reads game output only at the end. Full check: fmt clean; clippy clean with default features, `--no-default-features` and `--features gpu-tests`; `cargo test --workspace` passes (598 lib).
- [x] 25. FOREVER BEAR F16: running the game binary (`cargo run`, `target/release/...`) after `scene patch` played the old cooked level with no warning. Loading a project's cooked main scene now compares file times and prints "warning: build/main.rscene.bin is older than scenes/main.rscene ... Run `rusting cook .` first" to stderr. Exported folders (no `Cargo.toml`) are skipped, since copying resets file times. `rusting run` and `rusting test` already cook first. Tests: unit `a_cooked_scene_older_than_its_source_is_reported`; by hand on 2026-10-06, `forever_bear_booth` with a touched scene printed the warning on `RUSTING_HEADLESS_TICKS=5 cargo run`, and was silent after the cooked file was newer. Docs: `docs/getting-started.md`. Full check: fmt clean; clippy clean with default features, `--no-default-features` and `--features gpu-tests`; `cargo test --workspace` passes (599 lib).
- [x] 26. FOREVER BEAR F8: CCTV feeds of dark aisles were near black, and the only fix was lighting the room for the player too. `rusting.camera_screen` gains `exposure` (default 1), which multiplies the scene's tone mapping exposure for that feed only, through `ExtractedScreen` and `SceneRenderOptions::exposure`. Old scenes load unchanged (serde default). Tests: headless Vulkan `a_screen_feed_takes_a_crt_look_with_repeatable_grain` now also checks that exposure 0.25 darkens over 256 channels of the frame and brightens none; all 125 `rendering::` gpu-tests pass serially on the RTX 3060. Docs: `docs/cameras.md`, schema summary and example. Full check: fmt clean; clippy clean with default features, `--no-default-features` and `--features gpu-tests`; `cargo test --workspace` passes (599 lib).
- [x] 27. FOREVER BEAR F9: the `dark_interior` preset's 3000 lux sun made no visible light, and nothing said how lux relates to point lights. The renderer maps 100000 lux to 1 on a white surface facing the light (now `DirectionalLight::LUX_PER_UNIT`, used by the renderer), the same as a point light of 1000 up close or ambient intensity 1; 3000 lux is 0.03. `dark_interior` now uses 30000 lux. Existing scenes keep their values, since presets are patches. Tests: unit `every_preset_sun_lights_a_surface_visibly` (sun / LUX_PER_UNIT x exposure >= 0.25 for every preset; the old value gave 0.042). Docs: `docs/look-and-feel.md` light scale paragraph, `directional_light.illuminance` schema hint. Full check: fmt clean; clippy clean with default features, `--no-default-features` and `--features gpu-tests`; `cargo test --workspace` passes (600 lib).
- [x] 28. FOREVER BEAR F2: `text_texture` was hard to find and game code could not reach it (no `AssetServer` access, no `set_material`). `GameScene` gains `set_text(name, text, TextStyle) -> Option<[u32; 2]>` (draws the text as the object's base color map, returns the texture size, and keeps one texture per distinct text and style), `set_material` and `create_texture`. `docs search` snippets now show the line naming the most query words, so "text texture" shows the `text_texture` line instead of a line with "context". Tests: unit `game_code_puts_text_and_materials_on_existing_objects`, and `search_needs_every_word_and_ranks_head_hits_first` extended; `rusting docs search "text texture"` now lists `GameScene::set_text` first. Docs: `docs/menus-and-ui.md` "Text on meshes", `src/project_agents.md`. Full check: fmt clean; clippy clean with default features, `--no-default-features` and `--features gpu-tests`; `cargo test --workspace` passes (601 lib).
- [x] 29. FOREVER BEAR F1: `asset reimport assets/models/bear.glb` failed with ASSET_NOT_FOUND because models added by `scene add-model` have `.rmesh` files but no `.rmeta`. Reimport by path now registers a supported file under `assets/` that has no metadata: it gets a new ID, the file is revalidated in place (a glTF re-cooks its `.rmesh` files) and the `.rmeta` is written. Unknown paths still fail with ASSET_NOT_FOUND. Test: unit `a_model_without_metadata_is_registered_by_reimport`. Docs: the `asset reimport` schema summary. Full check: fmt clean; clippy clean with default features, `--no-default-features` and `--features gpu-tests`; `cargo test --workspace` passes (602 lib).
- [x] 30. FOREVER BEAR F7: primitive mesh sizes and axes were not written down. `docs/look-and-feel.md` "Mesh kit" now has a table of every built-in primitive with its size at scale 1 and its axis (all centered; `Cylinder`, `Cone`, `Capsule` and `Pyramid` run along Y; `Capsule` is 1 x 2 x 1; `Plane` faces +Y, `Quad` faces +Z), and the `mesh/BuiltinPrimitive` schema field gives the same summary and lists `RoundedCube` and `Capsule`. Test: unit `primitive_sizes_match_the_documented_table` measures each generated mesh against the table. Full check: fmt clean; clippy clean with default features, `--no-default-features` and `--features gpu-tests`; `cargo test --workspace` passes (603 lib).
- [x] 31. FOREVER BEAR F6: the patch docs listed `create` but never showed a whole entity, so where `name`, `parent`, built-in sections and `rusting.*` components go was found by trial. `scene_patch::CREATE_EXAMPLE` is a full patch (a parent, a child that names it, a mesh and a component); `rusting schema --json` shows it as `scene_patch.create_example` next to a new `create_entity` layout note, `rusting explain PATCH_JSON` prints it, and the PATCH_OPERATION fix points there. Test: unit `the_documented_create_example_applies` applies the example to a scene; `rusting explain PATCH_JSON` prints it. Docs: `src/project_agents.md`. Full check: fmt clean; clippy clean with default features, `--no-default-features` and `--features gpu-tests`; `cargo test --workspace` passes (604 lib).
- [x] 32. FOREVER BEAR F24: each `expect_screen` took 15-20 s on FOREVER BEAR's 3,895-entity scene, so six on one tick tripped the 60 s stall watchdog. Every sampled pixel (two passes of up to 64x64) recomputed each mesh's bounds from its vertices and inverted its matrix. `CameraView::pick_rect` now prepares the meshes once per check (bounds once per mesh asset, matrices once per entity) and drops meshes entirely outside the view frustum; `pick` shares the same code. Measured on FOREVER BEAR's `w1_booth` steps with all six `expect_screen` on tick 15 (debug build): 143 s before (about 30 s of it rebuilding), 3 s after against 1 s with no screen checks; the real `w1_booth` now passes in 48 s (was about 1 min 54 s). Tests: the existing `expect_screen` and `pick_rect` scenario tests pass unchanged. Full check: fmt clean; clippy clean with default features, `--no-default-features` and `--features gpu-tests`; `cargo test --workspace` passes (604 lib).
- [x] 33. FOREVER BEAR F10: game code could switch and move cameras but not change a field of view, so a zoom needed a second camera. `GameScene::set_camera_fov(name, radians) -> bool` sets a perspective camera's vertical field of view and `camera_fov(name) -> Option<f32>` reads it; both refuse orthographic cameras and names that are not cameras. Test: unit `aim_hits_what_the_camera_faces_and_despawn_removes_it` extended. Docs: `docs/cameras.md` (with an eased zoom example), the camera schema summary. Full check: fmt clean; clippy clean with default features, `--no-default-features` and `--features gpu-tests`; `cargo test --workspace` passes (604 lib).
- [x] 34. FOREVER BEAR F5: the player controller had a fixed ±89° pitch clamp and no yaw clamp, so a seated view was clamped by hand in game code one frame late. `rusting.player_controller` gains `pitch_limits` (`[low, high]` radians, default ±89°) and `yaw_limits` (`[low, high]` or `null`, the default, to turn freely). Both apply after mouse, stick and `set_look` changes, before the camera transform is written, so the view never shows past them. Test: unit `player_look_needs_captured_cursor_and_clamps_pitch` extended (limits hold against large mouse motion, the camera child already shows the clamped pitch, and resetting the limits turns freely again). Docs: `docs/cameras.md` (a booth seat example), the schema summary and example, reflected field docs. Full check: fmt clean; clippy clean with default features, `--no-default-features` and `--features gpu-tests`; `cargo test --workspace` passes (604 lib).
- [x] 35. FOREVER BEAR F23: `rusting check` printed CLI_OUTDATED on every run while the engine source was being edited, so the warning was noise that hid real ones. The source-newer-than-CLI warning now shows at most once a day per project; the marker is `build/cli-outdated-shown`, and its modified time is the last time it showed. A version mismatch between the CLI and the engine still shows on every check. Test: unit `once_a_day_lets_one_call_through_per_day` (first call passes, second is held, a marker older than a day passes again). Docs: the CLI_OUTDATED explanation. Full check: fmt clean; clippy clean with default features, `--no-default-features` and `--features gpu-tests`; `cargo test --workspace` passes (605 lib).
- [x] 36. FOREVER BEAR F19: the `restart()` docs did not say what happens to playing sounds, so a game could not tell whether a looped hum started in a `once` block would play twice. Restart and `load_scene` stop no sound; the docs now say so and name `stop_all_sounds()` or `stop_sound(id)` as the fix. Test: unit `restart_puts_the_starting_scene_back_and_reruns_setup` extended (a looped sound played in setup is requested twice across a restart and no stop is issued). Docs: `GameScene::restart` and `GameScene::load_scene` reference docs, `docs/audio.md` "Playing sounds". Full check: fmt clean; clippy clean with default features, `--no-default-features` and `--features gpu-tests`; `cargo test --workspace` passes (605 lib).
- [x] 37. FOREVER BEAR F15: a scenario `expect` could not count an array, so "at least three ambience sounds are playing" was checked by indexing `/playing/0`, `/playing/1` and `/playing/3`. `greater_than` and `less_than` on an array now compare its length (`{"entity": "audio:", "path": "/playing", "greater_than": 2}`); invariants get the same rule. No new field. Test: unit `an_attached_sound_pans_as_its_entity_moves_and_the_listener_can_move` extended (`/playing` greater than 0 and less than 2 with one sound). Docs: `docs/audio.md` "Testing sound in scenarios", the schema `expect` summary, the field docs. Full check: fmt clean; clippy clean with default features, `--no-default-features` and `--features gpu-tests`; `cargo test --workspace` passes (605 lib).
- [x] 38. FOREVER BEAR F25: the pan law and the fixed-listener behaviour were undocumented, so a scene clipped on one channel and the cause took reading `spatialize` and kira. `Sound::pan` now documents kira's constant-power law (both sides 1 at pan 0; at pan ±1 the near side √2, +3 dB, the far side silent); `docs/audio.md` warns that a named listener does not turn with the active camera and explains the louder hard-panned side. `audio:/playing` entries gain `gain`: `[left, right]` from volume and pan, before bus and master volume, through the new `audio_output::pan_gains`. Test: unit `pan_gains_match_the_mix` (pan 0 and pan 1 values, and the offline kira mix at pan 1 has a silent left and a right √2 times the centre sample). Full check: fmt clean; clippy clean with default features, `--no-default-features` and `--features gpu-tests`; `cargo test --workspace` passes (606 lib).
- [x] 39. FOREVER BEAR F12: with `mouse_look` on, the first left click captures the cursor, so `pointer_ray()` had no useful cursor and there was no documented way to look and click together. Docs only: `docs/cameras.md` now says the left click captures and Escape releases, and that a first-person view clicks through `scene.aim(distance)` with a centre crosshair, keeping `pointer_ray()` for `mouse_look: false` views; the `pointer_ray` reference doc points to `aim`. The capture behaviour is already covered by unit `player_look_needs_captured_cursor_and_clamps_pitch` and `aim` by `aim_hits_what_the_camera_faces_and_despawn_removes_it`. Full check: fmt clean; clippy clean with default features, `--no-default-features` and `--features gpu-tests`; `cargo test --workspace` passes (606 lib).
- [x] 40. FOREVER BEAR F4: it was unclear whether the player controller's `camera_height` and the camera child's own position add or which wins. In first person (`camera_distance` 0) the child keeps its own local position and `camera_height` is unused; a non-zero `camera_offset` replaces the position. In third person the orbit centre is `camera_height` above the body plus `camera_offset`. Documented on the `camera_height` field and in `docs/cameras.md` "Where the eye sits" with a seated-eye example. The warning when both are set was skipped: `camera_height` defaults to 0.6, so a set value cannot be told from the default. Test: unit `player_look_needs_captured_cursor_and_clamps_pitch` now asserts a first-person child at 0.7 stays at 0.7 with `camera_height` 0.6. Full check: fmt clean; clippy clean with default features, `--no-default-features` and `--features gpu-tests`; `cargo test --workspace` passes (606 lib).
- [x] 41. FOREVER BEAR F3: `rusting schema --json` was one very large document, so finding one component took Python one-liners. `rusting schema NAME` now prints only that part: a component, resource or asset type (`camera_screen` or `rusting.camera_screen`), an operation (`doctor`), or a top-level section (`scenario`). An unknown NAME fails with `CLI_USAGE` and lists the known names, which doubles as the list. New `schema::catalog_entry`. Test: unit `catalog_entry_finds_one_component_or_section`; by hand `rusting schema camera_screen --json` prints its default, example and fields. Docs: the schema operation usage, summary and example, and the generated project AGENTS.md. Full check: fmt clean; clippy clean with default features, `--no-default-features` and `--features gpu-tests`; `cargo test --workspace` passes (607 lib).
- [x] 42. FOREVER BEAR F11: scenario step kinds were spread over several guide pages with no single list. `rusting docs show scenario` now prints the generated page `reference/scenario`: every scenario file field (file, steps, invariants, budgets, files, gpu, audio, counter, tick order) and every step kind with an example, built from the schema catalog's scenario section so it cannot drift from what `rusting test` reads. `rusting schema scenario` (item 41) gives the same as JSON. Test: unit `the_scenario_reference_lists_every_step_kind` (bare `scenario` finds the page; `expect_pixels`, `expect_screen`, `capture`, `log`, `set` and `budgets` are on it). Full check: fmt clean; clippy clean with default features, `--no-default-features` and `--features gpu-tests`; `cargo test --workspace` passes (608 lib).
- [x] 43. FOREVER BEAR F22: the game believed a scenario could not turn the player's view, so clicks on objects off the view centre were untested. Scenario `set` already reaches `/components/rusting.player_controller/yaw` and `/pitch`; the controller applies them, within its limits, the same tick. No new step: the `set` step schema text (and so `rusting docs show scenario`) and `docs/cameras.md` now show turning the view before a click. Test: unit `set_turns_a_player_controllers_look` (set yaw 1.0 and pitch -0.3 at tick 1; at tick 2 the body's Y rotation is 1.0 and the pitch held). Full check: fmt clean; clippy clean with default features, `--no-default-features` and `--features gpu-tests`; `cargo test --workspace` passes (609 lib).
- [x] 44. FOREVER BEAR F21: the `audio:` probe gave one level for the whole mix, so a scenario could not prove a voice bus is heard over the ambience bus. Each bus's kira track now ends with a volume control and a meter effect, and `/buses/<bus>/level` and `/peak` report the bus's RMS and peak over the last tick, after its effects and `set_bus_volume` (named bus volumes moved from the track volume to that control; the main bus `""` keeps its track volume, so its level is the mix before the master volume). Docs: the `AUDIO_ENTITY` rustdoc and `docs/audio.md` probe list, with a voice-over-ambience example. Test: unit `the_offline_mix_reports_levels_pan_buses_and_scheduled_starts` extended (music bus level above 0.1 on the left and silent on the right when panned left, main bus level above 0.1, music bus silent after its volume goes to 0 while the main bus still carries a later sound). Full check: fmt clean; clippy clean with default features, `--no-default-features` and `--features gpu-tests`; `cargo test --workspace` passes (609 lib).
- [x] 45. FOREVER BEAR F17: no trustworthy 1080p frame-time p95. Split:
  - [x] 45a. `RUSTING_PERF` prints frame-time percentiles. The once-a-second `[rusting] perf` line now has `frame p50 / p95 / p99 / max` over that second (nearest rank over each frame's length), and `docs/look-and-feel.md` says how to measure in the real window and that `rusting test` frame times (offscreen plus readback) are a regression guard, not the budget. Test: unit `frame_percentiles_use_the_nearest_rank` (100 frames, six slow: p50 10, p95 20, p99 20, max 40; empty gives zeros). Full check: fmt clean; clippy clean with default features, `--no-default-features` and `--features gpu-tests`; `cargo test --workspace` passes (610 lib).
  - [x] 45b. A window size in `project.json`, so a 1080p run needs no `set_window_size` build. `ProjectManifest.window: Option<[u32; 2]>`; when the game loads from its project folder, `request_project_window` reads it and asks for that size through the same `WindowRequest` as `set_window_size` (applied after the first frame; headless and scenario runs ignore it). The JSON schema lists `window`; `docs/look-and-feel.md` shows it next to `RUSTING_PERF`. Exported builds carry no `project.json`, so they keep calling `set_window_size`. Test: unit `project_json_window_size_is_asked_for` (no project file asks nothing; `"window": [1920, 1080]` asks for 1920x1080) and `json_schemas_describe_every_key_the_engine_writes` covers the new key. Full check: fmt clean; clippy clean with default features, `--no-default-features` and `--features gpu-tests`; `cargo test --workspace` passes (611 lib).
  - [x] 45c. `perf.render.cameras` in the test report lists camera screens. It was filled only for viewport cameras, so a wall of `camera_screen` feeds showed no cost. `SceneRenderer::screen_stats` gives the camera, work counters and GPU time of each feed drawn that frame, and `HeadlessCapture::metadata` appends them after the viewport cameras with `"screen": true`. Docs: `PerfReport.render` rustdoc and `docs/cameras.md`. Test: headless Vulkan `viewport_cameras_split_the_frame` extended with an inactive `Feed` camera shown on the mesh (cameras `Left`, `Right`, `Feed`; `Feed` has `screen: true` and draws > 0). Full check: fmt clean; clippy clean with default features, `--no-default-features` and `--features gpu-tests`; `cargo test --workspace` passes (611 lib), and with `--features gpu-tests --test-threads=1` on the RTX 3060 (698 lib).
  - [x] 45d. `rusting run --bench FRAMES` runs the windowed game for that many frames and prints p50/p95/p99 once at the end. Acceptance: unit test of the summary line; the windowed run itself is real-hardware only. Evidence: `RUSTING_BENCH_FRAMES` makes the windowed runner skip 60 warm-up frames, measure the rest, print a `[rusting] bench` JSON line and exit; `run --bench` reports it as `timings.bench` and a `bench:` text line. Test `project::tests::a_bench_line_round_trips_its_frame_times`; the windowed run is real-hardware only.
- [x] 46. FOREVER BEAR F29: easing hundreds of object colors cost about 25 µs a call. Name lookups were already hashed; the cost was `set_color`/`set_emissive` scanning every material for an equal one, and every eased value adding a material that was never freed. Acceptance: edits find equal materials through a hash index, unused edited materials are removed, and the count stays bounded. Evidence: test `project_runner::tests::easing_many_colors_keeps_the_material_count_bounded` (4,000 objects, 900 eased for 20 ticks): release build 488 µs a call and 18,000 materials kept before, 348 ns and under 4,000 after.
- [x] 47. FOREVER BEAR F27: a scenario `expect` on a counter nobody created yet failed with "no entity", while game code reads it as 0. Acceptance: the scenario reads it as 0 too. Evidence: `scenario::tests::counter_shorthand_sets_checks_and_logs_a_counter_by_name` runs `{"expect": {"counter": "plays", "equals": 0}}` with no such counter and passes; schema scenario docs say so.
- [x] 48. FOREVER BEAR F30: a flag a command does not take gave only "invalid command or arguments". Acceptance: the error names the flag and shows the command's usage; `determinism --release` says determinism always compares debug and release. Evidence: test `a_bad_flag_is_named_with_the_command_usage` in `src/bin/rusting.rs`.
- [x] 49. FOREVER BEAR F28: tuning needed one run per wrong guess because a scenario stops at its first failed check. The scenario field `keep_going` existed but had no CLI flag, and the error showed only the first failure. Acceptance: `rusting test --keep-going` runs every step and the SCENARIO_FAILED message lists every failed step. Evidence: `a_bad_flag_is_named_with_the_command_usage` checks the flag is accepted; `scenario::tests::keep_going_reports_every_failed_check` covers the run; the message joins every failed step from the report.
- [x] 50. FOREVER BEAR F26: which sound loses on a full bus needed a source read. Acceptance: docs/audio.md and `Sound::priority` say priority compares within one bus, quietest means after distance and occlusion, a new sound must rank strictly higher, and ties replace the oldest. Evidence: docs/audio.md "When the bus is full"; `scenario::tests::a_thousand_sounds_in_one_tick_stay_within_the_voice_limit` now also plays a priority 0 sound on another bus while `chorus` is full and expects it to play.
- [x] 51. FOREVER BEAR F18: headless runs gave no render timing. Acceptance: a `rusting test` scenario that renders offscreen reports GPU frame-time percentiles over every frame, not only the last one. Evidence: `perf.render` now has `gpu_frames`, `gpu_ms_p50`, `gpu_ms_p95` and `gpu_ms_max` (nearest rank over each `HeadlessCapture::frame`); `scenario::tests::a_render_budget_reports_render_counters_without_a_capture` (gpu-tests) checks the frame count covers every tick and p95 <= max; docs/look-and-feel.md explains `"gpu": true` with `capture_size`.
- [x] 52. Freddy's F9/F18: game code had no per-tick hook, and `pressed` lasted one frame, so a game stepping per tick inside `update` missed presses at 144 Hz. Acceptance: `rusting_game!(update, tick: tick)` calls `tick` once per fixed tick, and `pressed` there is true on the first tick after a press made on a frame that ran no tick. Evidence: `project_runner::tests::a_tick_function_sees_a_press_from_a_frame_without_a_tick` (a quarter-step frame with the press, then a three-tick frame: one press seen, three ticks run); macro arm compile-checked in a scratch crate; docs/concepts.md "Game code" and "Frames and fixed ticks".
- [x] 53. Freddy's F17: whether `restart` keeps counters and what `set_paused` does to `fixed_tick` was not stated. Acceptance: the `GameScene::restart` and `set_paused` docs say both, with a restart example that carries a value across. Evidence: rustdoc on both; `restart_puts_the_starting_scene_back_and_reruns_setup` now checks a `set_counter` counter is gone after restart; `a_tick_function_sees_a_press_from_a_frame_without_a_tick` checks pause holds `fixed_tick` and tick functions while `frame` counts on.
- [x] 54. Freddy's F13: ray hits on ragdoll bodies had an empty name and ignored the bone's classes. Acceptance: a hit on a ragdoll body reports its bone's name, and `raycast_skipping` skips it by the bone's classes. Evidence: ragdoll bodies carry `RagdollPart { bone }` (snapshot-registered); `runtime::tests::ray_hits_on_ragdoll_bodies_carry_the_bone_name_and_classes` hits an active ragdoll's hips body ("Hips") and skips it by class `animatronic`; docs/animation.md and `GameScene::raycast` rustdoc.
- [x] 55. Freddy's F15: an active ragdoll's hips drifted when a clip keyed only their position, because the whole transform no longer matched what the ragdoll wrote and the rotation target became the last physics pose. Acceptance: unkeyed position, rotation and scale each return to the pose from the handoff. Evidence: `drive` restores per field; `runtime::tests::an_active_ragdoll_returns_an_unkeyed_rotation_to_its_pose` spins the hips body while keying only the hips' position for 90 ticks and expects under 0.2 rad of turn (fails before the fix); docs/animation.md.
- [x] 56. Freddy's F16: teleporting an active ragdoll left its bodies trailing, and muscle 0 collapsed it instead of returning to animation. Acceptance: `GameScene::reset_ragdoll(name)` drops the bodies and restores the bones' animated pose; an active ragdoll respawns its bodies at rest on the bones next tick. Evidence: `runtime::tests::reset_ragdoll_teleports_an_active_ragdoll_with_its_bodies` moves the character 5 m, resets, and finds the hips body 5 m on (within 0.05 m) at under 1 m/s; blending back now also restores unkeyed fields one by one; docs/animation.md.
- [x] 57. Freddy's F14: `AnimationEvent` fields were missing from the docs. Evidence: the struct is in the `api/` index with a doc line per field (`rusting docs show api/AnimationEvent`); `docs.rs` test asserts every field is listed; `docs/animation.md` names the fields.
- [x] 58. Freddy's F1: `guide/look-and-feel` said skinned meshes import as a still pose, against `guide/animation`. Evidence: the stale sentence is replaced by a pointer to `guide/animation` "Skinned meshes"; `grep -rn "still pose" docs` finds nothing; docs tests pass.
- [x] 59. Freddy's F6: `scene add-model` refused models whose node names repeat or clash with the scene. Evidence: node and extra-primitive names already taken get " 2", " 3"; a root name clash fails with "pass --name"; `a_gltf_model_lands_in_a_scene_under_one_named_object` adds a second copy, a root named like its own node, and checks the suffixes and the hint.
- [x] 60. Freddy's F12: exposure and mouse look were reachable only through `world()`. Evidence: `GameScene::set_exposure(f32)` writes or adds `rusting.tone_mapping`, `GameScene::set_mouse_look(name, bool)` toggles the player's mouse look (off frees the cursor); `video_settings_write_render_settings_and_the_window_request` covers both, including ignored negative exposure and an unknown player.
- [x] 61. Freddy's F2: `scene add-model` has no `--license`/`--author` flags and nothing said where provenance lives. Evidence: the `scene add-model` schema summary and `guide/look-and-feel` step 3 say the `.rmeta` written by `asset import` is the record; schema, docs and workspace tests pass.
- [x] 62. Freddy's F8: fog, color grading and player controller fields had no setter from game code. Evidence: `GameScene::set_field(name, path, value)` takes the scenario `set` JSON pointers (`/components/rusting.fog/density`); `video_settings_write_render_settings_and_the_window_request` sets `rusting.player_controller/walk_speed`, and checks the errors for an unknown field and object; the project AGENTS.md names it.
- [x] 63. Freddy's F11: faint egui white came out as a solid grey band. Cause: egui's `from_white_alpha(8)` and `from_rgba_unmultiplied` premultiply in linear light and give grey 50; the painter matches egui's backends. Evidence: GPU test `faint_egui_white_is_premultiplied_in_linear_light` paints both and `from_rgba_premultiplied(8, 8, 8, 8)` (adds 8); `guide/menus-and-ui` says which constructor to use for faint overlays.
- [x] 64. `rusting capture --at X,Y,Z` renders from a camera placed at a point, aimed with `--look-at X,Y,Z` or `--look YAW,PITCH` (Freddy F7). It takes its lens from `--camera` or the active camera.
  Evidence: `capture_without_vulkan_still_reports_camera_and_picks_per_tick` picks the cube at the center from the side and from above and nothing when facing away, and rejects `--look-at` without `--at`; the GPU test `capture_renders_a_png_and_maps_pixels_to_scene_ids` renders from a placed camera and finds the cube. A light on the capture camera is not added.
- [x] 65. Scenario captures and `rusting capture` render only the four ticks before each image step (and through its `until`/`within`), not every tick of the run, unless the scene has GPU bodies or the scenario sets `gpu` or a render budget (FOREVER BEAR F33).
  Evidence: `perf_is_reported_and_budgets_fail_the_run` runs a 40-tick scenario with one capture at tick 40 and gets `gpu_frames` 5 and the PNG; `capture_renders_a_png_and_maps_pixels_to_scene_ids` captures `--tick 300` with `gpu_frames` 5. Every existing capture, pixel and camera-screen test still passes. The wall-clock gain on FOREVER BEAR's 10,000-tick runs is not measured here.
- [x] 66. `rusting docs show api/WaypointGraph` lists its fields and every method (`add_node`, `connect`, `disconnect`, `nearest`, `path`, `length`), read from the source (FOREVER BEAR F31). The struct page reader no longer gives an undocumented type the previous item's doc comment.
  Evidence: `api_index_lists_documented_gameplay_methods` checks the page names `nodes`, `nearest`, `disconnect`, `path` and `length`; every docs test passes.
- [x] 67. `rusting run --stderr` and `rusting test --stderr` print the game's stderr line by line while it runs, and still keep it whole in `game.stderr` (FOREVER BEAR F32).
  Evidence: `echoed_stderr_is_still_kept_whole` runs a child that writes to both streams with echo on and gets its stderr and stdout back intact. No test runs a real game with `--stderr` end to end.
- [x] 68. `rusting docs show guide/lighting` names each light type and whether it casts shadows (only the first shadowed directional light does), the light cap (64 at High, 32 at Balanced, 16 at Eco) and the order lights are dropped in; scenario perf reports `dropped_lights` (Freddy F3, docs half).
  Evidence: `api_index_lists_documented_gameplay_methods` checks the page names the cap, Eco, shadows and `dropped_lights`; `perf_is_reported_and_budgets_fail_the_run` checks `perf.render.dropped_lights` is 0. Drop order was read from `prepare_lights` and `collect_point_lights`/`collect_spot_lights`.
- [x] 69. Shadowed spot lights: `"shadows": true` on a `spot_light` gives it one shadow map, so a flashlight does not light the far side of a wall (Freddy F3, shadow half). Acceptance: a GPU test with a spot light behind a wall leaves the floor past the wall unlit with shadows on and lit with them off. Evidence: GPU test `spot_light_shadow_stops_at_a_wall_only_when_enabled` reads the floor behind a wall at 0 with shadows on and 255 with them off; the directional shadow tests pass unchanged. A frame still has one shadow map: a shadowed directional light wins over spot lights, and point lights cast none.
- [x] 70. `rusting.player_controller` crouches: `crouch_height` above 0 makes the new `player.crouch` action (C, left Ctrl, pad East) shrink a capsule or box body to that height from the top at `crouch_multiplier` speed, drop first-person cameras by the lost height, block jumping, and stand again only when a shape cast up finds room (Freddy F10). Evidence: `player_controller_crouches_under_a_table_and_stands_only_with_room` (a 1.8 m capsule stops at a 0.75 m table, crouched to 0.7 m walks under with feet on the floor and the camera 1.1 m lower, stays crouched under it after release, stands up past it). Sensors still see the standing collider.
- [x] 71. A scenario `audio:` check on `/clips/<clip>` for a clip that never played reads 0 instead of failing with "does not exist", so `"equals": 0` proves a sound never played; `"exists": false` still passes for it (FOREVER BEAR F34). Evidence: `the_audio_entity_counts_sound_cue_requests_per_clip` checks an unplayed `sfx/miss.wav` both ways.
- [x] 72. `asset import --to assets/sounds` (and `asset generate --to`) writes to `assets/sounds`, not `assets/assets/sounds`: a folder starting with `assets/` is read from the project root (playplace F14). Evidence: `audio_imports_and_dry_runs_write_nothing` imports with `assets/music` and gets `assets/music/hit.wav`.
- [x] 73. `rusting docs show api/GpuBodySettings`, `api/GpuConditionShader` and `api/MaterialAsset` list each type's fields with docs; the GPU cube rain tutorial and the `collider` field say friction decides whether a dense GPU pile flows (about 0.05 for plastic balls, 0.3 jams) and that dense piles jam below a threshold push (playplace F13, F15, F17). Evidence: the docs test checks a documented field line on each new page.
- [x] 74. Scenario checks and invariants capture only the entity they read (`scene_entity_lenient`) instead of the whole scene each tick, and `perf.wall_ms_mean` reports the whole run per tick, checks included (playplace F16, first half). Evidence: a throwaway release benchmark on 5,000 entities measured 3.1 ms per whole-scene capture against 35 us for one entity; the existing scenario tests pass unchanged, and `perf` tests assert `wall_ms_mean >= tick_ms_mean`.
- [x] 75. GPU physics steps without drawing a frame: on scenario ticks with GPU bodies and no image step due, submit only the physics compute work, so long GPU scenarios run faster than render speed (playplace F16, second half). Acceptance: a GPU test shows the same `gpu_state_hashes` with and without the skipped draws, and `gpu_frames` counts only image ticks. Evidence: `SceneRenderer::step_physics` records the uploads and physics dispatches of a frame and submits before the culling and draw passes; scenario runs and `rusting capture` use it on ticks with GPU bodies and no image due. GPU test `gpu_bodies_step_without_drawing_and_hash_the_same` gets identical `gpu_state_hashes` from a 30-tick run drawn every tick and one drawn only for its last capture, whose `gpu_frames` is 5. The CPU-side draw preparation still runs on skipped ticks.
- [x] 76. Games keep state in their own components without extra crates: `rusting_game!(update, components: [Night => "game.night"])` registers them (also with `tick:`), and the prelude re-exports `Component`, `Serialize`, `Deserialize`, `bevy_ecs` and `serde` (serde derives take `#[serde(crate = "rusting_engine::serde")]`) (playplace F12). Evidence: unit test `rusting_game_components_are_saved_in_scenes`; ignored CLI test `rusting_game_registers_a_custom_component_without_extra_crates`, run here and passing, builds a generated project that depends on `rusting_engine` alone and passes a scenario that sets and reads `/components/game.night/power`.
- [x] 77. `rusting.post_volume` (`extents`, `blend`, `priority`) limits the `rusting.fog` and `rusting.color_grading` on its object to a box around it: fully inside, fading out over `blend` metres, higher priority blended last; fog and grading without a volume stay global (Freddy F5). Acceptance and evidence: unit test `post_volumes_blend_fog_and_grading_around_the_camera` reads the extracted render world outside the hall (global grade, no fog), inside it (the hall's fog and grade) and half way through the blend (half of each); the editor offers fog and grading on a volume object.
- [x] 78. A game built against a newer or edited engine than the `rusting` CLI no longer fails every run with a bincode error (`InvalidBoolEncoding`) when it cannot decode the cooked scene: in a dev project (with `Cargo.toml`) it prints a warning naming the mismatch and loads the source main scene instead; an exported game still reports the error (forever-bear F35). Evidence: unit test `a_cooked_scene_from_another_engine_build_falls_back_to_the_source` loads an undecodable cooked scene with and without `Cargo.toml`.
- [x] 79. Mesh level of detail is documented and findable: the renderer already picks a level per object from a `.rlod` file beside the `.rmesh` (GPU test `lod_groups_draw_the_level_for_the_camera_distance_on_every_path`), but `rusting docs search lod` found nothing (forever-bear F36). `guide/look-and-feel` gains a "Level of detail (LOD)" section with the file format and both metrics, and SKILL.md points to it. Evidence: unit test `level_of_detail_is_found_by_its_usual_names` (`lod`, `level of detail`, `rlod`).
- [x] 80. The lighting guide says what light brightness means and where lights point: sun `illuminance` in lux next to point and spot `intensity`, the built-in presets' sun, ambient and sky values, lamp ranges, and that directional and spot lights shine along -Z with the rotation that aims one down (forty-thousand-and-one F34; also triaged that game's older items, most already fixed). Evidence: unit test `the_lighting_guide_explains_brightness_and_direction`; the -Z direction read from `light_direction` in the renderer and the values from `src/art_direction.rs`.
- [x] 81. GPU contact-grid overflow is visible outside the editor: `physics_grid_overflow`, `physics_oversized_bodies` and `physics_fallback_tests` are in `rusting test --json` under `perf.render`, game runs (window and headless capture) insert `RenderCapacityDiagnostics` into the world each frame (now in the prelude), and docs/concepts.md "Many GPU bodies" explains the 8-bodies-per-cell limit and warns against clamping many bodies to one plane (forty-thousand-and-one F36). Evidence: GPU test `contact_grid_overflow_is_in_perf_and_the_world` (20 kinematic bodies in one spot: overflow and fallback tests above 0 in perf, overflow above 0 in the world resource); unit test `contact_grid_overflow_is_explained`.
- [x] 82. Many-body costs are documented (forty-thousand-and-one F35): docs/concepts.md "Many bodies: what costs time" gives `4 * subdivisions^2` triangles per `SphereSpawn` sphere (36 at 3, 144 at 6, 1,024 at 16), says `state_hash` is O(class size) and belongs on checkpoint ticks, and points to `perf.tick_ms_p95` and `perf.render.gpu_ms_p95`; the `SphereSpawn::new`, `subdivisions` and `GameScene::state_hash` rustdoc say the same. Evidence: unit test `many_body_costs_are_explained` (docs search finds both topics, and `procedural_sphere_mesh` has exactly `4 * n * n` triangles for 3, 6 and 16).
- [x] 83. A settled GPU pile stays asleep (forty-thousand-and-one F13 re-test: the pile slept by tick 40, then woke with nothing touching it). Cause: sleeping balls still took every tiny contact correction, crept, spread the pile (top rose 0.11 m over 400 ticks) and woke. `physics_contacts.comp` now has sleeping bodies ignore pushes under `SLEEP_SLOP` (0.5 mm) and velocity changes under the rest speed; a larger slop (the 2 mm contact margin) let the sideways-pressed pit squeeze a ball into a wall. Still order-independent (per-body max/min). Evidence: GPU test `a_settled_gpu_pile_stays_asleep` (3,600-ball 9-layer pit: before, 3,032 asleep at tick 200 and 2,181 at 300; now over 3,500 at tick 240, no fewer at 400, top still within 2 mm); `a_deep_gpu_ball_pit_settles_and_its_walls_hold` still holds (3,592 resting, none escape). The 40,000-ball case is for the game to re-test.
- [x] 84. Remaining metronome-mines API doc gaps (F21, F26): `api/BusEffect` page (variants and fields from the source), and the `GameScene::press_tick` doc says a scenario press with `"at": 0.4` gives `tick + 0.4` instead of claiming scenario presses land on whole ticks. The other open metronome-mines items were checked against the current binary and marked: F1, F5, F19, F20, F22 to F25, F27 and F28 already work. Evidence: unit test `bus_effects_and_press_tick_are_documented`; `rusting fix --dry-run` prints `No fixes needed.`; a rerun `upsert` patch reports `0 field changes`; `rusting docs search fixed_tick scenario` finds `reference/scenario`.
- [x] 85. `expect_pixels` can compare a region with an earlier capture (split-signal F24): `"differs_from": "shots/a.png"` with `difference_min` and/or `difference_max` checks the largest of the R, G and B mean differences (0..255) in the region, so a scenario can prove two moments look different or the same. Either half alone is a validation error; the schema line describes it. Evidence: unit test `differs_from_needs_a_difference_limit` (validation and `region_difference` numbers); the GPU test `pixel_checks_goldens_and_named_cameras` now also checks the main frame against `main.png` (difference at most 1) and against the away view `away.png` (difference at least 1).
- Parallel gpu-tests on the RTX 3060: the clean HEAD (before this track) also fails 5 of 10 parallel runs (a segfault, a hang and failures), so this is not new. A hung run stops inside the NVIDIA driver in `vkDestroyCommandPool` while a test drops its command buffer allocator. The ninth `rusting-core` increment above already records the same SIGSEGV; run gpu-tests with `--test-threads=1` as before. On llvmpipe the parallel run passes.
- Full check (2026-10-06): fmt clean; clippy clean with default features, `--no-default-features`, `--features gpu-tests` and `--features steam`. `cargo test --workspace` passes (597 lib). `cargo test --workspace --features gpu-tests -- --test-threads=1` passes (684 lib, 27 CLI, 5 vertical slice, 17 doc tests). Parallel gpu-tests runs were flaky: one run failed about 30 scene_renderer tests, one hung in `pbr_maps_and_unlit_model_shape_the_lit_color`, and a third run passed all 684. Each failing test passed alone. The failure messages of the first run were not kept. A later serial run had two intermittent failures, `headless_device_creation_succeeds_with_a_vulkan_driver_present` and `devices_enable_anisotropy_when_the_driver_supports_it`, both at device creation (`src/rendering/mod.rs:117`); both passed alone and the next full serial run passed all 684. `rusting test samples/forever_bear_booth` passes after the final formatting.
