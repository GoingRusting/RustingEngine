//! Deterministic placeholder textures: a preset name and a seed give the
//! same tileable image on every machine, so `rusting asset generate <root>
//! texture "bricks 3"` makes a surface that is not a flat grey without a
//! generator hook.

use image::{Rgba, RgbaImage};

/// Preset names `synth` accepts.
pub const PRESETS: [&str; 6] =
    ["grid", "checker", "bricks", "planks", "tiles", "noise"];

/// Width and height of every texture; each pattern's period divides it, so
/// the image tiles without a seam.
pub const SIZE: u32 = 256;

/// One muted palette shared by every preset, so generated textures look
/// like one set. The seed picks the colour: seed 1 is the first.
const PALETTE: [[f32; 3]; 8] = [
    [0.86, 0.52, 0.24], // orange
    [0.36, 0.55, 0.78], // blue
    [0.45, 0.68, 0.40], // green
    [0.72, 0.42, 0.62], // purple
    [0.80, 0.70, 0.36], // sand
    [0.70, 0.36, 0.33], // brick red
    [0.42, 0.64, 0.64], // teal
    [0.58, 0.58, 0.60], // grey
];

/// Seeded splitmix64 hash of a lattice point; never the global RNG.
fn hash(seed: u64, x: u32, y: u32) -> f32 {
    let mut z = seed
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(u64::from(x) << 32 | u64::from(y));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 40) as f32 / (1u64 << 24) as f32
}

/// Smooth value noise in 0..1 on a lattice of `cells` per side, wrapped so
/// it tiles.
fn noise(seed: u64, x: u32, y: u32, cells: u32) -> f32 {
    let step = SIZE / cells;
    let (cx, cy) = (x / step, y / step);
    let fade = |t: f32| t * t * (3.0 - 2.0 * t);
    let (fx, fy) = (
        fade((x % step) as f32 / step as f32),
        fade((y % step) as f32 / step as f32),
    );
    let at = |i: u32, j: u32| hash(seed, (cx + i) % cells, (cy + j) % cells);
    let top = at(0, 0) + (at(1, 0) - at(0, 0)) * fx;
    let bottom = at(0, 1) + (at(1, 1) - at(0, 1)) * fx;
    top + (bottom - top) * fy
}

/// A `SIZE` square tileable RGBA image for `preset`, or `None` for an
/// unknown preset.
#[must_use]
pub fn synth(preset: &str, seed: u64) -> Option<RgbaImage> {
    if !PRESETS.contains(&preset) {
        return None;
    }
    let base = PALETTE[(seed.wrapping_sub(1) % PALETTE.len() as u64) as usize];
    Some(RgbaImage::from_fn(SIZE, SIZE, |x, y| {
        // Fine grain on every preset so no surface is perfectly flat.
        let grain = noise(seed, x, y, 64) * 0.08 - 0.04;
        let shade = grain
            + match preset {
                // Prototype grid: 4 major cells, 16 minor lines.
                "grid" => {
                    let line = |v: u32, period: u32, width: u32| {
                        v % period < width || v % period >= period - width
                    };
                    if line(x, 64, 2) || line(y, 64, 2) {
                        0.35
                    } else if line(x, 16, 1) || line(y, 16, 1) {
                        0.15
                    } else {
                        0.0
                    }
                }
                "checker" => {
                    if (x / 32 + y / 32) % 2 == 0 {
                        0.12
                    } else {
                        -0.12
                    }
                }
                // Running bond: rows of 32 px, every other row shifted half
                // a 64 px brick, with dark mortar.
                "bricks" => {
                    let row = y / 32;
                    let shifted = x + if row % 2 == 1 { 32 } else { 0 };
                    let brick = (shifted % SIZE) / 64;
                    if y % 32 < 3 || shifted % 64 < 3 {
                        -0.45
                    } else {
                        hash(seed, brick, row) * 0.2 - 0.1
                    }
                }
                // Vertical boards 32 px wide with grain along them.
                "planks" => {
                    let board = x / 32;
                    if x % 32 < 2 {
                        -0.4
                    } else {
                        hash(seed, board, 0) * 0.16 - 0.08
                            + (noise(seed ^ board as u64, x, y, 8) - 0.5) * 0.15
                    }
                }
                // Square floor tiles 64 px with light grout.
                "tiles" => {
                    if x % 64 < 2 || y % 64 < 2 {
                        0.3
                    } else {
                        hash(seed, x / 64, y / 64) * 0.1 - 0.05
                    }
                }
                // Mottled ground: two octaves of value noise.
                _ => {
                    (noise(seed, x, y, 8) - 0.5) * 0.35
                        + (noise(seed ^ 7, x, y, 32) - 0.5) * 0.15
                }
            };
        let channel = |value: f32| {
            ((value * (1.0 + shade)).clamp(0.0, 1.0) * 255.0).round() as u8
        };
        Rgba([channel(base[0]), channel(base[1]), channel(base[2]), 255])
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_seeded_tileable_and_not_flat() {
        for preset in PRESETS {
            let one = synth(preset, 1).unwrap();
            assert_eq!(one, synth(preset, 1).unwrap(), "{preset} repeats");
            assert_ne!(one, synth(preset, 2).unwrap(), "{preset} seeds differ");
            let lumas: Vec<u32> = one
                .pixels()
                .map(|p| u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]))
                .collect();
            let (min, max) = (lumas.iter().min(), lumas.iter().max());
            assert!(max.unwrap() - min.unwrap() > 30, "{preset} is flat");
            // Tiling: the step across the wrap edge is about the size of the
            // largest step inside the image. The margin allows for a tile
            // or brick whose own shade sits next to grout only at the edge.
            let step = |a: &Rgba<u8>, b: &Rgba<u8>| {
                (0..3).map(|c| a[c].abs_diff(b[c])).max().unwrap()
            };
            let inner = (0..SIZE)
                .flat_map(|y| (1..SIZE).map(move |x| (x, y)))
                .map(|(x, y)| {
                    step(one.get_pixel(x - 1, y), one.get_pixel(x, y))
                })
                .max()
                .unwrap();
            for y in 0..SIZE {
                let edge =
                    step(one.get_pixel(SIZE - 1, y), one.get_pixel(0, y));
                assert!(edge <= inner + 32, "{preset} has a seam at row {y}");
            }
        }
        assert!(synth("marble", 1).is_none());
    }
}
