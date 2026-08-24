//! Renderer-facing snapshot extracted from canonical gameplay ECS state.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::{Resource, World};

use crate::assets::{Handle, MaterialAsset, MeshAsset};

use super::{
    AmbientLight, App, AppError, Camera, DirectionalLight, GlobalTransform,
    MeshRenderer, Plugin, PointLight, Projection, ScheduleStage, Visibility,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExtractedRenderable {
    pub entity: Entity,
    pub transform: GlobalTransform,
    pub mesh: Handle<MeshAsset>,
    pub material: Handle<MaterialAsset>,
    pub cast_shadows: bool,
    pub receive_shadows: bool,
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
    pub active_camera: Option<ExtractedCamera>,
    pub directional_lights: Vec<ExtractedDirectionalLight>,
    pub point_lights: Vec<ExtractedPointLight>,
    pub ambient_light: Option<AmbientLight>,
    pub dirty_ranges: Vec<Range<usize>>,
    pub report: ExtractionReport,
    cached: HashMap<Entity, ExtractedRenderable>,
    previous_order: Vec<Entity>,
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
    let renderables = collect_renderables(world);
    let active_camera = collect_active_camera(world);
    let directional_lights = collect_directional_lights(world);
    let point_lights = collect_point_lights(world);
    let ambient_light = collect_ambient_light(world);

    let mut render_world = world.resource_mut::<RenderWorld>();
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
    let order = renderables
        .iter()
        .map(|renderable| renderable.entity)
        .collect::<Vec<_>>();
    let dirty_ranges = if order != render_world.previous_order {
        (!renderables.is_empty())
            .then_some(0..renderables.len())
            .into_iter()
            .collect()
    } else {
        contiguous_ranges(renderables.iter().enumerate().filter_map(
            |(index, renderable)| {
                dirty_entities.contains(&renderable.entity).then_some(index)
            },
        ))
    };

    render_world.cached = renderables
        .iter()
        .copied()
        .map(|renderable| (renderable.entity, renderable))
        .collect();
    render_world.previous_order = order;
    render_world.report = ExtractionReport {
        added,
        changed,
        removed,
        total: renderables.len(),
    };
    render_world.renderables = renderables;
    render_world.active_camera = active_camera;
    render_world.directional_lights = directional_lights;
    render_world.point_lights = point_lights;
    render_world.ambient_light = ambient_light;
    render_world.dirty_ranges = dirty_ranges;
}

fn collect_renderables(world: &mut World) -> Vec<ExtractedRenderable> {
    let mut query = world.query::<(
        Entity,
        &GlobalTransform,
        &MeshRenderer,
        Option<&Visibility>,
    )>();
    let mut renderables = query
        .iter(world)
        .filter(|(_, _, _, visibility)| {
            visibility.is_none_or(|visibility| visibility.visible)
        })
        .map(|(entity, transform, renderer, _)| ExtractedRenderable {
            entity,
            transform: *transform,
            mesh: renderer.mesh,
            material: renderer.material,
            cast_shadows: renderer.cast_shadows,
            receive_shadows: renderer.receive_shadows,
        })
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

fn collect_ambient_light(world: &mut World) -> Option<AmbientLight> {
    let mut query = world.query::<(Entity, &AmbientLight)>();
    query
        .iter(world)
        .min_by_key(|(entity, _)| entity.to_bits())
        .map(|(_, light)| *light)
}

fn contiguous_ranges(
    indices: impl Iterator<Item = usize>,
) -> Vec<Range<usize>> {
    let mut ranges: Vec<Range<usize>> = Vec::new();
    for index in indices {
        match ranges.last_mut() {
            Some(range) if range.end == index => range.end += 1,
            _ => ranges.push(index..index + 1),
        }
    }
    ranges
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
        assert_eq!(render_world.dirty_ranges, vec![0..2]);

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
        assert_eq!(render_world.dirty_ranges, vec![0..1]);
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
}
