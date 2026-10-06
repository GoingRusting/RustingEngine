//! Game-feel kit: tweens, event-triggered sounds, burst effects, and a
//! scene-data HUD.
//!
//! Each piece is an ordinary registered scene component, so scene files,
//! `scene patch`, scenarios, and the editor inspector see and edit it:
//!
//! - [`Tween`] animates one `Transform` property between two values with an
//!   [`Easing`] curve, once, looping, or ping-ponging.
//! - [`SoundCue`] sends a [`SoundEvent`] when its body starts touching
//!   something or when game code calls [`SoundCue::trigger`]. The runner
//!   turns the event into an [`AudioQueue`](super::AudioQueue) request and
//!   plays it; see `GameScene::play_sound` for sounds from game code.
//! - [`BurstEmitter`] spawns short-lived [`BurstParticle`] entities that fly
//!   out, fall, and shrink. Particles copy the emitter's `MeshRenderer`.
//!   With a `rate` it emits every fixed step, across an `area` box.
//! - [`HudElement`] draws a text label or button over the game view (with the
//!   `ui` feature) and sends [`HudButtonPressed`] when a button is clicked.
//!   `{name}` in its text shows the value of the [`Counter`] called `name`.
//! - [`Counter`] is a named integer with an optional target, and a
//!   [`Pickup`] adds to one when a player body touches it, then hides itself
//!   and fires the cue and emitter on the same entity.
//!
//! Tweens, triggers, and particles run on the fixed step, so scenario tests
//! see the same values on every run. Burst directions come from
//! [`RandomSeed`] indexed by the fixed tick and the emitter's `SceneId`.

use std::collections::BTreeSet;
use std::f32::consts::{PI, TAU};

use bevy_ecs::change_detection::DetectChangesMut;
use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::{Commands, Component, Or, Query, Res, ResMut, With};
use serde::{Deserialize, Serialize};

use super::sim_math;
use super::{
    EventQueue, FrameTime, GlobalTransform, MeshRenderer, PhysicsWorld,
    RandomSeed, SceneId,
};
use crate::Transform;

/// Shape of a tween's progress over time. `apply` maps 0..1 to 0..1 (Back
/// overshoots past 1 on the way).
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
pub enum Easing {
    #[default]
    Linear,
    QuadIn,
    QuadOut,
    QuadInOut,
    CubicOut,
    SineInOut,
    BackOut,
    BounceOut,
}

impl Easing {
    #[must_use]
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::QuadIn => t * t,
            Self::QuadOut => 1.0 - (1.0 - t) * (1.0 - t),
            Self::QuadInOut => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    let u = -2.0 * t + 2.0;
                    1.0 - u * u / 2.0
                }
            }
            Self::CubicOut => 1.0 - (1.0 - t) * (1.0 - t) * (1.0 - t),
            Self::SineInOut => -(sim_math::sin_cos(PI * t).1 - 1.0) / 2.0,
            Self::BackOut => {
                let c1 = 1.70158;
                let u = t - 1.0;
                1.0 + (c1 + 1.0) * u * u * u + c1 * u * u
            }
            Self::BounceOut => {
                let (n, d) = (7.5625, 2.75);
                if t < 1.0 / d {
                    n * t * t
                } else if t < 2.0 / d {
                    let t = t - 1.5 / d;
                    n * t * t + 0.75
                } else if t < 2.5 / d {
                    let t = t - 2.25 / d;
                    n * t * t + 0.9375
                } else {
                    let t = t - 2.625 / d;
                    n * t * t + 0.984_375
                }
            }
        }
    }
}

/// The `Transform` field a [`Tween`] writes.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
pub enum TweenProperty {
    #[default]
    Position,
    Rotation,
    Scale,
}

/// What a [`Tween`] does after reaching `to`.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
pub enum TweenRepeat {
    /// Stops at `to`.
    #[default]
    Once,
    /// Jumps back to `from` and plays again.
    Loop,
    /// Plays back to `from`, then forward again.
    PingPong,
}

/// Animates one `Transform` property from `from` to `to`. Times are in
/// seconds of game time; rotation values are radians. One tween per entity.
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Tween {
    pub property: TweenProperty,
    pub from: [f32; 3],
    pub to: [f32; 3],
    pub duration: f32,
    /// Seconds to hold `from` before starting.
    pub delay: f32,
    pub easing: Easing,
    pub repeat: TweenRepeat,
    /// Time played so far, including the delay.
    #[serde(skip)]
    pub elapsed: f32,
}

