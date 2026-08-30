//! Scene View object picking.
//!
//! Picking belongs to the editor because it is an authoring action. It uses
//! render meshes, not physics colliders, so an object is selectable even when
//! it has no physics component.

use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::World;
use egui::{Pos2, Rect};
use nalgebra::{Matrix4, Orthographic3, Perspective3, Vector3, Vector4};

use crate::assets::{AssetServer, MeshAsset};
use crate::runtime::{Camera, GlobalTransform, MeshRenderer, Projection};

/// A world-space ray produced by one Scene View mouse click.
#[derive(Clone, Copy, Debug)]
pub(super) struct Ray {
    pub origin: Vector3<f32>,
    pub direction: Vector3<f32>,
}

/// Finds the nearest renderable mesh under a Scene View click.
pub(super) fn pick_entity(
    world: &mut World,
    camera_entity: Entity,
    click: Pos2,
    viewport: Rect,
) -> Option<Entity> {
    let camera = world.get::<Camera>(camera_entity).copied()?;
    let camera_transform = *world.get::<GlobalTransform>(camera_entity)?;
    let ray = scene_ray(click, viewport, camera, camera_transform)?;

    // Collect ECS references first. This ends the mutable query borrow before
    // looking up meshes in AssetServer.
    let candidates = {
        let mut query =
            world.query::<(Entity, &MeshRenderer, &GlobalTransform)>();
        query
            .iter(world)
            .map(|(entity, mesh, transform)| (entity, mesh.mesh, *transform))
            .collect::<Vec<_>>()
    };
    let assets = world.resource::<AssetServer>();
    candidates
        .into_iter()
        .filter_map(|(entity, handle, transform)| {
            let mesh = assets.meshes.get(handle)?;
            let distance = ray_mesh_bounds(ray, transform, mesh)?;
            Some((distance, entity))
        })
        .min_by(|left, right| left.0.total_cmp(&right.0))
        .map(|(_, entity)| entity)
}

/// Builds a ray by unprojecting Vulkan near and far depth points.
pub(super) fn scene_ray(
    click: Pos2,
    viewport: Rect,
    camera: Camera,
    camera_transform: GlobalTransform,
) -> Option<Ray> {
    let size = viewport.size();
    if size.x <= 0.0 || size.y <= 0.0 {
        return None;
    }
    let relative = click - viewport.min;
    let ndc_x = relative.x / size.x * 2.0 - 1.0;
    // Vulkan viewport coordinates have their top edge at NDC Y = -1.
    let ndc_y = relative.y / size.y * 2.0 - 1.0;
    let aspect = size.x / size.y;
    let projection = match camera.projection {
        Projection::Perspective {
            vertical_fov_radians,
            near,
            far,
        } => Perspective3::new(aspect, vertical_fov_radians, near, far)
            .to_homogeneous(),
        Projection::Orthographic {
            vertical_size,
            near,
            far,
        } => Orthographic3::new(
            -vertical_size * aspect * 0.5,
            vertical_size * aspect * 0.5,
            -vertical_size * 0.5,
            vertical_size * 0.5,
            near,
            far,
        )
        .to_homogeneous(),
    };
    let world_from_camera = matrix_from_array(camera_transform.matrix);
    let view = world_from_camera.try_inverse()?;
    let inverse =
        (vulkan_clip_correction() * projection * view).try_inverse()?;
    let near = inverse * Vector4::new(ndc_x, ndc_y, 0.0, 1.0);
    let far = inverse * Vector4::new(ndc_x, ndc_y, 1.0, 1.0);
    let near = Vector3::new(near.x / near.w, near.y / near.w, near.z / near.w);
    let far = Vector3::new(far.x / far.w, far.y / far.w, far.z / far.w);
    let direction = (far - near).try_normalize(f32::EPSILON)?;
    Some(Ray {
        origin: near,
        direction,
    })
}

