# Changelog

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