impl Default for Tween {
    fn default() -> Self {
        Self {
            property: TweenProperty::Position,
            from: [0.0; 3],
            to: [0.0, 1.0, 0.0],
            duration: 1.0,
            delay: 0.0,
            easing: Easing::QuadInOut,
            repeat: TweenRepeat::PingPong,
            elapsed: 0.0,
        }
    }
}

impl Tween {
    /// Progress through the curve, 0..1, before easing.
    #[must_use]
    pub fn progress(&self) -> f32 {
        if self.duration <= 0.0 {
            return 1.0;
        }
        let played = (self.elapsed - self.delay).max(0.0) / self.duration;
        match self.repeat {
            TweenRepeat::Once => played.min(1.0),
            TweenRepeat::Loop => played.fract(),
            TweenRepeat::PingPong => {
                let phase = played % 2.0;
                if phase > 1.0 {
                    2.0 - phase
                } else {
                    phase
                }
            }
        }
    }

    /// True once a `Once` tween has reached `to`.
    #[must_use]
    pub fn finished(&self) -> bool {
        self.repeat == TweenRepeat::Once
            && self.elapsed >= self.delay + self.duration
    }

    /// The property value at the current time.
    #[must_use]
    pub fn sample(&self) -> [f32; 3] {
        let eased = self.easing.apply(self.progress());
        std::array::from_fn(|axis| {
            self.from[axis] + (self.to[axis] - self.from[axis]) * eased
        })
    }

    /// Starts again from `from`.
    pub fn restart(&mut self) {
        self.elapsed = 0.0;
    }
}

/// Sent by a [`SoundCue`] when it fires. Visible to readers on the next
/// frame, like other events.
#[derive(Clone, Debug, PartialEq)]
pub struct SoundEvent {
    pub entity: Entity,
    /// Asset path of the clip, relative to the project's `assets` folder.
    pub clip: String,
    pub volume: f32,
}

/// Asks for a sound. See the module docs.
#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundCue {
    pub clip: String,
    /// Linear gain, 1 is the clip's own level.
    pub volume: f32,
    /// Fire when this body's collider starts touching another one.
    pub on_collision: bool,
    /// Set by [`SoundCue::trigger`]; the next fixed step fires and clears it.
    #[serde(skip)]
    pub triggered: bool,
    #[serde(skip)]
    pub touching: bool,
}

impl Default for SoundCue {
    fn default() -> Self {
        Self {
            clip: String::new(),
            volume: 1.0,
            on_collision: true,
            triggered: false,
            touching: false,
        }
    }
}

impl SoundCue {
    pub fn trigger(&mut self) {
        self.triggered = true;
    }
}

/// Spawns a burst of particles. See the module docs.
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BurstEmitter {
    pub count: u32,
    /// Launch speed in metres per second.
    pub speed: f32,
    /// Seconds each particle lives.
    pub lifetime: f32,
    /// Particle size as a factor of the emitter's scale.
    pub particle_scale: f32,
    /// Downward acceleration in metres per second squared.
    pub gravity: f32,
    /// Fire when this body's collider starts touching another one.
    pub on_collision: bool,
    /// Particles per second emitted every fixed step while above 0, with
    /// no trigger: rain, smoke, sparks.
    pub rate: f32,
    /// Half extents in metres of a world-aligned box around the emitter
    /// where particles start; 0 starts them all at its center.
    pub area: [f32; 3],
    /// Height factor of each particle, so 8 makes a falling streak.
    pub stretch: f32,
    /// Carries the fraction of a particle `rate` has not emitted yet.
    #[serde(skip)]
    pub pending: f32,
    /// Set by [`BurstEmitter::trigger`]; the next fixed step fires and
    /// clears it.
    #[serde(skip)]
    pub triggered: bool,
    #[serde(skip)]
    pub touching: bool,
}

impl Default for BurstEmitter {
    fn default() -> Self {
        Self {
            count: 12,
            speed: 3.0,
            lifetime: 0.6,
            particle_scale: 0.15,
            gravity: 9.81,
            on_collision: true,
            rate: 0.0,
            area: [0.0; 3],
            stretch: 1.0,
            pending: 0.0,
            triggered: false,
            touching: false,
        }
    }
}

