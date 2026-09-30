# Changelog

## [Unreleased]

### Fixed

- Code-review fixes:
  - A fixed GPU body only becomes a CPU collider with a Full or Simplified solver and an explicit Fixed rigid body.
  - Fluid Block and Water Body no longer share a material; water and fluid coupling skip GPU and child bodies; water and the player's platform ride use the parent's world position.
  - A fluid surface or water surface that is switched off, removed or orphaned is despawned and its mesh and material freed. A resting fluid keeps its mesh, and the surface mesh is indexed and welded.
  - Fluid blocks are capped at 50,000 particles; `Fluid::step` ignores `dt <= 0`; `GameScene::restore` brings back fluid particles.
  - Scene `InputAction` bindings follow the component (edit, removal, scene replace); game-code bindings stay.
  - `set_scene` rejects mistyped setting names, and unrelated entities no longer fail a patch's field check.
  - Third-person camera collision at distance 0, `set_tile` growth (max 4096 cells), environment map load retry, CPU fast-body sweep prefilter, `EditorTheme::ICON_CAMERA`.
  - Not changed: the slow-motion clamp is recorded before replay, so replays are exact by design; `Sleeping` bodies wake when buoyancy changes their velocity.
- The first-person player, raycasts and CPU bodies pass through fixed boxes whose simulation is GPU. A fixed GPU body never moves, so it now also stands in the CPU collision world as a static collider (GPU bodies still collide with each other on the GPU). Dynamic GPU bodies stay invisible to CPU queries; use `GpuQueryProxy` for those.
- A Blend material at Opacity 0 is no longer faintly visible. Reflections are not scaled by alpha, so it still drew a faint mirror; a surface with alpha 0 and no transmission now draws nothing.

### Added

- Add Component picker redesign: a wide two-pane popup. The left list is grouped and searchable (search also matches descriptions); hovering a row shows its name, "What it does", "Use it for" and the Add button on the right. Blocked components show why. Help text lives in `placement::component_help`.
- Hierarchy and Assets visual pass: both panels use the shared toolbar, search field with clear button, bordered list well and footer (object/selection count, file count or status message). Hierarchy icons are tinted by kind (camera, light, mesh).
- `rusting.water` (`WaterBody`): water for seas, lakes and rivers. A rectangle of animated waves (three sine waves, deterministic) that dynamic bodies with sphere, box or capsule colliders float in, with a `flow_speed` current along `flow_direction` for rivers. Add it from Add Object > Environment and UI > Water. Use Fluid Block only for small splashing volumes.
- `runtime::fluid_surface`: fluid is drawn as one smooth surface mesh (density grid plus marching tetrahedra, deterministic) in place of a sphere per particle. `rusting.fluid_block` `visible` now draws a translucent "Water" surface; the new `show_particles` adds the debug spheres. Code sets `FluidVolume::surface` to `FluidSurface::new(material)`. The mesh is rebuilt and uploaded every fixed tick (CPU), so keep fluids to a few thousand particles.
- Materials have a name (Inspector Material section, Name field; imported glTF materials keep their glTF name). The Mesh Renderer combos show names, or "Material 1" and "Mesh 1" for unnamed ones, in place of hex ids. Cooked scenes are format 9; format 8 files still load.
- Editor: the Project panel is now a Project Settings page. A category rail (Overview, Rendering, Display, Export, Diagnostics). Settings are cards with icon headers and switch and segmented controls. Anti-aliasing, shadow size and reflections are editable there, and Diagnostics shows stat tiles, a CPU frame bar and renderer counters. The shared pieces are in `gui_elements::kit`.
- Editor: Add Component is a full-width button that opens a searchable picker, grouped into Physics, Environment, Engine, Game and User Interface. Inspector sections are bordered cards with an accent bar on the header.
- Editor: the Console has a framed toolbar, striped rows, a level stripe, a source badge, monospace text and a repeat count badge.
- Editor: lowering a material's Opacity below 1 switches an Opaque material to Blend, because Opaque ignores alpha.
- Editor: Save Layout and Load Layout now also store the gizmo snap toggle and its move, rotate and scale increments. Older layout files load without changing snap.
- `runtime::fluid`: a deterministic CPU particle fluid (position based fluids) with a box container, viscosity and a state hash. `FluidVolume` steps it every fixed tick. Set `FluidVolume::visual` to draw each particle as a mesh. Dynamic bodies with sphere, box or capsule colliders float and sink in it and displace it. The `rusting.fluid_block` scene component (a block of particles in a box) creates one. The particles are not saved in scenes (a block makes them again on load) but are in snapshots. Particle entities carry a `FluidParticle` marker, so the editor Hierarchy and Scene View picking skip them.

