use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::{Component, With, World};

use crate::rendering::debug_overlay::RenderDebugOverlay;
use crate::runtime::{
    Camera, DirectionalLight, GlobalTransform, PointLight, Projection,
    RenderBounds, SpotLight,
};

/// One world-space line segment.
pub type Segment = [[f32; 3]; 2];

/// Blender-style wire shapes for cameras and lights, which have no mesh to
/// see or click. Each entry is the object and its world-space segments; the
/// Scene View draws them and picks objects by clicking near them. `skip` is
/// the editor camera.
pub fn object_shapes(
    world: &World,
    skip: Option<Entity>,
) -> Vec<(Entity, Vec<Segment>)> {
    // One query per kind: a filter over a component type no object has
    // used yet (never registered) would hide every other kind too.
    fn objects<T: Component>(world: &World) -> Vec<(Entity, GlobalTransform)> {
        world
            .try_query_filtered::<(Entity, &GlobalTransform), With<T>>()
            .map(|mut query| {
                query
                    .iter(world)
                    .map(|(entity, transform)| (entity, *transform))
                    .collect()
            })
            .unwrap_or_default()
    }
    let mut found = objects::<Camera>(world);
    found.extend(objects::<DirectionalLight>(world));
    found.extend(objects::<PointLight>(world));
    found.extend(objects::<SpotLight>(world));
    found.sort_by_key(|(entity, _)| *entity);
    found.dedup_by_key(|(entity, _)| *entity);
    found
        .into_iter()
        .filter(|(entity, _)| Some(*entity) != skip)
        .map(|(entity, transform)| {
            let lines = local_shape(world, entity)
                .into_iter()
                .map(|line| {
                    line.map(|point| transform_point(transform.matrix, point))
                })
                .collect();
            (entity, lines)
        })
        .collect()
}

/// Local-space shape: forward is -Z and up is +Y, like the renderer.
fn local_shape(world: &World, entity: Entity) -> Vec<Segment> {
    let mut lines = Vec::new();
    if let Some(camera) = world.get::<Camera>(entity) {
        // ponytail: 16:9 frame and a fixed 50° pyramid for orthographic
        // cameras; use the game resolution once the project stores one.
        let fov = match camera.projection {
            Projection::Perspective {
                vertical_fov_radians,
                ..
            } => vertical_fov_radians,
            Projection::Orthographic { .. } => 50f32.to_radians(),
        };
        let depth = 0.8;
        let half_height = depth * (fov * 0.5).tan().clamp(0.1, 2.0);
        let half_width = half_height * 16.0 / 9.0;
        let corners = [
            [-half_width, -half_height, -depth],
            [half_width, -half_height, -depth],
            [half_width, half_height, -depth],
            [-half_width, half_height, -depth],
        ];
        for index in 0..4 {
            lines.push([[0.0; 3], corners[index]]);
            lines.push([corners[index], corners[(index + 1) % 4]]);
        }
        // The filled triangle in Blender marks the camera's up side.
        let base = half_height * 1.1;
        let tip = [0.0, base + half_height * 0.6, -depth];
        let left = [-half_width * 0.6, base, -depth];
        let right = [half_width * 0.6, base, -depth];
        lines.extend([[left, right], [right, tip], [tip, left]]);
    }
    if world.get::<DirectionalLight>(entity).is_some() {
        circle(&mut lines, [0.0; 3], 0.2, [0, 1]);
        for step in 0..8 {
            let angle = step as f32 / 8.0 * std::f32::consts::TAU;
            let (sin, cos) = angle.sin_cos();
            lines.push([
                [cos * 0.3, sin * 0.3, 0.0],
                [cos * 0.45, sin * 0.45, 0.0],
            ]);
        }
        lines.push([[0.0; 3], [0.0, 0.0, -1.5]]);
    }
    if world.get::<PointLight>(entity).is_some() {
        for axes in [[0, 1], [1, 2], [2, 0]] {
            circle(&mut lines, [0.0; 3], 0.15, axes);
        }
    }
    if let Some(spot) = world.get::<SpotLight>(entity) {
        let length = 1.0;
        let radius = length * spot.outer_angle.clamp(0.01, 1.5).tan();
        circle(&mut lines, [0.0; 3], 0.1, [0, 1]);
        circle(&mut lines, [0.0, 0.0, -length], radius, [0, 1]);
        for step in 0..4 {
            let angle = step as f32 / 4.0 * std::f32::consts::TAU;
            let (sin, cos) = angle.sin_cos();
            lines.push([[0.0; 3], [cos * radius, sin * radius, -length]]);
        }
    }
    lines
}