impl BurstEmitter {
    pub fn trigger(&mut self) {
        self.triggered = true;
    }
}

/// One particle from a [`BurstEmitter`]. Runtime only; never saved.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct BurstParticle {
    pub velocity: [f32; 3],
    pub gravity: f32,
    pub remaining: f32,
    pub lifetime: f32,
    pub scale: [f32; 3],
}

/// Screen corner, edge, or center a [`HudElement`] is placed against.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
pub enum HudAnchor {
    #[default]
    TopLeft,
    Top,
    TopRight,
    Center,
    BottomLeft,
    Bottom,
    BottomRight,
}

/// A text label or button drawn over the game view. `offset` is in logical
/// pixels and points from the anchor toward the screen center.
#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HudElement {
    pub text: String,
    pub anchor: HudAnchor,
    pub offset: [f32; 2],
    pub font_size: f32,
    /// sRGB text color with alpha.
    pub color: [f32; 4],
    /// Draw as a button that sends [`HudButtonPressed`] when clicked.
    pub button: bool,
    /// Name of a [`Counter`] that must reach its target before this element
    /// shows.
    pub requires: Option<String>,
    /// Name of a camera: the element anchors to that camera's viewport and
    /// shows only while the camera is active.
    pub camera: Option<String>,
}

impl Default for HudElement {
    fn default() -> Self {
        Self {
            text: "Text".into(),
            anchor: HudAnchor::TopLeft,
            offset: [16.0, 16.0],
            font_size: 18.0,
            color: [1.0; 4],
            button: false,
            requires: None,
            camera: None,
        }
    }
}

/// A named integer shown by HUD `{name}` placeholders and raised by
/// [`Pickup`]s.
#[derive(Component, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Counter {
    pub name: String,
    pub value: i32,
    /// The counter is complete once `value` reaches this.
    pub target: Option<i32>,
}

impl Default for Counter {
    fn default() -> Self {
        Self {
            name: "score".into(),
            value: 0,
            target: None,
        }
    }
}

impl Counter {
    #[must_use]
    pub fn complete(&self) -> bool {
        self.target.is_some_and(|target| self.value >= target)
    }
}

/// Collected once when a body with a `PlatformerController` or
/// `PlayerController` touches this entity's collider (make it a sensor).
/// Collecting adds `value` to the [`Counter`] named `counter`, hides the
/// entity, and triggers its [`SoundCue`] and [`BurstEmitter`].
#[derive(Component, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Pickup {
    pub counter: String,
    pub value: i32,
    /// Name of a [`Counter`] that must be complete before this can be
    /// collected.
    pub requires: Option<String>,
    pub collected: bool,
}

impl Default for Pickup {
    fn default() -> Self {
        Self {
            counter: "score".into(),
            value: 1,
            requires: None,
            collected: false,
        }
    }
}

/// The counter called `name` with the lowest `SceneId`, so duplicates
/// resolve the same way on every run.
pub(crate) fn find_counter<'a, C: std::ops::Deref<Target = Counter>>(
    counters: impl Iterator<Item = (C, Option<&'a SceneId>)>,
    name: &str,
) -> Option<C> {
    counters
        .filter(|(counter, _)| counter.name == name)
        .min_by_key(|(_, id)| id.map(|id| id.0))
        .map(|(counter, _)| counter)
}