- Scenario field `keep_going`: the run continues after a failed step and reports every failure.

- `RenderSettings::reflections` (default on; a checkbox in the editor's Render Settings). Off skips screen-space reflections, and with no refractive material also the scene copy, mip chain and depth pyramid. Games call `GameScene::set_reflections(false)`.

- `RUSTING_PERF=1` makes a windowed `rusting` game print one line per second to stderr: fps, CPU phase times, GPU pass times, draw/dispatch/triangle counts. It also prints the present mode and shows fps and frame time in the window title.

- Editor: the Scene view's Snap button has a menu that sets the move, rotation and scale snap increments.
- Editor: the Inspector shows game components this editor has no registration for as read-only JSON, and can remove them.
- Editor: Home frames every rendered object; Numpad 1, 3 and 7 turn the Scene view to the front, right and top views around the orbit pivot, and with Ctrl to the back, left and bottom. Numpad 5 or the viewport menu's Orthographic switches the Scene view to an orthographic view that zooms with the wheel and Frame.
- `rusting.fog` scene component: exponential height fog that fades distant surfaces and the sky into its color, thins out above a height, and brightens toward the sun.
- `rusting.bloom` scene component: bright light glows into its surroundings before tone mapping, with a threshold, intensity and spread.
- `rusting.ambient_occlusion` scene component: screen-space ambient occlusion darkens ambient and sky light in creases and corners. Off on the Eco quality profile.
- Editor: the Scene view's viewport menu switches fog, bloom and ambient occlusion off for editing; Play still draws them. A selected Fog shows its height as squares over the grid; drag them to set the fog's height and falloff.
- Editor: a selected Render Bounds override shows face handles in the Scene view; drag them to resize the box or sphere. "Fit to Mesh" wraps it around the mesh again.
- Editor: Add Component places Fog, Bloom and Ambient Occlusion on the scene's environment object, next to the other atmosphere settings.
- Editor: the material inspector edits Transmission, IOR and Thickness; Add Object offers a Reflection Probe, whose box shows in the Scene view, picks it on click, and resizes by dragging its face handles; the environment map's texture field lists loaded textures.
- `rusting.reflection_probe` scene component: surfaces inside its box reflect the scene as seen from the probe, projected onto the box walls, in place of the environment map. Overlapping probes blend; up to four are used. Probes are captured when added or changed.
- Screen-space reflections: smooth surfaces (roughness under 0.5) and glass reflect the scene on screen, falling back to the sky or environment map where the ray leaves the screen. Off on the Eco quality profile.
- Refractive glass: materials take `transmission`, `ior` and `thickness`. Transmissive surfaces bend, blur (by roughness) and tint what is behind them.
- `rusting.environment_map` scene component: an equirectangular sky image lights the scene and shows in reflections. Smooth metals mirror it; rough ones see a blurred version.
- Player and platformer controllers ride moving platforms and stop rising when they hit a ceiling.
- `samples/core_defense`: a top-down turret shooter aimed with the mouse, built with the `rusting` CLI alone, with four scenarios.
- `GameScene::pointer_ray` returns the ray through the mouse cursor; `GameObject::rotation` and `GameObject::scale` read an object's rotation and scale.
- `samples/putt_course`: a mini golf hole with a rolling physics ball, a charged putt aimed with the mouse and a sensor cup, built with the `rusting` CLI alone, with four scenarios.
- `GameScene::counter_value`, `add_to_counter` and `counter_complete` read, change and test a counter by name in one call.
- `samples/lantern_grid`: a lights-out puzzle played with mouse clicks, built with the `rusting` CLI alone, with two scenarios.
- `GameScene::color`, `set_color` and `set_emissive` read and change one object's material color without recoloring objects that share it.
- `samples/snake_trail`: a snake game on a tile-map arena, built with the `rusting` CLI alone, with three scenarios.
- `GameScene::set_counter` sets a counter to a value.
- `samples/night_vault`: a third-person stealth game with patrolling guards, view cones and an alert meter, built with the `rusting` CLI alone, with three scenarios.
- The third-person camera stops in front of walls behind the player instead of going through them.
- `GameScene::load_scene` switches to another scene file of the project, for levels and menus.
- `skills/rusting-game/SKILL.md`: a skill file that teaches LLM agents to build and test games on the engine.
- `GameScene::random` draws a value from the run's seed and the fixed tick, so scenarios repeat.
- Editor: the Tile Painter has Rectangle, Line and Fill tools, and outlines the cell under the pointer or the dragged rectangle or line in the Scene View. Fill stays inside the grid's used area. `TileMap::fill_rect`, `TileMap::fill_line` and `TileMap::fill` do the same from code.
- Editor: the Tile Painter picks its brush from a palette of swatches in each tile's color instead of a drop-down.
- Editor: D, R, L and B pick the Tile Painter's Paint, Rectangle, Line and Fill tools while a brush is active, as in Godot. They are rebindable.
- Editor: hovering a reflected Inspector field's label shows the field's description from its `doc` hint; a nested struct, list or map shows it on its group header.
- Editor: the reflected Inspector adds and removes entries of string-keyed map fields, such as a tile map's tiles.
- Editor: the Project area shows the resolved quality profile, marked "(Auto)" when Auto picked it, and the number of loaded LOD groups.
- Editor: Numpad 9 turns the Scene view to look at the orbit pivot from the opposite side, as in Blender.
- Editor: a point light's Scene view shape is a circle facing the view, as in Blender, in place of three crossed circles.
- Editor: every shortcut can have a second key, set in the Keyboard Shortcuts area and removed with a right-click, as Blender deletes with both X and Delete. Redo also answers Ctrl+Y by default.
- Scenario `pointer` step: places the mouse cursor at a point of the view, given as fractions of its size.
- `samples/brick_bounce`: a 2D brick breaker built with the `rusting` CLI alone, with a bouncing dynamic ball, a steered paddle and four scenarios.
- `GameScene::linear_velocity` reads a body's velocity.
- `samples/crate_keeper`: a top-down crate-pushing puzzle built with the `rusting` CLI alone, on a tile map grid, with three scenarios.
- `GameScene::tile` and `GameScene::set_tile` read and write the `rusting.tile_map` cell under a world position.
- Scenario `tolerance` applies to every number in an array or object, so a position can be checked with `equals` and a tolerance.
- `samples/tower_topple`: a first-person block-toppling game built with the `rusting` CLI alone, where thrown balls knock towers and a pyramid off their stands, with three scenarios.
- `GameScene::camera_ray` returns the active camera's position and forward direction; `GameScene::set_body_kind` switches a body between fixed, dynamic and kinematic; `GameScene::restart` reloads the starting scene and reruns `once` setup.
- `samples/ember_arena`: a third-person arena survival game built with the `rusting` CLI alone, lit by fog, bloom, ambient occlusion and point lights, with three scenarios.
- `GameScene::spawn_copy` copies a template object and its children; `GameScene::in_class` lists the objects in a class.
- `rusting scene patch`: registered components may also be partial; missing fields take their defaults.
- `samples/sky_hop`: a 2D platformer built with the `rusting` CLI alone, with four scenarios including an input-only full clear.
- `samples/target_range`: a first-person shooting gallery built with the `rusting` CLI alone, with three scenarios.
- `rusting.input_action` scene component: binds a named action to keys and mouse buttons (`{"action": "fire", "inputs": ["MouseLeft", "KeyF"]}`).
- `GameScene::pressed`, `held`, `raycast`, `aim`, `despawn`, `set_visible` and `trigger`, and the `RayHit` type.
- Scenario expectations take `"exists": false` to check that an object is gone.
- `rusting test` runs every scenario in a folder, or in `tests/` when given no scenario.
- `rusting new` writes a `.gitignore`.
- `samples/hammer_run`: a third-person obstacle course built with the `rusting` CLI alone, with three scenarios. It is the first dogfooding game for the agent workflow.
- `rusting new` writes an `AGENTS.md` into the project: the files, the CLI workflow, scene basics and the game code API, so an agent can build the game without reading the engine source.
- `rusting scene patch`: built-in sections and their fields may be partial; missing fields take their defaults. Operations and a created entity's `parent` may name an entity by its unique name instead of its ID.
- Scenario `within`: an `expect` step passes on the first tick through `within` where it holds, for things whose exact tick a test cannot know.
- `rusting check`, `validate`, `cook`, `run`, `determinism`, `test`, `project inspect` and `asset list` default the project root to the current folder.
- `rusting scene patch` has a `set_scene` operation that changes scene-level fields such as the name, render settings and simulation settings.
- Scenario `set` steps write a transform or component value before a tick runs, so a test can place the player or fill a counter instead of replaying inputs to get there.
- `GameScene::counter(name)` reads and changes a `rusting.counter` by name.
- `rusting run --ticks N` saves the scene as it stands after the last tick to `build/final.rscene`, so `rusting scene query` can inspect the end state.
- `GameScene::touching(name)` lists the objects touching a named object in the last physics step, and `GameScene::world()` gives game code the ECS world.
- Reduced-coordinate articulations (`rusting.articulation`): on a root body, it solves the `Fixed`, `Hinge`, `Slider`, and `BallSocket` joints of its tree in joint space. Links never drift apart, hinge and slider limits hold exactly, and contacts on any link move the whole tree. The root floats when it is dynamic.
- Breakable joints: `break_force` and `break_torque` on `rusting.joint`. When a step's load passes either one, the joint is removed and a `JointBroken` event reports the joint, its target, and the force and torque.
- CPU physics joints (`rusting.joint`): fixed, hinge, slider, ball socket, cone twist, distance, spring, and a generic six-axis joint whose axes are locked, free, or limited, each with an optional spring and motor. A joint links a body to another body or to the world, and jointed bodies do not collide unless `collide_connected` is set. See "Physics: CPU and GPU" in `docs/concepts.md`.
- Project templates for a 3D first-person game, a 3D third-person game, and a physics sandbox (`rusting new --template first-person|third-person|sandbox`), next to the 2D platformer. The editor's Create Project form has a Template picker for every template.
- `PlayerController` third-person mode: `camera_distance` above 0 orbits the child camera behind the body around a point `camera_height` above its center.
- Optional `rusting-script` crate: sandboxed WebAssembly scripts on objects (`rusting.script` component), run by the Wasmi interpreter with fuel and memory limits. Scripts read and write reflected component fields and the transform, find objects by name, and read input actions. See `docs/scripting.md`.
- Edit-to-running-game latency test (`tests/iteration_latency.rs`, ignored) for the starter template, with a documented target of 3 seconds for an incremental Debug build after a Rust edit. Measured 0.85 seconds; see `docs/dev-environment.md` "Iteration speed".
- **Reload Code** in the editor while a game runs: the game saves its objects, the editor rebuilds the Rust code, and the game starts again from the saved objects, so a changed system runs on the same scene. Startup systems and `once` blocks do not run again. If a saved component no longer fits its changed type, the game starts clean and the Console says why.
- Scenes can contain other scenes, like prefabs. The `rusting.scene_instance` component places a saved scene under an object when the scene loads, with stable IDs and entity references pointed at the copies. Text saves keep only the link; cooked scenes include the placed objects. Instances can nest, and a scene that contains itself fails to load.
- Changes to the objects of a scene instance are saved as overrides: only the changed fields, per object. Everything else follows later edits to the source scene. Deleted objects stay deleted, and objects added under an instance are kept.
- Scene variants: **New Variant** in the Assets menu of a `.rscene` file (or `save_scene_variant` in Rust) writes a scene that inherits another one. The variant's changes and a level's changes to a placed variant both save as overrides and stack on top of the base.
- Instance actions in the Hierarchy right-click menu: Revert Object, Revert Instance, Apply to Source (other instances of the scene keep their own changes), and Unpack Completely, all with undo (`edit_instance` in Rust).
- Signals: named Rust handlers (`App::add_signal_handler`) connected per entity (`Connections`, `App::connect`) answer typed `EntityEvent`s and component additions and removals. Connections survive snapshots and replays, and scenes save them as `rusting.connections`, checked when a game loads the scene.
- Fast class lookups: `ClassIndex` (or `App::class_members`) lists the objects in a class, the engine's groups and tags, without scanning the scene.
- `SceneTree`, a system parameter for finding objects by name, by child name or `Arm/Hand` path, and by class. It returns entities, and your own queries read the components.
- Game code places scenes at runtime: `AssetServer::load_prefab` returns a `Handle<Prefab>`, and `spawn_prefab(world, handle, transform)` adds a copy with repeatable IDs.
- The editor's Assets panel places a scene in the open one by double-click or `Instance in Scene`, with undo.
- Render Settings can now change anti-aliasing (Auto, Off, MSAA 2x, MSAA 4x) and shadow quality (Auto, Low, Medium, High) while the editor runs. `RenderSettings` has the matching `antialiasing` and `shadows` fields.
- Data assets, like Godot resources: a type that implements `DataAsset` and is registered with `App::register_data_asset` saves as a `.rdata` file. `AssetServer::load_data` shares one copy per file, and `Handle` fields in components and in other data assets save as paths. The editor's Assets panel creates them with **New** and edits them in the Inspector.
- Unique data assets: a data asset reference can keep its own copy, saved inside the object as `{"$data": value}` instead of a file path. Each copy loads separately, so objects and prefab instances never share it. The Inspector's `Unique` choice copies a file's values into the object, and `AssetServer::duplicate_data` does the same in game code.
- Data assets hot reload: a running game reads a changed `.rdata` file again within half a second, into the handles it already has. A broken file keeps its last values and reports the error.
- Add Object has an **Environment and UI** category that creates a World Environment (sky, ambient light and tone mapping) or a HUD Element as its own object.
- The editor shows `rusting.tile_map` tiles and the `rusting.background` color while editing, not only in Play. Clicking a tile selects its tile map; tiles never appear in the Hierarchy or the saved scene.
- Tile Painter: select a tile map and pick a brush in the Inspector, then click or drag in the Scene View to paint or erase cells. Each stroke is one Undo step, and the map grows to fit cells painted past its edge. `TileMap::cell`, `cell_at` and `set_cell` do the same from code.
- The File and Edit menus show the key bound to Save Scene, Undo and Redo next to each entry.
- `GameScene::snapshot` and `restore` save and bring back the scene mid-game, with velocities, sleep and solver state.
- `GameScene::reset_body`, `set_angular_velocity`, `angular_velocity`, `set_look`, `initial` (the starting transform, color and body kind) and `state_hash(class)`.
- The prelude exports `Entity`, `World`, `Name`, `RigidBody`, `PlayerController`, `PhysicsWorld`, `GameSnapshot` and `InitialState`.
- Scenario `log` steps record a value, every tick with `until`, and `rusting test --json` lists them. `rusting run --ticks N --json` reports `timings.headless_ms_per_tick`.
- Materials have `uv_scale` and `uv_offset`, so a texture can repeat across a long floor instead of stretching.
- `PlayerController` has `max_slope`, `max_step_height` and `push_bodies`; `PhysicsWorld::move_character_on_foot` walks with the first two.

### Changed

- Objects with the same mesh and the same textures now share one instanced draw even when their materials differ in color, roughness, emissive and the like; those already travel per instance. Reflective (roughness below 0.5) materials and blended ones still batch apart. Same Shift went from 97 to 18 draws and from 0.56 to 0.22 ms of command recording.

- `rusting scene patch` rejects a field a built-in section does not have (for example `intensity` on `directional_light`) and lists the known fields. It used to drop the field and fill the default silently. The schema default for `mesh_renderer.material` now lists `transmission`, `ior`, `thickness`, `uv_scale` and `uv_offset`.

- Windowed games request at least 4 swapchain images (was 2). With 2 images the Wayland compositor throttled Immediate presentation. Same Shift went from 355 to 525 fps; render scale, antialiasing, shadows and quality made no difference before the change.

- Blended materials use premultiplied alpha: reflections and highlights keep their full strength, so a transparent material works as clear glass.
- The Inspector shows components by short name, such as "Sky Light" instead of `rusting.sky_light`, and sorts Add Component by that name.
- Add Component no longer offers environment settings, HUD elements or tile maps on meshes, cameras, lights or each other. A second sky, ambient light, tone mapping or background is greyed out, because the renderer reads only the first.
- Render Settings calls the device limit "Max MSAA".
- New projects and the samples build dependencies optimized in debug builds, so CPU physics runs at full speed during development.
- When fixed steps take longer than the time they simulate, the game runs in slow motion with one warning instead of freezing.
- `set_body_kind` to `Kinematic` or `Fixed` stops the body.

### Fixed

- Fluid coupling visits bodies in spawn order. Removing a fluid volume removes its particle entities, a copied volume makes its own, and snapshots restore fluids.
- Cooked scenes are format 8; format 7 files still load.
- The game window no longer fails on a display that allows fewer than four swapchain images.
- Fast bodies stop at the right distance from hull and mesh colliders. A player stepping onto a ledge measures the step at the contact point. A player with `push_bodies` off no longer lets crates fall through it.
- The slow-motion clamp follows `time_scale` and never changes a replay.
- `set_scene` refuses the scene root path. Whole numbers in scenario `equals` compare exactly. The headless ms-per-tick figure no longer includes scene load. `rusting schema` lists `pointer` under scenario steps and takes its `mesh_renderer` default from the material defaults.

- `rusting export` copies the project's `scenes/` folder, so a game that loads a second scene works from the export.

- `rusting test <project>` runs that project's `tests/` folder instead of reading the folder as a scenario, and scenario paths are looked up in the project folder first.
- Editor: hidden objects no longer draw a camera or light shape, cannot be clicked in the Scene view, and are left out of Home framing.
- Editor: a Tile Painter stroke that changes no cell no longer adds an Undo step.
- A fast CPU body that hit a dynamic body stopped dead instead of pushing it, and a ball could slip between two boxes through the seam its center passed. The sweep now casts against colliders grown by the ball's radius and hands a dynamic target the momentum.
- Hidden HUD elements, or elements under a hidden parent, are no longer drawn.
- HUD text no longer wraps when it grows longer (a score going from "9" to "14"); lines break only at `\n`.
- `GameScene::set_linear_velocity` wakes a sleeping body.
- CPU physics repeats exactly after `restart` or `load_scene`. Bodies were solved in entity order, which changed on every reload.
- Sleeping CPU bodies fall when the body under them is removed or moved.
- Textures on the side faces of built-in cubes were upside down. Cube UVs now follow glTF, with the image's first row at the top. Projects that flipped their images to compensate must flip them back.
- A scenario `expect` with tolerance 0 now passes for the value its failure message printed.
- The player controller no longer climbs boxes taller than `max_step_height` on the round bottom of its capsule.
- `rusting schema` describes scene texture slots as plain paths relative to the scene file.
- Lights on hidden objects, or under a hidden parent, no longer light the scene.
- Ambient light, sky light, tone mapping and the environment map now come from the entity with the lowest ID, as documented. The newest entity was used before.
- Textures now have mipmaps, and samplers read them. Before, every texture was sampled at full resolution only, so distant textures shimmered.
- Fixed a game crashing as it closed on Wayland when its runtime UI was on. The clipboard shut down after the window system connection had already closed.

## [1.4.0] - 2026-09-27

### Added

- Added the `rusting` command-line tool. It works without a window: `doctor` checks the setup, `new` creates a project, and `check`, `run`, `test` and `export` build, run, test and export a game.
- `rusting` can inspect projects and scenes, find objects by ID, name, class or component, and validate and cook scenes.
- Added `rusting scene patch` for atomic scene edits with dry-run diffs and revision checks. When the scene is open in the editor, the patch goes through undo and conflicts are reported instead of overwriting work.
- Added `rusting capture`, which renders a chosen camera at a chosen tick to PNG and tells which scene object is under each picked pixel.
- Added `rusting schema`, a generated catalog of every component, resource, asset type and command, with defaults, units, valid ranges and examples.
- Added input-driven test scenarios: named actions at fixed ticks, checks on game state and events, optional captures, fixed seeds, and a short trace from the first failing tick.
- Added reflection. Components, resources and assets describe their fields with the `reflect!` macro, with units, ranges, colors and docs as hints. Scene saving, the Inspector, the schema and field paths all use these descriptions.
- Reflection supports enums, nested structs, lists, optional values, string-keyed maps, entity references and typed asset handles.
- Entity references in components now save as object IDs and are reconnected on load. A reference to a deleted object loads as empty instead of failing. Asset handles save as asset paths.
- Renamed or removed component fields can be migrated with `migrate_scene_component`. Scenes with unknown fields, unknown enum variants or a newer version now fail with an error that names the component, the field and both versions.
- Added `DeterminismMode` (Off, Local and CrossPlatform) per project, with a startup check that every solver and custom shader supports the chosen mode.
- Added a shared simulation math module used by both CPU physics and GPU physics shaders, with defined float-to-integer conversion and no fast-math.
- Added a seeded random number generator with separate streams for each engine system.
- Added a world-state hash for every tick, including GPU physics bodies.
- Added headless simulation without a window or renderer, and a determinism runner that compares two runs and reports the first different tick and body.
- Added replays: recorded inputs with a seed and format version, replay verification against the recorded hashes, and fast seeking from saved snapshots.
- Added a determinism CI workflow that runs on changes to physics, math and shader code.
- Added a render benchmark with a fixed scene and camera path. It records frame times, GPU passes, draw calls, memory, uploads, physics and readback, and fails when a stored baseline gets worse.
- Added benchmark baselines for the RTX 3060 and llvmpipe, and documented the tested drivers and settings.
- Added a Profiler panel with CPU and GPU history graphs, CPU spans, GPU pass timings, counters and memory use.
- Added Render Settings and Physics Diagnostics panels.
- Added orbit, pan, dolly and Frame Selection (F) controls to the Scene View.
- Gizmos now have local and global modes and snapping for move, rotate and scale.
- Mesh Renderer can now pick meshes and materials from the loaded assets.
- Added a Restart button for the running game.
- Added an embedded preview that plays the scene inside the editor without a build, with Pause, Resume and Step. Objects created during preview are marked `[Runtime]`.
- Added image and glTF import with stable IDs, import settings, dependency reports, reimport, and source and license records.
- Added art-direction presets for lighting, camera, text and color.
- Added a first-person player controller with a camera.
- Added a game-feel kit: tweens with easing, sounds and particle bursts triggered by events, counters, pickups and a runtime HUD with text and buttons.
- Added in-game UI for games, separate from the editor UI.
- Added a quick 2D path: sprites, tile maps, an orthographic camera and a 2D starter scene.
- Added a starter game template with a scripted acceptance test.
- Added the vertical slice sample: an imported courtyard with PBR materials, a player, physics, lights, shadows, transparency and live asset reload.
- Added `RUSTING_VULKAN_DEVICE` to choose a GPU by index or name. A wrong choice now fails and lists every available device.
- Added feature flags for the editor, validation layers, experimental GPU physics and optional importers.
- Added a getting-started guide, a concepts guide, a determinism guide and four tutorials.

### Changed

- The Inspector now draws typed widgets for every registered component from its reflected description, including drop-downs for object and asset references. The raw JSON editor was removed.
- Scene component fields in `rusting schema` now come from the reflected types, so the schema cannot drift from the code. The schema catalog version is now 2.
- Registering a scene component now needs a `reflect!` description.
- More engine types moved into the `rusting-core` crate, which has no Vulkan dependency.
- The window is now created when the app resumes, which removes the last deprecated winit call.
- GPU physics buffers now grow with the body count and report overflow instead of dropping bodies.
- Selected objects now stay outlined when they are behind other geometry.
- Removed the old legacy renderer, pipeline and scene modules.
- Rewrote the editor guide. It now covers Scene View navigation, the transform gizmo, the Inspector, asset actions, Preview, the Profiler and diagnostics panels, and keyboard shortcuts. The README lists the current features and project layout.

### Fixed

- Fixed fixed-update steps in one frame all seeing the same tick number.
- Fixed CPU-to-GPU commands being applied at the wrong step when a frame ran several physics ticks.
- Fixed physics ticks being dropped when a frame needed more steps than the limit. They now run in the next frame.
- Fixed burst emitters without a scene ID all producing the same burst.
- Scenes with misspelled or removed component fields no longer drop that data silently.

---

## [1.3.0] - 2026-09-26

### Added

- Added a real Console panel with message filtering, log levels, repeated message counts and Clear button.
- Assets panel now shows image previews.
- Images can now be dragged directly onto objects and material texture slots.
- Inspector can now have custom sections for different component types instead of showing everything as raw data.
- Added editor text size setting. UI scale and text size are saved automatically.
- Added fully customizable keyboard shortcuts. Almost every editor action can now be rebound.
- Added Undo, Redo, Save, Delete and Rename shortcuts across the editor.
- Added automatic CPU/GPU physics mode. The engine can decide which one is better depending on the object and current scene.
- Added physics benchmark scenes for testing falling objects, stacks, debris and mixed scenes with different object counts.
- Added much more detailed renderer and physics performance statistics to the editor.
- GPU physics now has safer memory limits. If there are too many objects, the engine uses a slower fallback instead of breaking collisions.
- GPU objects can now properly collide with each other.
- GPU physics stacks are much more stable.
- Added GPU physics support for raycasts and interaction with CPU physics objects.
- Added Convex Mesh and Triangle Mesh colliders.
- Static level geometry can now use its real mesh for accurate collisions.
- CPU physics objects can now properly rotate from collisions and friction.
- Spheres can roll naturally.
- Box collisions are much more stable, especially for stacking.
- Added multiple GPU physics sync modes, so games can choose how much physics data should be copied back from the GPU.
- GPU objects can now receive commands like teleport, force, impulse and velocity changes.
- GPU physics state can now be manually read, saved, restored or reset.
- Added detection for lost GPU physics events when event memory becomes full.
- Custom GPU physics conditions can now be written with GLSL shaders.
- Added versioning for the GPU physics shader API to make custom shaders safer between engine updates.
- Added one-frame state snapshots for groups of GPU physics objects.
- Added shape casts for boxes, spheres and capsules.
- Added basic character movement with collision sliding and floor detection.
- CPU physics now uses a much faster collision search for large scenes.
- Custom GPU physics solvers now work during normal gameplay.
- Custom physics shaders can completely control how selected GPU objects move.
- GPU objects can now collide with CPU objects and static level geometry.
- GPU collisions support friction, bounce and collision layers.
- Added full CPU rigid body simulation for boxes, spheres and capsules.
- CPU objects can now fall, bounce, rest, sleep and wake up.
- Added proper raycast and overlap queries for CPU physics.
- Fast CPU objects now use collision protection to reduce tunneling through thin walls.
- Added Windows export from Linux and macOS.
- Added better Vulkan debugging information for graphics debugging tools.
- Added renderer statistics for draw calls, triangles, visible objects, uploads and GPU memory.
- Added CPU frame timing statistics for physics, rendering preparation and editor UI.
- Added GPU timing statistics for individual rendering stages.
- Added MSAA anti-aliasing to improve edge quality.
- Added anisotropic texture filtering for sharper textures viewed at an angle.
- Improved glTF scene spawning so imported objects receive proper scene IDs.

### Changed

- Dragging multiple selected objects in the Hierarchy now moves the entire selection together.
- The Scene View is now rendered as a normal editor panel, so menus and popups display correctly over it.
- Keyboard shortcut handling has been improved across the whole editor.
- The old GPU physics test solver is now called Grid Collision.
- Physics modes were renamed to simpler names: CPU and GPU.
- glTF import now adds the complete model hierarchy to the scene, including meshes, materials, cameras and lights.
- Imported glTF models now behave much more like normal scene objects.
- Assets panel has been redesigned into a file tree similar to Godot's FileSystem panel.
- Models can be double-clicked or dragged into the Scene View.
- Images can be dragged directly onto selected objects.
- Reusing an image no longer creates unnecessary duplicate materials.

### Fixed

- Fixed editor actions breaking when using the built-in white/error material.
- Fixed shadow acne that caused stripes and triangles on lit surfaces.
- Camera movement no longer accidentally clicks or types into editor UI.
- Fixed GPU physics stacks slowly sinking through each other.
- Improved GPU collision consistency and stability.
- Fixed GPU physics events disappearing when several physics updates happened during one frame.
- Fixed major editor slowdown when selecting objects with very large meshes.
- Fixed incorrect lighting on glTF models that do not include normal data.
- Fixed GPU grid collisions producing different results between runs.
- GPU physics events now arrive in a consistent order.
- Improved performance of GPU physics event rules.

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
