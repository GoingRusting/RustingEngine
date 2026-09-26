//! Inspector panel: the selected object's components drawn as Godot-style
//! sections of property rows.

mod json;
pub mod widgets;

use std::collections::BTreeMap;

use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::{Resource, World};
use egui::DragValue;
use serde::de::DeserializeOwned;
use serde::Serialize;

use super::gui_elements::EditorTheme;
use super::{AssetRequest, EditorState};
use crate::assets::{
    AlphaMode, AssetServer, Handle, MaterialAsset, MaterialModel, TextureAsset,
};
use crate::runtime::{
    AutoSimulation, Camera, Collider, ColliderShape, DirectionalLight,
    FrameTime, GpuStateMirror, MeshRenderer, Name, ObjectClasses,
    PhysicsBackendStatus, PhysicsBody, PhysicsSolver, PhysicsSyncMode,
    PointLight, Projection, RenderBounds, RigidBody, RigidBodyKind,
    SimulationClass, SpotLight, AUTO_SIMULATION_COMPONENT,
    PHYSICS_SYNC_COMPONENT,
};
use crate::Transform;

type ComponentInspector =
    Box<dyn Fn(&mut egui::Ui, &str) -> Option<String> + Send + Sync>;

/// Typed Inspector sections for scene components. Only components in the
/// [`crate::runtime::SceneComponentRegistry`] allowlist reach the Inspector;
/// one without an entry here is edited as generic JSON fields.
#[derive(Resource, Default)]
pub struct InspectorRegistry {
    inspectors: BTreeMap<String, ComponentInspector>,
}

impl InspectorRegistry {
    /// Draws the scene component registered as `name` with `draw`, which
    /// returns true when it changed the value. The change is saved through
    /// the scene registry, so it is undoable like any other edit.
    pub fn register<T>(
        &mut self,
        name: impl Into<String>,
        draw: fn(&mut egui::Ui, &mut T) -> bool,
    ) where
        T: Serialize + DeserializeOwned + 'static,
    {
        self.inspectors.insert(
            name.into(),
            Box::new(move |ui, serialized| {
                let Ok(mut value) = serde_json::from_str::<T>(serialized)
                else {
                    ui.colored_label(
                        EditorTheme::ERROR,
                        "Value does not match its inspector type",
                    );
                    return None;
                };
                draw(ui, &mut value)
                    .then(|| serde_json::to_string(&value).ok())
                    .flatten()
            }),
        );
    }
}

