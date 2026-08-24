//! Feature-gated egui editor state and ECS panels.
//!
//! The renderer consumes [`EditorUi::take_output`] and is responsible for
//! uploading egui textures and meshes in its compositing pass.

use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::{Resource, World};
use egui::{
    CentralPanel, ComboBox, Context, DragValue, RawInput, SidePanel,
    TopBottomPanel,
};

use crate::runtime::{
    add_registered_component, load_scene, registered_component_names,
    registered_component_values, remove_registered_component, save_scene,
    scene_document, set_registered_component, App, AppError, Camera, Collider,
    ColliderShape, CollisionLayers, FrameTime, MeshRenderer, Name, Parent,
    PhysicsBackendStatus, PhysicsBody, PhysicsSolver, Plugin, Projection,
    RenderCameraOverride, RenderSettings, RenderWorld, RigidBody,
    RigidBodyKind, SceneDocument, SceneLoadMode, ScheduleStage,
    ScriptComponent, ScriptRuntime, ScriptSettings, SimulationClass,
    TimeControl,
};
use crate::AssetServer;
use crate::Transform;

/// Applies the editor's compact dark workspace theme to an egui context.
pub fn configure_editor_style(context: &Context) {
    let mut style = (*context.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = egui::Color32::from_rgb(24, 26, 31);
    style.visuals.window_fill = egui::Color32::from_rgb(28, 30, 36);
    style.visuals.selection.bg_fill = egui::Color32::from_rgb(45, 104, 210);
    context.set_style(style);
}

/// Editor interaction mode. Edit state never advances gameplay fixed updates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditorMode {
    #[default]
    Edit,
    Play,
    Paused,
}

/// Main editor workspace. Scene and Game share the same renderer but select
/// different cameras; Code is a real project-file editor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditorWorkspace {
    #[default]
    Scene,
    Game,
    Code,
}

/// Persistent selection and panel visibility.
#[derive(Resource, Clone, Debug)]
pub struct EditorState {
    pub selected: Option<Entity>,
    pub mode: EditorMode,
    pub hierarchy_open: bool,
    pub inspector_open: bool,
    pub console_open: bool,
    pub assets_open: bool,
    pub scene_path: String,
    pub project_root: String,
    pub scene_message: Option<String>,
    pub component_drafts: std::collections::HashMap<(Entity, String), String>,
    pub workspace: EditorWorkspace,
    pub editor_camera: Option<Entity>,
    pub play_snapshot: Option<SceneDocument>,
    pub code_path: String,
    pub code_source: String,
    pub code_message: Option<String>,
    pub code_dirty: bool,
}

/// Physical pixel rectangle occupied by the editor's live scene view.
///
/// This is intentionally editor-owned layout data. The window runner converts
/// it into a renderer viewport, keeping egui out of the rendering API.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EditorViewport {
    pub offset: [u32; 2],
    pub extent: [u32; 2],
    pub valid: bool,
}

impl Default for EditorState {
    fn default() -> Self {
        Self {
            selected: None,
            mode: EditorMode::Edit,
            hierarchy_open: true,
            inspector_open: true,
            console_open: true,
            assets_open: true,
            scene_path: "testGame/scenes/main.rscene".into(),
            project_root: "testGame".into(),
            scene_message: None,
            component_drafts: std::collections::HashMap::new(),
            workspace: EditorWorkspace::Scene,
            editor_camera: None,
            play_snapshot: None,
            code_path: "testGame/scripts/main.rscript".into(),
            code_source: String::new(),
            code_message: None,
            code_dirty: false,
        }
    }
}

/// Structured editor console entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsoleEntry {
    pub level: ConsoleLevel,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsoleLevel {
    Info,
    Warning,
    Error,
}

#[derive(Resource, Clone, Debug, Default)]
pub struct EditorConsole {
    entries: Vec<ConsoleEntry>,
}

impl EditorConsole {
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

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// Egui context plus the latest tessellation-ready frame output.
#[derive(Resource, Default)]
pub struct EditorUi {
    context: Context,
    output: Option<egui::FullOutput>,
    next_input: RawInput,
}

impl EditorUi {
    #[must_use]
    pub fn context(&self) -> &Context {
        &self.context
    }

    pub fn set_input(&mut self, input: RawInput) {
        self.next_input = input;
    }

    pub fn take_output(&mut self) -> Option<egui::FullOutput> {
        self.output.take()
    }
}

/// Installs the editor shell without coupling runtime-only builds to egui.
#[derive(Clone, Copy, Debug, Default)]
pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) -> Result<(), AppError> {
        app.insert_resource(EditorState::default())
            .insert_resource(EditorViewport::default())
            .insert_resource(EditorConsole::default())
            .insert_resource(EditorUi::default())
            .add_systems(ScheduleStage::Update, draw_editor);
        Ok(())
    }
}

