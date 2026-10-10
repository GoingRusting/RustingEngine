//! Deterministic sfxr-style sound effects: a preset name and a seed give the
//! same samples on every machine, so `rusting asset generate <root> sfx
//! "coin 7"` makes a placeholder sound without a generator hook.

use crate::audio_output::MIX_RATE;

/// Clip paths starting with this play a built-in sound instead of a file:
/// `sfx:coin` or `sfx:coin 7` (preset, then an optional seed, default 1).
pub const CLIP_PREFIX: &str = "sfx:";

/// Samples for a built-in clip path such as `sfx:coin 7`, or `None` when
/// the path is not one or names an unknown preset or a bad seed.
#[must_use]
pub fn clip(path: &str) -> Option<Vec<f32>> {
    let mut words = path.strip_prefix(CLIP_PREFIX)?.split_whitespace();
    let preset = words.next()?;
    let seed = words.next().map_or(Some(1), |seed| seed.parse().ok())?;
    words.next().is_none().then_some(())?;
    synth(preset, seed)
}

/// Preset names `synth` accepts.
pub const PRESETS: [&str; 7] = [
    "jump",
    "coin",
    "hit",
    "explosion",
    "laser",
    "powerup",
    "blip",
];

#[derive(Clone, Copy)]
enum Wave {
    Square,
    Saw,
    Sine,
    Noise,
}

/// Seeded splitmix64; never the global RNG, so output stays reproducible.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `low..high`.
    fn range(&mut self, low: f32, high: f32) -> f32 {
        let unit = (self.next() >> 40) as f32 / (1u64 << 24) as f32;
        low + (high - low) * unit
    }
}

/// Interleaved stereo samples at `MIX_RATE` for `preset`, or `None` for
/// an unknown preset. Each preset draws its pitch, slide and envelope from
/// ranges with `seed`.
#[must_use]
pub fn synth(preset: &str, seed: u64) -> Option<Vec<f32>> {
    let mut rng = Rng(seed ^ 0x5F5F_5F5F);
    // (wave, start Hz, end Hz, attack s, sustain s, decay s, arpeggio step
    // at half the sound as a pitch ratio, or 1)
    let (wave, start, end, attack, sustain, decay, arp) = match preset {
        "jump" => {
            let start = rng.range(250.0, 450.0);
            let rise = rng.range(1.6, 2.4);
            (Wave::Square, start, start * rise, 0.0, 0.08, 0.15, 1.0)
        }
        "coin" => {
            let start = rng.range(700.0, 1100.0);
            (
                Wave::Square,
                start,
                start,
                0.0,
                0.05,
                rng.range(0.15, 0.3),
                rng.range(1.3, 1.6),
            )
        }
        "hit" => {
            let start = rng.range(300.0, 600.0);
            (
                Wave::Saw,
                start,
                start * 0.3,
                0.0,
                0.02,
                rng.range(0.1, 0.18),
                1.0,
            )
        }
        "explosion" => (
            Wave::Noise,
            rng.range(60.0, 140.0),
            rng.range(20.0, 40.0),
            0.0,
            rng.range(0.05, 0.15),
            rng.range(0.4, 0.7),
            1.0,
        ),
        "laser" => {
            let start = rng.range(900.0, 1600.0);
            (
                Wave::Saw,
                start,
                start * rng.range(0.15, 0.3),
                0.0,
                0.05,
                0.12,
                1.0,
            )
        }
        "powerup" => {
            let start = rng.range(300.0, 500.0);
            (
                Wave::Sine,
                start,
                start * 2.5,
                0.01,
                0.2,
                0.2,
                rng.range(1.2, 1.5),
            )
        }
        "blip" => {
            let start = rng.range(500.0, 900.0);
            (Wave::Square, start, start, 0.0, 0.03, 0.04, 1.0)
        }
        _ => return None,
    };
    let rate = MIX_RATE as f32;
    let length = attack + sustain + decay;
    let frames = (length * rate) as usize;
    let mut samples = Vec::with_capacity(frames * 2);
    let mut phase = 0.0f32;
    let mut noise = 0.0f32;
    for frame in 0..frames {
        let t = frame as f32 / rate;
        let progress = t / length;
        let mut frequency = start * (end / start).powf(progress);
        if progress >= 0.5 {
            frequency *= arp;
        }
        let before = phase;
        phase = (phase + frequency / rate).fract();
        if phase < before {
            noise = rng.range(-1.0, 1.0);
        }
        let value = match wave {
            Wave::Square => {
                if phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            Wave::Saw => 2.0 * phase - 1.0,
            Wave::Sine => (phase * std::f32::consts::TAU).sin(),
            Wave::Noise => noise,
        };
        let envelope = if t < attack {
            t / attack
        } else if t < attack + sustain {
            1.0
        } else {
            1.0 - (t - attack - sustain) / decay
        };
        let sample = value * envelope * 0.4;
        samples.extend([sample, sample]);
    }
    Some(samples)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sfx_clip_paths_name_a_preset_and_an_optional_seed() {
        assert_eq!(clip("sfx:coin"), synth("coin", 1));
        assert_eq!(clip("sfx:coin 7"), synth("coin", 7));
        assert!(clip("sfx:coin 7").is_some());
        for bad in ["sfx:", "sfx:nope", "sfx:coin x", "sfx:coin 7 8", "coin"] {
            assert_eq!(clip(bad), None, "{bad}");
        }
    }

    #[test]
    fn presets_are_seeded_short_and_quiet_enough() {
        for preset in PRESETS {
            let a = synth(preset, 7).unwrap();
            assert_eq!(a, synth(preset, 7).unwrap(), "{preset} repeats");
            assert_ne!(a, synth(preset, 8).unwrap(), "{preset} varies");
            let seconds = a.len() as f32 / 2.0 / MIX_RATE as f32;
            assert!((0.05..1.0).contains(&seconds), "{preset}: {seconds}");
            let peak = a.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            assert!(peak > 0.1 && peak <= 0.4, "{preset}: {peak}");
            // Fades out instead of ending on a click.
            assert!(a[a.len() - 1].abs() < 0.02, "{preset}");
        }
        assert!(synth("moo", 1).is_none());
    }
}
