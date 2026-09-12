//! Shared ray-casting math for object picking.
//!
//! This module has no editor dependency (in particular, no egui types) so it
//! can be used both by editor Scene View picking (`src/editor/picking.rs`,
//! which adapts egui's `Pos2`/`Rect` into the plain types used here) and by
//! runtime gameplay picking against a game's own render camera.

use nalgebra::{Matrix4, Orthographic3, Perspective3, Vector3, Vector4};

use crate::assets::MeshAsset;
use crate::runtime::{Camera, GlobalTransform, Projection};

/// A world-space ray produced by one screen-space click.
#[derive(Clone, Copy, Debug)]
pub struct Ray {
    pub origin: Vector3<f32>,
    pub direction: Vector3<f32>,
}

/// Builds the combined Vulkan clip-space `projection * view` matrix used by
/// both ray unprojection and forward point projection.
pub fn clip_from_world(
    viewport_size: [f32; 2],
    camera: Camera,
    camera_transform: GlobalTransform,
) -> Option<Matrix4<f32>> {
    let [width, height] = viewport_size;
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let aspect = width / height;
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
    Some(vulkan_clip_correction() * projection * view)
}

/// Builds a ray by unprojecting Vulkan near and far depth points.
///
/// `click` and `viewport_origin`/`viewport_size` are all in the same
/// screen-space units (pixels), with `viewport_origin` being the top-left
/// corner of the rendered viewport within the window.
pub fn scene_ray(
    click: [f32; 2],
    viewport_origin: [f32; 2],
    viewport_size: [f32; 2],
    camera: Camera,
    camera_transform: GlobalTransform,
) -> Option<Ray> {
    let relative =
        [click[0] - viewport_origin[0], click[1] - viewport_origin[1]];
    let ndc_x = relative[0] / viewport_size[0] * 2.0 - 1.0;
    // Vulkan viewport coordinates have their top edge at NDC Y = -1.
    let ndc_y = relative[1] / viewport_size[1] * 2.0 - 1.0;
    let inverse = clip_from_world(viewport_size, camera, camera_transform)?
        .try_inverse()?;
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

/// Projects a world-space point into viewport screen coordinates.
///
/// This is the forward counterpart to `scene_ray`: used to place and
/// hit-test the editor's translate/rotate/scale gizmo handles.
pub fn project_point(
    world: Vector3<f32>,
    camera: Camera,
    camera_transform: GlobalTransform,
    viewport_origin: [f32; 2],
    viewport_size: [f32; 2],
) -> Option<[f32; 2]> {
    let clip = clip_from_world(viewport_size, camera, camera_transform)?
        * world.push(1.0);
    if clip.w <= f32::EPSILON {
        return None;
    }
    let ndc = Vector3::new(clip.x / clip.w, clip.y / clip.w, clip.z / clip.w);
    Some([
        viewport_origin[0] + (ndc.x * 0.5 + 0.5) * viewport_size[0],
        viewport_origin[1] + (ndc.y * 0.5 + 0.5) * viewport_size[1],
    ])
}

/// Intersects a ray against one mesh's local-space axis-aligned bounds.
pub fn ray_mesh_bounds(
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
pub fn mesh_bounds(mesh: &MeshAsset) -> Option<(Vector3<f32>, Vector3<f32>)> {
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
pub fn ray_aabb(
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

pub fn matrix_from_array(matrix: [[f32; 4]; 4]) -> Matrix4<f32> {
    Matrix4::from_column_slice(&matrix.concat())
}

pub fn vulkan_clip_correction() -> Matrix4<f32> {
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