/// Replaces every `{name}` whose name is a counter with its value.
#[must_use]
pub fn hud_text<'a>(
    text: &str,
    counters: impl Iterator<Item = (&'a Counter, Option<&'a SceneId>)> + Clone,
) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let counter = after.find('}').and_then(|close| {
            find_counter(counters.clone(), &after[..close])
                .map(|counter| (counter.value, close))
        });
        match counter {
            Some((value, close)) => {
                out.push_str(&value.to_string());
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Scene-wide clear color behind everything drawn, as linear RGBA. The
/// first one found sets [`RenderSettings::background_color`](super::RenderSettings).
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SceneBackground {
    pub color: [f32; 4],
}

impl Default for SceneBackground {
    fn default() -> Self {
        Self {
            color: super::RenderSettings::default().background_color,
        }
    }
}

/// Per frame: copies the scene's background into the render settings.
pub(super) fn apply_scene_background(
    settings: Option<ResMut<super::RenderSettings>>,
    backgrounds: Query<(&SceneBackground, Option<&SceneId>)>,
) {
    let first = backgrounds
        .iter()
        .min_by_key(|(_, id)| id.map(|id| id.0))
        .map(|(background, _)| background.color);
    if let (Some(mut settings), Some(color)) = (settings, first) {
        if settings.background_color != color {
            settings.background_color = color;
        }
    }
}

/// Equirectangular (2:1) sky image that surfaces reflect and that lights
/// them from every side, replacing the [`SkyLight`](super::SkyLight)
/// hemisphere. Rough surfaces sample blurrier mip levels. The one on the
/// entity with the lowest ID is used.
// ponytail: 8-bit images only, so reflections top out at `intensity`; add
// `.hdr` loading with the sky system milestone.
#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EnvironmentMap {
    /// Image path relative to the project's `assets` folder.
    pub texture: std::path::PathBuf,
    pub intensity: f32,
    /// The loaded `texture`, set by the engine.
    #[serde(skip)]
    pub handle: Option<crate::assets::Handle<crate::assets::TextureAsset>>,
}

impl Default for EnvironmentMap {
    fn default() -> Self {
        Self {
            texture: std::path::PathBuf::new(),
            intensity: 1.0,
            handle: None,
        }
    }
}

/// Box around the entity's position whose surfaces reflect the scene as
/// seen from that position, in place of the [`EnvironmentMap`]. Reflections
/// are projected onto the box walls, so a probe sized to its room lines up
/// with the walls. Overlapping probes blend; up to four are used.
// ponytail: captured when a probe is added or changes, not every frame, so
// moving objects do not show in probe reflections.
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReflectionProbe {
    /// Half size of the box along each world axis.
    pub extents: [f32; 3],
    pub intensity: f32,
}

impl Default for ReflectionProbe {
    fn default() -> Self {
        Self {
            extents: [5.0; 3],
            intensity: 1.0,
        }
    }
}

/// Exponential height fog: the scene fades into `color` with distance, and
/// the fog thins out above `height`. Looking toward the sun brightens it.
/// The one on the entity with the lowest ID is used.
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Fog {
    /// Linear RGB color that distant objects fade into.
    pub color: [f32; 3],
    /// Extinction per metre at `height`. 0.01 hides things about 300 m away.
    pub density: f32,
    /// World height, in metres, where the fog has its full `density`.
    pub height: f32,
    /// How fast the fog thins with height, per metre. 0 makes it uniform.
    pub height_falloff: f32,
    /// Brightens fog toward the shadow-casting (or first) directional light.
    pub sun_scatter: f32,
    /// How much the background behind everything fades into the fog.
    pub sky_affect: f32,
}

impl Default for Fog {
    fn default() -> Self {
        Self {
            color: [0.5, 0.6, 0.7],
            density: 0.01,
            height: 0.0,
            height_falloff: 0.1,
            sun_scatter: 0.3,
            sky_affect: 1.0,
        }
    }
}

/// Glow around bright pixels. Light above `threshold` spreads over the
/// screen before tone mapping. The one on the entity with the lowest ID is
/// used.
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Bloom {
    /// Share of the light above `threshold` added back as glow.
    pub intensity: f32,
    /// Linear brightness where the glow starts, with a soft knee below it.
    pub threshold: f32,
    /// 0 keeps the glow tight around bright pixels; 1 spreads it widely.
    pub spread: f32,
}

impl Default for Bloom {
    fn default() -> Self {
        Self {
            intensity: 0.5,
            threshold: 1.0,
            spread: 0.7,
        }
    }
}

/// Shows what a camera sees on this object's mesh, like a CCTV monitor. The
/// camera's image replaces the base color and emissive maps of the object's
/// material, so a material with black base color and emissive `[1, 1, 1]`
/// is an unlit screen. Screens that share a material show the same feed;
/// give each screen its own material (a different `name` is enough). The
/// camera can stay inactive.
#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CameraScreen {
    /// Name of the camera entity to show.
    pub camera: String,
    /// Feed resolution in pixels.
    pub size: [u32; 2],
    /// Draws the feed every this many frames; 2 halves its cost. Screens
    /// with the same value take turns, so they do not all draw together.
    pub update_every: u32,
    /// Off keeps the last image without drawing; swap the material to show
    /// a dark monitor. A feed is drawn once when it first appears either way.
    pub enabled: bool,
    /// The feed's own color grading, such as scanlines and grain for a CCTV
    /// look. Without one the feed uses the scene's.
    pub grading: Option<ColorGrading>,
}

