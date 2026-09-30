//! Scene View object picking.
//!
//! Picking belongs to the editor because it is an authoring action. It uses
//! render meshes, not physics colliders, so an object is selectable even when
//! it has no physics component.

use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::World;
use egui::{Pos2, Rect};
use nalgebra::{Matrix4, Orthographic3, Perspective3, Vector3, Vector4};

use crate::assets::AssetServer;
use crate::runtime::{Camera, GlobalTransform, MeshRenderer, Projection};

/// A world-space ray produced by one Scene View mouse click.
#[derive(Clone, Copy, Debug)]
pub(super) struct Ray {
    pub origin: Vector3<f32>,
    pub direction: Vector3<f32>,
}

/// Screen distance in points within which a click hits a camera or light
/// wire shape.
const SHAPE_PICK_RADIUS: f32 = 6.0;

/// Finds the nearest renderable mesh, camera, or light under a Scene View
/// click. Cameras and lights are hit by clicking near their wire shape.
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
            // Hidden objects cannot be clicked; select them in the Hierarchy.
            .filter(|(entity, ..)| {
                crate::runtime::visible_in_hierarchy(world, *entity)
                    && world
                        .get::<crate::runtime::FluidParticle>(*entity)
                        .is_none()
            })
            .map(|(entity, mesh, transform)| (entity, mesh.mesh, *transform))
            .collect::<Vec<_>>()
    };
    let shapes =
        crate::editor::overlay::object_shapes(world, Some(camera_entity));
    let shape_hits = shapes.into_iter().filter_map(|(entity, lines)| {
        lines
            .iter()
            .filter_map(|&[start, end]| {
                let project = |point| {
                    project_world_to_screen(
                        world,
                        camera_entity,
                        point,
                        viewport,
                    )
                };
                let near =
                    segment_distance(click, project(start)?, project(end)?)
                        <= SHAPE_PICK_RADIUS;
                near.then(|| (Vector3::from(start) - ray.origin).norm())
            })
            .min_by(f32::total_cmp)
            .map(|distance| (distance, entity))
    });
    let assets = world.resource::<AssetServer>();
    candidates
        .into_iter()
        .filter_map(|(entity, handle, transform)| {
            let bounds = assets.mesh_bounds(handle)?;
            let distance = ray_mesh_bounds(ray, transform, bounds)?;
            Some((distance, entity))
        })
        .chain(shape_hits.collect::<Vec<_>>())
        .min_by(|left, right| left.0.total_cmp(&right.0))
        // A tile selects the tile map that owns it.
        .map(|(_, entity)| {
            world
                .get::<crate::runtime::TileOf>(entity)
                .map_or(entity, |tile| tile.0)
        })
}

/// The tile map cell under a Scene View point. The ray meets the map's
/// plane (its origin's Z).
pub(super) fn tile_cell_under(
    world: &World,
    camera_entity: Entity,
    map_entity: Entity,
    point: Pos2,
    viewport: Rect,
) -> Option<(usize, usize)> {
    let camera = world.get::<Camera>(camera_entity).copied()?;
    let camera_transform = *world.get::<GlobalTransform>(camera_entity)?;
    let ray = scene_ray(point, viewport, camera, camera_transform)?;
    let origin = world.get::<crate::Transform>(map_entity)?.position;
    let distance = (origin[2] - ray.origin.z) / ray.direction.z;
    if !distance.is_finite() || distance < 0.0 {
        return None;
    }
    let hit = ray.origin + ray.direction * distance;
    world
        .get::<crate::runtime::TileMap>(map_entity)?
        .cell_at([hit.x - origin[0], hit.y - origin[1]])
}

/// Runs `edit` on a copy of a tile map and stores the copy only when a cell
/// changed, so an unchanged map does not respawn its tiles.
pub(super) fn edit_tile_map(
    world: &mut World,
    map_entity: Entity,
    edit: impl FnOnce(&mut crate::runtime::TileMap) -> bool,
) -> bool {
    let Some(mut map) =
        world.get::<crate::runtime::TileMap>(map_entity).cloned()
    else {
        return false;
    };
    let changed = edit(&mut map);
    if changed {
        world.entity_mut(map_entity).insert(map);
    }
    changed
}

