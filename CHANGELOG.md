# Changelog

## [Unreleased]

Engine features for the horror game FOREVER BEAR.

### Added

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

### Fixed

- The new screen and text goldens failed on the software renderer (lavapipe); they now pass there and on the RTX 3060
- Each camera screen ran the GPU physics again; screens now reuse the frame's physics, so six screens cost about 3 ms instead of 113 ms

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