fn draw_editor(world: &mut World) {
    let (context, input) = {
        let mut editor_ui = world.resource_mut::<EditorUi>();
        (
            editor_ui.context.clone(),
            std::mem::take(&mut editor_ui.next_input),
        )
    };
    let mut state = world.resource::<EditorState>().clone();
    let frame_time = *world.resource::<FrameTime>();
    let entities = collect_entities(world);
    let console_entries = world.resource::<EditorConsole>().entries.clone();
    let asset_summary =
        world
            .get_resource::<AssetServer>()
            .map(|server| AssetSummary {
                meshes: server.meshes.len(),
                textures: server.textures.len(),
                materials: server.materials.len(),
                scenes: server.scenes.len(),
                paths: server
                    .meshes
                    .paths()
                    .map(|(_, path)| path.display().to_string())
                    .chain(
                        server
                            .textures
                            .paths()
                            .map(|(_, path)| path.display().to_string()),
                    )
                    .chain(
                        server
                            .materials
                            .paths()
                            .map(|(_, path)| path.display().to_string()),
                    )
                    .collect(),
            });
    let render_summary =
        world.get_resource::<RenderWorld>().map(|render_world| {
            (render_world.report, render_world.dirty_ranges.clone())
        });
    let mut edited_transform = state
        .selected
        .and_then(|entity| world.get::<Transform>(entity).copied());
    let mut play_clicked = false;
    let mut pause_clicked = false;
    let mut stop_clicked = false;
    let mut step_clicked = false;

    let output = context.run(input, |context| {
        TopBottomPanel::top("rusting_editor_toolbar").show(context, |ui| {
            ui.horizontal(|ui| {
                ui.heading("RustingEngine");
                ui.separator();
                play_clicked = ui.button("▶ Play").clicked();
                pause_clicked = ui.button("⏸ Pause").clicked();
                step_clicked = ui.button("⏭ Step").clicked();
                stop_clicked = ui.button("⏹ Stop").clicked();
                ui.separator();
                ui.label(format!(
                    "frame {} · {:.2} ms",
                    frame_time.frame,
                    frame_time.real_delta.as_secs_f64() * 1_000.0
                ));
            });
        });

        if state.hierarchy_open {
            SidePanel::left("rusting_editor_hierarchy")
                .default_width(220.0)
                .show(context, |ui| {
                    ui.heading("Hierarchy");
                    ui.separator();
                    for item in &entities {
                        let indentation = "  ".repeat(item.depth);
                        let label = format!("{indentation}{}", item.name);
                        if ui
                            .selectable_label(
                                state.selected == Some(item.entity),
                                label,
                            )
                            .clicked()
                        {
                            state.selected = Some(item.entity);
                            edited_transform = item.transform;
                        }
                    }
                });
        }

        if state.inspector_open {
            SidePanel::right("rusting_editor_inspector")
                .default_width(280.0)
                .show(context, |ui| {
                    ui.heading("Inspector");
                    ui.separator();
                    if let Some(entity) = state.selected {
                        ui.monospace(format!("{entity:?}"));
                        if let Some(transform) = &mut edited_transform {
                            ui.collapsing("Transform", |ui| {
                                edit_vector(
                                    ui,
                                    "Position",
                                    &mut transform.position,
                                    0.1,
                                );
                                edit_vector(
                                    ui,
                                    "Rotation",
                                    &mut transform.rotation,
                                    0.01,
                                );
                                edit_vector(
                                    ui,
                                    "Scale",
                                    &mut transform.scale,
                                    0.01,
                                );
                            });
                        } else {
                            ui.label("This entity has no editable Transform.");
                        }
                        if let Some(renderer) =
                            world.get::<MeshRenderer>(entity).copied()
                        {
                            ui.collapsing("Mesh Renderer", |ui| {
                                ui.monospace(format!(
                                    "Mesh: {}:{}",
                                    renderer.mesh.index(),
                                    renderer.mesh.generation()
                                ));
                                ui.monospace(format!(
                                    "Material: {}:{}",
                                    renderer.material.index(),
                                    renderer.material.generation()
                                ));
                                ui.label(format!(
                                    "Cast shadows: {}",
                                    renderer.cast_shadows
                                ));
                            });
                        }
                    } else {
                        ui.label("Select an entity to inspect it.");
                    }
                });
        }

        if state.console_open {
            TopBottomPanel::bottom("rusting_editor_console")
                .default_height(140.0)
                .show(context, |ui| {
                    ui.heading("Console");
                    ui.separator();
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for entry in &console_entries {
                            let color = match entry.level {
                                ConsoleLevel::Info => ui.visuals().text_color(),
                                ConsoleLevel::Warning => egui::Color32::YELLOW,
                                ConsoleLevel::Error => egui::Color32::LIGHT_RED,
                            };
                            ui.colored_label(color, &entry.message);
                        }
                        if let Some((report, dirty_ranges)) = &render_summary {
                            ui.separator();
                            ui.label(format!(
                                "Render extract: {} total, +{}, ~{}, -{}",
                                report.total,
                                report.added,
                                report.changed,
                                report.removed
                            ));
                            ui.monospace(format!(
                                "Dirty uploads: {dirty_ranges:?}"
                            ));
                        }
                    });
                });
        }

        if state.assets_open {
            egui::Window::new("Asset Browser")
                .default_pos([240.0, 80.0])
                .default_size([360.0, 260.0])
                .show(context, |ui| {
                    if let Some(summary) = &asset_summary {
                        ui.horizontal(|ui| {
                            ui.label(format!("Meshes: {}", summary.meshes));
                            ui.label(format!("Textures: {}", summary.textures));
                            ui.label(format!(
                                "Materials: {}",
                                summary.materials
                            ));
                            ui.label(format!("Scenes: {}", summary.scenes));
                        });
                        ui.separator();
                        if summary.paths.is_empty() {
                            ui.label("No file-backed assets loaded.");
                        } else {
                            egui::ScrollArea::vertical().show(ui, |ui| {
                                for path in &summary.paths {
                                    ui.monospace(path);
                                }
                            });
                        }
                    } else {
                        ui.label("AssetPlugin is not installed.");
                    }
                });
        }

        CentralPanel::default().show(context, |ui| {
            ui.heading("Scene Viewport");
            ui.separator();
            let rect = ui.available_rect_before_wrap();
            ui.painter().rect_filled(
                rect,
                2.0,
                egui::Color32::from_rgb(18, 20, 24),
            );
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Renderer viewport target",
                egui::FontId::proportional(18.0),
                egui::Color32::GRAY,
            );
        });
    });

    if play_clicked {
        state.mode = EditorMode::Play;
        world.resource_mut::<TimeControl>().resume();
    }
    if pause_clicked {
        state.mode = EditorMode::Paused;
        world.resource_mut::<TimeControl>().pause();
    }
    if step_clicked {
        state.mode = EditorMode::Paused;
        let mut time = world.resource_mut::<TimeControl>();
        time.pause();
        time.step();
    }
    if stop_clicked {
        state.mode = EditorMode::Edit;
        world.resource_mut::<TimeControl>().pause();
    }
    if let (Some(entity), Some(transform)) = (state.selected, edited_transform)
    {
        if let Ok(mut entity) = world.get_entity_mut(entity) {
            entity.insert(transform);
        } else {
            state.selected = None;
        }
    }
    *world.resource_mut::<EditorState>() = state;
    world.resource_mut::<EditorUi>().output = Some(output);
}

