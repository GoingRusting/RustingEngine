# Editor guide

The editor window is split into areas, as in Blender. Each area shows one
editor type, such as Scene View, Hierarchy, or Inspector, and you can split,
resize, and change areas freely. Every change to the scene can be undone.

- [Projects](#projects)
- [Areas and layout](#areas-and-layout)
- [Scene View](#scene-view)
- [Hierarchy](#hierarchy)
- [Inspector](#inspector)
- [Assets](#assets)
- [Preview and Play](#preview-and-play)
- [Code Editor](#code-editor)
- [Exporting a game](#exporting-a-game)
- [Profiler and diagnostics](#profiler-and-diagnostics)
- [Keyboard shortcuts](#keyboard-shortcuts)

## Projects

Start the editor from the engine folder:

```bash
./scripts/run_editor.sh
```

The Project Manager appears first.

1. To create a game, press `New Project...`, choose the parent folder, type
   the project name, and press `Create Project`. The editor makes a new
   folder and refuses to overwrite an existing one.
2. To open a game, select a recent project or press
   `Browse for existing project...` and choose its folder.
3. Press `Projects` in the top bar to change projects later.

A new project contains its `Cargo.toml`, `project.json`, Rust entry point,
main scene, `assets` folder, `shaders` folder, and `build` folder. See
[Getting started](docs/getting-started.md#what-is-in-a-project) for what
each one holds.

The top bar groups commands into menus:

- `File` holds project and scene files. `Save Project` writes the open Rust
  source and the scene. `Save Scene` and `Save Scene As...` write only the
  scene. The editor asks before it replaces a scene with unsaved changes.
- `Edit` holds Undo and Redo.
- `View` holds area and layout commands, UI scale, and text size.

`Unsaved scene` appears beside the engine name while changes still need to
be written.

## Areas and layout

Each area has a header with an editor-type drop-down.

- Click a header to select its area. A blue border marks the selected area.
- Choose the editor type from the header drop-down: Scene View, Game View,
  Code Editor, Hierarchy, Inspector, Project Settings, Console, Assets,
  Keyboard Shortcuts, Profiler, Render Settings, or Physics Diagnostics.
- Press `↔` to split an area left and right, or `↕` to split it top and
  bottom. `Add Area` in the `View` menu splits the selected area left and
  right.
- Drag the divider between two areas to resize them.
- Press `×` to close an area. The area beside it takes the space.

For example, to put Code Editor beside Scene View, select the Scene View
area, press `↔`, and choose `Code Editor` in the new area's drop-down.

`View` > `Save Layout` writes the layout to `editor_layout.json` in the open
project, and `Load Layout` restores it. `Reset Layout` returns to the
default layout.

Only one Scene View or Game View renders at a time. If the layout has
several, the first one in the layout draws and the others show a notice.
Other editor types can appear any number of times.

## Scene View

Scene View shows the scene through the editor camera. Game View shows it
through the scene's active camera.

### Moving the camera

| Action | Input |
| --- | --- |
| Fly | Hold the right mouse button, then `W` `A` `S` `D`, `Space` up, `Ctrl` down, `Shift` faster |
| Fly without holding a button | `Numpad 0` turns fly mode on and off |
| Orbit around the view center | Hold the middle mouse button |
| Pan | Hold `Shift` and the middle mouse button |
| Zoom toward the view center | Mouse wheel |
| Frame the selection | `F`, or `Frame` in the toolbar |

While the camera is flying or orbiting, the rest of the editor ignores the
mouse and keyboard.

### Selecting and transforming

Click an object to select it. `Shift`-click adds objects to the selection
or removes them from it.

The transform gizmo works like Blender's:

1. Press `G` to move, `R` to rotate, or `S` to scale the selection. You can
   also use the toolbar buttons, or drag a gizmo handle.
2. Press `X`, `Y`, or `Z` to lock the change to one axis.
3. Click to confirm. Right-click or `Escape` cancels and restores the old
   transform.

The toolbar has two toggles:

- `Snap` rounds moves to 1 unit, rotations to 15°, and scales to steps of
  0.1.
- `Global` and `Local` choose whether the gizmo follows world axes or the
  object's own axes.

A whole gizmo drag is one undo step.

## Hierarchy

Hierarchy lists the scene as a tree. Each child appears under its parent,
indented one level, with branch lines.

- Press `+` at the top of Hierarchy to add an Empty Object, a Perspective
  Camera, or a light.
- Click to select. `Ctrl`-click adds or removes one object. `Shift`-click
  selects a range.
- Drag a row onto another row to make it a child. A red outline means the
  move is not allowed, for example onto the object's own child. Dragging a
  selected row moves the whole selection.
- Press the eye icon to hide or show an object.
- Right-click a row to add a child object, rename, duplicate, delete, or
  move the object to the scene root. Deleting a parent also deletes its
  children.
- Rename opens a text field on the row. `Enter` confirms it and `Escape`
  restores the old name.
- Type in the filter field to find objects by name.

## Inspector

Inspector shows the selected object's components, each in its own section.

- Transform, Mesh Renderer, Material, Camera, lights, and Physics have their
  own editors.
- Every other component, including your own game components, gets typed
  fields generated from its `reflect!` description: numbers with their unit
  and valid range, colors, check boxes, drop-downs for enums, and lists
  with `+` and `-` buttons. See
  [Tutorial 3](docs/tutorials/03-gameplay-plugin.md#2-write-the-plugin)
  for how to describe a component.
- A field that refers to another scene object is a drop-down of the
  scene's objects. A field that refers to a texture or mesh is a drop-down
  of assets that are already loaded.
- `Add Component` at the bottom adds Physics or any registered game
  component. The `×` button in a section header removes that component.
- `Classes` holds the object's class names, which scene patches and
  scenarios use to find groups of objects.

A continuous drag in the Inspector is one undo step, not one step per
frame.

## Assets

Assets lists the files in the project's `assets` folder. Press `Import` to
copy models and images into it. Import never overwrites a file that is
already there. Images load as textures. Each triangle primitive in a GLB or
glTF file becomes a mesh and a material, with a reloadable `.rmesh` file
written beside the model.

- Double-click a model, or drag it into Scene View, to add it to the scene.
- Double-click an image to use it as the base color of the selected
  object. You can also drag it onto a Hierarchy row or an Inspector texture
  slot.
- Right-click a file for more actions. `Replace…` swaps an imported file
  for a new one of the same type, and shows its size and which objects use
  it before you confirm.
- Type in the filter field to find a file.

## Preview and Play

The editor runs a scene in two ways:

- **Preview** runs the scene inside the editor without compiling the
  project. Use `Pause`, `Resume`, and `Step` to go one fixed tick at a time.
  `Stop Preview` returns the scene to exactly how it was before Preview.
- **Play** saves and cooks the scene, compiles the project's Rust code, and
  starts the game in its own window. `Stop` ends the game or the build, and
  `Restart` rebuilds and runs again.

Choose `Debug` beside Play for fast compiles while you work, and `Release`
for full optimization. Build output and panics from the game appear in
`Cargo Output` in Code Editor.

## Code Editor

Code Editor edits the project's Rust source. `Check` saves the source and
runs `cargo check` in the background. Errors and warnings appear in
`Cargo Output`, and the editor stays responsive while Cargo runs.
`Build & Run` does the same as Play.

## Exporting a game

Open Project Settings and press `Export Game...`, then choose a parent
folder. The editor saves and cooks the scene, builds the game in release
mode, and creates a new `<game>_export` folder. It never overwrites an
export: later exports are named `_2`, `_3`, and so on.

The export holds the executable, `build/main.rscene.bin`, the project's
`assets` folder, the RustingEngine license, and a short README. All paths
are relative to the export folder, so it runs without the project or the
editor. Results appear in `Cargo Output`.

## Profiler and diagnostics

- **Profiler** graphs the last 180 frames. It splits CPU time into physics,
  extraction, preparation, recording, and editor work, and shows GPU pass
  times, draw counts, and culling results. GPU times can lag the CPU sample
  by a frame or more.
- **Render Settings** holds the scene's render settings, VSync, the FPS
  limit, the GPU in use, and rendering diagnostics.
- **Physics Diagnostics** shows which physics backends are available, how
  many bodies run on the GPU and the CPU, the GPU work and readback each
  frame costs, and what falls back when a capacity limit is reached.

## Keyboard shortcuts

These are the default keys. Open a Keyboard Shortcuts area to change them:
click a shortcut, then press the new key. `Escape` cancels. The keys are
saved to `editor_shortcuts.json` in your user config folder and apply to
every project.

| Action | Default key |
| --- | --- |
| Undo, redo | `Ctrl+Z`, `Ctrl+Shift+Z` |
| Save scene | `Ctrl+S` |
| Rename selection | `F2` |
| Delete selection | `Delete` |
| Move, rotate, scale | `G`, `R`, `S` |
| Lock to one axis | `X`, `Y`, `Z` |
| Frame selection | `F` |
| Fly mode on and off | `Numpad 0` |
| Fly | `W` `A` `S` `D`, `Space` up, `Ctrl` down, `Shift` faster |

No shortcut fires while a text field has focus.