/// Custom component change requested by the Inspector, applied after drawing.
pub(super) enum ComponentEdit {
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

const SYNC_MODES: [(PhysicsSyncMode, &str); 4] = [
    (PhysicsSyncMode::None, "None"),
    (PhysicsSyncMode::Events, "Events"),
    (PhysicsSyncMode::SelectedState, "Selected State"),
    (PhysicsSyncMode::FullState, "Full State"),
];

const SIMULATION_CLASSES: [(SimulationClass, &str); 4] = [
    (SimulationClass::None, "No Physics"),
    (SimulationClass::Static, "Static"),
    (SimulationClass::Cpu, "CPU"),
    (SimulationClass::Gpu, "GPU"),
];

// `PhysicsSolver::Space` is not offered for new bodies; it is still named in
// the drop-down when a loaded scene uses it.
const PHYSICS_SOLVERS: [(PhysicsSolver, &str); 5] = [
    (PhysicsSolver::Full, "Full Physics"),
    (PhysicsSolver::Simplified, "Simplified"),
    (PhysicsSolver::NoCollision, "Gravity / No Collision"),
    (PhysicsSolver::Custom, "Custom Compute Shader"),
    (PhysicsSolver::Space, "Space"),
];

const MATERIAL_MODELS: [(MaterialModel, &str); 2] =
    [(MaterialModel::Pbr, "PBR"), (MaterialModel::Unlit, "Unlit")];

/// Inspector names of the material texture slots, in [`texture_slots`] order.
pub(super) const TEXTURE_SLOTS: [&str; 5] = [
    "Base Color Map",
    "Normal Map",
    "Metal/Rough Map",
    "Occlusion Map",
    "Emissive Map",
];

pub(super) fn texture_slots(
    material: &mut MaterialAsset,
) -> [&mut Option<Handle<TextureAsset>>; 5] {
    [
        &mut material.base_color_texture,
        &mut material.normal_texture,
        &mut material.metallic_roughness_texture,
        &mut material.occlusion_texture,
        &mut material.emissive_texture,
    ]
}

const RIGID_BODY_KINDS: [(RigidBodyKind, &str); 3] = [
    (RigidBodyKind::Dynamic, "Dynamic"),
    (RigidBodyKind::Kinematic, "Kinematic"),
    (RigidBodyKind::Fixed, "Fixed"),
];

/// Draws settings for the object selected in the Hierarchy.
///
/// The `edited_*` values are temporary copies the caller writes back to the
/// ECS object after drawing; `component_edits`, `add_physics`,
/// `remove_physics`, and `edit_custom_shader` are requests applied later.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_inspector_area(
    ui: &mut egui::Ui,
    world: &World,
    state: &mut EditorState,
    physics_backends: PhysicsBackendStatus,
    edited_transform: &mut Option<Transform>,
    edited_camera: &mut Option<Camera>,
    edited_directional_light: &mut Option<DirectionalLight>,
    edited_point_light: &mut Option<PointLight>,
    edited_spot_light: &mut Option<SpotLight>,
    edited_classes: &mut Option<ObjectClasses>,
    edited_physics: &mut Option<PhysicsBody>,
    edited_rigid_body: &mut Option<RigidBody>,
    edited_collider: &mut Option<Collider>,
    edited_render_bounds: &mut Option<RenderBounds>,
    edited_material: &mut Option<MaterialAsset>,
    asset_request: &mut Option<AssetRequest>,
    registered_names: &[String],
    custom_values: &[(String, String)],
    component_edits: &mut Vec<ComponentEdit>,
    add_physics: &mut bool,
    remove_physics: &mut bool,
    edit_custom_shader: &mut bool,
) {
    let Some(entity) = state.selected else {
        ui.colored_label(
            EditorTheme::TEXT_MUTED,
            "Select an object in the Hierarchy.",
        );
        return;
    };
    if state.selection.len() > 1 {
        ui.colored_label(
            EditorTheme::TEXT_MUTED,
            format!(
                "{} objects selected; editing the active one.",
                state.selection.len()
            ),
        );
    }
    let name = world
        .get::<Name>(entity)
        .map_or_else(|| "Unnamed".to_owned(), |name| name.0.clone());
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(name).strong().size(14.0));
        ui.colored_label(EditorTheme::TEXT_MUTED, format!("{entity:?}"));
    });
    ui.add_space(4.0);

    egui::ScrollArea::vertical()
        .id_salt("inspector_panel_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if let Some(transform) = edited_transform {
                widgets::section(ui, "Transform", false, |ui| {
                    widgets::vec3(ui, "Position", &mut transform.position, 0.1);
                    let mut degrees = transform.rotation.map(f32::to_degrees);
                    if widgets::vec3(ui, "Rotation °", &mut degrees, 0.5) {
                        transform.rotation = degrees.map(f32::to_radians);
                    }
                    widgets::vec3(ui, "Scale", &mut transform.scale, 0.01);
                });
            }
            if let Some(renderer) = world.get::<MeshRenderer>(entity) {
                widgets::section(ui, "Mesh Renderer", false, |ui| {
                    widgets::value(
                        ui,
                        "Mesh",
                        &format!("#{:016x}", renderer.mesh.key()),
                    );
                    let mesh_box = world
                        .get_resource::<crate::assets::AssetServer>()
                        .and_then(|assets| assets.mesh_bounds(renderer.mesh));
                    edit_render_bounds(ui, edited_render_bounds, mesh_box);
                });
            }
            if let Some(material) = edited_material {
                widgets::section(ui, "Material", false, |ui| {
                    draw_material(ui, world, entity, material, asset_request);
                });
            }
            if let Some(camera) = edited_camera {
                widgets::section(ui, "Camera", false, |ui| {
                    draw_camera(ui, camera);
                });
            }
            if let Some(light) = edited_directional_light {
                widgets::section(ui, "Directional Light", false, |ui| {
                    widgets::color(ui, "Color", &mut light.color);
                    widgets::drag(
                        ui,
                        "Illuminance",
                        DragValue::new(&mut light.illuminance)
                            .range(0.0..=1_000_000.0)
                            .speed(100.0)
                            .suffix(" lx"),
                    );
                    widgets::checkbox(ui, "Cast Shadows", &mut light.shadows);
                });
            }
            if let Some(light) = edited_point_light {
                widgets::section(ui, "Point Light", false, |ui| {
                    widgets::color(ui, "Color", &mut light.color);
                    light_power(ui, &mut light.intensity, &mut light.range);
                });
            }
            if let Some(light) = edited_spot_light {
                widgets::section(ui, "Spot Light", false, |ui| {
                    widgets::color(ui, "Color", &mut light.color);
                    light_power(ui, &mut light.intensity, &mut light.range);
                    widgets::angle(
                        ui,
                        "Inner Angle",
                        &mut light.inner_angle,
                        0.1..=179.0,
                    );
                    widgets::angle(
                        ui,
                        "Outer Angle",
                        &mut light.outer_angle,
                        0.1..=179.0,
                    );
                    light.outer_angle =
                        light.outer_angle.max(light.inner_angle);
                });
            }
            if let Some(classes) = edited_classes {
                widgets::section(ui, "Classes", false, |ui| {
                    draw_classes(ui, classes, &mut state.class_draft);
                });
            }
            if edited_physics.is_some() {
                let removed = widgets::section(ui, "Physics", true, |ui| {
                    draw_physics(
                        ui,
                        world,
                        entity,
                        state,
                        physics_backends,
                        custom_values,
                        component_edits,
                        edited_physics,
                        edited_rigid_body,
                        edited_collider,
                        edit_custom_shader,
                    );
                });
                if removed {
                    *remove_physics = true;
                }
            }
            for (name, serialized) in custom_values
                .iter()
                .filter(|(name, _)| !has_dedicated_editor(name))
            {
                let inspector = world
                    .get_resource::<InspectorRegistry>()
                    .and_then(|registry| registry.inspectors.get(name));
                let removed = widgets::section(ui, name, true, |ui| {
                    if let Some(inspector) = inspector {
                        if let Some(value) = inspector(ui, serialized) {
                            component_edits.push(ComponentEdit::Set {
                                entity,
                                name: name.clone(),
                                value,
                            });
                        }
                        return;
                    }
                    match serde_json::from_str::<serde_json::Value>(serialized)
                    {
                        Ok(mut value) => {
                            if json::edit_component(ui, &mut value) {
                                component_edits.push(ComponentEdit::Set {
                                    entity,
                                    name: name.clone(),
                                    value: value.to_string(),
                                });
                            }
                        }
                        Err(error) => {
                            ui.colored_label(
                                EditorTheme::ERROR,
                                format!("Unreadable value: {error}"),
                            );
                        }
                    }
                });
                if removed {
                    component_edits.push(ComponentEdit::Remove {
                        entity,
                        name: name.clone(),
                    });
                }
            }

            ui.add_space(6.0);
            let addable = registered_names
                .iter()
                .filter(|name| {
                    !has_dedicated_editor(name)
                        && !custom_values
                            .iter()
                            .any(|(present, _)| present == *name)
                })
                .collect::<Vec<_>>();
            ui.menu_button("Add Component", |ui| {
                ui.set_min_width(200.0);
                if edited_physics.is_none()
                    && EditorTheme::menu_action(ui, "Physics", true).clicked()
                {
                    *add_physics = true;
                    *edited_physics = Some(PhysicsBody::default());
                    *edited_rigid_body = Some(RigidBody::default());
                    *edited_collider = Some(Collider::default());
                    ui.close_menu();
                }
                if !addable.is_empty() {
                    EditorTheme::menu_section(ui, "GAME COMPONENTS");
                }
                for name in addable {
                    if EditorTheme::menu_action(ui, name, true).clicked() {
                        component_edits.push(ComponentEdit::Add {
                            entity,
                            name: name.clone(),
                        });
                        ui.close_menu();
                    }
                }
            });
        });
}