impl Default for CameraScreen {
    fn default() -> Self {
        Self {
            camera: String::new(),
            size: [640, 360],
            update_every: 1,
            enabled: true,
            grading: None,
        }
    }
}

/// Color grading applied after tone mapping: contrast, saturation, a tint
/// for dark and for bright tones, a vignette, and film and CRT/VHS effects
/// (grain, chromatic aberration, scanlines, color bleed, a rolling noise
/// band, distortion). The defaults change nothing. The one on the entity with the lowest ID is used.
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorGrading {
    /// Contrast around mid grey. 1 leaves the image as is; 1.2 deepens
    /// shadows and brightens highlights.
    pub contrast: f32,
    /// 0 is grey, 1 leaves colors as they are, above 1 makes them stronger.
    pub saturation: f32,
    /// Linear RGB multiplied into dark tones, e.g. a cool `[0.9, 0.95, 1.1]`.
    pub shadows: [f32; 3],
    /// Linear RGB multiplied into bright tones, e.g. a warm `[1.1, 1.0, 0.9]`.
    pub highlights: [f32; 3],
    /// How much the corners darken. 0 is none, 1 makes them black.
    pub vignette: f32,
    /// Film grain strength, 0 to 1. The pattern changes every fixed tick
    /// and is the same for the same tick, so captures stay repeatable.
    pub grain: f32,
    /// Splits red and blue toward the edges like a cheap lens, 0 to 1.
    pub chromatic_aberration: f32,
    /// Darkens every other pixel row like a CRT, 0 to 1. Clearest on small
    /// camera-screen feeds.
    pub scanlines: f32,
    /// Smears color sideways while keeping the brightness sharp, like VHS
    /// tape, 0 to 1.
    pub color_bleed: f32,
    /// A band of static that rolls down the image, 0 to 1.
    pub noise_band: f32,
    /// Bulges the image like a curved tube and wobbles its rows over time,
    /// 0 to 1. Corners pushed off the image turn black.
    pub distortion: f32,
}

impl ColorGrading {
    /// The default, which changes nothing; usable in constants.
    pub const DEFAULT: Self = Self {
        contrast: 1.0,
        saturation: 1.0,
        shadows: [1.0; 3],
        highlights: [1.0; 3],
        vignette: 0.0,
        grain: 0.0,
        chromatic_aberration: 0.0,
        scanlines: 0.0,
        color_bleed: 0.0,
        noise_band: 0.0,
        distortion: 0.0,
    };
}

impl Default for ColorGrading {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Screen-space ambient occlusion: darkens ambient and sky light in creases
/// and corners where nearby geometry blocks it. Off on the Eco quality
/// profile. The one on the entity with the lowest ID is used.
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AmbientOcclusion {
    /// World-space distance, in metres, that is searched for occluders.
    pub radius: f32,
    /// Exponent on the occlusion. Above 1 darkens creases more.
    pub intensity: f32,
}

impl Default for AmbientOcclusion {
    fn default() -> Self {
        Self {
            radius: 1.0,
            intensity: 1.0,
        }
    }
}

/// Loads the image of each added or changed [`EnvironmentMap`]. A map whose
/// load failed (file missing or not written yet) is tried again about every
/// two seconds.
pub(super) fn load_environment_maps(
    assets: Option<ResMut<crate::assets::AssetServer>>,
    mut maps: Query<&mut EnvironmentMap>,
    mut frame: bevy_ecs::prelude::Local<u32>,
) {
    let Some(mut assets) = assets else {
        return;
    };
    *frame = frame.wrapping_add(1);
    for mut map in &mut maps {
        let retry = map.handle.is_none()
            && !map.texture.as_os_str().is_empty()
            && (*frame).is_multiple_of(120);
        if !bevy_ecs::change_detection::DetectChanges::is_changed(&map)
            && !retry
        {
            continue;
        }
        let map = map.bypass_change_detection();
        map.handle = (!map.texture.as_os_str().is_empty())
            .then(|| {
                assets
                    .textures
                    .handle_for_path(&map.texture)
                    .map(Ok)
                    .unwrap_or_else(|| assets.load_texture(&map.texture))
                    .map_err(|error| eprintln!("environment map: {error}"))
                    .ok()
            })
            .flatten();
    }
}

/// Sent the frame a HUD button is clicked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HudButtonPressed {
    pub entity: Entity,
}

