use crate::assets::MeshAsset;
use crate::rendering::debug_overlay::RenderDebugOverlay;

/// Adds a one-unit axis plus a small two-line arrow head.
pub fn add_axis(
    overlay: &mut RenderDebugOverlay,
    origin: [f32; 3],
    direction: [f32; 3],
    color: [f32; 4],
) {
    let end = [
        origin[0] + direction[0],
        origin[1] + direction[1],
        origin[2] + direction[2],
    ];
    overlay.line(origin, end, color);
    // A compact arrow head is intentionally world-space and simple. It is a
    // visual guide, not the interactive transform tool yet.
    let side = if direction[1] == 0.0 {
        [0.0, 1.0, 0.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    for sign in [-1.0, 1.0] {
        overlay.line(
            end,
            [
                end[0] - direction[0] * 0.18 + side[0] * 0.09 * sign,
                end[1] - direction[1] * 0.18 + side[1] * 0.09 * sign,
                end[2] - direction[2] * 0.18 + side[2] * 0.09 * sign,
            ],
            color,
        );
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
