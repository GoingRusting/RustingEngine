//! Deterministic XPBD soft bodies from tetrahedral meshes (Macklin, Müller
//! and Chentanez 2016) on the CPU.
//!
//! Each tetrahedron edge is a distance constraint and each tetrahedron a
//! volume constraint, both with XPBD compliance. Steps are split into small
//! substeps with one constraint pass each, so no Lagrange multipliers are
//! kept between passes. Constraints are solved Gauss-Seidel in ascending
//! index order, nothing is accumulated through atomics and no randomness is
//! used: the same mesh and settings give the same bits. It is independent of
//! the ECS; a component, rigid-body attachment and tearing come later.

// The particle loops index several parallel arrays at once.
#![allow(clippy::needless_range_loop)]

use nalgebra::Vector3;

/// Most substeps one step runs, whatever the settings ask.
pub const MAX_SOFT_BODY_SUBSTEPS: u32 = 64;

/// Solver settings. Lengths are meters and times seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoftBodySettings {
    pub gravity: [f32; 3],
    /// Substeps per step, up to [`MAX_SOFT_BODY_SUBSTEPS`]. More is stiffer
    /// and slower.
    pub substeps: u32,
    /// Edge compliance (inverse stiffness). 0 keeps edges rigid.
    pub edge_compliance: f32,
    /// Volume compliance. 0 keeps every tetrahedron at its rest volume.
    pub volume_compliance: f32,
    /// Height of a ground plane, if any. Particles below it are lifted onto
    /// it and lose their sideways motion (static friction).
    pub floor: Option<f32>,
    /// Fraction of velocity lost per second, so wobbles die out.
    pub damping: f32,
}

impl Default for SoftBodySettings {
    fn default() -> Self {
        Self {
            gravity: [0.0, -9.81, 0.0],
            substeps: 10,
            edge_compliance: 0.0,
            volume_compliance: 0.0,
            floor: Some(0.0),
            damping: 1.0,
        }
    }
}

/// A tetrahedral soft body: particles, their tetrahedra and the edges
/// between them, with rest lengths and volumes taken at creation.
#[derive(Clone, Debug, PartialEq)]
pub struct SoftBody {
    pub positions: Vec<[f32; 3]>,
    pub velocities: Vec<[f32; 3]>,
    /// 0 for pinned particles, which never move.
    pub inverse_masses: Vec<f32>,
    pub tetrahedra: Vec<[u32; 4]>,
    pub edges: Vec<[u32; 2]>,
    rest_lengths: Vec<f32>,
    rest_volumes: Vec<f32>,
}

fn vector(value: [f32; 3]) -> Vector3<f32> {
    Vector3::from(value)
}

fn tetrahedron_volume(positions: &[[f32; 3]], tet: [u32; 4]) -> f32 {
    let [a, b, c, d] = tet.map(|index| vector(positions[index as usize]));
    (b - a).cross(&(c - a)).dot(&(d - a)) / 6.0
}

