//! The GPU form of [`Cloth`] and [`SoftBody`]:
//! `src/shaders/compute/cloth.comp`, run by
//! [`crate::rendering::gpu_cloth::GpuClothRunner`], steps the particles in
//! one workgroup, and [`step_on_cpu`] is its bit-exact Rust reference.
//!
//! The constraints are colored so that no two of one color share a
//! particle. The shader solves each color in parallel with a barrier
//! between colors, which gives the same bits as solving the constraints
//! one by one in color order, as [`step_on_cpu`] does. All math goes
//! through [`super::sim_math`]: no atomics, no fused operations.
//!
//! The kernel covers gravity, damping, pins, distance constraints (cloth
//! stretch and bend, soft-body edges), tetrahedron volume constraints and
//! the floor. Wind, self-collision, tearing and rigid-body anchors stay
//! CPU-only for now. [`GpuCloth::floor_event`] reports floor contact through
//! the same [`super::GpuPhysicsEvent`] queue as rigid GPU bodies.

use bevy_ecs::prelude::Component;

use super::cloth::{Cloth, ClothSettings, MAX_CLOTH_PARTICLES};
use super::hybrid_physics::GpuEventId;
use super::sim_math::{dot, length, recip};
use super::soft_body::{SoftBody, SoftBodySettings, MAX_SOFT_BODY_SUBSTEPS};

/// Steps the [`super::ClothVolume`] or [`super::SoftBodyVolume`] on this
/// entity on the GPU instead of in the fixed tick.
/// `rendering::gpu_cloth::service_gpu_cloths` submits it and writes
/// finished particles back into the volume, so the volume shows the body
/// one to three frames behind the fixed tick, as `tick` records. Only the
/// kernel's features apply: attachments, obstacles, wind, self-collision
/// and tearing are ignored while this is on.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GpuCloth {
    /// Fixed tick the volume's particles show; `None` before the first run.
    pub tick: Option<u64>,
    /// A submission for this cloth is still on the GPU.
    pub in_flight: bool,
    /// Registered event sent when a free particle first reaches the floor,
    /// with a [`super::GpuEventPayload::Contact`] payload of the touching
    /// particle's position and index; see [`floor_contact`].
    pub floor_event: Option<GpuEventId>,
    /// A free particle rested on the floor at `tick`.
    pub on_floor: bool,
}

/// [`GpuCloth`] under the name that reads right on a soft body.
pub type GpuSoftBody = GpuCloth;

/// Words before the color tables; see the header comment in `cloth.comp`.
const HEADER_WORDS: usize = 17;
const CONSTRAINT_WORDS: usize = 4;
const VOLUME_WORDS: usize = 6;
const PARTICLE_WORDS: usize = 10;
/// The shader's `local_size_x`.
pub const GPU_CLOTH_LANES: u32 = 256;
/// 1 / 6 rounded to `f32`, written as bits in `cloth.comp`.
const SIXTH: f32 = f32::from_bits(0x3E2A_AAAB);
/// For each tetrahedron corner, the corners whose cross product (from the
/// first) gives that corner's outward volume gradient.
const OPPOSITE: [[usize; 3]; 4] = [[1, 3, 2], [0, 2, 3], [0, 3, 1], [0, 1, 2]];

/// Everything `cloth.comp` steps, in the form [`pack_parts`] packs.
struct Parts<'a> {
    positions: &'a [[f32; 3]],
    velocities: &'a [[f32; 3]],
    inverse_masses: &'a [f32],
    distances: Vec<([u32; 2], f32, f32)>,
    volumes: Vec<([u32; 4], f32, f32)>,
    substeps: u32,
    gravity: [f32; 3],
    damping: f32,
    floor: Option<f32>,
}

/// Packs `cloth` into the word buffer `cloth.comp` reads and writes, set to
/// advance `steps` steps of `dt` seconds.
pub fn pack(
    cloth: &Cloth,
    settings: &ClothSettings,
    dt: f32,
    steps: u32,
) -> Result<Vec<u32>, String> {
    let count = cloth.positions.len();
    if count as u64 > MAX_CLOTH_PARTICLES {
        return Err(format!(
            "{count} particles is more than the {MAX_CLOTH_PARTICLES} the GPU \
             kernel takes"
        ));
    }
    let parts = Parts {
        positions: &cloth.positions,
        velocities: &cloth.velocities,
        inverse_masses: &cloth.inverse_masses,
        distances: cloth.distance_constraints(settings),
        volumes: Vec::new(),
        substeps: settings.substeps,
        gravity: settings.gravity,
        damping: settings.damping,
        floor: settings.floor,
    };
    pack_parts(parts, dt, steps)
}