fn edit_vector(
    ui: &mut egui::Ui,
    label: &str,
    values: &mut [f32; 3],
    speed: f64,
) {
    ui.horizontal(|ui| {
        ui.label(label);
        for (axis, value) in ["X", "Y", "Z"].into_iter().zip(values) {
            ui.label(axis);
            ui.add(DragValue::new(value).speed(speed));
        }
    });
}

struct HierarchyItem {
    entity: Entity,
    name: String,
    depth: usize,
    transform: Option<Transform>,
}

struct AssetSummary {
    meshes: usize,
    textures: usize,
    materials: usize,
    scenes: usize,
    paths: Vec<String>,
}

fn collect_entities(world: &mut World) -> Vec<HierarchyItem> {
    let mut query = world.query::<(
        Entity,
        Option<&Name>,
        Option<&Parent>,
        Option<&Transform>,
    )>();
    let raw = query
        .iter(world)
        .filter(|(_, name, parent, transform)| {
            name.is_some() || parent.is_some() || transform.is_some()
        })
        .map(|(entity, name, parent, transform)| {
            (
                entity,
                name.map_or_else(
                    || format!("Entity {entity:?}"),
                    |name| name.0.clone(),
                ),
                parent.map(|parent| parent.0),
                transform.copied(),
            )
        })
        .collect::<Vec<_>>();
    let parents = raw
        .iter()
        .filter_map(|(entity, _, parent, _)| {
            parent.map(|parent| (*entity, parent))
        })
        .collect::<std::collections::HashMap<_, _>>();

    let mut items = raw
        .into_iter()
        .map(|(entity, name, _, transform)| {
            let mut depth = 0;
            let mut cursor = entity;
            let mut visited = std::collections::HashSet::new();
            while let Some(parent) = parents.get(&cursor).copied() {
                if !visited.insert(parent) {
                    break;
                }
                depth += 1;
                cursor = parent;
            }
            HierarchyItem {
                entity,
                name,
                depth,
                transform,
            }
        })
        .collect::<Vec<_>>();
    items.sort_by_key(|item| (item.depth, item.name.clone(), item.entity));
    items
}

