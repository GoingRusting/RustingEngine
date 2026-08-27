use crate::assets::MeshAsset;
use crate::rendering::debug_overlay::RenderDebugOverlay;

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

/// Returns the smallest local box containing every mesh vertex.
pub fn mesh_bounds(mesh: &MeshAsset) -> Option<([f32; 3], [f32; 3])> {
    let first = mesh.vertices.first()?.position;
    let mut minimum = first;
    let mut maximum = first;
    for vertex in &mesh.vertices[1..] {
        for axis in 0..3 {
            minimum[axis] = minimum[axis].min(vertex.position[axis]);
            maximum[axis] = maximum[axis].max(vertex.position[axis]);
        }
    }
    Some((minimum, maximum))
}

/// Finds the farthest transformed mesh corner from the object's origin.
/// The selected axes use this to extend beyond large or heavily scaled meshes.
pub fn mesh_world_radius_from_origin(
    mesh: &MeshAsset,
    matrix: [[f32; 4]; 4],
) -> Option<f32> {
    let (minimum, maximum) = mesh_bounds(mesh)?;
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
    Some(radius)
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
    use crate::assets::MeshVertex;

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
        let mesh = MeshAsset {
            vertices: vec![
                MeshVertex {
                    position: [-1.0, -1.0, -1.0],
                    ..MeshVertex::default()
                },
                MeshVertex {
                    position: [1.0, 1.0, 1.0],
                    ..MeshVertex::default()
                },
            ],
            indices: Vec::new(),
        };
        let matrix = crate::Transform::default()
            .with_scale(10.0, 10.0, 10.0)
            .to_matrix();

        let radius = mesh_world_radius_from_origin(&mesh, matrix).unwrap();
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
}
