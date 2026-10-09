//! Reflected descriptions of the engine's own components, resources, and
//! assets. Their hints and docs are what `rusting schema` prints.

use std::path::PathBuf;

use bevy_ecs::entity::Entity;

use super::TypeRegistry;
use crate::assets::{AlphaMode, MaterialAsset, MaterialModel};
use crate::runtime::{
    AmbientLight, AmbientOcclusion, Antialiasing, Articulation, AutoSimulation,
    AxisMotion, Bloom, BurstEmitter, CameraScreen, CameraShake, ColorGrading,
    Connection, Connections, Counter, CullingMode, DeterminismMode, Easing,
    EnvironmentMap, Flash, FluidBlock, Fog, HudAnchor, HudElement, InputAction,
    Joint, JointAxis, JointKind, JointMotor, JointSpring, PhysicsSettings,
    PhysicsSyncMode, Pickup, PlatformerController, PlayerController,
    PostVolume, QualityProfile, RandomSeed, ReflectionProbe, RenderBounds,
    RenderSettings, SceneBackground, SceneInstance, ShadowQuality, SkyLight,
    SoundCue, SpawnGrid, Squash, TileKind, TileMap, ToneMapper, ToneMapping,
    Tween, TweenProperty, TweenRepeat, WaterBody,
};
use crate::runtime::{
    Animation, AnimationClip, AnimationCompare, AnimationLayer,
    AnimationMarker, AnimationProperty, AnimationTrack, AnimationTransition,
    BlendPoint, HumanoidBone, Ik, IkKind, Interpolation, Keyframe, Morph,
    Ragdoll, RagdollBone, RootMotion, Skin,
};
use crate::runtime::{
    ColorKey, CurveKey, EmitterShape, ParticleBlend, ParticleBurst,
    ParticleEmitter, ParticleFacing, ParticleSpace, ParticleSprite,
};

crate::reflect! {
    struct AmbientLight {
        color: [f32; 3] { unit: "linear RGB", min: 0.0, max: 1.0, color: true },
        intensity: f32 { unit: "factor", min: 0.0 },
    }
}

crate::reflect! {
    struct SkyLight {
        sky_color: [f32; 3] {
            unit: "linear RGB", min: 0.0, max: 1.0, color: true,
            doc: "seen by up-facing surfaces",
        },
        ground_color: [f32; 3] {
            unit: "linear RGB", min: 0.0, max: 1.0, color: true,
            doc: "seen by down-facing surfaces",
        },
        intensity: f32 { unit: "factor", min: 0.0 },
    }
}

crate::reflect! {
    enum ToneMapper { Linear, Reinhard, Aces }
}

crate::reflect! {
    struct ToneMapping {
        mapper: ToneMapper,
        exposure: f32 { unit: "factor", min: 0.001 },
    }
}

crate::reflect! {
    enum RenderBounds {
        Sphere {
            center: [f32; 3] { unit: "m" },
            radius: f32 { unit: "m", min: 0.0 },
        },
        Aabb {
            min: [f32; 3] { unit: "m", doc: "<= max" },
            max: [f32; 3] { unit: "m", doc: ">= min" },
        },
    }
}

crate::reflect! {
    enum PhysicsSyncMode { None, Events, SelectedState, FullState }
}

crate::reflect! {
    struct AutoSimulation {
        #[skip] decision: Option<crate::runtime::AllocationDecision>,
    }
}