/// Grabs the fog height square nearest a Scene View point. Returns `true`
/// for the main square at `height` or `false` for the falloff square, and
/// the grabbed point on the square's edge.
pub(super) fn grab_fog_square(
    world: &World,
    camera_entity: Entity,
    fog: &crate::runtime::Fog,
    point: Pos2,
    viewport: Rect,
) -> Option<(bool, [f32; 3])> {
    crate::editor::overlay::fog_height_lines(fog)
        .into_iter()
        .filter_map(|([start, end], main)| {
            let project = |point| {
                project_world_to_screen(world, camera_entity, point, viewport)
            };
            let (from, to) = (project(start)?, project(end)?);
            let along = to - from;
            let t = ((point - from).dot(along)
                / along.length_sq().max(f32::EPSILON))
            .clamp(0.0, 1.0);
            let distance = (from + along * t).distance(point);
            // ponytail: screen `t` is not perspective-correct; the anchor
            // only picks the drag plane, so a small error does not matter.
            let anchor = Vector3::from(start).lerp(&Vector3::from(end), t);
            (distance <= SHAPE_PICK_RADIUS).then_some((
                distance,
                main,
                anchor.into(),
            ))
        })
        .min_by(|left, right| left.0.total_cmp(&right.0))
        .map(|(_, main, anchor)| (main, anchor))
}

/// World height under a Scene View point, on the upright plane through
/// `anchor` that faces the camera. Dragging a fog square uses it.
pub(super) fn drag_height(
    world: &World,
    camera_entity: Entity,
    anchor: [f32; 3],
    point: Pos2,
    viewport: Rect,
) -> Option<f32> {
    let camera = *world.get::<Camera>(camera_entity)?;
    let camera_transform = *world.get::<GlobalTransform>(camera_entity)?;
    let ray = scene_ray(point, viewport, camera, camera_transform)?;
    let anchor = Vector3::from(anchor);
    let mut normal = ray.origin - anchor;
    normal.y = 0.0;
    let normal = normal.try_normalize(1.0e-4)?;
    let facing = normal.dot(&ray.direction);
    if facing.abs() < 1.0e-3 {
        return None;
    }
    let distance = normal.dot(&(anchor - ray.origin)) / facing;
    (distance.is_finite() && distance >= 0.0)
        .then(|| (ray.origin + ray.direction * distance).y)
}

/// World positions of an object's point handles, with the handle each one
/// drags: a reflection probe's box faces and a `RenderBounds` override's
/// faces or sphere points.
pub(super) fn point_handles(
    world: &World,
    entity: Entity,
) -> Vec<(super::SceneHandle, [f32; 3])> {
    use crate::editor::overlay::{
        probe_face_centers, render_bounds_handles, transform_point,
    };
    let Some(matrix) = world
        .get::<GlobalTransform>(entity)
        .map(|transform| transform.matrix)
    else {
        return Vec::new();
    };
    let mut handles = Vec::new();
    if let Some(probe) = world.get::<crate::runtime::ReflectionProbe>(entity) {
        let center = transform_point(matrix, [0.0; 3]);
        handles.extend(probe_face_centers(center, probe.extents).map(
            |(axis, face)| (super::SceneHandle::ProbeFace { axis }, face),
        ));
    }
    if let Some(&bounds) = world.get::<crate::runtime::RenderBounds>(entity) {
        handles.extend(
            render_bounds_handles(bounds).into_iter().enumerate().map(
                |(index, local)| {
                    (
                        super::SceneHandle::BoundsFace { index },
                        transform_point(matrix, local),
                    )
                },
            ),
        );
    }
    handles
}

/// Grabs the handle of `entity` under a Scene View point: a fog height
/// square, or the nearest point handle.
pub(super) fn grab_scene_handle(
    world: &World,
    camera_entity: Entity,
    entity: Entity,
    point: Pos2,
    viewport: Rect,
) -> Option<super::SceneHandle> {
    let fog = world.get::<crate::runtime::Fog>(entity).and_then(|fog| {
        grab_fog_square(world, camera_entity, fog, point, viewport)
    });
    if let Some((main, anchor)) = fog {
        return Some(super::SceneHandle::FogHeight { main, anchor });
    }
    point_handles(world, entity)
        .into_iter()
        .filter_map(|(handle, position)| {
            let screen = project_world_to_screen(
                world,
                camera_entity,
                position,
                viewport,
            )?;
            let distance = screen.distance(point);
            (distance <= SHAPE_PICK_RADIUS * 1.5).then_some((distance, handle))
        })
        .min_by(|left, right| left.0.total_cmp(&right.0))
        .map(|(_, handle)| handle)
}

