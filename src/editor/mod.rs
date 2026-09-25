//! Feature-gated egui editor state and ECS panels.
//!
//! The window runner calls [`draw_editor_view`] inside its egui frame and
//! composites the resulting shapes over the Vulkan scene.

mod dock;
mod file_dialogs;
pub mod gui_elements;
mod hierarchy;
mod icons;
mod inspector;
mod overlay;
mod picking;
mod project;
mod shortcuts;
pub mod view;

use dock::EditorLayoutFile;
pub use dock::{EditorDockNode, EditorPanel, EditorSplitAxis};
use file_dialogs::{DialogPurpose, FileDialogs};
use hierarchy::{collect_entities, draw_hierarchy_area};
pub use icons::EditorIcon;
use inspector::{draw_inspector_area, ComponentEdit};
pub use view::draw_editor_view;

pub use project::{
    create_project, open_project, EditorPreferences, OpenProject, ProjectError,
    ProjectManagerState, ProjectManifest, RecentProject,
    PROJECT_FORMAT_VERSION,
};
pub use shortcuts::{
    add_mouse_delta, handle_keyboard_input, handle_mouse_button_input,
    handle_mouse_wheel, release_editor_navigation, update_fly_camera,
    EditorFlyCamera, EditorShortcuts, EditorTransformMode, KeyBinding,
    SceneViewAction, ShortcutAction, ShortcutContext, TransformModes,
};

use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::{Resource, World};
use egui::{CentralPanel, Context, DragValue, TopBottomPanel};
use std::path::PathBuf;

use crate::rendering::debug_overlay::RenderDebugOverlay;
use crate::rendering::scene_renderer::SceneDebugView;
use crate::runtime::{
    add_registered_component, cook_scene, load_scene,
    registered_component_names, registered_component_values,
    remove_registered_component, save_scene, scene_document,
    set_registered_component, App, AppError, Camera, Collider, CollisionLayers,
    DirectionalLight, GlobalTransform, MeshRenderer, Name, ObjectClasses,
    Parent, PhysicsBackendStatus, PhysicsBody, Plugin, PointLight,
    RenderBounds, RenderCameraOverride, RenderSettings, RenderWorld, RigidBody,
    SceneDocument, SceneId, SceneLoadMode, SpotLight, Visibility,
};
use crate::Transform;
use crate::{
    AssetServer, Handle, ImportedGltfPrimitive, MaterialAsset, MeshAsset,
    PrimitiveShape, TextureAsset,
};

/// True when the editor must redraw every frame: the game is playing or the
/// fly camera moves without window events. Otherwise it redraws only on input,
/// egui repaint requests, and a slow idle tick.
pub fn editor_needs_continuous_redraw(world: &World) -> bool {
    world
        .get_resource::<EditorState>()
        .is_some_and(|state| state.mode == EditorMode::Play)
        || world
            .get_resource::<EditorFlyCamera>()
            .is_some_and(|fly| fly.active || fly.drag.is_some())
}

/// Editor view state saved beside a scene, like Godot's per-scene edit
/// state. The game never reads it, so the scene file stays clean.
#[derive(serde::Serialize, serde::Deserialize, Default, Debug, PartialEq)]
struct EditorSceneMetadata {
    camera_position: Option<[f32; 3]>,
    camera_rotation: Option<[f32; 3]>,
    orbit_distance: Option<f32>,
    /// Selected objects by `SceneId`; the first one is the active object.
    selection: Vec<uuid::Uuid>,
}

/// Sidecar path for a scene's editor metadata: `main.rscene` becomes
/// `main.rscene.editor.json`.
fn editor_metadata_path(scene: &std::path::Path) -> PathBuf {
    let mut path = scene.as_os_str().to_owned();
    path.push(".editor.json");
    PathBuf::from(path)
}

/// Saves the scene, then the editor camera pose and selection beside it.
/// A failed metadata write does not fail the save.
pub fn save_editor_scene(
    world: &mut World,
    state: &EditorState,
    path: &std::path::Path,
) -> Result<(), crate::runtime::SceneIoError> {
    save_scene(world, path, "Main Scene")?;
    let camera = state
        .editor_camera
        .and_then(|camera| world.get::<Transform>(camera));
    let selection = state
        .selected
        .into_iter()
        .chain(state.selection.iter().copied())
        .filter_map(|entity| world.get::<SceneId>(entity).map(|id| id.0));
    let mut metadata = EditorSceneMetadata {
        camera_position: camera.map(|transform| transform.position),
        camera_rotation: camera.map(|transform| transform.rotation),
        orbit_distance: world
            .get_resource::<EditorFlyCamera>()
            .map(|fly| fly.orbit_distance),
        selection: Vec::new(),
    };
    for id in selection {
        if !metadata.selection.contains(&id) {
            metadata.selection.push(id);
        }
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(&metadata) {
        let _ =
            crate::runtime::write_atomic(editor_metadata_path(path), &bytes);
    }
    Ok(())
}

/// Replaces the scene, then restores the editor camera pose and selection
/// saved beside it. A missing or unreadable sidecar leaves the camera as is
/// and clears the selection.
pub fn load_editor_scene(
    world: &mut World,
    state: &mut EditorState,
    path: &std::path::Path,
) -> Result<usize, crate::runtime::SceneIoError> {
    let count = load_scene(world, path, SceneLoadMode::Replace)?;
    state.selected = None;
    state.selection.clear();
    let Some(metadata) = std::fs::read(editor_metadata_path(path))
        .ok()
        .and_then(|bytes| {
            serde_json::from_slice::<EditorSceneMetadata>(&bytes).ok()
        })
    else {
        return Ok(count);
    };
    if let Some(mut transform) = state
        .editor_camera
        .and_then(|camera| world.get_mut::<Transform>(camera))
    {
        if let Some(position) = metadata.camera_position {
            transform.position = position;
        }
        if let Some(rotation) = metadata.camera_rotation {
            transform.rotation = rotation;
        }
    }
    if let (Some(distance), Some(mut fly)) = (
        metadata.orbit_distance,
        world.get_resource_mut::<EditorFlyCamera>(),
    ) {
        fly.orbit_distance = distance;
    }
    let mut query = world.query::<(Entity, &SceneId)>();
    let by_id: std::collections::HashMap<_, _> = query
        .iter(world)
        .map(|(entity, id)| (id.0, entity))
        .collect();
    state.selection = metadata
        .selection
        .iter()
        .filter_map(|id| by_id.get(id).copied())
        .collect();
    state.selected = state.selection.first().copied();
    Ok(count)
}

/// Applies the editor's compact dark workspace theme to an egui context.
/// Also restores the user's saved UI scale.
pub fn configure_editor_style(context: &Context) {
    gui_elements::EditorTheme::apply(context);
    context.set_zoom_factor(EditorPreferences::load().ui_scale);
}

/// Editor interaction mode. Edit state never advances gameplay fixed updates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditorMode {
    #[default]
    Edit,
    Play,
    Paused,
}