crate::reflect! {
    struct PlayerController {
        walk_speed: f32 { unit: "m/s", min: 0.0 },
        sprint_multiplier: f32 { unit: "factor", min: 1.0 },
        jump_speed: f32 { unit: "m/s", min: 0.0 },
        air_jumps: u32 { doc: "extra jumps in the air before landing; 1 is a double jump" },
        gravity: f32 { unit: "m/s²", min: 0.0 },
        look_sensitivity: f32 { unit: "rad/pixel", min: 0.0 },
        mouse_look: bool {
            doc: "false: clicks never capture the cursor; the mouse stays free",
        },
        collision_mask: u32 {
            unit: "bitmask", doc: "layers the body stops against",
        },
        yaw: f32 { unit: "rad", doc: "0 looks toward -Z" },
        pitch: f32 { unit: "rad", min: -1.55, max: 1.55 },
        pitch_limits: [f32; 2] {
            unit: "rad", doc: "lowest, highest pitch; within ±1.55",
        },
        yaw_limits: Option<[f32; 2]> {
            unit: "rad", doc: "lowest, highest yaw for a seated or turret view; null turns freely",
        },
        camera_distance: f32 {
            unit: "m", min: 0.0,
            doc: "0 is first person; above 0 the camera orbits behind",
        },
        camera_height: f32 {
            unit: "m", doc: "third-person orbit center above the body",
        },
        camera_offset: [f32; 3] {
            unit: "m",
            doc: "added to the camera position in the body's frame; [0.6, 0, 0] looks over the right shoulder",
        },
        max_slope: f32 {
            unit: "rad", min: 0.0, max: 1.57,
            doc: "steepest ground it walks up; steeper is a wall",
        },
        max_step_height: f32 {
            unit: "m", min: 0.0, doc: "highest ledge it steps onto",
        },
        push_bodies: bool { doc: "false: dynamic bodies block it but never move" },
        turn_speed: f32 {
            unit: "rad/s", min: 0.0,
            doc: "how fast non-camera children turn to the walk direction",
        },
        crouch_height: f32 {
            unit: "m", min: 0.0,
            doc: "body height while player.crouch is held; 0 turns crouching off",
        },
        crouch_multiplier: f32 {
            unit: "factor", min: 0.0, doc: "speed factor while crouched",
        },
        #[skip] crouched: bool,
        #[skip] crouch_drop: f32,
        #[skip] camera_drop: f32,
        #[skip] vertical_speed: f32,
        #[skip] grounded: bool,
        #[skip] jump_requested: bool,
        #[skip] air_jumps_used: u32,
        #[skip] floor: Option<(bevy_ecs::entity::Entity, [f32; 3])>,
        #[skip] wall: Option<bevy_ecs::entity::Entity>,
        #[skip] velocity: [f32; 3],
        #[skip] dash_velocity: [f32; 3],
        #[skip] dash_left: f32,
    }
}

crate::reflect! {
    struct JointSpring {
        target: f32 { unit: "m or rad" },
        stiffness: f32 { unit: "N/m or N·m/rad", min: 0.0 },
        damping: f32 { unit: "N·s/m or N·m·s/rad", min: 0.0 },
    }
}

crate::reflect! {
    struct JointMotor {
        speed: f32 { unit: "m/s or rad/s" },
        max_force: f32 { unit: "N or N·m", min: 0.0 },
    }
}

crate::reflect! {
    enum AxisMotion {
        Locked,
        Free,
        Limited {
            min: f32 { unit: "m or rad" },
            max: f32 { unit: "m or rad" },
        },
    }
}

crate::reflect! {
    struct JointAxis {
        motion: AxisMotion,
        spring: Option<JointSpring>,
        motor: Option<JointMotor>,
    }
}

crate::reflect! {
    enum JointKind {
        Fixed,
        Hinge {
            limit: Option<[f32; 2]> { unit: "rad", doc: "min, max" },
            spring: Option<JointSpring>,
            motor: Option<JointMotor>,
        },
        Slider {
            limit: Option<[f32; 2]> { unit: "m", doc: "min, max" },
            spring: Option<JointSpring>,
            motor: Option<JointMotor>,
        },
        BallSocket,
        ConeTwist {
            swing: f32 { unit: "rad", min: 0.0 },
            twist: [f32; 2] { unit: "rad", doc: "min, max about X" },
        },
        Distance {
            min: f32 { unit: "m", min: 0.0 },
            max: f32 { unit: "m", min: 0.0 },
        },
        Spring {
            rest_length: f32 { unit: "m", min: 0.0 },
            stiffness: f32 { unit: "N/m", min: 0.0 },
            damping: f32 { unit: "N·s/m", min: 0.0 },
        },
        Generic {
            linear: [JointAxis; 3] { doc: "X, Y, Z along the target frame" },
            angular: [JointAxis; 3] { doc: "twist about X, swing about Y, Z" },
        },
    }
}

crate::reflect! {
    struct Joint {
        target: Entity { doc: "body this one hangs from; null is the world" },
        kind: JointKind,
        anchor: [f32; 3] { unit: "m", doc: "in this body's local space" },
        frame: [f32; 3] {
            unit: "rad", doc: "joint axes in this body's local space; X is the hinge, slider, and twist axis",
        },
        target_anchor: [f32; 3] {
            unit: "m", doc: "in the target's local space, or world space",
        },
        target_frame: [f32; 3] { unit: "rad", doc: "in the target's local space" },
        collide_connected: bool { doc: "false keeps the two bodies from colliding" },
        break_force: f32 { unit: "N", min: 0.0, doc: "0 never breaks" },
        break_torque: f32 { unit: "N·m", min: 0.0, doc: "0 never breaks" },
    }
}

crate::reflect! {
    struct Articulation {}
}

crate::reflect! {
    enum Easing {
        Linear, QuadIn, QuadOut, QuadInOut, CubicOut, SineInOut, BackOut,
        BounceOut,
    }
}