/// Applies a Scene View handle drag to the field it edits. Returns true
/// when the field changed.
pub(super) fn drag_scene_handle(
    world: &mut World,
    camera_entity: Entity,
    entity: Entity,
    handle: super::SceneHandle,
    point: Pos2,
    viewport: Rect,
) -> bool {
    match handle {
        super::SceneHandle::FogHeight { main, anchor } => {
            let Some(height) =
                drag_height(world, camera_entity, anchor, point, viewport)
            else {
                return false;
            };
            let Some(mut fog) = world.get_mut::<crate::runtime::Fog>(entity)
            else {
                return false;
            };
            if main {
                fog.height = height;
            } else {
                // At least 0.1 m above `height`, so the falloff stays finite.
                fog.height_falloff = 1.0 / (height - fog.height).max(0.1);
            }
            true
        }
        super::SceneHandle::ProbeFace { axis } => {
            let Some(center) = world
                .get::<GlobalTransform>(entity)
                .map(|transform| transform.matrix[3])
            else {
                return false;
            };
            let Some(along) = drag_along_line(
                world,
                camera_entity,
                [center[0], center[1], center[2]],
                Vector3::ith(axis, 1.0),
                point,
                viewport,
            ) else {
                return false;
            };
            let Some(mut probe) =
                world.get_mut::<crate::runtime::ReflectionProbe>(entity)
            else {
                return false;
            };
            // The box stays centered on the probe, so both faces move.
            probe.extents[axis] = along.abs().max(0.05);
            true
        }
        super::SceneHandle::BoundsFace { index } => {
            let (Some(transform), Some(&bounds)) = (
                world.get::<GlobalTransform>(entity),
                world.get::<crate::runtime::RenderBounds>(entity),
            ) else {
                return false;
            };
            let matrix = transform.matrix;
            let axis = index / 2;
            let local =
                crate::editor::overlay::render_bounds_handles(bounds)[index];
            // The handle moves along the object's local axis; its length
            // is the axis scale.
            let column =
                Vector3::new(matrix[axis][0], matrix[axis][1], matrix[axis][2]);
            let scale = column.norm();
            let Some(along) = (scale > f32::EPSILON)
                .then(|| {
                    drag_along_line(
                        world,
                        camera_entity,
                        crate::editor::overlay::transform_point(matrix, local),
                        column / scale,
                        point,
                        viewport,
                    )
                })
                .flatten()
            else {
                return false;
            };
            let mut moved = bounds;
            crate::editor::overlay::move_render_bounds_handle(
                &mut moved,
                index,
                local[axis] + along / scale,
            );
            if moved == bounds {
                return false;
            }
            world.entity_mut(entity).insert(moved);
            true
        }
    }
}

/// Distance along the unit `direction` from `origin` to the point on that
/// line closest to the ray under a Scene View point.
fn drag_along_line(
    world: &World,
    camera_entity: Entity,
    origin: [f32; 3],
    direction: Vector3<f32>,
    point: Pos2,
    viewport: Rect,
) -> Option<f32> {
    let camera = *world.get::<Camera>(camera_entity)?;
    let camera_transform = *world.get::<GlobalTransform>(camera_entity)?;
    let ray = scene_ray(point, viewport, camera, camera_transform)?;
    let offset = ray.origin - Vector3::from(origin);
    let cosine = direction.dot(&ray.direction);
    let denominator = 1.0 - cosine * cosine;
    // A ray along the line gives no distance.
    (denominator > 1.0e-4).then(|| {
        (direction.dot(&offset) - cosine * ray.direction.dot(&offset))
            / denominator
    })
}