impl SoftBody {
    /// Builds a body from particle positions and tetrahedra. Each
    /// tetrahedron's mass (`density` times its volume) is shared equally by
    /// its four particles. Inside-out tetrahedra are turned the right way.
    /// Refuses indices out of range and flat tetrahedra.
    pub fn new(
        positions: Vec<[f32; 3]>,
        mut tetrahedra: Vec<[u32; 4]>,
        density: f32,
    ) -> Result<Self, String> {
        let mut masses = vec![0.0_f32; positions.len()];
        let mut rest_volumes = Vec::with_capacity(tetrahedra.len());
        let mut edges = Vec::new();
        for (index, tet) in tetrahedra.iter_mut().enumerate() {
            if tet.iter().any(|&p| p as usize >= positions.len()) {
                return Err(format!(
                    "tetrahedron {index} has no such particle"
                ));
            }
            let mut volume = tetrahedron_volume(&positions, *tet);
            if volume < 0.0 {
                tet.swap(2, 3);
                volume = -volume;
            }
            if volume <= 1e-12 {
                return Err(format!("tetrahedron {index} is flat"));
            }
            rest_volumes.push(volume);
            for &particle in tet.iter() {
                masses[particle as usize] += density * volume / 4.0;
            }
            for (a, b) in [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)] {
                edges.push([tet[a].min(tet[b]), tet[a].max(tet[b])]);
            }
        }
        // Shared edges appear once, in a fixed (sorted) order.
        edges.sort_unstable();
        edges.dedup();
        let rest_lengths = edges
            .iter()
            .map(|&[a, b]| {
                (vector(positions[a as usize]) - vector(positions[b as usize]))
                    .norm()
            })
            .collect();
        let inverse_masses = masses
            .iter()
            .map(|&mass| if mass > 0.0 { 1.0 / mass } else { 0.0 })
            .collect();
        Ok(Self {
            velocities: vec![[0.0; 3]; positions.len()],
            positions,
            inverse_masses,
            tetrahedra,
            edges,
            rest_lengths,
            rest_volumes,
        })
    }

    /// A box of `cells` cubes of side `spacing` with its minimum corner at
    /// `min`, each cube cut into six tetrahedra around its main diagonal
    /// (the Kuhn split, so neighbor cubes share faces exactly).
    pub fn block(
        min: [f32; 3],
        cells: [u32; 3],
        spacing: f32,
        density: f32,
    ) -> Result<Self, String> {
        let [nx, ny, nz] = cells.map(|count| count + 1);
        let mut positions = Vec::with_capacity((nx * ny * nz) as usize);
        for z in 0..nz {
            for y in 0..ny {
                for x in 0..nx {
                    positions.push([
                        min[0] + x as f32 * spacing,
                        min[1] + y as f32 * spacing,
                        min[2] + z as f32 * spacing,
                    ]);
                }
            }
        }
        let index = |x: u32, y: u32, z: u32| x + nx * (y + ny * z);
        let mut tetrahedra = Vec::new();
        for z in 0..cells[2] {
            for y in 0..cells[1] {
                for x in 0..cells[0] {
                    let corner =
                        |c: [u32; 3]| index(x + c[0], y + c[1], z + c[2]);
                    for order in [
                        [0, 1, 2],
                        [0, 2, 1],
                        [1, 0, 2],
                        [1, 2, 0],
                        [2, 0, 1],
                        [2, 1, 0],
                    ] {
                        let mut step = [0u32; 3];
                        let mut tet = [corner(step); 4];
                        for (slot, axis) in order.into_iter().enumerate() {
                            step[axis] = 1;
                            tet[slot + 1] = corner(step);
                        }
                        tetrahedra.push(tet);
                    }
                }
            }
        }
        Self::new(positions, tetrahedra, density)
    }

    /// Pins a particle where it is: it keeps its place whatever pulls on it.
    pub fn pin(&mut self, particle: usize) {
        self.inverse_masses[particle] = 0.0;
        self.velocities[particle] = [0.0; 3];
    }

    /// Total volume of all tetrahedra.
    pub fn volume(&self) -> f32 {
        self.tetrahedra
            .iter()
            .map(|&tet| tetrahedron_volume(&self.positions, tet))
            .sum()
    }

    /// Total volume at rest.
    pub fn rest_volume(&self) -> f32 {
        self.rest_volumes.iter().sum()
    }

    /// Advances the body by `dt` seconds.
    pub fn step(&mut self, settings: &SoftBodySettings, dt: f32) {
        let substeps = settings.substeps.clamp(1, MAX_SOFT_BODY_SUBSTEPS);
        let h = dt / substeps as f32;
        if h <= 0.0 {
            return;
        }
        let gravity = vector(settings.gravity);
        let keep = (1.0 - settings.damping * h).clamp(0.0, 1.0);
        let mut previous = self.positions.clone();
        for _ in 0..substeps {
            for i in 0..self.positions.len() {
                previous[i] = self.positions[i];
                if self.inverse_masses[i] == 0.0 {
                    continue;
                }
                let velocity = vector(self.velocities[i]) * keep + gravity * h;
                self.velocities[i] = velocity.into();
                self.positions[i] =
                    (vector(self.positions[i]) + velocity * h).into();
            }
            self.solve_edges(settings.edge_compliance / (h * h));
            self.solve_volumes(settings.volume_compliance / (h * h));
            for i in 0..self.positions.len() {
                if self.inverse_masses[i] == 0.0 {
                    continue;
                }
                if let Some(floor) = settings.floor {
                    if self.positions[i][1] < floor {
                        self.positions[i] =
                            [previous[i][0], floor, previous[i][2]];
                    }
                }
                self.velocities[i] =
                    ((vector(self.positions[i]) - vector(previous[i])) / h)
                        .into();
            }
        }
    }

    fn solve_edges(&mut self, alpha: f32) {
        for (edge, &rest) in self.edges.iter().zip(&self.rest_lengths) {
            let [a, b] = edge.map(|index| index as usize);
            let weight = self.inverse_masses[a] + self.inverse_masses[b];
            let delta = vector(self.positions[a]) - vector(self.positions[b]);
            let length = delta.norm();
            if weight + alpha == 0.0 || length < 1e-9 {
                continue;
            }
            let normal = delta / length;
            let lambda = -(length - rest) / (weight + alpha);
            self.positions[a] = (vector(self.positions[a])
                + normal * (lambda * self.inverse_masses[a]))
                .into();
            self.positions[b] = (vector(self.positions[b])
                - normal * (lambda * self.inverse_masses[b]))
                .into();
        }
    }

    fn solve_volumes(&mut self, alpha: f32) {
        // For each corner, the two other corners whose cross product (from
        // a third) gives the outward volume gradient of that corner.
        const OPPOSITE: [[usize; 3]; 4] =
            [[1, 3, 2], [0, 2, 3], [0, 3, 1], [0, 1, 2]];
        for (tet, &rest) in self.tetrahedra.iter().zip(&self.rest_volumes) {
            let corners = tet.map(|index| index as usize);
            let points = corners.map(|index| vector(self.positions[index]));
            let mut gradients = [Vector3::zeros(); 4];
            let mut weight = 0.0;
            for (j, [p, q, r]) in OPPOSITE.into_iter().enumerate() {
                gradients[j] = (points[q] - points[p])
                    .cross(&(points[r] - points[p]))
                    / 6.0;
                weight += self.inverse_masses[corners[j]]
                    * gradients[j].norm_squared();
            }
            if weight + alpha == 0.0 {
                continue;
            }
            let volume = (points[1] - points[0])
                .cross(&(points[2] - points[0]))
                .dot(&(points[3] - points[0]))
                / 6.0;
            let lambda = -(volume - rest) / (weight + alpha);
            for j in 0..4 {
                let index = corners[j];
                self.positions[index] = (vector(self.positions[index])
                    + gradients[j] * (lambda * self.inverse_masses[index]))
                    .into();
            }
        }
    }

    /// FNV-1a over every position and velocity bit, for determinism checks.
    pub fn state_hash(&self) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        for value in self.positions.iter().chain(&self.velocities).flatten() {
            hash = (hash ^ u64::from(value.to_bits()))
                .wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn run(body: &mut SoftBody, settings: &SoftBodySettings, steps: u32) {
        for _ in 0..steps {
            body.step(settings, DT);
        }
    }

    #[test]
    fn a_block_is_six_tetrahedra_per_cube_and_fills_its_box() {
        let body = SoftBody::block([0.0; 3], [2, 3, 4], 0.5, 1000.0).unwrap();
        assert_eq!(body.positions.len(), 3 * 4 * 5);
        assert_eq!(body.tetrahedra.len(), 6 * 24);
        let expected = 1.0 * 1.5 * 2.0;
        assert!((body.volume() - expected).abs() < 1e-4, "{}", body.volume());
        let mass: f32 = body.inverse_masses.iter().map(|w| 1.0 / w).sum();
        assert!((mass - expected * 1000.0).abs() < 0.5, "{mass}");
        assert!(
            SoftBody::new(vec![[0.0; 3]; 4], vec![[0, 1, 2, 3]], 1.0).is_err()
        );
        assert!(
            SoftBody::new(vec![[0.0; 3]; 3], vec![[0, 1, 2, 3]], 1.0).is_err()
        );
    }

    #[test]
    fn a_dropped_jelly_lands_on_the_floor_and_keeps_its_volume() {
        let settings = SoftBodySettings {
            edge_compliance: 1e-4,
            ..SoftBodySettings::default()
        };
        let mut body =
            SoftBody::block([-0.5, 1.0, -0.5], [4, 4, 4], 0.25, 1000.0)
                .unwrap();
        let mut lowest_volume = f32::MAX;
        for _ in 0..300 {
            body.step(&settings, DT);
            lowest_volume = lowest_volume.min(body.volume());
        }
        let bottom =
            body.positions.iter().map(|p| p[1]).fold(f32::MAX, f32::min);
        let top = body.positions.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
        assert!((0.0..0.01).contains(&bottom), "bottom {bottom}");
        assert!(top > 0.8, "collapsed to {top}");
        assert!(
            lowest_volume > 0.95 * body.rest_volume(),
            "volume fell to {lowest_volume}"
        );
        let speed = body
            .velocities
            .iter()
            .map(|v| vector(*v).norm())
            .fold(0.0, f32::max);
        assert!(speed < 0.2, "still moving at {speed}");
    }

    #[test]
    fn a_pinned_beam_holds_its_end_and_softer_beams_sag_more() {
        let sag = |edge_compliance: f32| {
            let settings = SoftBodySettings {
                edge_compliance,
                volume_compliance: edge_compliance,
                floor: None,
                ..SoftBodySettings::default()
            };
            let mut beam =
                SoftBody::block([0.0, 1.0, 0.0], [8, 1, 1], 0.1, 500.0)
                    .unwrap();
            let pinned: Vec<usize> = (0..beam.positions.len())
                .filter(|&i| beam.positions[i][0] == 0.0)
                .collect();
            for &i in &pinned {
                beam.pin(i);
            }
            let start = beam.positions.clone();
            run(&mut beam, &settings, 240);
            for &i in &pinned {
                assert_eq!(beam.positions[i], start[i]);
            }
            beam.positions
                .iter()
                .map(|p| 1.0 - p[1])
                .fold(0.0, f32::max)
        };
        let stiff = sag(1e-7);
        let soft = sag(1e-4);
        assert!(stiff < 0.1, "a stiff beam sagged {stiff}");
        assert!(soft > stiff * 4.0, "soft {soft} against stiff {stiff}");
    }

    #[test]
    fn the_same_start_gives_the_same_bits() {
        let settings = SoftBodySettings {
            edge_compliance: 1e-4,
            volume_compliance: 1e-5,
            ..SoftBodySettings::default()
        };
        let make = || {
            let mut body =
                SoftBody::block([0.0, 0.5, 0.0], [3, 2, 3], 0.2, 800.0)
                    .unwrap();
            body.velocities[5] = [1.0, 2.0, -0.5];
            body
        };
        let (mut a, mut b) = (make(), make());
        run(&mut a, &settings, 60);
        run(&mut b, &settings, 60);
        assert_eq!(a.state_hash(), b.state_hash());
        assert!(a.positions.iter().flatten().all(|v| v.is_finite()));
    }
}
