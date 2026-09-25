# Editor overhaul

The owner asked for a large design and feature update of the egui editor,
aiming at a real-engine editor. The visual and interaction direction is
Blender + Godot. The work is split into waves; each wave ships working,
tested code before the next begins.

## Wave 1 (in progress): visual pass, inspector, hierarchy

### Visual design pass

- New `EditorTheme` palette: Godot-style dark neutral panels with one accent
  color, Blender-style compact spacing and area headers.
- Every dock area gets a header strip with an editor-type switcher (the
  `dock.rs` area tree stays the layout model).
- One toolbar style and one icon set across all panels.
- Theme and UI scale persist per user (roadmap Milestone 6, "DPI scaling,
  font configuration, and theme persistence").

### Inspector (property-row system)

- Module `src/editor/inspector/`:
  - `widgets.rs`: Godot-style property rows (label left, widget right) and
    shared widgets: vec3 with colored X/Y/Z fields, color, angle in degrees,
    enum, asset handle.
  - `json.rs`: custom (game-defined) components are stored as JSON. Their
    values render as typed widgets: numbers as drag values, bools as
    checkboxes, 3-number arrays as vec3, strings as text, nested objects as
    sub-sections. Raw JSON text editing is removed from the normal path.
  - `mod.rs`: the Inspector panel; built-in components are drawn as
    collapsible sections using the shared widgets.
- Every edit still flows through the snapshot undo system.

### Hierarchy

- Search/filter box.
- Per-type icons (camera, light, mesh, empty).
- Blender-style visibility eye toggle.
- Double-click inline rename.
- Drag and drop reparenting (roadmap Milestone 6).
- Right-click context menu: add child, duplicate, rename, delete.
- Multi-select (Ctrl toggles, Shift selects a range). The selection has a
  primary entity; the gizmo and Inspector act on the primary, delete and
  duplicate act on the whole selection.

### Wave 1 progress

Done (unit tests in `editor::hierarchy`, `editor::inspector::{widgets, json}`):

- Palette, compact density, flat list rows, area chrome.
- Hierarchy: search that keeps ancestors of matches, per-type icons, eye
  toggle (`EntityRequest::SetVisible`, undoable), double-click / F2 rename,
  Delete or X deletes the selection, multi-select with Ctrl and Shift and a
  primary object (`EditorState::selection` + `selected`), delete and
  duplicate act on the whole selection.
- Inspector module with property rows, axis-colored vec3, color, angle,
  choice, and Godot-style collapsible sections; custom JSON components are
  edited as typed fields live, and drags coalesce into one Undo step.

- UI scale persists per user (`EditorPreferences`, View > UI Scale). There
  is one palette, so no theme choice is stored yet.
- Editor-type icon in each area header.
- Hiding a parent hides its children in rendering.
- Viewport shows bounds for every selected object (primary in yellow, others
  in orange); Shift-click in the viewport adds, activates, or deselects
  Blender style.
- Hierarchy rows hidden by themselves or an ancestor are dimmed.
- Scene View mouse navigation: middle drag orbits around a pivot in front of
  the camera, Shift + middle drag pans, the wheel dollies toward the pivot,
  and F (focus) moves the pivot onto the selected object. Right drag still
  flies.
- Project panel "Scene Rendering" section: Quality (Auto, Eco, Balanced,
  High) and Culling (Auto, Off, Frustum, Frustum + Occlusion). Both are
  saved in the scene file (format 6) and load with the scene in the
  exported game. Edits are undoable scene changes (unit test
  `scene_render_settings_edits_are_undoable_scene_changes`).
- Inspector "Material" section for the selected Mesh Renderer: model, alpha
  mode and cutoff, base color and opacity, metallic, roughness, emissive,
  and the five texture slots. Each slot picks a loaded texture or loads an
  image file (copied into the project `assets` folder). "New Material"
  gives the object its own default material. Editing a material that other
  renderers share copies it first. Edits are undoable (unit test
  `material_edits_copy_shared_materials_and_edit_owned_ones_in_place`).
- Native file dialogs run on a worker thread (`editor::file_dialogs`), so
  the window keeps answering the window manager and is not reported as "not
  responding". While a dialog is open, a small modal blocks the panels.
- Cameras and lights draw Blender-style wire shapes in the Scene View
  (camera frustum with an up triangle, sun with rays and a direction line,
  point light circles, spot cone) in the selection colors. Clicking within
  6 points of a shape selects the object (unit test
  `clicks_near_light_and_camera_shapes_select_them`).

Open:

- Toolbar consistency pass.
- egui's built-in Ctrl +/- zoom is not persisted; only View > UI Scale is.
- Asset handle fields show the raw key; show the asset path and accept drops
  once the asset browser exists.

## Backlog (later waves)

Kept here so it is not lost between sessions. Roadmap milestones that own an
item are named in parentheses.

### Scene editing UX

- Scene viewport rendered to an editor texture (M6).
- Frame all (Home) and Blender numpad views (front, side, top) (M6).
- Selection outline readable behind other geometry (M6).
- Camera and light shapes: the camera frame is fixed at 16:9, the point
  light has three circles instead of a view-facing one, and there are no
  per-type icons. Hidden objects still draw their shape.
- Gizmo polish: local/global modes, snapping, visual restyle (M6).
- Multiple viewports and orthographic views (M20).
- Route keyboard and mouse focus between viewport navigation and UI (M6).

### Asset workflow

- Asset browser with folders, thumbnails, filtering, and drag and drop onto
  the viewport, the Hierarchy, and Inspector handle fields (M6).
- Wire editor glTF import to the full node-scene import (hierarchy, cameras,
  lights) from Milestone 2.
- Assign meshes, materials, textures, and physics shapes by typed handle (M6).
- Per-asset import settings and re-import (M20).
- Image textures always load as sRGB, both from the Inspector and from
  scene files. Normal, metal/rough, and occlusion maps need a linear load
  path, and the scene file needs to store the color space per slot.
- Material assets as files (`.rmaterial`) that several scenes share. Today
  every scene stores its materials inline.
- LOD group authoring: edit a mesh's `.rlod` levels and hand-over values in
  the Inspector, preview the active level in the viewport, and hot reload
  `.rlod` files. Import glTF `MSFT_lod` into LOD groups.

### Panels and tools

- Console with structured logs, filtering, and warnings (M6).
- Profiler with CPU spans, GPU pass timings, counters, and memory (M6, M20).
- Render settings panel (quality profile, capabilities) and physics
  diagnostics panel (M6). It should show `RenderCapacityDiagnostics`,
  including the missing mesh, material, and texture counts.
- Environment panel for the scene-wide `AmbientLight`, `SkyLight`, and
  `ToneMapping` components, like Godot's `WorldEnvironment` (M6).
- Settings panel for rebinding and persisting shortcuts; every command routed
  through the action map (M6).
- Project settings and input map editors (M20).
- Project-wide search (M20).

### Play workflow

- Stop and restart for the native game process (M6).
- Embedded preview without compiled Rust systems (M6).
- Remote scene tree and inspector for a running game (M20).

### Rendering integration

- Engine-owned egui painter is done (`rendering::egui_painter`). Texture
  uploads wait on their own submission; replace with a staging ring if the
  stall shows up in profiles.
- Golden-image test for editor compositing (M6 exit gate).
- Show the resolved quality profile when Quality is Auto, and the LOD
  groups loaded next to meshes, in the stats area.
- Profiler panel: today `CullingStats` is one line in the stats area.
  Add a panel with per-pass GPU timestamps and a history graph.
- Render Bounds are edited as numbers in the Inspector only. Add viewport
  handles to drag box faces and the sphere radius, plus a "Fit to Mesh"
  action. Multi-selection edits only the active object.

### Deferred from the `issues.md` audit

Items the audit found but left open, with the reason for each.

- Partial instance-buffer uploads. They need one instance buffer per frame
  in flight plus dirty tracking. The unused `dirty_ranges` code was removed.
- Reuse scratch `Vec`s in extraction, and avoid cloning blended instances
  before sorting. Minor cost.
- Sweep file-backed meshes and textures after a scene `Replace` load. Ten
  call sites load with `Replace`, and game code can hold path-loaded handles
  without `retain`, so an automatic sweep could free live assets. It needs
  handle ownership first.
- Make `Assets::len()` O(1) and move the hot-reload file scan to a worker.
  Both scans are small today.
- Remove the legacy engine's double fence wait. Removing it creates a
  write-after-read hazard on the indirect and physics buffers against the
  previous frame's draw. Check on real hardware with validation layers.
- Rewrite the legacy camera sign convention and remove `camera_rotate`. Needs
  visual checks. Only the `cgmath` removal was done.
- Replace the remaining `unwrap` calls in the legacy engine with errors.
- Enable device features at device creation instead of assuming them.
- Cache the Hierarchy tree and rebuild it only on change detection.
- Check the resource-state hazard with validation layers on real hardware.