fn draw_camera(ui: &mut egui::Ui, camera: &mut Camera) {
    widgets::checkbox(ui, "Active", &mut camera.active);
    widgets::drag(ui, "Priority", DragValue::new(&mut camera.priority));
    match &mut camera.projection {
        Projection::Perspective {
            vertical_fov_radians,
            near,
            far,
        } => {
            widgets::angle(
                ui,
                "Vertical FOV",
                vertical_fov_radians,
                10.0..=120.0,
            );
            projection_planes(ui, near, far);
        }
        Projection::Orthographic {
            vertical_size,
            near,
            far,
        } => {
            widgets::drag(
                ui,
                "Vertical Size",
                DragValue::new(vertical_size)
                    .range(0.01..=100_000.0)
                    .speed(0.1),
            );
            projection_planes(ui, near, far);
        }
    }
}

/// Keeps `far` strictly beyond `near` while either is edited.
fn projection_planes(ui: &mut egui::Ui, near: &mut f32, far: &mut f32) {
    widgets::drag(
        ui,
        "Near",
        DragValue::new(near).range(0.001..=1_000.0).speed(0.01),
    );
    *far = (*far).max(*near + 0.001);
    widgets::drag(
        ui,
        "Far",
        DragValue::new(far)
            .range((*near + 0.001)..=1_000_000.0)
            .speed(1.0),
    );
}

