//! Turns fluid particles into a triangle mesh: particles add a smooth
//! bump to a density grid, and marching tetrahedra extract the surface where
//! the density crosses a threshold.
//!
//! Deterministic: particles are splatted in index order and cells are
//! visited in ascending x, y, z order, with no atomics or hashing, so the
//! same particles give the same mesh bits.

// Grid loops index several small arrays at once; iterators read worse.
#![allow(clippy::needless_range_loop)]

use crate::assets::{MeshAsset, MeshVertex};

/// Density where the surface sits. One isolated particle reaches 1, so a
/// lone droplet still shows as a small blob.
const ISO: f32 = 0.5;
/// Splat radius in particle spacings.
const RADIUS: f32 = 1.6;
/// Upper bound on grid corners; coarser cells keep huge fluids affordable.
const MAX_CORNERS: f32 = 600_000.0;

/// Cube corner offsets; corners 0 and 6 are the diagonal every tetrahedron shares.
const CORNERS: [[usize; 3]; 8] = [
    [0, 0, 0],
    [1, 0, 0],
    [1, 1, 0],
    [0, 1, 0],
    [0, 0, 1],
    [1, 0, 1],
    [1, 1, 1],
    [0, 1, 1],
];
const TETS: [[usize; 4]; 6] = [
    [0, 5, 1, 6],
    [0, 1, 2, 6],
    [0, 2, 3, 6],
    [0, 3, 7, 6],
    [0, 7, 4, 6],
    [0, 4, 5, 6],
];

struct Grid {
    origin: [f32; 3],
    cell: f32,
    dims: [usize; 3],
    field: Vec<f32>,
}

impl Grid {
    fn at(&self, x: usize, y: usize, z: usize) -> f32 {
        self.field[x + self.dims[0] * (y + self.dims[1] * z)]
    }

    fn position(&self, corner: [usize; 3]) -> [f32; 3] {
        std::array::from_fn(|axis| {
            self.origin[axis] + corner[axis] as f32 * self.cell
        })
    }

    /// Density gradient by central differences; it points into the fluid.
    fn gradient(&self, c: [usize; 3]) -> [f32; 3] {
        std::array::from_fn(|axis| {
            let mut lo = c;
            let mut hi = c;
            lo[axis] = c[axis].saturating_sub(1);
            hi[axis] = (c[axis] + 1).min(self.dims[axis] - 1);
            self.at(hi[0], hi[1], hi[2]) - self.at(lo[0], lo[1], lo[2])
        })
    }
}

fn splat(positions: &[[f32; 3]], spacing: f32) -> Option<Grid> {
    let reach = RADIUS * spacing;
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for p in positions {
        for axis in 0..3 {
            min[axis] = min[axis].min(p[axis]);
            max[axis] = max[axis].max(p[axis]);
        }
    }
    if positions.is_empty() || min.iter().chain(&max).any(|v| !v.is_finite()) {
        return None;
    }
    let origin: [f32; 3] = std::array::from_fn(|axis| min[axis] - reach);
    let extent: [f32; 3] =
        std::array::from_fn(|axis| max[axis] - min[axis] + 2.0 * reach);
    let volume = extent[0] * extent[1] * extent[2];
    let cell = (spacing * 0.5).max((volume / MAX_CORNERS).cbrt());
    let dims: [usize; 3] =
        std::array::from_fn(|axis| (extent[axis] / cell).ceil() as usize + 2);
    let mut grid = Grid {
        origin,
        cell,
        dims,
        field: vec![0.0; dims[0] * dims[1] * dims[2]],
    };
    let span = (reach / cell).ceil() as isize;
    for p in positions {
        let base: [isize; 3] = std::array::from_fn(|axis| {
            ((p[axis] - origin[axis]) / cell).floor() as isize
        });
        for z in (base[2] - span).max(0)
            ..=(base[2] + span + 1).min(dims[2] as isize - 1)
        {
            for y in (base[1] - span).max(0)
                ..=(base[1] + span + 1).min(dims[1] as isize - 1)
            {
                for x in (base[0] - span).max(0)
                    ..=(base[0] + span + 1).min(dims[0] as isize - 1)
                {
                    let corner =
                        grid.position([x as usize, y as usize, z as usize]);
                    let r2 = (corner[0] - p[0]).powi(2)
                        + (corner[1] - p[1]).powi(2)
                        + (corner[2] - p[2]).powi(2);
                    let q = 1.0 - r2 / (reach * reach);
                    if q > 0.0 {
                        let index = x as usize
                            + dims[0] * (y as usize + dims[1] * z as usize);
                        grid.field[index] += q * q * q;
                    }
                }
            }
        }
    }
    Some(grid)
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len < 1e-9 {
        [0.0, 1.0, 0.0]
    } else {
        [v[0] / len, v[1] / len, v[2] / len]
    }
}

