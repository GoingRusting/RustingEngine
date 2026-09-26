//! Renderer-facing snapshot extracted from canonical gameplay ECS state.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use bevy_ecs::change_detection::{DetectChanges, Tick};
use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::{Component, Or, Ref, Resource, With, World};

use crate::assets::{Handle, MaterialAsset, MeshAsset};

use super::{
    AmbientLight, App, AppError, Camera, DirectionalLight, GlobalTransform,
    MeshRenderer, Plugin, PointLight, Projection, RenderBounds, ScheduleStage,
    SkyLight, SpotLight, ToneMapping, Visibility,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExtractedRenderable {
    pub entity: Entity,
    pub transform: GlobalTransform,
    pub mesh: Handle<MeshAsset>,
    pub material: Handle<MaterialAsset>,
    pub cast_shadows: bool,
    pub receive_shadows: bool,
    /// The entity's local-space `RenderBounds` override. Without one the
    /// renderer culls with the box around the mesh it actually draws, so
    /// bounds follow mesh loads and reloads.
    pub bounds: Option<RenderBounds>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExtractedCamera {
    pub entity: Entity,
    pub transform: GlobalTransform,
    pub projection: Projection,
    pub priority: i32,
}

/// Optional camera selected by a tool such as the editor Scene viewport.
/// Runtime Game views leave this empty and use the highest-priority active
/// gameplay camera.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderCameraOverride {
    pub entity: Option<Entity>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExtractedDirectionalLight {
    pub entity: Entity,
    pub transform: GlobalTransform,
    pub light: DirectionalLight,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExtractedPointLight {
    pub entity: Entity,
    pub transform: GlobalTransform,
    pub light: PointLight,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExtractedSpotLight {
    pub entity: Entity,
    pub transform: GlobalTransform,
    pub light: SpotLight,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExtractionReport {
    pub added: usize,
    pub changed: usize,
    pub removed: usize,
    pub total: usize,
}

/// Data consumed by the renderer, separate from the gameplay world.
#[derive(Resource, Default)]
pub struct RenderWorld {
    pub renderables: Vec<ExtractedRenderable>,
    /// Changes only when the extracted object list or one of its transforms
    /// changes. The renderer uses this instead of comparing every object.
    pub renderables_revision: u64,
    pub active_camera: Option<ExtractedCamera>,
    pub directional_lights: Vec<ExtractedDirectionalLight>,
    pub point_lights: Vec<ExtractedPointLight>,
    pub spot_lights: Vec<ExtractedSpotLight>,
    pub ambient_light: Option<AmbientLight>,
    pub sky_light: Option<SkyLight>,
    pub tone_mapping: Option<ToneMapping>,
    pub lights_revision: u64,
    pub report: ExtractionReport,
    /// Bodies whose newest runtime transforms will be owned by GPU compute.
    pub gpu_physics: Vec<super::ExtractedGpuPhysicsBody>,
    /// Signature of a rule-free GPU body set reused between render frames.
    pub gpu_physics_signature: Option<u64>,
    /// Changes only when CPU data used to create GPU physics buffers changes.
    pub gpu_physics_revision: u64,
    /// Commands taken from [`super::GpuPhysicsCommands`] by the last
    /// extraction that found any.
    pub gpu_physics_commands: Vec<(super::PhysicsId, super::GpuBodyCommand)>,
    /// Whether the batch asks for every body's state; see
    /// [`super::GpuPhysicsCommands::read_all_states`].
    pub gpu_physics_read_all: bool,
    /// Whether the batch restarts GPU bodies from their authored state; see
    /// [`super::GpuPhysicsCommands::reset_to_authored`].
    pub gpu_physics_reset: bool,
    /// Bumps with each new batch in `gpu_physics_commands`, so a renderer
    /// that draws twice without an extraction does not apply it twice.
    pub gpu_physics_commands_serial: u64,
    /// Solid CPU and static colliders from the last CPU physics step, for
    /// GPU bodies to collide against.
    pub gpu_colliders: Vec<super::GpuCollider>,
    /// Resolved [`super::GpuConditionShader`] sources, in dispatch order.
    pub gpu_condition_shaders: Vec<String>,
    /// Wrapped `PhysicsSolver::Custom` hook files, one per distinct path,
    /// dispatched before the condition shaders. Refreshed only when the GPU
    /// bodies are re-extracted.
    pub gpu_solver_shaders: Vec<String>,
    pub physics_tick: u64,
    pub fixed_delta_seconds: f32,
    pub elapsed_seconds: f32,
    pub physics_gravity: [f32; 3],
    pub physics_enabled: bool,
    pub background_color: [f32; 4],
    /// Requested profile; the renderer resolves `Auto` from device capabilities.
    pub quality: super::QualityProfile,
    pub culling: super::CullingMode,
    cached: HashMap<Entity, ExtractedRenderable>,
    renderables_signature: Option<u64>,
    /// Tick and component counts seen by the last signature hash.
    renderables_fingerprint: Option<(Tick, [usize; 5])>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RenderExtractPlugin;

impl Plugin for RenderExtractPlugin {
    fn build(&self, app: &mut App) -> Result<(), AppError> {
        app.insert_resource(RenderWorld::default())
            .insert_resource(RenderCameraOverride::default())
            .add_systems(ScheduleStage::RenderExtract, extract_render_world);
        Ok(())
    }
}

pub fn extract_render_world(world: &mut World) {
    // Most game frames do not add objects or change their CPU transforms.
    // Hashing in place is much cheaper than allocating and sorting a new list
    // of ten thousand objects only to discover that nothing changed.
    let previous_renderables_signature =
        world.resource::<RenderWorld>().renderables_signature;
    let renderables_signature = if renderables_changed(world) {
        Some(renderables_signature(world))
    } else {
        previous_renderables_signature
    };
    let renderables = (previous_renderables_signature != renderables_signature)
        .then(|| collect_renderables(world));
    let active_camera = collect_active_camera(world);
    let directional_lights = collect_directional_lights(world);
    let point_lights = collect_point_lights(world);
    let spot_lights = collect_spot_lights(world);
    let ambient_light = collect_first::<AmbientLight>(world);
    let sky_light = collect_first::<SkyLight>(world);
    let tone_mapping = collect_first::<ToneMapping>(world);
    let has_gpu_physics_resources = world
        .contains_resource::<super::PhysicsIdRegistry>()
        && world.contains_resource::<super::GpuEventRegistry>()
        && world.contains_resource::<super::GpuPhysicsClassWatches>();
    let commands = world
        .get_resource_mut::<super::GpuPhysicsCommands>()
        .map(|mut commands| std::mem::take(&mut *commands))
        .unwrap_or_default();
    // ponytail: resolves every frame; a handful of short strings. Track a
    // revision on the resource if scenes ever carry many large shaders.
    let condition_shaders = world
        .get_resource::<super::GpuConditionShaders>()
        .map(|shaders| shaders.0.clone())
        .unwrap_or_default();
    let condition_shaders =
        match world.get_resource_mut::<super::GpuEventRegistry>() {
            Some(mut registry) => condition_shaders
                .iter()
                .map(|shader| shader.resolve(&mut registry))
                .collect(),
            None => Vec::new(),
        };
    let gpu_physics_signature = has_gpu_physics_resources
        .then(|| super::hybrid_physics::simple_gpu_physics_signature(world));
    let previous_gpu_signature =
        world.resource::<RenderWorld>().gpu_physics_signature;
    let gpu_physics = if gpu_physics_signature.is_some()
        && gpu_physics_signature == previous_gpu_signature
    {
        None
    } else if has_gpu_physics_resources {
        Some(super::hybrid_physics::extract_gpu_physics_bodies(world))
    } else {
        Some(Vec::new())
    };
    // ponytail: rebuilt every frame from the CPU step; a revision would skip
    // the upload for static-only scenes if colliders ever number thousands.
    let gpu_colliders = if gpu_physics.as_ref().is_some_and(Vec::is_empty) {
        Vec::new()
    } else {
        world
            .get_resource::<super::PhysicsWorld>()
            .map(super::PhysicsWorld::gpu_colliders)
            .unwrap_or_default()
    };
    let time = *world.resource::<super::FrameTime>();
    let physics_settings = world.resource::<super::PhysicsSettings>().clone();
    let render_settings = world.resource::<super::RenderSettings>();
    let (background_color, quality, culling) = (
        render_settings.background_color,
        render_settings.quality,
        render_settings.culling,
    );

    let mut render_world = world.resource_mut::<RenderWorld>();
    match renderables {
        None => {
            // GPU-owned effects usually leave their canonical ECS transforms
            // unchanged. Avoid rebuilding three large hash collections when the
            // extracted render list is byte-for-byte identical to last frame.
            render_world.report = ExtractionReport {
                total: render_world.renderables.len(),
                ..ExtractionReport::default()
            };
        }
        Some(renderables) => {
            let current_entities = renderables
                .iter()
                .map(|renderable| renderable.entity)
                .collect::<HashSet<_>>();
            let removed = render_world
                .cached
                .keys()
                .filter(|entity| !current_entities.contains(entity))
                .count();
            let mut added = 0;
            let mut dirty_entities = HashSet::new();
            for renderable in &renderables {
                match render_world.cached.get(&renderable.entity) {
                    None => {
                        added += 1;
                        dirty_entities.insert(renderable.entity);
                    }
                    Some(previous) if previous != renderable => {
                        dirty_entities.insert(renderable.entity);
                    }
                    Some(_) => {}
                }
            }
            let changed = dirty_entities.len().saturating_sub(added);
            render_world.cached = renderables
                .iter()
                .copied()
                .map(|renderable| (renderable.entity, renderable))
                .collect();
            render_world.report = ExtractionReport {
                added,
                changed,
                removed,
                total: renderables.len(),
            };
            render_world.renderables = renderables;
            render_world.renderables_revision =
                render_world.renderables_revision.wrapping_add(1);
        }
    }
    render_world.renderables_signature = renderables_signature;
    render_world.active_camera = active_camera;
    render_world.tone_mapping = tone_mapping;
    if render_world.directional_lights != directional_lights
        || render_world.point_lights != point_lights
        || render_world.spot_lights != spot_lights
        || render_world.ambient_light != ambient_light
        || render_world.sky_light != sky_light
    {
        render_world.lights_revision =
            render_world.lights_revision.wrapping_add(1);
        render_world.directional_lights = directional_lights;
        render_world.point_lights = point_lights;
        render_world.spot_lights = spot_lights;
        render_world.ambient_light = ambient_light;
        render_world.sky_light = sky_light;
    }
    // A changed signature can still extract identical bodies (a tick bumped
    // without an edit). Bump the revision only on a real change; each bump
    // makes the renderer rebuild its physics tables.
    if let Some(gpu_physics) =
        gpu_physics.filter(|bodies| *bodies != render_world.gpu_physics)
    {
        render_world.gpu_solver_shaders = custom_solver_shaders(&gpu_physics);
        render_world.gpu_physics = gpu_physics;
        render_world.gpu_physics_revision =
            render_world.gpu_physics_revision.wrapping_add(1);
    }
    render_world.gpu_physics_signature = gpu_physics_signature;
    if !commands.commands.is_empty()
        || commands.read_all_states
        || commands.reset_to_authored
    {
        render_world.gpu_physics_commands = commands.commands;
        render_world.gpu_physics_read_all = commands.read_all_states;
        render_world.gpu_physics_reset = commands.reset_to_authored;
        render_world.gpu_physics_commands_serial += 1;
    }
    render_world.gpu_condition_shaders = condition_shaders;
    render_world.gpu_colliders = gpu_colliders;
    render_world.physics_tick = time.fixed_tick;
    render_world.fixed_delta_seconds = time.fixed_delta.as_secs_f32();
    render_world.elapsed_seconds = time.elapsed.as_secs_f32();
    render_world.physics_gravity = physics_settings.gravity;
    render_world.physics_enabled = physics_settings.enabled;
    render_world.background_color = background_color;
    render_world.quality = quality;
    render_world.culling = culling;
}

/// True unless no `GlobalTransform`, `MeshRenderer`, `Visibility`, or
/// `Parent` changed since the last call and none was removed. Removing one
/// lowers a count; adding one back is itself a change. A static scene then
/// skips the per-object hash and parent-chain walks.
fn renderables_changed(world: &mut World) -> bool {
    let this_run = world.increment_change_tick();
    let last_run = world
        .resource::<RenderWorld>()
        .renderables_fingerprint
        .map(|(tick, _)| tick);
    let newer = |tick: Tick| {
        last_run.is_none_or(|last_run| tick.is_newer_than(last_run, this_run))
    };
    let mut counts = [0; 5];
    let mut changed = false;
    let mut query = world.query_filtered::<(
        Option<Ref<GlobalTransform>>,
        Option<Ref<MeshRenderer>>,
        Option<Ref<Visibility>>,
        Option<Ref<super::Parent>>,
        Option<Ref<RenderBounds>>,
    ), Or<(
        With<MeshRenderer>,
        With<Visibility>,
        With<super::Parent>,
        With<RenderBounds>,
    )>>();
    for (transform, renderer, visibility, parent, bounds) in query.iter(world) {
        let ticks = [
            transform.map(|value| value.last_changed()),
            renderer.map(|value| value.last_changed()),
            visibility.map(|value| value.last_changed()),
            parent.map(|value| value.last_changed()),
            bounds.map(|value| value.last_changed()),
        ];
        for (count, tick) in counts.iter_mut().zip(ticks) {
            if let Some(tick) = tick {
                *count += 1;
                changed |= newer(tick);
            }
        }
    }
    let mut render_world = world.resource_mut::<RenderWorld>();
    changed |= render_world
        .renderables_fingerprint
        .is_none_or(|(_, previous)| previous != counts);
    render_world.renderables_fingerprint = Some((this_run, counts));
    changed
}

/// Creates a small fingerprint without allocating or sorting render objects.
fn renderables_signature(world: &mut World) -> u64 {
    let mut hasher = super::FastHasher::default();
    let mut count = 0_u64;
    let mut query = world.query::<(
        Entity,
        &GlobalTransform,
        &MeshRenderer,
        Option<&RenderBounds>,
    )>();
    let world = &*world;
    for (entity, transform, renderer, bounds) in query.iter(world) {
        if !visible_in_hierarchy(world, entity) {
            continue;
        }
        count += 1;
        entity.to_bits().hash(&mut hasher);
        renderer.mesh.key().hash(&mut hasher);
        renderer.material.key().hash(&mut hasher);
        renderer.cast_shadows.hash(&mut hasher);
        renderer.receive_shadows.hash(&mut hasher);
        format!("{bounds:?}").hash(&mut hasher);
        for row in transform.matrix {
            for value in row {
                value.to_bits().hash(&mut hasher);
            }
        }
    }
    count.hash(&mut hasher);
    hasher.finish()
}

/// An entity renders only when it and every ancestor are visible, like
/// Blender's and Godot's hide-with-parent behavior.
// ponytail: walks the parent chain per renderable; cache per frame if deep
// hierarchies show up in extraction profiles.
pub fn visible_in_hierarchy(world: &World, entity: Entity) -> bool {
    let mut current = Some(entity);
    // The step limit guards against a damaged scene with a parent cycle.
    for _ in 0..1024 {
        let Some(entity) = current else {
            return true;
        };
        if world
            .get::<Visibility>(entity)
            .is_some_and(|visibility| !visibility.visible)
        {
            return false;
        }
        current = world.get::<super::Parent>(entity).map(|parent| parent.0);
    }
    true
}

fn collect_renderables(world: &mut World) -> Vec<ExtractedRenderable> {
    let mut query = world.query::<(
        Entity,
        &GlobalTransform,
        &MeshRenderer,
        Option<&RenderBounds>,
    )>();
    let world_ref = &*world;
    let mut renderables = query
        .iter(world_ref)
        .filter(|(entity, ..)| visible_in_hierarchy(world_ref, *entity))
        .map(
            |(entity, transform, renderer, bounds)| ExtractedRenderable {
                entity,
                transform: *transform,
                mesh: renderer.mesh,
                material: renderer.material,
                cast_shadows: renderer.cast_shadows,
                receive_shadows: renderer.receive_shadows,
                bounds: bounds.copied(),
            },
        )
        .collect::<Vec<_>>();
    renderables.sort_by_key(|renderable| {
        (
            renderable.mesh.key(),
            renderable.material.key(),
            renderable.entity.to_bits(),
        )
    });
    renderables
}

fn collect_active_camera(world: &mut World) -> Option<ExtractedCamera> {
    let override_entity = world.resource::<RenderCameraOverride>().entity;
    let mut query = world.query::<(Entity, &GlobalTransform, &Camera)>();
    if let Some(entity) = override_entity {
        if let Ok((entity, transform, camera)) = query.get(world, entity) {
            return Some(ExtractedCamera {
                entity,
                transform: *transform,
                projection: camera.projection,
                priority: camera.priority,
            });
        }
    }
    query
        .iter(world)
        .filter(|(_, _, camera)| camera.active)
        .map(|(entity, transform, camera)| ExtractedCamera {
            entity,
            transform: *transform,
            projection: camera.projection,
            priority: camera.priority,
        })
        .max_by_key(|camera| {
            (camera.priority, std::cmp::Reverse(camera.entity.to_bits()))
        })
}

fn collect_directional_lights(
    world: &mut World,
) -> Vec<ExtractedDirectionalLight> {
    let mut query =
        world.query::<(Entity, &GlobalTransform, &DirectionalLight)>();
    let mut lights = query
        .iter(world)
        .map(|(entity, transform, light)| ExtractedDirectionalLight {
            entity,
            transform: *transform,
            light: *light,
        })
        .collect::<Vec<_>>();
    lights.sort_by_key(|light| light.entity.to_bits());
    lights
}

fn collect_point_lights(world: &mut World) -> Vec<ExtractedPointLight> {
    let mut query = world.query::<(Entity, &GlobalTransform, &PointLight)>();
    let mut lights = query
        .iter(world)
        .map(|(entity, transform, light)| ExtractedPointLight {
            entity,
            transform: *transform,
            light: *light,
        })
        .collect::<Vec<_>>();
    lights.sort_by_key(|light| light.entity.to_bits());
    lights
}

fn collect_spot_lights(world: &mut World) -> Vec<ExtractedSpotLight> {
    let mut query = world.query::<(Entity, &GlobalTransform, &SpotLight)>();
    let mut lights = query
        .iter(world)
        .map(|(entity, transform, light)| ExtractedSpotLight {
            entity,
            transform: *transform,
            light: *light,
        })
        .collect::<Vec<_>>();
    lights.sort_by_key(|light| light.entity.to_bits());
    lights
}

/// The environment component on the lowest entity, so the pick is stable.
fn collect_first<T: Component + Copy>(world: &mut World) -> Option<T> {
    let mut query = world.query::<(Entity, &T)>();
    query
        .iter(world)
        .min_by_key(|(entity, _)| entity.to_bits())
        .map(|(_, light)| *light)
}

/// Reads each distinct custom solver file once, in path order. A file that
/// cannot be read becomes a shader that fails to compile with the reason, so
/// it shows up in `SceneRenderer::condition_shader_errors`.
// ponytail: files are read only when GPU bodies are re-extracted, so edits
// need a scene change or restart; watch the files if hot reload is wanted.
fn custom_solver_shaders(
    bodies: &[super::ExtractedGpuPhysicsBody],
) -> Vec<String> {
    let paths = bodies
        .iter()
        .filter_map(|body| body.custom_shader.as_deref())
        .collect::<std::collections::BTreeSet<_>>();
    paths
        .into_iter()
        .map(|path| {
            let glsl = std::fs::read_to_string(path).unwrap_or_else(|error| {
                format!("#error cannot read custom solver {path}: {error}")
            });
            super::custom_solver_source(path, &glsl)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::assets::AssetServer;
    use crate::Transform;

    use super::*;

    fn renderer(server: &AssetServer) -> MeshRenderer {
        MeshRenderer {
            mesh: server.fallback_mesh,
            material: server.fallback_material,
            cast_shadows: true,
            receive_shadows: true,
        }
    }

    #[test]
    fn extraction_tracks_changes_removals_and_stable_order() {
        let mut app = App::new();
        app.add_plugin(RenderExtractPlugin).unwrap();
        let server = AssetServer::default();
        let renderer = renderer(&server);
        app.insert_resource(server);
        let first = app.spawn((Transform::new([1.0, 0.0, 0.0]), renderer));
        let second = app.spawn((Transform::new([2.0, 0.0, 0.0]), renderer));

        app.update(Duration::ZERO).unwrap();
        let render_world = app.world().resource::<RenderWorld>();
        assert_eq!(render_world.report.added, 2);

        app.world_mut()
            .get_mut::<Transform>(second)
            .unwrap()
            .position[0] = 3.0;
        app.update(Duration::ZERO).unwrap();
        assert_eq!(app.world().resource::<RenderWorld>().report.changed, 1);

        app.despawn(first).unwrap();
        app.update(Duration::ZERO).unwrap();
        let render_world = app.world().resource::<RenderWorld>();
        assert_eq!(render_world.report.removed, 1);
        assert_eq!(render_world.report.total, 1);
    }

    #[test]
    fn extraction_selects_highest_priority_active_camera() {
        let mut app = App::new();
        app.add_plugin(RenderExtractPlugin).unwrap();
        app.spawn((
            Transform::default(),
            Camera {
                active: true,
                priority: 1,
                ..Camera::default()
            },
        ));
        let expected = app.spawn((
            Transform::default(),
            Camera {
                active: true,
                priority: 10,
                ..Camera::default()
            },
        ));

        app.update(Duration::ZERO).unwrap();
        assert_eq!(
            app.world()
                .resource::<RenderWorld>()
                .active_camera
                .map(|camera| camera.entity),
            Some(expected)
        );
    }

    #[test]
    fn camera_override_can_select_an_inactive_editor_camera() {
        let mut app = App::new();
        app.add_plugin(RenderExtractPlugin).unwrap();
        app.spawn((
            Transform::default(),
            Camera {
                active: true,
                priority: 10,
                ..Camera::default()
            },
        ));
        let editor_camera = app.spawn((
            Transform::new([0.0, 3.0, 8.0]),
            Camera {
                active: false,
                ..Camera::default()
            },
        ));
        app.world_mut()
            .resource_mut::<RenderCameraOverride>()
            .entity = Some(editor_camera);

        app.update(Duration::ZERO).unwrap();
        assert_eq!(
            app.world()
                .resource::<RenderWorld>()
                .active_camera
                .map(|camera| camera.entity),
            Some(editor_camera)
        );
    }

    #[test]
    fn hidden_entities_are_removed_from_render_world() {
        let mut app = App::new();
        app.add_plugin(RenderExtractPlugin).unwrap();
        let server = AssetServer::default();
        let renderer = renderer(&server);
        app.insert_resource(server);
        let entity =
            app.spawn((Transform::default(), renderer, Visibility::default()));
        app.update(Duration::ZERO).unwrap();
        assert_eq!(app.world().resource::<RenderWorld>().report.total, 1);

        app.world_mut()
            .get_mut::<Visibility>(entity)
            .unwrap()
            .visible = false;
        app.update(Duration::ZERO).unwrap();
        let render_world = app.world().resource::<RenderWorld>();
        assert_eq!(render_world.report.total, 0);
        assert_eq!(render_world.report.removed, 1);
    }

    #[test]
    fn removing_a_hidden_visibility_after_a_quiet_frame_shows_the_object() {
        let mut app = App::new();
        app.add_plugin(RenderExtractPlugin).unwrap();
        let server = AssetServer::default();
        let renderer = renderer(&server);
        app.insert_resource(server);
        let entity = app.spawn((
            Transform::default(),
            renderer,
            Visibility { visible: false },
        ));
        app.update(Duration::ZERO).unwrap();
        app.update(Duration::ZERO).unwrap();
        assert_eq!(app.world().resource::<RenderWorld>().report.total, 0);

        // A removal changes no remaining component; only the count drops.
        app.world_mut().entity_mut(entity).remove::<Visibility>();
        app.update(Duration::ZERO).unwrap();
        assert_eq!(app.world().resource::<RenderWorld>().report.total, 1);
    }

    #[test]
    fn render_bounds_overrides_are_extracted_and_track_edits() {
        let mut app = App::new();
        app.add_plugin(RenderExtractPlugin).unwrap();
        let server = AssetServer::default();
        let renderer = renderer(&server);
        app.insert_resource(server);
        let entity = app.spawn((
            Transform::new([5.0, 0.0, 0.0]),
            renderer,
            RenderBounds::Sphere {
                center: [0.0, 1.0, 0.0],
                radius: 2.0,
            },
        ));
        let bounds = |app: &App| {
            app.world().resource::<RenderWorld>().renderables[0].bounds
        };
        app.update(Duration::ZERO).unwrap();
        assert_eq!(
            bounds(&app),
            Some(RenderBounds::Sphere {
                center: [0.0, 1.0, 0.0],
                radius: 2.0,
            }),
            "the override stays in local space"
        );

        let edited = RenderBounds::Aabb {
            min: [-1.0; 3],
            max: [1.0; 3],
        };
        *app.world_mut().get_mut::<RenderBounds>(entity).unwrap() = edited;
        app.update(Duration::ZERO).unwrap();
        assert_eq!(app.world().resource::<RenderWorld>().report.changed, 1);
        assert_eq!(bounds(&app), Some(edited));

        app.world_mut().entity_mut(entity).remove::<RenderBounds>();
        app.update(Duration::ZERO).unwrap();
        assert_eq!(bounds(&app), None, "the renderer uses the mesh box");
    }

    #[test]
    fn hiding_a_parent_hides_its_children() {
        let mut app = App::new();
        app.add_plugin(RenderExtractPlugin).unwrap();
        let server = AssetServer::default();
        let renderer = renderer(&server);
        app.insert_resource(server);
        let parent = app.spawn((Transform::default(), Visibility::default()));
        app.spawn((
            Transform::default(),
            renderer,
            super::super::Parent(parent),
        ));
        app.update(Duration::ZERO).unwrap();
        assert_eq!(app.world().resource::<RenderWorld>().report.total, 1);

        app.world_mut()
            .get_mut::<Visibility>(parent)
            .unwrap()
            .visible = false;
        app.update(Duration::ZERO).unwrap();
        assert_eq!(app.world().resource::<RenderWorld>().report.total, 0);
    }
}