/// Draws the interactive editor into an already-open egui frame.
///
/// Window runners use this entry point after forwarding winit input to egui.
pub fn draw_editor_view(world: &mut World, context: &Context) {
    let mut state = world.resource::<EditorState>().clone();
    let entities = collect_entities(world);
    let frame_time = *world.resource::<FrameTime>();
    let mut render_settings = world.resource::<RenderSettings>().clone();
    let physics_backends = *world.resource::<PhysicsBackendStatus>();
    let asset_counts = world.get_resource::<AssetServer>().map(|assets| {
        (
            assets.meshes.len(),
            assets.materials.len(),
            assets.textures.len(),
            assets.scenes.len(),
        )
    });
    let render_report = world
        .get_resource::<RenderWorld>()
        .map(|render_world| render_world.report);
    let mut edited_transform = state
        .selected
        .and_then(|entity| world.get::<Transform>(entity).copied());
    let mut edited_camera = state
        .selected
        .and_then(|entity| world.get::<Camera>(entity).copied());
    let mut edited_physics = state
        .selected
        .and_then(|entity| world.get::<PhysicsBody>(entity).cloned());
    let mut edited_rigid_body = state
        .selected
        .and_then(|entity| world.get::<RigidBody>(entity).copied());
    let mut edited_collider = state
        .selected
        .and_then(|entity| world.get::<Collider>(entity).copied());
    let mut edited_script = state
        .selected
        .and_then(|entity| world.get::<ScriptComponent>(entity).cloned());
    let registered_names = registered_component_names(world);
    let custom_values = state
        .selected
        .map(|entity| registered_component_values(world, entity))
        .transpose()
        .unwrap_or_else(|error| {
            state.scene_message = Some(format!("Inspector failed: {error}"));
            None
        })
        .unwrap_or_default();
    if let Some(entity) = state.selected {
        for (name, value) in &custom_values {
            state
                .component_drafts
                .entry((entity, name.clone()))
                .or_insert_with(|| value.clone());
        }
    }
    let mut requested_mode = None;
    let mut viewport_rect = None;
    let mut save_clicked = false;
    let mut load_clicked = false;
    let mut component_edits = Vec::new();
    let mut add_physics = false;
    let mut remove_physics = false;
    let mut edit_custom_shader = false;
    let mut load_code = false;
    let mut save_code = false;
    let mut validate_code = false;
    let mut remove_script = false;
    let mut open_script = false;

    TopBottomPanel::top("visible_editor_toolbar").show(context, |ui| {
        ui.horizontal(|ui| {
            ui.heading("RustingEngine");
            ui.separator();
            if ui.button("▶ Play").clicked() {
                requested_mode = Some(EditorMode::Play);
            }
            if ui.button("⏸ Pause").clicked() {
                requested_mode = Some(EditorMode::Paused);
            }
            if ui.button("⏭ Step").clicked() {
                requested_mode = Some(EditorMode::Paused);
                world.resource_mut::<TimeControl>().step();
            }
            if ui.button("⏹ Stop").clicked() {
                requested_mode = Some(EditorMode::Edit);
            }
            ui.separator();
            ui.selectable_value(
                &mut state.workspace,
                EditorWorkspace::Scene,
                "Scene",
            );
            ui.selectable_value(
                &mut state.workspace,
                EditorWorkspace::Game,
                "Game",
            );
            ui.selectable_value(
                &mut state.workspace,
                EditorWorkspace::Code,
                "Code",
            );
            ui.separator();
            save_clicked = ui.button("Save Scene").clicked();
            load_clicked = ui.button("Load Scene").clicked();
            ui.separator();
            ui.label(format!(
                "Frame {} · {:.2} ms",
                frame_time.frame,
                frame_time.real_delta.as_secs_f64() * 1_000.0
            ));
            ui.separator();
            ui.label(match state.mode {
                EditorMode::Edit => "EDIT",
                EditorMode::Play => "PLAYING",
                EditorMode::Paused => "PAUSED",
            });
        });
    });

    SidePanel::left("visible_editor_hierarchy")
        .default_width(230.0)
        .show(context, |ui| {
            ui.heading("Hierarchy");
            ui.separator();
            for item in &entities {
                let label = format!("{}{}", "  ".repeat(item.depth), item.name);
                if ui
                    .selectable_label(
                        state.selected == Some(item.entity),
                        label,
                    )
                    .clicked()
                {
                    state.selected = Some(item.entity);
                    edited_transform = item.transform;
                    edited_camera = world.get::<Camera>(item.entity).copied();
                    edited_physics =
                        world.get::<PhysicsBody>(item.entity).cloned();
                    edited_rigid_body =
                        world.get::<RigidBody>(item.entity).copied();
                    edited_collider =
                        world.get::<Collider>(item.entity).copied();
                    edited_script =
                        world.get::<ScriptComponent>(item.entity).cloned();
                }
            }
        });

    SidePanel::right("visible_editor_inspector")
        .default_width(300.0)
        .show(context, |ui| {
            ui.heading("Inspector");
            ui.separator();
            if let Some(entity) = state.selected {
                ui.monospace(format!("{entity:?}"));
                if let Some(transform) = &mut edited_transform {
                    ui.collapsing("Transform", |ui| {
                        edit_vector(
                            ui,
                            "Position",
                            &mut transform.position,
                            0.1,
                        );
                        edit_vector(
                            ui,
                            "Rotation",
                            &mut transform.rotation,
                            0.01,
                        );
                        edit_vector(ui, "Scale", &mut transform.scale, 0.01);
                    });
                }
                if let Some(renderer) = world.get::<MeshRenderer>(entity) {
                    ui.separator();
                    ui.label("Mesh Renderer");
                    ui.monospace(format!("Mesh: {}", renderer.mesh.key()));
                    ui.monospace(format!(
                        "Material: {}",
                        renderer.material.key()
                    ));
                }
                if let Some(camera) = &mut edited_camera {
                    ui.separator();
                    ui.label("Camera");
                    ui.checkbox(&mut camera.active, "Active");
                    ui.add(
                        DragValue::new(&mut camera.priority)
                            .prefix("Priority ")
                            .speed(1.0),
                    );
                    match &mut camera.projection {
                        Projection::Perspective {
                            vertical_fov_radians,
                            near,
                            far,
                        } => {
                            let mut fov_degrees =
                                vertical_fov_radians.to_degrees();
                            ui.add(
                                egui::Slider::new(
                                    &mut fov_degrees,
                                    10.0..=120.0,
                                )
                                .text("Vertical FOV")
                                .suffix("°"),
                            );
                            *vertical_fov_radians = fov_degrees.to_radians();
                            projection_planes(ui, near, far);
                        }
                        Projection::Orthographic {
                            vertical_size,
                            near,
                            far,
                        } => {
                            ui.add(
                                DragValue::new(vertical_size)
                                    .prefix("Vertical size ")
                                    .range(0.01..=100_000.0)
                                    .speed(0.1),
                            );
                            projection_planes(ui, near, far);
                        }
                    }
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Physics");
                    if edited_physics.is_some() {
                        if ui.small_button("Remove").clicked() {
                            remove_physics = true;
                        }
                    } else if ui.small_button("Add Physics").clicked() {
                        add_physics = true;
                        edited_physics = Some(PhysicsBody::default());
                        edited_rigid_body = Some(RigidBody::default());
                        edited_collider = Some(Collider::default());
                    }
                });
                if let Some(physics) = &mut edited_physics {
                    ComboBox::from_label("Simulation")
                        .selected_text(simulation_class_name(
                            physics.simulation,
                        ))
                        .show_ui(ui, |ui| {
                            for class in [
                                SimulationClass::None,
                                SimulationClass::Static,
                                SimulationClass::Gameplay,
                                SimulationClass::GpuDynamic,
                            ] {
                                ui.selectable_value(
                                    &mut physics.simulation,
                                    class,
                                    simulation_class_name(class),
                                );
                            }
                        });
                    match physics.simulation {
                        SimulationClass::None => {
                            ui.small("Excluded from every physics backend.");
                        }
                        SimulationClass::Static => {
                            ui.small(
                                "Static collider: uploaded once, never dispatched.",
                            );
                            if let Some(body) = &mut edited_rigid_body {
                                body.kind = RigidBodyKind::Fixed;
                            }
                        }
                        SimulationClass::Gameplay => {
                            if physics_backends.gameplay_available {
                                ui.small("CPU-authoritative gameplay physics.");
                            } else {
                                ui.colored_label(
                                    egui::Color32::LIGHT_RED,
                                    "Gameplay physics backend is not connected yet.",
                                );
                            }
                        }
                        SimulationClass::GpuDynamic => {
                            if physics_backends.gpu_dynamic_available {
                                ui.small("High-volume Vulkan compute simulation.");
                            } else {
                                ui.colored_label(
                                    egui::Color32::LIGHT_RED,
                                    "GPU physics is saved, but the ECS renderer cannot execute it yet.",
                                );
                            }
                            ComboBox::from_label("GPU solver")
                                .selected_text(physics_solver_name(
                                    physics.solver,
                                ))
                                .show_ui(ui, |ui| {
                                    for solver in [
                                        PhysicsSolver::Full,
                                        PhysicsSolver::Simplified,
                                        PhysicsSolver::NoCollision,
                                        PhysicsSolver::Custom,
                                    ] {
                                        ui.selectable_value(
                                            &mut physics.solver,
                                            solver,
                                            physics_solver_name(solver),
                                        );
                                    }
                                });
                            if physics.solver == PhysicsSolver::Custom {
                                let path = physics
                                    .custom_shader
                                    .get_or_insert_with(|| {
                                        format!(
                                            "{}/shaders/custom.comp",
                                            state.project_root
                                        )
                                    });
                                ui.horizontal(|ui| {
                                    ui.label("Shader");
                                    ui.text_edit_singleline(path);
                                });
                                if ui.button("Open in Code Editor").clicked() {
                                    edit_custom_shader = true;
                                }
                            }
                        }
                    }
                    if physics.simulation != SimulationClass::None {
                        let body = edited_rigid_body
                            .get_or_insert_with(RigidBody::default);
                        if physics.simulation != SimulationClass::Static {
                            ComboBox::from_label("Body")
                                .selected_text(rigid_body_kind_name(body.kind))
                                .show_ui(ui, |ui| {
                                    for kind in [
                                        RigidBodyKind::Dynamic,
                                        RigidBodyKind::Kinematic,
                                        RigidBodyKind::Fixed,
                                    ] {
                                        ui.selectable_value(
                                            &mut body.kind,
                                            kind,
                                            rigid_body_kind_name(kind),
                                        );
                                    }
                                });
                            ui.add(
                                DragValue::new(&mut body.mass)
                                    .prefix("Mass ")
                                    .suffix(" kg")
                                    .range(0.001..=1_000_000.0)
                                    .speed(0.1),
                            );
                            ui.add(
                                DragValue::new(&mut body.gravity_scale)
                                    .prefix("Gravity ")
                                    .range(-100.0..=100.0)
                                    .speed(0.05),
                            );
                            edit_vector(
                                ui,
                                "Velocity",
                                &mut body.linear_velocity,
                                0.1,
                            );
                        }
                        let collider = edited_collider
                            .get_or_insert_with(Collider::default);
                        edit_collider(ui, collider);
                    }
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Gameplay Script");
                    if edited_script.is_some() {
                        if ui.small_button("Remove").clicked() {
                            remove_script = true;
                        }
                    } else if ui.small_button("Add Script").clicked() {
                        edited_script = Some(ScriptComponent {
                            source_path: format!(
                                "{}/scripts/main.rscript",
                                state.project_root
                            ),
                            ..ScriptComponent::default()
                        });
                    }
                });
                if let Some(script) = &mut edited_script {
                    ui.checkbox(&mut script.enabled, "Enabled");
                    ui.horizontal(|ui| {
                        ui.label("Source");
                        ui.text_edit_singleline(&mut script.source_path);
                    });
                    if ui.button("Open Gameplay Script").clicked() {
                        open_script = true;
                    }
                    if let Some(error) = world
                        .get_resource::<ScriptRuntime>()
                        .and_then(|runtime| runtime.errors().get(&entity))
                    {
                        ui.colored_label(egui::Color32::LIGHT_RED, error);
                    }
                }
                ui.separator();
                ui.label("Compiled Components");
                for (name, _) in &custom_values {
                    let key = (entity, name.clone());
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.monospace(name);
                            if ui.button("Apply").clicked() {
                                if let Some(value) =
                                    state.component_drafts.get(&key)
                                {
                                    component_edits.push(ComponentEdit::Set {
                                        entity,
                                        name: name.clone(),
                                        value: value.clone(),
                                    });
                                }
                            }
                            if ui.button("Remove").clicked() {
                                component_edits.push(ComponentEdit::Remove {
                                    entity,
                                    name: name.clone(),
                                });
                            }
                        });
                        if let Some(value) =
                            state.component_drafts.get_mut(&key)
                        {
                            ui.text_edit_multiline(value);
                        }
                    });
                }
                for name in registered_names.iter().filter(|name| {
                    !custom_values.iter().any(|(present, _)| present == *name)
                }) {
                    if ui.button(format!("+ {name}")).clicked() {
                        component_edits.push(ComponentEdit::Add {
                            entity,
                            name: name.clone(),
                        });
                    }
                }
            } else {
                ui.label("Select an entity in the hierarchy.");
            }
        });

    TopBottomPanel::bottom("visible_editor_status")
        .default_height(110.0)
        .show(context, |ui| {
            ui.heading("Assets & Render Extract");
            ui.horizontal(|ui| {
                ui.label("Project");
                ui.text_edit_singleline(&mut state.project_root);
                ui.separator();
                ui.label("Scene");
                ui.text_edit_singleline(&mut state.scene_path);
            });
            if let Some(message) = &state.scene_message {
                ui.label(message);
            }
            ui.horizontal(|ui| {
                ui.checkbox(&mut render_settings.vsync, "VSync");
                ui.checkbox(&mut render_settings.limit_fps, "Limit FPS");
                ui.add_enabled(
                    render_settings.limit_fps,
                    DragValue::new(&mut render_settings.max_fps)
                        .prefix("Maximum ")
                        .range(1..=1_000),
                );
            });
            if let Some((meshes, materials, textures, scenes)) = asset_counts {
                ui.label(format!(
                    "Meshes {meshes} · Materials {materials} · Textures {textures} · Scenes {scenes}"
                ));
            }
            if let Some(report) = render_report {
                ui.label(format!(
                    "Renderables {} · added {} · changed {} · removed {}",
                    report.total, report.added, report.changed, report.removed
                ));
            }
        });

    CentralPanel::default()
        .frame(match state.workspace {
            EditorWorkspace::Scene | EditorWorkspace::Game => {
                egui::Frame::NONE.fill(egui::Color32::TRANSPARENT)
            }
            EditorWorkspace::Code => egui::Frame::NONE
                .inner_margin(egui::Margin::same(10))
                .fill(egui::Color32::from_rgb(20, 22, 27)),
        })
        .show(context, |ui| match state.workspace {
            EditorWorkspace::Scene | EditorWorkspace::Game => {
                ui.horizontal(|ui| {
                    ui.heading(match state.workspace {
                        EditorWorkspace::Scene => "Scene View",
                        EditorWorkspace::Game => "Game View",
                        EditorWorkspace::Code => unreachable!(),
                    });
                    ui.separator();
                    ui.small(match state.workspace {
                        EditorWorkspace::Scene => {
                            "Editor camera · selection and authoring"
                        }
                        EditorWorkspace::Game => {
                            "Active game camera · runtime preview"
                        }
                        EditorWorkspace::Code => unreachable!(),
                    });
                });
                ui.separator();
                let rect = ui.available_rect_before_wrap();
                viewport_rect = Some(rect);
            }
            EditorWorkspace::Code => {
                ui.horizontal(|ui| {
                    ui.heading("Code Editor");
                    ui.separator();
                    load_code = ui.button("Open").clicked();
                    save_code = ui.button("Save").clicked();
                    validate_code = ui.button("Validate").clicked();
                    if state.code_dirty {
                        ui.colored_label(
                            egui::Color32::YELLOW,
                            "Unsaved changes",
                        );
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Project file");
                    ui.text_edit_singleline(&mut state.code_path);
                });
                if let Some(message) = &state.code_message {
                    ui.small(message);
                }
                ui.separator();
                let editor = egui::TextEdit::multiline(&mut state.code_source)
                    .code_editor()
                    .desired_width(f32::INFINITY)
                    .desired_rows(30)
                    .lock_focus(true);
                if ui.add_sized(ui.available_size(), editor).changed() {
                    state.code_dirty = true;
                }
            }
        });

    if let Some(rect) = viewport_rect {
        let pixels_per_point = context.pixels_per_point();
        let min = rect.min * pixels_per_point;
        let max = rect.max * pixels_per_point;
        let offset =
            [min.x.max(0.0).floor() as u32, min.y.max(0.0).floor() as u32];
        let end = [
            max.x.max(min.x).ceil() as u32,
            max.y.max(min.y).ceil() as u32,
        ];
        let viewport = EditorViewport {
            offset,
            extent: [
                end[0].saturating_sub(offset[0]),
                end[1].saturating_sub(offset[1]),
            ],
            valid: true,
        };
        if let Some(mut current) = world.get_resource_mut::<EditorViewport>() {
            *current = viewport;
        } else {
            world.insert_resource(viewport);
        }
    } else if let Some(mut current) = world.get_resource_mut::<EditorViewport>()
    {
        current.valid = false;
    }

    if let (Some(entity), Some(transform)) = (state.selected, edited_transform)
    {
        if let Ok(mut entity) = world.get_entity_mut(entity) {
            entity.insert(transform);
        }
    }
    if let (Some(entity), Some(camera)) = (state.selected, edited_camera) {
        if let Ok(mut entity) = world.get_entity_mut(entity) {
            entity.insert(camera);
        }
    }
    if let Some(entity) = state.selected {
        if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
            if remove_physics {
                entity_mut.remove::<(
                    PhysicsBody,
                    RigidBody,
                    Collider,
                    CollisionLayers,
                    crate::runtime::GpuEffectBody,
                )>();
            } else if let Some(physics) = edited_physics {
                let uses_gpu = physics.uses_gpu();
                entity_mut.insert(physics);
                if let Some(rigid_body) = edited_rigid_body {
                    entity_mut.insert(rigid_body);
                }
                if let Some(collider) = edited_collider {
                    entity_mut.insert(collider);
                }
                if add_physics && !entity_mut.contains::<CollisionLayers>() {
                    entity_mut.insert(CollisionLayers::default());
                }
                if uses_gpu {
                    entity_mut.insert(crate::runtime::GpuEffectBody);
                } else {
                    entity_mut.remove::<crate::runtime::GpuEffectBody>();
                }
            }
        }
    }
    if let Some(entity) = state.selected {
        let existing_path = world
            .get::<ScriptComponent>(entity)
            .map(|script| script.source_path.clone());
        if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
            if remove_script {
                entity_mut.remove::<ScriptComponent>();
            } else if let Some(mut script) = edited_script {
                if existing_path.as_deref() != Some(&script.source_path) {
                    script.compiled = None;
                }
                entity_mut.insert(script);
            }
        }
    }
    if open_script {
        if let Some(path) = state
            .selected
            .and_then(|entity| world.get::<ScriptComponent>(entity))
            .map(|script| script.source_path.clone())
        {
            state.code_path = path;
            state.workspace = EditorWorkspace::Code;
            load_code = true;
        }
    }
    if edit_custom_shader {
        if let Some(path) = state
            .selected
            .and_then(|entity| world.get::<PhysicsBody>(entity))
            .and_then(|physics| physics.custom_shader.clone())
        {
            state.code_path = path;
            state.workspace = EditorWorkspace::Code;
            load_code = true;
        }
    }
    if load_code {
        match load_project_source(&state.project_root, &state.code_path) {
            Ok(source) => {
                state.code_source = source;
                state.code_dirty = false;
                state.code_message =
                    Some(format!("Opened {}", state.code_path));
            }
            Err(error) => state.code_message = Some(error),
        }
    }
    if validate_code {
        state.code_message = Some(
            validate_project_source(
                &state.project_root,
                &state.code_path,
                &state.code_source,
            )
            .map_or_else(|error| error, |()| "Validation passed".into()),
        );
    }
    if save_code {
        state.code_message = Some(
            match save_project_source(
                &state.project_root,
                &state.code_path,
                &state.code_source,
            ) {
                Ok(()) => {
                    state.code_dirty = false;
                    if state.code_path.ends_with(".rscript") {
                        crate::runtime::invalidate_script(
                            world,
                            &state.code_path,
                        );
                    }
                    format!("Saved {}", state.code_path)
                }
                Err(error) => error,
            },
        );
    }
    if let Some(mode) = requested_mode {
        match mode {
            EditorMode::Play => {
                if state.mode == EditorMode::Edit {
                    match scene_document(world, "Play Snapshot") {
                        Ok(snapshot) => state.play_snapshot = Some(snapshot),
                        Err(error) => {
                            state.scene_message =
                                Some(format!("Play failed: {error}"));
                            *world.resource_mut::<EditorState>() = state;
                            return;
                        }
                    }
                }
                state.mode = EditorMode::Play;
                state.workspace = EditorWorkspace::Game;
                world.resource_mut::<ScriptSettings>().enabled = true;
                world.resource_mut::<TimeControl>().resume();
            }
            EditorMode::Paused => {
                state.mode = EditorMode::Paused;
                world.resource_mut::<TimeControl>().pause();
            }
            EditorMode::Edit => {
                world.resource_mut::<TimeControl>().pause();
                world.resource_mut::<ScriptSettings>().enabled = false;
                world.resource_mut::<ScriptRuntime>().reset();
                if let Some(snapshot) = state.play_snapshot.take() {
                    match crate::runtime::load_scene_document(
                        world,
                        &snapshot,
                        SceneLoadMode::Replace,
                    ) {
                        Ok(_) => {
                            state.selected = None;
                            state.scene_message = Some(
                                "Stopped play mode and restored the scene"
                                    .into(),
                            );
                        }
                        Err(error) => {
                            state.scene_message = Some(format!(
                                "Could not restore play snapshot: {error}"
                            ));
                        }
                    }
                }
                state.mode = EditorMode::Edit;
                state.workspace = EditorWorkspace::Scene;
            }
        }
    }
    if save_clicked {
        state.scene_message =
            Some(match save_scene(world, &state.scene_path, "Main Scene") {
                Ok(()) => format!("Saved {}", state.scene_path),
                Err(error) => format!("Save failed: {error}"),
            });
    }
    if load_clicked {
        state.scene_message = Some(
            match load_scene(world, &state.scene_path, SceneLoadMode::Replace) {
                Ok(entity_count) => {
                    state.selected = None;
                    format!(
                        "Loaded {entity_count} entities from {}",
                        state.scene_path
                    )
                }
                Err(error) => format!("Load failed: {error}"),
            },
        );
    }
    for edit in component_edits {
        let result = match edit {
            ComponentEdit::Set {
                entity,
                name,
                value,
            } => set_registered_component(world, entity, &name, &value),
            ComponentEdit::Add { entity, name } => {
                add_registered_component(world, entity, &name)
            }
            ComponentEdit::Remove { entity, name } => {
                state.component_drafts.remove(&(entity, name.clone()));
                remove_registered_component(world, entity, &name)
            }
        };
        if let Err(error) = result {
            state.scene_message =
                Some(format!("Component edit failed: {error}"));
        }
    }
    world.resource_mut::<RenderCameraOverride>().entity = (state.workspace
        == EditorWorkspace::Scene)
        .then_some(state.editor_camera)
        .flatten();
    render_settings.max_fps = render_settings.max_fps.max(1);
    *world.resource_mut::<RenderSettings>() = render_settings;
    *world.resource_mut::<EditorState>() = state;
}