/// Packs a soft body's particles, edges and tetrahedra like [`pack`]. The
/// kernel has no anchors, obstacles or tearing.
pub fn pack_soft_body(
    body: &SoftBody,
    settings: &SoftBodySettings,
    dt: f32,
    steps: u32,
) -> Result<Vec<u32>, String> {
    let (lengths, volumes) = body.rests();
    let parts = Parts {
        positions: &body.positions,
        velocities: &body.velocities,
        inverse_masses: &body.inverse_masses,
        distances: body
            .edges
            .iter()
            .zip(lengths)
            .map(|(&pair, &rest)| (pair, rest, settings.edge_compliance))
            .collect(),
        volumes: body
            .tetrahedra
            .iter()
            .zip(volumes)
            .map(|(&tet, &rest)| (tet, rest, settings.volume_compliance))
            .collect(),
        substeps: settings.substeps,
        gravity: settings.gravity,
        damping: settings.damping,
        floor: settings.floor,
    };
    pack_parts(parts, dt, steps)
}

/// Greedy coloring in constraint order, then a stable sort by color, so no
/// two constraints of one color share a particle. Returns the color table
/// (starts, one more than the colors) and the constraint words.
fn color<const N: usize>(
    constraints: Vec<([u32; N], f32, f32)>,
    count: usize,
    alpha_scale: f32,
) -> Result<(Vec<u32>, Vec<u32>), String> {
    let mut used = vec![0u64; count];
    let mut colored = Vec::with_capacity(constraints.len());
    for (particles, rest, compliance) in constraints {
        let taken = particles
            .iter()
            .fold(0, |taken, &i| taken | used[i as usize]);
        let color = taken.trailing_ones();
        if color == 64 {
            return Err(format!("{particles:?} needs over 64 colors"));
        }
        for i in particles {
            used[i as usize] |= 1 << color;
        }
        let words = particles
            .into_iter()
            .chain([rest.to_bits(), (compliance * alpha_scale).to_bits()]);
        colored.push((color, words.collect::<Vec<_>>()));
    }
    colored.sort_by_key(|&(color, _)| color);
    let colors = colored.last().map_or(0, |&(color, _)| color as usize + 1);
    let mut starts = vec![0u32; colors + 1];
    for &(color, _) in &colored {
        starts[color as usize + 1] += 1;
    }
    for color in 0..colors {
        starts[color + 1] += starts[color];
    }
    Ok((starts, colored.into_iter().flat_map(|(_, w)| w).collect()))
}

fn pack_parts(parts: Parts, dt: f32, steps: u32) -> Result<Vec<u32>, String> {
    let count = parts.positions.len();
    let substeps = parts.substeps.clamp(1, MAX_SOFT_BODY_SUBSTEPS);
    let h = dt / substeps as f32;
    let alpha_scale = recip(h * h);
    let (distance_starts, distances) =
        color(parts.distances, count, alpha_scale)?;
    let (volume_starts, volumes) = color(parts.volumes, count, alpha_scale)?;

    let distance_table = HEADER_WORDS;
    let distance_base = distance_table + distance_starts.len();
    let volume_table = distance_base + distances.len();
    let volume_base = volume_table + volume_starts.len();
    let particle_base = volume_base + volumes.len();
    let keep = (1.0 - parts.damping * h).clamp(0.0, 1.0);
    let [gx, gy, gz] = parts.gravity;
    let mut words = vec![
        count as u32,
        distance_starts.len() as u32 - 1,
        substeps,
        steps,
        u32::from(parts.floor.is_some()),
        h.to_bits(),
        keep.to_bits(),
        gx.to_bits(),
        gy.to_bits(),
        gz.to_bits(),
        parts.floor.unwrap_or(0.0).to_bits(),
        volume_starts.len() as u32 - 1,
        distance_table as u32,
        distance_base as u32,
        particle_base as u32,
        volume_table as u32,
        volume_base as u32,
    ];
    words.extend(distance_starts);
    words.extend(distances);
    words.extend(volume_starts);
    words.extend(volumes);
    for i in 0..count {
        words.extend(
            parts.positions[i]
                .into_iter()
                .chain(parts.velocities[i])
                .chain([parts.inverse_masses[i]])
                .chain(parts.positions[i])
                .map(f32::to_bits),
        );
    }
    Ok(words)
}

/// Copies the positions and velocities in `words` back into `cloth`.
pub fn unpack(words: &[u32], cloth: &mut Cloth) {
    unpack_parts(words, &mut cloth.positions, &mut cloth.velocities);
}

/// Copies the positions and velocities in `words` back into `body`.
pub fn unpack_soft_body(words: &[u32], body: &mut SoftBody) {
    unpack_parts(words, &mut body.positions, &mut body.velocities);
}

