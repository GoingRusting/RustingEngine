# Editor GUI

The editor uses an area tree similar to Blender. An area is either a leaf that
shows one editor type, or a split containing two more areas. This keeps layout
state independent from the ECS world and Vulkan renderer.

## Quick start

Start the editor from the engine folder:

```bash
./scripts/run_editor.sh
```

The Project Manager appears when the editor starts:

1. To create a game, press `New Project...`, choose the parent folder, type the
   project name, and press `Create Project`. The editor never silently uses its
   own working directory.
2. To open a game, select a recent project or press
   `Browse for existing project...` and choose its folder.
3. Press `Projects` in the top toolbar whenever you want to change projects.

A created project includes its Cargo file, `project.json`, Rust entry point,
main scene, assets folder, shader folder, and build folder. The editor refuses
to overwrite an existing project folder. `Save Project` writes the open source
file and scene. `Save Scene` and `Save Scene As...` intentionally affect only
the scene and are named explicitly to avoid mixing the two operations.

## Editing a scene

- The compact top bar groups project and scene actions under `File`, Undo/Redo
  under `Edit`, and area/layout actions under `View`. `Play` and its build
  profile stay directly visible because they are used frequently.
- Use `New Scene`, `Open Scene...`, `Save Scene`, and `Save Scene As...` in the
  `File` menu for scene files. The editor asks before an unsaved scene is
  replaced.
- Open `Add Object` at the top of Hierarchy and choose Empty Object, Cube, or
  Camera. Related creation actions share one wide styled popup.
- Hierarchy is a parent-first tree: every child appears immediately below its
  parent, moves right by one indent level, and keeps visible branch lines.
- Right-click an object row to Rename, Duplicate, or Delete it. Rename opens an
  inline field on that row; Enter or Apply confirms it, while Escape or Cancel
  restores the old name.
- Select an object to inspect it or change its parent. Use its right-click menu
  to rename, duplicate, or delete it. Deleting a parent also deletes children.
- Use `Undo` and `Redo` in the top toolbar. A continuous Inspector drag is
  stored as one undo step instead of one step for every rendered frame.
- `Unsaved scene` appears beside the engine name while changes still need to
  be written.

## Importing assets

Change an area to `Assets`, then press `Import Files...`. Selected files are
copied into the current project's `assets` folder without overwriting files
that already exist. Images are loaded as typed textures. GLB/glTF triangle
primitives are imported as meshes and materials, with reloadable `.rmesh`
files generated beside the source model.

Select a scene object and press `Use on Selected` beside a loaded texture or
glTF primitive. Texture assignment creates a material for that object; glTF
assignment adds or replaces its Mesh Renderer. Use the Filter field to find a
project file quickly.

## Checking Rust code

In Code Editor, `Check` saves Rust source and runs a real `cargo check` in the
background. Compiler errors and warnings appear in `Cargo Output`, while the
editor and 3D view stay responsive.

`Play` in the top toolbar and `Build & Run` in Code Editor use the same native
game workflow: they save code and scene changes, cook runtime data, compile the
project, and start it in a separate window. Select `Debug` beside Play for fast
iteration, or `Release` for slower compilation with full optimizations. Output
and Rust panics from the running game are shown in `Cargo Output`.

## Exporting a game

Open `Project Settings` and press `Export Game...`, then choose a parent
folder. The editor saves and cooks the scene, builds the game with Cargo's
release profile, and creates a new `<game>_export` folder. Existing exports are
not overwritten; later exports receive `_2`, `_3`, and so on.

The export contains the native executable, `build/main.rscene.bin`, the
project's `assets` folder, RustingEngine's license, and a short README. Runtime
scene and asset paths are relative to this folder, so the exported game does
not depend on the original Cargo project or editor installation. Build and
export results appear in Code Editor's Cargo Output.

To change one area:

1. Click its top bar. A blue border shows that it is selected.
2. Open the dropdown in that bar.
3. Choose `Scene View`, `Code Editor`, `Console`, or another editor type.

To place Code Editor beside Scene View:

1. Select the Scene View area.
2. Press `↔` to split it left/right.
3. In the new area's dropdown, choose `Code Editor`.
4. Drag the divider to give more space to the view you are using.

Open `View` and press `Save Layout` when the arrangement is useful. The editor saves it as
`editor_layout.json` inside the current game project. Press `Load Layout` after
restarting the editor. `Reset Layout` returns to the original arrangement.

## Using areas

- Click an area's header to select it. The blue border marks the selected area.
- Use the dropdown in the header to choose Scene View, Game View, Code Editor,
  Hierarchy, Inspector, Project Settings, Console, or Assets.
- Press `↔` to split an area left/right or `↕` to split it top/bottom.
- Drag the divider between two areas to resize them.
- Press `×` to close an area. Its sibling expands into the released space.
- `+ Area` in the main toolbar splits the selected area left/right.
- `Reset Layout` restores the default hierarchy/scene/inspector/project layout.
- `Save Layout` writes `editor_layout.json` into the selected game project.
  `Load Layout` restores it.

`Add Area` in the `View` menu splits the selected area without using its small
header button.

Only one live Scene View or Game View is rendered for now. If several viewport
areas exist, the first one in the layout tree is active and the others display
a notice. Other editor types can be duplicated freely.