/// Per fixed step: advances every tween and writes its property.
pub(super) fn advance_tweens(
    time: Res<FrameTime>,
    mut tweens: Query<(&mut Tween, &mut Transform)>,
) {
    let dt = time.fixed_delta.as_secs_f32();
    for (mut tween, mut transform) in &mut tweens {
        if tween.finished() {
            continue;
        }
        tween.elapsed += dt;
        let value = tween.sample();
        match tween.property {
            TweenProperty::Position => transform.position = value,
            TweenProperty::Rotation => transform.rotation = value,
            TweenProperty::Scale => transform.scale = value,
        }
    }
}

/// Per fixed step, after physics: marks cues and emitters whose body
/// started touching something.
pub(super) fn trigger_on_contact(
    physics: Res<PhysicsWorld>,
    mut cues: Query<(Entity, &mut SoundCue)>,
    mut emitters: Query<(Entity, &mut BurstEmitter)>,
) {
    if cues.is_empty() && emitters.is_empty() {
        return;
    }
    let touching: BTreeSet<Entity> = physics
        .contacts()
        .iter()
        .flat_map(|contact| [contact.a, contact.b])
        .collect();
    for (entity, mut cue) in &mut cues {
        let now = cue.on_collision && touching.contains(&entity);
        cue.triggered |= now && !cue.touching;
        cue.touching = now;
    }
    for (entity, mut emitter) in &mut emitters {
        let now = emitter.on_collision && touching.contains(&entity);
        emitter.triggered |= now && !emitter.touching;
        emitter.touching = now;
    }
}

/// Bodies that collect pickups.
type IsPlayer = Or<(
    With<super::PlatformerController>,
    With<super::PlayerController>,
)>;

type PickupParts = (
    Entity,
    &'static mut Pickup,
    Option<&'static SceneId>,
    Option<&'static mut SoundCue>,
    Option<&'static mut BurstEmitter>,
);

/// Per fixed step, after physics: collects pickups a player touches, in
/// `SceneId` order.
pub(super) fn collect_pickups(
    mut commands: Commands,
    physics: Res<PhysicsWorld>,
    players: Query<(), IsPlayer>,
    mut pickups: Query<PickupParts>,
    mut counters: Query<(&mut Counter, Option<&SceneId>)>,
) {
    let touched: BTreeSet<Entity> = physics
        .contacts()
        .iter()
        .flat_map(|contact| [(contact.a, contact.b), (contact.b, contact.a)])
        .filter(|(_, other)| players.contains(*other))
        .map(|(entity, _)| entity)
        .collect();
    if touched.is_empty() {
        return;
    }
    let mut ready: Vec<_> = pickups
        .iter_mut()
        .filter(|(entity, pickup, ..)| {
            !pickup.collected && touched.contains(entity)
        })
        .collect();
    ready.sort_by_key(|(entity, _, id, ..)| (id.map(|id| id.0), *entity));
    for (entity, mut pickup, _, cue, emitter) in ready {
        if let Some(required) = &pickup.requires {
            let done = find_counter(counters.iter(), required)
                .is_some_and(|counter| counter.complete());
            if !done {
                continue;
            }
        }
        pickup.collected = true;
        if let Some(mut counter) =
            find_counter(counters.iter_mut(), &pickup.counter)
        {
            counter.value += pickup.value;
        }
        commands
            .entity(entity)
            .insert(super::Visibility { visible: false });
        if let Some(mut cue) = cue {
            cue.trigger();
        }
        if let Some(mut emitter) = emitter {
            emitter.trigger();
        }
    }
}

/// Per fixed step: sends a [`SoundEvent`] for every triggered cue.
pub(super) fn fire_sound_cues(
    mut events: ResMut<EventQueue<SoundEvent>>,
    mut cues: Query<(Entity, &mut SoundCue)>,
) {
    for (entity, mut cue) in &mut cues {
        if std::mem::take(&mut cue.triggered) {
            events.send(SoundEvent {
                entity,
                clip: cue.clip.clone(),
                volume: cue.volume,
            });
        }
    }
}

type EmitterParts = (
    &'static mut BurstEmitter,
    &'static Transform,
    Option<&'static GlobalTransform>,
    Option<&'static SceneId>,
    Option<&'static MeshRenderer>,
    Entity,
);