/// Cargo profile used when the editor starts the native game.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GameBuildProfile {
    /// Compiles quickly and keeps debug information for normal development.
    #[default]
    Debug,
    /// Takes longer to compile, but enables the game's optimizations.
    Release,
}

impl GameBuildProfile {
    /// Short name shown in the editor's Play selector.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Debug => "Debug",
            Self::Release => "Release",
        }
    }

    /// Cargo flag needed by this profile. Debug is Cargo's default.
    const fn cargo_flag(self) -> Option<&'static str> {
        match self {
            Self::Debug => None,
            Self::Release => Some("--release"),
        }
    }
}

/// Internal mode of the first live viewport. Areas use [`EditorPanel`] for
/// their independently selected content.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditorWorkspace {
    #[default]
    Scene,
    Game,
    Code,
}

/// Persistent editor selection, project state, and area layout.
#[derive(Resource, Clone, Debug)]
pub struct EditorState {
    /// Primary (active) selected object: the gizmo and Inspector act on it.
    pub selected: Option<Entity>,
    /// Every selected object. Always contains `selected` when it is set; the
    /// Hierarchy resets it to just `selected` when the two disagree.
    pub selection: Vec<Entity>,
    /// Name filter typed into the Hierarchy search box.
    pub hierarchy_filter: String,
    /// Tells the editor if the game is stopped, playing, or paused.
    pub mode: EditorMode,
    /// Chooses a fast Debug build or an optimized Release build for Play.
    pub game_build_profile: GameBuildProfile,
    /// Scene path relative to the selected project folder.
    pub scene_path: String,
    /// Folder that contains the current game project.
    pub project_root: String,
    /// Last save, load, layout, or component message.
    pub scene_message: Option<String>,
    /// True when the scene changed after its last successful save or load.
    pub scene_dirty: bool,
    /// Editable name copied from the selected object.
    pub rename_draft: String,
    /// Object currently showing an inline rename field in Hierarchy.
    pub rename_target: Option<Entity>,
    /// Whether the centered Add Object catalog is visible.
    pub add_object_modal_open: bool,
    /// Parent assigned to the next object created through the catalog.
    pub add_object_parent: Option<Entity>,
    /// New class name being typed in the Inspector.
    pub class_draft: String,
    /// Text being edited for custom serialized components.
    /// Camera mode used by the first live 3D area.
    pub workspace: EditorWorkspace,
    /// Camera used by Scene View instead of the game's camera.
    pub editor_camera: Option<Entity>,
    /// Copy of the scene restored when Stop is pressed.
    pub play_snapshot: Option<SceneDocument>,
    /// Source path relative to the selected project folder.
    pub code_path: String,
    /// Editable text loaded from `code_path`.
    pub code_source: String,
    /// Last message created by the Code Editor.
    pub code_message: Option<String>,
    /// True when code text changed but was not saved.
    pub code_dirty: bool,
    /// Tree that stores every editor area and divider.
    pub dock_layout: EditorDockNode,
    /// Area changed by the main toolbar and marked with a blue border.
    pub active_area: u64,
    /// Unique ID reserved for the next split area.
    pub next_area_id: u64,
}

/// Physical pixel rectangle occupied by the editor's live scene view.
///
/// This is intentionally editor-owned layout data. The window runner converts
/// it into a renderer viewport, keeping egui out of the rendering API.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EditorViewport {
    /// Top-left viewport position in physical window pixels.
    pub offset: [u32; 2],
    /// Width and height in physical window pixels.
    pub extent: [u32; 2],
    /// False when the layout does not contain a live 3D area.
    pub valid: bool,
    /// True when the pointer is over the view and no egui popup, menu, or
    /// viewport toolbar covers it.
    pub hovered: bool,
}

/// Options for helpers visible only while authoring in the Scene View.
///
/// These are not scene components. Saving a scene or running a cooked game
/// never includes a grid, selection axes, or any other editor helper.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct EditorGizmoSettings {
    /// Draw a ground grid on the XZ plane.
    pub show_grid: bool,
    /// Draw red X, green Y, and blue Z axes on the selected object.
    pub show_selected_axes: bool,
    /// Draw a yellow box around the selected render mesh.
    pub show_selected_bounds: bool,
    /// Diagnostic shading of the Scene View; Play always renders lit.
    pub shading: SceneDebugView,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GizmoAxis {
    X,
    Y,
    Z,
}

/// Translation drag retained across egui frames.
#[derive(Resource, Clone, Debug, Default)]
pub struct EditorGizmoDrag {
    pub mode: Option<TransformModes>,
    pub modal: bool,
    pub axis_mask: [bool; 3],
    pub active_axis: Option<GizmoAxis>,
    pub entity: Option<Entity>,
    pub start_pointer: Option<egui::Pos2>,
    pub original_transform: Option<Transform>,
    pub world_axis: [f32; 3],
    pub local_delta_axis: [f32; 3],
    pub screen_axis: [f32; 2],
    pub pixels_per_world_unit: f32,
    pub gizmo_axis_length: f32,
    pub origin_screen: Option<egui::Pos2>,
    pub world_axes: [[f32; 3]; 3],
    pub local_delta_axes: [[f32; 3]; 3],
    pub screen_vectors: [[f32; 2]; 3],
    pub rotation_screen_signs: [f32; 3],
    pub view_rotation_axis: [f32; 3],
    pub move_axis_origin: [f32; 3],
    pub move_axis_direction: [f32; 3],
    pub move_drag_plane_normal: [f32; 3],
    pub move_start_axis_parameter: Option<f32>,
    undo_snapshot: Option<SceneDocument>,
}

impl EditorGizmoDrag {
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.mode.is_some()
    }
}

/// Shading the renderer uses this frame: the Scene View's chosen shading,
/// or [`SceneDebugView::Lit`] in every other workspace.
pub fn editor_debug_view(world: &World) -> SceneDebugView {
    if world.resource::<EditorState>().workspace == EditorWorkspace::Scene {
        world.resource::<EditorGizmoSettings>().shading
    } else {
        SceneDebugView::Lit
    }
}

impl Default for EditorGizmoSettings {
    fn default() -> Self {
        Self {
            show_grid: true,
            show_selected_axes: true,
            show_selected_bounds: true,
            shading: SceneDebugView::Lit,
        }
    }
}

/// Editor-built geometry consumed by the renderer for the current frame.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct EditorDebugOverlay(pub RenderDebugOverlay);

/// Scene snapshots used by Undo and Redo.
#[derive(Resource, Clone, Debug, Default)]
struct EditorHistory {
    /// Scene states restored by Undo, newest last.
    undo: std::collections::VecDeque<SceneDocument>,
    /// Scene states restored by Redo, newest last.
    redo: Vec<SceneDocument>,
    /// Scene state from before the current continuous Inspector edit.
    pending_inspector: Option<SceneDocument>,
}

impl EditorHistory {
    /// Keeps history useful without growing memory forever.
    const MAX_SNAPSHOTS: usize = 100;

