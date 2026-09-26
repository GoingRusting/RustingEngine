//! Repeatable physics benchmark scenes.
//!
//! Each scene is a pure function of its kind and body count: the same
//! arguments give the same bodies, bit for bit, on every machine. Bodies are
//! unit cubes (1 m, `Collider::default()`) laid out around the origin above a
//! ground plane at `y = 0`; the caller supplies the ground and the renderer.

use super::PhysicsSolver;
use crate::Transform;

/// One benchmark scene layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhysicsBenchmark {
    /// Separated cubes in a block that drop onto the ground.
    Falling,
    /// Towers of ten touching cubes resting on the ground.
    Stacking,
    /// A tight clump of tilted cubes flying apart and piling up.
    Debris,
    /// The falling block with Full, Simplified, Space and NoCollision
    /// solvers interleaved body by body.
    Mixed,
}

/// One body of a benchmark scene.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BenchmarkBody {
    pub transform: Transform,
    pub linear_velocity: [f32; 3],
    pub solver: PhysicsSolver,
}

/// Cubes per tower in [`PhysicsBenchmark::Stacking`].
pub const BENCHMARK_TOWER_HEIGHT: usize = 10;

impl PhysicsBenchmark {
    pub const ALL: [Self; 4] =
        [Self::Falling, Self::Stacking, Self::Debris, Self::Mixed];

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Falling => "falling",
            Self::Stacking => "stacking",
            Self::Debris => "debris",
            Self::Mixed => "mixed",
        }
    }

    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|scene| scene.name() == name)
    }

    /// Returns `count` bodies in a fixed order.
    #[must_use]
    pub fn bodies(self, count: usize) -> Vec<BenchmarkBody> {
        let body = |position, solver| BenchmarkBody {
            transform: Transform::new(position),
            linear_velocity: [0.0; 3],
            solver,
        };
        match self {
            Self::Falling | Self::Mixed => {
                let side = square_side(count.div_ceil(10));
                (0..count)
                    .map(|index| {
                        let [x, z] = grid(index % (side * side), side, 1.5);
                        let layer = (index / (side * side)) as f32;
                        let solver = if self == Self::Mixed {
                            [
                                PhysicsSolver::Full,
                                PhysicsSolver::Simplified,
                                PhysicsSolver::Space,
                                PhysicsSolver::NoCollision,
                            ][index % 4]
                        } else {
                            PhysicsSolver::Full
                        };
                        body([x, 5.0 + 1.5 * layer, z], solver)
                    })
                    .collect()
            }
            Self::Stacking => {
                let towers = count.div_ceil(BENCHMARK_TOWER_HEIGHT);
                let side = square_side(towers);
                (0..count)
                    .map(|index| {
                        let tower = index / BENCHMARK_TOWER_HEIGHT;
                        let level = (index % BENCHMARK_TOWER_HEIGHT) as f32;
                        let [x, z] = grid(tower, side, 2.0);
                        body([x, 0.5 + level, z], PhysicsSolver::Full)
                    })
                    .collect()
            }
            Self::Debris => {
                let side = (count as f64).cbrt().ceil().max(1.0) as usize;
                (0..count)
                    .map(|index| {
                        // 1.8 m clears two unit cubes at any tilt (2 × √3 / 2).
                        let [x, z] = grid(index % (side * side), side, 1.8);
                        let layer = (index / (side * side)) as f32;
                        let mut body = body(
                            [x, 1.0 + 1.8 * layer, z],
                            PhysicsSolver::Full,
                        );
                        body.transform.rotation =
                            [0, 1, 2].map(|axis| 0.4 * jitter(index, axis));
                        // Outward from the clump's axis, plus some lift.
                        body.linear_velocity = [
                            0.5 * x + 2.0 * jitter(index, 3),
                            4.0 + 2.0 * jitter(index, 4),
                            0.5 * z + 2.0 * jitter(index, 5),
                        ];
                        body
                    })
                    .collect()
            }
        }
    }
}

fn square_side(cells: usize) -> usize {
    (cells as f64).sqrt().ceil().max(1.0) as usize
}

/// Position of cell `index` in a `side` × `side` grid centred on the origin.
fn grid(index: usize, side: usize, spacing: f32) -> [f32; 2] {
    let centre = (side - 1) as f32 / 2.0;
    [
        ((index % side) as f32 - centre) * spacing,
        ((index / side) as f32 - centre) * spacing,
    ]
}

/// A fixed value in [-1, 1] for one body and channel (SplitMix64 finaliser),
/// so the layout never reads a random number generator.
fn jitter(index: usize, channel: u64) -> f32 {
    let mut value = (index as u64) ^ (channel << 56);
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^= value >> 31;
    (value >> 40) as f32 / (1u64 << 23) as f32 - 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scenes_are_repeatable_at_every_size() {
        for scene in PhysicsBenchmark::ALL {
            assert_eq!(PhysicsBenchmark::from_name(scene.name()), Some(scene));
            for count in [1_000, 10_000, 100_000] {
                let bodies = scene.bodies(count);
                assert_eq!(bodies.len(), count);
                assert_eq!(bodies, scene.bodies(count), "{scene:?} {count}");
                assert!(bodies.iter().all(|body| {
                    body.transform.position.iter().all(|v| v.is_finite())
                        && body.transform.position[1] >= 0.5
                }));
            }
        }
    }

    #[test]
    fn scenes_start_without_overlaps_and_mix_their_solvers() {
        for scene in PhysicsBenchmark::ALL {
            let bodies = scene.bodies(1_000);
            // Untilted unit cubes never overlap when their centres are 1 m
            // apart on some axis; tilted debris cubes are 1.8 m apart.
            for (a, first) in bodies.iter().enumerate() {
                for second in &bodies[a + 1..] {
                    let apart = (0..3).any(|axis| {
                        (first.transform.position[axis]
                            - second.transform.position[axis])
                            .abs()
                            >= 0.999
                    });
                    assert!(apart, "{scene:?}: {first:?} and {second:?}");
                }
            }
        }
        let stacking = PhysicsBenchmark::Stacking.bodies(20);
        assert_eq!(stacking[9].transform.position[1], 9.5);
        assert_eq!(stacking[10].transform.position[1], 0.5);
        let mixed = PhysicsBenchmark::Mixed.bodies(8);
        let solvers: Vec<_> = mixed.iter().map(|body| body.solver).collect();
        assert_eq!(solvers[..4], solvers[4..]);
        assert!(solvers.contains(&PhysicsSolver::NoCollision));
        let debris = PhysicsBenchmark::Debris.bodies(100);
        assert!(debris.iter().all(|body| body.linear_velocity[1] >= 2.0));
    }
}