crate::reflect! {
    enum TweenProperty { Position, Rotation, Scale }
}

crate::reflect! {
    enum TweenRepeat { Once, Loop, PingPong }
}

crate::reflect! {
    struct Tween {
        property: TweenProperty { doc: "rotation is radians" },
        from: [f32; 3] { unit: "property unit" },
        to: [f32; 3] { unit: "property unit" },
        duration: f32 { unit: "s", min: 0.0, doc: "0 jumps to `to`" },
        delay: f32 { unit: "s", min: 0.0, doc: "holds `from` first" },
        easing: Easing,
        repeat: TweenRepeat,
        #[skip] elapsed: f32,
    }
}

crate::reflect! {
    struct CameraShake {
        trauma: f32 { unit: "0 to 1", min: 0.0, doc: "shake strength is trauma squared" },
        decay: f32 { unit: "1/s", min: 0.0, doc: "trauma lost per second" },
        max_offset: [f32; 3] { unit: "m", doc: "along the camera's right, up and back axes" },
        max_roll: f32 { unit: "rad", min: 0.0 },
        frequency: f32 { unit: "Hz", min: 0.0 },
    }
}

crate::reflect! {
    struct SpawnGrid {
        count: [u32; 3] { unit: "cells", doc: "along X, Y and Z, original included" },
        spacing: [f32; 3] { unit: "m" },
    }
}

crate::reflect! {
    struct Flash {
        color: [f32; 3] { unit: "linear RGB" },
        duration: f32 { unit: "s", min: 0.0, doc: "fade back to no tint" },
        #[skip] remaining: f32,
    }
}

crate::reflect! {
    struct Squash {
        stiffness: f32 { unit: "1/s²", min: 0.0, doc: "higher wobbles faster" },
        damping: f32 { unit: "1/s", min: 0.0, doc: "higher settles sooner" },
        #[skip] amount: f32,
        #[skip] velocity: f32,
        #[skip] rest: Option<[f32; 3]>,
    }
}

crate::reflect! {
    struct SoundCue {
        clip: String { unit: "asset path", doc: "relative to assets/, or sfx:coin 7 for a built-in sound" },
        volume: f32 { unit: "linear gain", min: 0.0 },
        on_collision: bool { doc: "CPU collider contacts only" },
        caption: String { doc: "shown for 2 s each time it fires, like [glass breaks]; empty for none" },
        #[skip] triggered: bool,
        #[skip] touching: bool,
    }
}

crate::reflect! {
    struct BurstEmitter {
        count: u32 { unit: "particles", doc: "each is one CPU entity" },
        speed: f32 { unit: "m/s", min: 0.0 },
        lifetime: f32 { unit: "s", min: 0.0 },
        particle_scale: f32 {
            unit: "factor", min: 0.0, doc: "of the emitter's scale",
        },
        gravity: f32 { unit: "m/s²" },
        on_collision: bool { doc: "CPU collider contacts only" },
        rate: f32 {
            unit: "particles/s", min: 0.0,
            doc: "above 0 emits every fixed step, no trigger needed",
        },
        area: [f32; 3] { unit: "m", doc: "half extents of the start box" },
        stretch: f32 { unit: "factor", min: 0.0, doc: "particle height" },
        #[skip] pending: f32,
        #[skip] triggered: bool,
        #[skip] touching: bool,
    }
}

crate::reflect! {
    enum EmitterShape { Point, Box, Sphere, Cone, Circle }
}

crate::reflect! {
    enum ParticleSpace { World, Local }
}

crate::reflect! {
    enum ParticleFacing { Billboard, Velocity }
}

crate::reflect! {
    enum ParticleBlend { Alpha, Additive }
}

crate::reflect! {
    enum ParticleSprite { Soft, Disc, Square }
}

crate::reflect! {
    struct ParticleBurst {
        time: f32 { unit: "s", min: 0.0, doc: "into each cycle" },
        count: u32 { unit: "particles" },
    }
}

crate::reflect! {
    struct CurveKey {
        t: f32 { unit: "life", min: 0.0, max: 1.0 },
        value: f32 { unit: "factor", min: 0.0 },
    }
}

crate::reflect! {
    struct ColorKey {
        t: f32 { unit: "life", min: 0.0, max: 1.0 },
        color: [f32; 4] {
            unit: "linear RGBA", min: 0.0, max: 1.0, color: true,
        },
    }
}