    fn push_undo(&mut self, document: SceneDocument) {
        if self.undo.back() != Some(&document) {
            self.undo.push_back(document);
            if self.undo.len() > Self::MAX_SNAPSHOTS {
                self.undo.pop_front();
            }
        }
        self.redo.clear();
    }
}

impl Default for EditorState {
    fn default() -> Self {
        Self {
            selected: None,
            selection: Vec::new(),
            hierarchy_filter: String::new(),
            mode: EditorMode::Edit,
            game_build_profile: GameBuildProfile::Debug,
            scene_path: String::new(),
            project_root: String::new(),
            scene_message: None,
            scene_dirty: false,
            rename_draft: String::new(),
            rename_target: None,
            add_object_modal_open: false,
            add_object_parent: None,
            class_draft: String::new(),
            workspace: EditorWorkspace::Scene,
            editor_camera: None,
            play_snapshot: None,
            code_path: "src/main.rs".into(),
            code_source: String::new(),
            code_message: None,
            code_dirty: false,
            dock_layout: EditorDockNode::default_layout(),
            active_area: 2,
            next_area_id: 5,
        }
    }
}

/// Switches every editor document path to one validated project.
fn set_open_project_paths(state: &mut EditorState, project: &OpenProject) {
    state.project_root = project.root.display().to_string();
    state.scene_path = editor_relative_path(&project.manifest.main_scene);
    let code_path = project
        .code_path
        .strip_prefix(&project.root)
        .unwrap_or(std::path::Path::new("src/main.rs"));
    state.code_path = editor_relative_path(code_path);
    // Never show text left over from the previously opened project.
    state.code_source.clear();
    state.code_dirty = false;
    state.rename_target = None;
}

/// Structured editor console entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsoleEntry {
    /// Controls the color and importance of this message.
    pub level: ConsoleLevel,
    /// Text shown in the Console area.
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsoleLevel {
    /// Normal status message.
    Info,
    /// Something may be wrong, but the editor can continue.
    Warning,
    /// An operation failed.
    Error,
}

#[derive(Resource, Clone, Debug, Default)]
pub struct EditorConsole {
    /// Messages kept in the order they were added.
    entries: Vec<ConsoleEntry>,
}

impl EditorConsole {
    /// Adds one message to the bottom of the Console area.
    pub fn push(&mut self, level: ConsoleLevel, message: impl Into<String>) {
        self.entries.push(ConsoleEntry {
            level,
            message: message.into(),
        });
    }

    #[must_use]
    pub fn entries(&self) -> &[ConsoleEntry] {
        &self.entries
    }

    /// Removes every old console message.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// Installs editor state, viewport data, and the message console.
///
/// Add this after render extraction when building an editor application:
///
/// ```no_run
/// # use rusting_engine::{App, EditorPlugin};
/// # use rusting_engine::runtime::RenderExtractPlugin;
/// let mut app = App::new();
/// app.add_plugin(RenderExtractPlugin)?;
/// app.add_plugin(EditorPlugin)?;
/// # Ok::<(), rusting_engine::runtime::AppError>(())
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) -> Result<(), AppError> {
        // These resources keep GUI data inside the same ECS world as the game.
        app.insert_resource(EditorState::default())
            .insert_resource(EditorViewport::default())
            .insert_resource(EditorGizmoSettings::default())
            .insert_resource(EditorGizmoDrag::default())
            .insert_resource(EditorDebugOverlay::default())
            .insert_resource(EditorShortcuts::default())
            .insert_resource(EditorTransformMode::default())
            .insert_resource(EditorFlyCamera::default())
            .insert_resource(EditorConsole::default())
            .insert_resource(EditorHistory::default())
            .insert_resource(PendingDestructiveAction::default())
            .insert_resource(EditorAssetState::default())
            .insert_resource(EditorBuildState::default())
            .insert_resource(ProjectManagerState::default())
            .insert_resource(FileDialogs::default());
        Ok(())
    }
}

/// Work requested by one Project Manager button.
#[derive(Clone)]
enum ProjectRequest {
    /// Creates a project inside the selected parent folder.
    Create { parent: PathBuf, name: String },
    /// Opens a folder that already contains a RustingEngine project.
    Open(PathBuf),
}

/// Action held while the editor asks what to do with unsaved changes.
#[derive(Clone)]
enum DestructiveRequest {
    NewScene,
    ReloadScene,
    OpenScene(PathBuf),
    OpenProject(ProjectRequest),
}

/// Keeps a confirmation request alive between GUI frames.
#[derive(Resource, Clone, Default)]
struct PendingDestructiveAction {
    request: Option<DestructiveRequest>,
}

/// Asset Browser filter, imported sub-assets, and its last status message.
#[derive(Resource, Clone, Default)]
struct EditorAssetState {
    /// Text used to filter project asset paths.
    filter: String,
    /// glTF primitives imported during this editor session.
    gltf_primitives: Vec<ImportedGltfPrimitive>,
    /// Result of the latest import or assignment.
    message: Option<String>,
    /// Cached `project_asset_files` result, so the disk is not walked every
    /// frame.
    files: Vec<PathBuf>,
    /// Project root and time of the last scan; `None` forces a rescan.
    files_scanned: Option<(String, std::time::Instant)>,
}

impl EditorAssetState {
    /// Rescans the project's `assets` folder when the project changed or the
    /// cached list is older than one second.
    fn refresh_files(&mut self, project_root: &str) {
        let fresh = self.files_scanned.as_ref().is_some_and(|(root, time)| {
            root == project_root
                && time.elapsed() < std::time::Duration::from_secs(1)
        });
        if !fresh {
            self.files = project_asset_files(project_root);
            self.files_scanned =
                Some((project_root.to_owned(), std::time::Instant::now()));
        }
    }
}

/// Cargo work that can run without freezing the editor window.
#[derive(Clone)]
enum BuildRequest {
    Check,
    BuildAndRun {
        /// Profile selected beside the editor's Play button.
        profile: GameBuildProfile,
    },
    Export {
        parent: PathBuf,
        project_name: String,
        binary_name: String,
        cooked_scene: PathBuf,
    },
}

/// Result sent from the Cargo worker back to the main editor thread.
struct BuildFinished {
    success: bool,
    output: String,
}

/// One update sent by the compiler or running game to the editor.
enum BuildWorkerMessage {
    /// Text that should appear immediately in Cargo Output.
    Output(String),
    /// Final status after Cargo or the native game exits.
    Finished(BuildFinished),
}