fn light_power(ui: &mut egui::Ui, intensity: &mut f32, range: &mut f32) {
    widgets::drag(
        ui,
        "Intensity",
        DragValue::new(intensity)
            .range(0.0..=1_000_000.0)
            .speed(10.0),
    );
    widgets::drag(
        ui,
        "Range",
        DragValue::new(range).range(0.01..=100_000.0).speed(0.1),
    );
}

fn draw_classes(
    ui: &mut egui::Ui,
    classes: &mut ObjectClasses,
    draft: &mut String,
) {
    let mut remove = None;
    for class in &classes.names {
        widgets::property_row(ui, class, |ui| {
            if ui.small_button("Remove").clicked() {
                remove = Some(class.clone());
            }
        });
    }
    if let Some(class) = remove {
        classes.remove(&class);
    }
    widgets::property_row(ui, "New Class", |ui| {
        let add = ui.small_button("Add").clicked();
        ui.add(egui::TextEdit::singleline(draft).desired_width(f32::INFINITY));
        if add && classes.add(draft.clone()) {
            draft.clear();
        }
    });
}

/// Registered components that the Inspector edits in their own section
/// rather than as raw values.
fn has_dedicated_editor(name: &str) -> bool {
    name == PHYSICS_SYNC_COMPONENT || name == AUTO_SIMULATION_COMPONENT
}