crate::reflect! {
    struct ParticleEmitter {
        autoplay: bool { doc: "off waits for play()" },
        rate: f32 { unit: "particles/s", min: 0.0 },
        bursts: Vec<ParticleBurst> { doc: "fired once per cycle" },
        max_particles: u32 { unit: "particles", doc: "alive at once" },
        prewarm: bool { doc: "start as if one cycle already played" },
        duration: f32 { unit: "s", min: 0.0, doc: "one cycle" },
        looping: bool { doc: "off stops emitting after one cycle" },
        on_collision: bool { doc: "restart on contact; CPU colliders only" },
        shape: EmitterShape,
        shape_size: [f32; 3] {
            unit: "m", min: 0.0,
            doc: "box half extents; x is the radius of round shapes",
        },
        lifetime: [f32; 2] { unit: "s", min: 0.0, doc: "min, max" },
        speed: [f32; 2] { unit: "m/s", doc: "min, max" },
        size: [f32; 2] { unit: "m", min: 0.0, doc: "min, max" },
        rotation: [f32; 2] { unit: "rad", doc: "min, max" },
        spin: [f32; 2] { unit: "rad/s", doc: "min, max" },
        direction: [f32; 3] { doc: "launch axis in the emitter's space" },
        spread: f32 {
            unit: "rad", min: 0.0, max: std::f64::consts::PI,
            doc: "half angle around direction; 3.14 is every way",
        },
        gravity: f32 { unit: "m/s²", doc: "downward" },
        drag: f32 { unit: "1/s", min: 0.0 },
        wind: [f32; 3] { unit: "m/s²", doc: "constant world push" },
        turbulence: f32 { unit: "m/s²", min: 0.0 },
        turbulence_frequency: f32 { unit: "1/m", min: 0.0 },
        size_over_life: Vec<CurveKey> { doc: "size factor keys by life" },
        color_over_life: Vec<ColorKey> { doc: "color keys by life" },
        start_colors: Vec<[f32; 4]> {
            doc: "each particle picks one; tints its color",
        },
        emissive: f32 { unit: "factor", min: 0.0, doc: "above 1 blooms" },
        fade_in: f32 { unit: "life", min: 0.0, max: 1.0 },
        fade_out: f32 { unit: "life", min: 0.0, max: 1.0 },
        space: ParticleSpace,
        facing: ParticleFacing,
        stretch: f32 {
            unit: "s", min: 0.0, doc: "Velocity facing: length per m/s",
        },
        blend: ParticleBlend,
        sprite: ParticleSprite,
        #[skip] command: crate::runtime::ParticleCommand,
    }
}

crate::reflect! {
    enum AnimationProperty {
        Position,
        Rotation,
        Scale,
        Color,
        Emissive,
        Visible,
        Orientation,
        Field {
            component: String { doc: "registered scene component name" },
            path: String { doc: "JSON pointer to a number, e.g. /intensity" },
        },
    }
}

crate::reflect! {
    enum Interpolation { Step, Linear, Smooth }
}

crate::reflect! {
    struct Keyframe {
        time: f32 { unit: "s", min: 0.0 },
        value: Vec<f32> {
            doc: "3 for transforms (rotation in rad), 4 for Color, 3 for Emissive, 1 otherwise",
        },
    }
}

crate::reflect! {
    struct AnimationTrack {
        target: String { doc: "empty for this entity, else child names: Arm/Hand" },
        property: AnimationProperty,
        interpolation: Interpolation,
        keys: Vec<Keyframe> { doc: "sorted by time" },
    }
}

crate::reflect! {
    struct AnimationMarker {
        time: f32 { unit: "s", min: 0.0 },
        name: String { doc: "event name game code receives" },
    }
}

crate::reflect! {
    struct AnimationClip {
        name: String,
        duration: f32 { unit: "s", min: 0.0, doc: "0 uses the last key" },
        repeat: TweenRepeat,
        tracks: Vec<AnimationTrack>,
        events: Vec<AnimationMarker>,
        blend: Vec<BlendPoint> {
            doc: "blend space: clips mixed by blend_parameter (and blend_parameter_y for 2D); empty for a plain clip",
        },
        blend_parameter: String { doc: "parameter that picks the blend position (x axis in 2D)" },
        blend_parameter_y: String { doc: "y axis parameter; set for a 2D blend space" },
    }
}

crate::reflect! {
    struct BlendPoint {
        clip: String,
        at: f32 { doc: "position on the blend parameter axis" },
        at_y: f32 { doc: "position on the y axis of a 2D blend space" },
    }
}

crate::reflect! {
    enum AnimationCompare { Above, Below, Equal }
}

crate::reflect! {
    struct AnimationTransition {
        from: String { doc: "clip name; empty matches any clip" },
        to: String,
        parameter: String { doc: "parameter to test; empty skips the test" },
        compare: AnimationCompare,
        value: f32,
        at_end: bool { doc: "also wait until `from` has played once" },
        fade: f32 { unit: "s", min: 0.0, doc: "crossfade length; 0 cuts" },
    }
}

