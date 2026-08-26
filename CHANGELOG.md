# Changelog

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

## [0.1.47] - 2026-08-25

### Added

- More settings in gui like: Game/Scene/Code modes
- Adding native Rust game projects that can be edited and built from GUI
- Adding simple Rust scene API to move and edit objects without ECS boilerplate
- Adding Blender-style editor areas that can be split, resized and changed to another panel type

### Fixed

- Gui design a bit improved

## [0.1.46] - 2026-08-24

### Added

- More settings and features in Editor GUI like FPS limit controls
- Little custom compiler from GUI scene to optimized no GUI game/simulation

### Fixed

- Rewriting architecture a bit to prepare for scaling

## [0.1.45] - 2026-08-24

### Added

- Added first version of ECS runtime with schedules, hierarchy and stable entity identities
- Added typed asset handles, render extraction and revision-aware GPU mesh cache
- Added first version of egui editor with hierarchy, inspector, camera settings, play controls and live 3D Vulkan viewport
- Added roadmap and architecture documentation for future engine implementation
- Added more tests for runtime, assets, GPU layouts and perspective projection

## [0.1.44] - 2026-08-24

### Added

- A lot of tests

### Fixed

- A lot of different fixes to improve stability before big implementation

## [0.1.43] - 2026-04-11

### Added

- Textures can be applied on engine build in shapes

### Fixed

- Now textures can be reused to save VRAM

## [0.1.42] - 2026-04-11

### Added

- A lot of docs for better user experience( description of functions/values on hover )

### Fixed

- Culling is now working much better, without bugs.
- Fixing multi gltf model import, now each texture and model render good even if there are 10k gltf models. But each texture is separate, so you cant reuse texture without vram loss, I will fix it so fast as possible.

## [0.1.41] - 2026-04-6

### Added

- Added gltf models import, already with Materials, Textures and everything that needed

## [0.1.4] - 2026-04-5

### Added

- Better code, now no warnings( before was like 60 )
- Added new very heavy fragment shader with noise and other things
- Culling toggle on 'C'

### Fixed

- Culling is working well and give insane performance boost on big scenes where fragment/vertex shader is heavy. But it uses object center, so some object might disappear earlier as needed. I will fix it in next patch

## [0.1.32] - 2026-04-5

### Added

- Added culling( when mesh is not in view => dont render ), but its beta, so its working bad

## [0.1.31] - 2026-04-4

### Added

- Just made code cleaner and only fixed some problems in shaders

### Fixed

- Grid collision shader

## [0.1.3] - 2026-04-3

### Added

- Optimizing physic shaders and fragment shaders render. Collision check was **O(n²)**, now it splitted on grid, so its **O(n*k) + O(n*j)** where k is objects count in cell and j is big objects count.
- adding more physic settings like **_friction_**, **_gravity direction_** and **_bounciness_**.

### Fixed

- Object collapse on stacking.

## [0.1.1] - 2026-04-1

### Added

- Collision types as enum (Sphere, Box).
- Optional apply one physic/visual shader for all object on scene

### Fixed

- Better performance( main loop refactoring )

## [0.1.0] - 2026-03-31

### Added

- Initial release!
- Different fragment shader support.
- Different physic shader support.
- Physics engine with per-object collision types (Box/Sphere).
- Same speed as pure Vulkano+winit