/// Distance from `point` to the segment between `start` and `end`.
fn segment_distance(point: Pos2, start: Pos2, end: Pos2) -> f32 {
    let along = end - start;
    let t = ((point - start).dot(along) / along.length_sq().max(f32::EPSILON))
        .clamp(0.0, 1.0);
    (start + along * t).distance(point)
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
    (minimum, maximum): ([f32; 3], [f32; 3]),
) -> Option<f32> {
    let (minimum, maximum) = (Vector3::from(minimum), Vector3::from(maximum));
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

    #[test]
    fn clicks_near_light_and_camera_shapes_select_them() {
        use crate::runtime::{PointLight, SpotLight};
        let mut app = crate::App::new();
        app.add_plugin(crate::AssetPlugin).unwrap();
        let world = app.world_mut();
        let at = |x: f32, y: f32, z: f32| GlobalTransform {
            matrix: Matrix4::new_translation(&Vector3::new(x, y, z)).into(),
        };
        let camera = Camera {
            projection: Projection::Perspective {
                vertical_fov_radians: 1.0,
                near: 0.1,
                far: 100.0,
            },
            active: true,
            priority: 0,
        };
        let editor_camera = world.spawn((camera, at(0.0, 0.0, 0.0))).id();
        // A mesh behind the light, and a scene camera off to the right.
        let mesh = world.resource::<AssetServer>().fallback_mesh;
        let material = world.resource::<AssetServer>().fallback_material;
        let wall = world
            .spawn((
                MeshRenderer {
                    mesh,
                    material,
                    cast_shadows: true,
                    receive_shadows: true,
                },
                at(0.0, 0.0, -10.0),
            ))
            .id();
        let light = world
            .spawn((PointLight::default(), at(0.0, 0.0, -5.0)))
            .id();
        let spot = world
            .spawn((SpotLight::default(), at(-2.0, 0.0, -5.0)))
            .id();
        let scene_camera = world.spawn((camera, at(2.0, 0.0, -5.0))).id();
        let viewport =
            Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
        let screen = |world: &World, point| {
            project_world_to_screen(world, editor_camera, point, viewport)
                .unwrap()
        };

        // The point light's circle passes 0.15 right of its center.
        let click = screen(world, [0.15, 0.0, -5.0]);
        assert_eq!(
            pick_entity(world, editor_camera, click, viewport),
            Some(light)
        );
        // Just outside the light's circles, the wall behind is hit.
        let click = screen(world, [0.0, 0.25, -5.0]);
        assert_eq!(
            pick_entity(world, editor_camera, click, viewport),
            Some(wall)
        );
        let click = screen(world, [-2.1, 0.0, -5.0]);
        assert_eq!(
            pick_entity(world, editor_camera, click, viewport),
            Some(spot)
        );
        // The scene camera's frustum edge runs from its origin forward.
        let click = screen(world, [2.0, 0.0, -5.0]);
        assert_eq!(
            pick_entity(world, editor_camera, click, viewport),
            Some(scene_camera)
        );
        // The editor camera never picks itself, and empty space picks nothing.
        let click = Pos2::new(5.0, 5.0);
        assert_eq!(pick_entity(world, editor_camera, click, viewport), None);
        // Hidden objects cannot be clicked, so the click passes to nothing.
        world
            .entity_mut(wall)
            .insert(crate::runtime::Visibility { visible: false });
        let click = screen(world, [0.0, 0.25, -5.0]);
        assert_eq!(pick_entity(world, editor_camera, click, viewport), None);
    }

    #[test]
    fn fog_squares_are_grabbed_and_dragged_to_a_height() {
        let mut world = World::new();
        let camera = Camera {
            projection: Projection::Perspective {
                vertical_fov_radians: 1.0,
                near: 0.1,
                far: 100.0,
            },
            active: true,
            priority: 0,
        };
        let editor_camera = world
            .spawn((
                camera,
                GlobalTransform {
                    matrix: Matrix4::new_translation(&Vector3::new(
                        0.0, 2.0, 10.0,
                    ))
                    .into(),
                },
            ))
            .id();
        let viewport =
            Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
        let screen = |point| {
            project_world_to_screen(&world, editor_camera, point, viewport)
                .unwrap()
        };
        // Default fog: main square at 0 m, falloff square at 10 m.
        let fog = crate::runtime::Fog::default();
        let grab = |point| {
            grab_fog_square(&world, editor_camera, &fog, point, viewport)
        };
        let (main, anchor) = grab(screen([3.0, 0.0, -20.0])).unwrap();
        assert!(main);
        assert!((anchor[0] - 3.0).abs() < 0.1 && anchor[2] == -20.0);
        assert!(!grab(screen([0.0, 10.0, -20.0])).unwrap().0);
        assert_eq!(grab(screen([0.0, 5.0, -20.0])), None);
        let height = drag_height(
            &world,
            editor_camera,
            anchor,
            screen([3.0, 4.0, -20.0]),
            viewport,
        )
        .unwrap();
        assert!((height - 4.0).abs() < 1.0e-3, "{height}");
    }

    #[test]
    fn dragging_scene_handles_resizes_a_probe_and_thins_fog() {
        use crate::editor::SceneHandle;
        use crate::runtime::{Fog, ReflectionProbe};
        let mut world = World::new();
        let editor_camera = world
            .spawn((
                Camera {
                    projection: Projection::Perspective {
                        vertical_fov_radians: 1.0,
                        near: 0.1,
                        far: 100.0,
                    },
                    active: true,
                    priority: 0,
                },
                GlobalTransform {
                    matrix: Matrix4::new_translation(&Vector3::new(
                        0.0, 2.0, 10.0,
                    ))
                    .into(),
                },
            ))
            .id();
        let probe = world
            .spawn((
                ReflectionProbe::default(),
                GlobalTransform {
                    matrix: Matrix4::new_translation(&Vector3::new(
                        0.0, 0.0, -20.0,
                    ))
                    .into(),
                },
            ))
            .id();
        let viewport =
            Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
        let screen = |world: &World, point| {
            project_world_to_screen(world, editor_camera, point, viewport)
                .unwrap()
        };
        // The +X face center of the 5 m box grabs axis 0; a point between
        // the faces grabs nothing.
        let face = screen(&world, [5.0, 0.0, -20.0]);
        assert!(matches!(
            grab_scene_handle(&world, editor_camera, probe, face, viewport),
            Some(SceneHandle::ProbeFace { axis: 0 })
        ));
        let between = screen(&world, [2.5, 2.5, -20.0]);
        assert_eq!(
            grab_scene_handle(&world, editor_camera, probe, between, viewport),
            None
        );
        let point = screen(&world, [7.0, 0.0, -20.0]);
        assert!(drag_scene_handle(
            &mut world,
            editor_camera,
            probe,
            SceneHandle::ProbeFace { axis: 0 },
            point,
            viewport,
        ));
        let extents = world.get::<ReflectionProbe>(probe).unwrap().extents;
        assert!((extents[0] - 7.0).abs() < 1.0e-3, "{extents:?}");
        assert_eq!(extents[1..], [5.0, 5.0]);
        // Dragging the falloff square to 5 m above `height` thins the fog
        // by 1/e over 5 m.
        let fog = world.spawn(Fog::default()).id();
        let point = screen(&world, [0.0, 5.0, -20.0]);
        assert!(drag_scene_handle(
            &mut world,
            editor_camera,
            fog,
            SceneHandle::FogHeight {
                main: false,
                anchor: [0.0, 10.0, -20.0],
            },
            point,
            viewport,
        ));
        let falloff = world.get::<Fog>(fog).unwrap().height_falloff;
        assert!((falloff - 0.2).abs() < 1.0e-3, "{falloff}");
    }

    #[test]
    fn dragging_render_bounds_handles_moves_one_face_in_local_space() {
        use crate::editor::SceneHandle;
        use crate::runtime::RenderBounds;
        let mut world = World::new();
        let editor_camera = world
            .spawn((
                Camera {
                    projection: Projection::Perspective {
                        vertical_fov_radians: 1.0,
                        near: 0.1,
                        far: 100.0,
                    },
                    active: true,
                    priority: 0,
                },
                GlobalTransform {
                    matrix: Matrix4::new_translation(&Vector3::new(
                        0.0, 2.0, 10.0,
                    ))
                    .into(),
                },
            ))
            .id();
        // Scaled 2x along X, so the +X face of the unit box is 2 m out.
        let object = world
            .spawn((
                RenderBounds::Aabb {
                    min: [-1.0; 3],
                    max: [1.0; 3],
                },
                GlobalTransform {
                    matrix: (Matrix4::new_translation(&Vector3::new(
                        0.0, 0.0, -20.0,
                    )) * Matrix4::new_nonuniform_scaling(
                        &Vector3::new(2.0, 1.0, 1.0),
                    ))
                    .into(),
                },
            ))
            .id();
        let viewport =
            Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
        let screen = |world: &World, point| {
            project_world_to_screen(world, editor_camera, point, viewport)
                .unwrap()
        };
        let face = screen(&world, [2.0, 0.0, -20.0]);
        let handle =
            grab_scene_handle(&world, editor_camera, object, face, viewport);
        assert_eq!(handle, Some(SceneHandle::BoundsFace { index: 1 }));
        // 4 m in world is 2 in local X; only the +X face moves.
        let point = screen(&world, [4.0, 0.0, -20.0]);
        assert!(drag_scene_handle(
            &mut world,
            editor_camera,
            object,
            handle.unwrap(),
            point,
            viewport,
        ));
        let RenderBounds::Aabb { min, max } =
            *world.get::<RenderBounds>(object).unwrap()
        else {
            panic!("still a box");
        };
        assert_eq!(min, [-1.0; 3]);
        assert!((max[0] - 2.0).abs() < 1.0e-3, "{max:?}");
        // A face stops at the opposite face; a sphere point sets the radius.
        let mut bounds = RenderBounds::Aabb {
            min: [-1.0; 3],
            max: [1.0; 3],
        };
        crate::editor::overlay::move_render_bounds_handle(&mut bounds, 1, -3.0);
        assert_eq!(
            bounds,
            RenderBounds::Aabb {
                min: [-1.0; 3],
                max: [-1.0, 1.0, 1.0],
            }
        );
        let mut sphere = RenderBounds::Sphere {
            center: [0.0, 1.0, 0.0],
            radius: 1.0,
        };
        crate::editor::overlay::move_render_bounds_handle(&mut sphere, 2, -2.0);
        assert_eq!(
            sphere,
            RenderBounds::Sphere {
                center: [0.0, 1.0, 0.0],
                radius: 3.0,
            }
        );
    }

    #[test]
    fn scene_view_points_paint_the_tile_map_cell_under_them() {
        fn paint_tile(
            world: &mut World,
            camera: Entity,
            map: Entity,
            point: Pos2,
            viewport: Rect,
            brush: char,
        ) -> bool {
            tile_cell_under(world, camera, map, point, viewport).is_some_and(
                |(column, row)| {
                    edit_tile_map(world, map, |tiles| {
                        tiles.set_cell(column, row, brush)
                    })
                },
            )
        }
        use crate::runtime::TileMap;
        let mut world = World::new();
        let camera = Camera {
            projection: Projection::Perspective {
                vertical_fov_radians: 1.0,
                near: 0.1,
                far: 100.0,
            },
            active: true,
            priority: 0,
        };
        let editor_camera = world
            .spawn((
                camera,
                GlobalTransform {
                    matrix: Matrix4::new_translation(&Vector3::new(
                        0.0, 0.0, 10.0,
                    ))
                    .into(),
                },
            ))
            .id();
        let map = world
            .spawn((
                crate::Transform::new([0.0, 0.0, 0.0]),
                TileMap {
                    rows: vec!["#".into()],
                    ..TileMap::default()
                },
            ))
            .id();
        let viewport =
            Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
        let screen = |world: &World, point| {
            project_world_to_screen(world, editor_camera, point, viewport)
                .unwrap()
        };
        let point = screen(&world, [1.5, -1.5, 0.0]);
        assert!(paint_tile(
            &mut world,
            editor_camera,
            map,
            point,
            viewport,
            '#'
        ));
        assert_eq!(world.get::<TileMap>(map).unwrap().rows, vec!["#", ".#"]);
        assert!(!paint_tile(
            &mut world,
            editor_camera,
            map,
            point,
            viewport,
            '#'
        ));
        // Above the map's top edge there is no cell.
        let point = screen(&world, [0.5, 0.5, 0.0]);
        assert!(!paint_tile(
            &mut world,
            editor_camera,
            map,
            point,
            viewport,
            '.'
        ));
        let point = screen(&world, [0.5, -0.5, 0.0]);
        assert!(paint_tile(
            &mut world,
            editor_camera,
            map,
            point,
            viewport,
            '.'
        ));
        assert_eq!(world.get::<TileMap>(map).unwrap().rows, vec![".", ".#"]);
    }
}