crate::reflect! {
    struct HumanoidBone {
        bone: String { doc: "standard name such as Hips, Spine, Head, LeftUpperArm, RightFoot" },
        path: String { doc: "this rig's path to the bone" },
    }
}

crate::reflect! {
    enum RootMotion {
        Off,
        InPlace,
        Transform,
        Velocity,
    }
}

crate::reflect! {
    struct AnimationLayer {
        clip: String,
        weight: f32 { min: 0.0, max: 1.0, doc: "how much of the layer shows" },
        weight_parameter: String { doc: "parameter read as the weight; empty uses weight" },
        additive: bool { doc: "add the clip's motion from its first frame instead of replacing" },
        mask: Vec<String> { doc: "target paths (with children) the layer writes; empty writes all" },
    }
}

crate::reflect! {
    struct Animation {
        clips: Vec<AnimationClip>,
        autoplay: String { doc: "clip played on start; empty plays nothing" },
        speed: f32 { unit: "factor", min: 0.0 },
        parameters: std::collections::BTreeMap<String, f32> {
            doc: "named numbers game code sets for transitions",
        },
        transitions: Vec<AnimationTransition> {
            doc: "state machine edges; the first match each tick wins",
        },
        layers: Vec<AnimationLayer> {
            doc: "clips played on top of the state machine, in order",
        },
        root_motion: RootMotion {
            doc: "Off moves the root bone as keyed; InPlace collects for game code; Transform moves this object; Velocity drives its GPU body",
        },
        root_bone: String { doc: "path of the bone whose Position track carries root motion; empty is this object" },
        humanoid: Vec<HumanoidBone> {
            doc: "humanoid bone names (Hips, Spine, LeftUpperArm...) of this rig's bone paths, for retargeting; empty matches by bone name",
        },
        #[skip] command: Option<crate::runtime::AnimationCommand>,
    }
}

crate::reflect! {
    enum IkKind {
        LookAt,
        TwoBone,
        Foot,
        Chain,
    }
}

crate::reflect! {
    struct RagdollBone {
        path: String { doc: "bone path from the character" },
        length: f32 { unit: "m", min: 0.0, doc: "capsule along the bone's local +Y, ends included" },
        radius: f32 { unit: "m", min: 0.0 },
        mass: f32 { unit: "kg", min: 0.0 },
        joint: JointKind { doc: "joint to the nearest ancestor bone's body" },
        frame: [f32; 3] { unit: "rad", doc: "joint axes in the bone's frame; the default turns X along the bone" },
    }
}

crate::reflect! {
    struct Ragdoll {
        bones: Vec<RagdollBone>,
        hit_speed: f32 { unit: "m/s", min: 0.0, doc: "a body closing on the character this fast makes it go limp; 0 only on command" },
        recover_after: f32 { unit: "s", min: 0.0, doc: "seconds limp before getting up; 0 waits for game code" },
        blend_time: f32 { unit: "s", min: 0.0, doc: "seconds to blend back to the animation, or for active muscles to regain strength" },
        muscle: f32 { unit: "Hz", min: 0.0, doc: "above 0 keeps the bodies on and turns them toward the animation; 5 loose, 15 stiff" },
        #[skip] command: Option<bool>,
    }
}

crate::reflect! {
    struct Ik {
        kind: IkKind { doc: "LookAt turns this object; TwoBone bends its parent and grandparent" },
        target: Entity { doc: "object to reach or look at; null turns the solve off" },
        pole: Entity { doc: "TwoBone: the middle joint bends toward it; null keeps the bend plane" },
        weight: f32 { min: 0.0, max: 1.0, doc: "0 keeps the animated pose, 1 solves fully" },
        forward: [f32; 3] { doc: "LookAt: local axis that points at the target" },
        reach: f32 { unit: "m", min: 0.0, doc: "Foot: ground search above and below the character's floor" },
        joints: u32 { doc: "Chain: how many joints above this object bend" },
    }
}

crate::reflect! {
    struct Morph {
        weights: Vec<f32> { doc: "one weight per blend shape of the mesh, usually 0 to 1" },
    }
}

crate::reflect! {
    struct Skin {
        joints: Vec<String> { doc: "joint paths from this object; `..` is the parent" },
        inverse_bind: Vec<[[f32; 4]; 4]> { doc: "column-major, one per joint" },
    }
}