enum ComponentEdit {
    Set {
        entity: Entity,
        name: String,
        value: String,
    },
    Add {
        entity: Entity,
        name: String,
    },
    Remove {
        entity: Entity,
        name: String,
    },
}

fn simulation_class_name(class: SimulationClass) -> &'static str {
    match class {
        SimulationClass::None => "No Physics",
        SimulationClass::Static => "Static",
        SimulationClass::Gameplay => "Gameplay (CPU)",
        SimulationClass::GpuDynamic => "GPU Dynamic",
    }
}

fn physics_solver_name(solver: PhysicsSolver) -> &'static str {
    match solver {
        PhysicsSolver::Full => "Full Physics",
        PhysicsSolver::Simplified => "Simplified",
        PhysicsSolver::NoCollision => "Gravity / No Collision",
        PhysicsSolver::Custom => "Custom Compute Shader",
    }
}

fn rigid_body_kind_name(kind: RigidBodyKind) -> &'static str {
    match kind {
        RigidBodyKind::Fixed => "Fixed",
        RigidBodyKind::Dynamic => "Dynamic",
        RigidBodyKind::Kinematic => "Kinematic",
    }
}

fn edit_collider(ui: &mut egui::Ui, collider: &mut Collider) {
    let shape_name = match collider.shape {
        ColliderShape::Box { .. } => "Box",
        ColliderShape::Sphere { .. } => "Sphere",
        ColliderShape::Capsule { .. } => "Capsule",
    };
    ComboBox::from_label("Collider")
        .selected_text(shape_name)
        .show_ui(ui, |ui| {
            if ui.selectable_label(shape_name == "Box", "Box").clicked() {
                collider.shape = ColliderShape::Box {
                    half_extents: [0.5; 3],
                };
            }
            if ui
                .selectable_label(shape_name == "Sphere", "Sphere")
                .clicked()
            {
                collider.shape = ColliderShape::Sphere { radius: 0.5 };
            }
            if ui
                .selectable_label(shape_name == "Capsule", "Capsule")
                .clicked()
            {
                collider.shape = ColliderShape::Capsule {
                    half_height: 0.5,
                    radius: 0.5,
                };
            }
        });
    match &mut collider.shape {
        ColliderShape::Box { half_extents } => {
            edit_vector(ui, "Half size", half_extents, 0.05);
            for extent in half_extents {
                *extent = extent.max(0.001);
            }
        }
        ColliderShape::Sphere { radius } => {
            ui.add(
                DragValue::new(radius)
                    .prefix("Radius ")
                    .range(0.001..=1_000_000.0)
                    .speed(0.05),
            );
        }
        ColliderShape::Capsule {
            half_height,
            radius,
        } => {
            ui.add(
                DragValue::new(half_height)
                    .prefix("Half height ")
                    .range(0.001..=1_000_000.0)
                    .speed(0.05),
            );
            ui.add(
                DragValue::new(radius)
                    .prefix("Radius ")
                    .range(0.001..=1_000_000.0)
                    .speed(0.05),
            );
        }
    }
    ui.add(
        egui::Slider::new(&mut collider.friction, 0.0..=1.0).text("Friction"),
    );
    ui.add(
        egui::Slider::new(&mut collider.restitution, 0.0..=1.0)
            .text("Bounciness"),
    );
    ui.checkbox(&mut collider.sensor, "Trigger / sensor");
}