fn unpack_parts(
    words: &[u32],
    positions: &mut [[f32; 3]],
    velocities: &mut [[f32; 3]],
) {
    let base = words[14] as usize;
    let float = |index: usize| f32::from_bits(words[index]);
    for i in 0..positions.len() {
        let p = base + i * PARTICLE_WORDS;
        positions[i] = [float(p), float(p + 1), float(p + 2)];
        velocities[i] = [float(p + 3), float(p + 4), float(p + 5)];
    }
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// The lowest-index free particle in `words` that lies on or below the
/// floor, with its position, or `None` without a floor or contact.
#[must_use]
pub fn floor_contact(words: &[u32]) -> Option<(u32, [f32; 3])> {
    if words[4] == 0 {
        return None;
    }
    let floor = f32::from_bits(words[10]);
    let base = words[14] as usize;
    (0..words[0]).find_map(|i| {
        let p = base + i as usize * PARTICLE_WORDS;
        let [x, y, z, w] =
            [p, p + 1, p + 2, p + 6].map(|index| f32::from_bits(words[index]));
        (w != 0.0 && y <= floor).then_some((i, [x, y, z]))
    })
}

/// Runs `cloth.comp` on the CPU, one constraint at a time in color order.
pub fn step_on_cpu(words: &mut [u32]) {
    let float = |words: &[u32], index: usize| f32::from_bits(words[index]);
    let read = |words: &[u32], index: usize| {
        [index, index + 1, index + 2].map(|i| float(words, i))
    };
    let write = |words: &mut [u32], index: usize, v: [f32; 3]| {
        for (offset, value) in v.into_iter().enumerate() {
            words[index + offset] = value.to_bits();
        }
    };
    let [count, colors, substeps, steps, has_floor] =
        [0, 1, 2, 3, 4].map(|index| words[index] as usize);
    let (h, keep) = (float(words, 5), float(words, 6));
    let gravity = read(words, 7);
    let floor = float(words, 10);
    let [color_base, constraint_base, particle_base, volume_table, volume_base] =
        [12, 13, 14, 15, 16].map(|index| words[index] as usize);
    let volume_colors = words[11] as usize;
    let particle = |i: usize| particle_base + i * PARTICLE_WORDS;

    for _ in 0..steps * substeps {
        for i in 0..count {
            let p = particle(i);
            let x = read(words, p);
            write(words, p + 7, x);
            if float(words, p + 6) == 0.0 {
                continue;
            }
            let v = read(words, p + 3);
            let v = [0, 1, 2].map(|k| v[k] * keep + gravity[k] * h);
            write(words, p + 3, v);
            write(words, p, [0, 1, 2].map(|k| x[k] + v[k] * h));
        }
        let first = words[color_base] as usize;
        let last = words[color_base + colors] as usize;
        for k in first..last {
            let q = constraint_base + k * CONSTRAINT_WORDS;
            let (pa, pb) =
                (particle(words[q] as usize), particle(words[q + 1] as usize));
            let (rest, alpha) = (float(words, q + 2), float(words, q + 3));
            let (wa, wb) = (float(words, pa + 6), float(words, pb + 6));
            let (xa, xb) = (read(words, pa), read(words, pb));
            let weight = wa + wb;
            let delta = [0, 1, 2].map(|k| xa[k] - xb[k]);
            let distance = length(delta);
            let denominator = weight + alpha;
            if denominator == 0.0 || distance < 1e-9 {
                continue;
            }
            let inverse = recip(distance);
            let normal = delta.map(|d| d * inverse);
            let lambda = -(distance - rest) * recip(denominator);
            write(
                words,
                pa,
                [0, 1, 2].map(|k| xa[k] + normal[k] * (lambda * wa)),
            );
            write(
                words,
                pb,
                [0, 1, 2].map(|k| xb[k] - normal[k] * (lambda * wb)),
            );
        }
        let first = words[volume_table] as usize;
        let last = words[volume_table + volume_colors] as usize;
        for k in first..last {
            let q = volume_base + k * VOLUME_WORDS;
            let p = [0, 1, 2, 3].map(|j| particle(words[q + j] as usize));
            let (rest, alpha) = (float(words, q + 4), float(words, q + 5));
            let x = p.map(|p| read(words, p));
            let w = p.map(|p| float(words, p + 6));
            let mut gradients = [[0.0; 3]; 4];
            let mut weight = 0.0;
            for (j, [a, b, c]) in OPPOSITE.into_iter().enumerate() {
                let g = cross(sub(x[b], x[a]), sub(x[c], x[a]));
                gradients[j] = g.map(|g| g * SIXTH);
                weight += w[j] * dot(gradients[j], gradients[j]);
            }
            let denominator = weight + alpha;
            if denominator == 0.0 {
                continue;
            }
            let volume =
                dot(cross(sub(x[1], x[0]), sub(x[2], x[0])), sub(x[3], x[0]))
                    * SIXTH;
            let lambda = -(volume - rest) * recip(denominator);
            for j in 0..4 {
                let moved = [0, 1, 2]
                    .map(|k| x[j][k] + gradients[j][k] * (lambda * w[j]));
                write(words, p[j], moved);
            }
        }
        let inverse_h = recip(h);
        for i in 0..count {
            let p = particle(i);
            if float(words, p + 6) == 0.0 {
                continue;
            }
            let mut x = read(words, p);
            let previous = read(words, p + 7);
            if has_floor != 0 && x[1] < floor {
                x = [previous[0], floor, previous[2]];
                write(words, p, x);
            }
            write(
                words,
                p + 3,
                [0, 1, 2].map(|k| (x[k] - previous[k]) * inverse_h),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hanging() -> (Cloth, ClothSettings) {
        let mut cloth = Cloth::grid(
            [0.0, 2.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, -1.0, 0.0],
            [8, 8],
            0.125,
            0.3,
        )
        .unwrap();
        for i in 0..9 {
            cloth.pin(i);
        }
        let settings = ClothSettings {
            substeps: 8,
            ..ClothSettings::default()
        };
        (cloth, settings)
    }

    #[test]
    fn colors_never_share_a_particle_and_the_reference_hangs_the_sheet() {
        let (mut cloth, settings) = hanging();
        let mut words = pack(&cloth, &settings, 1.0 / 60.0, 120).unwrap();
        let colors = words[1] as usize;
        let (color_base, constraint_base) =
            (words[12] as usize, words[13] as usize);
        assert!(colors > 1 && colors < 64, "{colors}");
        for color in 0..colors {
            let mut seen = std::collections::BTreeSet::new();
            for k in words[color_base + color]..words[color_base + color + 1] {
                let q = constraint_base + k as usize * CONSTRAINT_WORDS;
                assert!(seen.insert(words[q]) && seen.insert(words[q + 1]));
            }
        }
        let again = words.clone();
        step_on_cpu(&mut words);
        unpack(&words, &mut cloth);
        assert_eq!(cloth.positions[0], [0.0, 2.0, 0.0]);
        let lowest = cloth.positions.iter().map(|p| p[1]).fold(9.0, f32::min);
        assert!(lowest > 0.9 && lowest < 1.1, "{lowest}");
        assert!(cloth.positions.iter().flatten().all(|x| x.is_finite()));
        let mut repeat = again;
        step_on_cpu(&mut repeat);
        assert_eq!(repeat, words);
        assert_eq!(floor_contact(&words), None);
    }

    #[test]
    fn the_first_free_particle_on_the_floor_is_the_contact() {
        let (mut cloth, mut settings) = hanging();
        cloth.inverse_masses.fill(1.0);
        settings.floor = Some(0.0);
        let mut words = pack(&cloth, &settings, 1.0 / 60.0, 1).unwrap();
        assert_eq!(floor_contact(&words), None);
        words[3] = 120;
        step_on_cpu(&mut words);
        let (index, position) = floor_contact(&words).unwrap();
        // The unpinned sheet drops two seconds onto the floor.
        assert!(index < 81 && position[1] == 0.0, "{index} {position:?}");
    }

    #[test]
    fn the_shader_sixth_is_the_reference_sixth() {
        let shader = include_str!("../shaders/compute/cloth.comp");
        let literal = shader
            .lines()
            .find_map(|line| line.strip_prefix("const float SIXTH = "))
            .unwrap();
        let sixth: f32 = literal.trim_end_matches(';').parse().unwrap();
        assert_eq!(sixth.to_bits(), SIXTH.to_bits());
    }

    #[test]
    fn the_reference_drops_a_soft_block_that_keeps_its_volume() {
        assert_eq!(SIXTH, 1.0 / 6.0);
        let mut body =
            SoftBody::block([-0.5, 1.0, -0.5], [4, 4, 4], 0.25, 1000.0)
                .unwrap();
        let settings = SoftBodySettings::default();
        let mut words =
            pack_soft_body(&body, &settings, 1.0 / 60.0, 120).unwrap();
        assert!(words[11] > 1, "volume colors {}", words[11]);
        let again = words.clone();
        step_on_cpu(&mut words);
        unpack_soft_body(&words, &mut body);
        let lowest = body.positions.iter().map(|p| p[1]).fold(9.0, f32::min);
        assert_eq!(lowest, 0.0, "the block rests on the floor");
        let kept = body.volume() / body.rest_volume();
        assert!((kept - 1.0).abs() < 0.02, "{kept}");
        let mut repeat = again;
        step_on_cpu(&mut repeat);
        assert_eq!(repeat, words);
    }
}