#[allow(clippy::too_many_arguments)]
fn draw_physics(
    ui: &mut egui::Ui,
    world: &World,
    entity: Entity,
    state: &EditorState,
    physics_backends: PhysicsBackendStatus,
    custom_values: &[(String, String)],
    component_edits: &mut Vec<ComponentEdit>,
    edited_physics: &mut Option<PhysicsBody>,
    edited_rigid_body: &mut Option<RigidBody>,
    edited_collider: &mut Option<Collider>,
    edit_custom_shader: &mut bool,
) {
    let Some(physics) = edited_physics else {
        return;
    };
    let mut auto = custom_values
        .iter()
        .any(|(name, _)| name == AUTO_SIMULATION_COMPONENT);
    if widgets::checkbox(ui, "Auto CPU/GPU", &mut auto) {
        let name = AUTO_SIMULATION_COMPONENT.to_owned();
        component_edits.push(if auto {
            ComponentEdit::Add { entity, name }
        } else {
            ComponentEdit::Remove { entity, name }
        });
    }
    if auto {
        // The engine owns the class; show what it chose and why.
        let chosen = world
            .get::<AutoSimulation>(entity)
            .and_then(|auto| auto.decision)
            .map_or_else(
                || "Decided when the scene runs".to_owned(),
                |decision| {
                    format!("{:?} ({:?})", decision.class, decision.reason)
                },
            );
        widgets::value(ui, "Simulation", &chosen);
    } else {
        widgets::choice(
            ui,
            "Simulation",
            &mut physics.simulation,
            &SIMULATION_CLASSES,
        );
    }
    let note = |ui: &mut egui::Ui, color, text: &str| {
        widgets::property_row(ui, "", |ui| {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(text).small().color(color),
                )
                .wrap(),
            );
        });
    };
    match physics.simulation {
        SimulationClass::None => {
            note(ui, EditorTheme::TEXT_MUTED, "No physics simulation.");
        }
        SimulationClass::Static => {
            if let Some(body) = edited_rigid_body {
                body.kind = RigidBodyKind::Fixed;
            }
            note(
                ui,
                EditorTheme::TEXT_MUTED,
                "Static collider; never dispatched per frame.",
            );
        }
        SimulationClass::Cpu => {
            if physics_backends.gameplay_available {
                note(
                    ui,
                    EditorTheme::TEXT_MUTED,
                    "CPU-authoritative gameplay physics.",
                );
            } else {
                note(
                    ui,
                    EditorTheme::ERROR,
                    "CPU physics backend is not connected yet.",
                );
            }
        }
        SimulationClass::Gpu => {
            if !physics_backends.gpu_dynamic_available {
                note(
                    ui,
                    EditorTheme::WARNING,
                    "GPU gravity and condition events run in native Play; \
                     editor preview simulation is not active yet.",
                );
            }
            widgets::choice(
                ui,
                "GPU Solver",
                &mut physics.solver,
                &PHYSICS_SOLVERS,
            );
            draw_gpu_sync(ui, world, entity, custom_values, component_edits);
            if physics.solver == PhysicsSolver::Custom {
                let path = physics.custom_shader.get_or_insert_with(|| {
                    format!("{}/shaders/custom_solver.glsl", state.project_root)
                });
                widgets::text(ui, "Shader", path);
                widgets::property_row(ui, "", |ui| {
                    if ui.button("Open in Code Editor").clicked() {
                        *edit_custom_shader = true;
                    }
                });
            }
        }
    }
    if physics.simulation == SimulationClass::None {
        return;
    }
    let body = edited_rigid_body.get_or_insert_with(RigidBody::default);
    if physics.simulation != SimulationClass::Static {
        widgets::choice(ui, "Body", &mut body.kind, &RIGID_BODY_KINDS);
        widgets::drag(
            ui,
            "Mass",
            DragValue::new(&mut body.mass)
                .range(0.001..=1_000_000.0)
                .suffix(" kg"),
        );
        widgets::drag(
            ui,
            "Gravity Scale",
            DragValue::new(&mut body.gravity_scale).range(-100.0..=100.0),
        );
        widgets::vec3(ui, "Velocity", &mut body.linear_velocity, 0.1);
    }
    edit_collider(ui, edited_collider.get_or_insert_with(Collider::default));
}