/// Per fixed step: spawns particles for every triggered emitter and for
/// every emitter with a `rate`.
pub(super) fn fire_bursts(
    mut commands: Commands,
    time: Res<FrameTime>,
    seed: Res<RandomSeed>,
    mut emitters: Query<EmitterParts>,
) {
    let dt = time.fixed_delta.as_secs_f32();
    for (mut emitter, transform, global, id, mesh, entity) in &mut emitters {
        let mut count = 0;
        if std::mem::take(&mut emitter.triggered) {
            count += u64::from(emitter.count);
        }
        if emitter.rate > 0.0 {
            emitter.pending += emitter.rate * dt;
            let whole = emitter.pending.floor();
            emitter.pending -= whole;
            count += whole as u64;
        }
        if count == 0 {
            continue;
        }
        let origin = global.map_or(transform.position, |global| {
            let column = global.matrix[3];
            [column[0], column[1], column[2]]
        });
        // Authored emitters keep their stream across loads; others fall
        // back to the entity, which repeats for the same spawn order.
        let key = id.map_or(entity.to_bits(), |id| {
            let bits = id.0.as_u128();
            (bits as u64) ^ ((bits >> 64) as u64)
        });
        let stream = RandomSeed::stream("bursts", key);
        let area = RandomSeed::stream("burst_area", key);
        let mut scale =
            transform.scale.map(|axis| axis * emitter.particle_scale);
        scale[1] *= emitter.stretch;
        for index in 0..count {
            let up = seed.unit(time.fixed_tick, stream ^ (index << 1));
            let turn = seed.unit(time.fixed_tick, stream ^ (index << 1 | 1));
            let side = (1.0 - up * up).sqrt();
            let (sin, cos) = (turn * TAU).sin_cos();
            let velocity =
                [side * cos, up, side * sin].map(|axis| axis * emitter.speed);
            let particle = BurstParticle {
                velocity,
                gravity: emitter.gravity,
                remaining: emitter.lifetime,
                lifetime: emitter.lifetime,
                scale,
            };
            let position = std::array::from_fn(|axis| {
                let unit = seed
                    .unit(time.fixed_tick, area ^ (index * 3 + axis as u64));
                origin[axis] + (unit * 2.0 - 1.0) * emitter.area[axis]
            });
            let transform = Transform {
                position,
                scale,
                ..Transform::default()
            };
            let mut spawned = commands.spawn((transform, particle));
            if let Some(mesh) = mesh {
                spawned.insert(*mesh);
            }
        }
    }
}

/// Per fixed step: moves, shrinks, and removes burst particles.
pub(super) fn update_burst_particles(
    mut commands: Commands,
    time: Res<FrameTime>,
    mut particles: Query<(Entity, &mut BurstParticle, &mut Transform)>,
) {
    let dt = time.fixed_delta.as_secs_f32();
    for (entity, mut particle, mut transform) in &mut particles {
        particle.remaining -= dt;
        if particle.remaining <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        particle.velocity[1] -= particle.gravity * dt;
        for axis in 0..3 {
            transform.position[axis] += particle.velocity[axis] * dt;
        }
        let left = particle.remaining / particle.lifetime;
        transform.scale = particle.scale.map(|axis| axis * left);
    }
}

/// Caption lines of the sounds playing, bottom center over a dark band.
#[cfg(feature = "ui")]
fn draw_captions(
    context: &egui::Context,
    audio: Option<&super::AudioQueue>,
    settings: &super::CaptionSettings,
) {
    let lines = audio.map(super::AudioQueue::captions).unwrap_or_default();
    if !settings.enabled || lines.is_empty() {
        return;
    }
    let screen = context.screen_rect();
    egui::Area::new(egui::Id::new("rusting.captions"))
        .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -48.0))
        .fade_in(false)
        .order(egui::Order::Foreground)
        .show(context, |ui| {
            ui.set_max_width(screen.width() * 0.8);
            egui::Frame::new()
                .fill(egui::Color32::from_black_alpha(170))
                .inner_margin(egui::Margin::symmetric(12, 6))
                .corner_radius(4.0)
                .show(ui, |ui| {
                    for line in lines {
                        ui.label(
                            egui::RichText::new(line)
                                .size(settings.size)
                                .color(egui::Color32::WHITE),
                        );
                    }
                });
        });
}

