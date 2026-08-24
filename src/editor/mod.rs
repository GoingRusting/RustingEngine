//! Feature-gated egui editor state and ECS panels.
//!
//! The renderer consumes [`EditorUi::take_output`] and is responsible for
//! uploading egui textures and meshes in its compositing pass.

use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::{Resource, World};
use egui::{
    CentralPanel, Context, DragValue, RawInput, SidePanel, TopBottomPanel,
};

use crate::runtime::{
    add_registered_component, load_scene, registered_component_names,
    registered_component_values, remove_registered_component, save_scene,
    set_registered_component, App, AppError, Camera, FrameTime, MeshRenderer,
    Name, Parent, Plugin, Projection, RenderSettings, RenderWorld,
    SceneLoadMode, ScheduleStage, TimeControl,
};
use crate::AssetServer;
use crate::Transform;

/// Editor interaction mode. Edit state never advances gameplay fixed updates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditorMode {
    #[default]
    Edit,
    Play,
    Paused,
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
    pub scene_message: Option<String>,
    pub component_drafts: std::collections::HashMap<(Entity, String), String>,
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
            scene_path: "assets/scenes/main.rscene".into(),
            scene_message: None,
            component_drafts: std::collections::HashMap::new(),
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

    TopBottomPanel::top("visible_editor_toolbar").show(context, |ui| {
        ui.horizontal(|ui| {
            ui.heading("RustingEngine Editor");
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
            save_clicked = ui.button("Save Scene").clicked();
            load_clicked = ui.button("Load Scene").clicked();
            ui.separator();
            ui.label(format!(
                "Frame {} · {:.2} ms",
                frame_time.frame,
                frame_time.real_delta.as_secs_f64() * 1_000.0
            ));
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
        .frame(egui::Frame::NONE.fill(egui::Color32::TRANSPARENT))
        .show(context, |ui| {
            ui.heading("Scene Viewport");
            ui.separator();
            let rect = ui.available_rect_before_wrap();
            viewport_rect = Some(rect);
            ui.painter().text(
                rect.left_top() + egui::vec2(8.0, 8.0),
                egui::Align2::LEFT_TOP,
                "Live Vulkan scene",
                egui::FontId::proportional(15.0),
                egui::Color32::from_gray(190),
            );
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
    }

    if let Some(mode) = requested_mode {
        state.mode = mode;
        let mut time = world.resource_mut::<TimeControl>();
        match mode {
            EditorMode::Play => time.resume(),
            EditorMode::Edit | EditorMode::Paused => time.pause(),
        }
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
}