/// Sync mode choice plus what it costs and how old the last readback is.
fn draw_gpu_sync(
    ui: &mut egui::Ui,
    world: &World,
    entity: Entity,
    custom_values: &[(String, String)],
    component_edits: &mut Vec<ComponentEdit>,
) {
    let mut sync = custom_values
        .iter()
        .find(|(name, _)| name == PHYSICS_SYNC_COMPONENT)
        .and_then(|(_, value)| serde_json::from_str(value).ok())
        .unwrap_or_default();
    if widgets::choice(ui, "Sync", &mut sync, &SYNC_MODES) {
        component_edits.push(ComponentEdit::Set {
            entity,
            name: PHYSICS_SYNC_COMPONENT.to_owned(),
            value: serde_json::to_string(&sync).unwrap_or_default(),
        });
    }
    let cost = match sync {
        PhysicsSyncMode::None => "Nothing read back".to_owned(),
        PhysicsSyncMode::Events => "Only emitted events".to_owned(),
        _ => format!(
            "{} B per tick, 1-3 frames late",
            PhysicsSyncMode::STATE_READBACK_BYTES
        ),
    };
    widgets::value(ui, "Sync Cost", &cost);
    if sync.reads_back_state() {
        let age = match world.get::<GpuStateMirror>(entity) {
            Some(mirror) => {
                let now = world
                    .get_resource::<FrameTime>()
                    .map_or(mirror.tick, |time| time.fixed_tick);
                format!(
                    "tick {} ({} ticks old)",
                    mirror.tick,
                    mirror.age_ticks(now)
                )
            }
            None => "none yet (runs in Play)".to_owned(),
        };
        widgets::value(ui, "Last Readback", &age);
    }
}

/// Draws shape, friction, bounce, and trigger settings for one collider.
/// Render Bounds choice for the culling volume.
#[derive(Clone, Copy, Debug, PartialEq)]
enum BoundsKind {
    /// No override: culling uses the mesh's own box.
    Mesh,
    Box,
    Sphere,
}

/// Returns the override for a newly chosen kind, starting from the mesh box
/// (or the unit cube when the mesh is unknown) so the volume does not jump.
fn bounds_for_kind(
    kind: BoundsKind,
    mesh_box: Option<([f32; 3], [f32; 3])>,
) -> Option<RenderBounds> {
    let (min, max) = mesh_box.unwrap_or(([-0.5; 3], [0.5; 3]));
    match kind {
        BoundsKind::Mesh => None,
        BoundsKind::Box => Some(RenderBounds::Aabb { min, max }),
        BoundsKind::Sphere => {
            let center =
                std::array::from_fn(|axis| (min[axis] + max[axis]) * 0.5);
            let radius = (0..3)
                .map(|axis| (max[axis] - min[axis]).powi(2))
                .sum::<f32>()
                .sqrt()
                * 0.5;
            Some(RenderBounds::Sphere { center, radius })
        }
    }
}

