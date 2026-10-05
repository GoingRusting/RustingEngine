//! Ready-made particle effects for `rusting effect apply` and the editor's
//! preset menu. Each one is an ordinary [`ParticleEmitter`]; every value
//! stays editable after it is applied.

use std::f32::consts::{PI, TAU};

use super::{
    ColorKey, CurveKey, EmitterShape, ParticleBlend, ParticleBurst,
    ParticleEmitter, ParticleFacing, ParticleSprite,
};

/// One named effect.
pub struct EffectPreset {
    pub name: &'static str,
    pub summary: &'static str,
    /// Suggested height of the emitter above the ground, in metres.
    pub height: f32,
    build: fn() -> ParticleEmitter,
}

impl EffectPreset {
    #[must_use]
    pub fn emitter(&self) -> ParticleEmitter {
        (self.build)()
    }
}

/// Finds a preset by name.
#[must_use]
pub fn effect_preset(name: &str) -> Option<&'static EffectPreset> {
    EFFECT_PRESETS.iter().find(|preset| preset.name == name)
}

fn color(t: f32, color: [f32; 4]) -> ColorKey {
    ColorKey { t, color }
}

fn key(t: f32, value: f32) -> CurveKey {
    CurveKey { t, value }
}

pub const EFFECT_PRESETS: &[EffectPreset] = &[
    EffectPreset {
        name: "dust_motes",
        summary: "Slow glowing specks drifting in a room or a sunbeam.",
        height: 1.5,
        build: dust_motes,
    },
    EffectPreset {
        name: "falling_leaves",
        summary: "Autumn leaves tumbling down over a 10 m square.",
        height: 6.0,
        build: falling_leaves,
    },
    EffectPreset {
        name: "snow",
        summary: "Snowflakes drifting down over a 16 m square.",
        height: 8.0,
        build: snow,
    },
    EffectPreset {
        name: "rain",
        summary: "Rain streaks over a 16 m square.",
        height: 10.0,
        build: rain,
    },
    EffectPreset {
        name: "sparks",
        summary: "One burst of hot sparks; `scene.trigger` replays it.",
        height: 0.5,
        build: sparks,
    },
    EffectPreset {
        name: "smoke",
        summary:
            "Soft grey smoke rising and spreading, for chimneys and fires.",
        height: 0.0,
        build: smoke,
    },
    EffectPreset {
        name: "fire",
        summary: "A campfire flame about a metre tall.",
        height: 0.0,
        build: fire,
    },
    EffectPreset {
        name: "embers",
        summary: "Glowing embers rising and swirling above a fire.",
        height: 0.2,
        build: embers,
    },
    EffectPreset {
        name: "fireflies",
        summary: "Blinking yellow-green lights wandering over a 6 m square.",
        height: 1.0,
        build: fireflies,
    },
    EffectPreset {
        name: "magic_sparkle",
        summary: "Twinkling violet and cyan sparkles around a magic object.",
        height: 1.0,
        build: magic_sparkle,
    },
    EffectPreset {
        name: "confetti",
        summary: "One burst of colored paper; `scene.trigger` replays it.",
        height: 0.5,
        build: confetti,
    },
];

fn dust_motes() -> ParticleEmitter {
    ParticleEmitter {
        rate: 8.0,
        max_particles: 200,
        prewarm: true,
        duration: 10.0,
        shape: EmitterShape::Box,
        shape_size: [3.0, 1.5, 3.0],
        lifetime: [6.0, 10.0],
        speed: [0.02, 0.08],
        size: [0.03, 0.06],
        spread: PI,
        turbulence: 0.08,
        turbulence_frequency: 0.6,
        color_over_life: vec![color(0.0, [1.0, 0.85, 0.6, 0.7])],
        emissive: 1.2,
        fade_in: 0.3,
        fade_out: 0.3,
        blend: ParticleBlend::Additive,
        ..ParticleEmitter::default()
    }
}

fn falling_leaves() -> ParticleEmitter {
    ParticleEmitter {
        rate: 6.0,
        max_particles: 200,
        prewarm: true,
        duration: 10.0,
        shape: EmitterShape::Box,
        shape_size: [5.0, 0.2, 5.0],
        lifetime: [7.0, 10.0],
        speed: [0.0, 0.3],
        size: [0.1, 0.16],
        rotation: [0.0, TAU],
        spin: [-2.5, 2.5],
        direction: [0.0, -1.0, 0.0],
        spread: 0.5,
        gravity: 0.8,
        drag: 0.9,
        wind: [0.4, 0.0, 0.15],
        turbulence: 0.9,
        turbulence_frequency: 0.5,
        start_colors: vec![
            [0.55, 0.22, 0.04, 1.0],
            [0.45, 0.1, 0.03, 1.0],
            [0.6, 0.4, 0.06, 1.0],
            [0.3, 0.16, 0.05, 1.0],
        ],
        fade_in: 0.05,
        fade_out: 0.15,
        sprite: ParticleSprite::Square,
        ..ParticleEmitter::default()
    }
}