fn circle(
    lines: &mut Vec<Segment>,
    center: [f32; 3],
    radius: f32,
    [first, second]: [usize; 2],
) {
    const SEGMENTS: usize = 16;
    let point = |step: usize| {
        let angle = step as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
        let mut point = center;
        point[first] += radius * angle.cos();
        point[second] += radius * angle.sin();
        point
    };
    for step in 0..SEGMENTS {
        lines.push([point(step), point(step + 1)]);
    }
}

/// Adds one axis with an arrow head scaled proportionally to its shaft.
pub fn add_axis(
    overlay: &mut RenderDebugOverlay,
    origin: [f32; 3],
    direction: [f32; 3],
    length: f32,
    color: [f32; 4],
) {
    let length = length.max(0.01);
    let end = [
        origin[0] + direction[0] * length,
        origin[1] + direction[1] * length,
        origin[2] + direction[2] * length,
    ];
    overlay.line_on_top(origin, end, color, 4.0);

    // Choose a perpendicular that remains valid for axes pointing vertically.
    let reference = if direction[1].abs() < 0.9 {
        [0.0, 1.0, 0.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let side = normalize(cross(direction, reference));
    let head_length = length * 0.16;
    let head_width = length * 0.08;
    for sign in [-1.0, 1.0] {
        overlay.line_on_top(
            end,
            [
                end[0] - direction[0] * head_length
                    + side[0] * head_width * sign,
                end[1] - direction[1] * head_length
                    + side[1] * head_width * sign,
                end[2] - direction[2] * head_length
                    + side[2] * head_width * sign,
            ],
            color,
            4.0,
        );
    }
}

fn cross(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [
        left[1] * right[2] - left[2] * right[1],
        left[2] * right[0] - left[0] * right[2],
        left[0] * right[1] - left[1] * right[0],
    ]
}

fn normalize(value: [f32; 3]) -> [f32; 3] {
    let length =
        (value[0] * value[0] + value[1] * value[1] + value[2] * value[2])
            .sqrt();
    if length > f32::EPSILON {
        [value[0] / length, value[1] / length, value[2] / length]
    } else {
        [1.0, 0.0, 0.0]
    }
}

/// Finds the farthest transformed corner of a local mesh box from the
/// object's origin. The selected axes use this to extend beyond large or
/// heavily scaled meshes.
pub fn mesh_world_radius_from_origin(
    (minimum, maximum): ([f32; 3], [f32; 3]),
    matrix: [[f32; 4]; 4],
) -> f32 {
    let origin = [matrix[3][0], matrix[3][1], matrix[3][2]];
    let mut radius: f32 = 0.0;
    for x in [minimum[0], maximum[0]] {
        for y in [minimum[1], maximum[1]] {
            for z in [minimum[2], maximum[2]] {
                let corner = transform_point(matrix, [x, y, z]);
                let offset = [
                    corner[0] - origin[0],
                    corner[1] - origin[1],
                    corner[2] - origin[2],
                ];
                radius = radius.max(
                    (offset[0] * offset[0]
                        + offset[1] * offset[1]
                        + offset[2] * offset[2])
                        .sqrt(),
                );
            }
        }
    }
    radius
}

/// Adds a box around the selected mesh.
///
/// `minimum` and `maximum` are local mesh positions. `matrix` converts every
/// corner into world space, so the box follows object position, rotation,
/// hierarchy, and scale exactly like the rendered mesh.
pub fn add_bound_box(
    overlay: &mut RenderDebugOverlay,
    matrix: [[f32; 4]; 4],
    minimum: [f32; 3],
    maximum: [f32; 3],
    color: [f32; 4],
) {
    let corners = [
        [minimum[0], minimum[1], minimum[2]],
        [maximum[0], minimum[1], minimum[2]],
        [maximum[0], maximum[1], minimum[2]],
        [minimum[0], maximum[1], minimum[2]],
        [minimum[0], minimum[1], maximum[2]],
        [maximum[0], minimum[1], maximum[2]],
        [maximum[0], maximum[1], maximum[2]],
        [minimum[0], maximum[1], maximum[2]],
    ]
    .map(|corner| transform_point(matrix, corner));
    // Four bottom edges, four top edges, then four upright edges.
    for (start, end) in [
        (0, 1),
        (1, 2),
        (2, 3),
        (3, 0),
        (4, 5),
        (5, 6),
        (6, 7),
        (7, 4),
        (0, 4),
        (1, 5),
        (2, 6),
        (3, 7),
    ] {
        overlay.line(corners[start], corners[end], color);
    }
}

/// Adds the world-space volume that frustum culling tests for a
/// `RenderBounds` override, so the outline matches what the renderer uses:
/// a box becomes the world axis-aligned box around its transformed corners,
/// and a sphere grows by the largest axis scale.
pub fn add_render_bounds(
    overlay: &mut RenderDebugOverlay,
    bounds: RenderBounds,
    matrix: [[f32; 4]; 4],
    color: [f32; 4],
) {
    match bounds.transformed(&matrix) {
        RenderBounds::Aabb { min, max } => add_bound_box(
            overlay,
            nalgebra::Matrix4::<f32>::identity().into(),
            min,
            max,
            color,
        ),
        RenderBounds::Sphere { center, radius } => {
            const SEGMENTS: usize = 32;
            let point = |axis: usize, step: usize| {
                let angle =
                    step as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
                let mut point = center;
                point[(axis + 1) % 3] += radius * angle.cos();
                point[(axis + 2) % 3] += radius * angle.sin();
                point
            };
            // One great circle around each world axis.
            for axis in 0..3 {
                for step in 0..SEGMENTS {
                    overlay.line(
                        point(axis, step),
                        point(axis, step + 1),
                        color,
                    );
                }
            }
        }
    }
}

/// Multiplies a local point by the column-major transform array used by ECS.
fn transform_point(matrix: [[f32; 4]; 4], point: [f32; 3]) -> [f32; 3] {
    [
        matrix[0][0] * point[0]
            + matrix[1][0] * point[1]
            + matrix[2][0] * point[2]
            + matrix[3][0],
        matrix[0][1] * point[0]
            + matrix[1][1] * point[1]
            + matrix[2][1] * point[2]
            + matrix[3][1],
        matrix[0][2] * point[0]
            + matrix[1][2] * point[1]
            + matrix[2][2] * point[2]
            + matrix[3][2],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axis_uses_requested_world_length() {
        let mut overlay = RenderDebugOverlay::default();
        add_axis(
            &mut overlay,
            [2.0, 3.0, 4.0],
            [1.0, 0.0, 0.0],
            25.0,
            [1.0; 4],
        );

        assert_eq!(overlay.lines.len(), 3);
        assert_eq!(overlay.lines[0].end, [27.0, 3.0, 4.0]);
    }

    #[test]
    fn mesh_radius_includes_object_scale() {
        let matrix = crate::Transform::default()
            .with_scale(10.0, 10.0, 10.0)
            .to_matrix();

        let radius =
            mesh_world_radius_from_origin(([-1.0; 3], [1.0; 3]), matrix);
        assert!((radius - 300.0_f32.sqrt()).abs() < 0.001);
    }

    #[test]
    fn bound_box_has_twelve_edges() {
        let mut overlay = RenderDebugOverlay::default();
        add_bound_box(
            &mut overlay,
            [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            [-1.0, -1.0, -1.0],
            [1.0, 1.0, 1.0],
            [1.0, 1.0, 0.0, 1.0],
        );
        assert_eq!(overlay.lines.len(), 12);
    }

    #[test]
    fn render_bounds_outline_uses_the_culled_world_volume() {
        let matrix = crate::Transform::default()
            .with_position(10.0, 0.0, 0.0)
            .with_scale(2.0, 1.0, 1.0)
            .to_matrix();
        let mut overlay = RenderDebugOverlay::default();
        add_render_bounds(
            &mut overlay,
            RenderBounds::Aabb {
                min: [-1.0; 3],
                max: [1.0; 3],
            },
            matrix,
            [1.0; 4],
        );
        assert_eq!(overlay.lines.len(), 12);
        assert_eq!(overlay.lines[0].start, [8.0, -1.0, -1.0]);
        assert_eq!(overlay.lines[6].start, [12.0, 1.0, 1.0]);

        let mut overlay = RenderDebugOverlay::default();
        add_render_bounds(
            &mut overlay,
            RenderBounds::Sphere {
                center: [0.0; 3],
                radius: 1.0,
            },
            matrix,
            [1.0; 4],
        );
        assert_eq!(overlay.lines.len(), 96);
        // Every point sits on the radius-2 world sphere at x = 10.
        for line in &overlay.lines {
            let [x, y, z] = line.start;
            let distance = ((x - 10.0).powi(2) + y * y + z * z).sqrt();
            assert!((distance - 2.0).abs() < 1e-4, "{distance}");
        }
    }
}