fn project_source_path<'a>(
    project_root: &str,
    path: &'a str,
) -> Result<&'a std::path::Path, String> {
    use std::path::Component;

    let root = std::path::Path::new(project_root);
    if root.as_os_str().is_empty()
        || root.is_absolute()
        || root
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        || !root.join("project.json").is_file()
    {
        return Err(
            "Selected project must contain a project.json manifest".into()
        );
    }
    let path = std::path::Path::new(path);
    if path.is_absolute()
        || path.components().any(|part| {
            !matches!(part, Component::Normal(_) | Component::CurDir)
        })
        || !path.starts_with(project_root)
    {
        return Err(
            "Code editor paths must stay inside the selected game project"
                .into(),
        );
    }
    let supported = path.extension().and_then(|extension| extension.to_str());
    if !matches!(
        supported,
        Some("rs" | "rscript" | "glsl" | "comp" | "vert" | "frag")
    ) {
        return Err(
            "Supported source types: .rscript, .rs, .glsl, .comp, .vert, .frag"
                .into(),
        );
    }
    Ok(path)
}

fn load_project_source(
    project_root: &str,
    path: &str,
) -> Result<String, String> {
    let path = project_source_path(project_root, path)?;
    std::fs::read_to_string(path)
        .map_err(|error| format!("Could not open {}: {error}", path.display()))
}

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
        Some("rscript") => {
            crate::runtime::compile_script(source)
                .map_err(|error| error.to_string())?;
        }
        _ => {}
    }
    Ok(())
}

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
    std::fs::write(path, source)
        .map_err(|error| format!("Could not save {}: {error}", path.display()))
}