crate::reflect! {
    struct FluidBlock {
        spacing: f32 { unit: "m", min: 0.02, doc: "rest spacing of particles" },
        count_x: u32 { unit: "particles" },
        count_y: u32 { unit: "particles" },
        count_z: u32 { unit: "particles" },
        container_half_extents: [f32; 3] {
            unit: "m", min: 0.1, doc: "the box is centered on the entity",
        },
        iterations: u32 { unit: "passes", min: 1.0, max: 16.0 },
        viscosity: f32 { unit: "factor", min: 0.0, max: 1.0 },
        visible: bool { doc: "draw the fluid as a water surface" },
        show_particles: bool { doc: "also draw one sphere per particle" },
    }
}

crate::reflect! {
    struct WaterBody {
        size: [f32; 2] { unit: "m", min: 0.5, doc: "x and z size, centered on the entity" },
        resolution: u32 { unit: "cells", min: 2.0, max: 256.0, doc: "grid cells along the longer side" },
        wave_height: f32 { unit: "m", min: 0.0, doc: "crest height above the rest level" },
        wave_length: f32 { unit: "m", min: 0.1, doc: "distance between crests" },
        wave_speed: f32 { unit: "m/s", min: 0.0 },
        flow_direction: f32 { unit: "deg", doc: "direction of waves and current around +Y; 0 is +X" },
        flow_speed: f32 { unit: "m/s", min: 0.0, doc: "current that carries floating bodies; 0 is a lake" },
        color: [f32; 4] { doc: "RGBA of the surface" },
    }
}

crate::reflect! {
    enum HudAnchor {
        TopLeft, Top, TopRight, Center, BottomLeft, Bottom, BottomRight,
    }
}

crate::reflect! {
    struct HudElement {
        text: String { doc: "{name} shows the counter with that name" },
        anchor: HudAnchor,
        offset: [f32; 2] {
            unit: "logical pixels",
            doc: "[x, y] inward from the anchor: [24, 24] on BottomRight is 24 px left of the right edge and 24 px above the bottom; on Top, Bottom and Center, +x is right",
        },
        font_size: f32 { unit: "logical pixels", min: 1.0 },
        color: [f32; 4] { unit: "sRGBA", min: 0.0, max: 1.0, color: true },
        button: bool { doc: "a click sends HudButtonPressed" },
        requires: Option<String> {
            unit: "counter name",
            doc: "null, or shown only once that counter is complete",
        },
        camera: Option<String> {
            unit: "camera name",
            doc: "null anchors to the window; a name anchors to that camera's viewport and shows only while it is active",
        },
    }
}

crate::reflect! {
    struct InputAction {
        action: String { doc: "name game code and scenarios use" },
        inputs: Vec<String> {
            doc: "winit key names (KeyF, Space, ArrowUp), MouseLeft, MouseRight, MouseMiddle, or gamepad inputs (PadSouth, PadStart, PadDpadUp, PadLeftStickUp)",
        },
    }
}

crate::reflect! {
    struct Counter {
        name: String { doc: "the lowest-ID counter wins when names repeat" },
        value: i32,
        target: Option<i32> {
            doc: "null, or the value that completes the counter",
        },
    }
}

crate::reflect! {
    struct Pickup {
        counter: String { unit: "counter name" },
        value: i32 { doc: "added on collect" },
        requires: Option<String> {
            unit: "counter name",
            doc: "null, or collectable only once that counter is complete",
        },
        collected: bool { doc: "set by the engine; scenarios can check it" },
    }
}

crate::reflect! {
    struct Connection {
        handler: String {
            doc: "name the game registered with App::add_signal_handler",
        },
        target: Entity { doc: "object passed to the handler as the receiver" },
    }
}

crate::reflect! {
    struct Connections {
        list: Vec<Connection> {
            doc: "signal handlers run, in order, when this object emits",
        },
    }
}

crate::reflect! {
    struct SceneInstance {
        source: PathBuf {
            unit: "scene file",
            doc: "placed under this object on load, relative to this scene; \
                  empty places nothing",
        },
    }
}

crate::reflect! {
    struct SceneBackground {
        color: [f32; 4] {
            unit: "linear RGBA", min: 0.0, max: 1.0, color: true,
        },
    }
}

crate::reflect! {
    struct EnvironmentMap {
        texture: PathBuf {
            unit: "asset path",
            doc: "equirectangular (2:1) image that surfaces reflect",
        },
        intensity: f32 { unit: "factor", min: 0.0 },
        #[skip] handle: Option<crate::assets::Handle<crate::assets::TextureAsset>>,
    }
}

crate::reflect! {
    struct ReflectionProbe {
        extents: [f32; 3] {
            unit: "m", min: 0.0,
            doc: "half size of the box along each world axis",
        },
        intensity: f32 { unit: "factor", min: 0.0 },
    }
}