/// Builds the fluid surface for `positions` (world space, one per particle)
/// at the given rest `spacing`. Always returns a drawable mesh: with no
/// surface it is one degenerate triangle, because a GPU buffer cannot be empty.
pub fn surface_mesh(positions: &[[f32; 3]], spacing: f32) -> MeshAsset {
    let mut vertices: Vec<MeshVertex> = Vec::new();
    if let Some(grid) = splat(positions, spacing.max(1e-4)) {
        let [nx, ny, nz] = grid.dims;
        for z in 0..nz - 1 {
            for y in 0..ny - 1 {
                for x in 0..nx - 1 {
                    let corners: [[usize; 3]; 8] = std::array::from_fn(|i| {
                        [
                            x + CORNERS[i][0],
                            y + CORNERS[i][1],
                            z + CORNERS[i][2],
                        ]
                    });
                    let values: [f32; 8] = std::array::from_fn(|i| {
                        grid.at(corners[i][0], corners[i][1], corners[i][2])
                    });
                    if values.iter().all(|v| *v < ISO) {
                        continue;
                    }
                    for tet in TETS {
                        tetrahedron(
                            &grid,
                            &corners,
                            &values,
                            tet,
                            &mut vertices,
                        );
                    }
                }
            }
        }
    }
    if vertices.is_empty() {
        vertices = vec![MeshVertex::default(); 3];
    }
    // Triangles that share a corner share a vertex, so the mesh is a third
    // of the size to upload.
    let mut seen: std::collections::HashMap<[i64; 3], u32> =
        std::collections::HashMap::new();
    let scale = 1.0 / (spacing.max(1e-4) * 1e-3);
    let mut welded = Vec::with_capacity(vertices.len() / 3);
    let mut indices = Vec::with_capacity(vertices.len());
    for vertex in &vertices {
        let key = vertex.position.map(|axis| (axis * scale).round() as i64);
        let index = *seen.entry(key).or_insert_with(|| {
            welded.push(*vertex);
            (welded.len() - 1) as u32
        });
        indices.push(index);
    }
    MeshAsset {
        vertices: welded,
        indices,
    }
}

fn tetrahedron(
    grid: &Grid,
    corners: &[[usize; 3]; 8],
    values: &[f32; 8],
    tet: [usize; 4],
    out: &mut Vec<MeshVertex>,
) {
    let (inside, outside): (Vec<usize>, Vec<usize>) =
        tet.iter().partition(|&&i| values[i] >= ISO);
    if inside.is_empty() || outside.is_empty() {
        return;
    }
    let edge = |a: usize, b: usize| {
        let t = ((ISO - values[a]) / (values[b] - values[a])).clamp(0.0, 1.0);
        let (pa, pb) = (grid.position(corners[a]), grid.position(corners[b]));
        let (ga, gb) = (grid.gradient(corners[a]), grid.gradient(corners[b]));
        let position = std::array::from_fn(|i| pa[i] + (pb[i] - pa[i]) * t);
        let gradient = std::array::from_fn(|i| ga[i] + (gb[i] - ga[i]) * t);
        (position, gradient)
    };
    let triangles: Vec<[([f32; 3], [f32; 3]); 3]> = match inside.len() {
        1 => vec![[
            edge(inside[0], outside[0]),
            edge(inside[0], outside[1]),
            edge(inside[0], outside[2]),
        ]],
        3 => vec![[
            edge(outside[0], inside[0]),
            edge(outside[0], inside[1]),
            edge(outside[0], inside[2]),
        ]],
        _ => {
            let e00 = edge(inside[0], outside[0]);
            let e01 = edge(inside[0], outside[1]);
            let e11 = edge(inside[1], outside[1]);
            let e10 = edge(inside[1], outside[0]);
            vec![[e00, e01, e11], [e00, e11, e10]]
        }
    };
    for mut tri in triangles {
        let [a, b, c] = [tri[0].0, tri[1].0, tri[2].0];
        let u = std::array::from_fn::<f32, 3, _>(|i| b[i] - a[i]);
        let v = std::array::from_fn::<f32, 3, _>(|i| c[i] - a[i]);
        let face = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        // The gradient points into the fluid; the face must point out.
        let inward: f32 = (0..3)
            .map(|i| face[i] * (tri[0].1[i] + tri[1].1[i] + tri[2].1[i]))
            .sum();
        if inward > 0.0 {
            tri.swap(1, 2);
        }
        for (position, gradient) in tri {
            let normal = normalize([-gradient[0], -gradient[1], -gradient[2]]);
            out.push(MeshVertex {
                position,
                normal,
                uv: [0.0; 2],
                tangent: [1.0, 0.0, 0.0, 1.0],
                color: [1.0; 4],
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lattice(n: usize, spacing: f32) -> Vec<[f32; 3]> {
        let mut p = Vec::new();
        for z in 0..n {
            for y in 0..n {
                for x in 0..n {
                    p.push([
                        x as f32 * spacing,
                        y as f32 * spacing,
                        z as f32 * spacing,
                    ]);
                }
            }
        }
        p
    }

    #[test]
    fn a_block_of_particles_gets_one_closed_outward_surface() {
        let mesh = surface_mesh(&lattice(4, 0.1), 0.1);
        assert!(mesh.vertices.len() > 100);
        let center = [0.15; 3];
        // Every triangle faces away from the block's center.
        for tri in mesh.indices.chunks(3) {
            let at = |i: u32| mesh.vertices[i as usize].position;
            let (a, b, c) = (at(tri[0]), at(tri[1]), at(tri[2]));
            let u: [f32; 3] = std::array::from_fn(|i| b[i] - a[i]);
            let v: [f32; 3] = std::array::from_fn(|i| c[i] - a[i]);
            let n = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            let out: f32 = (0..3).map(|i| n[i] * (a[i] - center[i])).sum();
            assert!(out >= 0.0, "inward triangle");
        }
    }

    #[test]
    fn the_surface_is_reproducible_and_empty_fluid_still_draws() {
        let p = lattice(3, 0.1);
        assert_eq!(surface_mesh(&p, 0.1), surface_mesh(&p, 0.1));
        let empty = surface_mesh(&[], 0.1);
        assert_eq!(empty.indices.len(), 3);
    }
}