fn snow() -> ParticleEmitter {
    ParticleEmitter {
        rate: 150.0,
        max_particles: 2000,
        prewarm: true,
        duration: 10.0,
        shape: EmitterShape::Box,
        shape_size: [8.0, 0.2, 8.0],
        lifetime: [6.0, 7.0],
        speed: [0.0, 0.2],
        size: [0.04, 0.08],
        direction: [0.0, -1.0, 0.0],
        spread: 0.3,
        gravity: 1.4,
        drag: 1.0,
        turbulence: 0.5,
        turbulence_frequency: 0.7,
        color_over_life: vec![color(0.0, [1.0, 1.0, 1.0, 0.9])],
        fade_in: 0.05,
        fade_out: 0.1,
        sprite: ParticleSprite::Disc,
        ..ParticleEmitter::default()
    }
}

fn rain() -> ParticleEmitter {
    ParticleEmitter {
        rate: 800.0,
        max_particles: 1500,
        prewarm: true,
        duration: 10.0,
        shape: EmitterShape::Box,
        shape_size: [8.0, 0.1, 8.0],
        lifetime: [0.8, 1.0],
        speed: [10.0, 12.0],
        size: [0.008, 0.012],
        direction: [0.05, -1.0, 0.0],
        spread: 0.02,
        gravity: 9.8,
        color_over_life: vec![color(0.0, [0.75, 0.82, 0.92, 0.35])],
        fade_in: 0.0,
        fade_out: 0.05,
        facing: ParticleFacing::Velocity,
        stretch: 0.03,
        sprite: ParticleSprite::Square,
        ..ParticleEmitter::default()
    }
}

fn sparks() -> ParticleEmitter {
    ParticleEmitter {
        rate: 0.0,
        bursts: vec![ParticleBurst {
            time: 0.0,
            count: 60,
        }],
        max_particles: 200,
        duration: 1.0,
        looping: false,
        lifetime: [0.3, 0.8],
        speed: [4.0, 9.0],
        size: [0.03, 0.05],
        spread: 0.9,
        gravity: 9.8,
        drag: 0.5,
        color_over_life: vec![
            color(0.0, [1.0, 0.8, 0.45, 1.0]),
            color(0.4, [1.0, 0.45, 0.1, 1.0]),
            color(1.0, [0.7, 0.12, 0.02, 1.0]),
        ],
        emissive: 2.0,
        fade_in: 0.0,
        fade_out: 0.3,
        facing: ParticleFacing::Velocity,
        stretch: 0.03,
        blend: ParticleBlend::Additive,
        sprite: ParticleSprite::Disc,
        ..ParticleEmitter::default()
    }
}

fn smoke() -> ParticleEmitter {
    ParticleEmitter {
        rate: 12.0,
        max_particles: 100,
        prewarm: true,
        shape: EmitterShape::Cone,
        shape_size: [0.3, 0.3, 0.3],
        lifetime: [3.0, 5.0],
        speed: [0.6, 1.0],
        size: [0.4, 0.6],
        rotation: [0.0, TAU],
        spin: [-0.3, 0.3],
        spread: 0.25,
        drag: 0.3,
        wind: [0.25, 0.0, 0.0],
        turbulence: 0.3,
        turbulence_frequency: 0.8,
        size_over_life: vec![key(0.0, 0.5), key(1.0, 2.5)],
        color_over_life: vec![
            color(0.0, [0.22, 0.22, 0.23, 0.6]),
            color(1.0, [0.45, 0.45, 0.47, 0.0]),
        ],
        fade_in: 0.15,
        fade_out: 0.2,
        ..ParticleEmitter::default()
    }
}

fn fire() -> ParticleEmitter {
    ParticleEmitter {
        rate: 50.0,
        max_particles: 200,
        prewarm: true,
        shape: EmitterShape::Circle,
        shape_size: [0.2, 0.2, 0.2],
        lifetime: [0.6, 1.0],
        speed: [1.0, 1.6],
        size: [0.4, 0.6],
        rotation: [0.0, TAU],
        spin: [-1.0, 1.0],
        spread: 0.15,
        gravity: -1.5,
        drag: 0.5,
        turbulence: 0.3,
        turbulence_frequency: 2.0,
        size_over_life: vec![key(0.0, 0.7), key(0.25, 1.0), key(1.0, 0.25)],
        color_over_life: vec![
            color(0.0, [0.9, 0.4, 0.08, 0.35]),
            color(0.4, [0.8, 0.18, 0.03, 0.25]),
            color(1.0, [0.4, 0.05, 0.01, 0.0]),
        ],
        emissive: 1.0,
        fade_in: 0.1,
        fade_out: 0.3,
        blend: ParticleBlend::Additive,
        ..ParticleEmitter::default()
    }
}