crate::reflect! {
    struct Fog {
        color: [f32; 3] {
            unit: "linear RGB", min: 0.0, max: 1.0, color: true,
            doc: "color that distant objects fade into",
        },
        density: f32 {
            unit: "1/m", min: 0.0, max: 1.0,
            doc: "extinction per metre at `height`; 0.01 hides things \
                  about 300 m away",
        },
        height: f32 {
            unit: "m",
            doc: "world height where the fog has its full density",
        },
        height_falloff: f32 {
            unit: "1/m", min: 0.0, max: 2.0,
            doc: "how fast the fog thins above `height`; 0 is uniform",
        },
        sun_scatter: f32 {
            unit: "factor", min: 0.0, max: 1.0,
            doc: "brightens fog toward the sun",
        },
        sky_affect: f32 {
            unit: "factor", min: 0.0, max: 1.0,
            doc: "how much the background fades into the fog",
        },
    }
}

crate::reflect! {
    struct CameraScreen {
        camera: String { doc: "name of the camera whose image this mesh shows" },
        size: [u32; 2] { unit: "px", doc: "feed width and height" },
        update_every: u32 {
            unit: "frames", min: 1.0, max: 60.0,
            doc: "draws the feed every this many frames",
        },
        enabled: bool { doc: "off keeps the last image without drawing" },
        grading: Option<ColorGrading> {
            doc: "the feed's own color grading; none uses the scene's",
        },
        exposure: f32 {
            unit: "factor", min: 0.0, max: 64.0,
            doc: "multiplies the scene's exposure for this feed; 1 matches the player's view",
        },
    }
}

crate::reflect! {
    struct ColorGrading {
        contrast: f32 {
            unit: "factor", min: 0.0, max: 3.0,
            doc: "contrast around mid grey; 1 leaves the image as is",
        },
        saturation: f32 {
            unit: "factor", min: 0.0, max: 3.0,
            doc: "0 is grey, 1 unchanged, above 1 stronger colors",
        },
        shadows: [f32; 3] {
            unit: "linear rgb",
            doc: "tint multiplied into dark tones; [1, 1, 1] is none",
        },
        highlights: [f32; 3] {
            unit: "linear rgb",
            doc: "tint multiplied into bright tones; [1, 1, 1] is none",
        },
        vignette: f32 {
            unit: "factor", min: 0.0, max: 1.0,
            doc: "how much the corners darken; 0 is none",
        },
        grain: f32 {
            unit: "factor", min: 0.0, max: 1.0,
            doc: "film grain, changing every fixed tick",
        },
        chromatic_aberration: f32 {
            unit: "factor", min: 0.0, max: 1.0,
            doc: "red and blue split toward the edges",
        },
        scanlines: f32 {
            unit: "factor", min: 0.0, max: 1.0,
            doc: "darkens every other pixel row",
        },
        color_bleed: f32 {
            unit: "factor", min: 0.0, max: 1.0,
            doc: "smears color sideways like VHS tape",
        },
        noise_band: f32 {
            unit: "factor", min: 0.0, max: 1.0,
            doc: "a band of static rolling down the image",
        },
        distortion: f32 {
            unit: "factor", min: 0.0, max: 1.0,
            doc: "tube bulge and row wobble over time",
        },
    }
}

crate::reflect! {
    struct PostVolume {
        extents: [f32; 3] {
            unit: "m", min: 0.0,
            doc: "half size of the box around the object, world axes",
        },
        blend: f32 {
            unit: "m", min: 0.0,
            doc: "fade-out distance outside the box; 0 switches at the wall",
        },
        priority: i32 {
            doc: "higher volumes blend over lower ones",
        },
    }
}

crate::reflect! {
    struct Bloom {
        intensity: f32 {
            unit: "factor", min: 0.0, max: 4.0,
            doc: "share of the light above `threshold` added back as glow",
        },
        threshold: f32 {
            unit: "linear", min: 0.0, max: 16.0,
            doc: "brightness where the glow starts, with a soft knee",
        },
        spread: f32 {
            unit: "factor", min: 0.0, max: 1.0,
            doc: "0 keeps the glow tight; 1 spreads it widely",
        },
    }
}

crate::reflect! {
    struct AmbientOcclusion {
        radius: f32 {
            unit: "m", min: 0.05, max: 10.0,
            doc: "world distance searched for occluders",
        },
        intensity: f32 {
            unit: "exponent", min: 0.0, max: 8.0,
            doc: "above 1 darkens creases more",
        },
    }
}

crate::reflect! {
    struct TileKind {
        color: [f32; 4] {
            unit: "linear RGBA", min: 0.0, max: 1.0, color: true,
            doc: "multiplies the texture",
        },
        texture: Option<PathBuf> {
            unit: "asset path",
            doc: "null or an image; alpha below 0.5 is cut out",
        },
        solid: bool { doc: "blocks CPU colliders" },
    }
}