/// Edits the local-space `RenderBounds` override; `None` is the mesh box.
/// Material of the selected Mesh Renderer. The caller writes the copy back,
/// cloning the material first when other renderers share it.
fn draw_material(
    ui: &mut egui::Ui,
    world: &World,
    entity: Entity,
    material: &mut MaterialAsset,
    asset_request: &mut Option<AssetRequest>,
) {
    widgets::property_row(ui, "", |ui| {
        if ui
            .button("New Material")
            .on_hover_text("Give this object its own default material")
            .clicked()
        {
            *asset_request = Some(AssetRequest::NewMaterial);
        }
    });
    widgets::choice(ui, "Model", &mut material.model, &MATERIAL_MODELS);
    let mut alpha = match material.alpha_mode {
        AlphaMode::Opaque => 0,
        AlphaMode::Mask { .. } => 1,
        AlphaMode::Blend => 2,
    };
    if widgets::choice(
        ui,
        "Alpha",
        &mut alpha,
        &[(0, "Opaque"), (1, "Mask"), (2, "Blend")],
    ) {
        material.alpha_mode = match alpha {
            1 => AlphaMode::Mask { cutoff: 0.5 },
            2 => AlphaMode::Blend,
            _ => AlphaMode::Opaque,
        };
    }
    if let AlphaMode::Mask { cutoff } = &mut material.alpha_mode {
        widgets::drag(
            ui,
            "Alpha Cutoff",
            DragValue::new(cutoff).range(0.0..=1.0).speed(0.01),
        );
    }
    let [r, g, b, a] = &mut material.base_color;
    let mut rgb = [*r, *g, *b];
    if widgets::color(ui, "Base Color", &mut rgb) {
        [*r, *g, *b] = rgb;
    }
    widgets::drag(
        ui,
        "Opacity",
        DragValue::new(a).range(0.0..=1.0).speed(0.01),
    );
    widgets::drag(
        ui,
        "Metallic",
        DragValue::new(&mut material.metallic)
            .range(0.0..=1.0)
            .speed(0.01),
    );
    widgets::drag(
        ui,
        "Roughness",
        DragValue::new(&mut material.roughness)
            .range(0.0..=1.0)
            .speed(0.01),
    );
    widgets::color(ui, "Emissive", &mut material.emissive);

    let Some(assets) = world.get_resource::<AssetServer>() else {
        return;
    };
    let mut textures = vec![
        (None, "None".to_owned()),
        (Some(assets.fallback_texture), "White (built-in)".to_owned()),
    ];
    textures.extend(assets.textures.paths().map(|(handle, path)| {
        let name = path.file_name().unwrap_or(path.as_os_str());
        (Some(handle), name.to_string_lossy().into_owned())
    }));
    // ponytail: every image loads as sRGB, like scene loading does; normal
    // and metal/rough maps need a linear load path keyed by color space.
    for (slot, (label, texture)) in TEXTURE_SLOTS
        .iter()
        .zip(texture_slots(material))
        .enumerate()
    {
        let selected = textures
            .iter()
            .find(|(handle, _)| handle == texture)
            .map_or("Unknown", |(_, name)| name.as_str());
        let row = widgets::property_row(ui, label, |ui| {
            let load = ui
                .button("Load…")
                .on_hover_text("Choose an image file for this slot");
            if load.clicked() {
                *asset_request = Some(AssetRequest::LoadMaterialTexture(slot));
            }
            egui::ComboBox::from_id_salt(label)
                .width(ui.available_width())
                .selected_text(selected)
                .show_ui(ui, |ui| {
                    for (handle, name) in &textures {
                        ui.selectable_value(texture, *handle, name);
                    }
                });
            ui.min_rect()
        });
        // Dropping an Assets image on the row loads it into this slot.
        let drop = ui.interact(
            row,
            ui.id().with(("texture_drop", slot)),
            egui::Sense::hover(),
        );
        if let Some(payload) =
            drop.dnd_release_payload::<crate::editor::assets_panel::ImageDrag>()
        {
            *asset_request = Some(AssetRequest::SetMaterialTexture {
                entity,
                slot,
                path: payload.0.clone(),
            });
        }
    }
}

fn edit_render_bounds(
    ui: &mut egui::Ui,
    bounds: &mut Option<RenderBounds>,
    mesh_box: Option<([f32; 3], [f32; 3])>,
) {
    let current = match bounds {
        None => BoundsKind::Mesh,
        Some(RenderBounds::Aabb { .. }) => BoundsKind::Box,
        Some(RenderBounds::Sphere { .. }) => BoundsKind::Sphere,
    };
    let mut kind = current;
    widgets::choice(
        ui,
        "Render Bounds",
        &mut kind,
        &[
            (BoundsKind::Mesh, "Mesh"),
            (BoundsKind::Box, "Box"),
            (BoundsKind::Sphere, "Sphere"),
        ],
    );
    if kind != current {
        *bounds = bounds_for_kind(kind, mesh_box);
    }
    match bounds {
        None => {}
        Some(RenderBounds::Aabb { min, max }) => {
            widgets::vec3(ui, "Min", min, 0.05);
            widgets::vec3(ui, "Max", max, 0.05);
            for axis in 0..3 {
                max[axis] = max[axis].max(min[axis]);
            }
        }
        Some(RenderBounds::Sphere { center, radius }) => {
            widgets::vec3(ui, "Center", center, 0.05);
            widgets::drag(
                ui,
                "Radius",
                DragValue::new(radius).range(0.0..=1_000_000.0).speed(0.05),
            );
        }
    }
}