fn embers() -> ParticleEmitter {
    ParticleEmitter {
        rate: 10.0,
        max_particles: 100,
        prewarm: true,
        shape: EmitterShape::Circle,
        shape_size: [0.3, 0.3, 0.3],
        lifetime: [1.5, 3.0],
        speed: [0.8, 1.6],
        size: [0.03, 0.06],
        spread: 0.4,
        gravity: -0.4,
        drag: 0.3,
        turbulence: 1.2,
        turbulence_frequency: 1.2,
        color_over_life: vec![
            color(0.0, [1.0, 0.5, 0.12, 1.0]),
            color(1.0, [0.9, 0.15, 0.03, 1.0]),
        ],
        emissive: 2.0,
        fade_in: 0.05,
        fade_out: 0.4,
        blend: ParticleBlend::Additive,
        sprite: ParticleSprite::Disc,
        ..ParticleEmitter::default()
    }
}

fn fireflies() -> ParticleEmitter {
    ParticleEmitter {
        rate: 4.0,
        max_particles: 60,
        prewarm: true,
        duration: 10.0,
        shape: EmitterShape::Box,
        shape_size: [3.0, 0.8, 3.0],
        lifetime: [4.0, 7.0],
        speed: [0.05, 0.2],
        size: [0.08, 0.12],
        spread: PI,
        turbulence: 0.6,
        turbulence_frequency: 0.4,
        // Each fly blinks twice in its life.
        color_over_life: vec![
            color(0.0, [0.55, 1.0, 0.15, 0.0]),
            color(0.15, [0.55, 1.0, 0.15, 1.0]),
            color(0.35, [0.55, 1.0, 0.15, 0.1]),
            color(0.6, [0.55, 1.0, 0.15, 1.0]),
            color(0.8, [0.55, 1.0, 0.15, 0.1]),
            color(1.0, [0.55, 1.0, 0.15, 0.0]),
        ],
        emissive: 2.5,
        fade_in: 0.0,
        fade_out: 0.0,
        blend: ParticleBlend::Additive,
        ..ParticleEmitter::default()
    }
}

fn magic_sparkle() -> ParticleEmitter {
    ParticleEmitter {
        rate: 30.0,
        max_particles: 150,
        prewarm: true,
        shape: EmitterShape::Sphere,
        shape_size: [0.5, 0.5, 0.5],
        lifetime: [0.6, 1.2],
        speed: [0.1, 0.4],
        size: [0.1, 0.18],
        rotation: [0.0, TAU],
        spin: [-3.0, 3.0],
        spread: PI,
        gravity: -0.3,
        size_over_life: vec![key(0.0, 0.0), key(0.2, 1.0), key(1.0, 0.0)],
        start_colors: vec![[0.6, 0.3, 1.0, 1.0], [0.2, 0.7, 1.0, 1.0]],
        emissive: 2.0,
        fade_in: 0.0,
        fade_out: 0.2,
        blend: ParticleBlend::Additive,
        ..ParticleEmitter::default()
    }
}

fn confetti() -> ParticleEmitter {
    ParticleEmitter {
        rate: 0.0,
        bursts: vec![ParticleBurst {
            time: 0.0,
            count: 150,
        }],
        max_particles: 300,
        duration: 4.0,
        looping: false,
        lifetime: [2.5, 4.0],
        speed: [5.0, 8.0],
        size: [0.06, 0.1],
        rotation: [0.0, TAU],
        spin: [-8.0, 8.0],
        spread: 0.6,
        gravity: 4.0,
        drag: 1.2,
        turbulence: 1.0,
        turbulence_frequency: 1.0,
        start_colors: vec![
            [0.95, 0.25, 0.3, 1.0],
            [0.2, 0.6, 0.95, 1.0],
            [0.98, 0.8, 0.15, 1.0],
            [0.3, 0.85, 0.4, 1.0],
            [0.8, 0.35, 0.9, 1.0],
        ],
        fade_in: 0.0,
        fade_out: 0.15,
        sprite: ParticleSprite::Square,
        ..ParticleEmitter::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_have_unique_names_and_round_trip_as_json() {
        let mut names: Vec<_> =
            EFFECT_PRESETS.iter().map(|preset| preset.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), EFFECT_PRESETS.len());
        for preset in EFFECT_PRESETS {
            let emitter = preset.emitter();
            let json = serde_json::to_value(&emitter).unwrap();
            let back: ParticleEmitter = serde_json::from_value(json).unwrap();
            assert_eq!(back, emitter, "{}", preset.name);
            assert!(effect_preset(preset.name).is_some());
        }
    }
}
