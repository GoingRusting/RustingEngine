//! The GPU form of [`Cloth`]: `src/shaders/compute/cloth.comp`, run by
//! [`crate::rendering::gpu_cloth::GpuClothRunner`], steps the
//! particles in one workgroup, and [`step_on_cpu`] is its bit-exact Rust
//! reference.
//!
//! The constraints are colored so that no two of one color share a
//! particle. The shader solves each color in parallel with a barrier
//! between colors, which gives the same bits as solving the constraints
//! one by one in color order, as [`step_on_cpu`] does. All math goes
//! through [`super::sim_math`]: no atomics, no fused operations.
//!
//! The kernel covers gravity, damping, pins, stretch and bend constraints
//! and the floor. Wind, self-collision, tearing and rigid-body anchors stay
//! CPU-only for now.

use super::cloth::{Cloth, ClothSettings, MAX_CLOTH_PARTICLES};
use super::sim_math::{length, recip};
use super::soft_body::MAX_SOFT_BODY_SUBSTEPS;

/// Words before the color table; see the header comment in `cloth.comp`.
const HEADER_WORDS: usize = 16;
const CONSTRAINT_WORDS: usize = 4;
const PARTICLE_WORDS: usize = 10;
/// The shader's `local_size_x`.
pub const GPU_CLOTH_LANES: u32 = 256;

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
    let substeps = settings.substeps.clamp(1, MAX_SOFT_BODY_SUBSTEPS);
    let h = dt / substeps as f32;
    let alpha_scale = recip(h * h);
    // Greedy coloring in constraint order, then a stable sort by color.
    let mut used = vec![0u64; count];
    let mut colored = Vec::new();
    for ([a, b], rest, compliance) in cloth.distance_constraints(settings) {
        let taken = used[a as usize] | used[b as usize];
        let color = taken.trailing_ones();
        if color == 64 {
            return Err(format!("particle {a} or {b} needs over 64 colors"));
        }
        used[a as usize] |= 1 << color;
        used[b as usize] |= 1 << color;
        colored.push((
            color,
            [a, b, rest.to_bits(), (compliance * alpha_scale).to_bits()],
        ));
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

    let color_base = HEADER_WORDS;
    let constraint_base = color_base + starts.len();
    let particle_base = constraint_base + colored.len() * CONSTRAINT_WORDS;
    let keep = (1.0 - settings.damping * h).clamp(0.0, 1.0);
    let [gx, gy, gz] = settings.gravity;
    let mut words = vec![
        count as u32,
        colors as u32,
        substeps,
        steps,
        u32::from(settings.floor.is_some()),
        h.to_bits(),
        keep.to_bits(),
        gx.to_bits(),
        gy.to_bits(),
        gz.to_bits(),
        settings.floor.unwrap_or(0.0).to_bits(),
        0,
        color_base as u32,
        constraint_base as u32,
        particle_base as u32,
        0,
    ];
    words.extend(starts);
    words.extend(colored.into_iter().flat_map(|(_, words)| words));
    for i in 0..count {
        words.extend(
            cloth.positions[i]
                .into_iter()
                .chain(cloth.velocities[i])
                .chain([cloth.inverse_masses[i]])
                .chain(cloth.positions[i])
                .map(f32::to_bits),
        );
    }
    Ok(words)
}

/// Copies the positions and velocities in `words` back into `cloth`.
pub fn unpack(words: &[u32], cloth: &mut Cloth) {
    let base = words[14] as usize;
    let float = |index: usize| f32::from_bits(words[index]);
    for i in 0..cloth.positions.len() {
        let p = base + i * PARTICLE_WORDS;
        cloth.positions[i] = [float(p), float(p + 1), float(p + 2)];
        cloth.velocities[i] = [float(p + 3), float(p + 4), float(p + 5)];
    }
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
    let [color_base, constraint_base, particle_base] =
        [12, 13, 14].map(|index| words[index] as usize);
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
    }
}
