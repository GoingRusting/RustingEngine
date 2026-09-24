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

### Panels and tools

- Console with structured logs, filtering, and warnings (M6).
- Profiler with CPU spans, GPU pass timings, counters, and memory (M6, M20).
- Render settings panel (quality profile, capabilities) and physics
  diagnostics panel (M6).
- Settings panel for rebinding and persisting shortcuts; every command routed
  through the action map (M6).
- Project settings and input map editors (M20).
- Project-wide search (M20).

### Play workflow

- Stop and restart for the native game process (M6).
- Embedded preview without compiled Rust systems (M6).
- Remote scene tree and inspector for a running game (M20).

### Rendering integration

- Engine-owned egui texture/mesh upload path and render pass (M6).
- Golden-image test for editor compositing (M6 exit gate).