fn projection_planes(ui: &mut egui::Ui, near: &mut f32, far: &mut f32) {
    ui.add(
        DragValue::new(near)
            .prefix("Near ")
            .range(0.001..=1_000.0)
            .speed(0.01),
    );
    *far = (*far).max(*near + 0.001);
    ui.add(
        DragValue::new(far)
            .prefix("Far ")
            .range((*near + 0.001)..=1_000_000.0)
            .speed(1.0),
    );
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn editor_plugin_builds_panels_and_preserves_selection() {
        let mut app = App::new();
        app.add_plugin(EditorPlugin).unwrap();
        let entity = app.spawn((Name("Cube".into()), Transform::default()));
        app.world_mut().resource_mut::<EditorState>().selected = Some(entity);

        app.update(Duration::from_millis(16)).unwrap();

        assert_eq!(
            app.world().resource::<EditorState>().selected,
            Some(entity)
        );
        let output = app
            .world_mut()
            .resource_mut::<EditorUi>()
            .take_output()
            .expect("editor should emit a UI frame");
        assert!(!output.shapes.is_empty());
    }

    #[test]
    fn code_editor_rejects_paths_outside_project_sources() {
        assert!(project_source_path("testGame", "../outside.comp").is_err());
        assert!(project_source_path("testGame", "/tmp/outside.comp").is_err());
        assert!(project_source_path(
            "testGame",
            "testGame/shaders/custom.comp"
        )
        .is_ok());
        assert!(project_source_path(
            "testGame",
            "src/shaders/compute/full.comp"
        )
        .is_err());
    }

    #[test]
    fn compute_shader_validation_checks_entry_point_and_workgroup() {
        assert!(validate_project_source(
            "testGame",
            "testGame/shaders/custom.comp",
            "#version 450\nlayout(local_size_x = 64) in;\nvoid main() {}",
        )
        .is_ok());
        assert!(validate_project_source(
            "testGame",
            "testGame/shaders/custom.comp",
            "#version 450\nvoid main() {}",
        )
        .is_err());
    }
}