/// Current compiler state displayed below the Rust Code Editor.
#[derive(Resource, Default)]
struct EditorBuildState {
    /// True while Cargo is checking or compiling the project.
    running: bool,
    /// Combined Cargo output kept for the Code and Console areas.
    output: String,
    /// Byte position already copied into the Console panel.
    console_cursor: usize,
    /// Receiver is locked only because ECS resources must be thread-safe.
    receiver:
        std::sync::Mutex<Option<std::sync::mpsc::Receiver<BuildWorkerMessage>>>,
    /// Set by Stop; the worker kills its current process when it sees it.
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl EditorBuildState {
    /// Starts one Cargo task. A second click is ignored until it finishes.
    fn start(
        &mut self,
        project_root: PathBuf,
        request: BuildRequest,
    ) -> Result<(), String> {
        if self.running {
            return Err("A Cargo task is already running".into());
        }
        let manifest = project_root.join("Cargo.toml");
        if !manifest.is_file() {
            return Err(format!("{} is missing", manifest.display()));
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        *self.receiver.get_mut().map_err(|error| error.to_string())? =
            Some(receiver);
        self.running = true;
        self.output = match &request {
            BuildRequest::Check => "Running cargo check...".into(),
            BuildRequest::BuildAndRun { profile } => {
                format!("Building {} game...", profile.label())
            }
            BuildRequest::Export { .. } => "Building game for export...".into(),
        };
        self.console_cursor = 0;
        self.stop = std::sync::Arc::default();
        let stop = std::sync::Arc::clone(&self.stop);
        std::thread::spawn(move || {
            let command = match &request {
                BuildRequest::Check => "check",
                BuildRequest::BuildAndRun { .. }
                | BuildRequest::Export { .. } => "build",
            };
            let profile = match &request {
                BuildRequest::BuildAndRun { profile } => Some(*profile),
                BuildRequest::Export { .. } => Some(GameBuildProfile::Release),
                BuildRequest::Check => None,
            };
            let mut cargo = std::process::Command::new("cargo");
            cargo.arg(command);
            if let Some(flag) = profile.and_then(GameBuildProfile::cargo_flag) {
                cargo.arg(flag);
            }
            cargo
                .args(["--message-format", "short", "--manifest-path"])
                .arg(&manifest)
                .current_dir(&project_root);
            let finished = match run_streamed(&mut cargo, &sender, &stop) {
                Ok(None) => BuildFinished {
                    success: false,
                    output: "\nCargo task stopped.\n".into(),
                },
                Ok(Some(status)) => {
                    let mut success = status.success();
                    if success {
                        if let BuildRequest::BuildAndRun { profile } = &request
                        {
                            return run_native_game(
                                &project_root,
                                &manifest,
                                *profile,
                                sender,
                                &stop,
                            );
                        } else if let BuildRequest::Export {
                            parent,
                            project_name,
                            binary_name,
                            cooked_scene,
                        } = &request
                        {
                            match export_built_game(
                                &project_root,
                                &manifest,
                                parent,
                                project_name,
                                binary_name,
                                cooked_scene,
                            ) {
                                Ok(path) => {
                                    let _ = sender.send(
                                        BuildWorkerMessage::Output(format!(
                                            "\nExported game to {}\n",
                                            path.display()
                                        )),
                                    );
                                }
                                Err(error) => {
                                    success = false;
                                    let _ = sender.send(
                                        BuildWorkerMessage::Output(format!(
                                            "\nBuild passed, but export failed: {error}\n"
                                        )),
                                    );
                                }
                            }
                        }
                    }
                    BuildFinished {
                        success,
                        output: if success {
                            "\nCargo task finished successfully.\n".into()
                        } else {
                            "\nCargo task failed.\n".into()
                        },
                    }
                }
                Err(error) => BuildFinished {
                    success: false,
                    output: format!("Could not start Cargo: {error}"),
                },
            };
            let _ = sender.send(BuildWorkerMessage::Finished(finished));
        });
        Ok(())
    }

    /// Asks the worker to kill the running Cargo task or native game.
    fn request_stop(&self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// Receives new compiler and game output without blocking the editor.
    fn poll(&mut self) -> Option<bool> {
        let mut messages = Vec::new();
        let mut disconnected = false;
        {
            let receiver = self.receiver.get_mut().ok()?.as_ref()?;
            loop {
                match receiver.try_recv() {
                    Ok(message) => messages.push(message),
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }

        let mut finished_status = None;
        for message in messages {
            match message {
                BuildWorkerMessage::Output(text) => {
                    self.append_output(&text);
                }
                BuildWorkerMessage::Finished(finished) => {
                    self.append_output(&finished.output);
                    self.running = false;
                    finished_status = Some(finished.success);
                }
            }
        }
        if finished_status.is_some() {
            *self.receiver.get_mut().ok()? = None;
            finished_status
        } else if disconnected {
            if self.running {
                self.running = false;
                self.append_output("\nCargo worker stopped unexpectedly.\n");
                *self.receiver.get_mut().ok()? = None;
                Some(false)
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Keeps recent output while preventing an unlimited memory allocation.
    fn append_output(&mut self, text: &str) {
        const MAX_OUTPUT_BYTES: usize = 500_000;
        self.output.push_str(text);
        if self.output.len() > MAX_OUTPUT_BYTES {
            let mut remove = self.output.len() - MAX_OUTPUT_BYTES;
            while !self.output.is_char_boundary(remove) {
                remove += 1;
            }
            const MARKER: &str = "[older output truncated]\n";
            self.output.drain(..remove);
            self.output.insert_str(0, MARKER);
            // Keep the Console cursor on the same text; unsent text that was
            // dropped restarts the Console at the marker.
            self.console_cursor = match self.console_cursor.checked_sub(remove)
            {
                Some(cursor) => cursor + MARKER.len(),
                None => 0,
            };
        }
    }

    /// Returns build text that has not yet been sent to the Console panel.
    fn take_console_output(&mut self) -> String {
        let text = self.output[self.console_cursor..].to_owned();
        self.console_cursor = self.output.len();
        text
    }
}

/// Starts the native game and forwards both output streams to the editor.
fn run_native_game(
    project_root: &std::path::Path,
    manifest: &std::path::Path,
    profile: GameBuildProfile,
    sender: std::sync::mpsc::Sender<BuildWorkerMessage>,
    stop: &std::sync::atomic::AtomicBool,
) {
    let _ = sender.send(BuildWorkerMessage::Output(
        "\nBuild passed. Starting native game...\n".into(),
    ));
    // On Unix `cargo run` replaces itself with the game, so Stop kills the
    // game; on Windows Cargo's job object takes the game down with it.
    let mut cargo = std::process::Command::new("cargo");
    cargo.arg("run");
    if let Some(flag) = profile.cargo_flag() {
        cargo.arg(flag);
    }
    cargo
        .arg("--manifest-path")
        .arg(manifest)
        .current_dir(project_root);
    let finished = match run_streamed(&mut cargo, &sender, stop) {
        Ok(Some(status)) if status.success() => BuildFinished {
            success: true,
            output: "\nNative game exited successfully.\n".into(),
        },
        Ok(Some(status)) => BuildFinished {
            success: false,
            output: format!("\nNative game exited with status {status}.\n"),
        },
        Ok(None) => BuildFinished {
            success: false,
            output: "\nNative game stopped.\n".into(),
        },
        Err(error) => BuildFinished {
            success: false,
            output: format!("\nCould not run native game: {error}\n"),
        },
    };
    let _ = sender.send(BuildWorkerMessage::Finished(finished));
}

/// Runs one command and sends each stdout and stderr line to the editor as
/// it arrives. Returns `None` when `stop` was set and the process was killed.
// ponytail: killing `cargo build` can leave its rustc children running until
// they finish; add a process group kill if that becomes a problem.
fn run_streamed(
    command: &mut std::process::Command,
    sender: &std::sync::mpsc::Sender<BuildWorkerMessage>,
    stop: &std::sync::atomic::AtomicBool,
) -> std::io::Result<Option<std::process::ExitStatus>> {
    use std::io::BufRead;
    use std::process::Stdio;

    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let pipes: [Option<Box<dyn std::io::Read + Send>>; 2] = [
        child.stdout.take().map(|pipe| Box::new(pipe) as _),
        child.stderr.take().map(|pipe| Box::new(pipe) as _),
    ];
    let readers: Vec<_> = pipes
        .into_iter()
        .flatten()
        .map(|pipe| {
            let sender = sender.clone();
            std::thread::spawn(move || {
                for line in std::io::BufReader::new(pipe).lines() {
                    let text = match line {
                        Ok(line) => format!("{line}\n"),
                        Err(error) => {
                            format!("Could not read output: {error}\n")
                        }
                    };
                    let _ = sender.send(BuildWorkerMessage::Output(text));
                }
            })
        })
        .collect();

    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if stop.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    for reader in readers {
        let _ = reader.join();
    }
    Ok(status)
}

/// Creates a portable folder after Cargo has produced the release binary.
fn export_built_game(
    project_root: &std::path::Path,
    manifest: &std::path::Path,
    parent: &std::path::Path,
    project_name: &str,
    binary_name: &str,
    cooked_scene: &std::path::Path,
) -> Result<PathBuf, String> {
    if !parent.is_dir() {
        return Err(format!("{} is not a folder", parent.display()));
    }
    let metadata = std::process::Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .args(["--manifest-path"])
        .arg(manifest)
        .current_dir(project_root)
        .output()
        .map_err(|error| format!("Could not read Cargo metadata: {error}"))?;
    if !metadata.status.success() {
        return Err(String::from_utf8_lossy(&metadata.stderr).into_owned());
    }
    let metadata: serde_json::Value = serde_json::from_slice(&metadata.stdout)
        .map_err(|error| format!("Invalid Cargo metadata: {error}"))?;
    let target = metadata
        .get("target_directory")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "Cargo metadata has no target directory".to_owned())?;
    let executable_name = if cfg!(target_os = "windows") {
        format!("{binary_name}.exe")
    } else {
        binary_name.to_owned()
    };
    let executable =
        PathBuf::from(target).join("release").join(&executable_name);
    if !executable.is_file() {
        return Err(format!(
            "Release executable {} was not produced",
            executable.display()
        ));
    }
    package_game_files(
        project_root,
        &executable,
        parent,
        project_name,
        binary_name,
        cooked_scene,
    )
}

/// Copies a verified executable and runtime data into one new export folder.
fn package_game_files(
    project_root: &std::path::Path,
    executable: &std::path::Path,
    parent: &std::path::Path,
    project_name: &str,
    binary_name: &str,
    cooked_scene: &std::path::Path,
) -> Result<PathBuf, String> {
    let executable_name = if cfg!(target_os = "windows") {
        format!("{binary_name}.exe")
    } else {
        binary_name.to_owned()
    };
    let cooked_source = project_root.join(cooked_scene);
    if !cooked_source.is_file() {
        return Err(format!(
            "Cooked scene {} is missing",
            cooked_source.display()
        ));
    }

    // A unique final name avoids replacing an older playable export.
    let safe_name = binary_name.replace('-', "_");
    let mut destination = parent.join(format!("{safe_name}_export"));
    for number in 2.. {
        if !destination.exists() {
            break;
        }
        destination = parent.join(format!("{safe_name}_export_{number}"));
    }
    let temporary =
        parent.join(format!(".rusting-export-{}", uuid::Uuid::new_v4()));
    let result = (|| -> Result<(), String> {
        std::fs::create_dir_all(&temporary)
            .map_err(|error| error.to_string())?;
        std::fs::copy(executable, temporary.join(&executable_name))
            .map_err(|error| error.to_string())?;
        let cooked_destination = temporary.join(cooked_scene);
        if let Some(folder) = cooked_destination.parent() {
            std::fs::create_dir_all(folder)
                .map_err(|error| error.to_string())?;
        }
        std::fs::copy(&cooked_source, cooked_destination)
            .map_err(|error| error.to_string())?;
        let assets = project_root.join("assets");
        if assets.is_dir() {
            copy_directory(&assets, &temporary.join("assets"))?;
        }
        let engine_license =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("LICENSE.md");
        if engine_license.is_file() {
            std::fs::copy(
                engine_license,
                temporary.join("RUSTING_ENGINE_LICENSE.md"),
            )
            .map_err(|error| error.to_string())?;
        }
        std::fs::write(
            temporary.join("README.txt"),
            format!(
                "{project_name}\n\nRun {executable_name} to start the game.\nThe system needs a Vulkan-capable graphics driver.\n"
            ),
        )
        .map_err(|error| error.to_string())?;
        std::fs::rename(&temporary, &destination)
            .map_err(|error| error.to_string())?;
        Ok(())
    })();
    if result.is_err() && temporary.exists() {
        let _ = std::fs::remove_dir_all(&temporary);
    }
    result.map(|()| destination)
}

/// Recursively copies normal files and folders while ignoring symbolic links.
fn copy_directory(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> Result<(), String> {
    std::fs::create_dir_all(destination).map_err(|error| error.to_string())?;
    for entry in std::fs::read_dir(source).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        if file_type.is_symlink() {
            continue;
        }
        let target = destination.join(entry.file_name());
        if file_type.is_dir() {
            copy_directory(&entry.path(), &target)?;
        } else if file_type.is_file() {
            std::fs::copy(entry.path(), target)
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

/// Work selected inside the Assets area and applied after drawing.
enum AssetRequest {
    ImportFiles(Vec<PathBuf>),
    LoadTexture(PathBuf),
    ImportGltf(PathBuf),
    AssignTexture(Handle<TextureAsset>),
    AssignPrimitive(Handle<MeshAsset>, Handle<MaterialAsset>),
    /// Gives the selected renderer a fresh default material.
    NewMaterial,
    /// Picks an image file for one `inspector::TEXTURE_SLOTS` slot.
    LoadMaterialTexture(usize),
    /// Puts the image a `LoadMaterialTexture` dialog chose into the slot.
    SetMaterialTexture {
        entity: Entity,
        slot: usize,
        path: PathBuf,
    },
}

/// Scene-object operation requested by the Hierarchy area.
enum EntityRequest {
    /// Adds an object that only has a name and transform.
    CreateEmpty(Option<Entity>),
    /// Adds one of the engine's built-in procedural meshes.
    CreatePrimitive(PrimitiveShape, Option<Entity>),
    /// Adds an inactive perspective camera.
    CreateCamera(Option<Entity>),
    /// Adds an authored light used by editor and game rendering.
    CreateLight(EditorLightType, Option<Entity>),
    /// Copies every serialized component of each object.
    Duplicate(Vec<Entity>),
    /// Removes objects and all of their children.
    Delete(Vec<Entity>),
    /// Shows or hides one object (the Hierarchy eye toggle).
    SetVisible(Entity, bool),
    /// Changes the display name stored in the scene.
    Rename(Entity, String),
    /// Moves one object below another object, or back to the scene root.
    Reparent(Entity, Option<Entity>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EditorLightType {
    Directional,
    Point,
    Spot,
}

impl EditorLightType {
    const ALL: [Self; 3] = [Self::Directional, Self::Point, Self::Spot];

    const fn label(self) -> &'static str {
        match self {
            Self::Directional => "Directional Light",
            Self::Point => "Point Light",
            Self::Spot => "Spot Light",
        }
    }
}

/// Stores the current scene before an editor command changes it.
fn remember_scene_before_edit(
    world: &mut World,
    history: &mut EditorHistory,
) -> Result<(), String> {
    if let Some(document) = history.pending_inspector.take() {
        history.push_undo(document);
    }
    scene_document(world, "Undo Snapshot")
        .map(|document| history.push_undo(document))
        .map_err(|error| format!("Could not create undo snapshot: {error}"))
}

/// Gives a new object a readable name that is not already in the Hierarchy.
fn unique_object_name(world: &mut World, base: &str) -> String {
    let mut query = world.query::<&Name>();
    let names = query
        .iter(world)
        .map(|name| name.0.clone())
        .collect::<std::collections::HashSet<_>>();
    if !names.contains(base) {
        return base.into();
    }
    for number in 2.. {
        let candidate = format!("{base} {number}");
        if !names.contains(&candidate) {
            return candidate;
        }
    }
    unreachable!("a free object name always exists")
}

/// Returns normal files below a project's `assets` folder without following
/// directory symbolic links.
fn project_asset_files(project_root: &str) -> Vec<PathBuf> {
    let root = PathBuf::from(project_root).join("assets");
    let mut pending = vec![root];
    let mut files = Vec::new();
    while let Some(folder) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                pending.push(entry.path());
            } else if file_type.is_file() {
                files.push(entry.path());
            }
            if files.len() >= 10_000 {
                break;
            }
        }
    }
    files.sort();
    files
}

/// Copies an external file into the project without replacing an existing
/// asset. Name collisions receive `_2`, `_3`, and so on.
fn copy_into_project_assets(
    project_root: &str,
    source: &std::path::Path,
) -> Result<PathBuf, String> {
    if !source.is_file() {
        return Err(format!("{} is not a file", source.display()));
    }
    let asset_folder = PathBuf::from(project_root).join("assets");
    std::fs::create_dir_all(&asset_folder)
        .map_err(|error| format!("Could not create assets folder: {error}"))?;
    let source = source
        .canonicalize()
        .map_err(|error| format!("Could not open asset: {error}"))?;
    let asset_folder = asset_folder
        .canonicalize()
        .map_err(|error| format!("Could not open assets folder: {error}"))?;
    if source.starts_with(&asset_folder) {
        return Ok(source);
    }
    let file_name = source
        .file_name()
        .ok_or_else(|| "Asset path has no file name".to_owned())?;
    let mut destination = asset_folder.join(file_name);
    let stem = source
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("asset");
    let extension = source.extension().and_then(|value| value.to_str());
    for number in 2.. {
        if !destination.exists() {
            break;
        }
        let name = extension.map_or_else(
            || format!("{stem}_{number}"),
            |extension| format!("{stem}_{number}.{extension}"),
        );
        destination = asset_folder.join(name);
    }
    std::fs::copy(&source, &destination).map_err(|error| {
        format!(
            "Could not copy {} to {}: {error}",
            source.display(),
            destination.display()
        )
    })?;
    Ok(destination)
}

/// Draws the startup Project Manager and returns one requested action.
fn draw_project_manager(
    context: &Context,
    manager: &mut ProjectManagerState,
    dialogs: &mut FileDialogs,
) -> Option<ProjectRequest> {
    if !manager.open {
        return None;
    }

    let mut request = None;
    egui::Window::new("RustingEngine Project Manager")
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .collapsible(false)
        .resizable(true)
        .default_width(620.0)
        .show(context, |ui| {
            ui.heading("Open a project");
            if manager.recent_projects.is_empty() {
                ui.label("No recent projects yet.");
            } else {
                egui::ScrollArea::vertical()
                    .max_height(160.0)
                    .show(ui, |ui| {
                        for project in &manager.recent_projects {
                            let text = format!(
                                "{}\n{}",
                                project.name,
                                project.path.display()
                            );
                            if ui.button(text).clicked() {
                                request = Some(ProjectRequest::Open(
                                    project.path.clone(),
                                ));
                            }
                        }
                    });
            }
            if ui.button("Browse for existing project...").clicked() {
                dialogs.pick_folder(
                    DialogPurpose::OpenProject,
                    rfd::AsyncFileDialog::new()
                        .set_title("Open RustingEngine Project"),
                );
            }

            ui.separator();
            ui.heading("Create a project");
            ui.horizontal(|ui| {
                ui.label("Name");
                ui.text_edit_singleline(&mut manager.project_name);
            });
            ui.horizontal(|ui| {
                ui.label("Parent folder");
                if manager.parent_directory.as_os_str().is_empty() {
                    ui.colored_label(
                        gui_elements::EditorTheme::TEXT_MUTED,
                        "Not selected",
                    );
                } else {
                    ui.monospace(
                        manager.parent_directory.display().to_string(),
                    );
                }
                if ui.button("Browse...").clicked() {
                    let mut dialog = rfd::AsyncFileDialog::new()
                        .set_title("Choose Project Parent Folder");
                    if !manager.parent_directory.as_os_str().is_empty() {
                        dialog =
                            dialog.set_directory(&manager.parent_directory);
                    }
                    dialogs.pick_folder(
                        DialogPurpose::ProjectParentFolder,
                        dialog,
                    );
                }
            });
            if ui.button("Create Project").clicked() {
                if manager.parent_directory.as_os_str().is_empty() {
                    dialogs.pick_folder(
                        DialogPurpose::CreateProjectIn,
                        rfd::AsyncFileDialog::new()
                            .set_title("Choose Where to Create the Project"),
                    );
                } else {
                    request = Some(ProjectRequest::Create {
                        parent: manager.parent_directory.clone(),
                        name: manager.project_name.clone(),
                    });
                }
            }
            if let Some(message) = &manager.message {
                ui.separator();
                ui.label(message);
            }
        });
    request
}

/// Draws one fixed-size button in an editor area header.
///
/// The rectangle and icon are painted separately. Different icon sizes can
/// therefore never change the size or vertical position of the button.
fn dock_header_button(
    ui: &mut egui::Ui,
    icon: EditorIcon,
    hover_text: &str,
) -> egui::Response {
    gui_elements::icon_button_sized(
        ui,
        icon,
        false,
        true,
        egui::vec2(22.0, 20.0),
    )
    .on_hover_text(hover_text)
}

#[derive(Clone, Copy)]
enum DockAction {
    /// Area ID and direction for a new divider.
    Split(u64, EditorSplitAxis),
    /// Area ID that should be removed.
    Close(u64),
}

/// Draws one layout node and all children below it.
///
/// # Arguments
/// * `ui` - Egui area used to paint controls.
/// * `node` - Area or split currently being drawn.
/// * `rect` - Screen space available to this node.
/// * `active_area` - ID of the area with the blue border.
/// * `actions` - Changes that will be applied after drawing is finished.
/// * `show_panel` - Draws the selected content inside a leaf area.
fn show_dock_node(
    ui: &mut egui::Ui,
    node: &mut EditorDockNode,
    rect: egui::Rect,
    active_area: &mut u64,
    actions: &mut Vec<DockAction>,
    show_panel: &mut impl FnMut(&mut egui::Ui, EditorPanel),
) {
    const DIVIDER: f32 = 4.0;
    match node {
        EditorDockNode::Area { id, panel } => {
            let id = *id;
            // Leave breathing room between the panel and its resize divider.
            let panel_rect = rect.shrink(1.0);
            // Every area gets its own ID space. Two areas can show the same
            // panel without Egui thinking that their controls are duplicates.
            // A leaf draws its header first and panel content below it.
            ui.scope_builder(
                egui::UiBuilder::new()
                    .id_salt(("dock_area", id))
                    .max_rect(panel_rect)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
                |ui| {
                    // A panel must never draw or receive clicks over its neighbor.
                    ui.set_clip_rect(ui.clip_rect().intersect(panel_rect));
                    // Clicking any empty space or control selects this area.
                    // `rect_contains_pointer` respects layers, so a popup from
                    // another area floating over this one does not select it.
                    let pressed_inside = ui
                        .input(|input| input.pointer.primary_pressed())
                        && ui.rect_contains_pointer(panel_rect);
                    if pressed_inside {
                        *active_area = id;
                    }
                    let active = *active_area == id;
                    if !matches!(panel, EditorPanel::Scene | EditorPanel::Game)
                    {
                        // Normal panels need a solid background. A 3D view stays
                        // transparent because Vulkan draws below egui.
                        ui.painter().rect_filled(
                            panel_rect,
                            4.0,
                            gui_elements::EditorTheme::PANEL,
                        );
                    }
                    // Blender marks the active area subtly, not with a thick frame.
                    let stroke = if active {
                        egui::Stroke::new(
                            1.0_f32,
                            gui_elements::EditorTheme::ACCENT,
                        )
                    } else {
                        egui::Stroke::new(
                            1.0_f32,
                            gui_elements::EditorTheme::BORDER,
                        )
                    };
                    egui::Frame::NONE
                        .fill(gui_elements::EditorTheme::PANEL_RAISED)
                        .corner_radius(egui::CornerRadius {
                            nw: 4,
                            ne: 4,
                            sw: 0,
                            se: 0,
                        })
                        .inner_margin(egui::Margin::symmetric(4, 2))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                // Blender shows the editor type as an icon
                                // before its switcher.
                                let (icon_rect, _) = ui.allocate_exact_size(
                                    egui::vec2(18.0, 18.0),
                                    egui::Sense::hover(),
                                );
                                icons::paint_editor_icon(
                                    ui.painter(),
                                    panel.icon(),
                                    icon_rect,
                                    if active {
                                        gui_elements::EditorTheme::ACCENT_HOVER
                                    } else {
                                        gui_elements::EditorTheme::TEXT_MUTED
                                    },
                                );
                                let buttons_width = 22.0 * 3.0
                                    + ui.spacing().item_spacing.x * 2.0;
                                let selector_width = (ui.available_width()
                                    - buttons_width
                                    - 12.0)
                                    .clamp(60.0, 140.0);
                                gui_elements::EditorTheme::toolbar_combo_box_with_popup(
                                    ui,
                                    ("dock_panel_selector", id),
                                    panel.title(),
                                    selector_width,
                                    220.0,
                                    |ui| {
                                            for choice in EditorPanel::ALL {
                                                if gui_elements::EditorTheme::menu_choice(
                                                    ui,
                                                    choice.title(),
                                                    *panel == choice,
                                                    true,
                                                )
                                                .clicked()
                                                {
                                                    *panel = choice;
                                                }
                                            }
                                        },
                                );
                                // Keep all three buttons together on the right side.
                                ui.add_space(
                                    (ui.available_width() - buttons_width)
                                        .max(0.0),
                                );
                                if dock_header_button(
                                    ui,
                                    EditorIcon::SplitColumns,
                                    "Split into left/right areas",
                                )
                                .clicked()
                                {
                                    actions.push(DockAction::Split(
                                        id,
                                        EditorSplitAxis::Columns,
                                    ));
                                }
                                if dock_header_button(
                                    ui,
                                    EditorIcon::SplitRows,
                                    "Split into top/bottom areas",
                                )
                                .clicked()
                                {
                                    actions.push(DockAction::Split(
                                        id,
                                        EditorSplitAxis::Rows,
                                    ));
                                }
                                if dock_header_button(
                                    ui,
                                    EditorIcon::Close,
                                    "Close this area",
                                )
                                .clicked()
                                {
                                    actions.push(DockAction::Close(id));
                                }
                            });
                        });
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::same(6))
                        .show(ui, |ui| show_panel(ui, *panel));
                    // Paint the selection border last so panel contents cannot hide it.
                    ui.painter().rect_stroke(
                        panel_rect,
                        4.0,
                        stroke,
                        egui::StrokeKind::Inside,
                    );
                },
            );
        }
        EditorDockNode::Split {
            axis,
            ratio,
            first,
            second,
        } => {
            // Keep both children large enough to stay usable.
            let ratio_clamped = ratio.clamp(0.1, 0.9);
            *ratio = ratio_clamped;
            // Turn the ratio into two child rectangles and one thin divider.
            let (first_rect, divider_rect, second_rect) = match axis {
                EditorSplitAxis::Columns => {
                    let split =
                        rect.left() + (rect.width() - DIVIDER) * ratio_clamped;
                    (
                        egui::Rect::from_min_max(
                            rect.min,
                            egui::pos2(split, rect.bottom()),
                        ),
                        egui::Rect::from_min_max(
                            egui::pos2(split, rect.top()),
                            egui::pos2(split + DIVIDER, rect.bottom()),
                        ),
                        egui::Rect::from_min_max(
                            egui::pos2(split + DIVIDER, rect.top()),
                            rect.max,
                        ),
                    )
                }
                EditorSplitAxis::Rows => {
                    let split =
                        rect.top() + (rect.height() - DIVIDER) * ratio_clamped;
                    (
                        egui::Rect::from_min_max(
                            rect.min,
                            egui::pos2(rect.right(), split),
                        ),
                        egui::Rect::from_min_max(
                            egui::pos2(rect.left(), split),
                            egui::pos2(rect.right(), split + DIVIDER),
                        ),
                        egui::Rect::from_min_max(
                            egui::pos2(rect.left(), split + DIVIDER),
                            rect.max,
                        ),
                    )
                }
            };
            let divider_id = ui.id().with((
                "dock_divider",
                first_rect.min.x.to_bits(),
                first_rect.min.y.to_bits(),
                second_rect.max.x.to_bits(),
                second_rect.max.y.to_bits(),
            ));
            let response = ui.interact(
                divider_rect,
                divider_id,
                egui::Sense::click_and_drag(),
            );
            if response.dragged() {
                // Mouse position becomes the new size ratio while dragging.
                if let Some(pointer) = response.interact_pointer_pos() {
                    *ratio = match axis {
                        EditorSplitAxis::Columns => {
                            (pointer.x - rect.left()) / rect.width()
                        }
                        EditorSplitAxis::Rows => {
                            (pointer.y - rect.top()) / rect.height()
                        }
                    }
                    .clamp(0.1, 0.9);
                }
            }
            if response.hovered() || response.dragged() {
                ui.ctx().set_cursor_icon(match axis {
                    EditorSplitAxis::Columns => {
                        egui::CursorIcon::ResizeHorizontal
                    }
                    EditorSplitAxis::Rows => egui::CursorIcon::ResizeVertical,
                });
            }
            ui.painter().rect_filled(
                divider_rect,
                0.0,
                if response.hovered() || response.dragged() {
                    gui_elements::EditorTheme::ACCENT_HOVER
                } else {
                    gui_elements::EditorTheme::BORDER_SOFT
                },
            );
            // Splits can contain more splits, so draw both children the same way.
            show_dock_node(
                ui,
                first,
                first_rect,
                active_area,
                actions,
                show_panel,
            );
            show_dock_node(
                ui,
                second,
                second_rect,
                active_area,
                actions,
                show_panel,
            );
        }
    }
}

/// Applies split and close requests after the layout is no longer borrowed.
///
/// Egui creates actions while drawing. Waiting until drawing is finished keeps
/// Rust from borrowing and changing the same tree at the same time.
fn apply_dock_actions(state: &mut EditorState, actions: Vec<DockAction>) {
    for action in actions {
        match action {
            DockAction::Split(id, axis) => {
                let new_id = state.next_area_id;
                state.next_area_id = state.next_area_id.saturating_add(1);
                if state.dock_layout.split(id, axis, new_id) {
                    state.active_area = new_id;
                }
            }
            DockAction::Close(id) => {
                if state.dock_layout.close(id) && state.active_area == id {
                    state.active_area = state.dock_layout.first_area_id();
                }
            }
        }
    }
}

/// Checks that an editable source file stays inside the selected game project.
///
/// * `project_root` - Folder containing the game's `project.json`.
/// * `path` - Rust or shader file requested by Code Editor.
fn project_source_path(
    project_root: &str,
    path: &str,
) -> Result<PathBuf, String> {
    let path = project_content_path(project_root, path)?;
    let supported = path.extension().and_then(|extension| extension.to_str());
    if !matches!(supported, Some("rs" | "glsl" | "comp" | "vert" | "frag")) {
        return Err(
            "Supported source types: .rs, .glsl, .comp, .vert, .frag".into()
        );
    }
    Ok(path)
}

/// Resolves one project-relative file without allowing it to leave the root.
fn project_content_path(
    project_root: &str,
    path: &str,
) -> Result<PathBuf, String> {
    use std::path::Component;

    let root = std::path::Path::new(project_root);
    if root.as_os_str().is_empty() || !root.join("project.json").is_file() {
        return Err(
            "Selected project must contain a project.json manifest".into()
        );
    }
    let requested = std::path::Path::new(path);
    if requested
        .components()
        .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(
            "Project paths must stay inside the selected game project".into()
        );
    }
    let absolute_root = root
        .canonicalize()
        .map_err(|error| format!("Could not open project folder: {error}"))?;
    let path = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        absolute_root.join(requested)
    };
    // Walk up to the closest existing folder. This allows a new `scripts`
    // folder while still catching an existing symbolic link that leaves the
    // project.
    let mut existing_ancestor = path.as_path();
    while !existing_ancestor.exists() {
        existing_ancestor = existing_ancestor.parent().ok_or_else(|| {
            "Project path must stay inside the project".to_owned()
        })?;
    }
    let checked_ancestor = existing_ancestor
        .canonicalize()
        .map_err(|error| format!("Could not check source path: {error}"))?;
    if !checked_ancestor.starts_with(&absolute_root) {
        return Err(
            "Project paths must stay inside the selected game project".into()
        );
    }
    Ok(path)
}

/// Converts a checked project file back into the short path stored by the GUI.
fn project_relative_path(
    project_root: &str,
    path: &std::path::Path,
) -> Result<String, String> {
    let absolute = project_content_path(project_root, &path.to_string_lossy())?;
    let root = std::path::Path::new(project_root)
        .canonicalize()
        .map_err(|error| format!("Could not open project folder: {error}"))?;
    absolute
        .strip_prefix(root)
        .map_err(|_| "File must stay inside the selected project".to_owned())
        .map(editor_relative_path)
}

/// Uses one portable separator style for paths shown and stored by the editor.
///
/// `PathBuf` keeps native separators for real filesystem work. Text fields,
/// scene files, project files, and tests use `/` so a project has the same
/// document paths on Windows and Linux.
fn editor_relative_path(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Performs quick checks before source text is saved.
///
/// Rust still receives its complete check from Cargo during Build & Run.
fn validate_project_source(
    project_root: &str,
    path: &str,
    source: &str,
) -> Result<(), String> {
    let path = project_source_path(project_root, path)?;
    if source.trim().is_empty() {
        return Err("Source file is empty".into());
    }
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("comp" | "vert" | "frag") => {
            if !source.contains("#version") {
                return Err(
                    "GLSL shader is missing a #version directive".into()
                );
            }
            if !source.contains("void main") {
                return Err("GLSL shader is missing void main()".into());
            }
            if path
                .extension()
                .is_some_and(|extension| extension == "comp")
                && !source.contains("local_size_")
            {
                return Err(
                    "Compute shader is missing a local workgroup size".into()
                );
            }
        }
        Some("rs") if !source.contains('{') => {
            return Err("Rust source does not contain a code block".into());
        }
        _ => {}
    }
    Ok(())
}

/// Validates and saves text from Code Editor inside the game project.
fn save_project_source(
    project_root: &str,
    path: &str,
    source: &str,
) -> Result<(), String> {
    validate_project_source(project_root, path, source)?;
    let path = project_source_path(project_root, path)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            format!("Could not create {}: {error}", parent.display())
        })?;
    }
    crate::runtime::write_atomic(&path, source.as_bytes())
        .map_err(|error| format!("Could not save {}: {error}", path.display()))
}

#[cfg(test)]
mod tests;