crate::reflect! {
    struct TileMap {
        tile_size: f32 { unit: "m", min: 0.001 },
        rows: Vec<String> {
            doc: "one string per row, top first; characters without a kind are empty",
        },
        tiles: std::collections::BTreeMap<String, TileKind> {
            doc: "maps a one-character string to its kind",
        },
    }
}

crate::reflect! {
    struct PlatformerController {
        run_speed: f32 { unit: "m/s", min: 0.0 },
        jump_speed: f32 { unit: "m/s", min: 0.0 },
        gravity: f32 { unit: "m/s²", min: 0.0 },
        collision_mask: u32 {
            unit: "bitmask", doc: "layers the body stops against",
        },
        air_jumps: u32 { doc: "extra jumps in the air before landing; 1 is a double jump" },
        #[skip] air_jumps_used: u32,
        #[skip] vertical_speed: f32,
        #[skip] grounded: bool,
        #[skip] jump_buffer: f32,
        #[skip] air_time: f32,
        #[skip] floor: Option<(bevy_ecs::entity::Entity, [f32; 3])>,
        #[skip] dash_velocity: [f32; 3],
        #[skip] dash_left: f32,
    }
}

crate::reflect! {
    enum QualityProfile { Auto, Eco, Balanced, High }
}

crate::reflect! {
    enum CullingMode { Auto, Disabled, Frustum, FrustumAndOcclusion }
}

crate::reflect! {
    enum Antialiasing { Auto, Off, Msaa2, Msaa4 }
}

crate::reflect! {
    enum ShadowQuality { Auto, Low, Medium, High }
}

crate::reflect! {
    struct RenderSettings {
        quality: QualityProfile,
        vsync: bool,
        limit_fps: bool,
        max_fps: u32 { unit: "frames/s", min: 1.0 },
        render_scale: f32 {
            unit: "factor", min: 0.25, max: 2.0,
            doc: "Fraction of the window size the 3D scene renders at, stretched over the window. Below 1 is faster and blurrier; UI stays sharp.",
        },
        pixelated: bool {
            doc: "Stretch a scaled frame with nearest-neighbour filtering: square pixels instead of blur.",
        },
        background_color: [f32; 4] {
            unit: "linear RGBA", min: 0.0, max: 1.0, color: true,
        },
        culling: CullingMode,
        antialiasing: Antialiasing,
        shadows: ShadowQuality,
        reflections: bool,
    }
}

crate::reflect! {
    struct PhysicsSettings {
        gravity: [f32; 3] { unit: "m/s²" },
        enabled: bool,
    }
}

crate::reflect! {
    struct RandomSeed(u64)
}

crate::reflect! {
    enum DeterminismMode { Off, Local, CrossPlatform }
}

crate::reflect! {
    enum MaterialModel { Pbr, Unlit }
}

crate::reflect! {
    enum AlphaMode {
        Opaque,
        Mask { cutoff: f32 { min: 0.0, max: 1.0 } },
        Blend,
    }
}

crate::reflect! {
    struct MaterialAsset {
        name: String,
        model: MaterialModel,
        alpha_mode: AlphaMode,
        base_color: [f32; 4] {
            unit: "linear RGBA", min: 0.0, max: 1.0, color: true,
        },
        emissive: [f32; 3] { unit: "linear RGB", min: 0.0, color: true },
        metallic: f32 { min: 0.0, max: 1.0 },
        roughness: f32 { min: 0.0, max: 1.0 },
        transmission: f32 { min: 0.0, max: 1.0 },
        ior: f32 { min: 1.0, max: 3.0 },
        thickness: f32 { unit: "m", min: 0.0 },
        uv_scale: [f32; 2],
        uv_offset: [f32; 2],
        base_color_texture: Option<crate::assets::Handle<crate::assets::TextureAsset>>,
        normal_texture: Option<crate::assets::Handle<crate::assets::TextureAsset>>,
        metallic_roughness_texture: Option<crate::assets::Handle<crate::assets::TextureAsset>>,
        occlusion_texture: Option<crate::assets::Handle<crate::assets::TextureAsset>>,
        emissive_texture: Option<crate::assets::Handle<crate::assets::TextureAsset>>,
    }
}

pub(super) fn register(registry: &mut TypeRegistry) {
    registry.register_resource::<RenderSettings>("rusting.render_settings");
    registry.register_resource::<PhysicsSettings>("rusting.physics_settings");
    registry.register_resource::<RandomSeed>("rusting.random_seed");
    registry.register_resource::<DeterminismMode>("rusting.determinism");
    registry.register_asset::<MaterialAsset>("rusting.material");
}