/// Per frame: draws HUD elements in `SceneId` order and reports clicks.
#[cfg(feature = "ui")]
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_hud(
    ui: Res<super::RuntimeUi>,
    mut pressed: ResMut<EventQueue<HudButtonPressed>>,
    elements: Query<(Entity, &HudElement, Option<&SceneId>)>,
    counters: Query<(&Counter, Option<&SceneId>)>,
    visibility: Query<&super::Visibility>,
    parents: Query<&super::Parent>,
    cameras: Query<(&super::Name, &super::Camera)>,
    audio: Option<Res<super::AudioQueue>>,
    caption_settings: Option<Res<super::CaptionSettings>>,
) {
    draw_captions(
        ui.context(),
        audio.as_deref(),
        &caption_settings.as_deref().cloned().unwrap_or_default(),
    );
    // Hidden like a mesh: by itself or by any parent.
    let visible = |entity: Entity| {
        std::iter::successors(Some(entity), |&entity| {
            parents.get(entity).ok().map(|parent| parent.0)
        })
        .take(1024)
        .all(|entity| visibility.get(entity).map_or(true, |v| v.visible))
    };
    let mut elements: Vec<_> = elements.iter().collect();
    elements.sort_by_key(|(entity, _, id)| (id.map(|id| id.0), *entity));
    for (entity, element, _) in elements {
        if !visible(entity) {
            continue;
        }
        if let Some(required) = &element.requires {
            if !find_counter(counters.iter(), required)
                .is_some_and(|counter| counter.complete())
            {
                continue;
            }
        }
        let screen = ui.context().screen_rect();
        let area = match &element.camera {
            None => screen,
            Some(wanted) => {
                let Some((_, camera)) = cameras
                    .iter()
                    .find(|(name, camera)| name.0 == *wanted && camera.active)
                else {
                    continue;
                };
                match camera.viewport {
                    Some([x, y, w, h]) => egui::Rect::from_min_size(
                        screen.min + egui::vec2(x, y) * screen.size(),
                        egui::vec2(w, h) * screen.size(),
                    ),
                    None => screen,
                }
            }
        };
        let (align, inward) = match element.anchor {
            HudAnchor::TopLeft => (egui::Align2::LEFT_TOP, [1.0, 1.0]),
            HudAnchor::Top => (egui::Align2::CENTER_TOP, [1.0, 1.0]),
            HudAnchor::TopRight => (egui::Align2::RIGHT_TOP, [-1.0, 1.0]),
            HudAnchor::Center => (egui::Align2::CENTER_CENTER, [1.0, 1.0]),
            HudAnchor::BottomLeft => (egui::Align2::LEFT_BOTTOM, [1.0, -1.0]),
            HudAnchor::Bottom => (egui::Align2::CENTER_BOTTOM, [1.0, -1.0]),
            HudAnchor::BottomRight => {
                (egui::Align2::RIGHT_BOTTOM, [-1.0, -1.0])
            }
        };
        let offset = egui::vec2(
            element.offset[0] * inward[0],
            element.offset[1] * inward[1],
        );
        let [r, g, b, a] =
            element.color.map(|c| (c.clamp(0.0, 1.0) * 255.0) as u8);
        let text =
            egui::RichText::new(hud_text(&element.text, counters.iter()))
                .size(element.font_size)
                .color(egui::Color32::from_rgba_unmultiplied(r, g, b, a));
        // Measure this frame's text: an anchored egui area places itself by
        // last frame's size, so a value that grew ran past the edge.
        let context = ui.context();
        let galley = egui::WidgetText::from(text.clone()).into_galley_impl(
            context,
            &context.style(),
            egui::text::TextWrapping::no_max_width(),
            egui::FontSelection::Default,
            egui::Align::LEFT,
        );
        let padding = if element.button {
            2.0 * context.style().spacing.button_padding
        } else {
            egui::Vec2::ZERO
        };
        let point = align.pos_in_rect(&area) + offset;
        let position = align.anchor_size(point, galley.size() + padding).min;
        // No fade-in: an element shown for a few ticks would stay faint.
        egui::Area::new(egui::Id::new(("rusting.hud", entity)))
            .fixed_pos(position)
            .fade_in(false)
            .show(context, |ui| {
                if element.button {
                    if ui.button(text).clicked() {
                        pressed.send(HudButtonPressed { entity });
                    }
                } else {
                    // Lines break only where the text says: an anchored
                    // area would otherwise wrap text that grew this frame.
                    ui.add(egui::Label::new(text).extend());
                }
            });
    }
}