/// Projects a world point into the egui Scene View rectangle.
pub(super) fn project_world_to_screen(
    world: &World,
    camera_entity: Entity,
    point: [f32; 3],
    viewport: Rect,
) -> Option<Pos2> {
    let camera = *world.get::<Camera>(camera_entity)?;
    let camera_transform = *world.get::<GlobalTransform>(camera_entity)?;
    let size = viewport.size();
    if size.x <= 0.0 || size.y <= 0.0 {
        return None;
    }
    let aspect = size.x / size.y;
    let projection = match camera.projection {
        Projection::Perspective {
            vertical_fov_radians,
            near,
            far,
        } => Perspective3::new(aspect, vertical_fov_radians, near, far)
            .to_homogeneous(),
        Projection::Orthographic {
            vertical_size,
            near,
            far,
        } => Orthographic3::new(
            -vertical_size * aspect * 0.5,
            vertical_size * aspect * 0.5,
            -vertical_size * 0.5,
            vertical_size * 0.5,
            near,
            far,
        )
        .to_homogeneous(),
    };
    let view = matrix_from_array(camera_transform.matrix).try_inverse()?;
    let clip = vulkan_clip_correction()
        * projection
        * view
        * Vector4::new(point[0], point[1], point[2], 1.0);
    if clip.w <= f32::EPSILON {
        return None;
    }
    let ndc = [clip.x / clip.w, clip.y / clip.w];
    Some(Pos2::new(
        viewport.left() + (ndc[0] + 1.0) * 0.5 * size.x,
        viewport.top() + (ndc[1] + 1.0) * 0.5 * size.y,
    ))
}

/// Intersects a ray against one mesh's local-space axis-aligned bounds.
fn ray_mesh_bounds(
    ray: Ray,
    transform: GlobalTransform,
    mesh: &MeshAsset,
) -> Option<f32> {
    let (minimum, maximum) = mesh_bounds(mesh)?;
    let world_from_local = matrix_from_array(transform.matrix);
    let local_from_world = world_from_local.try_inverse()?;
    let local_origin4 = local_from_world * ray.origin.push(1.0);
    let local_direction4 = local_from_world * ray.direction.push(0.0);
    let local_origin =
        Vector3::new(local_origin4.x, local_origin4.y, local_origin4.z);
    let local_direction = Vector3::new(
        local_direction4.x,
        local_direction4.y,
        local_direction4.z,
    );
    let local_distance =
        ray_aabb(local_origin, local_direction, minimum, maximum)?;
    let local_hit = local_origin + local_direction * local_distance;
    let world_hit = world_from_local * local_hit.push(1.0);
    Some(
        (Vector3::new(world_hit.x, world_hit.y, world_hit.z) - ray.origin)
            .norm(),
    )
}

/// Returns the smallest box containing every mesh vertex.
fn mesh_bounds(mesh: &MeshAsset) -> Option<(Vector3<f32>, Vector3<f32>)> {
    let first = mesh.vertices.first()?.position;
    let mut minimum = Vector3::from(first);
    let mut maximum = minimum;
    for vertex in &mesh.vertices[1..] {
        let position = Vector3::from(vertex.position);
        minimum = minimum.inf(&position);
        maximum = maximum.sup(&position);
    }
    Some((minimum, maximum))
}

/// Standard slab intersection. The returned distance is in local ray units.
fn ray_aabb(
    origin: Vector3<f32>,
    direction: Vector3<f32>,
    minimum: Vector3<f32>,
    maximum: Vector3<f32>,
) -> Option<f32> {
    let mut entry = f32::NEG_INFINITY;
    let mut exit = f32::INFINITY;
    for axis in 0..3 {
        if direction[axis].abs() <= f32::EPSILON {
            if origin[axis] < minimum[axis] || origin[axis] > maximum[axis] {
                return None;
            }
            continue;
        }
        let first = (minimum[axis] - origin[axis]) / direction[axis];
        let second = (maximum[axis] - origin[axis]) / direction[axis];
        entry = entry.max(first.min(second));
        exit = exit.min(first.max(second));
    }
    (exit >= entry.max(0.0)).then_some(entry.max(0.0))
}

fn matrix_from_array(matrix: [[f32; 4]; 4]) -> Matrix4<f32> {
    Matrix4::from_column_slice(&matrix.concat())
}

fn vulkan_clip_correction() -> Matrix4<f32> {
    Matrix4::new(
        1.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.5, 0.0, 0.0,
        0.0, 1.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ray_aabb_returns_the_nearest_forward_hit() {
        let hit = ray_aabb(
            Vector3::new(0.0, 0.0, -3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(-1.0, -1.0, -1.0),
            Vector3::new(1.0, 1.0, 1.0),
        );
        assert_eq!(hit, Some(2.0));
    }
}