fn edit_collider(ui: &mut egui::Ui, collider: &mut Collider) {
    #[derive(Clone, Copy, PartialEq)]
    enum Shape {
        Box,
        Sphere,
        Capsule,
        Convex,
        Triangles,
    }
    let current = match collider.shape {
        ColliderShape::Box { .. } => Shape::Box,
        ColliderShape::Sphere { .. } => Shape::Sphere,
        ColliderShape::Capsule { .. } => Shape::Capsule,
        ColliderShape::ConvexMesh => Shape::Convex,
        ColliderShape::TriangleMesh => Shape::Triangles,
    };
    let mut shape = current;
    widgets::choice(
        ui,
        "Collider",
        &mut shape,
        &[
            (Shape::Box, "Box"),
            (Shape::Sphere, "Sphere"),
            (Shape::Capsule, "Capsule"),
            (Shape::Convex, "Convex Mesh"),
            (Shape::Triangles, "Triangle Mesh"),
        ],
    );
    if shape != current {
        collider.shape = match shape {
            Shape::Box => ColliderShape::Box {
                half_extents: [0.5; 3],
            },
            Shape::Sphere => ColliderShape::Sphere { radius: 0.5 },
            Shape::Capsule => ColliderShape::Capsule {
                half_height: 0.5,
                radius: 0.5,
            },
            Shape::Convex => ColliderShape::ConvexMesh,
            Shape::Triangles => ColliderShape::TriangleMesh,
        };
    }
    let size =
        |value| DragValue::new(value).range(0.001..=1_000_000.0).speed(0.05);
    match &mut collider.shape {
        ColliderShape::Box { half_extents } => {
            widgets::vec3(ui, "Half Size", half_extents, 0.05);
            for extent in half_extents {
                *extent = extent.max(0.001);
            }
        }
        ColliderShape::Sphere { radius } => {
            widgets::drag(ui, "Radius", size(radius));
        }
        ColliderShape::Capsule {
            half_height,
            radius,
        } => {
            widgets::drag(ui, "Half Height", size(half_height));
            widgets::drag(ui, "Radius", size(radius));
        }
        ColliderShape::ConvexMesh => {
            widgets::value(ui, "Source", "This mesh (convex hull)");
        }
        ColliderShape::TriangleMesh => {
            widgets::value(ui, "Source", "This mesh (static only)");
        }
    }
    let unit = |value| DragValue::new(value).range(0.0..=1.0).speed(0.01);
    widgets::drag(ui, "Friction", unit(&mut collider.friction));
    widgets::drag(ui, "Bounciness", unit(&mut collider.restitution));
    widgets::checkbox(ui, "Trigger", &mut collider.sensor);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_bounds_kinds_start_from_the_mesh_box() {
        let mesh_box = Some(([0.0, 0.0, 0.0], [2.0, 2.0, 1.0]));
        assert_eq!(bounds_for_kind(BoundsKind::Mesh, mesh_box), None);
        assert_eq!(
            bounds_for_kind(BoundsKind::Box, mesh_box),
            Some(RenderBounds::Aabb {
                min: [0.0; 3],
                max: [2.0, 2.0, 1.0],
            })
        );
        assert_eq!(
            bounds_for_kind(BoundsKind::Sphere, mesh_box),
            Some(RenderBounds::Sphere {
                center: [1.0, 1.0, 0.5],
                radius: 1.5,
            })
        );
        assert_eq!(
            bounds_for_kind(BoundsKind::Box, None),
            Some(RenderBounds::default())
        );
    }
}
